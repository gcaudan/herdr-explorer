//! The preview's edit mode (src/preview.js): i, or Edit here in the View menu, edits a text file in the pane: read
//! again whole (buffer::read: UTF-8 only, up to buffer::MAX_BYTES), shown as its source without wrapping,
//! highlighted, with the terminal's cursor. The edit lives in its tab (TabView), so that the other tabs keep working.
//! state.top and state.left are then a line and a column of the text. ctrl+s writes it back (save_edit), esc leaves
//! (stop_edit); both, and closing the tab or the pane, ask first about unsaved changes (unsaved) or a newer file.
//! Also the questions in a card (ask), which the edits are the first to need.

use super::{Kind, Preview};
use crate::core::actions;
use crate::core::buffer::{self, Editor, LineKind, Match, Pos};
use crate::core::exec;
use crate::core::screen;
use crate::core::ui::{self, Ask, Button, Hints, Mouse, Rgb, Seg, bg, c, fg, g, seg, text_width};
use crate::core::{js, paths};
use crate::sys;
use std::cell::RefCell;
use std::fs::Metadata;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

/// Lines highlighted before and after the ones shown, so that a comment or a string opened above is still coloured.
const LOOK: usize = 200;

/// Something to do later on the pane: a question's answer, what follows a save.
pub type Run = Box<dyn FnOnce(&mut Preview)>;

/// A question in a card (ui::ask): on, what each button's key does; Cancel (escape) leaves things as they were.
/// dropped: called when it goes without an answer (canceled, or another question or a request taking its place).
pub struct Question {
    pub ask: Ask,
    pub on: Vec<(String, Run)>,
    pub dropped: Option<Run>,
}

/// The file on disk, while the bar about it shows: deleted, or newer than the text.
pub enum Disk {
    Gone,
    Newer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Find,
    Replace,
    Line,
}

/// The footer's field: with, the replacement's field; on, in it.
pub struct Prompt {
    kind: PromptKind,
    field: ui::Field,
    with: Option<ui::Field>,
    on: bool,
}

/// The highlighted lines a..a+lines.len(), kept for the lines lo..hi until the text changes.
struct Hl {
    version: u64,
    lo: usize,
    hi: usize,
    a: usize,
    lines: Vec<Vec<Seg>>,
}

/// A row above the footer: the bar of a file changed on disk, or the replacement's field, with its hints and where
/// they start, and its row once drawn.
struct Bar {
    segs: Vec<Seg>,
    hints: Hints,
    from: i64,
    back: Rgb,
    y: i64,
    with: bool,
}

/// An edit:
///   ed      the text, its cursor and history (buffer::Editor)
///   base    the file's stat when read or saved: a file on disk that differs is newer than the text
///   disk    while the bar about a deleted or newer file shows
///   prompt  the footer's field; query, the last one searched
///   hl, found  the highlighted lines and the find's matches, kept until the text changes
pub struct Edit {
    /// Which edit this is, for the clipboard read on the worker: the same edit still shown when it is back.
    id: u64,
    pub ed: Editor,
    base: Metadata,
    disk: Option<Disk>,
    prompt: Option<Prompt>,
    query: String,
    hl: Option<Hl>,
    found: Option<(String, u64, Vec<Match>)>,
    /// The cursor to show at the next drawing (a request's line).
    pub reveal: bool,
    bars: Vec<Bar>,
}

impl Edit {
    pub fn forget_colours(&mut self) {
        self.hl = None;
    }
}

/// An edit shared by the state and its tab, as the JS's object: equal when the same.
#[derive(Clone)]
pub struct Shared(pub Rc<RefCell<Edit>>);

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Edit")
    }
}

impl Shared {
    pub fn dirty(&self) -> bool {
        self.0.borrow().ed.dirty()
    }
}

fn same_stat(a: Option<&Metadata>, b: &Metadata) -> bool {
    let ms = |m: &Metadata| m.modified().ok().map(js::time_ms);
    a.is_some_and(|a| ms(a) == ms(b) && a.len() == b.len())
}

fn eol_name(ed: &Editor) -> &'static str {
    if ed.eol == "\r\n" { "CRLF" } else { "LF" }
}

/// The terminal's cursor shown (a bar), to put back as it was at exit.
pub static CURSOR_SHOWN: AtomicBool = AtomicBool::new(false);

/// Moves by key, with shift extending the selection.
fn move_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "left" => "left",
        "right" => "right",
        "up" => "up",
        "down" => "down",
        "home" => "home",
        "end" => "end",
        "pageup" => "pageup",
        "pagedown" => "pagedown",
        "ctrl+left" => "wordleft",
        "ctrl+right" => "wordright",
        "ctrl+home" => "top",
        "ctrl+end" => "bottom",
        _ => return None,
    })
}

impl Preview {
    /// The edit of tab i (the tab shown has its own in state).
    pub(super) fn edit_at(&self, i: usize) -> Option<Shared> {
        if i == self.tabs.current {
            return self.state.edit.clone();
        }
        self.tabs.list.get(i).and_then(|t| t.extra.as_ref()).and_then(|v| v.edit.clone())
    }

    /// Every tab's edit, with its file.
    fn edits(&self) -> Vec<(Shared, String)> {
        (0..self.tabs.list.len()).filter_map(|i| self.edit_at(i).map(|e| (e, self.tabs.list[i].file.clone()))).collect()
    }

    pub(super) fn unsaved_at(&self, i: usize) -> bool {
        self.edit_at(i).is_some_and(|e| e.dirty())
    }

