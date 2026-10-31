//! What the source control pane does to the repository: stage, unstage, discard, commit (with a
//! message written by Claude on request), continue or abort an operation, branches, fetch, pull, push, stashes, merge,
//! rebase, cherry-pick, tags, force push, undo commit and reset --hard; the rows opened, and the menu (m) of these
//! actions.
//!
//! This file holds the helpers they share, the rows opened and the menu; changes.rs, commit.rs, branches.rs and
//! history.rs the operations.

mod branches;
mod changes;
mod commit;
mod history;
mod pulls;

use super::context::Row;
use super::parse::{self, Item};
use super::state::{self, FROM_FILE, GitOpts};
use super::{Op, Pane, Yes};
use crate::core::exec::{self, Exec, Opts};
use crate::core::ui::{Choice, Confirm, Menu, MenuItem};
use crate::core::{actions, paths};
use crate::diff;
use history::STRATEGIES;

/// The index entries of the staged files (unindex) that are gone (went, their paths) taken out, in the repository
/// top, with the hardened environment; the message to show when it fails.
fn unindexed(top: &str, unindex: Option<&[Item]>, went: &[String]) -> Option<String> {
    let gone: Vec<Item> = unindex?.iter().filter(|i| went.contains(&i.path)).cloned().collect();
    if gone.is_empty() {
        return None;
    }
    let mut args = vec!["rm", "--cached", "-r", "-q"];
    args.extend(FROM_FILE);
    let res = state::git(top, &args, &GitOpts { safe: true, input: Some(state::nul(&gone)), ..GitOpts::default() });
    (res.code != Some(0)).then(|| format!("Discarding failed: {}", parse::error_of(&res)))
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// A text of a card, None when empty (left out).
fn opt(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn card(title: impl Into<String>, name: impl Into<String>, action: &str) -> Confirm {
    Confirm { title: title.into(), name: name.into(), action: action.into(), ..Confirm::default() }
}

fn plural(n: f64, word: &str) -> String {
    format!("{} {word}{}", num_text(n), if n > 1.0 { "s" } else { "" })
}

/// A number as text (NaN included).
fn num_text(n: f64) -> String {
    if n.is_nan() { "NaN".into() } else { crate::json::number(n) }
}

/// The first letter in capital: "Merge".
fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

fn trim(s: &str) -> &str {
    s.trim_matches(crate::unicode::is_space)
}

/// `+text.trim()`: 0 for an empty text, NaN for a text that is not a number.
fn count_of(res: &Exec) -> f64 {
    match trim(&res.out) {
        "" => 0.0,
        t => t.parse().unwrap_or(f64::NAN),
    }
}

/// What --continue would run that the hardening cannot stop, from files a repository received with its .git can
/// carry: the exec lines of a rebase, and a merge strategy other than git's own, run as git merge-<name>, which can be
/// an alias of the repository's config.
fn unsafe_to_continue(top: &str, gitdir: &str) -> Option<String> {
    let file = |name: &str| paths::join(&[gitdir, name]);
    let read = |name: &str| exec::read_text(file(name)).unwrap_or_default();
    // /^\s*(x|exec)(\s|$)/m
    let todo = read("rebase-merge/git-rebase-todo");
    let exec_line = todo.split(crate::core::regex::is_line_terminator).any(|l| {
        let l = l.trim_start_matches(crate::unicode::is_space);
        ["x", "exec"].iter().any(|w| {
            l.strip_prefix(w).is_some_and(|rest| rest.is_empty() || rest.starts_with(crate::unicode::is_space))
        })
    });
    if exec_line {
        return Some("commands (exec lines)".into());
    }
    let mut strategies = vec![trim(&read("rebase-merge/strategy")).to_string()];
    // sequencer/opts read by git itself, as the sequencer does (no includes): its syntax has more forms than a key
    // per line. Exit code 1: no strategy set.
    let opts = file("sequencer/opts");
    if std::path::Path::new(&opts).exists() {
        let res = state::git(top, &["config", "--file", &opts, "--get-all", "options.strategy"], &GitOpts::default());
        if !matches!(res.code, Some(0 | 1)) {
            return Some("options it could not check (sequencer/opts)".into());
        }
        strategies.extend(res.out.split('\n').map(String::from));
    }
    // Without one there, the strategy is pull.twohead, its first value: one of the repository's wins over -c.
    let twohead = state::git(top, &["config", "-z", "--includes", "--get-all", "pull.twohead"], &GitOpts::default());
    if !matches!(twohead.code, Some(0 | 1)) {
        return Some("options it could not check (pull.twohead)".into());
    }
    strategies.extend(twohead.out.split('\0').map(String::from));
    strategies
        .iter()
        .map(|s| trim(s))
        .find(|s| !s.is_empty() && !STRATEGIES.contains(s))
        .map(|s| format!("the merge strategy {s}"))
}

impl Pane {
    fn status_of(&self) -> Option<&parse::Status> {
        self.repo().map(|r| &r.status)
    }

    fn head_name(&self) -> String {
        self.repo().map_or(String::new(), |r| state::head_name(r).to_string())
    }

    pub(super) fn open_row(&mut self, row: Option<&Row>, follow: bool) {
        let Some(row) = row else { return };
        if row.kind == "file" {
            if let Some(it) = &row.item {
                self.open_item_diff(it);
            }
            return;
        }
        if let Some(p) = row.pull.as_ref().filter(|_| row.kind == "pull") {
            let title = format!("#{} {}", p.number, p.title);
            return self.show_pull(&row.key.clone(), &title, p.number);
        }
        if let Some((what, title)) = self.show_of(Some(row)) {
            return self.show_diff(&row.key.clone(), &title, diff::show_args(&what), None, "");
        }
        if follow {
            if state::comparable(&self.s, Some(row)) {
                self.compare(row, false);
            }
            return;
        }
        match row.kind {
            "section" | "folder" => self.toggle(row, None),
            "branch" => {
                if let Some(b) = row.branch() {
                    let current = !b.remote && row.current;
                    self.checkout(&b.clone(), current);
                }
            }
            "action" => self.new_stash(),
            _ => {}
        }
    }

    /// Go to file and Search in files, as from the explorer: the files open in the tab's preview.
    pub(super) fn search(&mut self, mode: &'static str) {
        let top = self.top();
        let from = exec::env_var("EXPLORER_PANE").filter(|v| !v.is_empty()).unwrap_or_else(|| self.pane.clone());
        self.herdr.send(Box::new(move || {
            let msg = actions::search(&top, &from, mode, "");
            Box::new(move |p: &mut Pane| {
                if let Some(msg) = msg {
                    p.flash(msg, true);
                }
            })
        }));
    }

    /// y Y e E o on a file of Changes, as in the explorer and the preview: copy its path or relative path, open it in
    /// the editor, edit it in the terminal, open it in its app.
    pub(super) fn file_action(&mut self, k: &str, row: Option<&Row>) {
        let Some(it) = row.filter(|r| r.kind == "file").and_then(|r| r.item.as_ref()) else {
            return self.flash("Select a file first", false);
        };
        let rel = it.path.clone();
        let abs = paths::join(&[&self.top(), &rel]);
        if "eEo".contains(k) && !std::path::Path::new(&abs).exists() {
            return self.flash(format!("{rel} is deleted"), true);
        }
        let (k, pane) = (k.to_string(), self.pane.clone());
        self.herdr.send(Box::new(move || {
            let msg = match k.as_str() {
                "y" => Some(actions::copy(&abs, "Path copied")),
                "Y" => Some(actions::copy(&rel, "Relative path copied")),
                "e" => actions::editor(&abs, &pane),
                "E" => actions::edit(&abs, &pane, None),
                _ => actions::app(&abs, &pane),
            };
            // The actions' messages tell success from failure (ui::message_kind), as in the explorer.
            Box::new(move |p: &mut Pane| {
                if let Some(msg) = msg.filter(|m| !m.is_empty()) {
                    p.flash(msg, false);
                }
            })
        }));
    }

    /// m, the menu key, shift+F10 or the header's ⋯: the actions of the selected branch or commit, with their keys,
    /// then those of the repository. From a row, it opens under it; from the header, on the right.
    pub(super) fn open_menu(&mut self, from_header: bool) {
        let Some(r) = self.repo() else { return };
        let row = if from_header { None } else { self.frame.rows.get(self.s.cursor as usize).cloned() };
        let mut title = paths::basename(&r.top, None);
        let head = self.head_name();
        let item = |label: String, key: &str, danger: bool, run: Op| {
            Some(MenuItem { label, key: key.into(), value: None, danger, keep: false, run })
        };
        let mut items = Vec::new();
        match &row {
            Some(row) if self.s.view == "refs" && row.kind == "branch" && !row.current => {
                let b = row.branch().cloned().unwrap_or_default();
                title = b.name.clone();
                items.push(item("Check out".into(), "", false, Op::Checkout(b.clone())));
                items.push(item(format!("Merge into {head}"), "M", false, Op::Integrate(b.clone(), "merge")));
                items.push(item(format!("Rebase {head} onto it"), "R", false, Op::Integrate(b.clone(), "rebase")));
                if !b.remote {
                    items.push(item("Delete".into(), "D", true, Op::DeleteBranch(b)));
                }
                items.push(None);
            }
            Some(row) if self.s.view == "history" && row.kind == "commit" => {
                let c = row.commit.as_ref().map(|h| h.commit.clone()).unwrap_or_default();
                title = format!("{} {}", c.short, c.subject);
                items.push(item("Create tag…".into(), "t", false, Op::NewTag(Some(c.clone()))));
                items.push(item(format!("Cherry-pick onto {head}"), "C", false, Op::CherryPick(c.clone())));
                items.push(item(format!("Reset --hard {head} here…"), "", true, Op::ResetHard(Some(c))));
                items.push(None);
            }
            _ => {}
        }
        // The picker of a branch to merge or rebase onto hangs where this menu was.
        items.extend([
            item("Merge a branch…".into(), "", false, Op::Picker("merge")),
            item("Rebase onto a branch…".into(), "", false, Op::Picker("rebase")),
            item("Create tag on HEAD…".into(), "T", false, Op::NewTag(None)),
            None,
            item("Fetch".into(), "f", false, Op::Fetch),
            item("Pull".into(), "L", false, Op::Pull),
            item("Push".into(), "P", false, Op::Push),
            item("Force push (with lease)…".into(), "", true, Op::ForcePush),
            None,
            item("Undo last commit…".into(), "", false, Op::UndoCommit),
            item("Reset --hard to HEAD…".into(), "", true, Op::ResetHard(None)),
        ]);
        let (w, h) = self.size();
        let y = self.frame.list_y + self.s.cursor - self.s.top;
        let at = if from_header || row.is_none() { 1 } else { 1.max((y + 1).min(h - 3 - items.len() as i64)) };
        self.menu = Some(Menu::new(&title, items, at, if from_header { w } else { 0 }, 1));
        self.menu_header = from_header;
        self.menu_view = false;
        self.draw();
    }

    /// The diff's options (⚙ or , in the diff), as in the preview's Diff view: each changes in place, the menu
    /// staying open (keep).
    pub(super) fn open_diff_view(&mut self, sel: usize) {
        let Some(d) = self.s.diff.as_ref().filter(|d| !d.compare) else { return };
        let items =
            diff::view_items(diff::layout_of(d.width), self.wrap, Op::Layout, Op::Wrap).into_iter().map(Some).collect();
        // under ⚙
        let mut menu = Menu::new("View", items, 2, self.size().0, 1);
        menu.sel = sel;
        self.menu = Some(menu);
        // reopened after a choice that keeps it
        self.menu_header = false;
        self.menu_view = true;
        self.draw();
    }

    pub(super) fn switch_layout(&mut self) {
        let Some(d) = &self.s.diff else { return };
        super::render::switch_layout(d.width);
        self.draw();
    }

    pub(super) fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        if let Some(d) = self.s.diff.as_mut() {
            d.left = 0;
        }
        let mut settings = exec::read_settings();
        settings.set("wrap", crate::json::Value::Bool(self.wrap));
        exec::write_settings(&settings);
        self.draw();
    }

    /// A choice of the menu, by key or click: the menu closes, then the item runs; the diff's options show again with
    /// their new values (keep).
    pub(super) fn menu_choice(&mut self, r: Option<Choice<Op>>) {
        let Some(menu) = &self.menu else { return };
        let (sel, view) = (menu.sel, self.menu_view);
        // The picker hangs where this menu was: its right edge and its top line.
        let anchor = (menu.x + menu.inner + 1, menu.at);
        match r {
            None => {}
            Some(Choice::Close) => {
                self.menu = None;
                self.draw();
            }
            Some(Choice::Run { run, keep }) => {
                self.menu = None;
                self.draw();
                self.run_op(run, anchor);
                if keep && view {
                    self.open_diff_view(sel);
                }
            }
        }
        self.draw();
    }

    fn run_op(&mut self, op: Op, anchor: (i64, i64)) {
        match op {
            Op::Checkout(b) => self.checkout(&b, false),
            Op::Integrate(b, how) => self.integrate(b, how),
            Op::DeleteBranch(b) => self.delete_branch(b),
            Op::NewTag(c) => self.new_tag(c),
            Op::CherryPick(c) => self.cherry_pick(c),
            Op::ResetHard(c) => self.reset_hard(c),
            Op::Picker(mode) => self.open_picker(Some(mode), Some(anchor)),
            Op::Fetch => self.fetch(),
            Op::Pull => self.pull(),
            Op::Push => self.push(),
            Op::ForcePush => self.force_push(),
            Op::UndoCommit => self.undo_commit(),
            Op::Layout => self.switch_layout(),
            Op::Wrap => self.toggle_wrap(),
        }
    }

    /// What y does to a question of the card.
    pub(super) fn yes(&mut self, yes: Yes) {
        match yes {
            Yes::Init => {
                let root = self.root.clone();
                self.git.send(Box::new(move || {
                    let mut env = exec::git_env(&root);
                    state::english(&mut env);
                    let res = exec::exec(
                        exec::git(),
                        &["init"],
                        &Opts { cwd: Some(root), env: Some(env), ..Opts::default() },
                    );
                    Box::new(move |p: &mut Pane| {
                        let ok = res.code == Some(0);
                        let text = if ok {
                            "Repository created".into()
                        } else {
                            format!("git init failed: {}", parse::error_of(&res))
                        };
                        p.flash(text, !ok);
                        p.schedule();
                    })
                }));
            }
            Yes::DeleteForGood { top, left, unindex } => self.delete_for_good(top, left, unindex),
            Yes::DiscardStaged { top, others, added } => self.discard_staged(top, others, added),
            Yes::Discard { top, tracked, untracked } => self.discard_changes(top, tracked, untracked),
            Yes::StageAndCommit(msg) => {
                self.op("Staging all", strings(&["add", "-A"]), GitOpts::default(), false, move |p, res| {
                    if res.is_some() {
                        p.reload_status(move |p| p.do_commit(msg));
                    }
                });
            }
            Yes::Abort(o) => self.do_abort(o),
            Yes::DeleteBranch(b) => self.do_delete_branch(b),
            Yes::DeleteAnyway(b) => {
                let label = format!("Deleting {}", b.name);
                self.op(&label, strings(&["branch", "-D", &b.name]), GitOpts::default(), false, move |p, res| {
                    if res.is_some() {
                        p.flash(format!("Deleted {}", b.name), false);
                    }
                });
            }
            Yes::Stash(s, action) => self.run_stash(s, action),
            Yes::Integrate(b, how) => self.do_integrate(b, how),
            Yes::CherryPick(c, merge) => self.do_cherry_pick(c, merge),
            Yes::ForcePush(upstream) => {
                self.net("Force pushing", strings(&["push", "--force-with-lease"]), move |p, _| {
                    p.flash(format!("Force pushed to {upstream}"), false)
                });
            }
            Yes::Undo(subject, whole) => {
                self.op(
                    "Undoing the last commit",
                    strings(&["reset", "--soft", "-q", "HEAD~1"]),
                    GitOpts::default(),
                    false,
                    move |p, res| {
                        if res.is_none() {
                            return;
                        }
                        if p.s.field.value.is_empty() {
                            p.s.field.set(&subject);
                            p.s.kept = Some((subject, whole));
                        }
                        p.flash("Last commit undone: its changes are staged", false);
                    },
                );
            }
            Yes::Reset(c) => {
                let target = c.as_ref().map_or("HEAD".to_string(), |c| c.hash.clone());
                let head = self.head_name();
                self.op(
                    "Resetting",
                    strings(&["reset", "--hard", "-q", &target]),
                    GitOpts::default(),
                    false,
                    move |p, res| {
                        if res.is_some() {
                            p.flash(
                                c.map_or("Changes discarded".into(), |c| format!("{head} reset to {}", c.short)),
                                false,
                            );
                        }
                    },
                );
            }
        }
    }
}
