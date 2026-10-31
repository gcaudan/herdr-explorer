//! What the explorer watches: the repository and its git status, the refresh when files change, and the terminal
//! it follows (its folder, the files the panes ask for).

use super::{Explorer, Then, big, mtime_ms, rel_of, rel_under};
use crate::core::actions;
use crate::core::exec::{self, Opts};
use crate::core::paths;
use crate::core::screen::{self};
use crate::json::{self, Value};
use crate::sys::Watcher;
use std::time::{Duration, Instant};

/// A repository in a folder of the root: its last status (None until read or when git failed), whether it changed
/// since, and the watch on its index and HEAD.
pub(super) struct Repo {
    name: String,
    status: Option<String>,
    dirty: bool,
    _watcher: Option<Watcher>,
}

/// The folders of root holding a repository of their own (a .git in them), each with its git folder, by name.
fn repos_in(root: &str) -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir(root) else { return Vec::new() };
    let mut names: Vec<String> = dir.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    names.sort_by(|a, b| super::tree::name_order(a, b));
    names
        .into_iter()
        .filter_map(|name| {
            let dir = paths::join(&[root, &name]);
            if name == ".git" || !std::fs::metadata(&dir).is_ok_and(|m| m.is_dir()) {
                return None;
            }
            std::fs::symlink_metadata(paths::join(&[&dir, ".git"])).ok()?;
            // Its top folder: a .git deeper in a repository is not its own.
            let res = git_run(&dir, &["rev-parse", "--absolute-git-dir", "--show-prefix"])?;
            let mut lines = res.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
            let gitdir = lines.next().filter(|g| !g.is_empty())?.to_string();
            lines.next().unwrap_or("").is_empty().then_some((name, gitdir))
        })
        .collect()
}

