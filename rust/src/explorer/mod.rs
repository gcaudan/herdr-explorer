//! The explorer pane (src/explorer.js): the space's tree, folded with the mouse and the keyboard, VS Code-style git
//! status, filter as you type, file operations, automatic refresh. Enter or double-click: preview (or the editor);
//! right-click: action menu. Header icon: settings.
//!
//! The JS ran its herdr and git calls asynchronously; here they run on two worker threads, one for herdr and the
//! actions, one for git, each a job at a time, and their results come back to the pane's loop (`Then`), which owns all
//! the state. The JS's timers are deadlines of that loop.

mod input;
mod render;
mod settings;
pub mod tree;

use crate::core::actions::{self, Kept};
use crate::core::exec::{self, Bindings, KeyMap, Opts};
use crate::core::icons::{self, Loaded};
use crate::core::screen::{self, Input, Screen};
use crate::core::ui::{self, Ask, Confirm, Field, HelpState, Menu, Scrollbar};
use crate::core::watch::Watch;
use crate::core::{js, paths, worker};
use crate::json::{self, Value};
use crate::sys::{self, Watcher};
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tree::{Entry, Settings, Tree, holds, parent_of, tree_before};

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

/// What an action gives: its message at once, or a job whose message comes later (the JS's promise).
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

/// An item of the menu as menu_items makes it: its label, what it runs, the action whose key runs it too ("" for
/// none), red when it destroys.
pub struct It {
    label: String,
    run: Run,
    action: Option<&'static str>,
    danger: bool,
}

impl It {
    fn new(label: &str, run: Run, action: &'static str) -> Option<It> {
        Some(It { label: label.into(), run, action: (!action.is_empty()).then_some(action), danger: false })
    }

    fn danger(label: &str, run: Run) -> Option<It> {
        Some(It { danger: true, ..It::new(label, run, "")? })
    }
}

/// What a prompt does with the name typed.
type Named = Rc<dyn Fn(&mut Explorer, String) -> Act>;

/// A menu item's payload: what it runs, and the action whose key runs it too.
#[derive(Clone)]
pub struct Item {
    action: Option<&'static str>,
    run: Run,
}

/// A name typed: label, field, what it does (done), whether an empty answer is accepted, and where it shows.
pub struct Prompt {
    label: String,
    field: Field,
    done: Named,
    empty: bool,
    inline: Option<Inline>,
}

/// A name typed in the tree: on the row renamed, or on a new row at the top of its folder.
#[derive(Clone)]
pub enum Inline {
    Rename { rel: String },
    New { dir: String, folder: bool },
}

/// The filter: the field, and whether it is being typed (else kept, Tab).
pub struct Filter {
    field: Field,
    active: bool,
}

/// A question with several answers, and what each answer's key runs.
pub struct Choice {
    ask: Ask,
    on: Vec<(String, Run)>,
}