    /// The header's details while editing.
    pub(super) fn edit_info(&self) -> Option<String> {
        let e = self.state.edit.as_ref()?.0.borrow();
        let n = e.ed.lines.len();
        let state = if e.ed.dirty() { "modified" } else { "editing" };
        Some(format!("{state} · {n} line{} · {}", if n == 1 { "" } else { "s" }, eol_name(&e.ed)))
    }

    pub(super) fn start_edit(&mut self) {
        if self.state.edit.is_some() {
            return;
        }
        if self.state.diff || !matches!(self.state.kind, Kind::Text | Kind::Markdown) {
            return self.flash(Some("Cannot edit this file here (e: in the terminal)".into()));
        }
        let (doc, stat) = match buffer::read(&self.state.file) {
            Ok(r) => r,
            Err(e) => return self.flash(Some(format!("Cannot edit: {e}"))),
        };
        // The cursor on the line highlighted, else on the first line shown.
        let rows = self.rows();
        let first = rows.list.iter().skip(self.state.top.max(0) as usize).find(|r| r.src >= 0).map_or(0, |r| r.src);
        let mut ed = Editor::new(&doc);
        let line = if self.state.line != 0.0 { self.state.line } else { (first + 1) as f64 };
        ed.goto(line as usize);
        let mixed = doc.mixed;
        let eol = eol_name(&ed);
        static IDS: AtomicU64 = AtomicU64::new(0);
        let edit = Edit {
            id: IDS.fetch_add(1, Ordering::Relaxed),
            ed,
            base: stat,
            disk: None,
            prompt: None,
            query: String::new(),
            hl: None,
            found: None,
            reveal: false,
            bars: Vec::new(),
        };
        let s = &mut self.state;
        s.edit = Some(Shared(Rc::new(RefCell::new(edit))));
        s.top = first;
        s.left = 0;
        s.select = None;
        s.find = None;
        s.matches.clear();
        s.menu = None;
        self.keep_tab(self.tabs.current);
        self.reveal_cursor();
        if mixed {
            self.flash(Some(format!("Mixed line endings: saved as {eol}")));
        } else {
            self.render();
        }
    }

    /// esc: back to the preview at the same place, once unsaved changes are saved or dropped.
    pub(super) fn stop_edit(&mut self) {
        let (edit, file) = (self.state.edit.clone(), self.state.file.clone());
        self.unsaved(
            edit,
            file,
            Box::new(|p: &mut Preview| {
                let line = p.state.top;
                p.state.edit = None;
                p.load(true);
                p.state.top = p.row_of(line as f64);
                p.render();
            }),
        );
    }

    // --- questions

    pub(super) fn ask(&mut self, mut q: Question) {
        self.drop_ask();
        // { focus: 0, ...q }: the focus given, else the first button.
        q.ask.focus = q.ask.focus.min(q.ask.buttons.len().saturating_sub(1));
        self.state.ask = Some(q);
        self.render();
    }

    pub(super) fn drop_ask(&mut self) {
        if let Some(dropped) = self.state.ask.take().and_then(|q| q.dropped) {
            dropped(self);
        }
    }

    pub(super) fn ask_choice(&mut self, k: Option<String>) {
        let Some(k) = k else { return };
        if k == "move" {
            return self.render();
        }
        let Some(mut q) = self.state.ask.take() else { return };
        if let Some(i) = q.on.iter().position(|(key, _)| *key == k) {
            let (_, run) = q.on.swap_remove(i);
            return run(self);
        }
        if let Some(dropped) = q.dropped {
            dropped(self);
        }
        self.render();
    }

    pub(super) fn ask_key(&mut self, k: &str) {
        let Some(q) = self.state.ask.as_mut() else { return };
        let choice = ui::ask_key(k, &mut q.ask);
        self.ask_choice(choice);
    }

    /// The folder of a file for a card: relative to the root, nothing for the root itself.
    fn folder_of(&self, file: &str) -> Option<String> {
        let root = &self.state.root;
        let dir = paths::dirname(file);
        let rel = (!root.is_empty()).then(|| paths::relative(root, &dir));
        match rel {
            Some(r) if r.is_empty() => None,
            Some(r) if !(r.starts_with("..") || paths::is_absolute(&r)) => Some(format!("in {r}")),
            _ => Some(format!("in {dir}")),
        }
    }

    /// The unsaved changes of an edit (the tab shown's, or another's) saved or dropped, then then runs.
    pub(super) fn unsaved(&mut self, edit: Option<Shared>, file: String, then: Run) {
        let Some(edit) = edit.filter(Shared::dirty) else { return then(self) };
        let ask = Ask {
            title: "Save the changes?".into(),
            name: paths::basename(&file, None),
            where_: self.folder_of(&file),
            buttons: vec![
                Button { key: "s".into(), label: "Save".into(), tone: "go".into() },
                Button { key: "d".into(), label: "Don't save".into(), tone: "danger".into() },
                Button { key: "escape".into(), label: "Cancel".into(), tone: String::new() },
            ],
            ..Ask::default()
        };
        // then goes to the answer given, s or d.
        let then = Rc::new(RefCell::new(Some(then)));
        let other = then.clone();
        let on: Vec<(String, Run)> = vec![
            (
                "s".into(),
                Box::new(move |p: &mut Preview| {
                    let then = then.borrow_mut().take();
                    p.save_edit(edit, file, then, false)
                }),
            ),
            (
                "d".into(),
                Box::new(move |p: &mut Preview| {
                    if let Some(f) = other.borrow_mut().take() {
                        f(p)
                    }
                }),
            ),
        ];
        self.ask(Question { ask, on, dropped: None });
    }

