//! Pull requests (gh): the list, loaded on entering the view and on r, never polled; a pull request shown, opened in
//! the browser or checked out. gh runs on a thread of its own, so that a slow network never holds up git's answers.

use crate::core::gh::{self, Pull};
use crate::core::{actions, worker};
use crate::scm::state;
use crate::scm::{Pane, Then};

impl Pane {
    /// gh on a thread of its own, then its answer applied to the pane.
    fn gh_job<T: Send + 'static>(
        &self,
        f: impl FnOnce(&str) -> T + Send + 'static,
        then: impl FnOnce(&mut Pane, T) + Send + 'static,
    ) {
        let top = self.top();
        let job = Box::new(move || {
            let v = f(&top);
            Box::new(move |p: &mut Pane| then(p, v)) as Then
        });
        worker::spawn(self.tx.clone(), self.waker.clone(), job);
    }

    /// The list again: the one of the last opening shown meanwhile.
    pub(in crate::scm) fn load_pulls(&mut self) {
        if self.repo().is_none() || self.s.pulls_loading {
            return;
        }
        if self.s.pulls.is_none() {
            self.s.pulls = gh::cached(&self.top()).map(Ok);
        }
        self.s.pulls_loading = true;
        self.gh_job(gh::list, |p, res| {
            p.s.pulls_loading = false;
            p.s.pulls = Some(res);
            p.draw();
        });
    }

    /// The pull request in the diff beside the list: its header, description and changes.
    pub(in crate::scm) fn show_pull(&mut self, key: &str, title: &str, number: u64) {
        state::show_diff(&mut self.s, key, title, None, "");
        self.draw();
        let key = key.to_string();
        self.gh_job(
            move |top| gh::show(top, number),
            move |p, text| {
                let text = text.unwrap_or_else(|| "Cannot read the pull request from gh\n".into());
                state::show_diff_done(&mut p.s, &key, crate::diff::parse(&text));
                p.draw();
            },
        );
    }

    pub(in crate::scm) fn open_pull(&mut self, pull: &Pull) {
        let msg = actions::web(&pull.url, &format!("#{}", pull.number));
        self.flash(msg, false);
    }

    /// gh pr checkout: the pull request's branch, made or updated locally. Refused, as a fetch, when the repository
    /// sets network settings itself; the git it runs gets the hardened environment of the commands that write
    /// (the repository's ssh command, proxy, askpass and credential helpers replaced), and is refused when the
    /// repository sets its own upload-pack, which only a command-line option overrides.
    pub(in crate::scm) fn checkout_pull(&mut self, pull: Pull) {
        if let Some(busy) = &self.s.busy {
            return self.flash(format!("Busy: {busy}"), true);
        }
        self.query(state::repo_network_keys, move |p, keys| {
            if let Some(key) = keys.first() {
                return p.flash(format!("Checkout refused: the repository sets {key}; use the terminal"), true);
            }
            // Again: an op may have started while the question waited on the load worker.
            if let Some(busy) = &p.s.busy {
                return p.flash(format!("Busy: {busy}"), true);
            }
            let label = format!("Checking out #{}", pull.number);
            p.s.busy = Some(label);
            p.draw();
            let n = pull.number.to_string();
            p.gh_job(
                move |top| {
                    let (env, pack) = state::safe_env(top);
                    if !pack.fetch.is_empty() {
                        return Err("the repository sets its own upload-pack; use the terminal".into());
                    }
                    gh::run_in(top, &["pr", "checkout", &n], env, crate::scm::context::NET_TIMEOUT)
                },
                move |p, res| {
                    p.s.busy = None;
                    match res {
                        Ok(_) => p.flash(format!("Checked out #{} ({})", pull.number, pull.branch), false),
                        Err(e) => p.flash(format!("Checkout failed: {e}"), true),
                    }
                    p.schedule();
                },
            );
        });
    }
}