pub struct State {
    /// The folders unfolded, in the order they were (a JS Set).
    expanded: Vec<String>,
    rows: Vec<Entry>,
    cursor: i64,
    top: i64,
    selected: Option<String>,
    focused: bool,
    menu: Option<Menu<Item>>,
    help: Option<HelpState>,
    /// The settings view in place of the tree, and the page opened from it.
    settings: Option<settings::View>,
    flash: Option<String>,
    /// The last click, to detect double-clicks: its row and when.
    last_click: Option<(i64, Instant)>,
    filter: Option<Filter>,
    /// Rows limited to the changed files and their folders, all unfolded.
    changed: bool,
    prompt: Option<Prompt>,
    confirm: Option<(Confirm, Run)>,
    choice: Option<Choice>,
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

/// The pane: its state, the objects that stay, and the deadlines of the JS's timers.
pub struct Explorer {
    home: String,
    root: String,
    pane: String,
    save_file: Option<String>,
    settings: Value,
    defaults: Bindings,
    keys: KeyMap,
    theme: Option<Loaded>,
    // TODO(case 20): the update: a newer release, the one skipped, the update to finish, the one just done.
    latest: Option<String>,
    finish: Option<String>,
    notice: Option<String>,
    updating: bool,
    /// Restarted on new code: this process only waits for the new one (case 20).
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
    follow_at: Instant,
    // following the terminal
    last_cwd: Option<String>,
    forced: bool,
    /// The tab's request file (a file opened from the search), watched, and its time last seen.
    requests: Option<(Watch, f64)>,
    keys_watch: Option<Watch>,
    settings_watch: Option<Watch>,
}

/// The settings of the tree, from the explorer's.
fn tree_settings(s: &Value) -> Settings {
    let on = |k| s.get(k).is_some_and(Value::truthy);
    Settings { hidden: on("hidden"), ignored: on("ignored"), compact: on("compact") }
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
    let real = |p: &str| exec::node_realpath(p).unwrap_or_else(|_| p.to_string());
    let up = paths::relative(&real(dir), &real(&icons::homedir()));
    dir == paths::root(dir) || (!up.starts_with("..") && !paths::is_absolute(&up))
}

/// A file's modification time in ms, 0 when missing (watchFile's stat).
fn mtime_ms(file: &str) -> f64 {
    std::fs::metadata(file).ok().and_then(|m| m.modified().ok()).map_or(0.0, js::time_ms)
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
            Some(t) => !js::strict_eq(t.entry.get("file"), icons_entry.get("file")),
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
        let screen = Screen::open(screen::Options { mouse: true, motion: true, focus: true })?;
        let waker = screen.waker();
        let (tx, rx) = channel();
        let herdr = Worker::new(tx.clone(), waker.clone());
        let git_worker = Worker::new(tx.clone(), waker.clone());
        let width = Arc::new(Mutex::new(exec::read_width()));
        let keeper = Arc::new(Mutex::new(actions::WidthKeeper::new(&pane, side.as_deref())));
        let tree = Tree::new(&root, tree_settings(&settings));
        let now = Instant::now();
        Ok(Explorer {
            home,
            root,
            pane,
            save_file,
            settings,
            defaults,
            keys,
            theme,
            latest: None,
            finish: None,
            notice: None,
            updating: false,
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
            follow_at: now + Duration::from_secs(2),
            last_cwd: None,
            forced: false,
            requests: None,
            keys_watch,
            settings_watch,
        })
    }

    /// What the JS does as its script loads, after the screen is set up.
    fn start(&mut self) {
        // A file opened from the search in the tab's preview (reveal in its request, actions.preview): selected in
        // the tree, its folders expanded, when it is under the root. The tab's request file is watched once the tab
        // is known.
        let pane = if self.pane.is_empty() { "undefined".to_string() } else { self.pane.clone() };
        // The pane, then its tab in a job of its own: the herdr calls sent meanwhile (the right-click below) go
        // between the two, as while the JS awaited.
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
        self.graphics_at = Some(Instant::now() + Duration::from_millis(500));
        self.watch_root();
        if self.root != self.home {
            self.announce();
        }
        // TODO(case 20): the update watched, this explorer restarted on the new code.
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
        let mut all = vec![Some(self.scan_at), Some(self.follow_at)];
        all.extend([
            self.refresh_at,
            self.git_at,
            self.settle_at,
            self.flash_at,
            self.graphics_at,
            self.following_at,
            self.scrollbar.due(),
        ]);
        all.extend(self.width_at.iter().map(|&t| Some(t)));
        all.extend([&self.keys_watch, &self.settings_watch].into_iter().flatten().map(|w| Some(w.due())));
        all.extend(self.requests.iter().map(|(w, _)| Some(w.due())));
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
        if self.follow_at <= now {
            self.follow_at = now + Duration::from_secs(2);
            self.follow();
        }
        if self.keys_watch.as_mut().is_some_and(Watch::poll) {
            self.keys = exec::read_keys("explorer", &self.defaults);
        }
        if self.settings_watch.as_mut().is_some_and(Watch::poll) {
            self.settings_changed();
        }
        if self.requests.as_mut().is_some_and(|(w, _)| w.poll()) {
            self.request();
        }
    }

    // --- tree

    fn has(&self, rel: &str) -> bool {
        self.state.expanded.iter().any(|e| e == rel)
    }

