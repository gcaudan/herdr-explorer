//! File preview of a tab, opened next to the terminal by the explorer and the search:
//! syntax-highlighted text, rendered Markdown, images (in pixels with the Kitty graphics protocol, else drawn with half
//! blocks), a hex dump for other binary files; or the file's git changes (Diff view, d). Reloads by itself when the
//! file changes on disk, and shows the next file the explorer asks for (request file, see actions.preview), in tabs
//! (core::tabs); the tab's review notes show in the text.
//!
//! As in the explorer, the herdr and clipboard calls run on a worker thread and their results come back to the
//! pane's loop; timers are deadlines of that loop.
//!
//! Updated plugin (core::update): the preview starts again on the new code, on the same file and line.

mod diff;
mod edit;
mod input;
mod media;
mod render;
mod review;
mod state;

pub use state::{Chip, Find, Kind, Mark, Opt, Pos, Row, Rows, Select, State, TabView};
use state::{PaneTabs, clean_marks};

use crate::core::actions::{self, Kept};
use crate::core::exec;
use crate::core::scope::{self, Scope};
use crate::core::screen::{self, Input, Screen};
use crate::core::tabs::{self, Tabs};
use crate::core::ui::{self, Scrollbar, c, fg};
use crate::core::watch::Watch;
use crate::core::{paths, update, worker};
use crate::json::{self, Value};
use crate::sys::{self, Watcher};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Larger files: the start only.
const MAX_BYTES: u64 = 8 << 20;
/// Larger files: plain text.
const MAX_HIGHLIGHT: usize = 1 << 20;
const HEX_BYTES: usize = 64 << 10;

pub type Then = worker::Then<Preview>;
type Job = worker::Job<Preview>;
type Worker = worker::Worker<Preview>;

/// The width kept: None at first, the width herdr gave the pane.
struct Width(Arc<Mutex<Option<f64>>>);

impl Kept for Width {
    fn columns(&self) -> f64 {
        sys::size().map_or(80.0, |(w, _)| if w == 0 { 80.0 } else { f64::from(w) })
    }

    fn get(&self) -> Option<f64> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set(&mut self, width: f64) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(width);
    }
}

/// The pane: its state, the tabs, and the deadlines of its timers.
pub struct Preview {
    pane: String,
    tab: String,
    /// The file of the environment had its tab saved before a restart: shown at its scroll.
    restored: bool,
    settings: Value,
    state: State,
    tabs: PaneTabs,
    strip_first: i64,
    scrollbar: Scrollbar,
    kitty: media::Kitty,
    /// The images of rendered Markdown, drawn.
    pictures: media::Pictures,
    screen: Screen,
    tx: Sender<Then>,
    rx: Receiver<Then>,
    waker: sys::Waker,
    herdr: Worker,
    /// git's diffs and line changes, apart from herdr's calls.
    git: Worker,
    /// Beside a review's source control: q closes, whatever it shows.
    review: bool,
    // run counters: an answer of git that another call took the place of is dropped
    diff_run: u64,
    changes_run: u64,
    show_run: u64,
    /// The commits shown so far, each one's id.
    shows: u64,
    /// The changes set so far, for the scrollbar's marks kept (change_marks).
    changes_seen: u64,
    change_marks: Option<(Rc<Rows>, u64, Vec<ui::BarMark>)>,
    /// While stage::apply runs: a second s, u or D would apply the same rows again.
    editing: bool,
    /// The tab's scope file, chosen in another pane.
    scope_watch: Option<Watch>,
    /// The tab's notes, taken or sent in another pane.
    notes_watch: Option<Watch>,
    // width
    width: Arc<Mutex<Option<f64>>>,
    keeper: Arc<Mutex<actions::WidthKeeper>>,
    keeper_busy: bool,
    // timers
    flash_at: Option<Instant>,
    settle_at: Option<Instant>,
    reload_at: Option<Instant>,
    // watching
    watcher: Option<Watcher>,
    /// The tab's request file, watched, and the time of the request taken last.
    requests: Option<Watch>,
    seen: f64,
    settings_watch: Option<Watch>,
    /// The version on disk, watched once started.
    version_watch: Option<update::VersionWatch>,
    layout_seen: String,
    // clicks
    last_click: (Option<Instant>, i64, i64, u32),
    last_tab: (Option<Instant>, Option<usize>),
    /// What save_file wrote or would write now, for the exit hook: an ending signal exits from the screen's wait.
    snapshot: Arc<Mutex<Option<Value>>>,
    /// A drag selecting text while editing.
    edit_drag: bool,
    /// The text copied last, for ctrl+v when the system's clipboard cannot be read.
    clip: String,
}

