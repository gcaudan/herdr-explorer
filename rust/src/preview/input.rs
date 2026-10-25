//! The preview's keyboard and mouse (src/preview.js): the menu, the key help and the find first, then the keys of
//! the view; the header's tabs and chips, the body's selection of characters, copied when released.

use super::{Find, Kind, Opt, Pos, Preview, Rows, Select};
use crate::core::actions;
use crate::core::regex::Regex;
use crate::core::stage::Mode;
use crate::core::tabs;
use crate::core::ui::{self, BarHit, Choice, Event, Field, HelpState, Menu, MenuItem, Mouse, c, fg};
use crate::core::{js, paths};
use crate::unicode;
use std::time::{Duration, Instant};

/// Two clicks closer than this are a double click.
pub(super) const DOUBLE: Duration = Duration::from_millis(400);

/// The end of the character at, with the combining marks that follow it (a decomposed accent, an emoji selector).
fn char_end(text: &str, at: usize) -> usize {
    let Some(ch) = text.get(at..).and_then(|t| t.chars().next()) else { return at.max(text.len().min(at)) };
    let mut i = at + ch.len_utf8();
    while let Some(next) = text[i..].chars().next() {
        if ui::cols(next, None) != 0 {
            break;
        }
        i += next.len_utf8();
    }
    i
}

fn before(a: Pos, b: Pos) -> bool {
    a.row < b.row || (a.row == b.row && a.at <= b.at)
}

/// The ends of a selection, in order.
pub(super) fn ordered(sel: &Select) -> (Pos, Pos) {
    if before(sel.from, sel.to) { (sel.from, sel.to) } else { (sel.to, sel.from) }
}

/// A word's character: `/[\p{L}\p{N}\p{M}_$]/u`.
fn wordy(ch: char) -> bool {
    unicode::is_letter(ch) || unicode::is_number(ch) || unicode::is_mark(ch) || ch == '_' || ch == '$'
}

