//! What the explorer does to files: open them in the preview or the editor, create, rename, delete or discard
//! them, and the action menu.

use super::state::{Inline, It, Item, Prompt};
use super::tree::{Entry, parent_of};
use super::{Act, Explorer, job, later, now, rel_of, removal};
use crate::core::actions;
use crate::core::exec::{self};
use crate::core::paths;
use crate::core::ui::{self, Confirm, Field, Menu};
use crate::json::Value;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

impl Explorer {
    /// The changes of an entry: a file's in the preview's Diff view, a folder's in the Changes pane (the whole tab).
    /// The folder git runs in for the entry rel: the root in its repository, else the repository of the root's folder
    /// holding it; None outside any.
    pub(super) fn repo_dir(&self, rel: &str) -> Option<String> {
        let name = self.tree.git.repo_of(rel)?;
        Some(if name.is_empty() { self.root.clone() } else { self.abs_of(name) })
    }

    pub(super) fn view_changes(&self, r: &Entry) -> Act {
        let Some(root) = self.repo_dir(&r.rel) else { return Act::Now(Some("Not a git repository".into())) };
        let (abs, pane, dir) = (self.abs_of(&r.rel), self.pane.clone(), r.dir);
        later(move || {
            if dir {
                actions::diff(&abs, &root, &pane)
            } else {
                let shown = actions::Shown { view: "diff".into(), ..Default::default() };
                actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)
            }
        })
    }

    /// Enter and double-click: documents and media other than images with their application, the rest in the
    /// preview (or the editor, depending on the settings).
    pub(super) fn open_file(&mut self, r: &Entry) {
        if r.dir {
            return;
        }
        let abs = self.abs_of(&r.rel);
        // A deleted file: what it held, in its diff.
        if r.gone {
            let act = self.view_changes(r);
            return self.run_action(move |_| act);
        }
        let (root, pane) = (self.root.clone(), self.pane.clone());
        if self.settings.get("open").and_then(Value::as_str) == Some("preview") && actions::can_preview(&abs) {
            return self.run_action(move |_| {
                later(move || {
                    let shown = actions::Shown { keep: true, ..Default::default() };
                    actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)
                })
            });
        }
        self.run_action(move |_| later(move || actions::open(&abs, &pane)));
    }

    /// empty: an empty answer is accepted (otherwise it cancels), and the cursor stays at the end. inline: typed in
    /// the tree, on the row renamed or on a new row in its folder, rather than at the bottom.
    pub(super) fn ask(
        &mut self,
        label: &str,
        value: &str,
        done: impl Fn(&mut Explorer, String) -> Act + 'static,
        empty: bool,
        inline: Option<Inline>,
    ) {
        self.state.menu = None;
        let mut field = Field::new(value);
        // A new name: the cursor before the extension.
        if let Some(dot) = value.rfind('.').filter(|&d| d > 0 && !empty) {
            field.cursor = value[..dot].chars().count();
        }
        self.state.prompt = Some(Prompt { label: label.into(), field, done: Rc::new(done), empty, inline });
        self.render();
    }

    /// A yes or no question (ui.confirm), and what yes does.
    fn confirm(&mut self, q: Confirm, yes: impl Fn(&mut Explorer) -> Act + 'static) {
        self.state.menu = None;
        self.state.confirm = Some((Confirm { focus: "no".into(), ..q }, Rc::new(yes)));
        self.render();
    }

    /// Where an entry is, for a question: its folder from the tree's root.
    fn where_of(&self, rel: &str) -> String {
        let parent = parent_of(rel).filter(|p| !p.is_empty()).map_or(String::new(), |p| format!("{p}/"));
        format!("in {}/{parent}", paths::basename(&self.root, None))
    }

    /// An entry deleted: its folder read again, as the root's watch may not see it ($HOME and above, a failed
    /// recursive watch).
    pub(super) fn removed(&mut self) {
        self.tree.cache.clear();
        self.build_rows();
    }

    /// After a change on disk: the new entry selected, its folder expanded.
    fn created(&mut self, abs: &str) {
        self.tree.cache.clear();
        let rel = rel_of(&self.root, abs);
        self.reveal_rel(&rel);
        self.render();
    }

    /// The new name typed at the top of its folder, opened for it.
    pub(super) fn new_entry(&mut self, dir: &str, folder: bool) {
        if !dir.is_empty() && !self.has(dir) {
            self.expand(dir);
            self.build_rows();
        }
        let d = dir.to_string();
        let done = move |ex: &mut Explorer, name: String| {
            match actions::create(&ex.abs_of(&d), &name, folder) {
                (_, Some(err)) => ex.flash(Some(err)),
                (abs, None) => ex.created(&abs.unwrap_or_default()),
            }
            Act::Now(None)
        };
        let label = if folder { "New folder" } else { "New file" };
        self.ask(label, "", done, false, Some(Inline::New { dir: dir.into(), folder }));
    }

    pub(super) fn rename_entry(&mut self, r: &Entry) {
        let r2 = r.clone();
        let done = move |ex: &mut Explorer, name: String| {
            let r = &r2;
            let abs = match actions::rename(&ex.abs_of(&r.rel), &name) {
                (_, Some(err)) => {
                    ex.flash(Some(err));
                    return Act::Now(None);
                }
                (abs, None) => abs.unwrap_or_default(),
            };
            // The expanded subfolders follow the folder.
            if r.dir {
                let to = rel_of(&ex.root, &abs);
                let under = format!("{}/", r.rel);
                let moved = ex.state.expanded.iter().map(|e| {
                    if *e == r.rel || e.starts_with(&under) { format!("{to}{}", &e[r.rel.len()..]) } else { e.clone() }
                });
                let mut expanded: Vec<String> = Vec::new();
                for e in moved.collect::<Vec<_>>() {
                    if !expanded.contains(&e) {
                        expanded.push(e);
                    }
                }
                ex.state.expanded = expanded;
            }
            ex.created(&abs);
            Act::Now(None)
        };
        self.ask("Rename", &r.name, done, false, Some(Inline::Rename { rel: r.rel.clone() }));
    }

    pub(super) fn delete_entry(&mut self, r: &Entry) {
        let abs = self.abs_of(&r.rel);
        let name = format!("{}{}", r.name, if r.dir { "/" } else { "" });
        let where_ = Some(self.where_of(&r.rel));
        let short = r.name.clone();
        // Never to the trash, which would follow it (see actions::trash).
        if exec::remote_link(&abs, &self.root) {
            let q = Confirm {
                title: "Link to a network share: delete the link?".into(),
                name,
                where_,
                note: Some("Only the link goes; the share is untouched.".into()),
                action: "Delete".into(),
                ..Default::default()
            };
            return self.confirm(q, removal(abs, short));
        }
        let q = Confirm {
            title: "Move to the trash?".into(),
            name: name.clone(),
            where_: where_.clone(),
            action: "Move".into(),
            ..Default::default()
        };
        self.confirm(q, move |ex| {
            ex.flash(Some("Moving to the trash…".into()));
            let (abs, short, name, where_) = (abs.clone(), short.clone(), name.clone(), where_.clone());
            Act::Later(Box::new(move || {
                let trashed = actions::trash(&abs);
                Box::new(move |ex: &mut Explorer| {
                    if trashed {
                        ex.removed();
                        return ex.flash(Some(format!("{short} moved to the trash")));
                    }
                    let q = Confirm {
                        title: "No trash: delete for good?".into(),
                        name,
                        where_,
                        note: Some("This cannot be undone.".into()),
                        action: "Delete".into(),
                        ..Default::default()
                    };
                    ex.confirm(q, removal(abs, short));
                    ex.render();
                })
            }))
        });
    }

    /// Discard changes…: the question, then the entry back to its last commit.
    fn discard_item(&self, abs: &str, repo: &str, title: &str, name: String, note: &str, action: &str) -> Option<It> {
        let (abs, root, title, note, action) =
            (abs.to_string(), repo.to_string(), title.to_string(), note.to_string(), action.to_string());
        let where_ = Some(self.where_of(&rel_of(&self.root, &abs)));
        let run = now(move |ex| {
            let q = Confirm {
                title: title.clone(),
                name: name.clone(),
                where_: where_.clone(),
                note: Some(note.clone()),
                action: action.clone(),
                ..Default::default()
            };
            let (abs, root) = (abs.clone(), root.clone());
            ex.confirm(q, move |_| {
                let (abs, root) = (abs.clone(), root.clone());
                later(move || Some(actions::discard(&abs, &root)))
            });
        });
        It::danger("Discard changes…", run)
    }

    /// The menu's items, None for a separator.
    fn menu_items(&self, r: Option<&Entry>) -> Vec<Option<It>> {
        let mut items = Vec::new();
        let (root, pane) = (self.root.clone(), self.pane.clone());
        match r {
            // A deleted entry: its changes and history, or back with Discard.
            Some(r) if r.gone => {
                let abs = self.abs_of(&r.rel);
                let repo = self.repo_dir(&r.rel).unwrap_or_else(|| root.clone());
                let e = r.clone();
                items.push(It::new(
                    "View changes",
                    Rc::new(move |ex: &mut Explorer| ex.view_changes(&e)),
                    "viewChanges",
                ));
                if !r.dir {
                    let (a, rt, p) = (abs.clone(), repo.clone(), pane.clone());
                    items.push(It::new("File history", job(move || actions::scm(&rt, &p, &a, true)), "fileHistory"));
                }
                let (a, rt) = (abs.clone(), repo.clone());
                let stage = job(move || Some(actions::stage(&a, &rt)));
                let (a, rt) = (abs.clone(), repo.clone());
                let unstage = job(move || Some(actions::unstage(&a, &rt)));
                items.extend([None, It::new("Stage", stage, ""), It::new("Unstage", unstage, "")]);
                let name = format!("{}{}", r.name, if r.dir { "/" } else { "" });
                let note = "Its changes are discarded.";
                items.push(self.discard_item(&abs, &repo, "Restore it?", name, note, "Restore"));
                let rel = r.rel.clone();
                let copy = job(move || Some(actions::copy(&rel, "Path copied")));
                items.extend([None, It::new("Copy relative path", copy, "copyRelativePath")]);
            }
            Some(r) => {
                let abs = self.abs_of(&r.rel);
                let deco = self.tree.decoration(&r.rel, r.dir);
                let dir_rel = if r.dir { r.rel.clone() } else { parent_of(&r.rel).unwrap_or("").to_string() };
                let on = |f: fn(&str, &str) -> Option<String>| {
                    let (a, p) = (abs.clone(), pane.clone());
                    job(move || f(&a, &p))
                };
                if !r.dir {
                    if actions::can_preview(&abs) {
                        let (a, rt, p) = (abs.clone(), root.clone(), pane.clone());
                        let shown = actions::Shown { keep: true, ..Default::default() };
                        let run = job(move || actions::preview(&a, &rt, &p, 0.0, false, true, &shown));
                        items.push(It::new("Preview", run, "preview"));
                    }
                    items.push(It::new("Open in editor", on(actions::editor), "openInEditor"));
                    if actions::can_open(&abs) {
                        let a = abs.clone();
                        items.push(It::new("Open in app", job(move || Some(actions::system(&a))), "openInApp"));
                    }
                    items.push(It::new("Edit in terminal", on(|a, p| actions::edit(a, p, None)), "edit"));
                } else {
                    items.push(It::new("Open in editor", on(actions::editor), "openInEditor"));
                    items.push(It::new("Open in file manager", on(actions::app), "openInApp"));
                }
                items.extend([It::new("Insert into terminal", on(actions::insert), ""), None]);
                let d = dir_rel.clone();
                items.push(It::new("New file…", now(move |ex| ex.new_entry(&d, false)), "newFile"));
                let d = dir_rel;
                items.push(It::new("New folder…", now(move |ex| ex.new_entry(&d, true)), "newFolder"));
                if r.dir {
                    let search = on(|a, p| actions::search(a, p, "content", ""));
                    items.push(It::new("Search in folder…", search, "searchInFiles"));
                    let a = abs.clone();
                    items.push(It::new("Set as root", now(move |ex| ex.set_root(&a)), ""));
                }
                let e = r.clone();
                items.push(It::new("Rename…", now(move |ex| ex.rename_entry(&e)), "rename"));
                let a = abs.clone();
                // A big folder takes a while to copy: on the worker, the pane kept running.
                let duplicate = Rc::new(move |_: &mut Explorer| {
                    let a = a.clone();
                    Act::Later(Box::new(move || {
                        let (to, err) = actions::duplicate(&a);
                        Box::new(move |ex: &mut Explorer| match err {
                            Some(err) => ex.flash(Some(err)),
                            None => {
                                ex.created(&to.unwrap_or_default());
                                ex.flash(None);
                            }
                        })
                    }))
                });
                items.push(It::new("Duplicate", duplicate, ""));
                let e = r.clone();
                items.push(Some(It {
                    danger: true,
                    ..It::new("Delete…", now(move |ex| ex.delete_entry(&e)), "delete").unwrap()
                }));
                items.push(None);
                let a = abs.clone();
                items.push(It::new(actions::REVEAL, job(move || actions::reveal(&a)), ""));
                let a = abs.clone();
                items.push(It::new("Copy path", job(move || Some(actions::copy(&a, "Path copied"))), "copyPath"));
                let rel = r.rel.clone();
                let copy = job(move || Some(actions::copy(&rel, "Path copied")));
                items.push(It::new("Copy relative path", copy, "copyRelativePath"));
                if let Some(repo) = self.repo_dir(&r.rel).filter(|_| deco != Some('!')) {
                    let changed = deco.is_some();
                    items.push(None);
                    if changed {
                        let e = r.clone();
                        let run = Rc::new(move |ex: &mut Explorer| ex.view_changes(&e));
                        items.push(It::new("View changes", run, "viewChanges"));
                    }
                    if !r.dir && deco != Some('U') {
                        let (a, rt, p) = (abs.clone(), repo.clone(), pane.clone());
                        items.push(It::new(
                            "File history",
                            job(move || actions::scm(&rt, &p, &a, true)),
                            "fileHistory",
                        ));
                    }
                    if changed {
                        let (a, rt) = (abs.clone(), repo.clone());
                        items.push(It::new("Stage", job(move || Some(actions::stage(&a, &rt))), ""));
                    }
                    let (a, rt) = (abs.clone(), repo.clone());
                    items.push(It::new("Unstage", job(move || Some(actions::unstage(&a, &rt))), ""));
                    if changed {
                        let (title, note) = ("Discard the changes?", "This cannot be undone.");
                        items.push(self.discard_item(&abs, &repo, title, r.name.clone(), note, "Discard"));
                    }
                }
            }
            None => {
                items.push(It::new("New file…", now(|ex| ex.new_entry("", false)), "newFile"));
                items.push(It::new("New folder…", now(|ex| ex.new_entry("", true)), "newFolder"));
                items.push(None);
                let (rt, p) = (root.clone(), pane.clone());
                items.push(It::new("Go to file…", job(move || actions::search(&rt, &p, "files", "")), "goToFile"));
                let (rt, p) = (root.clone(), pane.clone());
                let search = job(move || actions::search(&rt, &p, "content", ""));
                items.push(It::new("Search in files…", search, "searchInFiles"));
                if self.tree.git.on() {
                    // Outside a repository, that of the entry selected.
                    let selected = self.state.selected.as_deref().unwrap_or("");
                    if let Some(rt) = self.repo_dir(selected) {
                        let p = pane.clone();
                        items.push(It::new(
                            "Source control",
                            job(move || actions::scm(&rt, &p, "", false)),
                            "sourceControl",
                        ));
                    }
                    let label = if self.state.changed { "All files" } else { "Changed files only" };
                    items.push(It::new(label, now(|ex| ex.show_changed(!ex.state.changed)), "changedOnly"));
                }
                items.push(None);
                let rt = root.clone();
                items.push(It::new(actions::REVEAL, job(move || actions::reveal(&rt)), ""));
                let rt = root.clone();
                items.push(It::new("Copy path", job(move || Some(actions::copy(&rt, "Path copied"))), ""));
                let up = paths::dirname(&root);
                items.push(It::new("Up one level", now(move |ex| ex.set_root(&up)), "parentRoot"));
                if self.root != self.home {
                    let home = self.home.clone();
                    items.push(It::new("Back to the space folder", now(move |ex| ex.set_root(&home)), ""));
                }
                items.push(It::new("Refresh", now(|ex| ex.schedule()), "refresh"));
                items.push(It::new("Settings…", now(|ex| ex.open_settings()), "settings"));
            }
        }
        items
    }

    pub(super) fn open_menu(&mut self, i: Option<i64>) {
        let r = i.and_then(|i| usize::try_from(i).ok()).and_then(|i| self.state.rows.get(i)).cloned();
        if let (Some(_), Some(i)) = (&r, i) {
            self.select(i);
            self.reveal();
        }
        let items = self.menu_items(r.as_ref());
        let h = self.size().1 as i64;
        let y = match (&r, i) {
            (Some(_), Some(i)) => i - self.state.top + 1,
            _ => (h - 1).min(self.state.rows.len() as i64 - self.state.top + 1),
        };
        let mh = (items.len() as i64).min(h - 2) + 2;
        let title = match &r {
            Some(r) => format!("{}{}", r.name, if r.dir { "/" } else { "" }),
            None => format!("{}/", paths::basename(&self.root, None)),
        };
        let at = if y + 1 + mh <= h { y + 1 } else { (y - mh).min(h - mh).max(0) };
        // Each item with the first key of its action on the right.
        let items = items
            .into_iter()
            .map(|it| {
                it.map(|It { label, run, action, danger }| {
                    let key = action.and_then(|a| self.keys_of(a).into_iter().next());
                    ui::MenuItem {
                        label,
                        key: key.map_or(String::new(), |k| ui::key_label(&k)),
                        value: None,
                        danger,
                        keep: false,
                        run: Item { action, run },
                    }
                })
            })
            .collect();
        self.state.menu = Some(Menu::new(&title, items, at, 0, 2));
        self.render();
    }

    /// Runs an action: the menu closed, then its message shown, at once or when its job is done ("Action failed"
    /// when it panicked).
    pub(super) fn run_action(&mut self, action: impl FnOnce(&mut Explorer) -> Act) {
        self.state.menu = None;
        self.render();
        match action(self) {
            Act::Now(text) => self.flash(text),
            Act::Later(job) => self.herdr.send(Box::new(move || {
                std::panic::catch_unwind(AssertUnwindSafe(job))
                    .unwrap_or_else(|_| Box::new(|ex: &mut Explorer| ex.flash(Some("Action failed".into()))))
            })),
        }
    }
}