    /// ctrl+s: the text written over the file (buffer::save) as it was read: its line endings, final newline and byte
    /// order mark. A file changed on disk since it was read (by an agent, another editor) is replaced only once
    /// confirmed. then: what to do once saved (a tab or the pane closing).
    pub(super) fn save_edit(&mut self, edit: Shared, file: String, then: Option<Run>, force: bool) {
        let (dirty, gone) = {
            let e = edit.0.borrow();
            (e.ed.dirty(), matches!(e.disk, Some(Disk::Gone)))
        };
        if !dirty && !gone {
            return match then {
                Some(f) => f(self),
                None => self.flash(Some("No changes to save".into())),
            };
        }
        let now = std::fs::metadata(&file).ok();
        if !force && now.is_some() && !same_stat(now.as_ref(), &edit.0.borrow().base) {
            let ask = Ask {
                title: "The file changed on disk".into(),
                name: paths::basename(&file, None),
                note: Some("Saving replaces the newer file with your text".into()),
                buttons: vec![
                    Button { key: "o".into(), label: "Overwrite".into(), tone: "danger".into() },
                    Button { key: "escape".into(), label: "Cancel".into(), tone: String::new() },
                ],
                focus: 1,
                ..Ask::default()
            };
            let on: Vec<(String, Run)> =
                vec![("o".into(), Box::new(move |p: &mut Preview| p.save_edit(edit, file, then, true)))];
            return self.ask(Question { ask, on, dropped: None });
        }
        let text = edit.0.borrow().ed.serialize();
        match buffer::save(&file, &text) {
            Ok(stat) => edit.0.borrow_mut().base = stat,
            Err(e) => return self.flash(Some(format!("Cannot save: {}", exec::io_code(&e)))),
        }
        let done = {
            let mut e = edit.0.borrow_mut();
            e.ed.mark_saved();
            e.disk = None;
            let last = if e.ed.final_ { "" } else { ", no final newline" };
            format!("Saved {} · {}{last}", paths::basename(&file, None), eol_name(&e.ed))
        };
        self.flash(Some(done));
        if let Some(f) = then {
            f(self);
        }
    }

    fn save_all(&mut self, mut list: Vec<(Shared, String)>, then: Run) {
        if list.is_empty() {
            return then(self);
        }
        let (edit, file) = list.remove(0);
        self.save_edit(edit, file, Some(Box::new(move |p: &mut Preview| p.save_all(list, then))), false);
    }

    /// q, or the last tab closed: the preview closes, once the unsaved edits of its tabs are saved or dropped.
    pub(super) fn quit(&mut self) {
        let mut list: Vec<(Shared, String)> = self.edits().into_iter().filter(|(e, _)| e.dirty()).collect();
        if list.is_empty() {
            return self.exit();
        }
        if list.len() == 1 {
            let (edit, file) = list.remove(0);
            return self.unsaved(Some(edit), file, Box::new(|p: &mut Preview| p.exit()));
        }
        let names: Vec<String> = list.iter().map(|(_, f)| paths::basename(f, None)).collect();
        let ask = Ask {
            title: format!("Unsaved changes in {} files", list.len()),
            name: names.join(", "),
            buttons: vec![
                Button { key: "s".into(), label: "Save all".into(), tone: "go".into() },
                Button { key: "d".into(), label: "Close without saving".into(), tone: "danger".into() },
                Button { key: "escape".into(), label: "Cancel".into(), tone: String::new() },
            ],
            ..Ask::default()
        };
        let on: Vec<(String, Run)> = vec![
            ("s".into(), Box::new(move |p: &mut Preview| p.save_all(list, Box::new(|p: &mut Preview| p.exit())))),
            ("d".into(), Box::new(|p: &mut Preview| p.exit())),
        ];
        self.ask(Question { ask, on, dropped: None });
    }

    /// The process ends, its tabs saved for the next one (the JS's process.exit and its exit handler).
    fn exit(&mut self) {
        self.save_file();
        self.screen.exit(0)
    }

    /// The file changed on disk while edited (load: the watcher, or the tab shown again). Nothing when it is the text
    /// read or saved last; read again when nothing was typed; else the bar, which offers to reload it (alt+r) or to
    /// keep the text (alt+k, the save asking before replacing the newer file).
    pub(super) fn disk_changed(&mut self) {
        let Some(shared) = self.state.edit.clone() else { return };
        let stat = std::fs::metadata(&self.state.file).ok();
        {
            let mut e = shared.0.borrow_mut();
            let Some(stat) = stat else {
                e.disk = Some(Disk::Gone);
                return;
            };
            if same_stat(Some(&stat), &e.base) {
                if matches!(e.disk, Some(Disk::Gone)) {
                    e.disk = None;
                }
                return;
            }
            if e.ed.dirty() {
                e.disk = Some(Disk::Newer);
                return;
            }
        }
        self.reload_edit();
    }

    fn reload_edit(&mut self) {
        let Some(shared) = self.state.edit.clone() else { return };
        match buffer::read(&self.state.file) {
            Err(err) => {
                self.state.edit = None;
                self.state.rows = None;
                self.flash(Some(format!("Cannot edit: {err}")));
            }
            Ok((doc, stat)) => {
                let mut e = shared.0.borrow_mut();
                e.ed.reset(&doc);
                e.base = stat;
                e.disk = None;
                e.hl = None;
                e.found = None;
            }
        }
    }

