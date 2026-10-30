//! Search opened from the explorer, in a tab of its own (actions.search): Go to file (fuzzy match on
//! paths, ctrl+p) and Search in files (text or regex in the contents, ctrl+f), the selected result in the tab's preview
//! on the right. Enter opens the file in the preview of the tab it was opened from, at the line; Esc closes the search
//! and its tab.
//!
//! As in the other panes, the herdr and clipboard calls run on a worker thread, the file list, the search and the
//! files previewed on threads of their own; their results come back to the pane's loop, timers are deadlines of
//! that loop.

mod find;
mod render;
mod state;

use crate::core::actions::{self, Sharer};
use crate::core::exec;
use crate::core::screen::{self, Input, Screen};
use crate::core::ui::{self, Event, HelpState, Hints, Mouse, Scrollbar, Seg};
use crate::core::{paths, worker};
use crate::json::{self, Value};
use crate::sys;
use find::{Group, MAX_MATCHES};
use state::{
    Act, CONTENT, EXCLUDE, FILES, Files, Focus, INCLUDE, Mode, Opt, Row, SField, Scored, Shown, State, Target, Zone,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(150);
/// Larger files previewed in plain text, as in the preview pane.
const MAX_HIGHLIGHT: usize = 1 << 20;

type Then = worker::Then<Search>;
type Worker = worker::Worker<Search>;

pub struct Search {
    state: State,
    root: String,
    /// The explorer's pane, for the actions.
    pane: String,
    /// The tab the search was opened from, when it has one of its own; its own pane, beside its preview.
    origin: String,
    own: String,
    save_file: Option<String>,
    wrap: bool,
    screen: Screen,
    tx: Sender<Then>,
    rx: Receiver<Then>,
    waker: sys::Waker,
    herdr: Worker,
    reader: Worker,
    /// Restarted on every change; a newer search makes the running one stop at its next batch.
    search_gen: Arc<AtomicU64>,
    debounce_at: Option<Instant>,
    paint_at: Option<Instant>,
    flash_at: Option<Instant>,
    follow_at: Option<Instant>,
    /// rel → the file read, None while it is read; the oldest first.
    previews: Vec<(String, Option<Shown>)>,
    zones: Vec<Zone>,
    scrollbar: Scrollbar,
    /// Width of the results list, the scrollbar in its last column.
    list_w: i64,
    sharer: Option<Arc<Mutex<Sharer>>>,
    shown_key: Option<String>,
    want_key: Option<String>,
    want: Option<(Option<Target>, String)>,
    previewed: bool,
    snapshot: Arc<Mutex<Option<String>>>,
}

pub fn run(args: &[String]) -> i32 {
    worker::quiet_panics();
    let mut s = match Search::new(args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("herdr-explorer: {e}");
            return 1;
        }
    };
    s.start();
    s.run()
}

