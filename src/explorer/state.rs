//! The explorer's state: the rows of the tree, folders expanded and the selection, the root set, the state saved
//! for the next start, and the next changed file.

use super::tree::{Entry, Settings, holds, parent_of, tree_before};
use super::watch::AfterGit;
use super::{Act, Explorer, Run, later, rel_of};
use crate::core::actions;
use crate::core::exec::{self};
use crate::core::paths;
use crate::core::ui::{Ask, Confirm, Field, HelpState, Menu};
use crate::json::{self, Value};
use std::rc::Rc;
use std::time::Instant;

/// An item of the menu as menu_items makes it: its label, what it runs, the action whose key runs it too ("" for
/// none), red when it destroys.
pub struct It {
    pub(super) label: String,
    pub(super) run: Run,
    pub(super) action: Option<&'static str>,
    pub(super) danger: bool,
}

impl It {
    pub(super) fn new(label: &str, run: Run, action: &'static str) -> Option<It> {
        Some(It { label: label.into(), run, action: (!action.is_empty()).then_some(action), danger: false })
    }

    pub(super) fn danger(label: &str, run: Run) -> Option<It> {
        Some(It { danger: true, ..It::new(label, run, "")? })
    }
}

/// What a prompt does with the name typed.
type Named = Rc<dyn Fn(&mut Explorer, String) -> Act>;

/// A menu item's payload: what it runs, and the action whose key runs it too.
#[derive(Clone)]
pub struct Item {
    pub(super) action: Option<&'static str>,
    pub(super) run: Run,
}

/// A name typed: label, field, what it does (done), whether an empty answer is accepted, and where it shows.
pub struct Prompt {
    pub(super) label: String,
    pub(super) field: Field,
    pub(super) done: Named,
    pub(super) empty: bool,
    pub(super) inline: Option<Inline>,
}

/// A name typed in the tree: on the row renamed, or on a new row at the top of its folder.
#[derive(Clone)]
pub enum Inline {
    Rename { rel: String },
    New { dir: String, folder: bool },
}

/// The filter: the field, and whether it is being typed (else kept, Tab).
pub struct Filter {
    pub(super) field: Field,
    pub(super) active: bool,
}

/// A question with several answers, and what each answer's key runs.
pub struct Choice {
    pub(super) ask: Ask,
    pub(super) on: Vec<(String, Run)>,
}

pub struct State {
    /// The folders unfolded, in the order they were unfolded, each once.
    pub(super) expanded: Vec<String>,
    pub(super) rows: Vec<Entry>,
    pub(super) cursor: i64,
    pub(super) top: i64,
    pub(super) selected: Option<String>,
    pub(super) focused: bool,
    pub(super) menu: Option<Menu<Item>>,
    pub(super) help: Option<HelpState>,
    /// The settings view in place of the tree, and the page opened from it.
    pub(super) settings: Option<super::settings::View>,
    pub(super) flash: Option<String>,
    /// The last click, to detect double-clicks: its row and when.
    pub(super) last_click: Option<(i64, Instant)>,
    pub(super) filter: Option<Filter>,
    /// Rows limited to the changed files and their folders, all unfolded.
    pub(super) changed: bool,
    pub(super) prompt: Option<Prompt>,
    pub(super) confirm: Option<(Confirm, Run)>,
    pub(super) choice: Option<Choice>,
}

/// The settings of the tree, from the explorer's.
pub(super) fn tree_settings(s: &Value) -> Settings {
    let on = |k| s.get(k).is_some_and(Value::truthy);
    Settings { hidden: on("hidden"), ignored: on("ignored"), compact: on("compact") }
}

impl Explorer {
    pub(super) fn has(&self, rel: &str) -> bool {
        self.state.expanded.iter().any(|e| e == rel)
    }

    pub(super) fn expand(&mut self, rel: &str) {
        if !self.has(rel) {
            self.state.expanded.push(rel.to_string());
        }
    }