    fn disk_key(&mut self, k: &str) {
        let Some(shared) = self.state.edit.clone() else { return };
        let gone = match &shared.0.borrow().disk {
            None => return,
            Some(d) => matches!(d, Disk::Gone),
        };
        if k == "alt+r" && !gone {
            self.reload_edit();
            if self.state.edit.is_some() {
                self.flash(Some("Reloaded from disk".into()));
            }
            return;
        }
        if k == "alt+k" {
            shared.0.borrow_mut().disk = None;
            self.render();
        }
    }

    // --- drawing

    /// The gutter: the change's mark, then the line numbers when on.
    fn gutter_of(&self, e: &Edit) -> usize {
        2 + if self.on("lineNumbers") { e.ed.lines.len().to_string().len() } else { 0 }
    }

    fn height_of(&self, e: &Edit) -> usize {
        let with = e.prompt.as_ref().is_some_and(|p| p.with.is_some());
        (self.size().1 as i64 - 2 - i64::from(e.disk.is_some()) - i64::from(with)).max(1) as usize
    }

    fn edit_height(&self) -> usize {
        self.state.edit.as_ref().map_or(1, |s| self.height_of(&s.0.borrow()))
    }

    /// The rows above the footer: the bar of a file changed on disk, and the replacement's field. Each with its hints,
    /// and what a click on one does.
    fn edit_bars(e: &Edit, w: usize) -> Vec<Bar> {
        let mut out = Vec::new();
        if let Some(disk) = &e.disk {
            let gone = matches!(disk, Disk::Gone);
            let lead = if gone { " ⚠ Deleted on disk " } else { " ⚠ Changed on disk " };
            let pairs: &[(&str, &str)] = if gone {
                &[("ctrl+s", "save it again"), ("alt+k", "dismiss")]
            } else {
                &[("alt+r", "reload, dropping your edits"), ("alt+k", "keep yours")]
            };
            let hints = ui::keys(pairs, w.saturating_sub(text_width(lead)));
            let mut segs = vec![seg(lead, format!("{}\x1b[1m", fg(c().orange)))];
            segs.extend(hints.segs.iter().cloned());
            out.push(Bar { segs, hints, from: text_width(lead) as i64, back: c().warn, y: 0, with: false });
        }
        if let Some(p) = e.prompt.as_ref().filter(|p| p.with.is_some()) {
            let with = p.with.as_ref().expect("with");
            let wi = w as i64;
            let hints = ui::keys(&[("⏎", "replace"), ("alt+a", "all")], usize::MAX);
            let hw = ui::seg_width(&hints.segs) as i64;
            let room = (wi - 4 - hw).max(1);
            let field = if p.on {
                with.segments(room, "")
            } else {
                vec![seg(ui::fit(&with.value, room as usize, None).0, fg(c().dim))]
            };
            let fw = ui::seg_width(&field) as i64;
            let mut segs = vec![seg(" → ", fg(c().dim))];
            segs.extend(field);
            segs.push(seg(" ".repeat((wi - 3 - fw - hw).max(0) as usize), ""));
            segs.extend(hints.segs.iter().cloned());
            out.push(Bar { segs, hints, from: (3 + fw).max(wi - hw), back: c().header, y: 0, with: true });
        }
        out
    }

    fn edit_footer(&mut self, w: usize) -> String {
        self.state.hints = None;
        let Some(shared) = self.state.edit.clone() else { return String::new() };
        let mut guard = shared.0.borrow_mut();
        let e = &mut *guard;
        let note = self.state.flash.as_ref().map_or(String::new(), |f| format!("  {f}"));
        let wi = w as i64;
        if let Some(p) = e.prompt.as_ref().filter(|p| p.kind == PromptKind::Line) {
            let lead = format!(" Go to line (1-{}) ", e.ed.lines.len());
            let mut segs = vec![seg(lead.clone(), fg(c().dim))];
            segs.extend(p.field.segments((wi - text_width(&(lead + &note)) as i64 - 1).max(1), ""));
            segs.push(seg(note, fg(c().dim)));
            return ui::line(&segs, w, Some(c().header));
        }
        if e.prompt.is_some() {
            let list = found(e);
            let sel = e.ed.selection();
            let i = sel.and_then(|(a, b)| {
                list.iter().position(|m| m.line == a.line && m.line == b.line && m.a == a.at && m.b == b.at)
            });
            let p = e.prompt.as_ref().expect("prompt");
            let count = if p.field.value.is_empty() {
                String::new()
            } else {
                format!(" {}/{}", i.map_or(0, |i| i + 1), list.len())
            };
            let room = (wi - 4 - text_width(&(count.clone() + &note)) as i64).max(1);
            let field = if p.on {
                vec![seg(ui::fit(&p.field.value, room as usize, None).0, fg(c().dim))]
            } else {
                p.field.segments(room, "")
            };
            let mut segs = vec![seg(format!(" {} ", g().search), fg(c().dim))];
            segs.extend(field);
            segs.push(seg(count + &note, fg(c().dim)));
            return ui::line(&segs, w, Some(c().header));
        }
        if let Some(text) = &self.state.flash {
            return ui::line(&ui::message(text, None), w, Some(c().header));
        }
        let ed = &e.ed;
        let pos = format!(" Ln {}, Col {} ", ed.cur.line + 1, buffer::col_of(&ed.lines[ed.cur.line], ed.cur.at) + 1);
        let pairs = [
            ("ctrl+s", "save"),
            ("esc", "done"),
            ("ctrl+z", "undo"),
            ("ctrl+f", "find"),
            ("ctrl+r", "replace"),
            ("ctrl+g", "line"),
            ("f1", "keys"),
        ];
        let keys = ui::keys(&pairs, w.saturating_sub(pos.len() + 1));
        let kw = ui::seg_width(&keys.segs);
        let mut segs = keys.segs.clone();
        segs.push(seg(format!("{pos:>0$}", w.saturating_sub(kw)), fg(c().dim)));
        self.state.hints = Some(keys);
        ui::line(&segs, w, Some(c().header))
    }

