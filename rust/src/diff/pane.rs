//! The Changes pane (src/diff.js's main): git changes of a file or folder, in a temporary side pane: compared with the
//! tab's scope (b), inline or side by side (v), staged, unstaged or discarded by the rows selected (s, u, D), with
//! review notes (a) and files marked viewed (m); q or Esc closes it.
//!
//! As in the other panes, the herdr calls run on a worker thread, git on a second one, a staging that may ask about
//! the trash on a thread of its own; their results come back to the pane's loop, the JS's timers are deadlines of
//! that loop.

use super::{Diff, File, Ref, Row, View, changes_of, collect, disk_lines, layout_of, render, save_mode, shown_lines};
use crate::core::scope::{self, Pick, RevKey, Scope};
use crate::core::screen::{self, Input, Screen};
use crate::core::stage::{self, Mode, Sel};
use crate::core::ui::{
    self, Choice, Event, Field, HelpState, Hints, Menu, MenuItem, Mouse, Scrollbar, Seg, c, fg, seg,
};
use crate::core::watch::Watch;
use crate::core::{actions, exec, js, notes, paths, viewed, worker};
use crate::json::Value;
use crate::sys;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

type Then = worker::Then<Pane>;
type Worker = worker::Worker<Pane>;
/// A switch of the header: its segments, its width, its choices (id, from, to in columns from 0).
type Switch = (Vec<Seg>, usize, Vec<(&'static str, usize, usize)>);

/// What an item of the menus does: the view's options, then the scopes of b's menus.
#[derive(Clone, Debug, PartialEq)]
enum Opt {
    Layout,
    Wrap,
    Compare,
    Scope(Pick),
}

/// A note while it is typed: its field, its kind, the lines of the row it was taken on.
struct Draft {
    field: Field,
    kind: String,
    r#ref: Ref,
}

/// What a question of the footer does on y: discard the rows selected then, or delete for good the untracked files
/// the trash did not take (the answer sent to the staging's thread, no on any other key).
enum Asked {
    /// The rows picked from the diff then shown: they name its files, hunks and lines.
    Discard(Sel, Mode, Arc<Diff>),
    NoTrash(Sender<bool>),
}

struct Confirm {
    text: String,
    asked: Asked,
}

/// A choice of the header's switches, in columns from 1.
struct Chip {
    id: &'static str,
    from: i64,
    to: i64,
}

fn dim() -> String {
    fg(c().dim)
}
fn green() -> String {
    fg(c().green)
}
fn red() -> String {
    fg(ui::git('D').unwrap_or(c().red))
}
fn cyan() -> String {
    fg(c().blue)
}
fn note_style() -> String {
    fg(c().yellow)
}

/// A file of the diff by its path as git gives it: the new one, else the old one.
fn name_of(f: &File) -> Option<&str> {
    f.to.as_deref().filter(|n| !n.is_empty()).or(f.from.as_deref()).filter(|n| !n.is_empty())
}

/// Where a note points: the file, its lines.
fn note_where(r: &Ref) -> String {
    let lines = match (r.from, r.to) {
        (0, _) => String::new(),
        (from, to) if to > from => format!(":{from}-{to}"),
        (from, _) => format!(":{from}"),
    };
    format!("{}{lines}", r.file)
}

fn help_groups() -> Vec<ui::Group> {
    let group = |title: &str, pairs: &[(&str, &str)]| -> ui::Group {
        (title.into(), pairs.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect())
    };
    vec![
        group(
            "Move",
            &[
                ("↑↓ j k", "line"),
                ("pgup pgdn", "page"),
                ("space", "page down"),
                ("ctrl+u/d", "half page"),
                ("g G", "top, bottom"),
            ],
        ),
        group(
            "Diff",
            &[
                (",", "the view's options (or ⚙)"),
                ("n N", "next, previous hunk"),
                ("v", "inline, split (side by side)"),
                ("w", "wrap long lines"),
                ("← → h l", "left, right (no wrap)"),
                ("b", "compare with: HEAD, merge base, last agent turn, a commit or range, a branch"),
            ],
        ),
        group(
            "Stage (HEAD)",
            &[
                ("shift+↑↓ J K", "select lines"),
                ("s", "stage the line, hunk (@@ row) or file (name row)"),
                ("u", "unstage it"),
                ("D", "discard it from the working tree"),
            ],
        ),
        group(
            "Review",
            &[
                ("a", "note on the line (hunk, file)"),
                ("tab", "kind of note, while typing"),
                ("A", "send the notes to the agent"),
                ("C", "copy the notes"),
                ("U", "remove the last note"),
                ("y", "copy path:line"),
                ("m", "mark the file viewed: folded until it changes"),
                ("] [", "next, previous file not viewed"),
            ],
        ),
        group(
            "Pane",
            &[
                ("ctrl+p", "go to file"),
                ("ctrl+f", "search in files"),
                ("z", "zoom the pane"),
                ("? f1", "keys"),
                ("q esc", "close"),
            ],
        ),
    ]
}

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
        crate::core::exec::home(None);
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

    fn got_tab(&mut self, tab: Option<String>) {
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
    fn size(&self) -> (usize, usize) {
        sys::size()
            .map_or((80, 24), |(w, h)| (if w == 0 { 80 } else { w as usize }, if h == 0 { 24 } else { h as usize }))
    }

    fn body(&self) -> i64 {
        self.size().1 as i64 - 2
    }

    fn exit(&mut self) -> ! {
        self.screen.exit(0)
    }

    // --- the changes

    /// The changes for the scope chosen now, parsed, on the git worker; None when b chose another one meanwhile,
    /// whose run then shows its own.
    fn changes(&mut self, then: impl FnOnce(&mut Pane, Option<Arc<Diff>>) + Send + 'static) {
        self.runs += 1;
        let run = self.runs;
        let (cwd, file, scope) = (self.cwd.clone(), self.file.clone(), self.scope.clone());
        let turn = match &self.tab {
            Some(tab) if scope.kind() == "turn" => Some(scope::read_turn(tab)),
            _ => None,
        };
        self.git.send(Box::new(move || {
            let res = std::panic::catch_unwind(|| collect(&cwd, &file, "head", Some(&scope), turn.as_ref()))
                .unwrap_or_else(|_| super::Collected { top: cwd.clone(), ..super::Collected::default() });
            let parsed = Arc::new(changes_of(&res));
            Box::new(move |p: &mut Pane| {
                if run != p.runs {
                    return then(p, None);
                }
                p.top = res.top;
                p.base_name = res.base_name;
                if let Some(e) = res.error {
                    p.say(e);
                }
                then(p, Some(parsed))
            })
        }));
    }

    fn set_parsed(&mut self, d: Arc<Diff>) {
        self.parsed = Some(d);
        self.disk.clear();
        self.cache.clear();
    }

    /// The changes loaded again, for a new scope.
    fn reload(&mut self) {
        self.parsed = None;
        self.cache.clear();
        self.draw();
        self.changes(|p, d| {
            let Some(d) = d else { return };
            p.set_parsed(d);
            if let Some(tab) = &p.tab {
                p.tab_notes = notes::current(tab, None);
            }
            p.read_viewed();
            p.draw();
            if p.scope.kind() == "branch" && p.base_name.is_empty() {
                p.say("No default branch: compared with HEAD".into());
            }
        });
    }

    fn real_of(&mut self, f: &str) -> String {
        if let Some(r) = self.reals.get(f) {
            return r.clone();
        }
        let r = exec::real_path(&paths::join(&[&self.top, f]));
        self.reals.insert(f.to_string(), r.clone());
        r
    }

    /// A row with a note of the tab on its lines. A staged row is at its index line, the notes at disk lines
    /// (note_key): compared where it is on disk.
    fn noted(&mut self, i: usize) -> bool {
        let Some(r) = self.rows[i].r#ref.clone() else { return false };
        if r.from == 0 || self.tab_notes.is_empty() {
            return false;
        }
        let at = if r.sec == "staged"
            && let Some(parsed) = &self.parsed
        {
            let key = (r.file.clone(), r.from, r.to);
            *self.disk.entry(key).or_insert_with(|| disk_lines(parsed, &r))
        } else {
            Some((r.from, r.to))
        };
        let Some((from, to)) = at else { return false };
        let real = Value::Str(self.real_of(&r.file));
        self.tab_notes.iter().any(|n| {
            n.get("from").is_some_and(Value::truthy)
                && !n.get("stale").is_some_and(Value::truthy)
                && js::strict_eq(n.get("file"), Some(&real))
                && from as f64 >= js::number(n.get("from"))
                && to as f64 <= js::number(n.get("to"))
        })
    }

    /// The files marked viewed and unchanged since, among those of the diff.
    fn read_viewed(&mut self) {
        let map = self.tab.as_deref().map_or(Value::Obj(Vec::new()), viewed::read);
        let names: Vec<String> = match &self.parsed {
            Some(d) => d.files.iter().filter_map(name_of).map(String::from).collect(),
            None => Vec::new(),
        };
        let mut out = Vec::new();
        for n in names {
            if !out.contains(&n) && viewed::is(&map, &self.real_of(&n)) {
                out.push(n);
            }
        }
        self.viewed = out;
    }

    // --- drawing

    /// The layout's key: left is not in it, the rows cut from another column are made again when drawn (redo).
    fn key_of(&self, width: usize, m: &str) -> String {
        format!("{width}:{m}:{}:{}", self.wrap, self.viewed.join("\0"))
    }

    fn layout(&mut self, width: usize, m: &str) {
        let key = self.key_of(width, m);
        let Some(parsed) = self.parsed.clone() else { return };
        if key == self.cache {
            return;
        }
        // The cursor stays on the same lines when the layout changes.
        let at = |r: &Row| r.r#ref.is_some() && !r.more;
        let was = usize::try_from(self.cur).ok().and_then(|i| self.rows.get(i));
        let (was, hunk) = (was.and_then(|r| r.r#ref.clone()), was.is_some_and(|r| r.hunk));
        let viewed = &self.viewed;
        let folded = |f: &File| name_of(f).is_some_and(|n| viewed.iter().any(|v| v == n));
        let view = View { wrap: self.wrap, left: 0, viewed: Some(&folded) };
        self.rows = render(&parsed, width, m, false, &view);
        self.cache = key;
        let rows = &self.rows;
        let mut i = match (self.stay, &was) {
            (Some(stay), _) => rows.iter().enumerate().position(|(k, r)| k as i64 >= stay && at(r)),
            (None, Some(was)) => rows.iter().position(|r| {
                at(r)
                    && r.r#ref.as_ref().is_some_and(|x| x.file == was.file && x.from == was.from && x.sec == was.sec)
                    && r.hunk == hunk
            }),
            (None, None) => None,
        };
        if i.is_none() && self.stay.is_some() {
            i = rows.iter().rposition(at);
        }
        self.cur = i.or_else(|| rows.iter().position(|r| r.r#ref.is_some())).map_or(-1, |i| i as i64);
        self.stay = None;
        self.anchor = None;
    }

    /// The rows selected: the cursor's, or from the anchor to it.
    fn range(&self) -> (i64, i64) {
        match self.anchor {
            None => (self.cur, self.cur),
            Some(a) => (a.min(self.cur), a.max(self.cur)),
        }
    }

    fn title(&self) -> String {
        if self.file.is_empty() {
            return paths::basename(&self.cwd, None);
        }
        let rel = paths::relative(&self.cwd, &self.file);
        if rel.is_empty() { paths::basename(&self.file, None) } else { rel }
    }

    fn draw(&mut self) {
        let (w, h) = self.size();
        let m = layout_of(w);
        let body = h as i64 - 2;
        // The first column holds the cursor's bar, the last the scrollbar when it shows (a narrower layout never has
        // fewer rows): one column narrower then, kept while it shows.
        if self.cache != self.key_of(w.saturating_sub(2), m) || self.rows.len() as i64 <= body {
            self.layout(w.saturating_sub(1), m);
        }
        if self.rows.len() as i64 > body {
            self.layout(w.saturating_sub(2), m);
        }
        let len = self.rows.len() as i64;
        self.top0 = self.top0.min(len - body).max(0);
        let bw = self.scrollbar.set(len, body, self.top0, &[]);
        // Header: the name and the counts, then the scope (a click opens b's menu) and ⚙, which opens the view's
        // options (,), as in the preview's Diff view; the scope left out when they do not fit.
        let mut head = vec![seg(format!(" {}", self.title()), "\x1b[1m")];
        if let Some(d) = &self.parsed {
            head.push(seg(format!(" +{}", d.add), green()));
            head.push(seg(format!(" −{}", d.del), red()));
        }
        let gear_style = fg(if self.menu.is_some() { c().text } else { c().dim });
        let label = scope::label(&self.scope, &self.base_name);
        let (scope_segs, scope_w, scope_hits) = ui::segmented(&[("scope", &label)], "scope");
        let mut switches: Vec<Switch> = vec![
            (scope_segs, scope_w, scope_hits.iter().map(|h| ("scope", h.from, h.to)).collect()),
            (vec![seg(format!(" {} ", ui::g().gear), gear_style)], 3, vec![("menu", 0, 2)]),
        ];
        let wi = w as i64;
        let head_w = ui::seg_width(&head) as i64;
        let width = |s: &[Switch]| s.iter().map(|x| x.1 as i64 + 1).sum::<i64>();
        while !switches.is_empty() && wi - head_w - width(&switches) < 1 {
            switches.remove(0);
        }
        self.chips.clear();
        let mut x = wi - width(&switches) + 1;
        let mut segs = head;
        segs.push(seg(" ".repeat((wi - head_w - width(&switches)).max(0) as usize), ""));
        for (s, sw, hits) in switches {
            segs.extend(s);
            segs.push(seg(" ", ""));
            for (id, from, to) in hits {
                self.chips.push(Chip { id, from: x + from as i64, to: x + to as i64 });
            }
            x += sw as i64 + 1;
        }
        let mut lines = vec![ui::line(&segs, w, Some(c().header))];
        let body_u = body.max(0) as usize;
        if self.parsed.is_none() {
            lines.extend(ui::empty::<&str, &str>(w, body_u, "Loading…", &[]));
        } else if self.rows.is_empty() {
            lines.extend(ui::empty(w, body_u, "No changes", &[("b", "compare with"), ("q", "close")]));
        } else {
            // The cursor's row and the others selected, and the noted ones: a bar in the first column.
            let (from, to) = self.range();
            let end = (self.top0 + body).min(len);
            for i in self.top0..end {
                let mark = if i >= from && i <= to {
                    seg("▌", fg(c().blue))
                } else if self.noted(i as usize) {
                    seg("▎", note_style())
                } else {
                    seg(" ", "")
                };
                let r = &self.rows[i as usize];
                let mut segs = vec![mark];
                match &r.redo {
                    Some(redo) if self.left > 0 => segs.extend(redo(self.left)),
                    _ => segs.extend(r.segs.iter().cloned()),
                }
                let back = if i == self.cur && r.back.is_none() { Some(c().select_blur) } else { r.back };
                lines.push(ui::line(&segs, w.saturating_sub(bw), back) + &self.scrollbar.cell(i - self.top0));
            }
        }
        while lines.len() < h.saturating_sub(1) {
            lines.push(ui::line(&[], w, None));
        }
        lines.truncate(h.saturating_sub(1));
        let foot = self.footer(w);
        lines.push(foot);
        if let Some(menu) = &mut self.menu {
            menu.draw(&mut lines, wi, h as i64);
        }
        if let Some(help) = &mut self.help {
            ui::help(&mut lines, w, "Changes", &help_groups(), help);
        }
        self.screen.paint(&lines);
    }

    fn footer(&mut self, w: usize) -> String {
        self.hints = Hints::default();
        let header = Some(c().header);
        let room = |lead: &str| (w as i64 - ui::text_width(lead) as i64).max(1);
        if let Some(f) = &self.prompt {
            let lead = " Commit or range (abc123, main..HEAD): ";
            let mut segs = vec![seg(lead, cyan())];
            segs.extend(f.segments(room(lead), ""));
            return ui::line(&segs, w, header);
        }
        if let Some(q) = &self.confirm {
            let mut segs = vec![seg(format!(" {} ", q.text), fg(c().red))];
            segs.extend(ui::keys(&[("y", "yes"), ("n", "no")], w).segs);
            return ui::line(&segs, w, header);
        }
        if let Some(n) = &self.note {
            let lead = format!(" ✎ {} ", note_where(&n.r#ref));
            let kind = if n.kind.is_empty() { "[tab: kind] ".to_string() } else { format!("[{}] ", n.kind) };
            let mut segs = vec![seg(lead.clone(), note_style()), seg(kind.clone(), dim())];
            segs.extend(n.field.segments(room(&format!("{lead}{kind}")), ""));
            return ui::line(&segs, w, header);
        }
        if let Some(text) = &self.flash {
            return ui::line(&ui::message(text, None), w, header);
        }
        // The keys, and how far down the diff is.
        let body = self.body();
        let len = self.rows.len() as i64;
        let pct = if len > body {
            format!(" {}% ", (100.0 * self.top0 as f64 / (len - body).max(1) as f64 + 0.5).floor())
        } else {
            String::new()
        };
        let mut pairs: Vec<(String, String)> = Vec::new();
        let n = self.tab_notes.len();
        if n > 0 {
            pairs.push(("A".into(), format!("send {n} note{}", if n > 1 { "s" } else { "" })));
        }
        // b and v are left out: the header's switches show them (their keys in the help).
        if self.scope.kind() == "head" {
            pairs.push(("s".into(), "stage".into()));
            pairs.push(("u".into(), "unstage".into()));
        }
        for (k, a) in [("a", "note"), ("n N", "hunks"), ("q", "close"), ("?", "keys")] {
            pairs.push((k.into(), a.into()));
        }
        self.hints = ui::keys(&pairs, w.saturating_sub(pct.len()));
        let mut segs = self.hints.segs.clone();
        let pad = w.saturating_sub(ui::seg_width(&segs));
        segs.push(seg(format!("{pct:>pad$}"), dim()));
        ui::line(&segs, w, header)
    }

    fn say(&mut self, text: String) {
        if text.is_empty() {
            return self.draw();
        }
        let time = ui::message_time(ui::message_kind(&text));
        self.flash = Some(text);
        self.draw();
        self.flash_at = Some(Instant::now() + Duration::from_millis(time));
    }

    /// An action on the herdr worker, its message said once done; "Action failed" when it panicked.
    fn act(&mut self, f: impl FnOnce() -> Option<String> + Send + 'static) {
        self.herdr.send(Box::new(move || {
            let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or_else(|_| Some("Action failed".into()))
                .unwrap_or_default();
            Box::new(move |p: &mut Pane| p.say(text))
        }));
    }

    // --- moving

    /// The cursor moves over the rows that show lines of a file, the first row of a wrapped line; the view follows
    /// it. select: shift held, the rows from where the cursor was are selected, else the selection goes.
    fn move_by(&mut self, n: i64, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        let dir = n.signum();
        let (mut i, mut todo) = (self.cur + dir, n.abs());
        while todo > 0 && i >= 0 && i < self.rows.len() as i64 {
            let r = &self.rows[i as usize];
            if r.r#ref.is_some() && !r.more {
                self.cur = i;
                todo -= 1;
            }
            i += dir;
        }
        self.follow();
        self.draw();
    }

    fn follow(&mut self) {
        let body = self.body();
        if self.cur < self.top0 {
            self.top0 = self.cur;
        }
        if self.cur >= self.top0 + body {
            self.top0 = self.cur - body + 1;
        }
    }

    fn scroll(&mut self, n: i64) {
        self.top0 += n;
        self.draw();
    }

    /// Next or previous hunk: its header comes to the top, with the cursor.
    fn jump(&mut self, dir: i64) {
        let cur = self.cur;
        let ok = |(k, r): &(usize, &Row)| r.hunk && if dir > 0 { *k as i64 > cur } else { (*k as i64) < cur };
        let found =
            if dir > 0 { self.rows.iter().enumerate().find(ok) } else { self.rows.iter().enumerate().rev().find(ok) };
        if let Some((i, _)) = found {
            self.cur = i as i64;
            self.top0 = i as i64;
            self.draw();
        }
    }

    /// m: the cursor's file viewed, or no longer.
    fn mark_viewed(&mut self) {
        let Some(r) = self.cur_ref() else { return };
        let Some(tab) = self.tab.clone() else { return self.say("Needs herdr".into()) };
        let real = self.real_of(&r.file);
        let now = viewed::toggle(&tab, &real);
        self.read_viewed();
        self.say(if now { "Viewed: folded until it changes" } else { "No longer viewed" }.into());
    }

    /// ] and [: the next or previous file not viewed, its name at the top.
    fn next_file(&mut self, dir: i64) {
        let cur = self.cur;
        let viewed = &self.viewed;
        let ok = |(k, r): &(usize, &Row)| {
            r.f.is_some()
                && r.r#ref.as_ref().is_some_and(|x| !viewed.contains(&x.file))
                && if dir > 0 { *k as i64 > cur } else { (*k as i64) < cur }
        };
        let found =
            if dir > 0 { self.rows.iter().enumerate().find(ok) } else { self.rows.iter().enumerate().rev().find(ok) };
        let Some((i, _)) = found else {
            return self.say(if dir > 0 { "No next file to view" } else { "No previous file to view" }.into());
        };
        self.cur = i as i64;
        self.top0 = i as i64;
        self.anchor = None;
        self.draw();
    }

    fn cur_ref(&self) -> Option<Ref> {
        usize::try_from(self.cur).ok().and_then(|i| self.rows.get(i)).and_then(|r| r.r#ref.clone())
    }

    // --- notes

    fn start_note(&mut self) {
        let Some(r) = self.cur_ref() else { return };
        if self.tab.is_none() {
            return self.say("Notes need herdr".into());
        }
        self.note = Some(Draft { field: Field::new(""), kind: String::new(), r#ref: r });
        self.draw();
    }

    fn note_key(&mut self, k: &str) {
        let Some(n) = self.note.as_mut() else { return };
        match k {
            "escape" | "ctrl+c" => {
                self.note = None;
                self.draw()
            }
            "tab" | "shift+tab" => {
                let len = notes::KINDS.len() as i64;
                let i = notes::KINDS.iter().position(|x| *x == n.kind).map_or(-1, |i| i as i64);
                let step = if k == "tab" { 1 } else { len - 1 };
                n.kind = notes::KINDS[((i + step) % len) as usize].to_string();
                self.draw()
            }
            "enter" => {
                let Some(n) = self.note.take() else { return };
                if n.field.value.chars().all(js::is_space) {
                    return self.draw();
                }
                let r = &n.r#ref;
                // A staged line: placed where it is on disk, which notes follow; stale when changed there since.
                let disk = match &self.parsed {
                    Some(parsed) if r.from != 0 && r.sec == "staged" => Some(disk_lines(parsed, r)),
                    _ => None,
                };
                let (from, to) = match disk {
                    Some(Some(d)) => d,
                    _ => (r.from, r.to),
                };
                let note = notes::New {
                    file: paths::join(&[&self.top, &r.file]),
                    from: from.max(0) as u64,
                    to: Some(to.max(0) as u64),
                    kind: n.kind.clone(),
                    text: n.field.value.clone(),
                    lines: self.parsed.as_ref().map(|parsed| shown_lines(parsed, r)),
                    stale: matches!(disk, Some(None)),
                };
                let tab = self.tab.clone().unwrap_or_default();
                let msg = notes::add(&tab, note);
                self.tab_notes = notes::current(&tab, None);
                self.say(msg)
            }
            _ => {
                if n.field.key(k) {
                    self.draw();
                }
            }
        }
    }

    /// y: the cursor's lines, relative to the pane's folder (src/app.js:12-14), to paste into a prompt.
    fn copy_reference(&mut self) {
        let Some(r) = self.cur_ref() else { return };
        // From the folder resolved as top is (git gives its real path): /home → /var/home would otherwise give
        // ../../..
        let rel = paths::relative(&exec::real_path(&self.cwd), &paths::join(&[&self.top, &r.file]));
        let rel = if paths::SEP == '/' { rel } else { rel.replace(paths::SEP, "/") };
        let text = note_where(&Ref { file: rel, ..r.clone() });
        let done = if r.from != 0 { "Reference copied" } else { "Path copied" };
        self.act(move || Some(actions::copy(&text, done)));
    }

    // --- staging

    /// s, u, D: the rows selected staged, unstaged or discarded (core::stage), with HEAD as the scope only (the
    /// others show changes the index has nothing to do with); D asks first. The cursor then stays where it was.
    fn edit(&mut self, mode: Mode) {
        let Some(parsed) = self.parsed.clone() else { return };
        if self.rows.is_empty() {
            return;
        }
        if self.busy {
            return self.say("Busy…".into());
        }
        if self.scope.kind() != "head" {
            return self.say("Not with this scope: stage compared with HEAD (b)".into());
        }
        let (from, to) = self.range();
        // rows.slice(from, to + 1): nothing from a cursor on no row.
        let picked = match (usize::try_from(from), usize::try_from(to)) {
            (Ok(a), Ok(b)) => stage::picked(self.rows.get(a..=b.min(self.rows.len() - 1)).unwrap_or(&[])),
            _ => Sel::default(),
        };
        if mode != Mode::Discard {
            return self.go(picked, mode, &parsed);
        }
        let n = stage::count(&parsed, &picked, mode);
        if n.is_empty() {
            return self.say("Nothing unstaged selected".into());
        }
        self.confirm = Some(Confirm {
            text: format!("Discard {n} from the working tree?"),
            asked: Asked::Discard(picked, mode, parsed),
        });
        self.draw();
    }

    /// Does it on a thread of its own: untracked files the trash did not take are deleted for good once asked again.
    /// The rows were picked from the diff of: the JS's picked holds its objects, which name nothing in a diff
    /// read again since (nothing selected then).
    fn go(&mut self, picked: Sel, mode: Mode, of: &Arc<Diff>) {
        self.confirm = None;
        if self.busy {
            return self.say("Busy…".into());
        }
        self.busy = true;
        let (top, parsed) = (self.top.clone(), self.parsed.clone());
        let picked = if parsed.as_ref().is_some_and(|p| Arc::ptr_eq(p, of)) { picked } else { Sel::default() };
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        let started = worker::spawn(
            self.tx.clone(),
            self.waker.clone(),
            Box::new(move || {
                let no_trash = |names: &[String]| {
                    let (answer, wait) = channel();
                    let name = if names.len() > 1 { format!("{} files", names.len()) } else { names[0].clone() };
                    let ask: Then = Box::new(move |p: &mut Pane| {
                        let text = format!("No trash: delete {name} for good?");
                        p.confirm = Some(Confirm { text, asked: Asked::NoTrash(answer) });
                        p.draw();
                    });
                    if tx.send(ask).is_err() {
                        return false;
                    }
                    waker.wake();
                    wait.recv().unwrap_or(false)
                };
                let text = parsed
                    .and_then(|parsed| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            stage::apply(&top, &parsed, &picked, mode, &actions::trash, Some(&no_trash))
                        }))
                        .ok()
                    })
                    .unwrap_or_else(|| format!("Cannot {}", mode.name()));
                Box::new(move |p: &mut Pane| {
                    p.say(text);
                    p.changes(|p, d| {
                        p.busy = false;
                        let Some(d) = d else { return };
                        p.set_parsed(d);
                        p.stay = Some(p.cur);
                        p.read_viewed();
                        p.draw();
                    });
                })
            }),
        );
        if !started {
            self.busy = false;
            self.say(format!("Cannot {}", mode.name()));
        }
    }

    // --- scopes and the view's options

    /// A scope chosen here, for every pane of the tab; the current one chosen again reloads it (an agent turn being
    /// recorded, for instance).
    fn pick(&mut self, s: Scope) {
        self.menu = None;
        self.prompt = None;
        if !scope::same(&s, &self.scope) {
            if let Some(tab) = &self.tab {
                scope::write(tab, &s);
            }
            self.scope = s;
        }
        self.reload();
    }

    /// The tab's scope, chosen in another pane.
    fn follow_scope(&mut self) {
        let Some(tab) = &self.tab else { return };
        let s = scope::read(tab);
        if !scope::same(&s, &self.scope) {
            self.scope = s;
            self.reload();
        }
    }

    /// The view's options (⚙ or ,), as in the preview's Diff view: each changes in place, the menu staying open.
    fn open_view(&mut self, sel: usize) {
        let w = self.size().0;
        let mut items: Vec<Option<MenuItem<Opt>>> =
            super::view_items(layout_of(w), self.wrap, Opt::Layout, Opt::Wrap).into_iter().map(Some).collect();
        items.push(Some(MenuItem {
            label: "Compare with".into(),
            key: "b".into(),
            value: Some(seg(scope::label(&self.scope, &self.base_name), cyan())),
            danger: false,
            keep: false,
            run: Opt::Compare,
        }));
        let mut menu = Menu::new("View", items, 1, w as i64, 0);
        menu.sel = sel;
        self.menu = Some(menu);
        self.draw();
    }

    /// A choice of a menu, by key or click: run; the view's options shown again with the new values, unless closed.
    fn menu_choice(&mut self, r: Option<Choice<Opt>>) {
        let sel = self.menu.as_ref().map_or(0, |m| m.sel);
        match r {
            None => {}
            Some(Choice::Close) => self.menu = None,
            Some(Choice::Run { run, keep }) => {
                self.menu = None;
                match run {
                    Opt::Layout => self.switch_layout(),
                    Opt::Wrap => self.toggle_wrap(),
                    Opt::Compare => self.open_scopes(),
                    Opt::Scope(Pick::Scope(s)) => self.pick(s),
                    Opt::Scope(Pick::Ask(kind)) => self.ask_scope(kind),
                }
                if keep {
                    self.open_view(sel);
                }
            }
        }
        self.draw();
    }

    fn switch_layout(&mut self) {
        let w = self.size().0;
        save_mode(w, if layout_of(w) == "split" { "inline" } else { "split" });
        self.draw();
    }

    fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.left = 0;
        let mut settings = exec::read_settings();
        settings.set("wrap", Value::Bool(self.wrap));
        exec::write_settings(&settings);
        self.draw();
    }

    fn open_scopes(&mut self) {
        let w = self.size().0 as i64;
        let m = scope::menu(&self.scope, &self.base_name, w);
        let items = m
            .items
            .into_iter()
            .map(|it| {
                it.map(|it| MenuItem {
                    label: it.label,
                    key: it.key,
                    value: it.value,
                    danger: it.danger,
                    keep: it.keep,
                    run: Opt::Scope(it.run),
                })
            })
            .collect();
        self.menu = Some(Menu::new(&m.title, items, m.at, m.right, m.left));
        self.draw();
    }

    /// Commit or range… and Branch…: the menu hidden, then the rev typed before prompted, or the branches' menu.
    fn ask_scope(&mut self, kind: &str) {
        self.menu = None;
        self.draw();
        if kind == "rev" {
            self.prompt = Some(Field::new(scope::prompt_rev(&self.scope)));
            return self.draw();
        }
        let (cwd, now) = (self.cwd.clone(), self.scope.clone());
        self.git.send(Box::new(move || {
            let list = scope::branches(&cwd);
            Box::new(move |p: &mut Pane| {
                let w = p.size().0 as i64;
                match scope::branch_menu(&now, &list, w) {
                    None => p.say("No branch".into()),
                    Some(m) => {
                        let items = m
                            .items
                            .into_iter()
                            .map(|it| {
                                it.map(|it| MenuItem {
                                    label: it.label,
                                    key: it.key,
                                    value: it.value,
                                    danger: it.danger,
                                    keep: it.keep,
                                    run: Opt::Scope(it.run),
                                })
                            })
                            .collect();
                        p.menu = Some(Menu::new(&m.title, items, m.at, m.right, m.left));
                        p.draw();
                    }
                }
            })
        }));
    }

    fn prompt_key(&mut self, k: &str) {
        let Some(field) = self.prompt.as_mut() else { return };
        match scope::rev_key(field, k) {
            RevKey::Changed(changed) => {
                if changed {
                    self.draw();
                }
            }
            RevKey::Done(rev) => {
                self.prompt = None;
                if rev.is_empty() {
                    return self.draw();
                }
                self.pick(Scope::new("rev", Some(&rev)));
            }
        }
    }

    // --- input

    fn event(&mut self, e: Event) {
        match e {
            Event::Key(k) => self.key(&k),
            Event::Mouse(m) => self.mouse(&m),
            // Left zoomed, the tab would show alone whatever pane takes the focus (actions::unzoom).
            Event::Focus(on) => {
                if !on {
                    let pane = self.pane.clone();
                    self.herdr.send(Box::new(move || {
                        actions::unzoom(&pane);
                        Box::new(|_: &mut Pane| {})
                    }));
                }
            }
            Event::Paste { text, .. } => {
                let f = match (&mut self.prompt, &mut self.note) {
                    (Some(f), _) => Some(f),
                    (None, Some(n)) => Some(&mut n.field),
                    _ => None,
                };
                if let Some(f) = f {
                    f.insert(&text);
                    self.draw();
                }
            }
            Event::Reply(_) => {}
        }
    }

    fn key(&mut self, k: &str) {
        let page = self.size().1 as i64 - 3;
        if self.note.is_some() {
            return self.note_key(k);
        }
        if self.prompt.is_some() {
            return self.prompt_key(k);
        }
        if let Some(q) = self.confirm.take() {
            let yes = k == "y" || k == "enter";
            return match q.asked {
                Asked::Discard(sel, mode, of) if yes => self.go(sel, mode, &of),
                Asked::NoTrash(answer) => {
                    let _ = answer.send(yes);
                    if !yes {
                        self.draw();
                    }
                }
                Asked::Discard(..) => self.draw(),
            };
        }
        let h = self.size().1 as i64;
        if let Some(menu) = &mut self.menu {
            let r = menu.key(k, h);
            return self.menu_choice(r);
        }
        if k == "ctrl+c" {
            self.exit();
        }
        if let Some(help) = &mut self.help {
            if !ui::help_key(k, help) {
                self.help = None;
            }
            return self.draw();
        }
        let tab = || self.tab.clone().unwrap_or_default();
        match k {
            "?" | "f1" => {
                self.help = Some(HelpState::default());
                self.draw();
            }
            "escape" if self.anchor.is_some() => {
                self.anchor = None;
                self.draw();
            }
            "q" | "escape" => self.exit(),
            "up" | "k" => self.move_by(-1, false),
            "down" | "j" | "enter" => self.move_by(1, false),
            "shift+up" | "K" => self.move_by(-1, true),
            "shift+down" | "J" => self.move_by(1, true),
            "pageup" => self.move_by(-page, false),
            "pagedown" | "space" => self.move_by(page, false),
            "ctrl+u" => self.move_by(-((page as f64 / 2.0).ceil() as i64), false),
            "ctrl+d" => self.move_by((page as f64 / 2.0).ceil() as i64, false),
            "z" => {
                let pane = self.pane.clone();
                self.herdr.send(Box::new(move || {
                    actions::zoom(&pane);
                    Box::new(|_: &mut Pane| {})
                }));
            }
            "ctrl+p" | "ctrl+f" => {
                // Go to file and Search in files from the diff's folder, as from the explorer.
                let (cwd, pane) = (self.cwd.clone(), self.pane.clone());
                let mode = if k == "ctrl+p" { "files" } else { "content" };
                self.act(move || actions::search(&cwd, &pane, mode, ""));
            }
            "home" | "g" => {
                self.cur = self.rows.iter().position(|r| r.r#ref.is_some()).map_or(-1, |i| i as i64);
                self.top0 = 0;
                self.draw();
            }
            "end" | "G" => {
                self.move_by(self.rows.len() as i64, false);
                self.top0 = self.rows.len() as i64;
                self.draw();
            }
            "n" => self.jump(1),
            "N" => self.jump(-1),
            "v" => self.switch_layout(),
            "," => self.open_view(0),
            "s" => self.edit(Mode::Stage),
            "u" => self.edit(Mode::Unstage),
            "D" => self.edit(Mode::Discard),
            "b" => self.open_scopes(),
            "w" => self.toggle_wrap(),
            "left" | "h" => {
                if !self.wrap && self.left > 0 {
                    self.left = self.left.saturating_sub(8);
                    self.draw();
                }
            }
            "right" | "l" => {
                let widest = self.parsed.as_deref().map_or(0, super::widest_of);
                if !self.wrap && self.left + 8 < widest {
                    self.left += 8;
                    self.draw();
                }
            }
            "a" => self.start_note(),
            "A" => {
                let (tab, from) = (tab(), self.from.clone());
                self.act(move || Some(notes::send(&tab, &from)));
            }
            "C" => {
                let (tab, top) = (tab(), self.top.clone());
                self.act(move || Some(notes::copy(&tab, Some(&top))));
            }
            "U" => {
                let msg = notes::undo(&tab());
                self.say(msg);
            }
            "m" => self.mark_viewed(),
            "]" => self.next_file(1),
            "[" => self.next_file(-1),
            "y" => self.copy_reference(),
            _ => {}
        }
    }

    fn mouse(&mut self, m: &Mouse) {
        let (w, h) = self.size();
        let h = h as i64;
        if let Some(help) = &mut self.help {
            if !ui::help_mouse(m, help) {
                self.help = None;
            }
            return self.draw();
        }
        if let Some(menu) = &mut self.menu {
            let r = menu.mouse(m, h);
            return self.menu_choice(r);
        }
        if m.wheel != 0 {
            return self.scroll(m.wheel * 3);
        }
        let click = m.press && !m.drag && m.button == 0;
        // The header's switches: a click on a choice takes it.
        if click && m.y == 1 {
            let chip = self.chips.iter().find(|c| m.x >= c.from && m.x <= c.to).map(|c| c.id);
            if let Some(id) = chip {
                match id {
                    "menu" => self.open_view(0),
                    _ => self.open_scopes(),
                }
                return;
            }
        }
        // A click on a hint of the footer does what its key does.
        if click
            && m.y == h
            && let Some(k) = ui::key_at(&self.hints, m.x - 1, 0)
        {
            return self.key(&k);
        }
        match self.scrollbar.mouse(m, m.y - 2, m.x == w as i64, Instant::now()) {
            ui::BarHit::Ignored => {}
            ui::BarHit::Took => return,
            ui::BarHit::Scroll(top) => {
                self.top0 = top;
                return self.draw();
            }
        }
        // A click on a row puts the cursor there; a drag selects the rows from there.
        let mut i = self.top0 + m.y - 2;
        let on_row = usize::try_from(i).ok().and_then(|i| self.rows.get(i)).is_some_and(|r| r.r#ref.is_some());
        if (click || (m.drag && m.button == 0)) && self.note.is_none() && m.y >= 2 && m.y < h && on_row {
            while i > 0 && self.rows[i as usize].more {
                i -= 1;
            }
            if click {
                self.anchor = None;
            } else if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
            self.cur = i;
            self.draw();
        }
    }
}