impl Search {
    fn new(args: &[String]) -> std::io::Result<Search> {
        let env = |k: &str| exec::env_var(k).filter(|v| !v.is_empty());
        ui::init_theme();
        let settings = exec::read_settings();
        ui::use_glyphs(settings.get("glyphs").and_then(Value::as_str).unwrap_or(""));
        let root = paths::resolve(&[&env("EXPLORER_ROOT").or_else(|| args.first().cloned()).unwrap_or_default()]);
        // the pane's folder for new spaces, tabs and panes made from it
        crate::core::exec::home(None);
        let save_file = exec::state_dir().map(|d| paths::join(&[&d, "search.json"]));
        let saved = save_file
            .as_deref()
            .and_then(|f| exec::read_text(f).ok())
            .and_then(|t| json::parse(&t).ok())
            .filter(Value::truthy)
            .unwrap_or(Value::Null);
        let text = |k: &str| saved.get(k).filter(|v| v.truthy()).map_or(String::new(), crate::json::text);
        let truthy = |k: &str| saved.get(k).is_some_and(Value::truthy);
        let mode =
            if exec::env_var("EXPLORER_MODE").as_deref() == Some("content") { Mode::Content } else { Mode::Files };
        let mut fields = [
            SField::new(&text("files")),
            SField::new(&text("content")),
            SField::new(&text("include")),
            SField::new(&text("exclude")),
        ];
        if let Some(q) = env("EXPLORER_QUERY") {
            fields[if mode == Mode::Content { CONTENT } else { FILES }] = SField::new(&q);
        }
        let screen = Screen::open(screen::Options { mouse: true, motion: true, focus: true })?;
        let waker = screen.waker();
        let (tx, rx) = channel();
        let origin = env("EXPLORER_ORIGIN").unwrap_or_default();
        let own = env("HERDR_PANE_ID").unwrap_or_default();
        Ok(Search {
            state: State {
                mode,
                fields,
                focus: Focus::Query,
                case: truthy("case"),
                word: truthy("word"),
                regex: truthy("regex"),
                hidden: false,
                files: None,
                capped: false,
                files_gen: 0,
                results: Vec::new(),
                line: 0.0,
                prev: None,
                groups: Vec::new(),
                total: 0,
                searching: false,
                error: None,
                re: None,
                search_key: None,
                touched: false,
                cursor: 0,
                top: 0,
                flash: None,
                last_click: None,
                help: None,
                hints: Hints::default(),
            },
            sharer: (!origin.is_empty()).then(|| Arc::new(Mutex::new(Sharer::new(&own, "searchShare")))),
            root,
            pane: env("EXPLORER_PANE").unwrap_or_default(),
            origin,
            own,
            save_file,
            wrap: settings.get("wrap").is_some_and(Value::truthy),
            herdr: Worker::new(tx.clone(), waker.clone()),
            reader: Worker::new(tx.clone(), waker.clone()),
            screen,
            tx,
            rx,
            waker,
            search_gen: Arc::new(AtomicU64::new(0)),
            debounce_at: None,
            paint_at: None,
            flash_at: None,
            follow_at: None,
            previews: Vec::new(),
            zones: Vec::new(),
            scrollbar: Scrollbar::new(),
            list_w: 0,
            shown_key: None,
            want_key: None,
            want: None,
            previewed: false,
            snapshot: Arc::new(Mutex::new(None)),
        })
    }

    fn start(&mut self) {
        // The state saved at exit, a signal included.
        let (file, snapshot) = (self.save_file.clone(), self.snapshot.clone());
        exec::at_exit(move || {
            if let (Some(file), Some(data)) = (file, snapshot.lock().unwrap_or_else(|e| e.into_inner()).take()) {
                // Whole or not at all, as another popup may read it.
                exec::write_atomic(&file, data);
            }
        });
        self.store();
        if !self.origin.is_empty() {
            actions::hold_tab("search", &self.origin, "Search");
        }
        self.render();
        self.load_files();
    }

    fn run(&mut self) -> i32 {
        loop {
            let timeout = self.next_deadline().map(|t| t.saturating_duration_since(Instant::now()));
            for input in self.screen.wait(timeout) {
                match input {
                    Input::Ui(e) => self.event(e),
                    Input::Resize | Input::Theme => {
                        self.render();
                        self.share();
                    }
                    Input::Wake => {}
                }
                self.results();
            }
            self.results();
            self.timers();
            self.store();
        }
    }

