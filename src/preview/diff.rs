//! The preview's git views: the Diff view, the file's changes compared with the tab's scope (b),
//! staged, unstaged or discarded by the rows selected (s, u, D); a commit, stash or tag of source control shown over
//! the tabs (enter_show); and the lines changed since HEAD beside the File view's lines (c, ] [).
//!
//! git runs on the pane's second worker (git), whose thread keeps diff::line_changes's cache; a staging that may ask
//! about the trash runs on a thread of its own, waiting for the answer.

use super::{Kind, Opt, Preview, Row, Rows};
use crate::core::buffer::{LineDiff, LineKind};
use crate::core::scope::{self, Pick, RevKey, Scope};
use crate::core::stage::{self, Mode};
use crate::core::ui::{self, Ask, BarMark, Button, Field, Menu, MenuItem, Seg, c, fg, seg, text_width};
use crate::core::{actions, paths, worker};
use crate::diff::{self, Collected, Diff};
use crate::json::{self, Value};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;

/// The keys of a commit shown: moving, hunks, layout, wrap, the options, zoom, width, help; q and Escape go back.
const SHOW_KEYS: [&str; 31] = [
    "?",
    "f1",
    "ctrl+c",
    "up",
    "k",
    "down",
    "j",
    "enter",
    "pageup",
    "pagedown",
    "space",
    "ctrl+u",
    "ctrl+d",
    "home",
    "g",
    "end",
    "G",
    "left",
    "h",
    "right",
    "l",
    "n",
    "N",
    "v",
    "w",
    ",",
    "z",
    "<",
    ">",
    "shift+left",
    "shift+right",
];

/// The Diff view's changes: loading, or the parsed diff, the repository's folder its paths are relative to, and the
/// default branch ("" when none).
pub enum DiffData {
    Loading,
    Done { parsed: Arc<Diff>, top: String, base_name: String },
}

/// A commit, stash or tag shown over the tabs: what source control asked for (diff::show_args's, with title and cwd;
/// or { blank } when nothing with changes is selected there), the parsed diff, the scroll of the tab under it. id:
/// which show this is, for the answers of git that come after another one took its place.
pub struct Show {
    pub what: Value,
    pub parsed: Option<Arc<Diff>>,
    pub loading: bool,
    pub failed: bool,
    pub back: (i64, i64),
    id: u64,
}

impl Show {
    fn blank(&self) -> bool {
        self.what.get("blank") == Some(&Value::Bool(true))
    }
}

/// The file's lines changed since HEAD (diff::line_changes); lines, those that start a block of changes.
pub struct Changes {
    file: String,
    diff: LineDiff,
    lines: Vec<usize>,
}

/// A line's change beside it: its sign's colour and its weight on the scrollbar, removed lines over changed ones over
/// added ones.
#[derive(Clone, Copy, PartialEq)]
enum Change {
    Add,
    Mod,
    Del,
}

impl Change {
    fn color(self) -> ui::Rgb {
        match self {
            Change::Add => c().green,
            Change::Mod => c().blue,
            Change::Del => c().red,
        }
    }

    fn weight(self) -> f64 {
        match self {
            Change::Add => 1.0,
            Change::Mod => 2.0,
            Change::Del => 3.0,
        }
    }
}

/// The nearest folder of the file still on disk (that of a deleted file may be gone).
fn near_dir(file: &str) -> String {
    let mut cwd = paths::dirname(file);
    while std::fs::metadata(&cwd).is_err() && paths::dirname(&cwd) != cwd {
        cwd = paths::dirname(&cwd);
    }
    cwd
}

