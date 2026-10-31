//! Merge, rebase, cherry-pick, tags, force push, undo commit and reset --hard; the branch compared with.

use super::commit::{SUBJECT_AND_MESSAGE, subject_and_message};
use super::{card, count_of, opt, plural, strings, trim};
use crate::core::exec::Exec;
use crate::core::ui::Confirm;
use crate::scm::context::{Row, files};
use crate::scm::parse::{self, Branch, Commit};
use crate::scm::state::{self, GitOpts};
use crate::scm::{Done, Pane, Yes};

/// The merge strategy is always given: without it, git takes pull.twohead, whose first value, the repository's, wins
/// over -c, and a strategy other than git's own runs as git merge-<name>, which can be an alias of the repository's
/// config (see unsafe_to_continue).
const STRATEGY: &str = "--strategy=ort";

pub(super) const STRATEGIES: [&str; 6] = ["ort", "recursive", "resolve", "octopus", "ours", "subtree"];

impl Pane {
    /// An operation already under way (merge, rebase, bisect…): the new one is refused until it ends.
    fn under_way(&mut self) -> bool {
        let state = self.repo().map_or("", |r| r.state);
        if state.is_empty() {
            return false;
        }
        self.flash(format!("{} in progress: continue or abort it first", state.to_lowercase()), true);
        true
    }

    /// The end of a merge, rebase or cherry-pick: done, stopped (on conflicts, which the Changes view shows with
    /// Continue and Abort, as for an operation started in the terminal), or failed.
    fn settle(&mut self, res: Option<Exec>, name: &'static str, done: String) {
        let Some(res) = res else { return };
        if res.code == Some(0) {
            let up = res.out.to_lowercase().contains("up to date");
            return self.flash(if up { "Already up to date".into() } else { done }, false);
        }
        self.reload_status(move |p| {
            let Some(r) = p.repo().filter(|r| !r.state.is_empty()) else {
                return p.flash(format!("{name} failed: {}", parse::error_of(&res)), true);
            };
            let conflicts = !r.status.conflicts.is_empty();
            p.set_view("changes");
            let text = if conflicts {
                "Stopped on conflicts: resolve them, stage them, then continue, or X to abort".into()
            } else {
                format!("Stopped: {}; continue, or X to abort", parse::error_of(&res))
            };
            p.flash(text, true);
        });
    }

    /// git's own message, which the full name given to git merge would otherwise show: "Merge branch 'feat' into
    /// dev", without " into" for main and master (merge.suppressDest).
    fn merge_message(&self, b: &Branch) -> String {
        let s = self.status_of().cloned().unwrap_or_default();
        let kind = if b.remote { "remote-tracking branch" } else { "branch" };
        let into = if s.detached || s.branch == "main" || s.branch == "master" {
            String::new()
        } else {
            format!(" into {}", s.branch)
        };
        format!("Merge {kind} '{}'{into}", b.name)
    }

    /// M and R on a branch of Refs, or the picker of the menu's Merge a branch… and Rebase onto a branch…: the branch
    /// merged into the current one, or the current one's commits replayed on top of it.
    pub(in crate::scm) fn integrate(&mut self, b: Branch, how: &'static str) {
        let Some(s) = self.status_of().cloned() else { return };
        if !s.has_head {
            return self.flash("No commits yet", true);
        }
        if self.under_way() {
            return;
        }
        if !b.remote && b.name == s.branch {
            return self.flash(format!("{} is the current branch", b.name), true);
        }
        let head = self.head_name();
        let q = if how == "rebase" {
            Confirm {
                where_: Some("its own commits replayed on top".into()),
                note: opt(if s.upstream.is_empty() { "" } else { "Pushed commits rewritten: then force push." }.into()),
                tone: "go".into(),
                ..card("Rebase the current branch?", format!("{head} onto {}", b.name), "Rebase")
            }
        } else {
            Confirm {
                tone: "go".into(),
                ..card("Merge into the current branch?", format!("{} into {head}", b.name), "Merge")
            }
        };
        self.confirm(q, Yes::Integrate(b, how));
    }

