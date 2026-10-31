//! The search's frame: tabs, the query and its options, the globs, the status, the results with the file on the right,
//! the keys.

use super::find::{self, MAX_FILES, MAX_MATCHES, MAX_ROWS};
use super::{Act, CONTENT, EXCLUDE, FILES, Focus, INCLUDE, Mode, Opt, Row, SField, Search, Shown, Target, Zone};
use crate::core::paths;
use crate::core::ui::{self, Rgb, Seg, bg, c, ellipsis, fg, line, seg, text_width};
use std::collections::{HashMap, HashSet};

/// A part of a line: its text, its style, what a click on it does.
type Part = (String, String, Option<Act>);

fn part(text: impl Into<String>, style: impl Into<String>, act: Option<Act>) -> Part {
    (text.into(), style.into(), act)
}

fn width(parts: &[Part]) -> i64 {
    parts.iter().map(|(t, _, _)| text_width(t) as i64).sum()
}

fn dim() -> String {
    fg(c().dim)
}

/// Fuzzy-matched characters.
fn hi() -> String {
    format!("\x1b[1m{}", fg(c().hit))
}

/// Text matches.
fn found_style() -> String {
    format!("{}{}", bg(c().found), fg(c().strong))
}

/// An option: blue when on, grey when off.
fn chip(label: &str, on: bool, o: Opt) -> Part {
    let style =
        if on { format!("{}{}", bg(c().select), fg(c().strong)) } else { format!("{}{}", bg(c().button_off), dim()) };
    part(format!(" {label} "), style, Some(Act::Toggle(o)))
}

/// The visible part of a field, the cursor shown only in the focused one; restored text shown selected: typing
/// replaces it.
fn field_parts(f: &SField, w: i64, focus: bool) -> Vec<Seg> {
    let style = if f.restored { bg(c().select) } else { String::new() };
    if !focus {
        return vec![seg(ellipsis(&f.field.value, w), if f.restored { String::new() } else { fg(c().text) })];
    }
    f.field.segments(w, &style)
}

fn is_blank(c: char) -> bool {
    crate::unicode::is_space(c) || crate::core::regex::is_line_terminator(c)
}