impl Explorer {
    /// Also called when a .git folder appears (git init, clone, archive extracted) in a folder that had none; then
    /// what follows (`after`).
    pub(super) fn init_git(&mut self, after: AfterGit) {
        if self.tree.git.top.is_some() || self.probing {
            return self.after_git(after);
        }
        self.probing = true;
        let (root, wide) = (self.root.clone(), big(&self.root));
        // A look that failed (a panic) frees the next one too.
        let failed: Then = Box::new(move |ex: &mut Explorer| {
            ex.probing = false;
            ex.after_git(after);
        });
        self.git_worker.send_or(
            Box::new(move || {
                let res = git_run(&root, &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--show-prefix"]);
                // Outside a repository, those of its folders; not in $HOME and above, with too many folders to look in.
                let repos = if res.is_none() && !wide { repos_in(&root) } else { Vec::new() };
                Box::new(move |ex: &mut Explorer| {
                    ex.probing = false;
                    if ex.stopped {
                        return ex.after_git(after);
                    }
                    // Another root meanwhile (Set as root), whose look was skipped while this one ran: its own.
                    if root != ex.root {
                        return ex.init_git(after);
                    }
                    match res {
                        Some(res) => ex.found_git(&res),
                        None => ex.found_repos(repos),
                    }
                    ex.after_git(after);
                })
            }),
            failed,
        );
    }

    fn found_git(&mut self, res: &str) {
        let mut parts = res.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
        let (top, gitdir, prefix) = (parts.next().unwrap_or(""), parts.next(), parts.next().unwrap_or(""));
        self.tree.git.top = Some(top.to_string());
        self.tree.git.prefix = prefix.strip_suffix('/').unwrap_or(prefix).to_string();
        self.tree.git.repos.clear();
        self.repos.clear();
        // No git folder in the answer: none watched.
        self.git_watcher = gitdir.and_then(|gitdir| self.watch_git(gitdir, None));
    }

    /// The repositories of the root's folders, found again: the statuses of those already known kept until read
    /// again, their git folders watched anew (a folder replaced by a clone of the same name).
    fn found_repos(&mut self, list: Vec<(String, String)>) {
        let mut old = std::mem::take(&mut self.repos);
        for (name, gitdir) in list {
            let status = old.iter().position(|r| r.name == name).and_then(|i| old.swap_remove(i).status);
            let watcher = self.watch_git(&gitdir, Some(name.clone()));
            self.repos.push(Repo { name, status, dirty: true, _watcher: watcher });
        }
        self.tree.git.repos = self.repos.iter().map(|r| r.name.clone()).collect();
        // Those gone no longer marked.
        if !old.is_empty() {
            self.apply_repos();
        }
    }

    /// The statuses of the repositories of the root's folders, to the tree.
    fn apply_repos(&mut self) {
        let list: Vec<(&str, &str)> =
            self.repos.iter().filter_map(|r| Some((r.name.as_str(), r.status.as_deref()?))).collect();
        self.tree.apply_statuses(&list);
    }

    /// The repository changes without touching the working tree (commit, stage, branch switch): its status read
    /// again; repo: the folder of the root it is in, None for the root's own.
    fn watch_git(&self, gitdir: &str, repo: Option<String>) -> Option<Watcher> {
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        Watcher::new(gitdir, false, move |f| {
            if matches!(f.as_deref(), Some("index" | "HEAD")) {
                let repo = repo.clone();
                let _ = tx.send(Box::new(move |ex: &mut Explorer| match repo {
                    Some(name) => ex.schedule_repo(&name),
                    None => ex.schedule(),
                }));
                waker.wake();
            }
        })
        .ok()
    }

    fn after_git(&mut self, after: AfterGit) {
        match after {
            AfterGit::Refresh => self.refresh(),
            AfterGit::ScheduleIfTop if self.tree.git.on() => self.arm(),
            AfterGit::ScheduleIfTop => {}
        }
    }

    /// A refresh, every repository of the root's folders read again.
    pub(super) fn schedule(&mut self) {
        self.repos.iter_mut().for_each(|r| r.dirty = true);
        self.arm();
    }

    /// A refresh, only the repository of that folder of the root read again.
    fn schedule_repo(&mut self, name: &str) {
        self.repos.iter_mut().filter(|r| r.name == name).for_each(|r| r.dirty = true);
        self.arm();
    }

    /// At most one refresh every 250 ms: a file written continuously (a log) must not postpone it forever.
    fn arm(&mut self) {
        if self.refresh_at.is_none() && !self.stopped {
            self.refresh_at = Some(Instant::now() + Duration::from_millis(250));
        }
    }

    pub(super) fn refresh(&mut self) {
        if self.stopped {
            return;
        }
        if self.busy {
            self.again = true;
            return;
        }
        self.busy = true;
        self.tree.stale();
        if self.tree.git.top.is_none() {
            return self.refresh_repos();
        }
        let root = self.root.clone();
        self.git_worker.send_or(
            Box::new(move || {
                let args =
                    ["status", "--porcelain=v1", "-z", "--ignored=matching", "--ignore-submodules=all", "--", "."];
                let status = git_run(&root, &args);
                Box::new(move |ex: &mut Explorer| ex.refreshed(status.filter(|_| root == ex.root)))
            }),
            Box::new(|ex: &mut Explorer| ex.refreshed(None)),
        );
    }

    /// The statuses of the repositories of the root's folders that changed, read again; outside any, the tree only.
    fn refresh_repos(&mut self) {
        let mut todo = Vec::new();
        for r in self.repos.iter_mut().filter(|r| r.dirty) {
            r.dirty = false;
            todo.push((r.name.clone(), paths::join(&[&self.root, &r.name])));
        }
        if todo.is_empty() {
            return self.refreshed(None);
        }
        let root = self.root.clone();
        self.git_worker.send_or(
            Box::new(move || {
                let args =
                    ["status", "--porcelain=v1", "-z", "--ignored=matching", "--ignore-submodules=all", "--", "."];
                let read: Vec<(String, Option<String>)> =
                    todo.into_iter().map(|(name, dir)| (name, git_run(&dir, &args))).collect();
                Box::new(move |ex: &mut Explorer| {
                    if root == ex.root {
                        for (name, status) in read {
                            if let Some(r) = ex.repos.iter_mut().find(|r| r.name == name) {
                                r.status = status;
                            }
                        }
                        ex.apply_repos();
                    }
                    ex.refreshed(None);
                })
            }),
            Box::new(|ex: &mut Explorer| ex.refreshed(None)),
        );
    }

    fn refreshed(&mut self, status: Option<String>) {
        if let Some(status) = status {
            self.tree.apply_status(&status);
        }
        self.build_rows();
        self.render();
        self.busy = false;
        if self.again {
            self.again = false;
            self.arm();
        }
    }

    /// The root as the folder herdr gives the new spaces, tabs and panes made from this one: the launcher's.
    pub(super) fn announce(&self) {
        screen::send(&crate::obj! { "cwd" => self.root.clone() });
    }

    /// Recursive, except on $HOME, one of its parents or the filesystem root: on Linux, one watch per folder of the
    /// tree, which would take seconds and could use up the system's limit. Top level only there, and also when a
    /// folder of the tree cannot be read (the recursive watch then fails); the 30 s scan covers the rest.
    pub(super) fn watch_root(&mut self) {
        self.root_watcher = None;
        if self.stopped {
            return;
        }
        let tries: &[bool] = if big(&self.root) { &[false] } else { &[true, false] };
        for &recursive in tries {
            let (tx, waker) = (self.tx.clone(), self.waker.clone());
            let watched = Watcher::new(&self.root, recursive, move |f| {
                let _ = tx.send(Box::new(move |ex: &mut Explorer| ex.changed(f)));
                waker.wake();
            });
            if let Ok(w) = watched {
                self.root_watcher = Some(w);
                return;
            }
        }
    }

    /// A change under the root.
    fn changed(&mut self, f: Option<String>) {
        let rel = f.map_or_else(String::new, |f| if paths::SEP == '/' { f } else { f.replace(paths::SEP, "/") });
        if rel == ".git" || rel.starts_with(".git/") {
            if self.tree.git.top.is_none() {
                self.probe_later();
            }
            return;
        }
        // Outside a repository, a .git in one of its folders: a repository of its own there, once git finds it; a
        // known one's .git or folder itself made, removed or moved, looked for again too.
        let (name, rest) = rel.split_once('/').unwrap_or((&rel, ""));
        if self.tree.git.top.is_none() && !name.is_empty() {
            let known = self.tree.git.repos.iter().any(|r| r == name);
            if rest == ".git" || rest.starts_with(".git/") {
                if !known || rest == ".git" {
                    self.probe_later();
                }
                return;
            }
            if rest.is_empty() && (known || std::fs::symlink_metadata(paths::join(&[&self.root, name, ".git"])).is_ok())
            {
                self.probe_later();
            }
        }
        if rel.contains("/.git/") || self.tree.hidden(&rel, &self.state.expanded) {
            return;
        }
        match self.tree.git.repo_of(&rel) {
            Some(name) if !name.is_empty() => {
                let name = name.to_string();
                self.schedule_repo(&name);
            }
            _ if rel.is_empty() => self.schedule(),
            // The root's repository; outside any, the tree only.
            _ => self.arm(),
        }
    }

    /// git looked for again 500 ms later, and again after the events seen meanwhile.
    fn probe_later(&mut self) {
        let at = Instant::now() + Duration::from_millis(500);
        if self.git_at.is_none() {
            self.git_at = Some(at);
        } else {
            self.git_more = Some(at);
        }
    }

    /// The folder of the terminal next to the explorer (that of its running program when herdr knows it), revealed
    /// in the tree when it changes; outside the root, the root changes. The setting is read each time, so that a
    /// change made in another explorer applies here too.
    pub(super) fn follow(&mut self) {
        let follow = exec::read_settings().get("follow").cloned().unwrap_or(Value::Null);
        let on = follow.truthy();
        self.settings.set("follow", follow);
        // One look at a time: a slow herdr would otherwise fill the worker's queue, the preview and the copy waiting
        // behind.
        if !on || self.pane.is_empty() || self.follow_busy {
            return;
        }
        self.follow_busy = true;
        let pane = self.pane.clone();
        // Its tab, needed once the hook tells the tab shown.
        let learn = self.tab.is_none() && self.shown.is_some();
        self.herdr.send(Box::new(move || {
            let get = |id: &str| {
                let res = exec::result(exec::run(exec::herdr(), &["pane", "get", id], &Opts::default()).as_deref());
                res.get("pane").filter(|p| p.truthy()).cloned()
            };
            let tab = || get(&pane).and_then(|p| p.get("tab_id").filter(|t| t.truthy()).map(crate::json::text));
            let tab = if learn { std::panic::catch_unwind(tab).ok().flatten() } else { None };
            let look = || {
                let (target, _) = actions::terminal_of(&pane);
                target.and_then(|t| get(&t)).and_then(|p| {
                    ["foreground_cwd", "cwd"]
                        .iter()
                        .find_map(|k| p.get(k).filter(|v| v.truthy()).map(crate::json::text))
                })
            };
            let cwd = std::panic::catch_unwind(look).ok().flatten();
            Box::new(move |ex: &mut Explorer| {
                ex.follow_busy = false;
                ex.tab = ex.tab.take().or(tab);
                ex.followed(cwd)
            })
        }));
    }

    /// The explorer's tab asked of herdr again; a look at once if it is the one shown.
    pub(super) fn learn_tab(&mut self) {
        let pane = self.pane.clone();
        if pane.is_empty() {
            return;
        }
        self.herdr.send(Box::new(move || {
            let res = exec::result(exec::run(exec::herdr(), &["pane", "get", &pane], &Opts::default()).as_deref());
            let tab = res.get("pane").and_then(|p| p.get("tab_id")).filter(|t| t.truthy()).map(crate::json::text);
            Box::new(move |ex: &mut Explorer| {
                if tab.is_some() && tab != ex.tab {
                    ex.tab = tab;
                    if !ex.asleep() {
                        ex.follow_at.get_or_insert(Instant::now());
                    }
                }
            })
        }));
    }

    /// Follow asleep: herdr shows another tab than the explorer's. Never before the first look once the hook told the
    /// tab shown in this session, which learns the explorer's tab and notes the terminal's folder.
    pub(super) fn asleep(&self) -> bool {
        matches!((&self.tab, &self.shown), (Some(t), Some(s)) if t != s)
    }

    fn followed(&mut self, cwd: Option<String>) {
        let s = &self.state;
        // Retried once the menu, the settings, the filter or the prompt is closed.
        let busy = s.filter.is_some()
            || s.menu.is_some()
            || s.settings.is_some()
            || s.help.is_some()
            || s.prompt.is_some()
            || s.confirm.is_some()
            || s.choice.is_some();
        let Some(cwd) = cwd.filter(|c| !self.stopped && Some(c) != self.last_cwd.as_ref() && !busy) else { return };
        let first = self.last_cwd.is_none() && !self.forced;
        self.last_cwd = Some(cwd.clone());
        self.forced = false;
        // Outside the root: back to the space folder when it holds the terminal folder, else the terminal folder
        // itself. Never $HOME, one of its parents or the filesystem root, where the tree would be too big to follow.
        // Not at the first check: the terminal has not moved, and the root may be the one Set as root chose before a
        // herdr restart.
        if rel_under(&cwd, &self.root).is_none() {
            if first {
                return;
            }
            if rel_under(&cwd, &self.home.clone()).is_some() {
                let home = self.home.clone();
                self.set_root(&home);
            } else if big(&cwd) {
                return;
            } else {
                self.set_root(&cwd);
            }
        }
        let Some(rel) = rel_under(&cwd, &self.root).filter(|r| !r.is_empty()) else { return };
        self.expand(&rel);
        self.reveal_rel(&rel);
        self.render();
    }

    /// The tab's request file changed: a file to reveal.
    pub(super) fn request(&mut self) {
        let Some((w, seen)) = &mut self.requests else { return };
        let file = w.file().to_string();
        let t = mtime_ms(&file);
        if self.stopped || t == 0.0 || t == *seen {
            return;
        }
        *seen = t;
        let req = exec::read_text(&file).ok().and_then(|t| json::parse(&t).ok()).unwrap_or(Value::Null);
        if req.get("reveal") != Some(&Value::Bool(true)) {
            return;
        }
        let Some(target) = req.get("file").and_then(Value::as_str) else { return };
        let rel = paths::relative(&self.root, target);
        if rel.is_empty() || paths::leaves(&rel) {
            return;
        }
        self.reveal_rel(&rel_of(&self.root, target));
        self.render();
    }
}

/// What follows a look for the repository.
#[derive(Clone, Copy)]
pub(super) enum AfterGit {
    Refresh,
    /// A .git folder appeared: a refresh once the repository is found.
    ScheduleIfTop,
}

/// git in the root, the repository's config read again before each call, as it may change while the explorer runs.
fn git_run(root: &str, args: &[&str]) -> Option<String> {
    exec::run(exec::git(), args, &Opts { cwd: Some(root.into()), env: Some(exec::git_env(root)), ..Default::default() })
}