/// The row of a line of the new version in the Diff view, or of the first change after it: -1 for none.
fn diff_row(rows: &Rows, line: f64) -> i64 {
    let at =
        |r: &Row| r.diff.as_ref().is_some_and(|d| !d.hunk && d.r#ref.as_ref().is_some_and(|x| x.from as f64 >= line));
    rows.list.iter().position(at).map_or(-1, |i| i as i64)
}

/// A scope menu as the view's menu takes it.
fn scope_menu(m: Menu<Pick>) -> Menu<Opt> {
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
    Menu::new(&m.title, items, m.at, m.right, m.left)
}

/// What source control asked to show, cleaned: None when it cannot be shown.
fn clean_show(s: Option<&Value>) -> Option<Value> {
    let s = s.filter(|s| matches!(s, Value::Obj(_)))?;
    let cwd = s.get("cwd").and_then(Value::as_str).filter(|c| paths::is_absolute(c))?;
    if s.get("blank") == Some(&Value::Bool(true)) {
        let text = s
            .get("text")
            .filter(|t| t.truthy())
            .map_or("Select a file, commit, stash or tag".into(), crate::json::text);
        // 80 characters at most.
        let mut text = ui::clean_line(&text, 4, None);
        if let Some((i, _)) = text.char_indices().nth(80) {
            text.truncate(i);
        }
        return Some(crate::obj! { "blank" => true, "text" => text, "title" => "", "cwd" => cwd });
    }
    let mut what = Value::Obj(Vec::new());
    // Fields left out when missing.
    for k in ["kind", "ref"] {
        if let Some(v) = s.get(k) {
            what.set(k, v.clone());
        }
    }
    let paths = s.get("paths").filter(|p| matches!(p, Value::Arr(_))).cloned().unwrap_or(Value::Arr(Vec::new()));
    what.set("paths", paths);
    let title = match s.get("title").filter(|t| t.truthy()).or(s.get("ref")) {
        Some(t) => crate::json::text(t),
        None => "undefined".into(),
    };
    what.set("title", Value::Str(title));
    what.set("cwd", Value::Str(cwd.into()));
    (diff::show_args(&what).is_some() || diff::pull_of(&what).is_some()).then_some(what)
}

impl Preview {
    // --- the lines changed since HEAD

    /// Shown beside the File view's lines and on its scrollbar (Changes setting, c). Those of the file shown before
    /// stay until they come, so that a reload does not blink.
    pub(super) fn load_changes(&mut self) {
        let (run, file) = (self.changes_run.fetch_add(1, Ordering::SeqCst) + 1, self.state.file.clone());
        if self.state.changes.as_ref().is_some_and(|ch| ch.file != file) {
            self.state.changes = None;
        }
        if !self.on("changes") || !matches!(self.state.kind, Kind::Text | Kind::Markdown) {
            if self.state.changes.take().is_some() {
                self.render();
            }
            return;
        }
        let count = self.state.plain.len();
        let latest = self.changes_run.clone();
        self.git.send(Box::new(move || {
            if latest.load(Ordering::SeqCst) != run {
                return Box::new(|_: &mut Preview| {});
            }
            let res = std::panic::catch_unwind(|| diff::line_changes(&file, count)).ok().flatten();
            Box::new(move |p: &mut Preview| {
                if run != p.changes_run.load(Ordering::SeqCst) || p.state.file != file {
                    return;
                }
                let n = p.state.plain.len();
                // Where each block starts; removed lines at the end go to the last line.
                let lines: Vec<usize> = res.as_ref().map_or(Vec::new(), |r| {
                    let changed =
                        |i: usize| r.kinds.get(i).is_some_and(|k| *k != LineKind::Same) || r.deleted.contains(&i);
                    (0..=n)
                        .filter(|&i| changed(i) && !(i > 0 && changed(i - 1)))
                        .map(|i| i.min(n.saturating_sub(1)))
                        .collect()
                });
                p.state.changes = res.filter(|_| !lines.is_empty()).map(|diff| Changes { file, diff, lines });
                p.changes_seen += 1;
                p.render();
            })
        }));
    }

    /// The changes beside the lines: in the File view of a text file or of a Markdown file's source.
    pub(super) fn changes_shown(&self) -> bool {
        let s = &self.state;
        s.changes.as_ref().is_some_and(|ch| ch.file == s.file)
            && !s.diff
            && s.show.is_none()
            && s.edit.is_none()
            && !self.markdown_view()
            && matches!(s.kind, Kind::Text | Kind::Markdown)
    }

    /// The change of row i of the list: removed lines above a line, on its first row; at the end of the file, on its
    /// last row.
    fn change_of(&self, list: &[Row], i: usize) -> Option<Change> {
        let ch = self.state.changes.as_ref()?;
        let r = &list[i];
        let src = usize::try_from(r.src).ok()?;
        match ch.diff.kinds.get(src) {
            Some(LineKind::Add) => return Some(Change::Add),
            Some(LineKind::Mod) => return Some(Change::Mod),
            _ => {}
        }
        if r.first && ch.diff.deleted.contains(&src) {
            return Some(Change::Del);
        }
        let n = self.state.plain.len();
        let last = src + 1 == n && list.get(i + 1).is_none_or(|x| x.src != r.src);
        (last && ch.diff.deleted.contains(&n)).then_some(Change::Del)
    }

    /// The sign of row i in the column beside the lines.
    pub(super) fn change_sign(&self, list: &[Row], i: usize) -> Seg {
        let Some(k) = self.change_of(list, i) else { return seg(" ", "") };
        let r = &list[i];
        let first =
            r.first && self.state.changes.as_ref().is_some_and(|ch| ch.diff.deleted.contains(&(r.src as usize)));
        let glyph = if k != Change::Del {
            "▎"
        } else if first {
            "▔"
        } else {
            "▁"
        };
        seg(glyph, fg(k.color()))
    }

    /// The changes on the scrollbar, for the rows of the list: kept while the rows and the changes stay the same.
    pub(super) fn change_marks(&mut self, rows: &Rc<Rows>) -> Vec<BarMark> {
        if !self.changes_shown() {
            return Vec::new();
        }
        if let Some((of, seen, marks)) = &self.change_marks
            && Rc::ptr_eq(of, rows)
            && *seen == self.changes_seen
        {
            return marks.clone();
        }
        let marks: Vec<BarMark> = (0..rows.list.len())
            .filter_map(|i| {
                let k = self.change_of(&rows.list, i)?;
                Some(BarMark { row: i as i64, color: k.color(), weight: k.weight() })
            })
            .collect();
        self.change_marks = Some((rows.clone(), self.changes_seen, marks.clone()));
        marks
    }

    /// ] and [: the next or previous block of changes, a third of the way down, as n and N do with the marks.
    pub(super) fn next_change(&mut self, dir: i64) {
        if !self.changes_shown() {
            let text = if self.state.diff || self.markdown_view() {
                "Changes show in the file's source"
            } else {
                "No changes since HEAD"
            };
            return self.flash(Some(text.into()));
        }
        let third = self.third();
        let at = self.state.top + third;
        let lines = self.state.changes.as_ref().map(|ch| ch.lines.clone()).unwrap_or_default();
        let rows = self.rows();
        let row_of = |l: usize| rows.list.iter().position(|r| r.src >= l as i64).map_or(0, |i| i as i64);
        let k = if dir > 0 {
            lines.iter().position(|&l| row_of(l) > at)
        } else {
            lines.iter().rposition(|&l| row_of(l) < at)
        };
        let Some(k) = k else {
            return self.flash(Some(if dir > 0 { "Last change" } else { "First change" }.into()));
        };
        self.state.top = (row_of(lines[k]) - third).max(0);
        self.state.line = lines[k] as f64 + 1.0;
        self.flash(Some(format!("Change {}/{}", k + 1, lines.len())));
    }

    // --- Diff view

    /// The default branch the Diff view's merge base is with, for the scope's label.
    pub(super) fn branch_name(&self) -> String {
        match &self.state.diff_data {
            Some(DiffData::Done { base_name, .. }) if !base_name.is_empty() => base_name.clone(),
            _ => "main".into(),
        }
    }

    /// The file's changes (diff::collect), from near_dir. keep_scroll: a reload, where the view stays; otherwise it
    /// goes to the line asked for, or the first change. check_branch: once done, whatever came first, says when the
    /// merge base found no default branch (reload_scope); the diff of a call made meanwhile says it in its place.
    pub(super) fn load_diff(&mut self, keep_scroll: bool, check_branch: bool) {
        let (run, file) = (self.diff_run.fetch_add(1, Ordering::SeqCst) + 1, self.state.file.clone());
        self.check_branch |= check_branch;
        if !keep_scroll || self.state.diff_data.is_none() {
            self.state.diff_data = Some(DiffData::Loading);
        }
        let cwd = near_dir(&file);
        let scope = self.state.scope.clone();
        let turn = (scope.kind() == "turn" && !self.tab.is_empty()).then(|| scope::read_turn(&self.tab));
        let latest = self.diff_run.clone();
        self.git.send(Box::new(move || {
            if latest.load(Ordering::SeqCst) != run {
                return Box::new(|_: &mut Preview| {});
            }
            let res = std::panic::catch_unwind(|| diff::collect(&cwd, &file, "head", Some(&scope), turn.as_ref()))
                .unwrap_or_else(|_| Collected { top: cwd.clone(), ..Collected::default() });
            let parsed = Arc::new(diff::changes_of(&res));
            Box::new(move |p: &mut Preview| {
                if run != p.diff_run.load(Ordering::SeqCst) {
                    return;
                }
                // Another file's diff: its scope's default branch is not this one's.
                let check = std::mem::take(&mut p.check_branch) && p.state.file == file;
                let no_base = res.base_name.is_empty();
                if p.state.file == file {
                    if let Some(e) = &res.error {
                        p.flash(Some(e.clone()));
                    }
                    p.state.diff_data = Some(DiffData::Done { parsed, top: res.top, base_name: res.base_name });
                    // The hunk n went to is one of the diff replaced, unless a commit shows over it.
                    if p.state.show.is_none() {
                        p.state.hunk = None;
                    }
                    p.state.rows = None;
                    if !keep_scroll {
                        p.go_to_line();
                    } else if let Some(top) = p.state.want_top {
                        p.state.top = top;
                    }
                    p.state.want_top = None;
                    p.render();
                }
                if check && p.state.scope.kind() == "branch" && no_base {
                    p.flash(Some("No default branch: compared with HEAD".into()));
                }
            })
        }));
    }

    /// The Diff view at the line asked for (or the first mark), else at the first change, a third of the way down.
    fn go_to_line(&mut self) {
        let want = self.state.marks.first().map_or(self.state.line, |m| m.from as f64);
        let rows = self.rows();
        let i = if want != 0.0 {
            diff_row(&rows, want)
        } else {
            rows.list.iter().position(|r| r.diff.as_ref().is_some_and(|d| d.hunk)).map_or(-1, |i| i as i64)
        };
        self.state.top = (i - if want != 0.0 { self.third() } else { 0 }).max(0);
    }

    /// The row of a mark's line in the Diff view (next_mark).
    pub(super) fn diff_row(&mut self, line: f64) -> i64 {
        let rows = self.rows();
        diff_row(&rows, line)
    }

    /// The diff's rows: one column for the marks' bar on the left. Long lines wrap with the file's setting (w), else
    /// the text scrolls from state.left (← →): rendered from column 0, a row cut from another column is made again
    /// (segs_of). A commit shown takes the same rows, without the file's name when it has a single file.
    pub(super) fn diff_rows(&self, w: usize) -> Vec<Row> {
        let s = &self.state;
        let back = self.q_label();
        if let Some(show) = &s.show {
            if show.blank() {
                let text = show.what.get("text").and_then(Value::as_str).unwrap_or("");
                return vec![Row::empty(text, &[("q", "close")])];
            }
            if show.failed {
                return vec![Row::empty("Cannot show it", &[("q", back)])];
            }
        }
        let parsed = match (&s.show, &s.diff_data) {
            (Some(show), _) => show.parsed.clone().filter(|_| !show.loading),
            (None, Some(DiffData::Done { parsed, .. })) => Some(parsed.clone()),
            _ => None,
        };
        let Some(parsed) = parsed else {
            let pairs = if s.show.is_some() { [("q", back)] } else { [("d", "file")] };
            return vec![Row::empty("Loading…", &pairs)];
        };
        let width = (w as i64 - 2).max(10) as usize;
        let view = diff::View { wrap: self.on("wrap"), ..diff::View::default() };
        let list: Vec<Row> = diff::render(&parsed, width, diff::layout_of(width), s.show.is_none(), &view)
            .into_iter()
            .map(|r| {
                let mut segs = vec![seg(if self.diff_marked(&r) { "▌" } else { " " }, fg(c().blue))];
                segs.extend(r.segs.iter().cloned());
                Row { segs, src: -1, back: r.back, diff: Some(r), ..Row::default() }
            })
            .collect();
        if list.is_empty() && s.show.is_some() {
            return vec![Row::empty("No changes", &[("q", back)])];
        }
        if list.is_empty() {
            return vec![Row::empty("No changes", &[("d", "file"), ("b", "compare with"), ("q", "close")])];
        }
        list
    }

    /// A row as shown: in the Diff view, made again from state.left.
    pub(super) fn segs_of(&self, r: &Row) -> Vec<Seg> {
        match r.diff.as_ref().and_then(|d| d.redo.as_ref()) {
            Some(redo) if self.state.left > 0 => {
                let mut segs = vec![r.segs[0].clone()];
                segs.extend(redo(self.state.left as usize));
                segs
            }
            _ => r.segs.clone(),
        }
    }

    /// A row of the Diff view on a marked line (whole lines: the diff has no columns of the file), or the @@ row of
    /// the hunk n went to.
    fn diff_marked(&self, r: &diff::Row) -> bool {
        let s = &self.state;
        (r.h.is_some() && r.h == s.hunk)
            || (s.show.is_none()
                && !r.hunk
                && r.r#ref.as_ref().is_some_and(|x| {
                    x.from != 0 && x.from == x.to && s.marks.iter().any(|m| x.from >= m.from && x.from <= m.to)
                }))
    }

    /// n and N in the Diff view without marks: the next or previous hunk, its header at the top, marked (hunk) for
    /// s, u and D, even when the diff is too short to scroll.
    pub(super) fn next_hunk(&mut self, dir: i64) {
        let rows = self.rows();
        let list = &rows.list;
        let h = |r: &Row| r.diff.as_ref().and_then(|d| d.h);
        let hunk = |r: &Row| r.diff.as_ref().is_some_and(|d| d.hunk);
        let was = self.state.hunk.and_then(|x| list.iter().position(|r| h(r) == Some(x)));
        let at = was.map_or(self.state.top, |i| i as i64);
        let i = if dir > 0 {
            list.iter().enumerate().position(|(k, r)| k as i64 > at && hunk(r))
        } else {
            list.iter().enumerate().rposition(|(k, r)| (k as i64) < at && hunk(r))
        };
        if let Some(i) = i {
            self.state.top = i as i64;
            self.state.hunk = h(&list[i]);
            self.state.rows = None;
            self.render();
        }
    }

    /// b: the menu of the scopes (scope::items); a scope chosen holds for every pane of the tab (scope-<tab>.json).
    pub(super) fn open_scopes(&mut self) {
        let w = self.size().0 as i64;
        self.state.menu = Some(scope_menu(scope::menu(&self.state.scope, &self.branch_name(), w)));
        self.render();
    }

    /// An item of the scope menus: a scope, or Commit or range… and Branch…, which ask for theirs first.
    pub(super) fn scope_item(&mut self, pick: Pick) {
        match pick {
            Pick::Scope(s) => self.pick_scope(s),
            Pick::Ask(kind) => {
                self.state.menu = None;
                self.render();
                if kind == "rev" {
                    self.state.rev_prompt = Some(Field::new(scope::prompt_rev(&self.state.scope)));
                    return self.render();
                }
                let (cwd, now) = (near_dir(&self.state.file), self.state.scope.clone());
                self.git.send(Box::new(move || {
                    let list = scope::branches(&cwd);
                    Box::new(move |p: &mut Preview| {
                        let w = p.size().0 as i64;
                        match scope::branch_menu(&now, &list, w) {
                            None => p.flash(Some("No branch".into())),
                            Some(m) => {
                                p.state.menu = Some(scope_menu(m));
                                p.render();
                            }
                        }
                    })
                }));
            }
        }
    }

    /// A key in the field of Commit or range….
    pub(super) fn rev_key(&mut self, k: &str) {
        let Some(field) = self.state.rev_prompt.as_mut() else { return };
        match scope::rev_key(field, k) {
            RevKey::Changed(changed) => {
                if changed {
                    self.render();
                }
            }
            RevKey::Done(rev) => {
                self.state.rev_prompt = None;
                if rev.is_empty() {
                    return self.render();
                }
                self.pick_scope(Scope::new("rev", Some(&rev)));
            }
        }
    }

    /// The one already chosen loads its changes again (a new agent turn, a branch moved), as in the Changes pane.
    fn pick_scope(&mut self, s: Scope) {
        self.state.menu = None;
        self.state.rev_prompt = None;
        if !scope::same(&s, &self.state.scope) {
            if !self.tab.is_empty() {
                scope::write(&self.tab, &s);
            }
            self.state.scope = s;
        }
        self.reload_scope();
    }

    fn reload_scope(&mut self) {
        if !self.state.diff {
            return self.render();
        }
        self.load_diff(false, true);
    }

    /// The tab's scope chosen in another pane (the Changes pane, source control's compare).
    pub(super) fn follow_scope(&mut self) {
        let s = scope::read(&self.tab);
        if !scope::same(&s, &self.state.scope) {
            self.state.scope = s;
            self.reload_scope();
        }
    }

    /// s, u, D in the Diff view: the rows selected with the mouse, else the hunk n went to, else the hunk at the top
    /// (the first hunk of the file whose name is there), staged, unstaged or discarded (core::stage), compared with
    /// HEAD only; D asks first.
    pub(super) fn edit_diff(&mut self, mode: Mode) {
        let Some(DiffData::Done { parsed, top, .. }) = &self.state.diff_data else { return };
        let (parsed, top) = (parsed.clone(), top.clone());
        if self.editing {
            return self.flash(Some("Busy…".into()));
        }
        if self.state.scope.kind() != "head" {
            return self.flash(Some("Not with this scope: stage compared with HEAD (b)".into()));
        }
        let rows = self.rows();
        let list = &rows.list;
        let body_h = self.size().1 as i64 - 2;
        let sel = self.state.select.filter(|s| s.moved);
        let h = |r: &Row| r.diff.as_ref().and_then(|d| d.h);
        let named = |r: &Row| r.diff.as_ref().is_some_and(|d| d.h.is_some() || d.f.is_some());
        // The hunk n went to while some of it is in view: scrolled away from, the top's.
        let mut chosen =
            self.state.hunk.and_then(|x| list.iter().position(|r| h(r) == Some(x))).map_or(-1, |i| i as i64);
        let after = if chosen < 0 {
            -1
        } else {
            list.iter().enumerate().position(|(i, r)| i as i64 > chosen && named(r)).map_or(-1, |i| i as i64)
        };
        let top_row = self.state.top;
        if chosen >= top_row + body_h || (after >= 0 && after <= top_row) {
            chosen = -1;
        }
        let mut a = if chosen >= 0 { chosen } else { top_row };
        let mut b = a;
        if let Some(sel) = sel {
            let (x, y) = super::input::ordered(&sel);
            (a, b) = (x.row as i64, y.row as i64);
        } else if chosen < 0 {
            while a > 0 && list.get(a as usize).is_some_and(|r| !named(r)) {
                a -= 1;
            }
            // On a file's name, its first hunk: the whole file only when it has none.
            let file_row = list.get(a as usize).is_some_and(|r| r.diff.as_ref().is_some_and(|d| d.f.is_some()));
            let next =
                if file_row { list.iter().enumerate().position(|(i, r)| i as i64 > a && named(r)) } else { None };
            if let Some(n) = next
                && h(&list[n]).is_some()
            {
                a = n as i64;
            }
            b = a;
        }
        let (a, b) = (a.max(0) as usize, (b.max(0) as usize).min(list.len().saturating_sub(1)));
        let picked = stage::picked(list.get(a..=b).unwrap_or(&[]).iter().filter_map(|r| r.diff.as_ref()));
        if mode != Mode::Discard {
            return self.stage(top, parsed, picked, mode);
        }
        let n = stage::count(&parsed, &picked, mode);
        if n.is_empty() {
            return self.flash(Some("Nothing unstaged selected".into()));
        }
        let ask = Ask {
            title: "Discard from the working tree?".into(),
            name: n,
            where_: Some(paths::basename(&self.state.file, None)),
            buttons: vec![
                Button { key: "d".into(), label: "Discard".into(), tone: "danger".into() },
                Button { key: "escape".into(), label: "Cancel".into(), tone: String::new() },
            ],
            ..Ask::default()
        };
        let go: super::edit::Run = Box::new(move |p: &mut Preview| p.stage(top, parsed, picked, mode));
        self.ask(super::edit::Question { ask, on: vec![("d".into(), go)], dropped: None });
    }

    /// Does it on a thread of its own: untracked files the trash did not take are deleted for good once asked again.
    fn stage(&mut self, top: String, parsed: Arc<Diff>, picked: stage::Sel, mode: Mode) {
        if self.editing {
            return self.flash(Some("Busy…".into()));
        }
        self.editing = true;
        self.state.select = None;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        let started = worker::spawn(
            self.tx.clone(),
            self.waker.clone(),
            Box::new(move || {
                let no_trash = |names: &[String]| {
                    let (answer, wait) = channel();
                    let name = if names.len() > 1 { format!("{} files", names.len()) } else { names[0].clone() };
                    let ask: super::Then = Box::new(move |p: &mut Preview| {
                        let ask = Ask {
                            title: "No trash: delete for good?".into(),
                            name,
                            note: Some("This cannot be undone.".into()),
                            buttons: vec![
                                Button { key: "d".into(), label: "Delete".into(), tone: "danger".into() },
                                Button { key: "escape".into(), label: "Cancel".into(), tone: String::new() },
                            ],
                            ..Ask::default()
                        };
                        let no = answer.clone();
                        let yes: super::edit::Run = Box::new(move |_: &mut Preview| {
                            let _ = answer.send(true);
                        });
                        let dropped: super::edit::Run = Box::new(move |_: &mut Preview| {
                            let _ = no.send(false);
                        });
                        p.ask(super::edit::Question { ask, on: vec![("d".into(), yes)], dropped: Some(dropped) });
                    });
                    if tx.send(ask).is_err() {
                        return false;
                    }
                    waker.wake();
                    wait.recv().unwrap_or(false)
                };
                let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    stage::apply(&top, &parsed, &picked, mode, &actions::trash, Some(&no_trash))
                }))
                .unwrap_or_else(|_| format!("Cannot {}", mode.name()));
                Box::new(move |p: &mut Preview| {
                    p.flash(Some(text));
                    p.editing = false;
                    // The rows move once reloaded (the staged section first): the next s aims at the top again, not
                    // at a hunk n went to that is no longer where it was.
                    p.state.hunk = None;
                    p.load_diff(true, false);
                })
            }),
        );
        if !started {
            self.editing = false;
            self.flash(Some(format!("Cannot {}", mode.name())));
        }
    }

    /// v: inline or side by side, saved for the width.
    pub(super) fn switch_layout(&mut self) {
        let width = (self.size().0 as i64 - 2).max(10) as usize;
        diff::save_mode(width, if diff::layout_of(width) == "split" { "inline" } else { "split" });
        self.state.rows = None;
        self.render();
    }

    /// The Diff view's options in the View menu: layout and wrap, then what it compares with.
    pub(super) fn diff_items(&self) -> Vec<Option<MenuItem<Opt>>> {
        let width = (self.size().0 as i64 - 2).max(10) as usize;
        let mut items: Vec<_> = diff::view_items(diff::layout_of(width), self.on("wrap"), Opt::Layout, Opt::Wrap)
            .into_iter()
            .map(Some)
            .collect();
        if self.state.show.is_none() {
            items.push(Some(MenuItem {
                label: "Compare with".into(),
                key: "b".into(),
                value: Some(seg(scope::label(&self.state.scope, &self.branch_name()), fg(c().blue))),
                danger: false,
                keep: false,
                run: Opt::Compare,
            }));
        }
        items
    }

    /// In the Diff view: the new version's lines of the rows selected, or of the top row, and their section (staged
    /// or not; None when both are selected).
    pub(super) fn diff_lines(&mut self) -> Option<(i64, i64, Option<String>)> {
        let rows = self.rows();
        let (a, b) = match self.state.select.filter(|s| s.moved) {
            Some(sel) => {
                let (x, y) = super::input::ordered(&sel);
                (x.row, y.row)
            }
            None => (self.state.top.max(0) as usize, self.state.top.max(0) as usize),
        };
        let refs: Vec<&diff::Ref> = rows
            .list
            .get(a..=b.min(rows.list.len().saturating_sub(1)))
            .unwrap_or(&[])
            .iter()
            .filter_map(|r| r.diff.as_ref().and_then(|d| d.r#ref.as_ref()))
            .filter(|r| r.from != 0)
            .collect();
        let first = refs.first()?;
        let sec = refs.iter().all(|r| r.sec == first.sec).then(|| first.sec.clone());
        let from = refs.iter().map(|r| r.from).min()?;
        let to = refs.iter().map(|r| r.to).max()?;
        Some((from, to, sec))
    }

    // --- a commit, stash or tag (source control's review)

    /// Source control asks for one through the request file (show), as for a file: it shows over the tabs, read
    /// only, until a file is asked for or q.
    pub(super) fn enter_show(&mut self, asked: Option<&Value>) {
        let Some(what) = clean_show(asked) else { return };
        // Asked again (source control follows its selection at each refresh): left as it is, scroll included. A
        // stash's name moves to another stash once one is added: loaded again, without clearing the screen meanwhile.
        let same = self.state.show.as_ref().is_some_and(|d| json::stringify(&d.what) == json::stringify(&what));
        let stash = what.get("kind").and_then(Value::as_str) == Some("stash");
        if same && !stash {
            return;
        }
        self.show_run += 1;
        let run = self.show_run;
        if same {
            let id = self.state.show.as_ref().map_or(0, |d| d.id);
            return self.git_show(what, move |p, parsed| {
                if run != p.show_run || p.state.show.as_ref().is_none_or(|d| d.id != id) {
                    return;
                }
                let Some(parsed) = parsed else { return };
                if let Some(d) = &mut p.state.show {
                    d.parsed = Some(parsed);
                }
                p.state.hunk = None;
                p.state.rows = None;
                p.render();
            });
        }
        self.drop_ask();
        let s = &mut self.state;
        let back = s.show.as_ref().map_or((s.top, s.left), |d| d.back);
        self.shows += 1;
        let id = self.shows;
        let blank = what.get("blank") == Some(&Value::Bool(true));
        let s = &mut self.state;
        s.show = Some(Show { what: what.clone(), parsed: None, loading: true, failed: false, back, id });
        s.top = 0;
        s.left = 0;
        s.rows = None;
        s.hunk = None;
        s.select = None;
        s.find = None;
        s.matches.clear();
        s.menu = None;
        s.help = None;
        self.render();
        if blank {
            return;
        }
        self.git_show(what, move |p, parsed| {
            if run != p.show_run || p.state.show.as_ref().is_none_or(|d| d.id != id) {
                return;
            }
            if let Some(d) = &mut p.state.show {
                d.failed = parsed.is_none();
                d.parsed = parsed;
                d.loading = false;
            }
            p.state.rows = None;
            p.render();
        });
    }

    /// diff::show on the git worker, then on the pane; a pull request's on a thread of its own, so that gh and the
    /// network never hold up git's answers.
    fn git_show(&mut self, what: Value, then: impl FnOnce(&mut Preview, Option<Arc<Diff>>) + Send + 'static) {
        let pull = diff::pull_of(&what).is_some();
        let job: super::Job = Box::new(move || {
            let cwd = what.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
            let parsed = std::panic::catch_unwind(|| diff::show(&cwd, &what)).ok().flatten().map(Arc::new);
            Box::new(move |p: &mut Preview| then(p, parsed))
        });
        if pull {
            worker::spawn(self.tx.clone(), self.waker.clone(), job);
        } else {
            self.git.send(job);
        }
    }

    /// The tab under it shown again, where it was.
    pub(super) fn drop_show(&mut self) {
        let Some(d) = self.state.show.take() else { return };
        let s = &mut self.state;
        s.rows = None;
        (s.top, s.left) = d.back;
        s.hunk = None;
        s.select = None;
    }

    /// q on what is shown: back to the tab under it, or the preview closed.
    pub(super) fn q_label(&self) -> &'static str {
        if self.review || self.state.file.is_empty() { "close" } else { "back" }
    }

    pub(super) fn leave_show(&mut self) {
        if self.state.file.is_empty() || self.review {
            return self.quit();
        }
        self.drop_show();
        self.render();
    }

    /// Whether a key works while a commit shows.
    pub(super) fn show_key(k: &str) -> bool {
        SHOW_KEYS.contains(&k)
    }

    /// Its title and line counts, then ⚙ on the right, which opens the diff's options (,).
    pub(super) fn show_header(&mut self, w: usize) -> String {
        const GEAR: i64 = 3;
        let wi = w as i64;
        let Some(show) = &self.state.show else { return String::new() };
        let info = show.parsed.as_ref().map_or(String::new(), |d| format!("  +{} −{}", d.add, d.del));
        let title =
            format!(" {}", ui::clean_line(show.what.get("title").and_then(Value::as_str).unwrap_or(""), 4, None));
        let (title, _) = ui::fit(&title, (wi - text_width(&info) as i64 - GEAR - 1).max(1) as usize, None);
        let title = title.to_string();
        let used = (text_width(&title) + text_width(&info)) as i64;
        let x = (used + 1).max(wi - GEAR + 1);
        self.state.chips = vec![super::Chip { id: "menu".into(), from: x, to: x + GEAR - 1, tab: None }];
        let gear = fg(if self.state.menu.is_some() { c().text } else { c().dim });
        let segs = vec![
            seg(title, "\x1b[1m"),
            seg(info, fg(c().dim)),
            seg(" ".repeat((x - 1 - used).max(0) as usize), ""),
            seg(format!(" {} ", ui::g().gear), gear),
        ];
        ui::line(&segs, w, Some(c().header))
    }

    /// The footer of a commit shown.
    pub(super) fn show_pairs(&self) -> Vec<(String, String)> {
        let pair = |k: &str, a: &str| (k.to_string(), a.to_string());
        match &self.state.show {
            Some(d) if d.blank() => vec![pair("z", "zoom"), pair("q", "close"), pair("?", "keys")],
            _ => vec![
                pair("n N", "hunks"),
                pair("v", "layout"),
                pair("z", "zoom"),
                pair("q", self.q_label()),
                pair("?", "keys"),
            ],
        }
    }

    /// The field of Commit or range…, in the footer.
    pub(super) fn rev_footer(&self, w: usize) -> Option<String> {
        let f = self.state.rev_prompt.as_ref()?;
        let lead = " Commit or range (abc123, main..HEAD): ";
        let mut segs = vec![seg(lead, fg(c().blue))];
        segs.extend(f.segments((w as i64 - text_width(lead) as i64).max(1), ""));
        Some(ui::line(&segs, w, Some(c().header)))
    }
}
