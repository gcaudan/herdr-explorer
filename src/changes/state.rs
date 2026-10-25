//! The changes loaded in the Changes pane: read on the git worker for the tab's scope, the files viewed, the notes of
//! the tab on their lines; and what the pane asks or offers (menus, questions, notes being typed).

use super::Pane;
use crate::core::scope::{self, Pick};
use crate::core::stage::{Mode, Sel};
use crate::core::ui::Field;
use crate::core::{exec, js, notes, paths, viewed};
use crate::diff::{self, Diff, File, Ref, changes_of, collect, disk_lines};
use crate::json::Value;
use std::sync::Arc;
use std::sync::mpsc::Sender;

/// What an item of the menus does: the view's options, then the scopes of b's menus.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Opt {
    Layout,
    Wrap,
    Compare,
    Scope(Pick),
}

/// A note while it is typed: its field, its kind, the lines of the row it was taken on.
pub(super) struct Draft {
    pub(super) field: Field,
    pub(super) kind: String,
    pub(super) r#ref: Ref,
}

/// What a question of the footer does on y: discard the rows selected then, or delete for good the untracked files
/// the trash did not take (the answer sent to the staging's thread, no on any other key).
pub(super) enum Asked {
    /// The rows picked from the diff then shown: they name its files, hunks and lines.
    Discard(Sel, Mode, Arc<Diff>),
    NoTrash(Sender<bool>),
}

pub(super) struct Confirm {
    pub(super) text: String,
    pub(super) asked: Asked,
}

/// A choice of the header's switches, in columns from 1.
pub(super) struct Chip {
    pub(super) id: &'static str,
    pub(super) from: i64,
    pub(super) to: i64,
}

/// A file of the diff by its path as git gives it: the new one, else the old one.
pub(super) fn name_of(f: &File) -> Option<&str> {
    f.to.as_deref().filter(|n| !n.is_empty()).or(f.from.as_deref()).filter(|n| !n.is_empty())
}

/// Where a note points: the file, its lines.
pub(super) fn note_where(r: &Ref) -> String {
    let lines = match (r.from, r.to) {
        (0, _) => String::new(),
        (from, to) if to > from => format!(":{from}-{to}"),
        (from, _) => format!(":{from}"),
    };
    format!("{}{lines}", r.file)
}

impl Pane {
    /// The changes for the scope chosen now, parsed, on the git worker; None when b chose another one meanwhile,
    /// whose run then shows its own.
    pub(super) fn changes(&mut self, then: impl FnOnce(&mut Pane, Option<Arc<Diff>>) + Send + 'static) {
        self.runs += 1;
        let run = self.runs;
        let (cwd, file, scope) = (self.cwd.clone(), self.file.clone(), self.scope.clone());
        let turn = match &self.tab {
            Some(tab) if scope.kind() == "turn" => Some(scope::read_turn(tab)),
            _ => None,
        };
        self.git.send(Box::new(move || {
            let res = std::panic::catch_unwind(|| collect(&cwd, &file, "head", Some(&scope), turn.as_ref()))
                .unwrap_or_else(|_| diff::Collected { top: cwd.clone(), ..diff::Collected::default() });
            let parsed = Arc::new(changes_of(&res));
            Box::new(move |p: &mut Pane| {
                if run != p.runs {
                    return then(p, None);
                }
                p.top = res.top;
                p.base_name = res.base_name;
                if let Some(e) = res.error {
                    p.say(e);
                }
                then(p, Some(parsed))
            })
        }));
    }

    pub(super) fn set_parsed(&mut self, d: Arc<Diff>) {
        self.parsed = Some(d);
        self.disk.clear();
        self.cache.clear();
    }

    /// The changes loaded again, for a new scope.
    pub(super) fn reload(&mut self) {
        self.parsed = None;
        self.cache.clear();
        self.draw();
        self.changes(|p, d| {
            let Some(d) = d else { return };
            p.set_parsed(d);
            if let Some(tab) = &p.tab {
                p.tab_notes = notes::current(tab, None);
            }
            p.read_viewed();
            p.draw();
            if p.scope.kind() == "branch" && p.base_name.is_empty() {
                p.say("No default branch: compared with HEAD".into());
            }
        });
    }

    pub(super) fn real_of(&mut self, f: &str) -> String {
        if let Some(r) = self.reals.get(f) {
            return r.clone();
        }
        let r = exec::real_path(&paths::join(&[&self.top, f]));
        self.reals.insert(f.to_string(), r.clone());
        r
    }

    /// A row with a note of the tab on its lines. A staged row is at its index line, the notes at disk lines
    /// (note_key): compared where it is on disk.
    pub(super) fn noted(&mut self, i: usize) -> bool {
        let Some(r) = self.rows[i].r#ref.clone() else { return false };
        if r.from == 0 || self.tab_notes.is_empty() {
            return false;
        }
        let at = if r.sec == "staged"
            && let Some(parsed) = &self.parsed
        {
            let key = (r.file.clone(), r.from, r.to);
            *self.disk.entry(key).or_insert_with(|| disk_lines(parsed, &r))
        } else {
            Some((r.from, r.to))
        };
        let Some((from, to)) = at else { return false };
        let real = Value::Str(self.real_of(&r.file));
        self.tab_notes.iter().any(|n| {
            n.get("from").is_some_and(Value::truthy)
                && !n.get("stale").is_some_and(Value::truthy)
                && js::strict_eq(n.get("file"), Some(&real))
                && from as f64 >= js::number(n.get("from"))
                && to as f64 <= js::number(n.get("to"))
        })
    }

    /// The files marked viewed and unchanged since, among those of the diff.
    pub(super) fn read_viewed(&mut self) {
        let map = self.tab.as_deref().map_or(Value::Obj(Vec::new()), viewed::read);
        let names: Vec<String> = match &self.parsed {
            Some(d) => d.files.iter().filter_map(name_of).map(String::from).collect(),
            None => Vec::new(),
        };
        let mut out = Vec::new();
        for n in names {
            if !out.contains(&n) && viewed::is(&map, &self.real_of(&n)) {
                out.push(n);
            }
        }
        self.viewed = out;
    }

    /// The rows selected: the cursor's, or from the anchor to it.
    pub(super) fn range(&self) -> (i64, i64) {
        match self.anchor {
            None => (self.cur, self.cur),
            Some(a) => (a.min(self.cur), a.max(self.cur)),
        }
    }

    pub(super) fn cur_ref(&self) -> Option<Ref> {
        usize::try_from(self.cur).ok().and_then(|i| self.rows.get(i)).and_then(|r| r.r#ref.clone())
    }
}