    /// GIT_EDITOR ':' keeps the message git prepares.
    pub(super) fn do_integrate(&mut self, b: Branch, how: &'static str) {
        let rebase = how == "rebase";
        let full = state::full_ref(&b);
        let args = if rebase {
            strings(&["rebase", STRATEGY, &full])
        } else {
            strings(&["merge", "--no-edit", "-m", &self.merge_message(&b), STRATEGY, &full])
        };
        let opts = GitOpts {
            detached: true,
            env: vec![("GIT_EDITOR".into(), ":".into())],
            config: state::merge_options(&self.s),
            ..GitOpts::default()
        };
        let label = if rebase { format!("Rebasing onto {}", b.name) } else { format!("Merging {}", b.name) };
        self.op(&label, args, opts, true, move |p, res| {
            let (name, done) = if rebase {
                ("Rebase", format!("Rebased onto {}", b.name))
            } else {
                ("Merge", format!("Merged {}", b.name))
            };
            p.settle(res, name, done);
        });
    }

    /// C on a commit of History: its changes committed again on the current branch; a merge commit, those it
    /// brought to its first parent.
    pub(in crate::scm) fn cherry_pick(&mut self, c: Commit) {
        if !self.status_of().is_some_and(|s| s.has_head) {
            return self.flash("No commits yet", true);
        }
        if self.under_way() {
            return;
        }
        let hash = c.hash.clone();
        self.query(
            move |top| {
                let opts = GitOpts::default();
                if state::git(top, &["merge-base", "--is-ancestor", &hash, "HEAD"], &opts).code == Some(0) {
                    return None;
                }
                Some(state::git(top, &["rev-parse", "--verify", "-q", &format!("{hash}^2")], &opts).code == Some(0))
            },
            move |p, merge| {
                let head = p.head_name();
                let Some(merge) = merge else {
                    return p.flash(format!("{} is already in {head}", c.short), false);
                };
                let q = Confirm {
                    where_: Some(format!("onto {head}{}", if merge { ", from its first parent" } else { "" })),
                    tone: "go".into(),
                    ..card("Cherry-pick the commit?", format!("{} {}", c.short, c.subject), "Cherry-pick")
                };
                p.confirm(q, Yes::CherryPick(c, merge));
            },
        );
    }

    pub(super) fn do_cherry_pick(&mut self, c: Commit, merge: bool) {
        let mut args = strings(&["cherry-pick", STRATEGY]);
        if merge {
            args.extend(strings(&["-m", "1"]));
        }
        args.push(c.hash.clone());
        let opts = GitOpts { detached: true, env: vec![("GIT_EDITOR".into(), ":".into())], ..GitOpts::default() };
        self.op(&format!("Cherry-picking {}", c.short), args, opts, true, move |p, res| {
            p.settle(res, "Cherry-pick", format!("Cherry-picked {}", c.short));
        });
    }

    /// t on a commit of History, T elsewhere: a tag typed in place, on that commit or on HEAD. Enter makes a
    /// lightweight tag; alt+Enter or ctrl+O asks for its message, for an annotated one.
    pub(in crate::scm) fn new_tag(&mut self, c: Option<Commit>) {
        if !self.status_of().is_some_and(|s| s.has_head) {
            return self.flash("No commits yet", true);
        }
        let (at, target) = match &c {
            Some(c) => (c.short.clone(), c.hash.clone()),
            None => (self.head_name(), "HEAD".into()),
        };
        let right = format!("on {at}");
        self.inline(
            "tag",
            "",
            right,
            &[("⏎", "create"), ("ctrl+o", "annotated…"), ("esc", "cancel")],
            Done::Tag { at, target },
        );
    }

    /// The menu's Force push: --force-with-lease only, which git refuses when the upstream moved since the last
    /// fetch.
    pub(super) fn force_push(&mut self) {
        let Some(s) = self.status_of().cloned() else { return };
        if s.detached {
            return self.flash("Detached HEAD: switch to a branch to push", true);
        }
        if s.upstream.is_empty() {
            return self.flash("No upstream branch: publish the branch first", true);
        }
        let behind = s.behind != 0.0 && !s.behind.is_nan();
        let q = Confirm {
            where_: opt(if behind {
                format!("{} only there will be lost", plural(s.behind, "commit"))
            } else {
                String::new()
            }),
            note: opt("This cannot be undone.".into()),
            ..card(
                "Force push?",
                format!(
                    "{} replaces {}, unless it moved since the last fetch (--force-with-lease).",
                    s.branch, s.upstream
                ),
                "Force push",
            )
        };
        self.confirm(q, Yes::ForcePush(s.upstream));
    }