    fn expand(&mut self, rel: &str) {
        if !self.has(rel) {
            self.state.expanded.push(rel.to_string());
        }
    }

    fn abs_of(&self, rel: &str) -> String {
        paths::join(&[&self.root, rel])
    }

    fn current(&self) -> Option<&Entry> {
        usize::try_from(self.state.cursor).ok().and_then(|i| self.state.rows.get(i))
    }

    /// The rows of the tree as the view shows them, the selection kept on its entry (else on the first match, or at
    /// the same place).
    fn build_rows(&mut self) {
        let query = self.state.filter.as_ref().map(|f| js::lower(&f.field.value));
        let expanded = self.state.expanded.clone();
        self.state.rows = self.tree.rows(query.as_deref(), self.state.changed, &expanded);
        let rows = &self.state.rows;
        // An entry within a compact folder row: that row.
        let at = self.state.selected.as_deref().and_then(|s| rows.iter().position(|r| holds(r, s)));
        let i = match at {
            Some(i) => i as i64,
            None if query.as_deref().is_some_and(|q| !q.is_empty()) => {
                rows.iter().position(|r| r.hit.is_some()).map_or(-1, |i| i as i64)
            }
            None => self.state.cursor,
        };
        self.select(i);
    }

    fn select(&mut self, i: i64) {
        self.state.cursor = i.min(self.state.rows.len() as i64 - 1).max(0);
        self.state.selected = self.current().map(|r| r.rel.clone());
    }

    fn toggle(&mut self, row: Option<Entry>, open: Option<bool>) {
        let Some(row) = row.filter(|r| r.dir) else { return };
        if self.state.filter.is_some() || self.state.changed {
            return;
        }
        let open = open.unwrap_or(!row.open);
        // Opened, a compact folder row unfolds each folder of its chain, as they show when the setting is off;
        // closed, it folds them all.
        let first = row.from.clone().filter(|f| !f.is_empty()).unwrap_or_else(|| row.rel.clone());
        let stop = parent_of(&first).map(String::from);
        let mut a = Some(row.rel.clone());
        while let Some(at) = a.filter(|a| !a.is_empty() && Some(a) != stop.as_ref()) {
            if open {
                self.expand(&at);
            } else {
                self.state.expanded.retain(|e| *e != at);
            }
            a = parent_of(&at).map(String::from);
        }
        self.build_rows();
        self.save();
    }

    /// Expands the folders above an entry and selects it.
    fn reveal_rel(&mut self, rel: &str) {
        let mut a = parent_of(rel).map(String::from);
        while let Some(at) = a.filter(|a| !a.is_empty()) {
            self.expand(&at);
            a = parent_of(&at).map(String::from);
        }
        self.state.selected = Some(rel.to_string());
        self.build_rows();
        self.reveal();
        self.save();
    }

    /// Expanded folders, per space folder, restored when the explorer is reopened.
    fn load(&mut self) {
        let Some(file) = &self.save_file else { return };
        let Some(all) = exec::read_text(file).ok().and_then(|t| json::parse(&t).ok()) else { return };
        let list: Vec<String> = match all.get(&self.root) {
            Some(Value::Arr(items)) => items.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            // A string: its characters, as for…of reads it.
            Some(Value::Str(s)) => s.chars().map(String::from).collect(),
            _ => Vec::new(),
        };
        for rel in list {
            self.expand(&rel);
        }
    }

