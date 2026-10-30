//! The explorer pane: the space's tree, folded with the mouse and the keyboard, VS Code-style git
//! status, filter as you type, file operations, automatic refresh. Enter or double-click: preview (or the editor);
//! right-click: action menu. Header icon: settings.
//!
//! The herdr and git calls run on two worker threads, one for herdr and the
//! actions, one for git, each a job at a time, and their results come back to the pane's loop (`Then`), which owns all
//! the state. Timers are deadlines of that loop.

mod input;
mod ops;
mod render;
mod settings;
mod state;
pub mod tree;
mod update_ui;
mod watch;

pub use state::{Choice, Filter, Inline, It, Item, Prompt, State};

use crate::core::actions::{self, Kept};
use crate::core::exec::{self, Bindings, KeyMap, Opts};
use crate::core::icons::{self, Loaded};
use crate::core::screen::{self, Input, Screen};
use crate::core::ui::{self, HelpState, Scrollbar};
use crate::core::watch::Watch;
use crate::core::{paths, shown, update, worker};
use crate::json::{self, Value};
use crate::sys::{self, Watcher};
use state::tree_settings;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tree::Tree;
use watch::{AfterGit, Repo};

/// Columns when the terminal size is unknown.
const WIDTH: usize = exec::DEFAULT_WIDTH as usize;

/// The key help (help action), by group, with the keys bound now: (group, [(action, description)]).
const HELP: [(&str, &[(&str, &str)]); 5] = [
    (
        "Move",
        &[
            ("up", "up"),
            ("down", "down"),
            ("pageUp", "page up"),
            ("pageDown", "page down"),
            ("halfPageUp", "half page up"),
            ("halfPageDown", "half page down"),
            ("top", "first"),
            ("bottom", "last"),
            ("expand", "unfold"),
            ("collapse", "fold, parent"),
            ("filter", "filter"),
        ],
    ),
    (
        "Review",
        &[
            ("changedOnly", "changed files only"),
            ("viewChanges", "changes in the preview"),
            ("nextChange", "next changed file"),
            ("previousChange", "previous changed file"),
            ("fileHistory", "history of the file"),
        ],
    ),
    (
        "Open",
        &[
            ("open", "open, fold"),
            ("preview", "preview"),
            ("openInEditor", "open in editor"),
            ("edit", "edit (terminal)"),
            ("openInApp", "open in app"),
            ("copyPath", "copy path"),
            ("copyRelativePath", "copy relative path"),
        ],
    ),
    (
        "Files",
        &[
            ("newFile", "new file"),
            ("newFolder", "new folder"),
            ("rename", "rename"),
            ("delete", "to the trash"),
            ("menu", "actions menu"),
        ],
    ),
    (
        "Panes",
        &[
            ("goToFile", "go to file"),
            ("searchInFiles", "search in files"),
            ("sourceControl", "source control"),
            ("settings", "settings"),
            ("narrower", "narrower"),
            ("wider", "wider"),
            ("toggleHidden", "hidden files"),
            ("toggleIgnored", "git-ignored files"),
            ("parentRoot", "parent as root"),
            ("refresh", "refresh"),
            ("help", "keys"),
            ("close", "close"),
        ],
    ),
];

