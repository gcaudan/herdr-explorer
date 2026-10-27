//! Branches (checkout, create, rename, delete), the remote (fetch, pull, push) and stashes.

use super::{capital, card, count_of, num_text, opt, strings, trim};
use crate::core::ui::{Confirm, Field};
use crate::scm::parse::{self, Branch, Stash};
use crate::scm::state::{self, GitOpts};
use crate::scm::{Done, Inline, Pane, Yes};

impl Pane {
    pub(in crate::scm) fn pick_branch(&mut self, b: Option<crate::scm::context::PickItem>) {
        match b {
            None => {}
            Some(crate::scm::context::PickItem::Create(_)) => {
                let value = self.s.picker.as_ref().map_or(String::new(), |p| p.field.value.clone());
                self.new_branch(&value);
            }
            Some(crate::scm::context::PickItem::Branch(b)) => {
                let mode = self.s.picker.as_ref().and_then(|p| p.mode);
                self.s.picker = None;
                match mode {
                    Some(how) => self.integrate(b.branch, how),
                    None => self.checkout(&b.branch, b.current),
                }
            }
        }
    }

    pub(in crate::scm) fn checkout(&mut self, b: &Branch, current: bool) {
        let branch = self.status_of().map_or(String::new(), |s| s.branch.clone());
        if current || (!b.remote && b.name == branch) {
            return self.flash(format!("Already on {}", b.name), false);
        }
        if b.name.starts_with('-') {
            return self.flash("Branch name starting with \"-\": not switched", true);
        }
        if !b.remote {
            let name = b.name.clone();
            return self.op(
                &format!("Switching to {name}"),
                strings(&["switch", &name]),
                GitOpts::default(),
                false,
                move |p, res| {
                    if res.is_some() {
                        p.flash(format!("Switched to {name}"), false);
                    }
                },
            );
        }
        // A remote branch: its local branch if there is one, otherwise a new one tracking it.
        let local = b.name.find('/').map_or(b.name.clone(), |i| b.name[i + 1..].to_string());
        if local.starts_with('-') {
            return self.flash("Branch name starting with \"-\": not switched", true);
        }
        if local == branch {
            return self.flash(format!("Already on {local}"), false);
        }
        let known = self.s.refs.as_ref().is_some_and(|r| r.heads.iter().any(|h| h.name == local));
        let args = if known { strings(&["switch", &local]) } else { strings(&["switch", "--track", &b.name]) };
        self.op(&format!("Switching to {local}"), args, GitOpts::default(), false, move |p, res| {
            if res.is_some() {
                p.flash(format!("Switched to {local}"), false);
            }
        });
    }

    /// The new branch named on the picker's first row, from the filter, else the current branch's prefix (feat/).
    /// Enter creates it and switches to it, alt+Enter or ctrl+O only creates it.
    pub(in crate::scm) fn new_branch(&mut self, value: &str) {
        let cur = self.status_of().map_or(String::new(), |s| s.branch.clone());
        let start = if !value.is_empty() {
            // value.replace(/\s+/g, '-')
            let mut out = String::new();
            let mut space = false;
            for ch in value.chars() {
                if crate::unicode::is_space(ch) {
                    if !space {
                        out.push('-');
                    }
                    space = true;
                } else {
                    out.push(ch);
                    space = false;
                }
            }
            out
        } else {
            cur.rfind('/').map_or(String::new(), |i| cur[..=i].to_string())
        };
        if self.s.picker.is_none() {
            self.open_picker(None, None);
        }
        let Some(p) = self.s.picker.as_mut() else { return };
        p.sel = 0;
        p.top = 0;
        let from = format!("from {}", if cur.is_empty() { "HEAD" } else { &cur });
        self.inline(
            "branch",
            &start,
            from,
            &[("⏎", "create and switch"), ("ctrl+o", "create only"), ("esc", "cancel")],
            Done::Branch,
        );
    }

    /// A name typed in place, on its row (the picker's first one, or the list's first one). Enter gives the value,
    /// alt+Enter or ctrl+O too, with alt (Windows Terminal keeps alt+Enter for full screen). Escape cancels.
    pub(in crate::scm) fn inline(
        &mut self,
        kind: &'static str,
        value: &str,
        right: String,
        keys: &[(&str, &str)],
        done: Done,
    ) {
        let keys = keys.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect();
        self.inline = Some(Inline { kind, field: Field::new(value), right, keys, done });
        self.draw();
    }