    pub(super) fn abs_of(&self, rel: &str) -> String {
        paths::join(&[&self.root, rel])
    }

    pub(super) fn current(&self) -> Option<&Entry> {
        usize::try_from(self.state.cursor).ok().and_then(|i| self.state.rows.get(i))
    }

    /// The rows of the tree as the view shows them, the selection kept on its entry (else on the first match, or at
    /// the same place).
    pub(super) fn build_rows(&mut self) {
        let query = self.state.filter.as_ref().map(|f| f.field.value.to_lowercase());
        let expanded = self.state.expanded.clone();
        self.state.rows = self.tree.rows(query.as_deref(), self.state.changed, &expanded);
        let rows = &self.state.rows;
        // An entry within a compact folder row: that row.
        let at = self.state.selected.as_deref().and_then(|s| rows.iter().position(|r| holds(r, s)));
        let i = match at {
            Some(i) => i as i64,
            None if query.as_deref().is_some_and(|q| !q.is_empty()) => {
                rows.iter().position(|r| r.hit.is_some()).map_or(-1, |i| i as i64)
            }
            None => self.state.cursor,
        };
        self.select(i);
    }

    pub(super) fn select(&mut self, i: i64) {
        self.state.cursor = i.min(self.state.rows.len() as i64 - 1).max(0);
        self.state.selected = self.current().map(|r| r.rel.clone());
    }

    pub(super) fn toggle(&mut self, row: Option<Entry>, open: Option<bool>) {
        let Some(row) = row.filter(|r| r.dir) else { return };
        if self.state.filter.is_some() || self.state.changed {
            return;
        }
        let open = open.unwrap_or(!row.open);
        // Opened, a compact folder row unfolds each folder of its chain, as they show when the setting is off;
        // closed, it folds them all.
        let first = row.from.clone().filter(|f| !f.is_empty()).unwrap_or_else(|| row.rel.clone());
        let stop = parent_of(&first).map(String::from);
        let mut a = Some(row.rel.clone());
        while let Some(at) = a.filter(|a| !a.is_empty() && Some(a) != stop.as_ref()) {
            if open {
                self.expand(&at);
            } else {
                self.state.expanded.retain(|e| *e != at);
            }
            a = parent_of(&at).map(String::from);
        }
        self.build_rows();
        self.save();
    }

    /// Expands the folders above an entry and selects it.
    pub(super) fn reveal_rel(&mut self, rel: &str) {
        let mut a = parent_of(rel).map(String::from);
        while let Some(at) = a.filter(|a| !a.is_empty()) {
            self.expand(&at);
            a = parent_of(&at).map(String::from);
        }
        self.state.selected = Some(rel.to_string());
        self.build_rows();
        self.reveal();
        self.save();
    }

    /// Expanded folders, per space folder, restored when the explorer is reopened.
    pub(super) fn load(&mut self) {
        let Some(file) = &self.save_file else { return };
        let Some(all) = exec::read_text(file).ok().and_then(|t| json::parse(&t).ok()) else { return };
        let list: Vec<String> = match all.get(&self.root) {
            Some(Value::Arr(items)) => items.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            // A string: its characters, as for…of reads it.
            Some(Value::Str(s)) => s.chars().map(String::from).collect(),
            _ => Vec::new(),
        };
        for rel in list {
            self.expand(&rel);
        }
    }