/// Segments of a text with byte ranges highlighted, tabs expanded and control characters dropped.
fn marked(text: &str, found: &[(usize, usize)], style: &str, hi: &str, tab: usize) -> Vec<Seg> {
    let mut out = Vec::new();
    let (mut cur, mut on, mut col, mut r) = (String::new(), false, 0, 0);
    for (i, ch) in text.char_indices() {
        while r < found.len() && i >= found[r].1 {
            r += 1;
        }
        let inside = r < found.len() && i >= found[r].0;
        if inside != on {
            if !cur.is_empty() {
                out.push(seg(std::mem::take(&mut cur), if on { hi } else { style }));
            }
            on = inside;
        }
        if ch == '\t' {
            let n = tab - col % tab;
            cur.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else if !ui::is_control(ch) {
            cur.push(ch);
            col += 1;
        }
    }
    if !cur.is_empty() {
        out.push(seg(cur, if on { hi } else { style }));
    }
    out
}

/// The ranges [start, end) of a line's text moved to its cleaned text (tabs expanded, control characters dropped).
fn clean_ranges(text: &str, found: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if found.is_empty() || !text.chars().any(ui::is_control) {
        return found;
    }
    let mut at = Vec::new();
    ui::clean_line(text, 4, Some(&mut at));
    found.into_iter().map(|(a, b)| (at[a], at[b])).collect()
}

/// The key help (f1: ? is typed in the fields), over the whole pane.
fn help_groups() -> Vec<ui::Group> {
    let group = |title: &str, pairs: &[(&str, &str)]| -> ui::Group {
        (title.into(), pairs.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect())
    };
    vec![
        group(
            "Results",
            &[
                ("↑↓", "select"),
                ("ctrl+n", "next"),
                ("ctrl+k", "previous"),
                ("pgup pgdn", "page"),
                ("ctrl+home", "first"),
                ("ctrl+end", "last"),
                ("alt+← →", "fold, unfold a file"),
            ],
        ),
        group(
            "Open",
            &[
                ("⏎", "in the preview"),
                ("ctrl+e", "in the editor"),
                ("alt+⏎", "in the editor"),
                ("ctrl+o", "in its app (else the editor)"),
                ("ctrl+t", "insert in terminal"),
                ("ctrl+y", "copy the relative path"),
            ],
        ),
        group(
            "Search",
            &[
                ("ctrl+p", "go to file"),
                ("ctrl+f", "search in files"),
                ("tab", "next field"),
                ("alt+c", "match case"),
                ("alt+w", "whole word"),
                ("alt+r", "regular expression"),
                ("alt+h", "hidden files"),
            ],
        ),
        group("Pane", &[("f1", "keys"), ("esc", "close")]),
    ]
}

impl Search {
    /// A screen line whose parts are clickable.
    fn bar(&mut self, y: i64, parts: Vec<Part>, w: usize, back: Option<Rgb>) -> String {
        let mut x = 1;
        for (text, _, act) in &parts {
            let tw = text_width(text) as i64;
            if let Some(act) = act
                && tw > 0
            {
                self.zones.push(Zone { y, x1: x, x2: x + tw - 1, act: *act });
            }
            x += tw;
        }
        let segs: Vec<Seg> = parts.into_iter().map(|(t, s, _)| (t, s)).collect();
        line(&segs, w, back)
    }

    fn file_row(&mut self, k: usize, w: i64) -> Vec<Seg> {
        let s = &mut self.state;
        let Some(files) = &s.files else { return Vec::new() };
        let r = &mut s.results[k];
        let rel = files.list[r.i].as_str();
        if !s.fields[FILES].field.value.is_empty() && r.pos.is_none() {
            let (q, _) = find::parse_query(&s.fields[FILES].field.value);
            let (qc, ql) = find::query_chars(&q);
            r.pos = Some(if q.is_empty() {
                Vec::new()
            } else {
                find::fuzzy(&qc, &ql, rel, &files.lower[r.i], files.base[r.i], true).map(|f| f.1).unwrap_or_default()
            });
        }
        let pos: HashSet<usize> = r.pos.iter().flatten().copied().collect();
        let b = files.base[r.i];
        let (hi, dim) = (hi(), dim());
        // The bytes [from, to) of the path, the matched characters highlighted.
        let part = |from: usize, to: usize, style: &str| -> Vec<Seg> {
            let mut segs: Vec<Seg> = Vec::new();
            for (i, ch) in rel[from..to].char_indices() {
                let st = if pos.contains(&(from + i)) { hi.as_str() } else { style };
                match segs.last_mut() {
                    Some(last) if last.1 == st => last.0.push(ch),
                    _ => segs.push(seg(ch.to_string(), st)),
                }
            }
            segs
        };
        // The name, and its folder on the right (cut from the left when long).
        let name = part(b, rel.len(), "");
        let nw = ui::seg_width(&name) as i64;
        let mut dir = part(0, b.saturating_sub(1), &dim);
        let mut dw = ui::seg_width(&dir) as i64;
        let room = w - 1 - nw - 3;
        if dw > room {
            dir = if room > 1 {
                let mut d = vec![seg("…", dim.clone())];
                d.extend(ui::slice(&dir, (dw - room + 1) as usize, (room - 1) as usize));
                d
            } else {
                Vec::new()
            };
            dw = ui::seg_width(&dir) as i64;
        }
        let mut out = vec![seg(" ", "")];
        out.extend(name);
        out.push(seg(" ".repeat((w - 1 - nw - dw - 1).max(2) as usize), ""));
        out.extend(dir);
        out.push(seg(" ", ""));
        out
    }

    fn content_row(&self, r: Row, w: i64) -> Vec<Seg> {
        let dim = dim();
        let (gi, mi) = match r {
            Row::Head(gi) => (gi, None),
            Row::Line(gi, mi) => (gi, Some(mi)),
            Row::File(_) => return Vec::new(),
        };
        let g = &self.state.groups[gi];
        let Some(mi) = mi else {
            let b = g.rel.rfind('/').map_or(0, |i| i + 1);
            let left = vec![
                seg(if g.collapsed { " ▸ " } else { " ▾ " }, dim.clone()),
                seg(&g.rel[b..], "\x1b[1m"),
                seg(format!("  {}", &g.rel[..b.saturating_sub(1)]), dim.clone()),
            ];
            let count = format!(" {} ", g.count);
            let pad = (w - ui::seg_width(&left) as i64 - count.len() as i64).max(1) as usize;
            let mut out = left;
            out.push(seg(" ".repeat(pad), ""));
            out.push(seg(count, dim));
            return out;
        };
        let m = &g.matches[mi];
        let num = format!("   {:>5}  ", m.n);
        // Leading blanks removed, then the start cut so that the first match shows within the room left.
        let mut text = m.text.trim_start_matches(is_blank);
        let lead = m.text.len() - text.len();
        let mut found: Vec<(usize, usize)> =
            m.ranges.iter().map(|&(a, b)| (a.saturating_sub(lead), b.saturating_sub(lead))).collect();
        let mut cut = m.cut;
        let room = w - num.len() as i64 - 1;
        // The first match starts about a third into the room, its start moved by whole characters.
        if let Some(&(a, b)) = found.first()
            && text_width(&text[..b]) as i64 > room - 8
        {
            let back = (room as f64 * 0.3).floor() as i64;
            let first = text[..a].chars().count() as i64 - back;
            if first > 0 {
                let at = find::char_at(text, first as usize);
                text = &text[at..];
                found = found.iter().map(|&(a, b)| (a.saturating_sub(at), b.saturating_sub(at))).collect();
                cut = true;
            }
        }
        let mut out = vec![seg(num, dim.clone())];
        if cut {
            out.push(seg("…", dim));
        }
        out.extend(marked(text, &found, "", &found_style(), 1));
        out
    }

    /// The file around the target's line, the line about a third down; long lines wrapped when the preview pane wraps
    /// them.
    fn preview_lines(&mut self, t: Option<Target>, w: usize, h: i64) -> Vec<String> {
        let mut out = Vec::new();
        let Some(t) = t else { return out };
        let at_line = if t.line != 0.0 { format!(":{}", crate::json::number(t.line)) } else { String::new() };
        out.push(line(&[seg(format!(" {}{at_line}", t.rel), "\x1b[1m")], w, Some(c().header)));
        if self.preview(&t.rel).is_none() {
            return out;
        }
        let Some((_, Some(p))) = self.previews.iter().find(|(r, _)| *r == t.rel) else { return out };
        let (lines, segs) = match p {
            Shown::Error(e) => {
                out.push(line(&[seg(format!(" ({e})"), dim())], w, None));
                return out;
            }
            Shown::Text { lines, segs } => (lines, segs),
        };
        let h = h - 1;
        let n = lines.len();
        let at = ((if t.line != 0.0 { t.line } else { 1.0 }) - 1.0).min(n as f64 - 1.0).max(0.0) as usize;
        let digits = n.to_string().len();
        let room = (w as i64 - digits as i64 - 2).max(1) as usize;
        let re = if self.state.mode == Mode::Content { self.state.re.as_ref() } else { None };
        let (wrap, found_style) = (self.wrap, found_style());
        // The screen rows of line i: its segments with the matches marked, split when wrapping.
        let mut cut: HashMap<usize, Vec<Vec<Seg>>> = HashMap::new();
        let mut rows_of = |i: usize| -> i64 {
            cut.entry(i)
                .or_insert_with(|| {
                    let found = re.map_or(Vec::new(), |re| clean_ranges(&lines[i], find::ranges(re, &lines[i], 50)));
                    let marked = ui::mark(&segs[i], &found, &found_style);
                    if wrap { ui::split(&marked, room) } else { vec![marked] }
                })
                .len() as i64
        };
        // From the target's line, the lines above it up to a third of the height, then those below, then more above at
        // the end.
        let first = if t.line != 0.0 { at } else { 0 };
        let (mut from, mut to) = (first, first);
        let mut used = rows_of(first);
        while from > 0 && used + rows_of(from - 1) <= h.div_euclid(3) {
            from -= 1;
            used += rows_of(from);
        }
        while to + 1 < n && used < h {
            to += 1;
            used += rows_of(to);
        }
        while from > 0 && used + rows_of(from - 1) <= h {
            from -= 1;
            used += rows_of(from);
        }
        for i in from..=to {
            if out.len() as i64 > h {
                break;
            }
            let current = t.line != 0.0 && i == at;
            for (k, segs) in cut[&i].iter().enumerate() {
                if out.len() as i64 > h {
                    break;
                }
                let n = if k > 0 { " ".repeat(digits) } else { format!("{:>digits$}", i + 1) };
                let mut all = vec![seg(format!(" {n} "), if current { fg(c().text) } else { dim() })];
                all.extend(segs.iter().cloned());
                out.push(line(&all, w, current.then(|| c().select_blur)));
            }
        }
        out
    }

    pub(super) fn render(&mut self) {
        self.paint_at = None;
        let (wu, hu) = self.size();
        let (w, h_all) = (wu as i64, hu as i64);
        self.zones.clear();
        let mut lines = Vec::new();
        let content = self.state.mode == Mode::Content;
        let dim = dim();

        // Tabs, and the space folder on the right.
        let mode = self.state.mode;
        let tab = |label: &str, m: Mode| {
            let style = if mode == m { format!("\x1b[1m{}{}", bg(c().select), fg(c().strong)) } else { dim.clone() };
            part(format!(" {label} "), style, Some(Act::Mode(m)))
        };
        let mut tabs = vec![
            part(" ", "", None),
            tab("Go to file", Mode::Files),
            part(" ", "", None),
            tab("Search in files", Mode::Content),
        ];
        let tw = width(&tabs);
        let name = ellipsis(&paths::basename(&self.root, None), (w - tw - 2).max(0));
        tabs.push(part(" ".repeat((w - tw - text_width(&name) as i64 - 1).max(0) as usize), "", None));
        tabs.push(part(name, dim.clone(), None));
        let l = self.bar(0, tabs, wu, Some(c().header));
        lines.push(l);

        // Query and option chips.
        let s = &self.state;
        let mut chips = if content {
            vec![
                chip("Aa", s.case, Opt::Case),
                chip("ab", s.word, Opt::Word),
                chip(".*", s.regex, Opt::Regex),
                part(" ", "", None),
            ]
        } else {
            Vec::new()
        };
        chips.push(chip("hidden", s.hidden, Opt::Hidden));
        chips.push(part(" ", "", None));
        // Go to file: the count on the right of the field.
        let query = s.fields[FILES].field.value.clone();
        let n_results = s.results.len();
        let of = move |n: usize| {
            if query.is_empty() { String::new() } else { format!("{n}{} of ", if n >= MAX_ROWS { "+" } else { "" }) }
        };
        let count = match &s.files {
            Some(f) if !content => part(format!("{}{}  ", of(n_results), f.list.len()), dim.clone(), None),
            _ => part("", "", None),
        };
        let fq = Some(Act::Focus(Focus::Query));
        let prompt = part(format!(" {} ", ui::g().search), dim.clone(), fq);
        let cw = width(std::slice::from_ref(&count));
        let qw = (w - 3 - width(&chips) - cw - 1).max(1);
        let qparts: Vec<Part> = field_parts(&s.fields[self.query_index()], qw, !content || s.focus == Focus::Query)
            .into_iter()
            .map(|(t, st)| (t, st, fq))
            .collect();
        let pad = (w - 3 - width(&qparts) - width(&chips) - cw).max(0) as usize;
        let mut parts = vec![prompt];
        parts.extend(qparts);
        parts.push(part(" ".repeat(pad), "", fq));
        parts.push(count);
        parts.extend(chips);
        let l = self.bar(1, parts, wu, None);
        lines.push(l);

        // Include and exclude globs.
        if content {
            let half = w / 2;
            // Each glob a field on a lighter background, after its label.
            let s = &self.state;
            let glob_field = |label: &str, k: usize, f: Focus, gw: i64| -> Vec<Part> {
                let act = Some(Act::Focus(f));
                let field_bg = bg(c().field);
                let fw = (gw - text_width(label) as i64 - 2).max(1);
                let parts: Vec<Part> = field_parts(&s.fields[k], fw - 1, s.focus == f)
                    .into_iter()
                    .map(|(t, st)| (t, format!("{field_bg}{st}"), act))
                    .collect();
                let pad = (fw - 1 - width(&parts)).max(0) as usize;
                let mut out = vec![part(label, dim.clone(), act), part(" ", field_bg.clone(), act)];
                out.extend(parts);
                out.push(part(" ".repeat(pad), field_bg, act));
                out.push(part(" ", "", None));
                out
            };
            let mut parts = glob_field("   in ", INCLUDE, Focus::Include, half);
            parts.extend(glob_field("  not in ", EXCLUDE, Focus::Exclude, w - half));
            let l = self.bar(2, parts, wu, None);
            lines.push(l);
        }

        // Status.
        let s = &self.state;
        let (mut status, style) = match &s.files {
            None => ("indexing…".to_string(), dim.clone()),
            Some(_) if content && s.error.is_some() => (s.error.clone().unwrap_or_default(), fg(c().red)),
            Some(_) if content => {
                let (n, m) = (s.total, s.groups.len());
                let mut status = if s.fields[CONTENT].field.value.is_empty() {
                    "type to search".into()
                } else {
                    format!(
                        "{n}{} result{} in {m} file{}",
                        if n >= MAX_MATCHES { "+" } else { "" },
                        if n == 1 { "" } else { "s" },
                        if m == 1 { "" } else { "s" }
                    )
                };
                if s.searching {
                    status += "  searching…";
                }
                (status, dim.clone())
            }
            Some(f) => {
                let n = f.list.len();
                (format!("{}{n} file{}", of(n_results), if n == 1 { "" } else { "s" }), dim.clone())
            }
        };
        if s.capped && s.flash.is_none() {
            status += &format!("  (first {MAX_FILES} files only)");
        }
        // A rule above the results, with the status in it; Go to file has its count beside the field.
        let said = match &s.flash {
            Some(f) => ui::message(f, None),
            None if content || s.files.is_none() => vec![seg(status, style)],
            None if s.capped => vec![seg(format!("first {MAX_FILES} files only"), dim.clone())],
            None => Vec::new(),
        };
        let mut head = Vec::new();
        if !said.is_empty() {
            head.push(seg("── ", dim.clone()));
            head.extend(said);
            head.push(seg(" ", ""));
        }
        let rule = (w - ui::seg_width(&head) as i64).max(0) as usize;
        head.push(seg("─".repeat(rule), dim.clone()));
        lines.push(line(&head, wu, None));

        // Results, and the preview on the right.
        let list = self.rows();
        let h = self.list_height();
        let len = list.len() as i64;
        let s = &mut self.state;
        if content && !s.touched && len > 1 && s.cursor == 0 {
            s.cursor = 1;
        }
        s.cursor = s.cursor.min(len - 1).max(0);
        s.top = s.top.min(len - h).max(0);
        // In a tab of its own, the result shows in the tab's preview (follow), not on the right of the list.
        let lw = if self.origin.is_empty() && w >= 100 { w / 2 } else { w };
        self.list_w = lw;
        let right = if lw < w {
            let t = self.target(self.current());
            self.preview_lines(t, (w - lw - 1) as usize, h)
        } else {
            Vec::new()
        };
        let (top, cursor) = (self.state.top, self.state.cursor);
        let bw = self.scrollbar.set(len, h, top, &[]) as i64;
        let rw = (lw - bw) as usize;
        // Nothing to list: what to type, or what to try instead.
        let s = &self.state;
        let none = if s.files.is_none() || !list.is_empty() {
            None
        } else if content && s.fields[CONTENT].field.value.is_empty() {
            Some(ui::empty::<&str, &str>(rw, h as usize, "Type to search", &[]))
        } else if content && s.searching {
            // Not every file read yet: no verdict.
            None
        } else {
            let pairs: &[(&str, &str)] = if !content {
                &[("ctrl+f", "search in files")]
            } else if s.hidden {
                &[("ctrl+p", "go to file")]
            } else {
                &[("alt+h", "include hidden files")]
            };
            Some(ui::empty(rw, h as usize, "No results", pairs))
        };
        for k in 0..h {
            let i = top + k;
            let r = usize::try_from(i).ok().and_then(|i| list.get(i).copied());
            let mut l = match r {
                Some(r) => {
                    let segs = match r {
                        Row::File(k) => self.file_row(k, rw as i64),
                        _ => self.content_row(r, rw as i64),
                    };
                    line(&segs, rw, (i == cursor).then(|| c().select))
                }
                None => line(&[], rw, None),
            };
            if let Some(none) = &none {
                l = none[k as usize].clone();
            }
            l += &self.scrollbar.cell(k);
            if lw < w {
                l += &format!("{dim}│\x1b[0m");
                l += &right.get(k as usize).cloned().unwrap_or_else(|| line(&[], (w - lw - 1) as usize, None));
            }
            lines.push(l);
        }

        // Keys.
        let mut keys = vec![
            ("↵", "open"),
            if content { ("ctrl+p", "go to file") } else { ("ctrl+f", "search in files") },
            ("ctrl+e", "editor"),
            ("ctrl+o", "app"),
            ("ctrl+t", "insert"),
            ("ctrl+y", "copy relative path"),
        ];
        if content {
            keys.extend([("tab", "fields"), ("alt+←→", "fold")]);
        }
        keys.extend([("esc", "close"), ("f1", "keys")]);
        let body = (h_all - 1).max(0) as usize;
        while lines.len() < body {
            lines.push(line(&[], wu, None));
        }
        lines.truncate(body);
        self.state.hints = ui::keys(&keys, wu);
        lines.push(line(&self.state.hints.segs, wu, Some(c().header)));
        if let Some(help) = &mut self.state.help {
            let title = if content { "Search in files" } else { "Go to file" };
            ui::help(&mut lines, wu, title, &help_groups(), help, crate::core::update::BUILD);
        }
        lines.truncate(hu);
        self.screen.paint(&lines);
        if !self.origin.is_empty() {
            self.follow();
        }
    }
}