    /// The text with its gutter: on the left of each line, ▎ in green for a line added since the file was opened, in
    /// blue for one changed, ▔ in red over a line where lines were removed (▁ under the last line, for the end).
    pub(super) fn render_edit(&mut self) {
        let Some(shared) = self.state.edit.clone() else { return };
        let (w, h) = self.size();
        if std::mem::take(&mut shared.0.borrow_mut().reveal) {
            self.reveal_cursor();
        }
        // The header reads the edits: drawn before the edit is borrowed.
        let mut lines = vec![self.header(w)];
        let numbers = self.on("lineNumbers");
        let (bars, body_h, gw) = {
            let e = shared.0.borrow();
            (Self::edit_bars(&e, w), self.height_of(&e), self.gutter_of(&e))
        };
        let mut guard = shared.0.borrow_mut();
        let e = &mut *guard;
        let n = e.ed.lines.len();
        self.state.top = self.state.top.min(n as i64 - body_h as i64).max(0);
        let bw = self.scrollbar.set(n as i64, body_h as i64, self.state.top, &[]);
        let width = w.saturating_sub(gw + bw).max(1);
        let top = self.state.top as usize;
        highlight_lines(e, &self.state.file, top, top + body_h);
        let diff = e.ed.changed().clone();
        let sel = e.ed.selection();
        let mut hits: std::collections::HashMap<usize, Vec<Match>> = std::collections::HashMap::new();
        if e.prompt.as_ref().is_some_and(|p| p.kind != PromptKind::Line) {
            for m in found(e).iter().filter(|m| m.line >= top && m.line < top + body_h) {
                hits.entry(m.line).or_default().push(*m);
            }
        }
        let hl = e.hl.as_ref().expect("highlighted");
        for k in 0..body_h {
            let i = top + k;
            if i >= n {
                lines.push(ui::line(&[], w.saturating_sub(bw), None) + &self.scrollbar.cell(k as i64));
                continue;
            }
            let map = buffer::shown(&e.ed.lines[i]).map;
            let mut segs = hl.lines[i - hl.a].clone();
            if let Some(list) = hits.get(&i) {
                let ranges: Vec<(usize, usize)> = list.iter().map(|m| (map[m.a], map[m.b])).collect();
                segs = ui::mark(&segs, &ranges, &bg(c().find));
            }
            // The selection, one cell more past the end when it takes the line break.
            if let Some((a, b)) = sel.filter(|(a, b)| i >= a.line && i <= b.line) {
                let from = if i == a.line { a.at } else { 0 };
                let to = if i == b.line { b.at } else { e.ed.lines[i].len() };
                segs = ui::mark(&segs, &[(map[from], map[to])], &bg(c().select));
                if i < b.line {
                    segs.push(seg(" ", bg(c().select)));
                }
            }
            let end = i == n - 1 && diff.deleted.contains(&n);
            let mark = match diff.kinds.get(i) {
                Some(LineKind::Add) => seg("▎", fg(c().green)),
                Some(LineKind::Mod) => seg("▎", fg(c().blue)),
                _ if diff.deleted.contains(&i) => seg("▔", fg(c().red)),
                _ if end => seg("▁", fg(c().red)),
                _ => seg(" ", ""),
            };
            let num = if numbers { format!("{:>1$} ", i + 1, gw - 2) } else { " ".into() };
            let mut row = vec![mark, seg(num, fg(if i == e.ed.cur.line { c().text } else { c().dim }))];
            row.extend(ui::slice(&segs, self.state.left.max(0) as usize, width));
            lines.push(ui::line(&row, w.saturating_sub(bw), None) + &self.scrollbar.cell(k as i64));
        }
        let mut bars = bars;
        for b in &mut bars {
            b.y = lines.len() as i64 + 1;
            lines.push(ui::line(&b.segs, w, Some(b.back)));
        }
        e.bars = bars;
        let prompt = e.prompt.is_some();
        let x = gw as i64 + buffer::col_of(&e.ed.lines[e.ed.cur.line], e.ed.cur.at) as i64 - self.state.left + 1;
        let y = e.ed.cur.line as i64 - self.state.top + 2;
        drop(guard);
        lines.push(self.edit_footer(w));
        self.overlays(&mut lines, w, h);
        self.screen.paint(&lines);
        // The terminal's cursor on the text's, unless something covers it or it is scrolled out.
        let s = &self.state;
        let covered = s.menu.is_some() || s.help.is_some() || s.ask.is_some() || prompt;
        let inside = y >= 2 && y < 2 + body_h as i64 && x > gw as i64 && x <= (w - bw) as i64;
        show_cursor((!covered && inside).then_some((y, x)));
        self.place_image();
        self.place_figures();
    }

