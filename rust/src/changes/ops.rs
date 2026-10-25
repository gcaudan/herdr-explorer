//! What the Changes pane does: rows staged, unstaged or discarded, files marked viewed, a reference copied, the scope
//! compared with and the view's options.

use super::render::cyan;
use super::state::{Asked, Confirm, Opt, note_where};
use super::{Pane, Then};
use crate::core::scope::{self, Pick, Scope};
use crate::core::stage::{self, Mode, Sel};
use crate::core::ui::{Choice, Field, Menu, MenuItem, seg};
use crate::core::{actions, exec, paths, viewed, worker};
use crate::diff::{self, Diff, Ref, layout_of, save_mode};
use crate::json::Value;
use std::sync::Arc;
use std::sync::mpsc::channel;

impl Pane {
    /// m: the cursor's file viewed, or no longer.
    pub(super) fn mark_viewed(&mut self) {
        let Some(r) = self.cur_ref() else { return };
        let Some(tab) = self.tab.clone() else { return self.say("Needs herdr".into()) };
        let real = self.real_of(&r.file);
        let now = viewed::toggle(&tab, &real);
        self.read_viewed();
        self.say(if now { "Viewed: folded until it changes" } else { "No longer viewed" }.into());
    }

    /// y: the cursor's lines, relative to the pane's folder (src/app.js:12-14), to paste into a prompt.
    pub(super) fn copy_reference(&mut self) {
        let Some(r) = self.cur_ref() else { return };
        // From the folder resolved as top is (git gives its real path): /home → /var/home would otherwise give
        // ../../..
        let rel = paths::relative(&exec::real_path(&self.cwd), &paths::join(&[&self.top, &r.file]));
        let rel = if paths::SEP == '/' { rel } else { rel.replace(paths::SEP, "/") };
        let text = note_where(&Ref { file: rel, ..r.clone() });
        let done = if r.from != 0 { "Reference copied" } else { "Path copied" };
        self.act(move || Some(actions::copy(&text, done)));
    }

    /// s, u, D: the rows selected staged, unstaged or discarded (core::stage), with HEAD as the scope only (the
    /// others show changes the index has nothing to do with); D asks first. The cursor then stays where it was.
    pub(super) fn edit(&mut self, mode: Mode) {
        let Some(parsed) = self.parsed.clone() else { return };
        if self.rows.is_empty() {
            return;
        }
        if self.busy {
            return self.say("Busy…".into());
        }
        if self.scope.kind() != "head" {
            return self.say("Not with this scope: stage compared with HEAD (b)".into());
        }
        let (from, to) = self.range();
        // rows.slice(from, to + 1): nothing from a cursor on no row.
        let picked = match (usize::try_from(from), usize::try_from(to)) {
            (Ok(a), Ok(b)) => stage::picked(self.rows.get(a..=b.min(self.rows.len() - 1)).unwrap_or(&[])),
            _ => Sel::default(),
        };
        if mode != Mode::Discard {
            return self.go(picked, mode, &parsed);
        }
        let n = stage::count(&parsed, &picked, mode);
        if n.is_empty() {
            return self.say("Nothing unstaged selected".into());
        }
        self.confirm = Some(Confirm {
            text: format!("Discard {n} from the working tree?"),
            asked: Asked::Discard(picked, mode, parsed),
        });
        self.draw();
    }