/// file inside dir (or dir itself), by their paths.
fn within(file: &str, dir: &str) -> bool {
    let r = paths::relative(dir, file);
    r != ".." && !r.starts_with(&format!("..{}", paths::SEP)) && !paths::is_absolute(&r)
}

fn ext(file: &str) -> String {
    paths::extname(file).get(1..).unwrap_or("").to_lowercase()
}

fn is_markdown(file: &str) -> bool {
    matches!(ext(file).as_str(), "md" | "markdown" | "mdx" | "mkd")
}

fn size(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1 << 20 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / f64::from(1 << 20))
    }
}

fn hexdump(buf: &[u8]) -> Vec<String> {
    buf.chunks(16)
        .enumerate()
        .map(|(k, row)| {
            let hex = row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
            let ascii: String = row.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
            format!("{:08x}  {hex:<47}  {ascii}", k * 16)
        })
        .collect()
}

/// `+v || 0`, for a field of a request or a saved tab.
fn num(v: Option<&Value>) -> f64 {
    let n = crate::json::num(v);
    if n.is_nan() { 0.0 } else { n }
}

/// The tabs saved before a restart (EXPLORER_TABS), { list, current }.
fn saved_tabs(text: Option<String>) -> PaneTabs {
    let empty = Tabs { list: Vec::new(), current: 0 };
    let Some(t) = json::parse(text.as_deref().unwrap_or("null")).ok() else { return empty };
    let list: Vec<tabs::Tab<Option<TabView>>> = t
        .get("list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|x| x.get("file").and_then(Value::as_str).is_some_and(|f| !f.is_empty()))
        .take(tabs::MAX)
        .map(|x| tabs::Tab {
            file: x.get("file").and_then(Value::as_str).unwrap_or("").to_string(),
            kept: x.get("kept").is_some_and(Value::truthy),
            extra: Some(TabView {
                root: x.get("root").and_then(Value::as_str).unwrap_or("").to_string(),
                line: num(x.get("line")),
                top: num(x.get("top")) as i64,
                left: num(x.get("left")) as i64,
                diff: x.get("view").and_then(Value::as_str) == Some("diff"),
                marks: clean_marks(x.get("marks")),
                mark: 0,
                raw: x.get("raw").is_some_and(Value::truthy),
                edit: None,
            }),
        })
        .collect();
    // A fractional index is cut to a whole one.
    let current = num(t.get("current")).min(list.len() as f64 - 1.0).max(0.0) as usize;
    Tabs { list, current }
}

/// The modification time of a file in milliseconds, 0 when missing (fs.Stats' mtimeMs).
fn mtime_ms(file: &str) -> f64 {
    std::fs::metadata(file).ok().and_then(|m| m.modified().ok()).map_or(0.0, crate::core::clock::ms)
}

/// Runs the pane: the process's exit code.
pub fn run(_args: &[String]) -> i32 {
    worker::quiet_panics();
    let mut p = match Preview::new() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("herdr-explorer: {e}");
            return 1;
        }
    };
    p.start();
    p.run()
}

