//! Drawing of the preview: the display rows, the header with the tabs, the body with the find's
//! matches, the marks and the selection, the footer, then the menu and the key help over them.

use super::{Chip, Kind, Preview, Row, Rows};
use crate::core::paths;
use crate::core::scope;
use crate::core::tabs;
use crate::core::ui::{self, Seg, bg, c, fg, g, seg, text_width};
use std::collections::HashMap;
use std::rc::Rc;

/// The key help (? or f1), over the whole pane.
const HELP: [(&str, &[(&str, &str)]); 10] = [
    (
        "Scroll",
        &[
            ("↑↓ j k", "line"),
            ("pgup pgdn", "page"),
            ("space", "page down"),
            ("ctrl+u/d", "half page"),
            ("g G", "top, bottom"),
            ("← → h l", "left, right"),
        ],
    ),
    (
        "Find",
        &[
            ("/", "find in the file"),
            ("n N", "next, previous match (else mark)"),
            ("esc", "stop finding, clear the marks"),
        ],
    ),
    (
        "Diff",
        &[
            (",", "the view's options (or ⚙)"),
            ("d", "the file or its changes"),
            ("n N", "next, previous hunk"),
            ("b", "compare with: HEAD, merge base, last agent turn, a commit or range, a branch"),
            ("v", "inline, side by side"),
            ("s", "stage the lines selected (drag), else the hunk at the top (HEAD)"),
            ("u", "unstage them"),
            ("D", "discard them from the working tree"),
        ],
    ),
    (
        "Tabs",
        &[
            ("tab shift+tab", "next, previous tab"),
            ("ctrl+pgdn/pgup", "next, previous tab"),
            ("t", "keep the tab (in italics: the next file clicked replaces it)"),
            ("x", "close the tab (middle click)"),
        ],
    ),
    (
        "Changes",
        &[
            ("]  [", "next, previous block changed since HEAD"),
            ("c", "show the changes beside the lines and on the scrollbar"),
        ],
    ),
    (
        "View",
        &[
            ("w", "wrap"),
            ("L", "line numbers"),
            ("m", "Markdown source"),
            ("p", "image sharp or pixelated"),
            ("z", "zoom the pane"),
            ("< >", "narrower, wider"),
            ("shift+← →", "move the border"),
            ("r", "reload"),
        ],
    ),
    (
        "Edit",
        &[
            ("i", "edit the file here (or the View menu)"),
            ("ctrl+s", "save"),
            ("esc", "stop editing"),
            ("ctrl+z ctrl+y", "undo, redo (or ctrl+shift+z)"),
            ("ctrl+f", "find: ⏎ ↓ next, ↑ previous"),
            ("ctrl+r", "replace: ⏎ one, alt+a all, tab to the other field"),
            ("ctrl+g", "go to line"),
            ("ctrl+c ctrl+x ctrl+v", "copy, cut, paste"),
            ("ctrl+a", "select all"),
            ("shift+arrows", "select (or drag)"),
            ("ctrl+← →", "word"),
            ("tab shift+tab", "indent, outdent"),
            ("alt+r alt+k", "file changed on disk: reload, keep yours"),
        ],
    ),
    (
        "File",
        &[
            ("e", "open in the editor"),
            ("E", "edit in the terminal"),
            ("o", "open in its app (else the editor)"),
            ("y", "copy path"),
            ("Y", "copy relative path, with the selected lines"),
            ("H", "history of the file"),
        ],
    ),
    (
        "Review",
        &[
            ("a", "note on the selected lines (or the file)"),
            ("tab", "kind of note, while typing"),
            ("A", "send the notes to the agent"),
            ("C", "copy the notes"),
            ("U", "remove the last note"),
        ],
    ),
    ("Pane", &[("ctrl+p", "go to file"), ("ctrl+f", "search in files"), ("? f1", "keys"), ("q esc", "close")]),
];

/// The columns of ⚙: the glyph and the cell after it, which a Nerd Font glyph often spills over.
const GEAR: i64 = 3;

