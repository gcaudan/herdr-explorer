//! Stage, unstage and discard: files and folders, to the trash or for good once asked.

use super::{card, opt, strings, unindexed};
use crate::core::ui::Confirm;
use crate::core::{actions, stage, worker};
use crate::scm::context::files;
use crate::scm::parse::Item;
use crate::scm::state::{self, FROM_FILE, GitOpts};
use crate::scm::{Pane, Yes};

impl Pane {
    pub(in crate::scm) fn stage(&mut self, items: Vec<Item>) {
        if items.is_empty() {
            return;
        }
        let conflict = items[0].sec == "conflicts";
        let mut args = vec!["add"];
        if !conflict {
            args.push("-A");
        }
        args.extend(FROM_FILE);
        let opts = GitOpts { input: Some(state::nul(&items)), ..GitOpts::default() };
        self.op("Staging", strings(&args), opts, false, move |p, res| {
            if res.is_some() {
                p.pulse(&items, "staged");
            }
        });
    }

    pub(in crate::scm) fn unstage(&mut self, items: Vec<Item>) {
        if items.is_empty() {
            return;
        }
        let has_head = self.status_of().is_some_and(|s| s.has_head);
        let mut args = if has_head { vec!["restore", "--staged"] } else { vec!["rm", "--cached", "-r", "-q"] };
        args.extend(FROM_FILE);
        let opts = GitOpts { input: Some(state::nul(&items)), ..GitOpts::default() };
        self.op("Unstaging", strings(&args), opts, false, move |p, res| {
            if res.is_some() {
                p.pulse(&items, "changes");
            }
        });
    }

    pub(in crate::scm) fn stage_all(&mut self) {
        let Some(s) = self.status_of() else { return };
        let items: Vec<Item> = s.conflicts.iter().chain(&s.changes).cloned().collect();
        if items.is_empty() {
            return self.flash("Nothing to stage", false);
        }
        self.op("Staging all", strings(&["add", "-A"]), GitOpts::default(), false, move |p, res| {
            if res.is_some() {
                p.pulse(&items, "staged");
            }
        });
    }

    pub(in crate::scm) fn unstage_all(&mut self) {
        let Some(s) = self.status_of() else { return };
        let items = s.staged.clone();
        if items.is_empty() {
            return self.flash("Nothing staged", false);
        }
        // A path, even all of them: reset without one ends a merge, cherry-pick or revert under way.
        let args: &[&str] =
            if s.has_head { &["reset", "-q", "--", "."] } else { &["rm", "--cached", "-r", "-q", "--", "."] };
        self.op("Unstaging all", strings(args), GitOpts::default(), false, move |p, res| {
            if res.is_some() {
                p.pulse(&items, "changes");
            }
        });
    }

    /// Files of the repository top (paths from it) to the trash; those it does not take are deleted for good once
    /// asked again. Never a path that resolves outside the repository, nor one through a symbolic link (refused); one
    /// already gone is left out. unindex: the staged files whose index entry goes once they are gone (once moved,
    /// then once deleted for good). kept: what becomes of those not deleted for good, said in the question.
    fn trash_files(&mut self, top: String, list: Vec<String>, unindex: Option<Vec<Item>>, kept: &'static str) {
        worker::spawn(
            self.tx.clone(),
            self.waker.clone(),
            Box::new(move || {
                let mut failed = Vec::new();
                let left = stage::remove_files(&top, &list, &mut failed, &|abs, _| Ok(actions::trash(abs)), false);
                let went: Vec<String> =
                    list.into_iter().filter(|n| !left.contains(n) && !failed.iter().any(|(f, _)| f == n)).collect();
                let error = unindexed(&top, unindex.as_deref(), &went);
                Box::new(move |p: &mut Pane| p.trashed(top, left, failed, error, unindex, kept))
            }),
        );
    }

    fn trashed(
        &mut self,
        top: String,
        left: Vec<String>,
        failed: stage::Failed,
        error: Option<String>,
        unindex: Option<Vec<Item>>,
        kept: &'static str,
    ) {
        if let Some(error) = error {
            self.flash(error, true);
        }
        self.schedule();
        if left.is_empty() {
            if !failed.is_empty() {
                self.flash(format!("Could not {}", stage::cannot_delete(&failed)), true);
            }
            return;
        }
        // The refusals said in the question, which would hide a flash.
        let name = if left.len() == 1 { left[0].clone() } else { files(left.len()) };
        let mut note = "This cannot be undone.".to_string();
        if !kept.is_empty() {
            note = format!("{note} {kept}");
        }
        if !failed.is_empty() {
            note = format!("{note} Could not {}.", stage::cannot_delete(&failed));
        }
        let q = Confirm { note: opt(note), ..card("No trash: delete for good?", name, "Delete") };
        self.confirm(q, Yes::DeleteForGood { top, left, unindex });
    }