/// Actions and their default keys, changed in keys.json ("explorer" object). Key names: up, down, left, right, home,
/// end, pageup, pagedown, enter, space, tab, backspace, delete, escape, menu, f1…f5, ctrl+<letter>, alt+<key>, or the
/// character itself ('/', 'n', 'N'). In the menu: up, down, open, expand, close; in the filter: the keys that are not
/// characters. inner: the side of the pane's inner border, which shift+← and shift+→ push.
fn default_keys(inner: &str) -> Bindings {
    let (narrower, wider) =
        if inner == "right" { ("shift+left", "shift+right") } else { ("shift+right", "shift+left") };
    let list: &[(&str, &[&str])] = &[
        ("up", &["up", "k"]),
        ("down", &["down", "j"]),
        ("pageUp", &["pageup"]),
        ("pageDown", &["pagedown"]),
        ("top", &["home", "g"]),
        ("bottom", &["end", "G"]),
        ("open", &["enter", "space"]),
        ("expand", &["right", "l"]),
        ("collapse", &["left", "h"]),
        ("close", &["escape", "q"]),
        ("menu", &["m", "menu", "shift+f10"]),
        ("refresh", &["r"]),
        ("filter", &["/"]),
        ("goToFile", &["ctrl+p"]),
        ("searchInFiles", &["ctrl+f"]),
        ("sourceControl", &["s"]),
        ("settings", &[","]),
        ("toggleHidden", &["."]),
        ("toggleIgnored", &["i"]),
        ("newFile", &["n"]),
        ("newFolder", &["N"]),
        ("rename", &["R", "f2"]),
        ("delete", &["delete"]),
        ("edit", &["E"]),
        ("openInEditor", &["e"]),
        ("openInApp", &["o"]),
        ("preview", &["p"]),
        ("copyPath", &["y"]),
        ("copyRelativePath", &["Y"]),
        ("parentRoot", &["backspace"]),
        ("changedOnly", &["c"]),
        ("viewChanges", &["d"]),
        ("nextChange", &["]"]),
        ("previousChange", &["["]),
        ("fileHistory", &["H"]),
        ("narrower", &[">", narrower]),
        ("wider", &["<", wider]),
        ("halfPageUp", &["ctrl+u"]),
        ("halfPageDown", &["ctrl+d"]),
        ("help", &["?", "f1"]),
    ];
    list.iter().map(|(a, keys)| (a.to_string(), keys.iter().map(|k| k.to_string()).collect())).collect()
}

/// What the loop runs once a job of a worker is done: the result applied to the pane.
pub type Then = worker::Then<Explorer>;

/// A job of a worker.
type Job = worker::Job<Explorer>;

type Worker = worker::Worker<Explorer>;

/// What an action gives: its message at once, or a job whose message comes later.
pub enum Act {
    Now(Option<String>),
    Later(Job),
}

/// An action whose message comes from a job: a call to herdr, git, the clipboard, an application.
fn later(f: impl FnOnce() -> Option<String> + Send + 'static) -> Act {
    Act::Later(Box::new(move || {
        let text = f();
        Box::new(move |ex: &mut Explorer| ex.flash(text))
    }))
}

/// An action run on the pane, as a menu item, a question's answer or a prompt's end runs it.
type Run = Rc<dyn Fn(&mut Explorer) -> Act>;

/// An action whose job gives the message, made again at each run.
fn job(f: impl FnOnce() -> Option<String> + Clone + Send + 'static) -> Run {
    Rc::new(move |_: &mut Explorer| later(f.clone()))
}

/// An action done on the pane, without a message.
fn now(f: impl Fn(&mut Explorer) + 'static) -> Run {
    Rc::new(move |ex: &mut Explorer| {
        f(ex);
        Act::Now(None)
    })
}

/// Deleted for good: the message of actions::remove, else that the entry is gone.
fn removal(abs: String, name: String) -> impl Fn(&mut Explorer) -> Act + 'static {
    move |_| Act::Now(Some(actions::remove(&abs).unwrap_or_else(|| format!("{name} deleted"))))
}

/// The width kept, as the keeper (on the herdr worker) and the pane share it.
struct Width(Arc<Mutex<f64>>);

impl Kept for Width {
    fn columns(&self) -> f64 {
        sys::size().map_or(f64::NAN, |(w, _)| f64::from(w))
    }

