//! The Changes pane (src/diff.js's main): git changes of a file or folder, in a temporary side pane: compared with the
//! tab's scope (b), inline or side by side (v), staged, unstaged or discarded by the rows selected (s, u, D), with
//! review notes (a) and files marked viewed (m); q or Esc closes it.
//!
//! As in the other panes, the herdr calls run on a worker thread, git on a second one, a staging that may ask about
//! the trash on a thread of its own; their results come back to the pane's loop, the JS's timers are deadlines of
//! that loop.
//!
//! This file holds the pane, its loop and timers; state.rs the changes loaded and what is asked, ops.rs staging,
//! viewed files and scopes, render.rs the frame, input.rs keys, mouse and notes.

mod input;
mod ops;
mod render;
mod state;

use crate::core::scope::Scope;
use crate::core::screen::{self, Input, Screen};
use crate::core::ui::{self, Field, HelpState, Hints, Menu, Scrollbar};
use crate::core::watch::Watch;
use crate::core::{actions, exec, js, notes, paths, scope, viewed, worker};
use crate::diff::{Diff, Row};
use crate::json::Value;
use crate::sys;
use state::{Chip, Confirm, Draft, Opt};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub(super) type Then = worker::Then<Pane>;
type Worker = worker::Worker<Pane>;

pub struct Pane {
    file: String,
    cwd: String,
    pane: String,
    /// The explorer's pane, whose terminal the notes go to; this pane's own when opened without it.
    from: String,
    /// What the changes are compared with, the tab's (core::scope), HEAD until the tab is known. top: the
    /// repository's folder, which the diff's paths are relative to. base_name: the default branch, once found.
    scope: Scope,
    top: String,
    base_name: String,
    /// The changes asked for last: those of an older run are dropped.
    runs: u64,
    parsed: Option<Arc<Diff>>,
    rows: Vec<Row>,
    cache: String,
    top0: i64,
    /// The row of the cursor, which notes and references take (-1 for none).
    cur: i64,
    note: Option<Draft>,
    flash: Option<String>,
    flash_at: Option<Instant>,
    tab: Option<String>,
    tab_notes: Vec<Value>,
    /// The other end of the rows selected with shift (or the mouse), None for the cursor's row alone. stay: the row
    /// the cursor goes back to once the changes are read again (after s, u or D), None to follow its lines.
    anchor: Option<i64>,
    stay: Option<i64>,
    /// Long lines wrapped (the preview's setting, w in either); left: the first column shown of the text otherwise.
    wrap: bool,
    left: usize,
    /// The paths (relative to top) of the files marked viewed (m) and unchanged since, shown folded; in the order of
    /// the diff.
    viewed: Vec<String>,
    scrollbar: Scrollbar,
    /// The real path of each file of the diff, looked up once; disk_lines of the staged rows of the parsed diff.
    reals: HashMap<String, String>,
    disk: HashMap<(String, i64, i64), Option<(i64, i64)>>,
    help: Option<HelpState>,
    hints: Hints,
    chips: Vec<Chip>,
    confirm: Option<Confirm>,
    /// While stage::apply runs: a second s, u or D would apply the same rows again.
    busy: bool,
    /// b's menu, the branches of Branch… or the view's options; prompt: while a commit or range is typed.
    menu: Option<Menu<Opt>>,
    prompt: Option<Field>,
    screen: Screen,
    tx: Sender<Then>,
    rx: Receiver<Then>,
    waker: sys::Waker,
    herdr: Worker,
    git: Worker,
    settings_watch: Option<Watch>,
    notes_watch: Option<Watch>,
    viewed_watch: Option<Watch>,
    scope_watch: Option<Watch>,
}

pub fn run(_args: &[String]) -> i32 {
    worker::quiet_panics();
    let mut p = match Pane::new() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("herdr-explorer: {e}");
            return 1;
        }
    };
    p.start();
    p.run()
}

impl Pane {
    fn new() -> std::io::Result<Pane> {
        let env = |k: &str| exec::env_var(k).filter(|v| !v.is_empty());
        ui::init_theme();
        let settings = exec::read_settings();
        // The glyphs stay Nerd Font ones whatever the settings say, as in the JS.
        let file = env("EXPLORER_FILE").unwrap_or_default();
        let cwd = env("EXPLORER_ROOT")
            .or_else(|| (!file.is_empty()).then(|| paths::dirname(&file)))
            .unwrap_or_else(|| std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default());
        // the pane's folder for new spaces, tabs and panes made from it
        exec::home(None);
        let pane = exec::env_var("HERDR_PANE_ID").unwrap_or_default();
        let from = env("EXPLORER_PANE").unwrap_or_else(|| pane.clone());
        let screen = Screen::open(screen::Options { mouse: true, motion: true, focus: true })?;
        let waker = screen.waker();
        let (tx, rx) = channel();
        Ok(Pane {
            top: cwd.clone(),
            file,
            cwd,
            pane,
            from,
            scope: Scope::head(),
            base_name: String::new(),
            runs: 0,
            parsed: None,
            rows: Vec::new(),
            cache: String::new(),
            top0: 0,
            cur: -1,
            note: None,
            flash: None,
            flash_at: None,
            tab: None,
            tab_notes: Vec::new(),
            anchor: None,
            stay: None,
            wrap: settings.get("wrap").is_some_and(Value::truthy),
            left: 0,
            viewed: Vec::new(),
            scrollbar: Scrollbar::new(),
            reals: HashMap::new(),
            disk: HashMap::new(),
            help: None,
            hints: Hints::default(),
            chips: Vec::new(),
            confirm: None,
            busy: false,
            menu: None,
            prompt: None,
            herdr: Worker::new(tx.clone(), waker.clone()),
            git: Worker::new(tx.clone(), waker.clone()),
            screen,
            tx,
            rx,
            waker,
            settings_watch: exec::settings_file().map(|f| Watch::new(&f, Duration::from_millis(1000))),
            notes_watch: None,
            viewed_watch: None,
            scope_watch: None,
        })
    }