    /// What Enter (alt: alt+Enter or ctrl+O) does with a name typed in place.
    pub(in crate::scm) fn inline_done(&mut self, done: Done, value: String, alt: bool) {
        match done {
            Done::Branch => {
                let name = trim(&value).to_string();
                if name.is_empty() {
                    return;
                }
                if name.starts_with('-') {
                    return self.flash("A branch name cannot start with \"-\"", true);
                }
                self.s.picker = None;
                let label = format!("Creating {name}");
                if alt {
                    self.op(&label, strings(&["branch", &name]), GitOpts::default(), false, move |p, res| {
                        if res.is_some() {
                            p.flash(format!("Created branch {name}"), false);
                        }
                    });
                } else {
                    self.op(&label, strings(&["switch", "-c", &name]), GitOpts::default(), false, move |p, res| {
                        if res.is_some() {
                            p.flash(format!("Switched to new branch {name}"), false);
                        }
                    });
                }
            }
            Done::Stash => {
                let mut args = strings(&["stash", "push"]);
                if !alt {
                    args.push("-u".into());
                }
                let msg = trim(&value);
                if !msg.is_empty() {
                    args.extend(["-m".into(), msg.to_string()]);
                }
                self.op("Stashing", args, GitOpts::default(), false, |p, res| {
                    if let Some(res) = res {
                        let first = trim(&res.out).split('\n').next().unwrap_or("").to_string();
                        p.flash(if first.is_empty() { "Stashed".into() } else { first }, false);
                    }
                });
            }
            Done::Tag { at, target } => {
                let name = trim(&value).to_string();
                if name.is_empty() {
                    return;
                }
                if name.starts_with('-') {
                    return self.flash("A tag name cannot start with \"-\"", true);
                }
                if !alt {
                    let args = strings(&["tag", &name, &target]);
                    return self.op(&format!("Tagging {at}"), args, GitOpts::default(), false, move |p, res| {
                        if res.is_some() {
                            p.flash(format!("Tagged {at} {name}"), false);
                        }
                    });
                }
                let right = format!("tag {name}");
                self.inline(
                    "message",
                    "",
                    right,
                    &[("⏎", "create"), ("esc", "cancel")],
                    Done::TagMessage { at, target, name },
                );
            }
            Done::TagMessage { at, target, name } => {
                let msg = trim(&value);
                if msg.is_empty() {
                    return self.flash("An annotated tag needs a message", true);
                }
                let opts =
                    GitOpts { input: Some(format!("{msg}\n").into_bytes()), detached: true, ..GitOpts::default() };
                let args = strings(&["tag", "-a", "--cleanup=strip", "-F", "-", &name, &target]);
                self.op(&format!("Tagging {at}"), args, opts, false, move |p, res| {
                    if res.is_some() {
                        p.flash(format!("Tagged {at} {name}"), false);
                    }
                });
            }
        }
    }

    pub(in crate::scm) fn delete_branch(&mut self, b: Branch) {
        if b.remote {
            return self.flash("Remote branches are not deleted from here", true);
        }
        if self.status_of().is_some_and(|s| s.branch == b.name) {
            return self.flash("Cannot delete the current branch", true);
        }
        if b.name.starts_with('-') {
            return;
        }
        self.s.picker = None;
        let q = card("Delete the branch?", b.name.clone(), "Delete");
        self.confirm(q, Yes::DeleteBranch(b));
    }

    pub(super) fn do_delete_branch(&mut self, b: Branch) {
        let label = format!("Deleting {}", b.name);
        self.op(&label, strings(&["branch", "-d", &b.name]), GitOpts::default(), true, move |p, res| {
            let Some(res) = res else { return };
            if res.code == Some(0) {
                return p.flash(format!("Deleted {}", b.name), false);
            }
            if !res.err.contains("not fully merged") {
                return p.flash(format!("Delete failed: {}", parse::error_of(&res)), true);
            }
            // The commits lost: those of the branch on no other branch (--exclude takes the name without
            // refs/heads/).
            let name = b.name.clone();
            p.query(
                move |top| {
                    let head = format!("refs/heads/{name}");
                    let exclude = format!("--exclude={name}");
                    let args = ["rev-list", "--count", &head, "--not", &exclude, "--branches", "--remotes", "--"];
                    let r = state::git(top, &args, &GitOpts::default());
                    if r.code == Some(0) { count_of(&r) } else { 0.0 }
                },
                move |p, n| {
                    let lost = if n != 0.0 && !n.is_nan() {
                        format!("its {} commit{} will be lost", num_text(n), if n > 1.0 { "s" } else { "" })
                    } else {
                        "its commits will be lost".into()
                    };
                    let q = Confirm {
                        where_: Some(format!("{} (git branch -D).", capital(&lost))),
                        note: opt("This cannot be undone.".into()),
                        ..card("Not fully merged", b.name.clone(), "Delete anyway")
                    };
                    p.confirm(q, Yes::DeleteAnyway(b));
                },
            );
        });
    }