/// The byte index of the character u of a text (its end past it), for the columns of the caller's marks.
fn byte_of(text: &str, u: i64) -> usize {
    text.char_indices().nth(u.max(0) as usize).map_or(text.len(), |(i, _)| i)
}

impl Preview {
    // --- display rows

    /// The line numbers, the first of their columns taking the changes' signs; without them, a column of its own for
    /// the changes, only when the file has some.
    pub(super) fn gutter(&self) -> usize {
        let s = &self.state;
        if !s.diff
            && s.show.is_none()
            && self.on("lineNumbers")
            && !self.markdown_view()
            && !matches!(s.kind, Kind::Binary | Kind::Image)
        {
            s.source.len().to_string().len() + 2
        } else {
            usize::from(self.changes_shown())
        }
    }

    /// Display rows for the current width: wrapped or rendered text, built again when the width changes.
    pub(super) fn rows(&mut self) -> Rc<Rows> {
        let (w, h) = self.size();
        let width = (w as i64 - self.gutter() as i64 - 1).max(1) as usize;
        // state.left is not in the key: the rows are cut there when drawn (ui::slice).
        if let Some(r) = &self.state.rows
            && r.width == width
            && r.height == h
        {
            return r.clone();
        }
        // A selection holds positions in the old rows.
        self.state.select = None;
        let s = &self.state;
        let mut list = Vec::new();
        if s.diff || s.show.is_some() {
            list = self.diff_rows(w);
        } else if s.kind == Kind::Image {
            list = self.image_rows(w, h);
        } else if self.markdown_view() {
            list = self.markdown_rows(width);
        } else if s.kind == Kind::Error {
            list.push(Row::empty(&s.info, &[("r", "reload"), ("q", "close")]));
        } else if self.on("wrap") {
            // off: where the part starts in its line, for the find's matches.
            for (i, segs) in s.source.iter().enumerate() {
                let mut off = 0;
                for (k, part) in ui::split(segs, width).into_iter().enumerate() {
                    let n = ui::seg_text(&part).len();
                    list.push(Row { segs: part, src: i as i64, first: k == 0, off, ..Row::default() });
                    off += n;
                }
            }
        } else {
            list = s
                .source
                .iter()
                .enumerate()
                .map(|(i, segs)| Row { segs: segs.clone(), src: i as i64, first: true, ..Row::default() })
                .collect();
        }
        let mut figures = Vec::new();
        for f in list.iter().filter_map(|r| r.figure.as_ref()) {
            if !figures.contains(&f.pic.id) {
                figures.push(f.pic.id);
            }
        }
        let list = self.with_notes(list, width);
        let rows = Rc::new(Rows { list, width, height: h, figures });
        self.state.rows = Some(rows.clone());
        rows
    }

    /// The first row of a line (0-based), or of the first line after it.
    pub(super) fn row_of(&mut self, src: f64) -> i64 {
        let rows = self.rows();
        rows.list.iter().position(|r| r.src as f64 >= src).map_or(0, |i| i as i64)
    }

    // --- rendering