    /// What the JS does as its script loads, once the screen is set up.
    fn start(&mut self) {
        self.draw();
        self.changes(|p, d| {
            if let Some(d) = d {
                p.set_parsed(d);
                p.read_viewed();
                p.draw();
            }
        });
        // The tab, whose notes this pane shows and takes.
        if self.pane.is_empty() {
            return;
        }
        let pane = self.pane.clone();
        self.herdr.send(Box::new(move || {
            let tab = actions::tab_of(&pane).and_then(|t| t.me.get("tab_id").filter(|v| v.truthy()).map(js::string));
            Box::new(move |p: &mut Pane| p.got_tab(tab))
        }));
    }

    pub(super) fn got_tab(&mut self, tab: Option<String>) {
        let Some(tab) = tab else { return };
        self.tab_notes = notes::current(&tab, None);
        // Placed again by the pane that saw a file change (or at a reload here): read as saved.
        self.notes_watch = notes::watch(&tab);
        self.viewed_watch = viewed::watch(&tab);
        // The tab's scope, and the one another pane chooses later.
        self.scope_watch = scope::watch(&tab);
        self.tab = Some(tab);
        self.read_viewed();
        self.follow_scope();
        self.draw();
    }

    /// The loop: the screen's events, the workers' results and the timers, until the pane is closed.
    fn run(&mut self) -> i32 {
        loop {
            let timeout = self.next_deadline().map(|t| t.saturating_duration_since(Instant::now()));
            for input in self.screen.wait(timeout) {
                match input {
                    Input::Ui(e) => self.event(e),
                    Input::Resize => self.draw(),
                    Input::Theme => {
                        self.cache.clear();
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
        let watches = [&self.settings_watch, &self.notes_watch, &self.viewed_watch, &self.scope_watch];
        [self.flash_at, self.scrollbar.due()]
            .into_iter()
            .chain(watches.into_iter().map(|w| w.as_ref().map(Watch::due)))
            .flatten()
            .min()
    }

    fn timers(&mut self) {
        let now = Instant::now();
        if self.flash_at.is_some_and(|t| t <= now) {
            self.flash_at = None;
            self.flash = None;
            self.draw();
        }
        if let Some(top) = self.scrollbar.tick(now) {
            self.top0 = top;
            self.draw();
        }
        // Wrap lines and the layout changed in another pane (the preview, source control): shown at once.
        if self.settings_watch.as_mut().is_some_and(Watch::poll) {
            let now = exec::read_settings().get("wrap").is_some_and(Value::truthy);
            if now != self.wrap {
                self.wrap = now;
                self.left = 0;
            }
            self.draw();
        }
        if self.notes_watch.as_mut().is_some_and(Watch::poll) {
            self.tab_notes = notes::read(self.tab.as_deref().unwrap_or(""));
            self.draw();
        }
        if self.viewed_watch.as_mut().is_some_and(Watch::poll) {
            self.read_viewed();
            self.draw();
        }
        if self.scope_watch.as_mut().is_some_and(Watch::poll) {
            self.follow_scope();
        }
    }

    /// The terminal's size: columns and rows, 80 and 24 when unknown.
    pub(super) fn size(&self) -> (usize, usize) {
        sys::size()
            .map_or((80, 24), |(w, h)| (if w == 0 { 80 } else { w as usize }, if h == 0 { 24 } else { h as usize }))
    }

    pub(super) fn body(&self) -> i64 {
        self.size().1 as i64 - 2
    }

    pub(super) fn exit(&mut self) -> ! {
        self.screen.exit(0)
    }

    pub(super) fn say(&mut self, text: String) {
        if text.is_empty() {
            return self.draw();
        }
        let time = ui::message_time(ui::message_kind(&text));
        self.flash = Some(text);
        self.draw();
        self.flash_at = Some(Instant::now() + Duration::from_millis(time));
    }

    /// An action on the herdr worker, its message said once done; "Action failed" when it panicked.
    pub(super) fn act(&mut self, f: impl FnOnce() -> Option<String> + Send + 'static) {
        self.herdr.send(Box::new(move || {
            let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or_else(|_| Some("Action failed".into()))
                .unwrap_or_default();
            Box::new(move |p: &mut Pane| p.say(text))
        }));
    }
}