    /// The view moved to the cursor: just enough when it is near, a third of the way down when it jumped (a find, a
    /// line).
    pub(super) fn reveal_cursor(&mut self) {
        let Some(shared) = self.state.edit.clone() else { return };
        let e = shared.0.borrow();
        let (w, body_h, gw) = (self.size().0 as i64, self.height_of(&e) as i64, self.gutter_of(&e) as i64);
        let ed = &e.ed;
        let line = ed.cur.line as i64;
        let s = &mut self.state;
        if line < s.top || line >= s.top + body_h {
            let far = line < s.top - 1 || line > s.top + body_h;
            s.top = if far {
                (line - body_h / 3).max(0)
            } else if line < s.top {
                line
            } else {
                line - body_h + 1
            };
        }
        let width = (w - gw - i64::from(ed.lines.len() as i64 > body_h)).max(1);
        let col = buffer::col_of(&ed.lines[ed.cur.line], ed.cur.at) as i64;
        let margin = 8.min(width / 4);
        if col < s.left {
            s.left = (col - margin).max(0);
        } else if col >= s.left + width {
            s.left = col - width + 1 + margin;
        }
    }

    fn follow(&mut self) {
        self.reveal_cursor();
        self.render();
    }

    // --- keys, mouse, clipboard

    pub(super) fn edit_key(&mut self, k: &str) {
        let Some(shared) = self.state.edit.clone() else { return };
        if shared.0.borrow().prompt.is_some() {
            return self.prompt_key(k);
        }
        let name = k.replacen("shift+", "", 1);
        let shift = name != k && k != "shift+tab";
        if let Some(to) = move_of(&name) {
            let page = self.edit_height() - 1;
            shared.0.borrow_mut().ed.move_(to, shift, page);
            return self.follow();
        }
        let ed = |f: &mut dyn FnMut(&mut Editor)| f(&mut shared.0.borrow_mut().ed);
        match k {
            "ctrl+s" => {
                let file = self.state.file.clone();
                self.save_edit(shared.clone(), file, None, false)
            }
            "escape" => {
                let mut had = false;
                ed(&mut |ed| {
                    if ed.selection().is_some() {
                        ed.place(ed.cur, false);
                        had = true;
                    }
                });
                if had { self.render() } else { self.stop_edit() }
            }
            "ctrl+z" => {
                ed(&mut |ed| {
                    ed.undo();
                });
                self.follow()
            }
            "ctrl+y" | "ctrl+shift+z" => {
                ed(&mut |ed| {
                    ed.redo();
                });
                self.follow()
            }
            "ctrl+c" | "ctrl+x" => self.copy_cut(k == "ctrl+x"),
            "ctrl+v" => self.paste_clipboard(),
            "ctrl+a" => {
                ed(&mut Editor::select_all);
                self.render()
            }
            "ctrl+f" => self.open_prompt(PromptKind::Find),
            "ctrl+r" => self.open_prompt(PromptKind::Replace),
            "ctrl+g" => self.open_prompt(PromptKind::Line),
            "alt+r" | "alt+k" => self.disk_key(k),
            "enter" => {
                ed(&mut |ed| {
                    ed.newline();
                });
                self.follow()
            }
            "tab" | "shift+tab" => {
                ed(&mut |ed| {
                    ed.indent_lines(if k == "tab" { 1 } else { -1 });
                });
                self.follow()
            }
            "backspace" | "alt+backspace" => {
                ed(&mut |ed| {
                    ed.erase(-1, k != "backspace");
                });
                self.follow()
            }
            "delete" | "ctrl+delete" => {
                ed(&mut |ed| {
                    ed.erase(1, k != "delete");
                });
                self.follow()
            }
            "f1" => {
                self.state.help = Some(ui::HelpState::default());
                self.render()
            }
            "ctrl+p" => {
                let root = &self.state.root;
                let dir = if root.is_empty() { paths::dirname(&self.state.file) } else { root.clone() };
                let pane = self.pane.clone();
                self.act(move || actions::search(&dir, &pane, "files", ""))
            }
            "ctrl+pagedown" | "ctrl+pageup" => {
                let step = if k == "ctrl+pagedown" { 1 } else { -1 };
                self.show_tab(crate::core::tabs::step(&self.tabs, step).current)
            }
            _ => {
                if ui::printable(k) {
                    let text = if k == "space" { " " } else { k };
                    ed(&mut |ed| {
                        ed.type_(text, "type");
                    });
                    self.follow()
                }
            }
        }
    }

    /// ctrl+f, ctrl+r, ctrl+g: the footer's field, with the selected text (or the last query) to find. A find selects
    /// the first match from the cursor as the query is typed.
    fn open_prompt(&mut self, kind: PromptKind) {
        let Some(shared) = self.state.edit.clone() else { return };
        {
            let mut e = shared.0.borrow_mut();
            let sel = e.ed.selected();
            let query = if !sel.is_empty() && !sel.contains('\n') { sel } else { e.query.clone() };
            let was = e.prompt.as_ref().and_then(|p| p.with.as_ref()).map(|f| f.value.clone()).unwrap_or_default();
            e.prompt = Some(Prompt {
                kind,
                field: ui::Field::new(if kind == PromptKind::Line { "" } else { &query }),
                with: (kind == PromptKind::Replace).then(|| ui::Field::new(&was)),
                on: false,
            });
        }
        self.render();
    }