    pub(super) fn render(&mut self) {
        if self.state.edit.is_some() && self.state.show.is_none() {
            return self.render_edit();
        }
        let (w, h) = self.size();
        let rows = self.rows();
        let list = &rows.list;
        let body_h = h as i64 - 2;
        self.state.top = self.state.top.min(list.len() as i64 - body_h).max(0);
        let mut lines = vec![self.header(w)];
        let gw = self.gutter();
        let ranges = self.match_ranges();
        let md = self.markdown_view();
        let s = &self.state;
        let current =
            s.find.as_ref().filter(|_| s.matched >= 0).and_then(|_| s.matches.get(s.matched as usize).copied());
        // Wrapped and rendered rows leave the last column free for the scrollbar.
        let marks = self.change_marks(&rows);
        let s = &self.state;
        let bw = self.scrollbar.set(list.len() as i64, body_h, s.top, &marks);
        let signs = self.changes_shown();
        // Nothing to show (an error, an image that cannot be drawn, an empty file): why, centred, and what to do.
        let none = if list.len() == 1 && list[0].empty.is_some() {
            list[0].empty.clone()
        } else if !s.diff && s.show.is_none() && matches!(s.kind, Kind::Text | Kind::Markdown) && s.text.is_empty() {
            Some((
                "Empty file".into(),
                [("i", "edit here"), ("e", "editor"), ("q", "close")].map(|(k, a)| (k.into(), a.into())).to_vec(),
            ))
        } else {
            None
        };
        if let Some((text, pairs)) = &none {
            lines.extend(ui::empty(w, body_h.max(0) as usize, text, pairs));
        }
        let top = s.top;
        let end = (list.len() as i64).min(top + body_h);
        for i in top..end {
            if none.is_some() {
                break;
            }
            let r = &list[i as usize];
            let s = &self.state;
            let mut num: Vec<Seg> = Vec::new();
            if gw > 1 {
                let n = if r.first { (r.src + 1).to_string() } else { String::new() };
                let style = if r.src >= 0 && self.noted(r.src) {
                    Self::noted_style()
                } else {
                    fg(if (r.src + 1) as f64 == s.line { c().text } else { c().dim })
                };
                num.push(seg(format!("{n:>width$} ", width = gw - 1), style));
            }
            if signs {
                let sign = self.change_sign(list, i as usize);
                num = match num.pop() {
                    Some((text, style)) => vec![sign, (text.chars().skip(1).collect(), style)],
                    None => vec![sign],
                };
            }
            // The find's matches: every one marked, the current one brighter (in rendered Markdown, matches are by
            // row).
            let (at, off) = (if md { i } else { r.src }, r.off);
            let mut segs = self.segs_of(r);
            if (md || r.src >= 0)
                && let Some(rs) = ranges.get(&(at as usize))
            {
                let shifted: Vec<(usize, usize)> =
                    rs.iter().map(|&(a, b)| (a.saturating_sub(off), b.saturating_sub(off))).collect();
                segs = ui::mark(&segs, &shifted, &bg(c().find));
                if let Some((l, a, b)) = current.filter(|m| m.0 as i64 == at) {
                    let _ = l;
                    let style = format!("{}\x1b[1m", bg(c().current));
                    segs = ui::mark(&segs, &[(a.saturating_sub(off), b.saturating_sub(off))], &style);
                }
            }
            // The caller's marks: parts of the line on the selection's colour, whole lines on the line highlight's.
            if let Some(text) = usize::try_from(r.src).ok().filter(|_| !md).and_then(|i| s.plain.get(i)) {
                let cols: Vec<(usize, usize)> = self
                    .marks_of(r.src)
                    .filter(|m| m.a != 0)
                    .map(|m| (byte_of(text, m.a - 1).saturating_sub(off), byte_of(text, m.b).saturating_sub(off)))
                    .collect();
                if !cols.is_empty() {
                    segs = ui::mark(&segs, &cols, &bg(c().select));
                }
            }
            // A selection going on past the end of the row shows one more cell, as an editor shows the line break.
            if let Some((a, b, past)) = self.selected_range(&rows, i as usize) {
                segs = ui::mark(&segs, &[(a, b)], &bg(c().select));
                if past {
                    segs.push(seg(" ", bg(c().select)));
                }
            }
            if !self.on("wrap") && r.src >= 0 {
                segs = ui::slice(&segs, self.state.left.max(0) as usize, w.saturating_sub(gw + bw));
            }
            let s = &self.state;
            let lit = r.src >= 0
                && (((r.src + 1) as f64 == s.line && s.marks.is_empty()) || self.marks_of(r.src).any(|m| m.a == 0));
            let back = if lit { Some(c().select_blur) } else { r.back };
            num.extend(segs);
            lines.push(ui::line(&num, w.saturating_sub(bw), back) + &self.scrollbar.cell(i - top));
        }
        while lines.len() < h.saturating_sub(1) {
            lines.push(ui::line(&[], w, None));
        }
        lines.push(self.footer(w));
        self.overlays(&mut lines, w, h);
        self.screen.paint(&lines);
        super::edit::show_cursor(None);
        self.place_image();
        self.place_figures();
    }