    fn get(&self) -> Option<f64> {
        Some(*self.0.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn set(&mut self, width: f64) {
        save_width(&self.0, width);
    }
}

/// A new width, remembered for the next openings.
fn save_width(width: &Mutex<f64>, w: f64) {
    let mut kept = width.lock().unwrap_or_else(|p| p.into_inner());
    if *kept == w {
        return;
    }
    *kept = w;
    if let Some(file) = exec::width_file() {
        exec::write_atomic(&file, json::number(w));
    }
}

/// The pane: its state, the objects that stay, and the deadlines of its timers.
pub struct Explorer {
    home: String,
    root: String,
    pane: String,
    save_file: Option<String>,
    settings: Value,
    defaults: Bindings,
    keys: KeyMap,
    theme: Option<Loaded>,
    /// A newer release, found by the hook's daily check: ↑ and its version in the header; skipped: the one the user
    /// skipped, only in the settings.
    latest: Option<String>,
    skipped: Option<String>,
    /// Started on the code of an update made since the explorers last ran: the version in a message, and the
    /// release's notes in the header's pill for half a minute (until notice_at).
    notice: Option<String>,
    notice_at: Option<Instant>,
    /// update.json, read again when it changes; the version on disk, watched once started.
    update_watch: Option<Watch>,
    version_watch: Option<update::VersionWatch>,
    /// Restarted on new code: this process only waits for the new one, its work still running stops there.
    stopped: bool,
    state: State,
    scrollbar: Scrollbar,
    tree: Tree,
    /// Whether the terminal has the Kitty graphics protocol: None until its answer, at most 500 ms.
    graphics: Option<bool>,
    screen: Screen,
    tx: Sender<Then>,
    rx: Receiver<Then>,
    waker: sys::Waker,
    herdr: Worker,
    git_worker: Worker,
    // refresh
    refresh_at: Option<Instant>,
    busy: bool,
    again: bool,
    probing: bool,
    root_watcher: Option<Watcher>,
    git_watcher: Option<Watcher>,
    /// The repositories of the root's folders, when the root is in none (tree.git.repos, their names).
    repos: Vec<Repo>,
    /// A .git folder appeared: git looked for again 500 ms later, and again after the events seen meanwhile.
    git_at: Option<Instant>,
    git_more: Option<Instant>,
    // width
    width: Arc<Mutex<f64>>,
    keeper: Arc<Mutex<actions::WidthKeeper>>,
    keeper_busy: bool,
    /// A look at the terminal's folder still on the herdr worker.
    follow_busy: bool,
    width_at: Vec<Instant>,
    settle_at: Option<Instant>,
    // timers
    flash_at: Option<Instant>,
    graphics_at: Option<Instant>,
    following_at: Option<Instant>,
    scan_at: Instant,
    follow_at: Option<Instant>,
    // following the terminal
    /// The explorer's tab, learned at the first look; Follow sleeps while herdr shows another (`shown`).
    tab: Option<String>,
    shown: Option<String>,
    shown_watch: Option<Watch>,
    last_cwd: Option<String>,
    forced: bool,
    /// The tab's request file (a file opened from the search), watched, and its time last seen.
    requests: Option<(Watch, f64)>,
    keys_watch: Option<Watch>,
    settings_watch: Option<Watch>,
}

/// `path.relative(from, to)` with / between the parts.
fn rel_of(from: &str, to: &str) -> String {
    let r = paths::relative(from, to);
    if paths::SEP == '/' { r } else { r.replace(paths::SEP, "/") }
}

/// Whether dir is root or under it.
fn inside(dir: &str, root: &str) -> bool {
    let r = paths::relative(root, dir);
    !r.starts_with("..") && !paths::is_absolute(&r)
}

/// $HOME, one of its parents or the filesystem root.
fn big(dir: &str) -> bool {
    // /home → /var/home on some systems.
    let real = |p: &str| exec::walk_realpath(p).unwrap_or_else(|_| p.to_string());
    let up = paths::relative(&real(dir), &real(&icons::homedir()));
    dir == paths::root(dir) || (!up.starts_with("..") && !paths::is_absolute(&up))
}

/// A file's modification time in ms, 0 when missing (watchFile's stat).
fn mtime_ms(file: &str) -> f64 {
    std::fs::metadata(file).ok().and_then(|m| m.modified().ok()).map_or(0.0, crate::core::clock::ms)
}

/// Runs the pane: the process's exit code.
pub fn run(args: &[String]) -> i32 {
    worker::quiet_panics();
    let mut ex = match Explorer::new(args) {
        Ok(ex) => ex,
        Err(e) => {
            eprintln!("herdr-explorer: {e}");
            return 1;
        }
    };
    ex.start();
    ex.run()
}

impl Explorer {
    fn new(args: &[String]) -> std::io::Result<Explorer> {
        let given = exec::env_var("EXPLORER_ROOT").filter(|r| !r.is_empty()).or_else(|| args.first().cloned());
        let home = paths::resolve(&[&given.unwrap_or_else(paths::cwd)]);
        // The space folder, or another one chosen with Set as root (EXPLORER_TREE: the one it had before a restart).
        let root = exec::env_var("EXPLORER_TREE")
            .filter(|r| !r.is_empty())
            .map_or_else(|| home.clone(), |t| paths::resolve(&[&t]));
        let pane = exec::env_var("HERDR_PANE_ID").unwrap_or_default();
        let save_file = exec::env_var("HERDR_PLUGIN_STATE_DIR")
            .filter(|d| !d.is_empty())
            .map(|d| paths::join(&[&d, "expanded.json"]));
        ui::init_theme();
        let mut settings = exec::read_settings();
        ui::use_glyphs(settings.get("glyphs").and_then(Value::as_str).unwrap_or(""));
        // Where this explorer was docked: a later change applies to the next tabs. The side of its inner border, on
        // the right of an explorer docked on the left.
        let side = settings.get("side").and_then(Value::as_str).map(String::from);
        let inner = if side.as_deref() == Some("right") { "left" } else { "right" };
        let defaults = default_keys(inner);
        let keys = exec::read_keys("explorer", &defaults);
        let keys_watch = exec::keys_file().map(|f| Watch::new(&f, Duration::from_millis(1000)));
        let icons_entry = settings.get("icons").cloned().unwrap_or(Value::Null);
        let theme = icons::load(&icons_entry);
        // Moved by an update of its extension: its new place saved for the next starts. No longer installed: the
        // default icons saved, rather than searching for it again at every start.
        let moved = match &theme {
            Some(t) => t.entry.get("file") != icons_entry.get("file"),
            None => icons_entry.truthy() && icons_entry.get("id").is_some_and(Value::truthy),
        };
        if moved {
            let entry = theme.as_ref().map_or_else(|| Value::Str(String::new()), |t| t.entry.clone());
            settings.set("icons", entry.clone());
            let mut saved = exec::read_settings();
            saved.set("icons", entry);
            exec::write_settings(&saved);
        }
        let settings_watch = exec::settings_file().map(|f| Watch::new(&f, Duration::from_millis(1000)));
        let shown_watch = shown::file().map(|f| Watch::new(&f, Duration::from_millis(1000)));
        let screen = Screen::open(screen::Options { mouse: true, motion: true, focus: true })?;
        let waker = screen.waker();
        let (tx, rx) = channel();
        let herdr = Worker::new(tx.clone(), waker.clone());
        let git_worker = Worker::new(tx.clone(), waker.clone());
        let width = Arc::new(Mutex::new(exec::read_width()));
        let keeper = Arc::new(Mutex::new(actions::WidthKeeper::new(&pane, side.as_deref())));
        let tree = Tree::new(&root, tree_settings(&settings));
        let now = Instant::now();
        update::init();
        update::note_version();
        let notice = update::just_updated();
        Ok(Explorer {
            home,
            root,
            pane,
            save_file,
            settings,
            defaults,
            keys,
            theme,
            latest: update::available(),
            skipped: update::skipped(),
            notice_at: notice.as_ref().map(|_| now + Duration::from_secs(30)),
            notice,
            update_watch: update::file().map(|f| Watch::new(&f, Duration::from_millis(1000))),
            version_watch: None,
            stopped: false,
            state: State {
                expanded: vec![String::new()],
                rows: Vec::new(),
                cursor: 0,
                top: 0,
                selected: None,
                focused: true,
                menu: None,
                help: None,
                settings: None,
                flash: None,
                last_click: None,
                filter: None,
                changed: false,
                prompt: None,
                confirm: None,
                choice: None,
            },
            scrollbar: Scrollbar::new(),
            tree,
            graphics: None,
            screen,
            tx,
            rx,
            waker,
            herdr,
            git_worker,
            refresh_at: None,
            busy: false,
            again: false,
            probing: false,
            root_watcher: None,
            git_watcher: None,
            repos: Vec::new(),
            git_at: None,
            git_more: None,
            width,
            keeper,
            keeper_busy: false,
            follow_busy: false,
            width_at: Vec::new(),
            settle_at: None,
            flash_at: None,
            graphics_at: None,
            following_at: None,
            scan_at: now + Duration::from_secs(30),
            follow_at: Some(now + Duration::from_secs(2)),
            tab: None,
            shown: shown::get(),
            shown_watch,
            last_cwd: None,
            forced: false,
            requests: None,
            keys_watch,
            settings_watch,
        })
    }

    /// What the pane does first, after the screen is set up.
    fn start(&mut self) {
        // A file opened from the search in the tab's preview (reveal in its request, actions.preview): selected in
        // the tree, its folders expanded, when it is under the root. The tab's request file is watched once the tab
        // is known.
        let pane = if self.pane.is_empty() { "undefined".to_string() } else { self.pane.clone() };
        // The pane, then its tab in a job of its own: the herdr calls sent meanwhile (the right-click below) go
        // between the two.
        self.herdr.send(Box::new(move || {
            let me = actions::pane_of(&pane);
            Box::new(move |ex: &mut Explorer| {
                let Some(me) = me else { return };
                ex.herdr.send(Box::new(move || {
                    let tab = actions::tab_with(me).id();
                    Box::new(move |ex: &mut Explorer| {
                        if ex.stopped {
                            return;
                        }
                        // No state folder: nothing to watch.
                        let Some(file) = actions::request_file(&tab) else { return };
                        let seen = mtime_ms(&file);
                        ex.requests = Some((Watch::new(&file, Duration::from_millis(300)), seen));
                    })
                }));
            })
        }));
        // The Kitty graphics query, as the preview sends it.
        let _ = sys::write_stdout(ui::KITTY_QUERY.as_bytes());
        crate::core::setup::refresh_skill();
        self.offer_setup();
        if let Some(to) = self.notice.clone() {
            self.flash(Some(format!("Updated to {to}")));
        }
        self.flash(exec::keys_problem("explorer"));
        self.graphics_at = Some(Instant::now() + Duration::from_millis(500));
        self.watch_root();
        if self.root != self.home {
            self.announce();
        }
        // Updated plugin: this explorer starts again on the new code once nothing is being typed or chosen.
        self.version_watch = Some(update::VersionWatch::new());
        // Right-clicks in this pane go to the explorer (its menu) instead of herdr's menu; the frame keeps herdr's.
        if !self.pane.is_empty() {
            let pane = self.pane.clone();
            self.herdr.send(Box::new(move || {
                exec::run(exec::herdr(), &["pane", "input", &pane, "--right-click", "pane"], &Opts::default());
                Box::new(|_: &mut Explorer| {})
            }));
        }
        self.write_pane_state();
        self.load();
        self.build_rows();
        self.render();
        self.init_git(AfterGit::Refresh);
        let now = Instant::now();
        self.width_at = [300, 1000, 2500].iter().map(|&ms| now + Duration::from_millis(ms)).collect();
    }

    /// The loop: the screen's events, the workers' results and the timers, until the pane is closed.
    fn run(&mut self) -> i32 {
        loop {
            let timeout = self.next_deadline().map(|t| t.saturating_duration_since(Instant::now()));
            for input in self.screen.wait(timeout) {
                match input {
                    Input::Ui(e) => self.event(e),
                    Input::Resize => {
                        self.resized();
                        self.reveal();
                        self.render();
                    }
                    Input::Theme => {
                        self.tree.cache.clear();
                        self.build_rows();
                        self.render();
                    }
                    Input::Wake => {}
                }
                self.results();
            }
            self.results();
            self.timers();
        }
    }

    /// The workers' results, applied.
    fn results(&mut self) {
        while let Ok(then) = self.rx.try_recv() {
            then(self);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        let mut all = vec![Some(self.scan_at), self.follow_at];
        all.extend([
            self.refresh_at,
            self.git_at,
            self.settle_at,
            self.flash_at,
            self.graphics_at,
            self.following_at,
            self.notice_at,
            self.scrollbar.due(),
        ]);
        all.extend(self.update_watch.iter().map(Watch::due));
        all.extend(self.version_watch.iter().map(update::VersionWatch::due));
        all.extend(self.width_at.iter().map(|&t| Some(t)));
        all.extend([&self.keys_watch, &self.settings_watch, &self.shown_watch].into_iter().flatten().map(Watch::due));
        all.extend(self.requests.iter().map(|(w, _)| w.due()));
        if !self.keeper_busy {
            // Held by a check on the worker: its result wakes the loop, which looks again.
            all.push(self.keeper.try_lock().ok().and_then(|k| k.due()));
        }
        all.into_iter().flatten().min()
    }

    /// The timers whose time has come.
    fn timers(&mut self) {
        let now = Instant::now();
        let due = |t: &mut Option<Instant>| t.is_some_and(|t| t <= now) && t.take().is_some();
        if due(&mut self.refresh_at) {
            self.refresh();
        }
        if due(&mut self.git_at) {
            self.git_at = self.git_more.take();
            self.init_git(AfterGit::ScheduleIfTop);
        }
        if due(&mut self.settle_at) {
            self.settle();
        }
        if due(&mut self.flash_at) {
            self.state.flash = None;
            self.render();
        }
        if due(&mut self.graphics_at) && self.graphics.is_none() {
            self.graphics = Some(false);
            self.render();
        }
        if due(&mut self.following_at) {
            self.follow_selection();
        }
        if let Some(top) = self.scrollbar.tick(now) {
            self.state.top = top;
            self.render();
        }
        let before = self.width_at.len();
        self.width_at.retain(|&t| t > now);
        for _ in self.width_at.len()..before {
            self.ensure_width();
        }
        if !self.keeper_busy && self.keeper.try_lock().ok().and_then(|k| k.due()).is_some_and(|t| t <= now) {
            let (keeper, width) = (self.keeper.clone(), self.width.clone());
            self.keeper_job(move || {
                let mut k = keeper.lock().unwrap_or_else(|p| p.into_inner());
                k.tick(Instant::now(), &mut Width(width));
            });
        }
        if self.scan_at <= now {
            // A safety net in case a file event is lost.
            self.scan_at = now + Duration::from_secs(30);
            self.schedule();
        }
        if self.shown_watch.as_mut().is_some_and(Watch::poll) {
            self.shown = shown::get();
            // Its tab shown again: a look at once. Another tab: the explorer's own asked again, as its pane may have
            // moved there.
            if !self.asleep() {
                self.follow_at.get_or_insert(now);
            } else {
                self.learn_tab();
            }
        }
        // Every 2 s while Follow is on and herdr shows the explorer's tab; off, again once the settings change or
        // the tab shows.
        if self.follow_at.is_some_and(|t| t <= now) {
            self.follow_at = None;
            if !self.asleep() {
                self.follow();
                let on = self.settings.get("follow").is_some_and(Value::truthy);
                self.follow_at = on.then(|| now + Duration::from_secs(2));
            }
        }
        if self.keys_watch.as_mut().is_some_and(Watch::poll) {
            self.keys = exec::read_keys("explorer", &self.defaults);
            self.flash(exec::keys_problem("explorer"));
        }
        if self.settings_watch.as_mut().is_some_and(Watch::poll) {
            self.settings_changed();
            self.follow_at.get_or_insert(now);
        }
        if self.requests.as_mut().is_some_and(|(w, _)| w.poll()) {
            self.request();
        }
        if due(&mut self.notice_at) {
            self.notice = None;
            self.render();
        }
        if self.update_watch.as_mut().is_some_and(Watch::poll) {
            self.update_changed();
        }
        let idle = self.idle_for_update();
        if self.version_watch.as_mut().is_some_and(|v| v.poll(|| idle)) {
            self.restart_on_update();
        }
    }

    /// A check of the width on the herdr worker; its result wakes the loop, which then looks at the next one due.
    fn keeper_job(&mut self, f: impl FnOnce() + Send + 'static) {
        self.keeper_busy = true;
        self.herdr.send(Box::new(move || {
            // Free again even when the check failed.
            let _ = std::panic::catch_unwind(AssertUnwindSafe(f));
            Box::new(|ex: &mut Explorer| ex.keeper_busy = false)
        }));
    }

    /// Enforced on opening and whenever another resize moves the explorer (actions.WidthKeeper). A resize of its own
    /// split, dragging its border or with < >, becomes the new width, remembered for the next openings.
    fn ensure_width(&mut self) {
        self.check_width(true);
    }

    fn check_width(&mut self, force: bool) {
        let (keeper, width) = (self.keeper.clone(), self.width.clone());
        self.keeper_job(move || {
            let mut k = keeper.lock().unwrap_or_else(|p| p.into_inner());
            k.check(force, &mut Width(width));
        });
    }

    /// The width checked at once, so that a pane moved by another one's resize goes back while the gesture goes on.
    /// Alone in the tab (the terminal has exited), the explorer closes, and the tab with it: decided at the end of
    /// the gesture (a drag sends one event per column).
    fn resized(&mut self) {
        self.check_width(false);
        self.settle_at = Some(Instant::now() + Duration::from_millis(400));
    }

    fn settle(&mut self) {
        if self.pane.is_empty() {
            return;
        }
        let pane = self.pane.clone();
        self.herdr.send(Box::new(move || {
            let layout = actions::layout_of(&pane);
            let single = layout.truthy()
                && !layout.get("zoomed").is_some_and(Value::truthy)
                && layout.get("panes").and_then(Value::as_array).is_some_and(|p| p.len() == 1);
            let close = single && alone(&pane);
            Box::new(move |ex: &mut Explorer| {
                if close {
                    ex.close();
                }
            })
        }));
    }

    /// < and >: 4 columns wider or narrower, from the width asked for last, so that a key held down adds up.
    fn widen(&mut self, cols: f64) {
        let w = *self.width.lock().unwrap_or_else(|p| p.into_inner());
        save_width(&self.width, (w + cols).max(10.0));
        self.ensure_width();
    }

    /// The tab's preview closes with the explorer: reopened, the explorer then takes the side of the tab, and the
    /// next preview opens beside the terminal.
    fn close(&mut self) {
        self.save();
        if self.pane.is_empty() {
            return;
        }
        let pane = self.pane.clone();
        self.herdr.send(Box::new(move || {
            let panes = actions::tab_of(&pane).map(|t| t.panes).unwrap_or_default();
            for p in panes.iter().filter(|p| p.get("label").and_then(Value::as_str) == Some("Preview")) {
                let id = p.get("pane_id").map_or_else(|| "undefined".into(), crate::json::text);
                exec::run(exec::herdr(), &["pane", "close", &id], &Opts::default());
            }
            exec::run(exec::herdr(), &["pane", "close", &pane], &Opts::default());
            Box::new(|_: &mut Explorer| {})
        }));
    }

    /// The icon theme or icon set chosen in another explorer, the update check, the opening in new tabs, what Enter
    /// opens, the editor and the preview following the selection changed there, and the preview and diff settings
    /// changed in their panes. Hidden and ignored files stay this explorer's; Follow terminal is read again at each
    /// check (follow).
    fn settings_changed(&mut self) {
        // A file read while being written is skipped, rather than taken for the default settings.
        let Some(file) = exec::settings_file() else { return };
        let Some(saved) = exec::read_text(&file).ok().and_then(|t| json::parse(&t).ok()) else { return };
        if saved.is_null() {
            // Null settings: nothing changed.
            return;
        }
        let chosen = saved.get("icons").filter(|v| v.truthy()).cloned().unwrap_or(Value::Str(String::new()));
        let names = [
            "updateCheck",
            "autoOpen",
            "open",
            "editor",
            "singleClick",
            "wrap",
            "lineNumbers",
            "changes",
            "images",
            "diffLayout",
            "glyphs",
        ];
        for name in names {
            let Some(v) = saved.get(name) else { continue };
            if Some(v) == self.settings.get(name) {
                continue;
            }
            self.settings.set(name, v.clone());
            if name == "glyphs" {
                ui::use_glyphs(v.as_str().unwrap_or(""));
            }
            self.render();
        }
        let same = json::stringify(&chosen) == json::stringify(self.settings.get("icons").unwrap_or(&Value::Null));
        if same {
            return;
        }
        self.theme = icons::load(&chosen);
        self.settings.set("icons", chosen);
        self.render();
    }

    fn flash(&mut self, text: Option<String>) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return self.render() };
        let time = ui::message_time(ui::message_kind(&text));
        self.state.flash = Some(text);
        self.render();
        self.flash_at = Some(Instant::now() + Duration::from_millis(time));
    }

    /// The terminal's size: columns and rows, 32 and 24 when unknown.
    fn size(&self) -> (usize, usize) {
        // `out.columns || WIDTH`: a size of 0, as a pty gets before its first resize, is unknown too.
        sys::size().map_or((WIDTH, 24), |(w, h)| {
            (if w == 0 { WIDTH } else { w as usize }, if h == 0 { 24 } else { h as usize })
        })
    }

    /// A setting changed from the settings view or a key, saved for every explorer.
    fn set_setting(&mut self, name: &str, value: Value) {
        self.settings.set(name, value.clone());
        let mut saved = exec::read_settings();
        saved.set(name, value.clone());
        exec::write_settings(&saved);
        if name == "glyphs" {
            ui::use_glyphs(value.as_str().unwrap_or(""));
        }
        if name == "theme" {
            ui::use_theme(ui::setting_theme(value.as_str()));
        }
        self.tree.settings = tree_settings(&self.settings);
        self.tree.cache.clear();
        self.build_rows();
        if name == "follow" {
            self.last_cwd = None;
            self.forced = true;
            self.follow();
        }
    }

    /// Every key of the explorer, as bound now, over the whole pane.
    fn open_help(&mut self) {
        self.state.help = Some(HelpState::default());
        self.render();
    }

    fn help_groups(&self) -> Vec<ui::Group> {
        HELP.iter()
            .map(|(group, items)| {
                let keys = items
                    .iter()
                    .map(|(action, text)| {
                        let k = self.keys_of(action).join(" ");
                        (if k.is_empty() { "—".into() } else { k }, text.to_string())
                    })
                    .collect();
                (group.to_string(), keys)
            })
            .collect()
    }

    /// The keys of an action, its default ones first in their order.
    fn keys_of(&self, action: &str) -> Vec<String> {
        let defaults = self.defaults.iter().find(|(a, _)| a == action).map(|(_, k)| k.as_slice()).unwrap_or(&[]);
        let rank = |k: &str| defaults.iter().position(|d| d == k).unwrap_or(usize::MAX);
        let mut list: Vec<String> = self.keys.0.iter().filter(|(_, a)| a == action).map(|(k, _)| k.clone()).collect();
        list.sort_by_key(|k| rank(k));
        list
    }
}

/// Alone in its tab by the pane list too: the layout alone can show a single pane while the tab has others (a pane
/// zoomed over the explorer, a client attaching), and closing then lost the explorer.
fn alone(pane: &str) -> bool {
    let ask = |args: &[&str]| exec::result(exec::run(exec::herdr(), args, &Opts::default()).as_deref());
    let Some(me) = ask(&["pane", "get", pane]).get("pane").filter(|p| p.truthy()).cloned() else { return false };
    let ws = me.get("workspace_id").map_or_else(|| "undefined".into(), crate::json::text);
    let all = ask(&["pane", "list", "--workspace", &ws]);
    let Some(all) = all.get("panes").filter(|p| p.truthy()) else { return false };
    let mine = all.as_array().into_iter().flatten().filter(|p| p.get("tab_id") == me.get("tab_id"));
    mine.count() == 1
}