/// A query searched literally: the regex's special characters escaped.
fn escape(q: &str) -> String {
    let mut out = String::new();
    for ch in q.chars() {
        if ".*+?^${}()|[]\\".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

impl Preview {
    /// An event of the screen.
    pub(super) fn event(&mut self, e: Event) {
        match e {
            Event::Key(k) => self.key(&k),
            Event::Mouse(m) => self.mouse(&m),
            // Back to the pane: a commit made meanwhile changes what is changed since HEAD.
            Event::Focus(f) => {
                self.state.focused = f;
                if f {
                    self.load_changes();
                } else {
                    self.scrollbar.stop();
                    self.end_selection();
                    let pane = self.pane.clone();
                    self.herdr.send(Box::new(move || {
                        actions::unzoom(&pane);
                        Box::new(|_: &mut Preview| {})
                    }));
                }
            }
            Event::Paste { text, raw } => {
                if self.state.ask.is_some() {
                    return;
                }
                if self.state.edit.is_some() {
                    return self.edit_paste(&text, Some(&raw));
                }
                if let Some(f) = &mut self.state.rev_prompt {
                    f.insert(&text);
                    self.render();
                } else if let Some(f) = self.state.find.as_mut().filter(|f| f.active) {
                    f.field.insert(&text);
                    self.search(true);
                }
            }
            Event::Reply(text) => {
                if ui::kitty_ok(&text) {
                    self.kitty.supported = true;
                    self.kitty.probing = false;
                    self.state.rows = None;
                    self.render();
                }
                // The size of a cell in pixels: \e[6;<height>;<width>t.
                if let Some(rest) = text.strip_prefix("\x1b[6;").and_then(|t| t.strip_suffix('t')) {
                    let parts: Vec<&str> = rest.split(';').collect();
                    let n = |s: &str| {
                        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<i64>().unwrap_or(0))
                    };
                    if let [h, w] = parts[..]
                        && let (Some(h), Some(w)) = (n(h), n(w))
                        && h != 0
                        && w != 0
                    {
                        self.kitty.cell = (w, h);
                        self.state.rows = None;
                        self.render();
                    }
                }
            }
        }
    }

    fn scroll(&mut self, n: i64) {
        self.state.top = self.state.top.saturating_add(n);
        self.render();
    }

    pub(super) fn key(&mut self, k: &str) {
        if self.state.ask.is_some() {
            return self.ask_key(k);
        }
        if self.state.rev_prompt.is_some() {
            return self.rev_key(k);
        }
        if self.state.menu.is_some() {
            return self.menu_key(k);
        }
        if self.state.edit.is_some() && self.state.help.is_none() && self.state.show.is_none() {
            return self.edit_key(k);
        }
        if let Some(f) = self.state.find.as_mut().filter(|f| f.active) {
            if k == "escape" {
                self.state.find = None;
                self.state.matches.clear();
                return self.render();
            }
            if k == "enter" || k == "down" {
                f.active = k != "enter" || self.state.matches.is_empty();
                return self.next(1, false);
            }
            if k == "up" {
                return self.next(-1, false);
            }
            if f.field.key(k) {
                self.search(true);
            }
            return;
        }
        let page = self.size().1 as i64 - 3;
        // The key help (ui::help_key): scrolls, finds with /, closes with esc, q, ? or f1.
        // While editing, ctrl+c (copy there) only closes the help, never the pane.
        if self.state.help.is_some() && self.state.edit.is_some() && k == "ctrl+c" {
            self.state.help = None;
            return self.render();
        }
        if let Some(help) = &mut self.state.help
            && k != "ctrl+c"
        {
            if !ui::help_key(k, help) {
                self.state.help = None;
            }
            return self.render();
        }
        if self.state.show.is_some() {
            if k == "q" || k == "escape" {
                return self.leave_show();
            }
            if !Preview::show_key(k) {
                return;
            }
        }
        let wrap = self.on("wrap");
        let s = &mut self.state;
        match k {
            "?" | "f1" => {
                s.help = Some(HelpState::default());
                self.render()
            }
            "q" | "escape" | "ctrl+c" => {
                if k == "escape" && s.select.is_some() {
                    s.select = None;
                    return self.render();
                }
                if k == "escape" && !s.marks.is_empty() {
                    s.marks.clear();
                    s.rows = None;
                    return self.render();
                }
                if s.find.is_some() {
                    s.find = None;
                    s.matches.clear();
                    return self.render();
                }
                self.quit()
            }
            "up" | "k" => self.scroll(-1),
            "down" | "j" | "enter" => self.scroll(1),
            "pageup" => self.scroll(-page),
            "pagedown" | "space" => self.scroll(page),
            "ctrl+u" => self.scroll(-page.div_euclid(2)),
            "ctrl+d" => self.scroll(page.div_euclid(2)),
            "home" | "g" => {
                s.top = 0;
                s.left = 0;
                self.render()
            }
            "end" | "G" => {
                s.top = i64::MAX;
                self.render()
            }
            "left" | "h" => {
                s.left = (s.left - 8).max(0);
                self.render()
            }
            "right" | "l" => {
                if !wrap {
                    s.left += 8;
                }
                self.render()
            }
            "/" => {
                if s.diff {
                    return self.flash(Some("Find works in the file: d".into()));
                }
                s.find = Some(Find { field: Field::default(), active: true });
                self.render()
            }
            "n" | "N" => {
                let dir = if k == "n" { 1 } else { -1 };
                if s.show.is_some() {
                    return self.next_hunk(dir);
                }
                if !s.marks.is_empty() && s.matches.is_empty() {
                    return self.next_mark(dir);
                }
                if s.diff { self.next_hunk(dir) } else { self.next(dir, false) }
            }
            "d" => self.toggle_view(),
            "tab" | "ctrl+pagedown" => self.show_tab(tabs::step(&self.tabs, 1).current),
            "shift+tab" | "ctrl+pageup" => self.show_tab(tabs::step(&self.tabs, -1).current),
            "t" => {
                let text = if self.keep_tab(self.tabs.current) {
                    "Tab kept: the next file clicked opens in another tab"
                } else {
                    "Tab already kept"
                };
                self.flash(Some(text.into()))
            }
            "x" => self.close_tab(self.tabs.current),
            "," => self.open_menu(0),
            "b" => {
                if s.diff {
                    self.open_scopes()
                }
            }
            "v" => {
                if s.diff || s.show.is_some() {
                    self.switch_layout()
                }
            }
            "s" | "u" | "D" => {
                if s.diff {
                    self.edit_diff(match k {
                        "s" => Mode::Stage,
                        "u" => Mode::Unstage,
                        _ => Mode::Discard,
                    })
                }
            }
            "w" => self.toggle("wrap"),
            "L" => self.toggle("lineNumbers"),
            "c" => {
                self.toggle("changes");
                self.load_changes()
            }
            "]" | "[" => {
                self.next_change(if k == "]" { 1 } else { -1 });
                self.render()
            }
            "p" => self.toggle_sharp(),
            "m" => {
                if s.kind == Kind::Markdown {
                    s.raw = !s.raw;
                    s.rows = None;
                    s.top = 0;
                    self.render();
                }
            }
            "r" => self.load(true),
            "e" | "E" | "o" => {
                self.keep_tab(self.tabs.current);
                let (file, pane, k) = (self.state.file.clone(), self.pane.clone(), k.to_string());
                self.act(move || match k.as_str() {
                    "e" => actions::editor(&file, &pane),
                    "E" => actions::edit(&file, &pane, None),
                    _ => actions::app(&file, &pane),
                })
            }
            "i" => self.start_edit(),
            "y" => {
                let file = self.state.file.clone();
                self.act(move || Some(actions::copy(&file, "Path copied")))
            }
            "Y" => self.copy_reference(),
            "H" => {
                let (dir, pane, file) = (self.dir(), self.pane.clone(), self.state.file.clone());
                self.act(move || actions::scm(&dir, &pane, &file, true))
            }
            // TODO(case 13): the review notes (a, A, C, U).
            "a" => {
                if self.tab.is_empty() {
                    self.flash(Some("Notes need the explorer".into()))
                }
            }
            "z" => {
                let pane = self.pane.clone();
                self.herdr.send(Box::new(move || {
                    actions::zoom(&pane);
                    Box::new(|_: &mut Preview| {})
                }));
            }
            "<" | ">" => self.widen(if k == "<" { -4.0 } else { 4.0 }),
            // The inner border pushed: on the left of a preview that is the right part of its split, as it is most
            // often.
            "shift+left" | "shift+right" => {
                let left = self.keeper.lock().unwrap_or_else(|p| p.into_inner()).left() == Some(true);
                self.widen(if (k == "shift+left") == left { -4.0 } else { 4.0 })
            }
            "ctrl+p" | "ctrl+f" => {
                let (dir, pane) = (self.dir(), self.pane.clone());
                let mode = if k == "ctrl+p" { "files" } else { "content" };
                self.act(move || actions::search(&dir, &pane, mode, ""))
            }
            _ => {}
        }
    }

    /// The root, else the file's folder: where go to file, the search and source control start.
    fn dir(&self) -> String {
        if self.state.root.is_empty() { paths::dirname(&self.state.file) } else { self.state.root.clone() }
    }

    fn toggle_view(&mut self) {
        if self.state.edit.is_some() {
            return self.flash(Some("Not while editing: esc first".into()));
        }
        let s = &mut self.state;
        s.diff = !s.diff;
        s.rows = None;
        s.top = 0;
        s.left = 0;
        s.select = None;
        s.find = None;
        s.matches.clear();
        if s.diff {
            self.load_diff(false, false);
        } else if s.line != 0.0 {
            let line = s.line;
            self.state.top = (self.row_of(line - 1.0) - self.third()).max(0);
        }
        self.render();
    }

    // --- find in file

    /// Case-insensitive unless the query has a capital letter.
    pub(super) fn search(&mut self, jump: bool) {
        let q = self.state.find.as_ref().map(|f| f.field.value.clone()).unwrap_or_default();
        let mut matches = Vec::new();
        if !q.is_empty() {
            let flags = if q != js::lower(&q) { "g" } else { "gi" };
            if let Ok(re) = Regex::new(&escape(&q), flags) {
                // Rendered Markdown: matches in the displayed text.
                let lines: Vec<String> = if self.markdown_view() {
                    self.rows().list.iter().map(|r| ui::seg_text(&r.segs)).collect()
                } else {
                    self.state.plain.clone()
                };
                for (i, l) in lines.iter().enumerate() {
                    matches.extend(re.matches(l).iter().map(|m| (i, m.start(), m.end())));
                }
            }
        }
        self.state.matches = matches;
        self.state.matched = -1;
        if jump {
            self.next(1, true);
        }
    }

    pub(super) fn next(&mut self, dir: i64, from_top: bool) {
        if self.state.matches.is_empty() {
            return self.render();
        }
        let len = self.state.matches.len() as i64;
        let md = self.markdown_view();
        if from_top {
            // `(rows()[top] || {}).src || 0` (the top row in rendered Markdown), less one going down.
            let rows = self.rows();
            let top =
                if md { self.state.top } else { rows.list.get(self.state.top.max(0) as usize).map_or(0, |r| r.src) };
            let at = top - i64::from(dir > 0);
            let i = self.state.matches.iter().position(|m| m.0 as i64 > at);
            self.state.matched = i.map_or(0, |i| i as i64);
        } else {
            self.state.matched = (self.state.matched + dir).rem_euclid(len);
        }
        let (l, a, _) = self.state.matches[self.state.matched as usize];
        let row = if md { l as i64 } else { self.row_of(l as f64) };
        let (w, h) = self.size();
        let body_h = h as i64 - 2;
        if row < self.state.top || row >= self.state.top + body_h {
            self.state.top = (row - body_h.div_euclid(3)).max(0);
        }
        if !md {
            // Less the scrollbar's column, shown when the rows do not fit.
            let rows = self.rows().list.len() as i64;
            let width = w as i64 - self.gutter() as i64 - i64::from(rows > body_h);
            // Matches found in rendered Markdown, then m: cut as slice does, a line past the end empty.
            let line = self.state.plain.get(l).map_or("", String::as_str);
            let col = ui::text_width(&line[..line.floor_char_boundary(a.min(line.len()))]) as i64;
            if col < self.state.left || col >= self.state.left + width {
                self.state.left = (col - 8).max(0);
            }
        }
        // The match itself is marked: no line highlight.
        self.state.line = 0.0;
        self.render();
    }

    // --- the view's options (⚙ or ,)

    /// The options of the view shown, as the explorer's settings show theirs: each changes in place with ← →, enter
    /// or a click, the menu staying open; the key beside it does the same outside the menu.
    pub(super) fn open_menu(&mut self, sel: usize) {
        let w = self.size().0 as i64;
        let s = &self.state;
        let item = |label: &str, key: &str, value, run, keep| {
            Some(MenuItem { label: label.into(), key: key.into(), value: Some(value), danger: false, keep, run })
        };
        let mut items = Vec::new();
        if s.show.is_some() || s.diff {
            items = self.diff_items();
        } else {
            let editing = s.edit.is_some();
            if s.kind == Kind::Markdown && !editing {
                let value = (if s.raw { "raw" } else { "rendered" }).to_string();
                items.push(item("Markdown", "m", (value, fg(c().blue)), Opt::Markdown, true));
            }
            if !editing && (s.kind == Kind::Text || (s.kind == Kind::Markdown && s.raw)) {
                items.push(item("Wrap lines", "w", ui::on_off(self.on("wrap")), Opt::Wrap, true));
                items.push(item("Line numbers", "L", ui::on_off(self.on("lineNumbers")), Opt::LineNumbers, true));
                items.push(item("Changes", "c", ui::on_off(self.on("changes")), Opt::Changes, true));
            }
            if editing {
                items.push(item("Line numbers", "L", ui::on_off(self.on("lineNumbers")), Opt::LineNumbers, true));
            }
            if editing || matches!(s.kind, Kind::Text | Kind::Markdown) {
                items.push(item("Edit here", "i", ui::on_off(editing), Opt::Edit, false));
            }
            if let Some((label, value)) = self.sharp_item() {
                items.push(item(label, "p", (value.to_string(), fg(c().blue)), Opt::Sharp, true));
            }
        }
        if items.is_empty() {
            return self.flash(Some("No view option for this file".into()));
        }
        let mut menu = Menu::new("View", items, 1, w, 0);
        menu.sel = sel.min(menu.items.len() - 1);
        self.state.menu = Some(menu);
        self.render();
    }

    /// A choice of the menu, by key or click: run, then the menu shown again with the new values, unless closed.
    fn menu_choice(&mut self, r: Option<Choice<Opt>>) {
        let sel = self.state.menu.as_ref().map_or(0, |m| m.sel);
        match r {
            None => {}
            Some(Choice::Close) => self.state.menu = None,
            Some(Choice::Run { run, keep }) => {
                self.state.menu = None;
                match run {
                    Opt::Wrap => self.toggle("wrap"),
                    Opt::LineNumbers => self.toggle("lineNumbers"),
                    Opt::Changes => {
                        self.toggle("changes");
                        self.load_changes()
                    }
                    Opt::Markdown => {
                        let s = &mut self.state;
                        s.raw = !s.raw;
                        s.rows = None;
                        s.top = 0;
                    }
                    Opt::Sharp => self.toggle_sharp(),
                    Opt::Layout => self.switch_layout(),
                    Opt::Compare => self.open_scopes(),
                    Opt::Scope(pick) => self.scope_item(pick),
                    Opt::Edit => {
                        if self.state.edit.is_some() {
                            self.stop_edit()
                        } else {
                            self.start_edit()
                        }
                    }
                }
                if keep {
                    self.open_menu(sel);
                }
            }
        }
        self.render();
    }

    fn menu_key(&mut self, k: &str) {
        let h = self.size().1 as i64;
        let r = self.state.menu.as_mut().and_then(|m| m.key(k, h));
        self.menu_choice(r);
    }

    // --- mouse

    pub(super) fn mouse(&mut self, m: &Mouse) {
        let (w, h) = self.size();
        if let Some(q) = &self.state.ask {
            let choice = ui::ask_mouse(m, &q.ask);
            return self.ask_choice(choice);
        }
        if let Some(menu) = &mut self.state.menu {
            let r = menu.mouse(m, h as i64);
            return self.menu_choice(r);
        }
        if let Some(help) = &mut self.state.help {
            if !ui::help_mouse(m, help) {
                self.state.help = None;
            }
            return self.render();
        }
        // A click on a hint of the footer does what its key does.
        if m.y == h as i64
            && m.press
            && !m.drag
            && m.wheel == 0
            && m.button == 0
            && let Some(k) = self.state.hints.as_ref().and_then(|hints| ui::key_at(hints, m.x - 1, 0))
        {
            return self.key(&k);
        }
        if self.state.edit.is_some() && self.state.show.is_none() {
            return self.edit_mouse(m);
        }
        // A commit shown: the wheel and the scrollbar only.
        if self.state.show.is_some() {
            if m.wheel != 0 {
                return self.scroll(m.wheel * 3);
            }
            if let BarHit::Scroll(top) = self.scrollbar.mouse(m, m.y - 2, m.x == w as i64, Instant::now()) {
                self.state.top = top;
                self.render();
            }
            return;
        }
        // The wheel scrolls under a selection, which may still be dragged: the button state is unknown here.
        if m.wheel != 0 {
            return self.scroll(m.wheel * 3);
        }
        let held = self.state.select.is_some_and(|s| !s.done);
        match self.scrollbar.mouse(m, m.y - 2, m.x == w as i64 && !held, Instant::now()) {
            BarHit::Ignored => {}
            BarHit::Took => return,
            BarHit::Scroll(top) => {
                self.state.top = top;
                return self.render();
            }
        }
        // The header has the chips; a selection going on is dragged or released over it like over the body.
        let selecting = held && (m.drag || !m.press);
        if m.y == 1 && !selecting {
            return self.header_mouse(m);
        }
        if m.button != 0 && !(m.drag && self.state.select.is_some()) {
            return;
        }
        self.body_mouse(m);
    }

    /// A click on the header: a tab shown, kept on a double-click, closed by a middle click or its ×; ⚙; File · Diff.
    pub(super) fn header_mouse(&mut self, m: &Mouse) {
        if !m.press || m.drag || (m.button != 0 && m.button != 1) {
            return;
        }
        self.end_selection();
        let Some(chip) = self.state.chips.iter().find(|c| m.x >= c.from && m.x <= c.to).cloned() else { return };
        if m.button == 1 {
            if let Some(tab) = chip.tab {
                self.close_tab(tab);
            }
            return;
        }
        match chip.id.as_str() {
            "close" => self.close_tab(chip.tab.unwrap_or(0)),
            "tab" => {
                let tab = chip.tab.unwrap_or(0);
                let now = Instant::now();
                let again = self.last_tab.0.is_some_and(|t| now - t < DOUBLE) && self.last_tab.1 == Some(tab);
                self.last_tab = (if again { None } else { Some(now) }, Some(tab));
                if again && self.keep_tab(tab) {
                    return self.render();
                }
                self.show_tab(tab)
            }
            "menu" => self.open_menu(0),
            "scope" => self.open_scopes(),
            id => {
                if (id == "diff") != self.state.diff {
                    self.toggle_view();
                }
            }
        }
    }

    /// The body: the selection of characters with the left button, copied when released; a double click selects a
    /// word, a triple click the line. Above the body: from the start of the top row; below: to the end of the bottom
    /// row.
    fn body_mouse(&mut self, m: &Mouse) {
        let (w, h) = self.size();
        let (w, h) = (w as i64, h as i64);
        let rows = self.rows();
        // An image: its hint's parts only.
        if self.state.kind == Kind::Image {
            let at = usize::try_from(self.state.top + m.y - 2).ok().filter(|_| m.press && !m.drag);
            let r = at.and_then(|i| rows.list.get(i));
            let chip = r.and_then(|r| r.hint.as_ref()).and_then(|h| h.iter().find(|c| m.x >= c.from && m.x <= c.to));
            if let Some(id) = chip.map(|c| c.id.clone()) {
                self.image_click(&id);
            }
            return;
        }
        if rows.list.is_empty() {
            return;
        }
        // right: the last column of text, before the scrollbar when it shows.
        let right = w - i64::from(self.scrollbar.bar.is_some());
        let under = |p: &Preview, rows: &Rows| {
            let row = (p.state.top + m.y.clamp(2, (h - 1).max(2)) - 2).clamp(0, rows.list.len() as i64 - 1) as usize;
            let x = if m.y < 2 {
                i64::MIN / 2
            } else if m.y >= h {
                i64::MAX / 2
            } else {
                m.x.min(right)
            };
            p.position_at(rows, row, x)
        };
        let mut pos = under(self, &rows);
        if m.press && !m.drag {
            self.end_selection();
            let now = Instant::now();
            let (time, x, y, count) = self.last_click;
            let again = time.is_some_and(|t| now - t < DOUBLE) && m.x == x && m.y == y;
            let count = if again { count % 3 + 1 } else { 1 };
            self.last_click = (Some(now), m.x, m.y, count);
            // The rows again: end_selection may have drawn the pane, and rows built for the same size are the same.
            let rows = self.rows();
            let sel = match count {
                2 => self.word_at(&rows, pos),
                3 => self.line_at(&rows, pos),
                _ => None,
            };
            self.state.select =
                Some(sel.unwrap_or(Select { from: pos, to: pos, moved: false, fixed: false, done: false }));
            return self.render();
        }
        let Some(sel) = self.state.select else { return };
        if m.drag {
            if sel.fixed || sel.done {
                return;
            }
            // At the edges, scrolls, then takes the position under the mouse again.
            let (top, left) = (self.state.top, self.state.left);
            if m.y <= 2 && self.state.top > 0 {
                self.state.top -= 1;
            }
            if m.y >= h - 1 {
                self.state.top = (self.state.top + 1).min((rows.list.len() as i64 - (h - 2)).max(0));
            }
            // Rendered rows never scroll sideways.
            if !self.on("wrap") && rows.list.get(pos.row).is_some_and(|r| r.src >= 0) {
                if m.x >= right {
                    self.state.left += 2;
                } else if m.x <= self.gutter() as i64 + 1 {
                    self.state.left = (self.state.left - 2).max(0);
                }
            }
            if self.state.top != top || self.state.left != left {
                pos = under(self, &rows);
            }
            if let Some(s) = &mut self.state.select {
                s.to = pos;
                s.moved = true;
            }
            return self.render();
        }
        if !m.press {
            self.end_selection();
        }
    }

    // --- mouse selection, by character

    /// The position under a screen cell of the body.
    fn position_at(&self, rows: &Rows, row: usize, x: i64) -> Pos {
        let r = &rows.list[row];
        let text = ui::seg_text(&self.segs_of(r));
        let shift = if !self.on("wrap") && r.src >= 0 { self.state.left } else { 0 };
        let col = x.saturating_sub(1 + self.gutter() as i64).saturating_add(shift);
        let (mut w, mut prev, mut prev_at) = (0i64, None, 0);
        for (at, ch) in text.char_indices() {
            let c = ui::cols(ch, prev) as i64;
            w += c;
            // The cell an emoji selector adds belongs to the character before it.
            if w > col {
                return Pos { row, at: if c != 0 && ch == '\u{fe0f}' && prev.is_some() { prev_at } else { at } };
            }
            prev = Some(ch);
            prev_at = at;
        }
        Pos { row, at: text.len() }
    }

    /// The selected bytes [start, end) of a display row, and whether the selection takes the line break at its end
    /// (none between the rows of a wrapped line).
    pub(super) fn selected_range(&self, rows: &Rows, i: usize) -> Option<(usize, usize, bool)> {
        let sel = self.state.select.filter(|s| s.moved)?;
        let (a, b) = ordered(&sel);
        if i < a.row || i > b.row {
            return None;
        }
        let text = ui::seg_text(&self.segs_of(rows.list.get(i)?));
        let start = if i == a.row { a.at } else { 0 };
        let end = if i == b.row { char_end(&text, b.at) } else { text.len() };
        let next = rows.list.get(i + 1);
        let line_end = next.is_none_or(|n| n.src < 0 || n.first);
        Some((start, end, line_end && (i < b.row || b.at >= text.len())))
    }

    /// The selected text: wrapped rows of a line joined again, rendered rows without their left margin and trailing
    /// spaces.
    fn selected_text(&self, rows: &Rows) -> String {
        let Some(sel) = self.state.select else { return String::new() };
        let (a, b) = ordered(&sel);
        let mut text = String::new();
        for i in a.row..=b.row {
            // The rows built again shorter (a resize not yet seen): the rows left.
            let Some(r) = rows.list.get(i) else { break };
            let Some((start, end, _)) = self.selected_range(rows, i) else { continue };
            let row = ui::seg_text(&self.segs_of(r));
            let from = start.max(r.pad);
            let mut part = row.get(from..end.max(from)).unwrap_or("").to_string();
            if r.src < 0 {
                part = part.trim_end_matches(js::is_space).to_string();
            }
            if i > a.row && (r.src < 0 || r.first) {
                text.push('\n');
            }
            text += &part;
        }
        text
    }

    fn copy_selection(&mut self) {
        let rows = self.rows();
        let text = self.selected_text(&rows);
        if text.is_empty() {
            return;
        }
        let n = text.split('\n').count();
        let chars = text.chars().count();
        let done = if n > 1 {
            format!("Copied {n} lines")
        } else {
            format!("Copied {chars} character{}", if chars > 1 { "s" } else { "" })
        };
        self.act(move || Some(actions::copy(&text, &done)));
    }

    /// Ends the selection, copied if the mouse moved: on release, or later when the release came outside the pane
    /// and never reached it. A copied selection stays shown (done) until the next click or Escape.
    fn end_selection(&mut self) {
        let Some(sel) = self.state.select.filter(|s| !s.done) else { return };
        if sel.moved {
            self.copy_selection();
            if let Some(s) = &mut self.state.select {
                s.done = true;
            }
        } else {
            self.state.select = None;
        }
        self.render();
    }

    /// The word, run of spaces or punctuation character under a position: its first and last characters.
    fn word_at(&self, rows: &Rows, p: Pos) -> Option<Select> {
        let text = ui::seg_text(&self.segs_of(rows.list.get(p.row)?));
        let chars: Vec<(usize, u8)> = text
            .char_indices()
            .map(|(i, ch)| {
                (
                    i,
                    if wordy(ch) {
                        1
                    } else if js::is_space(ch) {
                        2
                    } else {
                        3
                    },
                )
            })
            .collect();
        let mut a = chars.iter().position(|&(j, _)| j == p.at)?;
        let mut b = a;
        let kind = chars[a].1;
        if kind != 3 {
            while a > 0 && chars[a - 1].1 == kind {
                a -= 1;
            }
            while b + 1 < chars.len() && chars[b + 1].1 == kind {
                b += 1;
            }
        }
        let at = |k: usize| Pos { row: p.row, at: chars[k].0 };
        Some(Select { from: at(a), to: at(b), moved: true, fixed: true, done: false })
    }

    /// All the rows of the line under a position, up to its end.
    fn line_at(&self, rows: &Rows, p: Pos) -> Option<Select> {
        let list = &rows.list;
        let (mut a, mut b) = (p.row, p.row);
        let src = list.get(p.row)?.src;
        if src >= 0 {
            while a > 0 && list[a - 1].src == src {
                a -= 1;
            }
            while b + 1 < list.len() && list[b + 1].src == src {
                b += 1;
            }
        }
        let end = ui::seg_text(&self.segs_of(&list[b])).len();
        Some(Select { from: Pos { row: a, at: 0 }, to: Pos { row: b, at: end }, moved: true, fixed: true, done: false })
    }

    // --- references

    /// The file's lines under the selection, else the highlighted line: (from, to), 1-based; None when none.
    fn selected_lines(&mut self) -> Option<(i64, i64)> {
        if self.state.diff {
            return self.diff_lines().map(|(a, b, _)| (a, b));
        }
        if let Some(sel) = self.state.select.filter(|s| s.moved) {
            let (a, b) = ordered(&sel);
            let rows = self.rows();
            let mut src: Vec<i64> = rows.list.get(a.row..=b.row)?.iter().map(|r| r.src).filter(|&i| i >= 0).collect();
            // A selection ending at the very start of a row leaves that line out, as an editor does.
            let last = &rows.list[b.row];
            if src.len() > 1 && b.at == 0 && last.first && Some(&last.src) == src.last() {
                src.pop();
            }
            return (!src.is_empty()).then(|| (src.iter().min().unwrap() + 1, src.iter().max().unwrap() + 1));
        }
        (self.state.line != 0.0).then_some((self.state.line as i64, self.state.line as i64))
    }

    /// Y: the path relative to the root, with the selected lines (src/app.js:12-14), to paste into a prompt.
    fn copy_reference(&mut self) {
        let s = &self.state;
        let rel = if s.root.is_empty() { s.file.clone() } else { paths::relative(&s.root, &s.file) };
        let rel = if paths::SEP == '/' { rel } else { rel.replace(paths::SEP, "/") };
        let at = self.selected_lines();
        let text = match at {
            Some((a, b)) if b > a => format!("{rel}:{a}-{b}"),
            Some((a, _)) => format!("{rel}:{a}"),
            None => rel,
        };
        let done = if at.is_some() { "Reference copied" } else { "Path copied" };
        self.act(move || Some(actions::copy(&text, done)));
    }
}