    /// The menu, the key help and a question, over the pane.
    pub(super) fn overlays(&mut self, lines: &mut Vec<String>, w: usize, h: usize) {
        if let Some(menu) = &mut self.state.menu {
            menu.draw(lines, w as i64, h as i64);
        }
        if self.state.help.is_some() {
            let groups: Vec<ui::Group> = HELP
                .iter()
                .map(|(g, items)| (g.to_string(), items.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect()))
                .collect();
            let title = paths::basename(&self.state.file, None);
            if let Some(help) = &mut self.state.help {
                ui::help(lines, w, &title, &groups, help);
            }
        }
        if let Some(q) = &mut self.state.ask {
            ui::ask(lines, w as i64, h as i64, &mut q.ask);
        }
    }

    /// The find's matches by line: [start, end) byte ranges.
    fn match_ranges(&self) -> HashMap<usize, Vec<(usize, usize)>> {
        let mut map: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for &(i, a, b) in &self.state.matches {
            map.entry(i).or_default().push((a, b));
        }
        map
    }

    // --- header

    /// Header: the tabs on the left, or with a single tab, the file's path; then the details; on the right, what the
    /// Diff view compares with, the File · Diff switch and ⚙, which opens the view's options (,). A narrow pane drops
    /// the reminder, then the switch, then the details, then the tabs out of reach of the one shown (tab_strip).
    pub(super) fn header(&mut self, w: usize) -> String {
        if self.state.show.is_some() {
            return self.show_header(w);
        }
        let wi = w as i64;
        let s = &self.state;
        let one = self.tabs.list.len() < 2;
        let t = self.tabs.list.get(self.tabs.current);
        let base = paths::basename(&s.file, None);
        let name = if s.root.is_empty() {
            base.clone()
        } else {
            let rel = paths::relative(&s.root, &s.file);
            if rel.is_empty() { base.clone() } else { rel }
        };
        let sw = (!s.file.is_empty())
            .then(|| ui::segmented(&[("file", "File"), ("diff", "Diff")], if s.diff { "diff" } else { "file" }));
        // What the Diff view compares with, as in the Changes pane: the only option that does not show in the content.
        let mut note = if s.diff { scope::label(&s.scope, &self.branch_name()) } else { String::new() };
        let sw_w = sw.as_ref().map_or(0, |x| x.1 as i64 + 1);
        let note_w = |n: &str| if n.is_empty() { 0 } else { text_width(n) as i64 + 3 };
        let room = |n: &str| wi - GEAR - sw_w - note_w(n);
        let name_room = 20.min(text_width(&format!(" {} ×  ", if one { &name } else { &base })) as i64);
        if !note.is_empty() && room(&note) < name_room {
            note.clear();
        }
        let show_switch = sw.is_some() && room(&note) >= name_room;
        let right = note_w(&note) + if show_switch { sw_w } else { 0 } + GEAR;
        let mut chips = Vec::new();
        let (mut segs, lw): (Vec<Seg>, i64);
        if one {
            // A temporary tab in italics: the next file clicked replaces it.
            let text = format!(" {name}{}", if self.unsaved_at(self.tabs.current) { " ●" } else { "" });
            let (left, fw) = ui::fit(&text, (wi - right - 1).max(1) as usize, None);
            let style = format!("\x1b[1m{}", if t.is_some_and(|t| !t.kept) { "\x1b[3m" } else { "" });
            segs = vec![seg(left, style)];
            lw = fw as i64;
            chips.push(Chip { id: "tab".into(), from: 1, to: lw, tab: Some(self.tabs.current) });
        } else {
            let (strip, sw, strip_chips) = self.tab_strip((wi - right - 1).max(1));
            segs = strip;
            lw = sw;
            chips.extend(strip_chips);
        }
        // The details whole, else their first part (the lines), else none; in the Diff view, the added and removed
        // lines.
        let s = &self.state;
        let details = match (self.edit_info(), &s.diff_data) {
            (Some(info), _) => info,
            (None, Some(super::diff::DiffData::Done { parsed, .. })) if s.diff => {
                format!("+{} −{}", parsed.add, parsed.del)
            }
            (None, _) if s.diff => String::new(),
            (None, _) => s.info.clone(),
        };
        let first = details.split(" · ").next().unwrap_or("").to_string();
        let info = [format!("  {details}"), format!("  {first}"), String::new()]
            .into_iter()
            .find(|t| t.is_empty() || (text_width(t) as i64) < wi - right - lw)
            .unwrap_or_default();
        let iw = text_width(&info) as i64;
        segs.push(seg(info, fg(c().dim)));
        segs.push(seg(" ".repeat((wi - right - lw - iw).max(0) as usize), ""));
        let mut x = wi - right + 1;
        if !note.is_empty() {
            let (chip, cw, _) = ui::segmented(&[("scope", &note)], "scope");
            segs.extend(chip);
            segs.push(seg(" ", ""));
            chips.push(Chip { id: "scope".into(), from: x, to: x + cw as i64 - 1, tab: None });
            x += cw as i64 + 1;
        }
        if show_switch && let Some((ssegs, sww, hits)) = sw {
            segs.extend(ssegs);
            segs.push(seg(" ", ""));
            for hit in hits {
                chips.push(Chip { id: hit.id, from: x + hit.from as i64, to: x + hit.to as i64, tab: None });
            }
            x += sww as i64 + 1;
        }
        segs.push(seg(format!(" {} ", g().gear), fg(if self.state.menu.is_some() { c().text } else { c().dim })));
        chips.push(Chip { id: "menu".into(), from: x, to: x + GEAR - 1, tab: None });
        self.state.chips = chips;
        ui::line(&segs, w, Some(c().header))
    }