    /// Shared by every explorer: written to a temporary file then renamed, so that another one never reads it
    /// half-written; left alone when unreadable rather than overwritten with this space only.
    fn save(&self) {
        let Some(file) = &self.save_file else { return };
        let mut all = match exec::read_text(file) {
            Ok(text) => match json::parse(&text) {
                // The JS threw on null or a value that is not an object, and wrote an array back as it was.
                Ok(v @ Value::Obj(_)) => v,
                _ => return,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Obj(Vec::new()),
            Err(_) => return,
        };
        let list = self.state.expanded.iter().filter(|e| !e.is_empty()).map(|e| Value::Str(e.clone())).collect();
        all.set(&self.root, Value::Arr(list));
        exec::write_atomic(file, json::stringify_pretty(&all, 1));
    }

    fn write_pane_state(&self) {
        exec::write_pane_state(&self.pane, &crate::obj! { "home" => self.home.clone(), "root" => self.root.clone() });
    }

    /// Another folder as the root of the tree: from the menu (Set as root), or its parent (Backspace).
    fn set_root(&mut self, dir: &str) {
        let dir = paths::resolve(&[dir]);
        if self.stopped || dir == self.root {
            return;
        }
        let from = self.root.clone();
        self.save();
        self.root = dir.clone();
        self.write_pane_state();
        self.announce();
        self.state.expanded = vec![String::new()];
        self.state.filter = None;
        self.state.top = 0;
        self.state.cursor = 0;
        self.state.selected = None;
        self.load();
        self.tree.reset(&dir);
        // Going up: the folder left is selected.
        let back = rel_of(&self.root, &from);
        if !back.is_empty() && !back.starts_with("..") && !paths::is_absolute(&back) {
            self.reveal_rel(&back);
        }
        self.watch_root();
        self.build_rows();
        self.render();
        self.init_git(AfterGit::Refresh);
    }

    // --- git

    /// Also called when a .git folder appears (git init, clone, archive extracted) in a folder that had none; then
    /// what follows (the JS's `.then`).
    fn init_git(&mut self, after: AfterGit) {
        if self.tree.git.top.is_some() || self.probing {
            return self.after_git(after);
        }
        self.probing = true;
        let root = self.root.clone();
        self.git_worker.send(Box::new(move || {
            let res = git_run(&root, &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--show-prefix"]);
            Box::new(move |ex: &mut Explorer| {
                ex.probing = false;
                if let Some(res) = res.filter(|_| !ex.stopped && root == ex.root) {
                    ex.found_git(&res);
                }
                ex.after_git(after);
            })
        }));
    }

    fn found_git(&mut self, res: &str) {
        let mut parts = res.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
        let (top, gitdir, prefix) = (parts.next().unwrap_or(""), parts.next(), parts.next().unwrap_or(""));
        self.tree.git.top = Some(top.to_string());
        self.tree.git.prefix = prefix.strip_suffix('/').unwrap_or(prefix).to_string();
        // The repository changes without touching the working tree (commit, stage, branch switch).
        self.git_watcher = None;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        // The JS's fs.watch(undefined) threw, and was caught.
        let Some(gitdir) = gitdir else { return };
        self.git_watcher = Watcher::new(gitdir, false, move |f| {
            if matches!(f.as_deref(), Some("index" | "HEAD")) {
                let _ = tx.send(Box::new(|ex: &mut Explorer| ex.schedule()));
                waker.wake();
            }
        })
        .ok();
    }

    fn after_git(&mut self, after: AfterGit) {
        match after {
            AfterGit::Refresh => self.refresh(),
            AfterGit::ScheduleIfTop if self.tree.git.top.is_some() => self.schedule(),
            AfterGit::ScheduleIfTop => {}
        }
    }

    // --- refresh

    /// At most one refresh every 250 ms: a file written continuously (a log) must not postpone it forever.
    fn schedule(&mut self) {
        if self.refresh_at.is_none() && !self.stopped {
            self.refresh_at = Some(Instant::now() + Duration::from_millis(250));
        }
    }

    fn refresh(&mut self) {
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
            return self.refreshed(None);
        }
        let root = self.root.clone();
        self.git_worker.send(Box::new(move || {
            let args = ["status", "--porcelain=v1", "-z", "--ignored=matching", "--ignore-submodules=all", "--", "."];
            let status = git_run(&root, &args);
            Box::new(move |ex: &mut Explorer| ex.refreshed(status))
        }));
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
            self.schedule();
        }
    }

    /// The root as the folder herdr gives the new spaces, tabs and panes made from this one: the launcher's.
    fn announce(&self) {
        screen::send(&crate::obj! { "cwd" => self.root.clone() });
    }

