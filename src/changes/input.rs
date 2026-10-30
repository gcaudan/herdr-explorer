//! Keys and mouse of the Changes pane: moving and selecting, the menus and questions, and the notes typed in the
//! footer.

use super::Pane;
use super::state::{Asked, Draft};
use crate::core::scope::{self, RevKey, Scope};
use crate::core::stage::Mode;
use crate::core::ui::{self, Event, Field, HelpState, Mouse};
use crate::core::{actions, notes, paths};
use crate::diff::{self, Row, disk_lines, shown_lines};
use std::time::Instant;

impl Pane {
    pub(super) fn event(&mut self, e: Event) {
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

    pub(super) fn key(&mut self, k: &str) {
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
                self.anchor = None;
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
                let widest = self.parsed.as_deref().map_or(0, diff::widest_of);
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

    pub(super) fn mouse(&mut self, m: &Mouse) {
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
        // A note, a prompt or a question takes the keys: no menu opened over it, the cursor left where it is.
        let asking = self.note.is_some() || self.prompt.is_some() || self.confirm.is_some();
        // The header's switches: a click on a choice takes it.
        if click && m.y == 1 && !asking {
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
        if (click || (m.drag && m.button == 0)) && !asking && m.y >= 2 && m.y < h && on_row {
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

    /// The cursor moves over the rows that show lines of a file, the first row of a wrapped line; the view follows
    /// it. select: shift held, the rows from where the cursor was are selected, else the selection goes.
    pub(super) fn move_by(&mut self, n: i64, select: bool) {
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

    pub(super) fn follow(&mut self) {
        let body = self.body();
        if self.cur < self.top0 {
            self.top0 = self.cur;
        }
        if self.cur >= self.top0 + body {
            self.top0 = self.cur - body + 1;
        }
    }

    pub(super) fn scroll(&mut self, n: i64) {
        self.top0 += n;
        self.draw();
    }

    /// Next or previous hunk: its header comes to the top, with the cursor.
    pub(super) fn jump(&mut self, dir: i64) {
        let cur = self.cur;
        let ok = |(k, r): &(usize, &Row)| r.hunk && if dir > 0 { *k as i64 > cur } else { (*k as i64) < cur };
        let found =
            if dir > 0 { self.rows.iter().enumerate().find(ok) } else { self.rows.iter().enumerate().rev().find(ok) };
        if let Some((i, _)) = found {
            self.anchor = None;
            self.cur = i as i64;
            self.top0 = i as i64;
            self.draw();
        }
    }

    /// ] and [: the next or previous file not viewed, its name at the top.
    pub(super) fn next_file(&mut self, dir: i64) {
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

    pub(super) fn start_note(&mut self) {
        let Some(r) = self.cur_ref() else { return };
        if self.tab.is_none() {
            return self.say("Notes need herdr".into());
        }
        self.note = Some(Draft { field: Field::new(""), kind: String::new(), r#ref: r });
        self.draw();
    }

    pub(super) fn note_key(&mut self, k: &str) {
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
                if n.field.value.chars().all(crate::unicode::is_space) {
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

    pub(super) fn prompt_key(&mut self, k: &str) {
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
}