    fn prompt_key(&mut self, k: &str) {
        let Some(shared) = self.state.edit.clone() else { return };
        let kind = match k {
            "ctrl+f" => Some(PromptKind::Find),
            "ctrl+r" => Some(PromptKind::Replace),
            "ctrl+g" => Some(PromptKind::Line),
            _ => None,
        };
        if k == "escape" {
            shared.0.borrow_mut().prompt = None;
            return self.render();
        }
        if let Some(kind) = kind {
            return self.open_prompt(kind);
        }
        if k == "ctrl+s" {
            let file = self.state.file.clone();
            return self.save_edit(shared, file, None, false);
        }
        if k == "alt+r" || k == "alt+k" {
            return self.disk_key(k);
        }
        let mut guard = shared.0.borrow_mut();
        let e = &mut *guard;
        let Some(p) = e.prompt.as_mut() else { return };
        if p.kind == PromptKind::Line {
            if k == "enter" {
                let n = js::parse_int(&p.field.value);
                e.prompt = None;
                if let Some(n) = n.filter(|&n| n > 0.0) {
                    let line = (n as usize).min(e.ed.lines.len());
                    e.ed.goto(line);
                }
                drop(guard);
                return self.follow();
            }
            let changed = p.field.key(k);
            drop(guard);
            if changed {
                self.render();
            }
            return;
        }
        let q = p.field.value.clone();
        let with = p.with.as_ref().map(|f| f.value.clone());
        if (k == "tab" || k == "shift+tab") && with.is_some() {
            p.on = !p.on;
            drop(guard);
            return self.render();
        }
        if let Some(with) = with.as_ref().filter(|_| k == "alt+a") {
            let n = if q.is_empty() { 0 } else { e.ed.replace_all(&q, with) };
            drop(guard);
            self.flash(Some(if n > 0 { format!("Replaced {n}") } else { "No match".into() }));
            return self.follow();
        }
        if k == "enter" && p.on {
            if !q.is_empty() {
                e.ed.replace_one(&q, with.as_deref().unwrap_or(""));
            }
            drop(guard);
            return self.follow();
        }
        if k == "enter" || k == "down" || k == "up" {
            let miss = !q.is_empty() && e.ed.find(&q, if k == "up" { -1 } else { 1 }, false).is_none();
            drop(guard);
            if miss {
                self.flash(Some("No match".into()));
            }
            return self.follow();
        }
        let field = if p.on { p.with.as_mut().expect("with") } else { &mut p.field };
        if !field.key(k) {
            return;
        }
        if !p.on {
            e.query = p.field.value.clone();
            if !e.query.is_empty() {
                let query = e.query.clone();
                e.ed.find(&query, 1, true);
            }
        }
        drop(guard);
        self.follow();
    }

    /// ctrl+c, ctrl+x: the selection, else the cursor's line, as VS Code does.
    fn copy_cut(&mut self, cut: bool) {
        let Some(shared) = self.state.edit.clone() else { return };
        let text = {
            let mut e = shared.0.borrow_mut();
            let mut text = e.ed.selected();
            if text.is_empty() {
                text = format!("{}\n", e.ed.lines[e.ed.cur.line]);
                if cut {
                    let cur = e.ed.cur;
                    e.ed.select_line(cur);
                }
            }
            text
        };
        self.clip = text.clone();
        if cut {
            shared.0.borrow_mut().ed.type_("", "");
        }
        self.follow();
        let n = text.split('\n').count() - usize::from(text.ends_with('\n'));
        let what = if n > 1 { format!("{n} lines") } else { format!("{} characters", text.chars().count()) };
        let done = format!("{} {what}", if cut { "Cut" } else { "Copied" });
        self.act(move || Some(actions::copy(&text, &done)));
    }

    /// ctrl+v: the system's clipboard (actions::clipboard), else the text copied last here; the terminal's own paste
    /// comes as a bracketed paste (edit_paste).
    fn paste_clipboard(&mut self) {
        let Some(id) = self.state.edit.as_ref().map(|e| e.0.borrow().id) else { return };
        let clip = self.clip.clone();
        self.herdr.send(Box::new(move || {
            let text = actions::clipboard();
            Box::new(move |p: &mut Preview| {
                if p.state.edit.as_ref().map(|e| e.0.borrow().id) != Some(id) {
                    return;
                }
                let text = text.unwrap_or(clip);
                if text.is_empty() {
                    return p.flash(Some("Nothing to paste".into()));
                }
                p.edit_paste(&text.replace('\n', " "), Some(&text));
            })
        }));
    }

    /// A paste: into the footer's field, on one line; else into the text as it came, tabs and lines kept, as one step.
    pub(super) fn edit_paste(&mut self, text: &str, raw: Option<&str>) {
        let Some(shared) = self.state.edit.clone() else { return };
        {
            let mut guard = shared.0.borrow_mut();
            let e = &mut *guard;
            if let Some(p) = e.prompt.as_mut() {
                let field = if p.on { p.with.as_mut().expect("with") } else { &mut p.field };
                field.insert(text);
                if !p.on && p.kind != PromptKind::Line {
                    e.query = p.field.value.clone();
                    if !e.query.is_empty() {
                        let query = e.query.clone();
                        e.ed.find(&query, 1, true);
                    }
                }
            } else {
                e.ed.type_(raw.unwrap_or(text), "");
            }
        }
        self.follow();
    }

