//! The source control pane (src/scm.js): the repository's changes, history, stashes and refs, in a review tab opened
//! from the explorer. This file holds the pane, its loop and timers, the refresh, git run for an operation, and the
//! selection that the diff or the preview follows; ops.rs, render.rs and input.rs hold what the JS's scm/ops.js,
//! render.js and input.js did.
//!
//! As in the other panes, git runs on worker threads (the loads on one, the operations on another), herdr on a third;
//! their results come back to the pane's loop, the JS's timers are deadlines of that loop.

use super::context::{self, Hist, Repo, RepoState, Row, State};
use super::parse::{self, Branch, Commit, Item, Stash};
use super::state::{self, GitOpts, LoadAsk, Loaded, Ongoing};
use crate::core::exec::{self, Exec};
use crate::core::screen::{self, Input, Screen};
use crate::core::ui::{self, Event, Field, HelpState, Menu, Scrollbar};
use crate::core::watch::Watch;
use crate::core::{actions, icons, js, paths, worker};
use crate::json::Value;
use crate::sys::{self, Watcher};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) type Then = worker::Then<Pane>;
type Worker = worker::Worker<Pane>;

/// What a click on the frame does: the header's branch, buttons and tabs, List and Tree, the commit box, a key of
/// the status line, the diff's ⚙, the buttons of the selected row.
#[derive(Clone, Debug)]
pub(super) enum Act {
    Picker,
    Push,
    Pull,
    Fetch,
    Actions,
    View(&'static str),
    List,
    Tree,
    AllFiles,
    Edit,
    Commit,
    Generate,
    Amend,
    Abort,
    Key(String),
    DiffView,
    Stage(Vec<Item>),
    Unstage(Vec<Item>),
    Discard(Vec<Item>, &'static str),
}

/// A clickable area of the last frame: its line and columns (from 0, x1 excluded).
pub(super) struct Hit {
    pub y: i64,
    pub x0: i64,
    pub x1: i64,
    pub act: Act,
}

/// The last frame drawn: its clickable areas, the rows of the current view, the list's first row, height and width,
/// and the column of the header's branch.
#[derive(Default)]
pub(super) struct Frame {
    pub hits: Vec<Hit>,
    pub rows: Vec<Row>,
    pub list_y: i64,
    pub list_h: i64,
    pub list_w: i64,
    pub pill_x: i64,
}

/// What an item of the menus does: the actions of a branch, a commit and the repository (m), the diff's options.
#[derive(Clone, Debug)]
pub(super) enum Op {
    Checkout(Branch),
    Integrate(Branch, &'static str),
    DeleteBranch(Branch),
    NewTag(Option<Commit>),
    CherryPick(Commit),
    ResetHard(Option<Commit>),
    Picker(&'static str),
    Fetch,
    Pull,
    Push,
    ForcePush,
    UndoCommit,
    Layout,
    Wrap,
}

/// What y does to a question of the card (ui::confirm).
pub(super) enum Yes {
    Init,
    DeleteForGood { top: String, left: Vec<String>, unindex: Option<Vec<Item>> },
    DiscardStaged { top: String, others: Vec<Item>, added: Vec<Item> },
    Discard { top: String, tracked: Vec<Item>, untracked: Vec<Item> },
    StageAndCommit(String),
    Abort(Ongoing),
    DeleteBranch(Branch),
    DeleteAnyway(Branch),
    Stash(Stash, &'static str),
    Integrate(Branch, &'static str),
    CherryPick(Commit, bool),
    ForcePush(String),
    Undo(String),
    Reset(Option<Commit>),
}

/// What a name typed in place does once given: a branch, a stash's message, a tag, an annotated tag's message.
pub(super) enum Done {
    Branch,
    Stash,
    Tag { at: String, target: String },
    TagMessage { at: String, target: String, name: String },
}

/// A name typed in place, on its row (the picker's first one, or above the list): its kind, field, what is dimmed on
/// its right, the status line's keys, and what Enter does (alt+Enter or ctrl+O too, with alt).
pub(super) struct Inline {
    pub kind: &'static str,
    pub field: Field,
    pub right: String,
    pub keys: Vec<(String, String)>,
    pub done: Done,
}

pub struct Pane {
    pub(super) s: State,
    pub(super) root: String,
    /// A review (context::review): the tab reviewed, "" for none; split: columns from which the diff opens on the
    /// right of the list.
    pub(super) review: String,
    pub(super) split: f64,
    pub(super) pane: String,
    /// The Wrap lines setting, shared with the preview and the Changes pane.
    pub(super) wrap: bool,
    pub(super) theme: Option<icons::Loaded>,
    pub(super) frame: Frame,
    pub(super) confirm: Option<(ui::Confirm, Yes)>,
    pub(super) inline: Option<Inline>,
    /// The menu while open; header: opened from the header's Actions; view: the diff's options, shown again after a
    /// choice that keeps it.
    pub(super) menu: Option<Menu<Op>>,
    pub(super) menu_header: bool,
    pub(super) menu_view: bool,
    pub(super) help: Option<HelpState>,
    /// The rows just staged or unstaged, briefly highlighted.
    pub(super) pulse: Option<HashSet<String>>,
    pulse_at: Option<Instant>,
    flash_at: Option<Instant>,
    pub(super) last_click: Option<(i64, Instant)>,
    pub(super) list_bar: Scrollbar,
    pub(super) diff_bar: Scrollbar,
    /// The refresh: due, running, asked again while running; poll_at: the next look at the working tree (5 s).
    refresh_at: Option<Instant>,
    loading: bool,
    again: bool,
    poll_at: Instant,
    /// The file or folder selected in the explorer, selected here once the changes are loaded.
    reveal_file: Option<String>,
    follow_at: Option<Instant>,
    /// A commit selected in a comparison below SPLIT columns: shown in the preview shortly after.
    pub(super) compare_at: Option<(Instant, Row)>,
    /// The preview opens by itself once, with the first selection; preview_key: the row it was asked for last.
    pub(super) previewed: bool,
    pub(super) preview_key: Option<Option<String>>,
    /// The pane's tab once looked up (shareCompare).
    pub(super) my_tab: Option<Option<String>>,
    sharer: Option<Arc<Mutex<actions::Sharer>>>,
    git_watchers: Vec<Watcher>,
    settings_watch: Option<Watch>,
    pub(super) screen: Screen,
    pub(super) tx: Sender<Then>,
    rx: Receiver<Then>,
    pub(super) waker: sys::Waker,
    pub(super) herdr: Worker,
    pub(super) git: Worker,
    pub(super) ops: Worker,
}

pub fn run(args: &[String]) -> i32 {
    worker::quiet_panics();
    let mut p = match Pane::new(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("herdr-explorer: {e}");
            return 1;
        }
    };
    p.start();
    p.run()
}

/// The names in the git folder whose change means a commit, a staging, a branch switch or a fetch.
const GIT_NAMES: [&str; 9] = [
    "index",
    "HEAD",
    "ORIG_HEAD",
    "FETCH_HEAD",
    "MERGE_HEAD",
    "packed-refs",
    "stash",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
];

impl Pane {
    fn new(args: &[String]) -> std::io::Result<Pane> {
        let root = context::root(args);
        // the pane's folder for new spaces, tabs and panes made from it
        crate::launch::home(None);
        ui::init_theme();
        let settings = exec::read_settings();
        ui::use_glyphs(settings.get("glyphs").and_then(Value::as_str).unwrap_or(""));
        // the explorer's icon theme
        let theme = icons::load(settings.get("icons").unwrap_or(&Value::Null));
        let review = context::review();
        let pane = exec::env_var("HERDR_PANE_ID").unwrap_or_default();
        let screen = Screen::open(screen::Options { mouse: true, motion: true, focus: true })?;
        let waker = screen.waker();
        let (tx, rx) = channel();
        Ok(Pane {
            s: State::new(&settings),
            split: context::split(&review),
            sharer: (!review.is_empty()).then(|| Arc::new(Mutex::new(actions::Sharer::new(&pane, "reviewShare")))),
            review,
            root,
            pane,
            wrap: settings.get("wrap").is_some_and(Value::truthy),
            theme,
            frame: Frame::default(),
            confirm: None,
            inline: None,
            menu: None,
            menu_header: false,
            menu_view: false,
            help: None,
            pulse: None,
            pulse_at: None,
            flash_at: None,
            last_click: None,
            list_bar: Scrollbar::new(),
            diff_bar: Scrollbar::new(),
            refresh_at: None,
            loading: false,
            again: false,
            poll_at: Instant::now() + Duration::from_secs(5),
            reveal_file: exec::env_var("EXPLORER_FILE").filter(|f| !f.is_empty()),
            follow_at: None,
            compare_at: None,
            previewed: false,
            preview_key: None,
            my_tab: None,
            git_watchers: Vec::new(),
            settings_watch: exec::settings_file().map(|f| Watch::new(&f, Duration::from_millis(1000))),
            herdr: Worker::new(tx.clone(), waker.clone()),
            git: Worker::new(tx.clone(), waker.clone()),
            ops: Worker::new(tx.clone(), waker.clone()),
            screen,
            tx,
            rx,
            waker,
        })
    }

    /// What the JS does as its script loads, once the screen is set up.
    fn start(&mut self) {
        self.draw();
        self.refresh();
        // The review recorded for the tab reviewed, which leads back here while this pane runs (hook --focused), in a
        // tab named "(SC 2)", closed with the preview it holds at any exit.
        if !self.review.is_empty() {
            actions::hold_tab("review", &self.review, "SC");
        }
    }

    /// The loop: the screen's events, the workers' results and the timers, until the pane is closed.
    fn run(&mut self) -> i32 {
        loop {
            let timeout = self.next_deadline().map(|t| t.saturating_duration_since(Instant::now()));
            for input in self.screen.wait(timeout) {
                match input {
                    Input::Ui(e) => self.event(e),
                    Input::Resize => self.resized(),
                    Input::Theme => {
                        if let Some(d) = self.s.diff.as_mut() {
                            d.cache.clear();
                        }
                        self.draw();
                    }
                    Input::Wake => {}
                }
                self.results();
            }
            self.results();
            self.timers();
        }
    }

    fn results(&mut self) {
        while let Ok(then) = self.rx.try_recv() {
            then(self);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        let spin = self.s.busy.is_some().then(|| Instant::now() + Duration::from_millis(100));
        [
            self.refresh_at,
            Some(self.poll_at),
            self.flash_at,
            self.pulse_at,
            self.follow_at,
            self.compare_at.as_ref().map(|c| c.0),
            spin,
            self.list_bar.due(),
            self.diff_bar.due(),
            self.settings_watch.as_ref().map(Watch::due),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn timers(&mut self) {
        let now = Instant::now();
        if self.refresh_at.is_some_and(|t| t <= now) {
            self.refresh_at = None;
            self.refresh();
        }
        // The working tree is not watched: edits show up within 5 s.
        if self.poll_at <= now {
            self.poll_at = now + Duration::from_secs(5);
            self.schedule();
        }
        if self.flash_at.is_some_and(|t| t <= now) {
            self.flash_at = None;
            self.s.flash = None;
            self.draw();
        }
        if self.pulse_at.is_some_and(|t| t <= now) {
            self.pulse_at = None;
            self.pulse = None;
            self.draw();
        }
        if self.follow_at.is_some_and(|t| t <= now) {
            self.follow_at = None;
            self.follow_now();
        }
        if self.compare_at.as_ref().is_some_and(|c| c.0 <= now) {
            let (_, row) = self.compare_at.take().unwrap();
            let open = !self.previewed;
            self.preview_row(Some(&row), open, false);
            self.previewed = true;
        }
        // The spinner of the status line and of the header's button.
        if self.s.busy.is_some() {
            self.draw();
        }
        if let Some(top) = self.list_bar.tick(now) {
            self.s.top = top;
            self.draw();
        }
        if let Some(top) = self.diff_bar.tick(now) {
            if let Some(d) = self.s.diff.as_mut() {
                d.top = top;
            }
            self.draw();
        }
        // Wrap lines and the diff layout chosen in another pane (the preview, the Changes pane): shown at once.
        if self.settings_watch.as_mut().is_some_and(Watch::poll) {
            let wrap = exec::read_settings().get("wrap").is_some_and(Value::truthy);
            if wrap != self.wrap {
                self.wrap = wrap;
                if let Some(d) = self.s.diff.as_mut() {
                    d.left = 0;
                }
            }
            if self.s.diff.is_some() {
                self.draw();
            }
        }
    }

    // --- the pane

    /// The terminal's size: columns and rows, 80 and 24 when unknown.
    pub(super) fn size(&self) -> (i64, i64) {
        sys::size().map_or((80, 24), |(w, h)| (if w == 0 { 80 } else { w as i64 }, if h == 0 { 24 } else { h as i64 }))
    }

    /// The diff shows on the right of the list.
    pub(super) fn wide(&self) -> bool {
        self.size().0 as f64 >= self.split
    }

    pub(super) fn quit(&mut self) -> ! {
        self.screen.exit(0)
    }

    /// The repository's folder, the pane's before one is found: where git runs.
    pub(super) fn top(&self) -> String {
        self.s.repo().map_or_else(|| self.root.clone(), |r| r.top.clone())
    }

    pub(super) fn repo(&self) -> Option<&Repo> {
        self.s.repo()
    }

    pub(super) fn flash(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        let kind = if error { ui::Kind::Error } else { ui::message_kind(&text) };
        self.s.flash = Some((text, error));
        self.draw();
        self.flash_at = Some(Instant::now() + Duration::from_millis(ui::message_time(kind)));
    }

    /// A question in a card over the pane (ui::confirm), what y does.
    pub(super) fn confirm(&mut self, q: ui::Confirm, yes: Yes) {
        self.confirm = Some((ui::Confirm { focus: "no".into(), ..q }, yes));
        self.draw();
    }

    /// Rows just moved to the other section, highlighted for a moment where they land.
    pub(super) fn pulse(&mut self, items: &[Item], sec: &str) {
        self.pulse = Some(items.iter().map(|i| format!("{sec}:{}", i.path)).collect());
        self.pulse_at = Some(Instant::now() + Duration::from_millis(900));
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Key(k) => self.key(&k),
            Event::Mouse(m) => self.mouse(&m),
            Event::Focus(on) => {
                if on {
                    self.schedule();
                } else {
                    let pane = self.pane.clone();
                    self.herdr.send(Box::new(move || {
                        actions::unzoom(&pane);
                        Box::new(|_: &mut Pane| {})
                    }));
                }
            }
            Event::Paste { text, .. } => {
                let s = &mut self.s;
                let (f, picker) = match (&mut self.inline, &mut s.picker) {
                    (Some(it), _) => (Some(&mut it.field), false),
                    (None, Some(p)) => (Some(&mut p.field), true),
                    (None, None) if s.editing => (Some(&mut s.field), false),
                    _ => (None, false),
                };
                if let Some(f) = f {
                    f.insert(&text);
                    if picker && let Some(p) = s.picker.as_mut() {
                        p.typing = true;
                        state::filter_picker(p);
                    }
                    self.draw();
                }
            }
            Event::Reply(_) => {}
        }
    }

    /// Below SPLIT columns, a diff the selection opened by itself would hide the list.
    fn resized(&mut self) {
        if self.s.diff.as_ref().is_some_and(|d| d.auto) && !self.wide() {
            self.s.diff = None;
        }
        self.draw();
        self.follow();
        self.keep_share(0.0);
    }

    /// A review's share of its tab beside the preview (reviewShare setting, actions::Sharer).
    pub(super) fn keep_share(&mut self, change: f64) {
        let Some(sharer) = self.sharer.clone() else { return };
        self.herdr.send(Box::new(move || {
            sharer.lock().unwrap_or_else(|p| p.into_inner()).share(change);
            Box::new(|_: &mut Pane| {})
        }));
    }

    // --- the refresh

    /// At most one refresh every 250 ms, never two at once.
    pub(super) fn schedule(&mut self) {
        if self.refresh_at.is_none() {
            self.refresh_at = Some(Instant::now() + Duration::from_millis(250));
        }
    }

    pub(super) fn refresh(&mut self) {
        if self.loading {
            self.again = true;
            return;
        }
        self.loading = true;
        let ask = LoadAsk::of(&self.s, &self.root);
        self.git.send(Box::new(move || {
            let loaded = std::panic::catch_unwind(|| state::load(&ask)).ok();
            Box::new(move |p: &mut Pane| p.loaded(loaded))
        }));
    }

    fn loaded(&mut self, l: Option<Loaded>) {
        if let Some(l) = l {
            let found = l.found;
            state::apply(&mut self.s, l);
            if found {
                self.watch_git();
            }
        }
        self.loading = false;
        match self.reveal_file.take() {
            Some(f) if self.repo().is_some() => self.reveal(&f),
            f => {
                self.reveal_file = f;
                self.draw();
                self.follow();
            }
        }
        if self.again {
            self.again = false;
            self.schedule();
        }
    }

    /// The status loaded again, before what an operation does next (loadStatus): kept as it was when git fails.
    pub(super) fn reload_status(&mut self, then: impl FnOnce(&mut Pane) + Send + 'static) {
        let Some(r) = self.repo() else { return then(self) };
        let dirs = parse::Dirs { top: r.top.clone(), gitdir: r.gitdir.clone(), common: r.common.clone() };
        self.git.send(Box::new(move || {
            let repo = state::load_status(&dirs);
            Box::new(move |p: &mut Pane| {
                if let Some(repo) = repo {
                    p.s.repo = RepoState::Some(Box::new(repo));
                }
                then(p)
            })
        }));
    }

    /// Commits, staging, branch switches and fetches change files of the git folder, not the working tree.
    fn watch_git(&mut self) {
        if !self.git_watchers.is_empty() {
            return;
        }
        let Some(r) = self.repo() else { return };
        let (gitdir, common) = (r.gitdir.clone(), r.common.clone());
        let on = |names: bool| {
            let (tx, waker) = (self.tx.clone(), self.waker.clone());
            move |f: Option<String>| {
                let hit = match &f {
                    Some(f) if names => GIT_NAMES.contains(&f.as_str()),
                    Some(f) => !f.ends_with(".lock"),
                    None => !names,
                };
                if hit {
                    let _ = tx.send(Box::new(|p: &mut Pane| p.schedule()));
                    waker.wake();
                }
            }
        };
        let mut list: Vec<Watcher> = Watcher::new(&gitdir, false, on(true)).into_iter().collect();
        if common != gitdir {
            list.extend(Watcher::new(&common, false, on(true)));
        }
        list.extend(Watcher::new(&paths::join(&[&common, "refs"]), true, on(false)));
        self.git_watchers = list;
    }

    /// The file or folder selected in the explorer, selected here once the changes are loaded: the file, else the
    /// first changed file of the folder.
    fn reveal(&mut self, abs: &str) {
        let Some(repo) = self.repo() else { return };
        let it = match state::reveal_item(repo, abs) {
            Ok(it) => it,
            Err(name) => {
                self.draw();
                return self.flash(format!("No changes in {name}"), false);
            }
        };
        // Its section and folders unfolded.
        self.s.collapsed.remove(it.sec);
        let prefix = format!("{}:", it.sec);
        self.s.collapsed.retain(|k| !(k.starts_with(&prefix) && it.path.starts_with(&k[prefix.len()..])));
        // Drawn first, so that the list's height is known (none before the first load) and select scrolls to the row.
        self.draw();
        let key = format!("{}:{}", it.sec, it.path);
        let i = self.frame.rows.iter().position(|r| r.key == key).map_or(-1, |i| i as i64);
        self.select(i);
        self.draw();
    }

    // --- git

    /// git's answer to a question asked in the repository (f run with its folder), on the load worker.
    pub(super) fn query<T: Send + 'static>(
        &self,
        f: impl FnOnce(&str) -> T + Send + 'static,
        then: impl FnOnce(&mut Pane, T) + Send + 'static,
    ) {
        let top = self.top();
        self.git.send(Box::new(move || {
            let v = f(&top);
            Box::new(move |p: &mut Pane| then(p, v))
        }));
    }

    /// A git command that changes something: shown as running, then its error, if any, in the status line. then gets
    /// its result: None when busy or failed (failed and quiet: the result, its error not shown).
    pub(super) fn op(
        &mut self,
        label: &str,
        args: Vec<String>,
        opts: GitOpts,
        quiet: bool,
        then: impl FnOnce(&mut Pane, Option<Exec>) + Send + 'static,
    ) {
        if let Some(busy) = &self.s.busy {
            self.flash(format!("Busy: {busy}"), true);
            return then(self, None);
        }
        self.s.busy = Some(label.to_string());
        self.draw();
        let (top, label) = (opts.top.clone().unwrap_or_else(|| self.top()), label.to_string());
        self.ops.send(Box::new(move || {
            let res = std::panic::catch_unwind(|| {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                state::git(&top, &args, &GitOpts { safe: true, ..opts })
            })
            .unwrap_or(Exec { code: None, out: String::new(), err: String::new() });
            Box::new(move |p: &mut Pane| {
                p.s.busy = None;
                if res.code != Some(0) {
                    if !quiet {
                        p.flash(format!("{label} failed: {}", parse::error_of(&res)), true);
                    }
                    p.schedule();
                    return then(p, quiet.then_some(res));
                }
                p.schedule();
                then(p, Some(res))
            })
        }));
    }

    /// fetch, pull and push: no password prompt in the terminal, and a time limit. Refused when the repository sets
    /// network settings itself: the user runs them in the terminal after checking them.
    pub(super) fn net(&mut self, label: &str, args: Vec<String>, then: impl FnOnce(&mut Pane, Exec) + Send + 'static) {
        let label = label.to_string();
        self.query(state::repo_network_keys, move |p, keys| {
            if let Some(key) = keys.first() {
                let what = label.split(' ').next().unwrap_or("");
                return p.flash(format!("{what} refused: the repository sets {key}; use the terminal"), true);
            }
            if let Some(busy) = &p.s.busy {
                return p.flash(format!("Busy: {busy}"), true);
            }
            p.s.net_op = args.first().cloned();
            let opts = GitOpts {
                detached: true,
                timeout: Some(context::NET_TIMEOUT),
                env: state::net_env(),
                ..GitOpts::default()
            };
            p.op(&label, args, opts, false, move |p, res| {
                p.s.net_op = None;
                // On Windows, ssh may still have asked for a passphrase or a host key on the pane's console, over the
                // screen.
                p.screen.again();
                p.draw();
                if let Some(res) = res {
                    then(p, res);
                }
            });
        });
    }

    // --- the selection

    pub(super) fn select(&mut self, i: i64) {
        let len = self.frame.rows.len() as i64;
        self.s.cursor = i.min(len - 1).max(0);
        self.s.sel_key = self.frame.rows.get(self.s.cursor as usize).map(|r| r.key.clone());
        if self.s.cursor < self.s.top {
            self.s.top = self.s.cursor;
        }
        if self.s.cursor >= self.s.top + self.frame.list_h {
            self.s.top = self.s.cursor - self.frame.list_h + 1;
        }
        self.follow();
    }

    /// From SPLIT columns, the diff shows the selected file, commit, stash or tag (shortly after, when moving fast). A
    /// branch other than the base shows its comparison with the base.
    pub(super) fn follow(&mut self) {
        self.follow_at = Some(Instant::now() + Duration::from_millis(120));
    }

    fn follow_now(&mut self) {
        if self.repo().is_none() {
            return;
        }
        let row = self.frame.rows.get(self.s.cursor as usize).cloned();
        // Narrow: the preview opened below the pane follows the selected file. Asked again only for another row: each
        // request reloads the preview, which would drop the lines selected there at each refresh.
        if !self.wide() {
            // its comparison open, which shows its commits there
            if let (Some(d), Some(row)) = (&self.s.diff, &row)
                && d.key == row.key
            {
                return;
            }
            let key = row.as_ref().map(|r| r.key.clone());
            if !self.previewed || self.preview_key.as_ref() != Some(&key) {
                let open = !self.previewed;
                self.preview_row(row.as_ref(), open, false);
            }
            self.previewed = true;
            return;
        }
        if state::diffable(&self.s, row.as_ref()) {
            let row = row.unwrap();
            // show_diff sets the diff before it waits for git.
            if !state::showing(&self.s, &row) {
                self.open_row(Some(&row), true);
                if let Some(d) = self.s.diff.as_mut().filter(|d| d.key == row.key) {
                    d.auto = true;
                }
            }
        } else if self.s.diff.is_some() {
            self.close_diff();
        }
    }

    /// A commit, stash or tag row as diff::show_args takes it, with a title: None for another row. A commit of a
    /// file's history: its changes to that file only.
    pub(super) fn show_of(&self, row: Option<&Row>) -> Option<(Value, String)> {
        let row = row?;
        match row.kind {
            "commit" => {
                let c = &row.commit.as_ref()?.commit;
                let names = if self.s.hist_file.is_some() { self.s.hist_names.clone() } else { Vec::new() };
                let title = format!("{} {}", c.short, c.subject);
                let paths = Value::Arr(names.into_iter().map(Value::Str).collect());
                Some((
                    crate::obj! { "kind" => "commit", "ref" => c.hash.as_str(), "paths" => paths, "title" => title.as_str() },
                    title,
                ))
            }
            "stash" => {
                let s = row.stash.as_ref()?;
                let title = format!("{} {}", s.r#ref, s.subject);
                Some((crate::obj! { "kind" => "stash", "ref" => s.r#ref.as_str(), "title" => title.as_str() }, title))
            }
            "tag" => {
                let b = row.branch()?;
                let title = format!("tag {}", b.name);
                Some((crate::obj! { "kind" => "tag", "ref" => b.name.as_str(), "title" => title.as_str() }, title))
            }
            _ => None,
        }
    }

    /// The row shows in the preview: a file's changes, a commit, a stash or a tag.
    pub(super) fn previewable(&self, row: Option<&Row>) -> bool {
        row.is_some_and(|r| r.kind == "file") || self.show_of(row).is_some()
    }

    /// Below SPLIT columns (always in a review), the selection shows in the tab's preview, opened on the right of the
    /// pane (open): a file's changes, a commit, a stash or a tag, else a line saying to select one (blank). focus: the
    /// preview's.
    pub(super) fn preview_row(&mut self, row: Option<&Row>, open: bool, focus: bool) {
        let Some(top) = self.repo().map(|r| r.top.clone()) else { return };
        self.preview_key = Some(row.map(|r| r.key.clone()));
        let pane = self.pane.clone();
        let (abs, shown) = match row.and_then(|r| r.item.as_ref().filter(|_| r.kind == "file")) {
            Some(it) => (
                paths::join(&[&top, &it.path]),
                actions::Shown { view: "diff".into(), beside: Some(pane.clone()), ..Default::default() },
            ),
            None => {
                let mut show = self.show_of(row).map_or_else(|| crate::obj! { "blank" => true }, |(v, _)| v);
                show.set("cwd", Value::Str(top.clone()));
                (String::new(), actions::Shown { show: Some(show), beside: Some(pane.clone()), ..Default::default() })
            }
        };
        self.herdr.send(Box::new(move || {
            let msg = actions::preview(&abs, &top, &pane, 0.0, focus, open, &shown);
            Box::new(move |p: &mut Pane| match msg {
                Some(msg) => p.flash(msg, true),
                // Its share set once the preview is in the layout: the resize that its opening sends may come before.
                None if open => p.keep_share(0.0),
                None => {}
            })
        }));
    }

    // --- diffs

    /// A diff opened: shown at once (Loading…), filled once git answers.
    pub(super) fn show_diff(
        &mut self,
        key: &str,
        title: &str,
        args: Option<Vec<String>>,
        item: Option<Item>,
        tag: &'static str,
    ) {
        state::show_diff(&mut self.s, key, title, item, tag);
        self.draw();
        // diff::show_args gives none for a request not well formed: the JS's git(null) threw, the diff left loading.
        let Some(args) = args else { return };
        let key = key.to_string();
        self.query(
            move |top| {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                crate::diff::parse(&state::git(top, &args, &GitOpts::default()).out)
            },
            move |p, parsed| {
                state::show_diff_done(&mut p.s, &key, parsed);
                p.draw();
            },
        );
    }

    pub(super) fn open_item_diff(&mut self, it: &Item) {
        let has_head = self.repo().is_some_and(|r| r.status.has_head);
        let args = state::item_diff_args(it, has_head);
        let key = format!("{}:{}", it.sec, it.path);
        self.show_diff(&key, &it.path.clone(), Some(args), Some(it.clone()), state::item_tag(it));
    }

    pub(super) fn close_diff(&mut self) {
        self.s.diff = None;
        self.draw();
    }

    /// A branch's comparison with the base: shown at once, filled once git answers; focus: then focused (=).
    pub(super) fn compare(&mut self, row: &Row, focus: bool) {
        let Some(d) = state::compare_view(&self.s, row) else { return };
        let Some(b) = row.branch() else { return };
        let range = format!("{}...{}", state::base_ref(&self.s), state::full_ref(b));
        let key = d.key.clone();
        self.s.diff = Some(d);
        self.draw();
        self.query(
            move |top| state::compare(top, &range),
            move |p, cmp| {
                let Some(d) = p.s.diff.as_mut().filter(|d| d.compare && d.key == key) else { return };
                d.cmp = Some(cmp);
                d.cache.clear();
                if focus {
                    d.focus = true;
                }
                p.draw();
            },
        );
    }

    /// = also has the Changes pane and the preview's Diff view of the tab show the branch's changes since it left the
    /// base (the scope base...branch, core::scope, in full names as compare has them: a tag of the same name does not
    /// stand in); a branch merely selected leaves the scope alone. In a review, the tab reviewed too, not only the
    /// review's own, whose preview shows the files.
    pub(super) fn share_compare(&mut self, row: &Row) {
        let Some(b) = row.branch() else { return };
        let rev = format!("{}...{}", state::base_ref(&self.s), state::full_ref(b));
        let (known, pane, review) = (self.my_tab.clone(), self.pane.clone(), self.review.clone());
        self.herdr.send(Box::new(move || {
            let mine = known.unwrap_or_else(|| {
                actions::tab_of(&pane).and_then(|t| t.me.get("tab_id").filter(|v| v.truthy()).map(js::string))
            });
            let scope = crate::core::scope::Scope::new("rev", Some(&rev));
            let mut tabs: Vec<&str> = Vec::new();
            for t in [mine.as_deref(), Some(review.as_str())].into_iter().flatten() {
                if !t.is_empty() && !tabs.contains(&t) {
                    tabs.push(t);
                }
            }
            for t in tabs {
                crate::core::scope::write(t, &scope);
            }
            Box::new(move |p: &mut Pane| p.my_tab = Some(mine))
        }));
    }

    /// The view shown: drawn, then loaded.
    pub(super) fn set_view(&mut self, id: &'static str) {
        if state::set_view(&mut self.s, id) {
            self.draw();
            self.schedule();
        }
    }

    pub(super) fn toggle(&mut self, row: &Row, open: Option<bool>) {
        state::toggle(&mut self.s, row, open);
        self.draw();
    }

    pub(super) fn toggle_tree(&mut self) {
        state::toggle_tree(&mut self.s);
        self.draw();
    }

    pub(super) fn all_files(&mut self) {
        let msg = state::all_files(&mut self.s);
        self.flash(msg, false);
        self.schedule();
    }

    pub(super) fn toggle_all_branches(&mut self) {
        let msg = state::toggle_all_branches(&mut self.s);
        self.flash(msg, false);
        self.schedule();
    }

    /// The branch picker opened, its branches loaded.
    pub(super) fn open_picker(&mut self, mode: Option<&'static str>, anchor: Option<(i64, i64)>) {
        if self.repo().is_none() {
            return;
        }
        state::open_picker(&mut self.s, mode, anchor);
        self.draw();
        self.query(state::load_refs, |p, refs| {
            let inline = p.inline.is_some();
            state::picker_loaded(&mut p.s, refs, inline);
            p.draw();
        });
    }

    /// A History row for a commit of a comparison (Enter or the preview shows it).
    pub(super) fn commit_row(c: &parse::Compared) -> Row {
        let commit =
            Commit { hash: c.hash.clone(), short: c.short.clone(), subject: c.subject.clone(), ..Commit::default() };
        Row {
            kind: "commit",
            key: format!("commit:{}", c.hash),
            commit: Some(Box::new(Hist { commit, ..Hist::default() })),
            ..Row::default()
        }
    }
}