    /// Does it on a thread of its own: untracked files the trash did not take are deleted for good once asked again.
    /// The rows were picked from the diff of: the JS's picked holds its objects, which name nothing in a diff
    /// read again since (nothing selected then).
    pub(super) fn go(&mut self, picked: Sel, mode: Mode, of: &Arc<Diff>) {
        self.confirm = None;
        if self.busy {
            return self.say("Busy…".into());
        }
        self.busy = true;
        let (top, parsed) = (self.top.clone(), self.parsed.clone());
        let picked = if parsed.as_ref().is_some_and(|p| Arc::ptr_eq(p, of)) { picked } else { Sel::default() };
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        let started = worker::spawn(
            self.tx.clone(),
            self.waker.clone(),
            Box::new(move || {
                let no_trash = |names: &[String]| {
                    let (answer, wait) = channel();
                    let name = if names.len() > 1 { format!("{} files", names.len()) } else { names[0].clone() };
                    let ask: Then = Box::new(move |p: &mut Pane| {
                        let text = format!("No trash: delete {name} for good?");
                        p.confirm = Some(Confirm { text, asked: Asked::NoTrash(answer) });
                        p.draw();
                    });
                    if tx.send(ask).is_err() {
                        return false;
                    }
                    waker.wake();
                    wait.recv().unwrap_or(false)
                };
                let text = parsed
                    .and_then(|parsed| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            stage::apply(&top, &parsed, &picked, mode, &actions::trash, Some(&no_trash))
                        }))
                        .ok()
                    })
                    .unwrap_or_else(|| format!("Cannot {}", mode.name()));
                Box::new(move |p: &mut Pane| {
                    p.say(text);
                    p.changes(|p, d| {
                        p.busy = false;
                        let Some(d) = d else { return };
                        p.set_parsed(d);
                        p.stay = Some(p.cur);
                        p.read_viewed();
                        p.draw();
                    });
                })
            }),
        );
        if !started {
            self.busy = false;
            self.say(format!("Cannot {}", mode.name()));
        }
    }

    /// A scope chosen here, for every pane of the tab; the current one chosen again reloads it (an agent turn being
    /// recorded, for instance).
    pub(super) fn pick(&mut self, s: Scope) {
        self.menu = None;
        self.prompt = None;
        if !scope::same(&s, &self.scope) {
            if let Some(tab) = &self.tab {
                scope::write(tab, &s);
            }
            self.scope = s;
        }
        self.reload();
    }

    /// The tab's scope, chosen in another pane.
    pub(super) fn follow_scope(&mut self) {
        let Some(tab) = &self.tab else { return };
        let s = scope::read(tab);
        if !scope::same(&s, &self.scope) {
            self.scope = s;
            self.reload();
        }
    }

    /// The view's options (⚙ or ,), as in the preview's Diff view: each changes in place, the menu staying open.
    pub(super) fn open_view(&mut self, sel: usize) {
        let w = self.size().0;
        let mut items: Vec<Option<MenuItem<Opt>>> =
            diff::view_items(layout_of(w), self.wrap, Opt::Layout, Opt::Wrap).into_iter().map(Some).collect();
        items.push(Some(MenuItem {
            label: "Compare with".into(),
            key: "b".into(),
            value: Some(seg(scope::label(&self.scope, &self.base_name), cyan())),
            danger: false,
            keep: false,
            run: Opt::Compare,
        }));
        let mut menu = Menu::new("View", items, 1, w as i64, 0);
        menu.sel = sel;
        self.menu = Some(menu);
        self.draw();
    }

    /// A choice of a menu, by key or click: run; the view's options shown again with the new values, unless closed.
    pub(super) fn menu_choice(&mut self, r: Option<Choice<Opt>>) {
        let sel = self.menu.as_ref().map_or(0, |m| m.sel);
        match r {
            None => {}
            Some(Choice::Close) => self.menu = None,
            Some(Choice::Run { run, keep }) => {
                self.menu = None;
                match run {
                    Opt::Layout => self.switch_layout(),
                    Opt::Wrap => self.toggle_wrap(),
                    Opt::Compare => self.open_scopes(),
                    Opt::Scope(Pick::Scope(s)) => self.pick(s),
                    Opt::Scope(Pick::Ask(kind)) => self.ask_scope(kind),
                }
                if keep {
                    self.open_view(sel);
                }
            }
        }
        self.draw();
    }

    pub(super) fn switch_layout(&mut self) {
        let w = self.size().0;
        save_mode(w, if layout_of(w) == "split" { "inline" } else { "split" });
        self.draw();
    }

    pub(super) fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.left = 0;
        let mut settings = exec::read_settings();
        settings.set("wrap", Value::Bool(self.wrap));
        exec::write_settings(&settings);
        self.draw();
    }

    pub(super) fn open_scopes(&mut self) {
        let w = self.size().0 as i64;
        let m = scope::menu(&self.scope, &self.base_name, w);
        let items = m
            .items
            .into_iter()
            .map(|it| {
                it.map(|it| MenuItem {
                    label: it.label,
                    key: it.key,
                    value: it.value,
                    danger: it.danger,
                    keep: it.keep,
                    run: Opt::Scope(it.run),
                })
            })
            .collect();
        self.menu = Some(Menu::new(&m.title, items, m.at, m.right, m.left));
        self.draw();
    }

    /// Commit or range… and Branch…: the menu hidden, then the rev typed before prompted, or the branches' menu.
    pub(super) fn ask_scope(&mut self, kind: &str) {
        self.menu = None;
        self.draw();
        if kind == "rev" {
            self.prompt = Some(Field::new(scope::prompt_rev(&self.scope)));
            return self.draw();
        }
        let (cwd, now) = (self.cwd.clone(), self.scope.clone());
        self.git.send(Box::new(move || {
            let list = scope::branches(&cwd);
            Box::new(move |p: &mut Pane| {
                let w = p.size().0 as i64;
                match scope::branch_menu(&now, &list, w) {
                    None => p.say("No branch".into()),
                    Some(m) => {
                        let items = m
                            .items
                            .into_iter()
                            .map(|it| {
                                it.map(|it| MenuItem {
                                    label: it.label,
                                    key: it.key,
                                    value: it.value,
                                    danger: it.danger,
                                    keep: it.keep,
                                    run: Opt::Scope(it.run),
                                })
                            })
                            .collect();
                        p.menu = Some(Menu::new(&m.title, items, m.at, m.right, m.left));
                        p.draw();
                    }
                }
            })
        }));
    }
}