impl Preview {
    fn new() -> std::io::Result<Preview> {
        let env = |k: &str| exec::env_var(k);
        update::init();
        let pane = env("HERDR_PANE_ID").unwrap_or_default();
        let tab = env("EXPLORER_TAB").unwrap_or_default();
        ui::init_theme();
        let settings = exec::read_settings();
        ui::use_glyphs(settings.get("glyphs").and_then(Value::as_str).unwrap_or(""));
        let file = env("EXPLORER_FILE").unwrap_or_default();
        let mut tabs = saved_tabs(env("EXPLORER_TABS"));
        // The file of the environment in its tab; restored: that tab was saved, with its scroll (see start).
        let mut fresh = true;
        if !file.is_empty() {
            let opened = tabs::open(&tabs, &file, env("EXPLORER_KEEP").as_deref() == Some("1"), |_| false);
            fresh = opened.fresh;
            tabs = opened.tabs;
        }
        let restored = (!file.is_empty() && !fresh).then(|| tabs.list[tabs.current].extra.clone()).flatten();
        let marks = env("EXPLORER_MARKS").map_or(Ok(Value::Arr(Vec::new())), |m| json::parse(&m));
        let screen = Screen::open(screen::Options { mouse: true, motion: true, focus: true })?;
        let waker = screen.waker();
        let (tx, rx) = channel();
        let herdr = Worker::new(tx.clone(), waker.clone());
        let git = Worker::new(tx.clone(), waker.clone());
        let mut p = Preview {
            restored: false,
            state: State {
                file,
                root: env("EXPLORER_ROOT").unwrap_or_default(),
                line: num(env("EXPLORER_LINE").map(Value::Str).as_ref()),
                kind: Kind::Text,
                info: String::new(),
                text: String::new(),
                source: Vec::new(),
                plain: Vec::new(),
                buffer: None,
                converting: false,
                missing: None,
                sharp: None,
                raw: false,
                rows: None,
                top: 0,
                left: 0,
                focused: true,
                find: None,
                matches: Vec::new(),
                matched: -1,
                select: None,
                flash: None,
                chips: Vec::new(),
                marks: clean_marks(marks.ok().as_ref()),
                mark: 0,
                diff: env("EXPLORER_VIEW").as_deref() == Some("diff"),
                diff_data: None,
                scope: if tab.is_empty() { Scope::head() } else { scope::read(&tab) },
                rev_prompt: None,
                hunk: None,
                want_top: None,
                show: None,
                changes: None,
                menu: None,
                help: None,
                hints: None,
                edit: None,
                ask: None,
                notes: Vec::new(),
                note_count: 0,
                note: None,
            },
            settings_watch: exec::settings_file().map(|f| Watch::new(&f, Duration::from_millis(1000))),
            version_watch: None,
            layout_seen: json::stringify(exec::read_settings().get("diffLayout").unwrap_or(&Value::Null)),
            settings,
            tabs,
            strip_first: 0,
            scrollbar: Scrollbar::new(),
            kitty: media::Kitty::new(),
            pictures: media::Pictures::default(),
            screen,
            tx,
            rx,
            waker,
            herdr,
            git,
            review: env("EXPLORER_REVIEW").is_some_and(|r| !r.is_empty()),
            diff_run: 0,
            changes_run: 0,
            show_run: 0,
            shows: 0,
            changes_seen: 0,
            change_marks: None,
            editing: false,
            scope_watch: if tab.is_empty() { None } else { scope::watch(&tab) },
            notes_watch: if tab.is_empty() { None } else { crate::core::notes::watch(&tab) },
            width: Arc::new(Mutex::new(None)),
            keeper: Arc::new(Mutex::new(actions::WidthKeeper::new(&pane, None))),
            keeper_busy: false,
            flash_at: None,
            settle_at: None,
            reload_at: None,
            watcher: None,
            requests: None,
            seen: 0.0,
            last_click: (None, 0, 0, 0),
            last_tab: (None, None),
            snapshot: Arc::new(Mutex::new(None)),
            edit_drag: false,
            clip: String::new(),
            pane,
            tab,
        };
        if let Some(r) = &restored {
            p.state.top = r.top;
            p.state.left = r.left;
            p.state.raw = r.raw;
            if p.state.diff {
                p.state.want_top = Some(r.top);
            }
        }
        p.restored = restored.is_some();
        Ok(p)
    }