    /// The tabs in room columns (tabs::strip), from the first column: each one " name × ", the one shown brighter on a
    /// lighter background, a temporary one in italics; ‹ and › where tabs are hidden, which show the nearest of them.
    /// The strip moves only when the tab shown would leave it (strip_first). Returns the segments, their width and
    /// the chips.
    fn tab_strip(&mut self, room: i64) -> (Vec<Seg>, i64, Vec<Chip>) {
        let files: Vec<String> = self.tabs.list.iter().map(|t| t.file.clone()).collect();
        let names: Vec<String> = tabs::labels(&files).iter().map(|n| ui::clean_line(n, 4, None)).collect();
        let widths: Vec<i64> = names.iter().map(|n| text_width(n) as i64 + 4).collect();
        let st = tabs::strip(&widths, self.tabs.current as i64, room, self.strip_first);
        self.strip_first = st.first;
        let (mut segs, mut chips, mut x) = (Vec::new(), Vec::new(), 1);
        let mut put = |text: String, style: String, chip: Option<(&str, usize)>| {
            let w = text_width(&text) as i64;
            if let Some((id, tab)) = chip {
                chips.push(Chip { id: id.into(), from: x, to: x + w - 1, tab: Some(tab) });
            }
            segs.push(seg(text, style));
            x += w;
        };
        if st.first > 0 {
            put("‹ ".into(), fg(c().dim), Some(("tab", st.first as usize - 1)));
        }
        for i in st.first..=st.last {
            let i = i as usize;
            let on = i == self.tabs.current;
            let back = if on { bg(c().select_blur) } else { String::new() };
            let name = if on && st.cut > 0 {
                ui::ellipsis(&names[i], (text_width(&names[i]) as i64 - st.cut).max(1))
            } else {
                names[i].clone()
            };
            let style = format!(
                "{back}{}{}{}",
                fg(if on { c().text } else { c().dim }),
                if on { "\x1b[1m" } else { "" },
                if self.tabs.list[i].kept { "" } else { "\x1b[3m" }
            );
            put(format!(" {name} "), style, Some(("tab", i)));
            // ● for unsaved edits, as VS Code: the click still closes, asking first.
            let close = if self.unsaved_at(i) { "● " } else { "× " };
            put(close.into(), format!("{back}{}", fg(c().dim)), Some(("close", i)));
        }
        if st.last < self.tabs.list.len() as i64 - 1 {
            put(" ›".into(), fg(c().dim), Some(("tab", st.last as usize + 1)));
        }
        (segs, x - 1, chips)
    }