    /// Recursive, except on $HOME, one of its parents or the filesystem root: on Linux, one watch per folder of the
    /// tree, which would take seconds and could use up the system's limit. Top level only there, and also when a
    /// folder of the tree cannot be read (the recursive watch then fails); the 30 s scan covers the rest.
    fn watch_root(&mut self) {
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
                let at = Instant::now() + Duration::from_millis(500);
                if self.git_at.is_none() {
                    self.git_at = Some(at);
                } else {
                    self.git_more = Some(at);
                }
            }
            return;
        }
        if rel.contains("/.git/") || self.tree.hidden(&rel, &self.state.expanded) {
            return;
        }
        self.schedule();
    }

    // --- pane width

    /// A check of the width on the herdr worker; its result wakes the loop, which then looks at the next one due.
    fn keeper_job(&mut self, f: impl FnOnce() + Send + 'static) {
        self.keeper_busy = true;
        self.herdr.send(Box::new(move || {
            // The JS's finally: free again even when the check failed.
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
                let id = p.get("pane_id").map_or_else(|| "undefined".into(), js::string);
                exec::run(exec::herdr(), &["pane", "close", &id], &Opts::default());
            }
            exec::run(exec::herdr(), &["pane", "close", &pane], &Opts::default());
            Box::new(|_: &mut Explorer| {})
        }));
    }

    // --- following the terminal

    /// The folder of the terminal next to the explorer (that of its running program when herdr knows it), revealed
    /// in the tree when it changes; outside the root, the root changes. The setting is read each time, so that a
    /// change made in another explorer applies here too.
    fn follow(&mut self) {
        let follow = exec::read_settings().get("follow").cloned().unwrap_or(Value::Null);
        let on = follow.truthy();
        self.settings.set("follow", follow);
        // One look at a time: a slow herdr would otherwise fill the worker's queue, the preview and the copy waiting
        // behind (the JS's own awaits only held this loop).
        if !on || self.pane.is_empty() || self.follow_busy {
            return;
        }
        self.follow_busy = true;
        let pane = self.pane.clone();
        self.herdr.send(Box::new(move || {
            let look = || {
                let (target, _) = actions::terminal_of(&pane);
                let p = target.and_then(|t| {
                    let res = exec::result(exec::run(exec::herdr(), &["pane", "get", &t], &Opts::default()).as_deref());
                    res.get("pane").filter(|p| p.truthy()).cloned()
                });
                p.and_then(|p| {
                    ["foreground_cwd", "cwd"].iter().find_map(|k| p.get(k).filter(|v| v.truthy()).map(js::string))
                })
            };
            let cwd = std::panic::catch_unwind(look).ok().flatten();
            Box::new(move |ex: &mut Explorer| {
                ex.follow_busy = false;
                ex.followed(cwd)
            })
        }));
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
        if !inside(&cwd, &self.root) {
            if first {
                return;
            }
            if inside(&cwd, &self.home.clone()) {
                let home = self.home.clone();
                self.set_root(&home);
            } else if big(&cwd) {
                return;
            } else {
                self.set_root(&cwd);
            }
        }
        let rel = rel_of(&self.root, &cwd);
        if rel.is_empty() {
            return;
        }
        self.expand(&rel);
        self.reveal_rel(&rel);
        self.render();
    }

    /// The tab's request file changed: a file to reveal.
    fn request(&mut self) {
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
        if rel.is_empty() || rel.starts_with("..") || paths::is_absolute(&rel) {
            return;
        }
        self.reveal_rel(&rel_of(&self.root, target));
        self.render();
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
            // The JS threw reading null's icons.
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
            if js::strict_eq(Some(v), self.settings.get(name)) {
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

    // --- opening

    /// The changes of an entry: a file's in the preview's Diff view, a folder's in the Changes pane (the whole tab).
    fn view_changes(&self, r: &Entry) -> Act {
        let (abs, root, pane, dir) = (self.abs_of(&r.rel), self.root.clone(), self.pane.clone(), r.dir);
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
    fn open_file(&mut self, r: &Entry) {
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

    // --- prompts

    /// empty: an empty answer is accepted (otherwise it cancels), and the cursor stays at the end. inline: typed in
    /// the tree, on the row renamed or on a new row in its folder, rather than at the bottom.
    fn ask(
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

    /// After a change on disk: the new entry selected, its folder expanded.
    fn created(&mut self, abs: &str) {
        self.tree.cache.clear();
        let rel = rel_of(&self.root, abs);
        self.reveal_rel(&rel);
        self.render();
    }

    /// The new name typed at the top of its folder, opened for it.
    fn new_entry(&mut self, dir: &str, folder: bool) {
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

    fn rename_entry(&mut self, r: &Entry) {
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

    fn delete_entry(&mut self, r: &Entry) {
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
    fn discard_item(&self, abs: &str, title: &str, name: String, note: &str, action: &str) -> Option<It> {
        let (abs, root, title, note, action) =
            (abs.to_string(), self.root.clone(), title.to_string(), note.to_string(), action.to_string());
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
                let e = r.clone();
                items.push(It::new(
                    "View changes",
                    Rc::new(move |ex: &mut Explorer| ex.view_changes(&e)),
                    "viewChanges",
                ));
                if !r.dir {
                    let (a, root, p) = (abs.clone(), root.clone(), pane.clone());
                    items.push(It::new("File history", job(move || actions::scm(&root, &p, &a, true)), "fileHistory"));
                }
                let (a, rt) = (abs.clone(), root.clone());
                let stage = job(move || Some(actions::stage(&a, &rt)));
                let (a, rt) = (abs.clone(), root.clone());
                let unstage = job(move || Some(actions::unstage(&a, &rt)));
                items.extend([None, It::new("Stage", stage, ""), It::new("Unstage", unstage, "")]);
                let name = format!("{}{}", r.name, if r.dir { "/" } else { "" });
                items.push(self.discard_item(&abs, "Restore it?", name, "Its changes are discarded.", "Restore"));
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
                let duplicate = Rc::new(move |ex: &mut Explorer| match actions::duplicate(&a) {
                    (_, Some(err)) => Act::Now(Some(err)),
                    (to, None) => {
                        ex.created(&to.unwrap_or_default());
                        Act::Now(None)
                    }
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
                if self.tree.git.top.is_some() && deco != Some('!') {
                    let changed = deco.is_some();
                    items.push(None);
                    if changed {
                        let e = r.clone();
                        let run = Rc::new(move |ex: &mut Explorer| ex.view_changes(&e));
                        items.push(It::new("View changes", run, "viewChanges"));
                    }
                    if !r.dir && deco != Some('U') {
                        let (a, rt, p) = (abs.clone(), root.clone(), pane.clone());
                        items.push(It::new(
                            "File history",
                            job(move || actions::scm(&rt, &p, &a, true)),
                            "fileHistory",
                        ));
                    }
                    if changed {
                        let (a, rt) = (abs.clone(), root.clone());
                        items.push(It::new("Stage", job(move || Some(actions::stage(&a, &rt))), ""));
                    }
                    let (a, rt) = (abs.clone(), root.clone());
                    items.push(It::new("Unstage", job(move || Some(actions::unstage(&a, &rt))), ""));
                    if changed {
                        let note = "This cannot be undone.";
                        items.push(self.discard_item(&abs, "Discard the changes?", r.name.clone(), note, "Discard"));
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
                if self.tree.git.top.is_some() {
                    let (rt, p) = (root.clone(), pane.clone());
                    items.push(It::new(
                        "Source control",
                        job(move || actions::scm(&rt, &p, "", false)),
                        "sourceControl",
                    ));
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

    fn open_menu(&mut self, i: Option<i64>) {
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
    /// when it panicked, as the JS caught a rejection).
    fn run_action(&mut self, action: impl FnOnce(&mut Explorer) -> Act) {
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

    fn flash(&mut self, text: Option<String>) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return self.render() };
        let time = ui::message_time(ui::message_kind(&text));
        self.state.flash = Some(text);
        self.render();
        self.flash_at = Some(Instant::now() + Duration::from_millis(time));
    }

    fn reveal(&mut self) {
        let list_h = self.body_height();
        if self.state.cursor < self.state.top {
            self.state.top = self.state.cursor;
        }
        if self.state.cursor >= self.state.top + list_h {
            self.state.top = self.state.cursor - list_h + 1;
        }
    }

    /// The terminal's size: columns and rows, 32 and 24 when unknown.
    fn size(&self) -> (usize, usize) {
        // `out.columns || WIDTH`: a size of 0, as a pty gets before its first resize, is unknown too.
        sys::size().map_or((WIDTH, 24), |(w, h)| {
            (if w == 0 { WIDTH } else { w as usize }, if h == 0 { 24 } else { h as usize })
        })
    }

    // --- settings

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

    // --- key help

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

    /// The changed files only, or the whole tree again with the selected entry shown.
    fn show_changed(&mut self, on: bool) {
        if on && self.tree.git.top.is_none() {
            return self.flash(Some("Not a git repository".into()));
        }
        let sel = self.state.selected.clone();
        self.state.changed = on;
        self.state.top = 0;
        match sel {
            Some(sel) if !on => self.reveal_rel(&sel),
            _ => {
                self.build_rows();
                self.reveal();
            }
        }
        self.render();
    }

    /// ] and [: the next or previous changed file after the selected entry, unfolded, selected and previewed; past
    /// the last one, back to the first.
    fn next_change(&mut self, dir: i64) {
        if self.tree.git.top.is_none() {
            return self.flash(Some("Not a git repository".into()));
        }
        let list = self.tree.changed_files();
        if list.is_empty() {
            return self.flash(Some("No changes".into()));
        }
        let i = match self.current() {
            None if dir > 0 => 0,
            None => list.len() - 1,
            Some(r) if dir > 0 => list.iter().position(|f| tree_before(&r.rel, r.dir, f, false)).unwrap_or(0),
            Some(r) => list.iter().rposition(|f| tree_before(f, false, &r.rel, r.dir)).unwrap_or(list.len() - 1),
        };
        let rel = list[i].clone();
        if !self.tree.git.gone.contains(&rel) {
            self.reveal_rel(&rel);
        } else {
            // A deleted file shows only among the changed files, every folder unfolded there.
            self.state.changed = true;
            self.state.selected = Some(rel.clone());
            self.build_rows();
            self.reveal();
        }
        self.render();
        let (abs, root, pane) = (self.abs_of(&rel), self.root.clone(), self.pane.clone());
        // Among the changed files, and for a deleted one, the preview shows the changes.
        if self.state.changed {
            let shown = actions::Shown { view: "diff".into(), ..Default::default() };
            self.run_action(move |_| later(move || actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)));
        } else if actions::can_preview(&abs) {
            let shown = actions::Shown::default();
            self.run_action(move |_| later(move || actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)));
        }
    }
}

/// What follows a look for the repository.
#[derive(Clone, Copy)]
enum AfterGit {
    Refresh,
    /// A .git folder appeared: a refresh once the repository is found.
    ScheduleIfTop,
}

/// git in the root, the repository's config read again before each call, as it may change while the explorer runs.
fn git_run(root: &str, args: &[&str]) -> Option<String> {
    exec::run(exec::git(), args, &Opts { cwd: Some(root.into()), env: Some(exec::git_env(root)), ..Default::default() })
}

/// Alone in its tab by the pane list too: the layout alone can show a single pane while the tab has others (a pane
/// zoomed over the explorer, a client attaching), and closing then lost the explorer.
fn alone(pane: &str) -> bool {
    let ask = |args: &[&str]| exec::result(exec::run(exec::herdr(), args, &Opts::default()).as_deref());
    let Some(me) = ask(&["pane", "get", pane]).get("pane").filter(|p| p.truthy()).cloned() else { return false };
    let ws = me.get("workspace_id").map_or_else(|| "undefined".into(), js::string);
    let all = ask(&["pane", "list", "--workspace", &ws]);
    let Some(all) = all.get("panes").filter(|p| p.truthy()) else { return false };
    let mine = all.as_array().into_iter().flatten().filter(|p| js::strict_eq(p.get("tab_id"), me.get("tab_id")));
    mine.count() == 1
}