    pub(in crate::scm) fn fetch(&mut self) {
        if self.repo().is_some() {
            self.net("Fetching", strings(&["fetch"]), |p, _| p.flash("Fetched", false));
        }
    }

    pub(in crate::scm) fn pull(&mut self) {
        let Some(s) = self.status_of().cloned() else { return };
        if s.upstream.is_empty() {
            return self.flash("No upstream branch to pull from", true);
        }
        let set = |n: f64| n != 0.0 && !n.is_nan();
        if set(s.ahead) && set(s.behind) {
            return self.flash(
                format!(
                    "{} and {} have diverged (↑{} ↓{}): merge or rebase in the terminal",
                    s.branch,
                    s.upstream,
                    num_text(s.ahead),
                    num_text(s.behind)
                ),
                true,
            );
        }
        self.net("Pulling", strings(&["pull", "--no-rebase", "--ff-only"]), |p, _| p.flash("Pulled", false));
    }

    /// The first push of a branch publishes it on origin (or the only remote) and sets its upstream.
    pub(in crate::scm) fn push(&mut self) {
        let Some(s) = self.status_of().cloned() else { return };
        if s.detached {
            return self.flash("Detached HEAD: switch to a branch to push", true);
        }
        if !s.upstream.is_empty() {
            return self.net("Pushing", strings(&["push"]), |p, _| p.flash("Pushed", false));
        }
        self.query(
            |top| state::git(top, &["remote"], &GitOpts::default()),
            move |p, res| {
                let remotes: Vec<String> = res
                    .out
                    .split('\n')
                    .map(|l| l.strip_suffix('\r').unwrap_or(l))
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                let Some(first) = remotes.first() else {
                    return p.flash("No remote: add one with git remote add", true);
                };
                let remote = if remotes.iter().any(|r| r == "origin") { "origin".to_string() } else { first.clone() };
                // A remote named after an option (--receive-pack=…) would be read as one.
                if remote.starts_with('-') {
                    return p.flash(format!("Invalid remote name: {remote}"), true);
                }
                let branch = s.branch.clone();
                p.net(&format!("Publishing {branch}"), strings(&["push", "-u", &remote, "HEAD"]), move |p, _| {
                    p.flash(format!("Published {branch} on {remote}"), false)
                });
            },
        );
    }

    pub(in crate::scm) fn new_stash(&mut self) {
        let Some(s) = self.status_of() else { return };
        let n = s.staged.len() + s.changes.len();
        if self.s.view != "stashes" {
            self.set_view("stashes");
        }
        // Enter stashes the untracked files too, as before; alt+Enter or ctrl+O leaves them.
        let right = format!("{n} file{}", if n == 1 { "" } else { "s" });
        self.inline(
            "stash",
            "",
            right,
            &[("⏎", "stash"), ("ctrl+o", "keep untracked files"), ("esc", "cancel")],
            Done::Stash,
        );
    }

    pub(in crate::scm) fn stash_action(&mut self, s: Stash, action: &'static str) {
        if action == "apply" {
            return self.run_stash(s, action);
        }
        let q = if action == "drop" {
            Confirm {
                where_: Some(s.r#ref.clone()),
                note: opt("This cannot be undone.".into()),
                ..card("Drop the stash?", s.subject.clone(), "Drop")
            }
        } else {
            Confirm {
                where_: Some(format!("{}: applied, then dropped", s.r#ref)),
                tone: "go".into(),
                ..card("Pop the stash?", s.subject.clone(), "Pop")
            }
        };
        self.confirm(q, Yes::Stash(s, action));
    }

    pub(super) fn run_stash(&mut self, s: Stash, action: &'static str) {
        let label = format!("{} {}", capital(action), s.r#ref);
        self.op(&label, strings(&["stash", action, &s.r#ref]), GitOpts::default(), false, move |p, res| {
            if res.is_some() {
                p.flash(format!("{}: {action} done", s.r#ref), false);
            }
        });
    }
}