    /// What the pane does first, once the screen is set up.
    fn start(&mut self) {
        let restored = self.restored;
        // The tabs saved at exit, a signal included.
        let (pane, snapshot, images) = (self.pane.clone(), self.snapshot.clone(), self.kitty.exit.clone());
        exec::at_exit(move || {
            if let Some(data) = snapshot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                exec::write_pane_state(&pane, &data);
            }
            let images = std::mem::take(&mut *images.lock().unwrap_or_else(|e| e.into_inner()));
            if !images.is_empty() {
                let _ = sys::write_stdout(images.as_bytes());
            }
            if edit::CURSOR_SHOWN.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = sys::write_stdout(b"\x1b[0 q");
            }
        });
        self.check_width(false);
        // The Kitty graphics query, and the size of a cell in pixels.
        let _ = sys::write_stdout(format!("{}\x1b[16t", ui::KITTY_QUERY).as_bytes());
        self.settle_at = Some(Instant::now() + Duration::from_millis(500));
        // The explorer and the search ask for another file through the request file. After a restart on new code,
        // seen is the previous process's, and on opening that of the request file when the explorer opened the
        // pane: a request made while the pane changed hands or started is still taken, once the file it showed is
        // loaded.
        let handed = exec::env_var("EXPLORER_SEEN");
        self.seen = num(handed.clone().map(Value::Str).as_ref());
        let request = if self.tab.is_empty() { None } else { actions::request_file(&self.tab) };
        if let Some(file) = &request {
            if handed.is_none() {
                self.seen = mtime_ms(file);
            }
            self.requests = Some(Watch::new(file, Duration::from_millis(200)));
        }
        if self.state.file.is_empty() {
            self.set_error("No file");
        } else {
            self.read_notes(true);
            self.watch();
            self.load(restored);
            self.show_marks();
            self.save_file();
            if !restored && self.state.line != 0.0 && !self.state.diff {
                self.state.top = (self.row_of(self.state.line - 1.0) - self.third()).max(0);
                self.render();
            }
        }
        // Opened, or restarted, on a commit of source control.
        if let Some(show) = exec::env_var("EXPLORER_SHOW").and_then(|t| json::parse(&t).ok()) {
            self.enter_show(Some(&show));
        }
        // setImmediate: the request of a pane that changed hands, taken once the file it showed is loaded.
        if handed.is_some() && request.as_deref().is_some_and(|f| std::fs::metadata(f).is_ok()) {
            self.take();
        }
        // Updated plugin: restarted on the new code once no search is typed nor selection dragged; never while a tab
        // is edited, its text would be lost.
        self.version_watch = Some(update::VersionWatch::new());
    }

    fn idle_for_update(&self) -> bool {
        let s = &self.state;
        !s.find.as_ref().is_some_and(|f| f.active)
            && s.note.is_none()
            && !s.select.as_ref().is_some_and(|x| !x.done)
            && self.edits().is_empty()
    }

    /// This process stops watching, the new one taking over the pane, at the same file, line, marks, view, tabs and
    /// commit shown; its images deleted first, the new one sending its own.
    fn restart_on_update(&mut self) {
        self.save_file();
        self.reload_at = None;
        let images = std::mem::take(&mut *self.kitty.exit.lock().unwrap_or_else(|e| e.into_inner()));
        if !images.is_empty() {
            let _ = sys::write_stdout(images.as_bytes());
        }
        self.watcher = None;
        self.requests = None;
        self.settings_watch = None;
        let tabs = json::stringify(&self.saved_tabs());
        let s = &self.state;
        let marks = s
            .marks
            .iter()
            .map(|m| crate::obj! { "from" => m.from as f64, "to" => m.to as f64, "a" => m.a as f64, "b" => m.b as f64 })
            .collect();
        let show = s.show.as_ref().map_or(String::new(), |x| json::stringify(&x.what));
        let env: Vec<(String, String)> = [
            ("EXPLORER_FILE", s.file.clone()),
            ("EXPLORER_ROOT", s.root.clone()),
            ("EXPLORER_LINE", json::number(s.line)),
            ("EXPLORER_SEEN", json::number(self.seen)),
            ("EXPLORER_MARKS", json::stringify(&Value::Arr(marks))),
            ("EXPLORER_VIEW", if s.diff { "diff" } else { "file" }.into()),
            ("EXPLORER_TABS", tabs),
            ("EXPLORER_SHOW", show),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        self.screen.restart(&env);
    }

    /// The loop: the screen's events, the worker's results and the timers, until the pane is closed.
    fn run(&mut self) -> i32 {
        loop {
            let timeout = self.next_deadline().map(|t| t.saturating_duration_since(Instant::now()));
            for input in self.screen.wait(timeout) {
                match input {
                    Input::Ui(e) => self.event(e),
                    Input::Resize => {
                        self.state.rows = None;
                        self.render();
                        self.check_width(false);
                    }
                    // The highlighted source too is in the theme's colours: read again.
                    Input::Theme => {
                        self.state.rows = None;
                        if let Some(e) = &self.state.edit {
                            e.0.borrow_mut().forget_colours();
                        }
                        if self.state.file.is_empty() {
                            self.render();
                        } else {
                            self.load(true);
                        }
                    }
                    Input::Wake => {}
                }
                self.results();
            }
            self.results();
            self.timers();
            let data = self.save_data();
            *self.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(data);
        }
    }

    fn results(&mut self) {
        while let Ok(then) = self.rx.try_recv() {
            then(self);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        let mut all = vec![self.flash_at, self.settle_at, self.reload_at, self.scrollbar.due()];
        all.extend(
            [&self.requests, &self.settings_watch, &self.scope_watch, &self.notes_watch]
                .into_iter()
                .flatten()
                .map(Watch::due),
        );
        all.extend(self.version_watch.iter().map(update::VersionWatch::due));
        if !self.keeper_busy {
            all.push(self.keeper.try_lock().ok().and_then(|k| k.due()));
        }
        all.into_iter().flatten().min()
    }

    fn timers(&mut self) {
        let now = Instant::now();
        let due = |t: &mut Option<Instant>| t.is_some_and(|t| t <= now) && t.take().is_some();
        if due(&mut self.flash_at) {
            self.state.flash = None;
            self.render();
        }
        if due(&mut self.settle_at) && self.kitty.probing {
            self.kitty.probing = false;
            self.state.rows = None;
            self.render();
        }
        if due(&mut self.reload_at) {
            // The notes follow their lines (notes::current), the file's own reloaded with it.
            self.read_notes(true);
            self.load(true);
        }
        if let Some(top) = self.scrollbar.tick(now) {
            self.state.top = top;
            self.render();
        }
        if !self.keeper_busy && self.keeper.try_lock().ok().and_then(|k| k.due()).is_some_and(|t| t <= now) {
            let (keeper, width) = (self.keeper.clone(), self.width.clone());
            self.keeper_job(move || {
                let mut k = keeper.lock().unwrap_or_else(|p| p.into_inner());
                k.tick(Instant::now(), &mut Width(width));
            });
        }
        if self.requests.as_mut().is_some_and(Watch::poll) {
            self.take();
        }
        if self.settings_watch.as_mut().is_some_and(Watch::poll) {
            self.settings_changed();
        }
        if self.scope_watch.as_mut().is_some_and(Watch::poll) {
            self.follow_scope();
        }
        if self.notes_watch.as_mut().is_some_and(Watch::poll) {
            self.load_notes();
        }
        let idle = self.idle_for_update();
        if self.version_watch.as_mut().is_some_and(|v| v.poll(|| idle)) {
            self.restart_on_update();
        }
    }

    /// The terminal's size: columns and rows, 80 and 24 when unknown.
    fn size(&self) -> (usize, usize) {
        sys::size()
            .map_or((80, 24), |(w, h)| (if w == 0 { 80 } else { w as usize }, if h == 0 { 24 } else { h as usize }))
    }

    /// A third of the body, where n, N and a line asked for put their row.
    fn third(&self) -> i64 {
        (self.size().1 as i64 - 2).div_euclid(3)
    }

    // --- tabs

    /// The tab shown's state put away in its tab.
    fn stash(&mut self) {
        let s = &self.state;
        let Some(t) = self.tabs.list.get_mut(self.tabs.current).filter(|t| t.file == s.file) else { return };
        t.extra = Some(TabView {
            root: s.root.clone(),
            line: s.line,
            top: s.top,
            left: s.left,
            diff: s.diff,
            marks: s.marks.clone(),
            mark: s.mark,
            raw: s.raw,
            edit: s.edit.clone(),
        });
    }

    /// The tab shown's state taken back: the defaults for a tab never shown, the root staying.
    fn unstash(&mut self) {
        let t = &self.tabs.list[self.tabs.current];
        let v = t.extra.clone().unwrap_or_else(|| TabView { root: self.state.root.clone(), ..TabView::default() });
        let s = &mut self.state;
        s.file = t.file.clone();
        s.root = v.root;
        s.line = v.line;
        s.top = v.top;
        s.left = v.left;
        s.diff = v.diff;
        s.marks = v.marks;
        s.mark = v.mark;
        s.raw = v.raw;
        s.edit = v.edit;
    }

    /// The tab i shown as it was left: its file loaded again, at its scroll, in its view.
    fn show_tab(&mut self, i: usize) {
        if i == self.tabs.current || i >= self.tabs.list.len() {
            return self.render();
        }
        self.stash();
        self.tabs.current = i;
        self.enter_tab();
    }

    fn enter_tab(&mut self) {
        self.unstash();
        let s = &mut self.state;
        s.diff_data = None;
        s.find = None;
        s.matches.clear();
        s.note = None;
        s.select = None;
        s.menu = None;
        if s.diff {
            s.want_top = Some(s.top);
        }
        self.watch();
        self.read_notes(true);
        self.load(true);
        self.save_file();
    }

    /// t, a double-click on the tab, an edit or a note: the tab kept. Returns whether it was temporary.
    fn keep_tab(&mut self, i: usize) -> bool {
        if self.tabs.list.get(i).is_none_or(|t| t.kept) {
            return false;
        }
        self.tabs = tabs::keep(&self.tabs, i);
        self.save_file();
        true
    }

    /// x, a click on its × or a middle click: the tab closed. The last one closes the preview, as q does.
    fn close_tab(&mut self, i: usize) {
        if self.tabs.list.len() < 2 {
            return self.quit();
        }
        if i >= self.tabs.list.len() {
            return;
        }
        let (edit, file) = (self.edit_at(i), self.tabs.list[i].file.clone());
        self.unsaved(
            edit,
            file,
            Box::new(move |p: &mut Preview| {
                let was = p.tabs.current;
                p.stash();
                p.tabs = tabs::close(&p.tabs, i);
                if i == was {
                    return p.enter_tab();
                }
                p.save_file();
                p.render();
            }),
        );
    }

    /// The tabs, with the scroll and view of each (the marks of the tab shown only: EXPLORER_MARKS), for the next
    /// process.
    fn saved_tabs(&mut self) -> Value {
        self.stash();
        let list = self
            .tabs
            .list
            .iter()
            .map(|t| {
                let mut o = crate::obj! { "file" => t.file.clone(), "kept" => t.kept };
                if let Some(v) = &t.extra {
                    o.set("root", Value::Str(v.root.clone()));
                    o.set("line", Value::Num(v.line));
                    o.set("top", Value::Num(v.top as f64));
                    o.set("left", Value::Num(v.left as f64));
                    o.set("view", Value::Str(if v.diff { "diff" } else { "file" }.into()));
                    o.set("raw", Value::Bool(v.raw));
                }
                o
            })
            .collect();
        crate::obj! { "current" => self.tabs.current as f64, "list" => Value::Arr(list) }
    }

    /// The file shown and the tabs, reopened by the startup hook after a herdr restart.
    fn save_file(&mut self) {
        let data = self.save_data();
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(data.clone());
        exec::write_pane_state(&self.pane, &data);
    }

    fn save_data(&mut self) -> Value {
        let tabs = self.saved_tabs();
        let s = &self.state;
        crate::obj! {
            "file" => s.file.clone(), "root" => s.root.clone(), "line" => Value::Num(s.line), "tabs" => tabs,
        }
    }

    // --- requests

    /// A request of the explorer or the search: a file shown in its tab, the temporary one, or a new one.
    fn take(&mut self) {
        let Some(file) = self.requests.as_ref().map(|w| w.file().to_string()) else { return };
        let t = mtime_ms(&file);
        if t == 0.0 || t == self.seen {
            return;
        }
        self.seen = t;
        let Some(req) = exec::read_text(&file).ok().and_then(|t| json::parse(&t).ok()) else { return };
        if let Some(show) = req.get("show").filter(|s| s.truthy()) {
            return self.enter_show(Some(show));
        }
        // A file that is not a string: left alone.
        let Some(asked) = req.get("file").filter(|f| f.truthy()).and_then(Value::as_str).map(String::from) else {
            return;
        };
        self.drop_show();
        // same: a tab that already showed the file, as it was left; other: the pane shows another file than before.
        // A question of the tab shown is dropped, as canceled: its answer would go to another tab.
        self.drop_ask();
        let (prev, was) = (self.state.file.clone(), self.tabs.current);
        self.stash();
        let busy =
            |t: &tabs::Tab<Option<TabView>>| t.extra.as_ref().and_then(|v| v.edit.as_ref()).is_some_and(|e| e.dirty());
        let opened = tabs::open(&self.tabs, &asked, req.get("keep") == Some(&Value::Bool(true)), busy);
        self.tabs = opened.tabs;
        if opened.fresh || self.tabs.current != was {
            self.unstash();
        }
        let same = !opened.fresh;
        let other = self.state.file != prev || opened.fresh;
        let diff = req.get("view").and_then(Value::as_str) == Some("diff");
        let keep = same && diff == self.state.diff;
        let s = &mut self.state;
        if let Some(root) = req.get("root").filter(|r| r.truthy()).and_then(Value::as_str) {
            s.root = root.to_string();
        }
        s.line = num(req.get("line"));
        s.marks = clean_marks(req.get("marks"));
        s.raw = same && s.raw;
        if other || diff != s.diff {
            s.diff_data = None;
            s.find = None;
            s.matches.clear();
        }
        s.diff = diff;
        // A tab being edited stays in its edit mode, its cursor on the line asked for.
        if let Some(e) = &s.edit {
            s.diff = false;
            if s.line != 0.0 {
                let mut e = e.0.borrow_mut();
                e.ed.goto(s.line as usize);
                e.reveal = true;
            }
        }
        if other {
            s.find = None;
            s.note = None;
            s.select = None;
            s.menu = None;
            self.watch();
            self.read_notes(true);
        }
        if keep && diff && self.state.diff_data.is_none() {
            self.state.want_top = Some(self.state.top);
        }
        let keep_scroll = keep && self.state.line == 0.0 && self.state.marks.is_empty();
        self.load(keep_scroll);
        self.show_marks();
        self.save_file();
        if self.state.line != 0.0 && !self.state.diff && self.state.edit.is_none() {
            self.state.top = (self.row_of(self.state.line - 1.0) - self.third()).max(0);
            self.render();
        }
    }

    /// Changes on disk, by an agent or an editor: reloaded, keeping the position. The folder is watched rather than
    /// the file, which editors and agents often replace by renaming a new one over it.
    fn watch(&mut self) {
        self.watcher = None;
        let base = paths::basename(&self.state.file, None);
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        let watched = Watcher::new(&paths::dirname(&self.state.file), false, move |f| {
            if f.is_some_and(|f| f != base) {
                return;
            }
            let _ = tx.send(Box::new(|p: &mut Preview| {
                p.reload_at = Some(Instant::now() + Duration::from_millis(80));
            }));
            waker.wake();
        });
        self.watcher = watched.ok();
    }

    /// Wrap lines and the diff layout chosen in another pane (another preview, the Changes pane, source control):
    /// shown at once. The Images setting changed: the images drawn again as it says.
    fn settings_changed(&mut self) {
        let saved = exec::read_settings();
        let layout = json::stringify(saved.get("diffLayout").unwrap_or(&Value::Null));
        let wrap = saved.get("wrap").is_some_and(Value::truthy);
        if wrap != self.settings.get("wrap").is_some_and(Value::truthy) {
            self.settings.set("wrap", Value::Bool(wrap));
            self.state.left = 0;
            self.state.rows = None;
            self.render();
        } else if layout != self.layout_seen && self.state.diff {
            self.state.rows = None;
            self.render();
        }
        self.layout_seen = layout;
        if saved.get("images") == self.settings.get("images") {
            return;
        }
        self.settings.set("images", saved.get("images").cloned().unwrap_or(Value::Null));
        self.state.sharp = None;
        self.state.rows = None;
        self.render();
    }

    // --- loading

    fn load(&mut self, keep_scroll: bool) {
        let file = self.state.file.clone();
        let root = &self.state.root;
        let top = if !root.is_empty() && within(&file, root) { root.clone() } else { String::new() };
        // Through a link to a network share, the read would send the user's credentials (see remote_link).
        if exec::remote_link(&file, &top) {
            return self.set_error("Link to a network share: not opened");
        }
        // An image is read whole, up to the decoder's limit and one byte more, so that a larger one is reported as too
        // large rather than decoded from its start.
        let picture = crate::render::image::FORMATS.contains(&ext(&file).as_str());
        let limit = if picture { crate::render::image::MAX_BYTES as u64 + 1 } else { MAX_BYTES };
        let read = || -> Result<Result<(Vec<u8>, u64), &'static str>, std::io::Error> {
            let stat = std::fs::metadata(&file)?;
            if stat.is_dir() {
                return Ok(Err("Folder"));
            }
            // A named pipe or a device would block the read, and the whole pane with it.
            if !stat.is_file() {
                return Ok(Err("Not a regular file"));
            }
            let mut buf = Vec::new();
            use std::io::Read;
            std::fs::File::open(&file)?.take(stat.len().min(limit)).read_to_end(&mut buf)?;
            Ok(Ok((buf, stat.len())))
        };
        let (buf, len) = match read() {
            Ok(Ok(r)) => r,
            Ok(Err(text)) => return self.set_error(text),
            Err(e) => {
                let code = exec::io_code(&e);
                return self.set_error(&if code == "ENOENT" {
                    "File deleted".into()
                } else {
                    format!("Cannot read: {code}")
                });
            }
        };
        let mut info = size(len) + if len > limit { " (start only)" } else { "" };
        let (top, left) = (self.state.top, self.state.left);
        let s = &mut self.state;
        s.rows = None;
        s.buffer = None;
        s.matches.clear();
        s.matched = -1;
        s.converting = false;
        s.missing = None;
        self.forget_pictures(&file);
        if picture {
            info = self.load_image(&file, buf, info);
        } else if buf[..buf.len().min(8000)].contains(&0) {
            let s = &mut self.state;
            s.kind = Kind::Binary;
            s.plain = hexdump(&buf[..buf.len().min(HEX_BYTES)]);
            s.source = s
                .plain
                .iter()
                .map(|l| vec![(l[..10].to_string(), fg(c().dim)), (l[10..].to_string(), String::new())])
                .collect();
            if buf.len() > HEX_BYTES {
                info += &format!(" · first {}", size(HEX_BYTES as u64));
            }
        } else {
            let s = &mut self.state;
            let text = String::from_utf8_lossy(&buf);
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
            s.kind = if is_markdown(&file) { Kind::Markdown } else { Kind::Text };
            // split(/\r?\n/): a \r goes only before a \n, the last line keeping its own.
            let mut lines: Vec<&str> = text.split('\n').collect();
            let n = lines.len();
            for l in &mut lines[..n - 1] {
                *l = l.strip_suffix('\r').unwrap_or(l);
            }
            if lines.len() > 1 && lines.last() == Some(&"") {
                lines.pop();
            }
            s.plain = lines.iter().map(|l| ui::clean_line(l, 4, None)).collect();
            let lang = if buf.len() <= MAX_HIGHLIGHT {
                crate::render::highlight::language(&paths::basename(&file, None))
            } else {
                None
            };
            let source = lang.map(|l| crate::render::highlight::highlight(&text, Some(l)));
            s.source = match source {
                Some(src) if src.len() >= s.plain.len() => src,
                _ => s.plain.iter().map(|l| vec![(l.clone(), String::new())]).collect(),
            };
            info = format!("{} line{} · {info}", lines.len(), if lines.len() == 1 { "" } else { "s" });
            s.text = text;
        }
        let s = &mut self.state;
        s.info = info;
        if keep_scroll {
            s.top = top;
            s.left = left;
        } else {
            s.top = 0;
            s.left = 0;
        }
        if self.state.find.as_ref().is_some_and(|f| !f.field.value.is_empty()) {
            self.search(false);
        }
        if self.state.diff {
            self.load_diff(keep_scroll, false);
        }
        if self.state.edit.is_some() {
            self.disk_changed();
        }
        self.load_changes();
        self.render();
    }

    fn set_error(&mut self, text: &str) {
        let s = &mut self.state;
        s.kind = Kind::Error;
        s.info = text.to_string();
        s.rows = None;
        s.plain.clear();
        s.source.clear();
        s.buffer = None;
        if self.state.edit.is_some() {
            self.disk_changed();
        }
        // A deleted file still has its changes.
        if self.state.diff && !self.state.file.is_empty() {
            self.load_diff(false, false);
        }
        self.render();
    }

    // --- marks

    /// The marks on a line (0-based).
    fn marks_of(&self, src: i64) -> impl Iterator<Item = &Mark> {
        self.state.marks.iter().filter(move |m| src + 1 >= m.from && src < m.to)
    }

    /// n and N without a find: the next or previous mark, a third of the way down the pane.
    fn next_mark(&mut self, dir: i64) {
        let n = self.state.marks.len() as i64;
        self.state.mark = (self.state.mark as i64 + dir).rem_euclid(n) as usize;
        let from = self.state.marks[self.state.mark].from;
        let row = if self.state.diff { self.diff_row(from as f64) } else { self.row_of(from as f64 - 1.0) };
        self.state.top = (row - self.third()).max(0);
        self.flash(Some(format!("Mark {}/{}", self.state.mark + 1, n)));
    }

    /// Marks are on the file's lines: rendered Markdown shows its source instead.
    fn show_marks(&mut self) {
        self.state.mark = 0;
        if !self.state.marks.is_empty() && !self.state.diff && self.state.kind == Kind::Markdown && !self.state.raw {
            self.state.raw = true;
            self.state.rows = None;
            self.render();
        }
    }

    // --- actions

    fn flash(&mut self, text: Option<String>) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return };
        let time = ui::message_time(ui::message_kind(&text));
        self.state.flash = Some(text);
        self.render();
        self.flash_at = Some(Instant::now() + Duration::from_millis(time));
    }

    /// An action on the worker, its message flashed once done; "Action failed" when it panicked.
    fn act(&mut self, f: impl FnOnce() -> Option<String> + Send + 'static) {
        let job: Job = Box::new(move || {
            let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or_else(|_| Some("Action failed".into()));
            Box::new(move |p: &mut Preview| p.flash(text))
        });
        self.herdr.send(job);
    }

    /// A setting switched (w, L, c), saved for every pane.
    fn toggle(&mut self, name: &str) {
        let on = !self.settings.get(name).is_some_and(Value::truthy);
        self.settings.set(name, Value::Bool(on));
        let mut saved = exec::read_settings();
        saved.set(name, Value::Bool(on));
        exec::write_settings(&saved);
        self.state.rows = None;
        self.render();
    }

    fn on(&self, name: &str) -> bool {
        self.settings.get(name).is_some_and(Value::truthy)
    }

    // --- pane width

    /// A check of the width on the worker; its result wakes the loop, which then looks at the next one due.
    fn keeper_job(&mut self, f: impl FnOnce() + Send + 'static) {
        self.keeper_busy = true;
        self.herdr.send(Box::new(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            Box::new(|p: &mut Preview| p.keeper_busy = false)
        }));
    }

    /// The preview keeps its width when another resize moves it (actions::WidthKeeper): the width herdr gave it at
    /// first, then the one set by dragging its border or with < >.
    fn check_width(&mut self, force: bool) {
        let (keeper, width) = (self.keeper.clone(), self.width.clone());
        self.keeper_job(move || {
            let mut k = keeper.lock().unwrap_or_else(|p| p.into_inner());
            k.check(force, &mut Width(width));
        });
    }

    /// < and >: 4 columns narrower or wider, from the width asked for last, so that a key held down adds up.
    fn widen(&mut self, cols: f64) {
        let w = self.size().0 as f64;
        {
            let mut kept = self.width.lock().unwrap_or_else(|p| p.into_inner());
            *kept = Some(kept.unwrap_or(w) + cols);
        }
        self.check_width(true);
    }
}