    /// The menu's Undo last commit: reset --soft HEAD~1, its changes left staged and its message in the field, as in
    /// VS Code.
    pub(super) fn undo_commit(&mut self) {
        if !self.status_of().is_some_and(|s| s.has_head) {
            return self.flash("No commits yet", true);
        }
        if self.under_way() {
            return;
        }
        self.query(
            |top| {
                let opts = GitOpts::default();
                std::thread::scope(|sc| {
                    let parent = sc.spawn(|| state::git(top, &["rev-parse", "--verify", "-q", "HEAD~1"], &opts));
                    let last = sc.spawn(|| state::git(top, &["log", "-1", SUBJECT_AND_MESSAGE], &opts));
                    let mine = state::git(top, &["rev-list", "--count", "HEAD", "--not", "--remotes", "--"], &opts);
                    let get = |h: std::thread::ScopedJoinHandle<Exec>| {
                        h.join().unwrap_or(Exec { code: None, out: String::new(), err: String::new() })
                    };
                    (get(parent), get(last), mine)
                })
            },
            |p, (parent, last, mine)| {
                if parent.code != Some(0) {
                    return p.flash("The first commit cannot be undone from here", true);
                }
                // On a remote branch already: none of its commits is only here.
                let pushed = mine.code == Some(0) && trim(&mine.out) == "0";
                let (subject, whole) = subject_and_message(&last.out);
                let q = Confirm {
                    where_: Some("its changes stay staged (reset --soft)".into()),
                    note: opt(if pushed { "Already pushed: the branch will need a force push." } else { "" }.into()),
                    tone: if pushed { "danger" } else { "go" }.into(),
                    ..card("Undo the last commit?", subject.clone(), "Undo")
                };
                p.confirm(q, Yes::Undo(subject, whole));
            },
        );
    }

    /// The menu's Reset --hard, to HEAD or to a commit of History: the changes of the tracked files are lost, and
    /// the commits of the current branch that no other branch, tag or remote branch has (left in the reflog only).
    /// Untracked files are kept.
    pub(super) fn reset_hard(&mut self, c: Option<Commit>) {
        let Some(s) = self.status_of().cloned() else { return };
        if !s.has_head {
            return self.flash("No commits yet", true);
        }
        if self.under_way() {
            return;
        }
        let mut seen = std::collections::HashSet::new();
        let changed = s
            .conflicts
            .iter()
            .chain(&s.staged)
            .chain(s.changes.iter().filter(|i| !i.untracked))
            .filter(|i| seen.insert(i.path.clone()))
            .count();
        let Some(commit) = c else { return self.ask_reset(None, changed, 0.0) };
        let (hash, detached, branch) = (commit.hash.clone(), s.detached, s.branch.clone());
        self.query(
            move |top| {
                let exclude = format!("--exclude={branch}");
                let mut args = vec!["rev-list", "--count", "HEAD", "--not", &hash];
                if !detached {
                    args.push(&exclude);
                }
                args.extend(["--branches", "--tags", "--remotes", "--"]);
                let res = state::git(top, &args, &GitOpts::default());
                let n = count_of(&res);
                if res.code == Some(0) && !n.is_nan() { n } else { 0.0 }
            },
            move |p, commits| p.ask_reset(Some(commit), changed, commits),
        );
    }

    fn ask_reset(&mut self, c: Option<Commit>, changed: usize, commits: f64) {
        if c.is_none() && changed == 0 {
            return self.flash("No changes to discard (files not tracked are kept)", false);
        }
        let mut lost = Vec::new();
        if changed > 0 {
            lost.push(format!("the changes in {}", files(changed)));
        }
        if commits != 0.0 {
            lost.push(format!("{} on no other branch", plural(commits, "commit")));
        }
        let head = self.head_name();
        let moves = c.as_ref().map_or(String::new(), |c| format!(" {head} moves to {} {}", c.short, c.subject));
        let lost = if lost.is_empty() { "nothing".into() } else { lost.join(" and ") };
        let title = c.as_ref().map_or("Reset --hard to HEAD?".into(), |c| format!("Reset --hard to {}?", c.short));
        let q = Confirm {
            where_: Some("files not tracked are kept".into()),
            note: opt("This cannot be undone.".into()),
            ..card(title, format!("Lost: {lost}.{moves}"), "Reset hard")
        };
        self.confirm(q, Yes::Reset(c));
    }

    /// B on a branch: the base of the comparisons, again B on it: back to the current branch.
    pub(in crate::scm) fn set_base(&mut self, row: &Row) {
        let Some(b) = row.branch() else { return };
        let same = self.s.compare_base.as_ref().is_some_and(|base| state::full_ref(base) == state::full_ref(b));
        self.s.compare_base = if same || (row.current && !b.remote) { None } else { Some(b.clone()) };
        self.flash(format!("Branches compared with {}", state::base_name(&self.s)), false);
        self.s.diff = None;
        self.follow();
    }
}