    /// Shared by every explorer: written to a temporary file then renamed, so that another one never reads it
    /// half-written; left alone when unreadable rather than overwritten with this space only.
    pub(super) fn save(&self) {
        let Some(file) = &self.save_file else { return };
        let mut all = match exec::read_text(file) {
            Ok(text) => match json::parse(&text) {
                // A value that is not an object (null, an array) leaves the file alone.
                Ok(v @ Value::Obj(_)) => v,
                _ => return,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Obj(Vec::new()),
            Err(_) => return,
        };
        let list = self.state.expanded.iter().filter(|e| !e.is_empty()).map(|e| Value::Str(e.clone())).collect();
        all.set(&self.root, Value::Arr(list));
        exec::write_atomic(file, json::stringify_pretty(&all, 1));
    }

    pub(super) fn write_pane_state(&self) {
        exec::write_pane_state(&self.pane, &crate::obj! { "home" => self.home.clone(), "root" => self.root.clone() });
    }

    /// Another folder as the root of the tree: from the menu (Set as root), or its parent (Backspace).
    pub(super) fn set_root(&mut self, dir: &str) {
        let dir = paths::resolve(&[dir]);
        if self.stopped || dir == self.root {
            return;
        }
        let from = self.root.clone();
        self.save();
        self.root = dir.clone();
        self.write_pane_state();
        self.announce();
        self.state.expanded = vec![String::new()];
        self.state.filter = None;
        self.state.top = 0;
        self.state.cursor = 0;
        self.state.selected = None;
        self.load();
        self.tree.reset(&dir);
        self.repos.clear();
        self.git_watcher = None;
        // Going up: the folder left is selected.
        let back = rel_of(&self.root, &from);
        if !back.is_empty() && !back.starts_with("..") && !paths::is_absolute(&back) {
            self.reveal_rel(&back);
        }
        self.watch_root();
        self.build_rows();
        self.render();
        self.init_git(AfterGit::Refresh);
    }

    pub(super) fn reveal(&mut self) {
        let list_h = self.body_height();
        if self.state.cursor < self.state.top {
            self.state.top = self.state.cursor;
        }
        if self.state.cursor >= self.state.top + list_h {
            self.state.top = self.state.cursor - list_h + 1;
        }
    }

    /// The changed files only, or the whole tree again with the selected entry shown.
    pub(super) fn show_changed(&mut self, on: bool) {
        if on && !self.tree.git.on() {
            return self.flash(Some("Not a git repository".into()));
        }
        let sel = self.state.selected.clone();
        self.state.changed = on;
        self.state.top = 0;
        match sel {
            Some(sel) if !on => self.reveal_rel(&sel),
            _ => {
                self.build_rows();
                self.reveal();
            }
        }
        self.render();
    }

    /// ] and [: the next or previous changed file after the selected entry, unfolded, selected and previewed; past
    /// the last one, back to the first.
    pub(super) fn next_change(&mut self, dir: i64) {
        if !self.tree.git.on() {
            return self.flash(Some("Not a git repository".into()));
        }
        let list = self.tree.changed_files();
        if list.is_empty() {
            return self.flash(Some("No changes".into()));
        }
        let i = match self.current() {
            None if dir > 0 => 0,
            None => list.len() - 1,
            Some(r) if dir > 0 => list.iter().position(|f| tree_before(&r.rel, r.dir, f, false)).unwrap_or(0),
            Some(r) => list.iter().rposition(|f| tree_before(f, false, &r.rel, r.dir)).unwrap_or(list.len() - 1),
        };
        let rel = list[i].clone();
        if !self.tree.git.gone.contains(&rel) {
            self.reveal_rel(&rel);
        } else {
            // A deleted file shows only among the changed files, every folder unfolded there.
            self.state.changed = true;
            self.state.selected = Some(rel.clone());
            self.build_rows();
            self.reveal();
        }
        self.render();
        let (abs, root, pane) = (self.abs_of(&rel), self.root.clone(), self.pane.clone());
        // Among the changed files, and for a deleted one, the preview shows the changes.
        if self.state.changed {
            let shown = actions::Shown { view: "diff".into(), ..Default::default() };
            self.run_action(move |_| later(move || actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)));
        } else if actions::can_preview(&abs) {
            let shown = actions::Shown::default();
            self.run_action(move |_| later(move || actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)));
        }
    }
}