    fn results(&mut self) {
        while let Ok(then) = self.rx.try_recv() {
            then(self);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        [self.debounce_at, self.paint_at, self.flash_at, self.follow_at, self.scrollbar.due()]
            .into_iter()
            .flatten()
            .min()
    }

    fn timers(&mut self) {
        let now = Instant::now();
        let due = |t: &mut Option<Instant>| t.is_some_and(|t| t <= now) && t.take().is_some();
        if due(&mut self.debounce_at) {
            self.search();
        }
        if due(&mut self.paint_at) {
            self.render();
        }
        if due(&mut self.flash_at) {
            self.state.flash = None;
            self.render();
        }
        if due(&mut self.follow_at) {
            self.followed();
        }
        if let Some(top) = self.scrollbar.tick(now) {
            self.state.top = top;
            self.render();
        }
    }

    /// The terminal's size: columns and rows, 80 and 24 when unknown.
    fn size(&self) -> (usize, usize) {
        sys::size()
            .map_or((80, 24), |(w, h)| (if w == 0 { 80 } else { w as usize }, if h == 0 { 24 } else { h as usize }))
    }

    // --- state

    fn query_index(&self) -> usize {
        if self.state.mode == Mode::Files { FILES } else { CONTENT }
    }

    fn focused(&mut self) -> &mut SField {
        let s = &mut self.state;
        let i = match s.focus {
            _ if s.mode == Mode::Files => FILES,
            Focus::Query => CONTENT,
            Focus::Include => INCLUDE,
            Focus::Exclude => EXCLUDE,
        };
        &mut s.fields[i]
    }

    /// What is saved at the exit, kept up to date for a signal.
    fn store(&self) {
        let f = &self.state.fields;
        let s = &self.state;
        let text = |i: usize| Value::Str(f[i].field.value.clone());
        let data = Value::Obj(vec![
            ("files".into(), text(FILES)),
            ("content".into(), text(CONTENT)),
            ("include".into(), text(INCLUDE)),
            ("exclude".into(), text(EXCLUDE)),
            ("case".into(), Value::Bool(s.case)),
            ("word".into(), Value::Bool(s.word)),
            ("regex".into(), Value::Bool(s.regex)),
        ]);
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(json::stringify(&data));
    }

    fn exit(&mut self) -> ! {
        self.store();
        self.screen.exit(0)
    }

    // --- file list

    fn load_files(&mut self) {
        self.state.files_gen += 1;
        let id = self.state.files_gen;
        // a running search would go on with the old list
        self.search_gen.fetch_add(1, Ordering::SeqCst);
        self.state.files = None;
        self.state.capped = false;
        let (root, hidden) = (self.root.clone(), self.state.hidden);
        worker::spawn(
            self.tx.clone(),
            self.waker.clone(),
            Box::new(move || {
                let mut files = if hidden { None } else { find::git_files(&root) };
                let mut capped = None;
                if files.as_ref().is_none_or(Vec::is_empty) {
                    let (list, c) = find::walk(&root, hidden);
                    files = Some(list);
                    capped = Some(c);
                }
                let mut list = files.unwrap_or_default();
                list.sort();
                let lower = list.iter().map(|f| f.to_lowercase()).collect();
                let len = list.iter().map(|f| f.chars().count()).collect();
                let base = list.iter().map(|f| f.rfind('/').map_or(0, |i| i + 1)).collect();
                let files = Files { list: Arc::new(list), lower, len, base };
                Box::new(move |s: &mut Search| {
                    if id != s.state.files_gen {
                        return;
                    }
                    if let Some(c) = capped {
                        s.state.capped = c;
                    }
                    s.state.files = Some(files);
                    s.state.prev = None;
                    s.state.search_key = None;
                    s.refresh();
                })
            }),
        );
    }

    /// Scores every path, or only the previous matches when the query grows (a match of the longer query matches the
    /// shorter one too); keeps the best MAX_ROWS.
    fn filter_files(&mut self) {
        let (q, line) = find::parse_query(&self.state.fields[FILES].field.value);
        let s = &mut self.state;
        s.line = line;
        let Some(files) = &s.files else {
            s.results = Vec::new();
            return;
        };
        if q.is_empty() {
            s.results = (0..files.list.len().min(find::MAX_ROWS)).map(|i| Scored { i, score: 0, pos: None }).collect();
            s.prev = None;
            return;
        }
        let lower = q.to_lowercase();
        let prev = s.prev.as_ref().filter(|(pq, _)| lower.starts_with(pq.as_str())).map(|(_, idx)| idx.clone());
        let (qc, ql) = find::query_chars(&q);
        let (mut idx, mut scored) = (Vec::new(), Vec::new());
        let mut each = |i: usize| {
            if let Some((score, _)) = find::fuzzy(&qc, &ql, &files.list[i], &files.lower[i], files.base[i], false) {
                idx.push(i);
                scored.push(Scored { i, score, pos: None });
            }
        };
        match prev {
            Some(prev) => prev.into_iter().for_each(&mut each),
            None => (0..files.list.len()).for_each(&mut each),
        }
        scored.sort_by(|a, b| {
            // Best score first, then the shorter path, then by name (names read lossily may repeat).
            b.score.cmp(&a.score).then(files.len[a.i].cmp(&files.len[b.i])).then(files.list[a.i].cmp(&files.list[b.i]))
        });
        scored.truncate(find::MAX_ROWS);
        s.prev = Some((lower, idx));
        s.results = scored;
    }

    // --- search in files

    fn search_key(&self) -> String {
        let s = &self.state;
        let f = &s.fields;
        format!(
            "{:?}",
            (
                &f[CONTENT].field.value,
                &f[INCLUDE].field.value,
                &f[EXCLUDE].field.value,
                s.case,
                s.word,
                s.regex,
                s.hidden,
                s.files_gen
            )
        )
    }

    fn schedule_search(&mut self) {
        self.state.searching = !self.state.fields[CONTENT].field.value.is_empty();
        self.debounce_at = Some(Instant::now() + DEBOUNCE);
    }

    fn search(&mut self) {
        self.debounce_at = None;
        let id = self.search_gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.search_key = Some(self.search_key());
        // The files read again, as they may have changed since.
        self.previews.clear();
        let s = &mut self.state;
        (s.groups, s.total, s.error, s.re, s.cursor, s.top, s.touched) = (Vec::new(), 0, None, None, 0, 0, false);
        let text = s.fields[CONTENT].field.value.clone();
        if text.is_empty() {
            s.searching = false;
            return self.render();
        }
        match find::matcher(&text, s.case, s.word, s.regex) {
            Ok(re) => s.re = Some(re),
            Err(e) => {
                s.searching = false;
                s.error = Some(e);
                return self.render();
            }
        }
        s.searching = true;
        self.render();
        // load_files starts it again
        let Some(files) = &self.state.files else { return };
        let (list, re) = (files.list.clone(), self.state.re.clone().expect("a matcher"));
        let inc = find::globs(&self.state.fields[INCLUDE].field.value);
        let exc = find::globs(&self.state.fields[EXCLUDE].field.value);
        let (root, current, tx, waker) =
            (self.root.clone(), self.search_gen.clone(), self.tx.clone(), self.waker.clone());
        let send = move |then: Then| {
            if tx.send(then).is_ok() {
                waker.wake();
            }
        };
        let _ = std::thread::Builder::new().name("pane-worker".into()).spawn(move || {
            let todo: Vec<&String> = list
                .iter()
                .filter(|rel| inc.as_ref().is_none_or(|r| r.test(rel)) && !exc.as_ref().is_some_and(|r| r.test(rel)))
                .collect();
            let mut total = 0;
            for batch in todo.chunks(find::BATCH) {
                if total >= MAX_MATCHES {
                    break;
                }
                let room = MAX_MATCHES - total;
                let found: Vec<Option<Group>> = batch.iter().map(|rel| find::grep(&root, rel, &re, room)).collect();
                if current.load(Ordering::SeqCst) != id {
                    return;
                }
                let mut add = Vec::new();
                for mut g in found.into_iter().flatten() {
                    if total >= MAX_MATCHES {
                        continue;
                    }
                    if total + g.count > MAX_MATCHES {
                        let (mut left, mut keep, mut count) = (MAX_MATCHES - total, 0, 0);
                        while keep < g.matches.len() && left > 0 {
                            let c = left.min(g.matches[keep].ranges.len());
                            count += c;
                            left -= c;
                            keep += 1;
                        }
                        g.matches.truncate(keep);
                        g.count = count;
                    }
                    total += g.count;
                    add.push(g);
                }
                send(Box::new(move |s: &mut Search| {
                    if s.search_gen.load(Ordering::SeqCst) != id {
                        return;
                    }
                    for g in add {
                        s.state.total += g.count;
                        s.state.groups.push(g);
                    }
                    s.paint_soon();
                }));
            }
            send(Box::new(move |s: &mut Search| {
                if s.search_gen.load(Ordering::SeqCst) != id {
                    return;
                }
                s.state.searching = false;
                s.render();
            }));
        });
    }

    // --- rows

    /// Go to file: one row per file. Search in files: a header row per file, then its matching lines.
    fn rows(&self) -> Vec<Row> {
        let s = &self.state;
        if s.mode == Mode::Files {
            // The list read again (alt+h): no results shown meanwhile.
            if s.files.is_none() {
                return Vec::new();
            }
            return (0..s.results.len()).map(Row::File).collect();
        }
        let mut out = Vec::new();
        for (gi, g) in s.groups.iter().enumerate() {
            out.push(Row::Head(gi));
            if !g.collapsed {
                out.extend((0..g.matches.len()).map(|mi| Row::Line(gi, mi)));
            }
        }
        out
    }

    fn target(&self, r: Option<Row>) -> Option<Target> {
        let s = &self.state;
        match r? {
            Row::File(k) => {
                let files = s.files.as_ref()?;
                Some(Target { rel: files.list[s.results[k].i].clone(), line: s.line })
            }
            Row::Head(gi) => {
                let g = &s.groups[gi];
                Some(Target { rel: g.rel.clone(), line: g.matches[0].n as f64 })
            }
            Row::Line(gi, mi) => {
                let g = &s.groups[gi];
                Some(Target { rel: g.rel.clone(), line: g.matches[mi].n as f64 })
            }
        }
    }

    fn current(&self) -> Option<Row> {
        usize::try_from(self.state.cursor).ok().and_then(|c| self.rows().get(c).copied())
    }

    // --- preview

    /// The file read for the preview on the right, None while it is read (asked then).
    fn preview(&mut self, rel: &str) -> Option<&Shown> {
        if let Some(i) = self.previews.iter().position(|(r, _)| r == rel) {
            return self.previews[i].1.as_ref();
        }
        self.previews.push((rel.to_string(), None));
        let (rel, root) = (rel.to_string(), self.root.clone());
        self.reader.send(Box::new(move || {
            let shown = match find::read_text(&paths::join(&[&root, &rel]), &root) {
                Err(e) => Shown::Error(e.to_string()),
                Ok(text) => {
                    let body = text.strip_suffix('\n').map(|t| t.strip_suffix('\r').unwrap_or(t)).unwrap_or(&text);
                    let lines: Vec<String> =
                        body.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
                    let segs = coloured(&rel, &lines, text.chars().count());
                    Shown::Text { lines, segs }
                }
            };
            Box::new(move |s: &mut Search| {
                if s.previews.len() > 30 {
                    s.previews.remove(0);
                }
                match s.previews.iter_mut().find(|(r, _)| *r == rel) {
                    Some(p) => p.1 = Some(shown),
                    None => s.previews.push((rel, Some(shown))),
                }
                s.paint_soon();
            })
        }));
        None
    }

    fn paint_soon(&mut self) {
        if self.paint_at.is_none() {
            self.paint_at = Some(Instant::now() + Duration::from_millis(50));
        }
    }

    fn flash(&mut self, text: Option<String>) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return };
        let time = ui::message_time(ui::message_kind(&text));
        self.state.flash = Some(text);
        self.render();
        self.flash_at = Some(Instant::now() + Duration::from_millis(time));
    }

    // --- in a tab of its own

    /// The tab's preview shows the result selected (the line found, marked), else what to do; it opens with the first
    /// one, and once closed (q there) stays closed.
    fn follow(&mut self) {
        let t = self.target(self.current());
        let s = &self.state;
        let empty = if s.files.is_none() {
            "Indexing…"
        } else if s.mode == Mode::Content && s.fields[CONTENT].field.value.is_empty() {
            "Type to search"
        } else if s.mode == Mode::Content && s.searching {
            "Searching…"
        } else {
            "No results"
        };
        let k = match &t {
            Some(t) => format!("file {} {}", paths::join(&[&self.root, &t.rel]), t.line),
            None => format!("text {empty}"),
        };
        if self.want_key.as_ref() == Some(&k) || (self.shown_key.as_ref() == Some(&k) && self.follow_at.is_none()) {
            return;
        }
        self.want_key = Some(k);
        self.want = Some((t, empty.to_string()));
        self.follow_at = Some(Instant::now() + Duration::from_millis(120));
    }

    fn followed(&mut self) {
        self.shown_key = self.want_key.clone();
        let Some((t, empty)) = self.want.take() else { return };
        let open = !self.previewed;
        self.previewed = true;
        let (root, own) = (self.root.clone(), self.own.clone());
        let content = self.state.mode == Mode::Content;
        self.herdr.send(Box::new(move || {
            let msg = match t {
                Some(t) => {
                    let line = t.line;
                    let marks = if content && line != 0.0 {
                        let n = Value::Num(line);
                        let zero = Value::Num(0.0);
                        vec![Value::Obj(vec![
                            ("from".into(), n.clone()),
                            ("to".into(), n),
                            ("a".into(), zero.clone()),
                            ("b".into(), zero),
                        ])]
                    } else {
                        Vec::new()
                    };
                    let o = actions::Shown { marks, beside: Some(own.clone()), ..Default::default() };
                    actions::preview(&paths::join(&[&root, &t.rel]), &root, &own, line, false, open, &o)
                }
                None => {
                    let show = Value::Obj(vec![
                        ("blank".into(), Value::Bool(true)),
                        ("text".into(), Value::Str(empty)),
                        ("cwd".into(), Value::Str(root.clone())),
                    ]);
                    let o = actions::Shown { show: Some(show), beside: Some(own.clone()), ..Default::default() };
                    actions::preview("", &root, &own, 0.0, false, open, &o)
                }
            };
            Box::new(move |s: &mut Search| match msg {
                // Not opened: asked to open again with the next result.
                Some(m) => {
                    s.previewed &= !open;
                    s.flash(Some(m));
                }
                None if open => s.share(),
                None => {}
            })
        }));
    }

    /// The share of the tab's width the search keeps beside its preview (actions::Sharer).
    fn share(&mut self) {
        let Some(sharer) = self.sharer.clone() else { return };
        self.herdr.send(Box::new(move || {
            sharer.lock().unwrap_or_else(|e| e.into_inner()).share(0.0);
            Box::new(|_: &mut Search| {})
        }));
    }

    // --- actions

    /// Results recomputed for the current mode (a search only when its inputs changed).
    fn refresh(&mut self) {
        if self.state.mode == Mode::Files {
            self.filter_files();
            return self.render();
        }
        if Some(self.search_key()) != self.state.search_key || self.state.files.is_none() {
            self.search()
        } else {
            self.render()
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.state.mode == mode {
            return;
        }
        let s = &mut self.state;
        s.mode = mode;
        s.focus = Focus::Query;
        s.cursor = 0;
        s.top = 0;
        self.refresh();
    }

    fn focus(&mut self, k: Focus) {
        if self.state.mode == Mode::Content {
            self.state.focus = k;
        }
        self.render();
    }

    fn toggle(&mut self, k: Opt) {
        let s = &mut self.state;
        let on = match k {
            Opt::Case => &mut s.case,
            Opt::Word => &mut s.word,
            Opt::Regex => &mut s.regex,
            Opt::Hidden => &mut s.hidden,
        };
        *on = !*on;
        if k == Opt::Hidden {
            self.load_files();
            return self.render();
        }
        if self.state.mode == Mode::Content { self.search() } else { self.render() }
    }

    fn list_height(&self) -> i64 {
        let (_, h) = self.size();
        (h as i64 - if self.state.mode == Mode::Content { 5 } else { 4 }).max(1)
    }

    fn select(&mut self, i: i64) {
        let n = self.rows().len() as i64;
        let h = self.list_height();
        let s = &mut self.state;
        s.cursor = i.min(n - 1).max(0);
        s.touched = true;
        if s.cursor < s.top {
            s.top = s.cursor;
        }
        if s.cursor >= s.top + h {
            s.top = s.cursor - h + 1;
        }
    }

    /// The preview pane and the insertion close the popup on success (None); otherwise their message shows.
    fn act(&mut self, kind: &'static str) {
        let Some(t) = self.target(self.current()) else { return };
        let abs = paths::join(&[&self.root, &t.rel]);
        let (root, pane) = (self.root.clone(), self.pane.clone());
        self.herdr.send(Box::new(move || {
            let run = || match kind {
                "open" => {
                    let o = actions::Shown { reveal: true, ..Default::default() };
                    actions::preview(&abs, &root, &pane, t.line, false, true, &o)
                }
                "insert" => actions::insert(&abs, &pane),
                "editor" => actions::editor(&abs, &pane),
                "app" => actions::app(&abs, &pane),
                _ => Some(actions::copy(&t.rel, "Path copied")),
            };
            let msg =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or(Some("Action failed".into()));
            Box::new(move |s: &mut Search| {
                if kind == "copy" {
                    return s.flash(msg);
                }
                // An outside editor answers with its name and the file's when it was started, an app with Opening and
                // the file's.
                let name = paths::basename(&abs, None);
                let started = |m: &str| m.ends_with(&format!(": {name}"));
                let done = match msg.as_deref() {
                    None => true,
                    Some(m) => {
                        (kind == "editor" && started(m))
                            || (kind == "app" && (started(m) || m == format!("Opening {name}")))
                    }
                };
                if done {
                    s.exit();
                }
                s.flash(msg);
            })
        }));
    }

    fn fold(&mut self, collapsed: bool) {
        if self.state.mode != Mode::Content {
            return;
        }
        let Some(Row::Head(gi) | Row::Line(gi, _)) = self.current() else { return };
        self.state.groups[gi].collapsed = collapsed;
        let i = self.rows().iter().position(|r| *r == Row::Head(gi)).map_or(-1, |i| i as i64);
        self.select(i);
    }

    // --- input

    fn event(&mut self, e: Event) {
        match e {
            Event::Key(k) => self.key(&k),
            Event::Mouse(m) => self.mouse(&m),
            // A button released out of the pane may never be reported: a held scrollbar stops with the focus.
            Event::Focus(f) => {
                if !f {
                    self.scrollbar.stop();
                }
            }
            Event::Paste { text, .. } => self.paste(&text),
            Event::Reply(_) => {}
        }
    }

    fn key(&mut self, k: &str) {
        let page = self.list_height() - 1;
        if let Some(help) = &mut self.state.help
            && k != "ctrl+c"
        {
            if !ui::help_key(k, help) {
                self.state.help = None;
            }
            return self.render();
        }
        let cursor = self.state.cursor;
        match k {
            "f1" => {
                self.state.help = Some(HelpState::default());
                return self.render();
            }
            "escape" | "ctrl+c" => self.exit(),
            "ctrl+p" => return self.set_mode(Mode::Files),
            "ctrl+f" => return self.set_mode(Mode::Content),
            "alt+c" => return self.toggle(Opt::Case),
            "alt+w" => return self.toggle(Opt::Word),
            "alt+r" => return self.toggle(Opt::Regex),
            "alt+h" => return self.toggle(Opt::Hidden),
            "tab" | "shift+tab" => {
                if self.state.mode != Mode::Content {
                    return;
                }
                let order = [Focus::Query, Focus::Include, Focus::Exclude];
                let i = order.iter().position(|f| *f == self.state.focus).unwrap_or(0);
                return self.focus(order[(i + if k == "tab" { 1 } else { 2 }) % 3]);
            }
            "up" | "ctrl+k" => self.select(cursor - 1),
            "down" | "ctrl+n" => self.select(cursor + 1),
            "pageup" => self.select(cursor - page),
            "pagedown" => self.select(cursor + page),
            "ctrl+home" => self.select(0),
            "ctrl+end" => self.select(self.rows().len() as i64 - 1),
            "alt+left" => self.fold(true),
            "alt+right" => self.fold(false),
            "enter" => return self.act("open"),
            "alt+enter" | "ctrl+e" => return self.act("editor"),
            "ctrl+o" => return self.act("app"),
            "ctrl+t" => return self.act("insert"),
            "ctrl+y" => return self.act("copy"),
            _ => {
                let f = self.focused();
                let before = f.field.value.clone();
                if f.restored && (ui::printable(k) || k == "backspace" || k == "delete") {
                    f.field.set("");
                    if ui::printable(k) {
                        f.field.key(k);
                    }
                } else if !f.field.key(k) {
                    return;
                }
                f.restored = false;
                if f.field.value != before {
                    return self.edited();
                }
            }
        }
        self.render();
    }

    fn edited(&mut self) {
        let s = &mut self.state;
        (s.cursor, s.top, s.touched) = (0, 0, false);
        if s.mode == Mode::Files {
            return self.refresh();
        }
        self.schedule_search();
        self.render();
    }

    fn paste(&mut self, text: &str) {
        let f = self.focused();
        if f.restored {
            f.field.set("");
        }
        f.restored = false;
        f.field.insert(text);
        self.edited();
    }

    fn mouse(&mut self, m: &Mouse) {
        if let Some(help) = &mut self.state.help {
            if !ui::help_mouse(m, help) {
                self.state.help = None;
            }
            return self.render();
        }
        let (_, h) = self.size();
        let h = h as i64;
        // A click on a hint of the footer does what its key does.
        if m.y == h
            && m.press
            && !m.drag
            && m.wheel == 0
            && m.button == 0
            && let Some(k) = ui::key_at(&self.state.hints, m.x - 1, 0)
        {
            return self.key(&k);
        }
        if m.wheel != 0 {
            let max = self.rows().len() as i64 - self.list_height();
            self.state.top = (self.state.top + m.wheel * 3).min(max).max(0);
            return self.render();
        }
        // screen rows above the list
        let first = if self.state.mode == Mode::Content { 4 } else { 3 };
        match self.scrollbar.mouse(m, m.y - 1 - first, m.x == self.list_w, Instant::now()) {
            ui::BarHit::Ignored => {}
            ui::BarHit::Took => return,
            ui::BarHit::Scroll(top) => {
                self.state.top = top;
                return self.render();
            }
        }
        if !m.press || m.drag || m.button != 0 {
            return;
        }
        if let Some(act) = self.zones.iter().find(|z| z.y == m.y - 1 && m.x >= z.x1 && m.x <= z.x2).map(|z| z.act) {
            return match act {
                Act::Mode(mode) => self.set_mode(mode),
                Act::Toggle(o) => self.toggle(o),
                Act::Focus(f) => self.focus(f),
            };
        }
        let i = self.state.top + m.y - 1 - first;
        let r = usize::try_from(i).ok().and_then(|i| self.rows().get(i).copied());
        // not on the preview on the right
        let Some(r) = r.filter(|_| m.y > first && m.y < h && m.x <= self.list_w) else { return };
        let now = Instant::now();
        let double =
            self.state.last_click.is_some_and(|(li, t)| li == i && now.duration_since(t) < Duration::from_millis(400));
        self.state.last_click = if double { None } else { Some((i, now)) };
        self.select(i);
        if double {
            return self.act("open");
        }
        if let Row::Head(gi) = r
            && self.state.mode == Mode::Content
        {
            let collapsed = self.state.groups[gi].collapsed;
            self.fold(!collapsed);
        }
        self.render();
    }
}

/// The segments of each line, coloured as in the preview pane (plain when the language is unknown or the file large),
/// their text the line's with tabs expanded and control characters dropped (ui::clean_line).
fn coloured(rel: &str, lines: &[String], size: usize) -> Vec<Vec<Seg>> {
    let lang =
        if size <= MAX_HIGHLIGHT { crate::render::highlight::language(&paths::basename(rel, None)) } else { None };
    let segs = lang.map(|l| crate::render::highlight::highlight(&lines.join("\n"), Some(l)));
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let t = ui::clean_line(l, 4, None);
            match segs.as_ref().and_then(|s| s.get(i)) {
                Some(s) if ui::seg_text(s) == t => s.clone(),
                _ => vec![ui::seg(t, "")],
            }
        })
        .collect()
}