    // --- footer

    fn footer(&mut self, w: usize) -> String {
        // The keys shown, for clicks: none while the find or a message takes the line.
        self.state.hints = None;
        if let Some(line) = self.rev_footer(w) {
            return line;
        }
        let s = &self.state;
        if let Some(f) = s.find.as_ref().filter(|f| f.active) {
            let count = if f.field.value.is_empty() {
                String::new()
            } else {
                format!(" {}/{}", if s.matches.is_empty() { 0 } else { s.matched + 1 }, s.matches.len())
            };
            let mut segs = vec![seg(format!(" {} ", g().search), fg(c().dim))];
            segs.extend(f.field.segments(w as i64 - 4 - count.len() as i64, ""));
            segs.push(seg(count, fg(c().dim)));
            return ui::line(&segs, w, Some(c().header));
        }
        if let Some(line) = self.note_footer(w) {
            return line;
        }
        let s = &self.state;
        if let Some(text) = &s.flash {
            return ui::line(&ui::message(text, None), w, Some(c().header));
        }
        let len = self.rows().list.len() as i64;
        let body_h = self.size().1 as i64 - 2;
        let s = &self.state;
        let pos = if len > body_h {
            format!(" {}%", (100.0 * (1f64).min((s.top + body_h) as f64 / len as f64)).round())
        } else {
            String::new()
        };
        let all = s.note_count;
        let send: Vec<(String, String)> = if all > 0 {
            vec![("A".into(), format!("send {all} note{}", if all > 1 { "s" } else { "" }))]
        } else {
            vec![]
        };
        let n = s.marks.len();
        let mut marks: Vec<(String, String)> = send.clone();
        if n > 1 {
            marks.push(("n N".into(), format!("{n} marks")));
        }
        if n > 0 {
            marks.push(("esc".into(), "clear".into()));
        }
        let pair = |k: &str, a: &str| (k.to_string(), a.to_string());
        let mut pairs = marks;
        if s.show.is_some() {
            pairs = self.show_pairs();
        } else if s.kind == Kind::Image && !s.diff {
            let sharp = if self.sharp() { "pixelated" } else { "sharp" };
            pairs = send;
            pairs.extend([
                pair("p", sharp),
                pair("o", "open"),
                pair("y", "copy path"),
                pair("q", "close"),
                pair("?", "keys"),
            ]);
        } else if s.diff {
            if n <= 1 {
                pairs.push(pair("n N", "hunks"));
            }
            pairs.extend([pair("a", "note"), pair("z", "zoom"), pair("q", "close"), pair("?", "keys")]);
        } else {
            pairs.extend([
                pair("/", "find"),
                pair("a", "note"),
                pair("e", "editor"),
                pair("o", "app"),
                pair("z", "zoom"),
                pair("q", "close"),
                pair("?", "keys"),
            ]);
        }
        let keys = ui::keys(&pairs, w.saturating_sub(pos.len() + 1));
        let kw = ui::seg_width(&keys.segs);
        let mut segs = keys.segs.clone();
        segs.push(seg(format!("{pos:>width$}", width = w.saturating_sub(kw)), fg(c().dim)));
        self.state.hints = Some(keys);
        ui::line(&segs, w, Some(c().header))
    }
}