    /// Checked again by remove_files: a folder may have been made a link while the question was open.
    pub(super) fn delete_for_good(&mut self, top: String, left: Vec<String>, unindex: Option<Vec<Item>>) {
        worker::spawn(
            self.tx.clone(),
            self.waker.clone(),
            Box::new(move || {
                let mut lost = Vec::new();
                let kept = stage::remove_files(&top, &left, &mut lost, &stage::for_good, false);
                let went: Vec<String> =
                    left.into_iter().filter(|n| !kept.contains(n) && !lost.iter().any(|(f, _)| f == n)).collect();
                let error = unindexed(&top, unindex.as_deref(), &went);
                Box::new(move |p: &mut Pane| {
                    if let Some(error) = error {
                        p.flash(error, true);
                    }
                    if !lost.is_empty() {
                        p.flash(format!("Could not {}", stage::cannot_delete(&lost)), true);
                    }
                    p.schedule();
                })
            }),
        );
    }

    /// Changes section: worktree changes restored from the index, untracked files to the trash, as those added with
    /// git add -N (their entry out of the index once gone: restored, they would be emptied). Staged section: index
    /// and worktree back to HEAD, the files added since (all of them before the first commit) out of the index and to
    /// the trash.
    pub(in crate::scm) fn discard(&mut self, items: Vec<Item>, sec: &str) {
        if items.is_empty() {
            return;
        }
        if sec == "conflicts" {
            return self.flash("Merge conflict: edit the file and stage it, or abort the merge", true);
        }
        // The repository the files were listed in, even if another is loaded meanwhile.
        let (what, top) = (if items.len() == 1 { items[0].path.clone() } else { files(items.len()) }, self.top());
        if sec == "staged" {
            let has_head = self.status_of().is_some_and(|s| s.has_head);
            let (added, others): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| !has_head || i.code == 'A');
            let where_ = if added.is_empty() {
                String::new()
            } else {
                format!("moves {} added since HEAD to the trash", files(added.len()))
            };
            let note = if others.is_empty() { "" } else { "The changes cannot be recovered." };
            let q = Confirm {
                where_: opt(where_),
                note: opt(note.into()),
                ..card("Discard all changes?", format!("{what}, staged and not"), "Discard")
            };
            return self.confirm(q, Yes::DiscardStaged { top, others, added });
        }
        let (untracked, tracked): (Vec<Item>, Vec<Item>) =
            items.into_iter().partition(|i| i.untracked || i.code == 'A');
        let mut q = if untracked.is_empty() {
            card("Discard the changes?", what, "Discard")
        } else {
            let name = if untracked.len() == 1 { untracked[0].path.clone() } else { files(untracked.len()) };
            let where_ = if tracked.is_empty() {
                String::new()
            } else {
                format!("and discard the changes in {}", files(tracked.len()))
            };
            Confirm {
                where_: opt(where_),
                ..card(format!("Move {} not tracked to the trash?", files(untracked.len())), name, "Discard")
            }
        };
        q.note = opt(if tracked.is_empty() { "" } else { "The changes cannot be recovered." }.into());
        self.confirm(q, Yes::Discard { top, tracked, untracked });
    }

    pub(super) fn discard_staged(&mut self, top: String, others: Vec<Item>, added: Vec<Item>) {
        // Out of the index once gone only: one refused, or kept when not deleted for good, keeps its staged content.
        // One deleted since it was added (AD) is gone already. Not through op: once deleted, the index follows even
        // with another command running, in the repository the files were listed in.
        let rest = move |p: &mut Pane, top: String, added: Vec<Item>| {
            if added.is_empty() {
                return;
            }
            let list = added.iter().map(|i| i.path.clone()).collect();
            p.trash_files(top, list, Some(added), "Kept, they stay staged.");
        };
        if others.is_empty() {
            return rest(self, top, added);
        }
        let opts = GitOpts { top: Some(top.clone()), input: Some(state::nul(&others)), ..GitOpts::default() };
        let mut args = vec!["restore", "--staged", "--worktree", "--source=HEAD"];
        args.extend(FROM_FILE);
        self.op("Discarding", strings(&args), opts, false, move |p, res| {
            if res.is_some() {
                rest(p, top, added);
            }
        });
    }

    pub(super) fn discard_changes(&mut self, top: String, tracked: Vec<Item>, untracked: Vec<Item>) {
        let rest = move |p: &mut Pane, top: String| {
            if untracked.is_empty() {
                p.schedule();
            } else {
                let ita: Vec<Item> = untracked.iter().filter(|i| !i.untracked).cloned().collect();
                let list = untracked.into_iter().map(|i| i.path).collect();
                p.trash_files(top, list, (!ita.is_empty()).then_some(ita), "");
            }
        };
        if tracked.is_empty() {
            return rest(self, top);
        }
        let opts = GitOpts { top: Some(top.clone()), input: Some(state::nul(&tracked)), ..GitOpts::default() };
        let mut args = vec!["restore", "--worktree"];
        args.extend(FROM_FILE);
        // The untracked files stay when the tracked ones could not be discarded.
        self.op("Discarding", strings(&args), opts, false, move |p, res| {
            if res.is_some() {
                rest(p, top);
            }
        });
    }
}