    /// The mouse while editing: a click puts the cursor (with shift, extends the selection), a double click selects a
    /// word, a triple click the line; a drag selects, scrolling at the edges; the wheel scrolls.
    pub(super) fn edit_mouse(&mut self, m: &Mouse) {
        let Some(shared) = self.state.edit.clone() else { return };
        let w = self.size().0 as i64;
        let (n, body_h, gw) = {
            let e = shared.0.borrow();
            (e.ed.lines.len() as i64, self.height_of(&e) as i64, self.gutter_of(&e) as i64)
        };
        if m.wheel != 0 {
            self.state.top = (n - body_h).min(self.state.top + m.wheel * 3).max(0);
            return self.render();
        }
        match self.scrollbar.mouse(m, m.y - 2, m.x == w && !self.edit_drag, Instant::now()) {
            ui::BarHit::Ignored => {}
            ui::BarHit::Took => return,
            ui::BarHit::Scroll(top) => {
                self.state.top = top;
                return self.render();
            }
        }
        if m.y == 1 && !self.edit_drag {
            return self.header_mouse(m);
        }
        if m.y > 1 + body_h && !self.edit_drag {
            let hit = if m.press && !m.drag && m.button == 0 {
                let e = shared.0.borrow();
                e.bars
                    .iter()
                    .find(|b| b.y == m.y)
                    .and_then(|b| ui::key_at(&b.hints, m.x - 1, b.from).map(|k| (k, b.with)))
            } else {
                None
            };
            if let Some((k, with)) = hit {
                if with {
                    if let Some(p) = shared.0.borrow_mut().prompt.as_mut() {
                        p.on = true;
                    }
                    self.prompt_key(&k);
                } else {
                    self.edit_key(&k);
                }
            }
            return;
        }
        if m.button != 0 && !(m.drag && self.edit_drag) {
            return;
        }
        let under = |e: &Edit, top: i64, left: i64| {
            let line = (top + m.y.max(2).min(1 + body_h) - 2).min(n - 1).max(0) as usize;
            let text = &e.ed.lines[line];
            let at = if m.y < 2 {
                0
            } else if m.y > 1 + body_h {
                text.len()
            } else {
                buffer::at_col(text, (m.x - 1 - gw + left).max(0) as usize)
            };
            Pos::new(line, at)
        };
        if m.press && !m.drag {
            let now = Instant::now();
            let (time, x, y, count) = self.last_click;
            let again = time.is_some_and(|t| now - t < super::input::DOUBLE) && m.x == x && m.y == y;
            let count = if again { count % 3 + 1 } else { 1 };
            self.last_click = (Some(now), m.x, m.y, count);
            {
                let mut e = shared.0.borrow_mut();
                let at = under(&e, self.state.top, self.state.left);
                match count {
                    2 => e.ed.select_word(at),
                    3 => e.ed.select_line(at),
                    _ => e.ed.place(at, m.mods & 4 != 0),
                }
            }
            self.edit_drag = count == 1;
            return self.render();
        }
        if m.drag && self.edit_drag {
            // At the edges, scrolls, then takes the position under the mouse.
            let s = &mut self.state;
            if m.y <= 2 && s.top > 0 {
                s.top -= 1;
            }
            if m.y > body_h {
                s.top = (s.top + 1).min((n - body_h).max(0));
            }
            if m.x >= w - 1 {
                s.left += 2;
            } else if m.x <= gw + 1 {
                s.left = (s.left - 2).max(0);
            }
            {
                let mut e = shared.0.borrow_mut();
                let at = under(&e, self.state.top, self.state.left);
                e.ed.place(at, true);
            }
            return self.render();
        }
        if !m.press {
            self.edit_drag = false;
        }
    }
}

/// The text's lines from..to highlighted (their shown text, buffer::shown), with LOOK lines before them, so that a
/// comment or a string opened above is still coloured. Kept until an edit, or a scroll out of them.
fn highlight_lines(e: &mut Edit, file: &str, from: usize, to: usize) {
    let version = e.ed.version;
    if e.hl.as_ref().is_some_and(|c| c.version == version && from >= c.lo && to <= c.hi) {
        return;
    }
    let a = from.saturating_sub(LOOK);
    let b = e.ed.lines.len().min(to + LOOK);
    let text: Vec<String> = e.ed.lines[a..b].iter().map(|l| buffer::shown(l).text).collect();
    let lang = crate::render::highlight::language(&paths::basename(file, None));
    let segs = lang.map(|l| crate::render::highlight::highlight(&text.join("\n"), Some(l)));
    // The highlighter's text must be the line's, the marks (selection, matches) being placed by offsets in it.
    let lines = text
        .into_iter()
        .enumerate()
        .map(|(i, t)| match segs.as_ref().and_then(|s| s.get(i)) {
            Some(s) if ui::seg_text(s) == t => s.clone(),
            _ => vec![seg(t, "")],
        })
        .collect();
    e.hl = Some(Hl { version, lo: if a == 0 { 0 } else { a + LOOK / 4 }, hi: b, a, lines });
}

/// The matches of the find's query, computed once per query and text.
fn found(e: &mut Edit) -> Vec<Match> {
    let q = e.prompt.as_ref().map_or(String::new(), |p| p.field.value.clone());
    let version = e.ed.version;
    if !e.found.as_ref().is_some_and(|(fq, v, _)| *fq == q && *v == version) {
        let list = buffer::matches(&e.ed.lines, &q);
        e.found = Some((q, version, list));
    }
    e.found.as_ref().map(|f| f.2.clone()).unwrap_or_default()
}

/// The terminal's cursor, a bar at (row, column) while editing; hidden otherwise.
pub fn show_cursor(at: Option<(i64, i64)>) {
    if screen::handed() {
        return;
    }
    if let Some((y, x)) = at {
        let _ = sys::write_stdout(format!("\x1b[{y};{x}H\x1b[6 q\x1b[?25h").as_bytes());
        CURSOR_SHOWN.store(true, Ordering::Relaxed);
    } else if CURSOR_SHOWN.swap(false, Ordering::Relaxed) {
        let _ = sys::write_stdout(b"\x1b[?25l\x1b[0 q");
    }
}
