//! A parsed diff laid out in rows (src/diff.js): the commit's header and stat, a row per file and per hunk, then the
//! lines, inline or side by side, the changed words on a stronger background; long lines wrapped or cut from a column.

use super::{Diff, File, Line};
use crate::core::exec;
use crate::core::ui::{self, Rgb, Seg, bg, c, fg, seg};
use crate::core::{js, regex::Regex};
use crate::json::Value;
use crate::unicode;
use std::rc::Rc;

/// Beyond, a huge diff would take seconds to lay out at every resize.
pub const MAX_ROWS: usize = 200_000;
/// Columns from which side by side is the default.
pub const SPLIT_MIN: usize = 120;

/// What a row shows: the path as git gives it, lines of the new version (0 for the whole file), the section of its
/// file ("" for none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ref {
    pub file: String,
    pub from: i64,
    pub to: i64,
    pub sec: String,
}

/// The row made again from another column (←, →), rather than the whole diff.
pub type Redo = Rc<dyn Fn(usize) -> Vec<Seg>>;

/// A row: segs and back for ui::line; hunk, a row n and N jump to; file, a file's name; more, a wrapped line's next
/// row; place, the line of the new version it stands at (0 for none); what it shows for core::stage: f, the file on
/// its name's row, h, the hunk on its @@ row, lines, the hunk's lines on theirs (two on a side by side row), as
/// indexes (file, hunk, line) in the diff.
#[derive(Clone, Default)]
pub struct Row {
    pub segs: Vec<Seg>,
    pub back: Option<Rgb>,
    pub hunk: bool,
    pub file: bool,
    pub more: bool,
    pub place: i64,
    pub r#ref: Option<Ref>,
    pub f: Option<usize>,
    pub h: Option<(usize, usize)>,
    pub lines: Vec<(usize, usize, usize)>,
    pub redo: Option<Redo>,
}

impl std::fmt::Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Row").field("segs", &self.segs).field("ref", &self.r#ref).finish_non_exhaustive()
    }
}

/// How rows are laid out: long lines wrapped, else cut from column left of the text; viewed(file): the file shows
/// folded, its name's row alone.
#[derive(Default)]
pub struct View<'a> {
    pub wrap: bool,
    pub left: usize,
    pub viewed: Option<&'a dyn Fn(&File) -> bool>,
}

/// A part of a line, and whether it changed.
type Span = (String, bool);

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

/// The tokens a line is compared by: runs of letters, digits and _, runs of white space, any other character alone
/// (`/[\p{L}\p{N}_]+|\s+|[^\p{L}\p{N}_\s]/gu`).
fn tokens(s: &str) -> Vec<&str> {
    let word = |ch: char| unicode::is_letter(ch) || unicode::is_number(ch) || ch == '_';
    let kind = |ch: char| {
        if word(ch) {
            1
        } else if js::is_space(ch) {
            2
        } else {
            3
        }
    };
    let mut out = Vec::new();
    let mut start = None::<(usize, u8)>;
    for (i, ch) in s.char_indices() {
        let k = kind(ch);
        match start {
            Some((_, sk)) if sk == k && k != 3 => {}
            Some((a, _)) => {
                out.push(&s[a..i]);
                start = Some((i, k));
            }
            None => start = Some((i, k)),
        }
    }
    if let Some((a, _)) = start {
        out.push(&s[a..]);
    }
    out
}

/// JS's trim: white space and line terminators at both ends.
fn trimmed_len(s: &str) -> usize {
    js::utf16_len(s.trim_matches(js::is_space))
}

/// Changed words of a removed line and its added counterpart: spans for each, or None when the lines have little in
/// common (the whole row is then the change).
fn words(a: &str, b: &str) -> Option<(Vec<Span>, Vec<Span>)> {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() || tb.is_empty() || ta.len() * tb.len() > 40000 {
        return None;
    }
    let cols = tb.len() + 1;
    let mut dp = vec![0u16; (ta.len() + 1) * cols];
    for i in (0..ta.len()).rev() {
        for j in (0..tb.len()).rev() {
            dp[i * cols + j] = if ta[i] == tb[j] {
                dp[(i + 1) * cols + j + 1].wrapping_add(1)
            } else {
                dp[(i + 1) * cols + j].max(dp[i * cols + j + 1])
            };
        }
    }
    let push = |spans: &mut Vec<Span>, text: &str, changed: bool| match spans.last_mut() {
        Some(last) if last.1 == changed => last.0.push_str(text),
        _ => spans.push((text.to_string(), changed)),
    };
    let (mut sa, mut sb) = (Vec::new(), Vec::new());
    let (mut i, mut j, mut same) = (0, 0, 0);
    while i < ta.len() || j < tb.len() {
        if i < ta.len() && j < tb.len() && ta[i] == tb[j] {
            push(&mut sa, ta[i], false);
            push(&mut sb, tb[j], false);
            same += trimmed_len(ta[i]);
            i += 1;
            j += 1;
        } else if j >= tb.len() || (i < ta.len() && dp[(i + 1) * cols + j] >= dp[i * cols + j + 1]) {
            push(&mut sa, ta[i], true);
            i += 1;
        } else {
            push(&mut sb, tb[j], true);
            j += 1;
        }
    }
    let longest = trimmed_len(a).max(trimmed_len(b));
    if (same as f64) < 0.4 * longest as f64 { None } else { Some((sa, sb)) }
}

/// Rows of segments filling exactly width columns each: spans on the base background, changed parts on the strong
/// one; wrapped on as many rows as needed, else on one row from column left.
fn cells(
    spans: &[Span],
    width: usize,
    style: &str,
    base: Option<Rgb>,
    strong: Option<Rgb>,
    wrap: bool,
    left: usize,
) -> Vec<Vec<Seg>> {
    let segs: Vec<Seg> = spans
        .iter()
        .filter(|(text, _)| !text.is_empty())
        .map(|(text, changed)| {
            let back = match (changed, strong, base) {
                (true, Some(s), _) => bg(s),
                (_, _, Some(b)) => bg(b),
                _ => String::new(),
            };
            seg(text.clone(), format!("{style}{back}"))
        })
        .collect();
    let parts = if wrap && width > 0 { ui::split(&segs, width) } else { vec![ui::slice(&segs, left, width)] };
    parts
        .into_iter()
        .map(|mut p| {
            let used = ui::seg_width(&p);
            if used < width {
                p.push(seg(" ".repeat(width - used), base.map_or(String::new(), bg)));
            }
            p
        })
        .collect()
}

fn num(v: Option<i64>, w: usize) -> String {
    format!("{:>w$}", v.map_or(String::new(), |n| n.to_string()))
}

/// Header lines of a commit (show) and its stat, coloured here.
fn pre_row(l: &str) -> Row {
    let row = |segs: Vec<Seg>| Row { segs, ..Row::default() };
    if l.starts_with("commit ") {
        return row(vec![seg(l, fg(c().yellow))]);
    }
    let label = ["Author", "AuthorDate", "Commit", "CommitDate", "Date", "Merge"];
    if label.iter().any(|k| l.strip_prefix(k).is_some_and(|r| r.starts_with(':'))) {
        return row(vec![seg(l, dim())]);
    }
    let stat = Regex::new(r"^( .*\| +\d+ )(\+*)(-*)$", "").expect("stat");
    if let Some(m) = stat.exec(l) {
        let g = |i| m.get(i).unwrap_or("").to_string();
        return row(vec![seg(g(1), ""), seg(g(2), green()), seg(g(3), red())]);
    }
    row(vec![seg(l, "")])
}

fn name_of(f: &File) -> String {
    f.to.clone().or_else(|| f.from.clone()).unwrap_or_default()
}

fn file_row(f: &File, width: usize, viewed: bool) -> Row {
    let name = if f.status == "renamed" || f.status == "copied" {
        format!("{} → {}", f.from.as_deref().unwrap_or("undefined"), f.to.as_deref().unwrap_or("undefined"))
    } else {
        f.to.clone().or_else(|| f.from.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "?".into())
    };
    let mut status = if f.status == "modified" { String::new() } else { format!("  {}", f.status) };
    if !f.sec.is_empty() {
        status += &format!("  {}", f.sec);
    }
    if viewed {
        status += "  ✓ viewed";
    }
    let counts = format!(" +{} −{} ", f.add, f.del);
    let (wi, cl, sl) = (width as i64, js::utf16_len(&counts) as i64, js::utf16_len(&status) as i64);
    let label = format!(" {}", ui::ellipsis(&ui::clean_line(&name, 4, None), (wi - cl - sl - 2).max(1)));
    let pad = " ".repeat((wi - ui::text_width(&label) as i64 - sl - cl).max(0) as usize);
    Row {
        segs: vec![
            seg(label, "\x1b[1m"),
            seg(status, dim()),
            seg(pad, ""),
            seg(format!(" +{}", f.add), green()),
            seg(format!(" −{} ", f.del), red()),
        ],
        back: Some(c().header),
        file: true,
        ..Row::default()
    }
}

/// Lines of a hunk grouped: an unchanged line or a note ("No newline at end of file"), or a change (removed lines
/// followed by added ones, paired for the word highlight and the side-by-side rows), as indexes in the hunk.
enum Block {
    Line(usize),
    Change { dels: Vec<usize>, adds: Vec<usize>, words: Vec<Option<(Vec<Span>, Vec<Span>)>> },
}

fn blocks(lines: &[Line]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let s = l.sign;
        if s == '-' || s == '+' {
            let reuse = matches!(out.last(), Some(Block::Change { adds, .. }) if !(s == '-' && !adds.is_empty()));
            if !reuse {
                out.push(Block::Change { dels: Vec::new(), adds: Vec::new(), words: Vec::new() });
            }
            if let Some(Block::Change { dels, adds, .. }) = out.last_mut() {
                if s == '-' { dels.push(i) } else { adds.push(i) }
            }
        } else {
            out.push(Block::Line(i));
        }
    }
    for b in &mut out {
        if let Block::Change { dels, adds, words: w } = b {
            *w = dels
                .iter()
                .enumerate()
                .map(|(k, &d)| {
                    adds.get(k).and_then(|&a| {
                        words(&ui::clean_line(&lines[d].text, 4, None), &ui::clean_line(&lines[a].text, 4, None))
                    })
                })
                .collect();
        }
    }
    out
}

/// A line's row inline: its gutter (both numbers), sign and text; a line too long for its row wraps onto rows marked
/// more, with a blank gutter.
#[derive(Clone)]
struct InlineSpec {
    sign: char,
    gutter: String,
    spans: Vec<Span>,
    rest: usize,
    style: String,
    back: Option<Rgb>,
    strong: Option<Rgb>,
    sign_style: String,
    wrap: bool,
}

impl InlineSpec {
    fn make(&self, left: usize) -> Vec<Vec<Seg>> {
        cells(&self.spans, self.rest, &self.style, self.back, self.strong, self.wrap, left)
    }

    fn lead(&self, k: usize) -> Vec<Seg> {
        let gutter = if k > 0 { " ".repeat(self.gutter.len()) } else { self.gutter.clone() };
        let sign = if k > 0 || self.sign == '\\' { " ".to_string() } else { self.sign.to_string() };
        vec![seg(gutter, dim()), seg(sign, self.sign_style.clone()), seg(" ", "")]
    }
}

/// One side of a side by side row.
#[derive(Clone)]
struct SideSpec {
    line: Option<(char, Option<i64>)>,
    width: usize,
    spans: Vec<Span>,
    base: Option<Rgb>,
    strong: Option<Rgb>,
    nw: usize,
    wrap: bool,
}

impl SideSpec {
    /// Its rows, and the blank cell padding it.
    fn build(&self, left: usize) -> (Vec<Vec<Seg>>, Vec<Seg>) {
        let Some((s, no)) = self.line else { return (Vec::new(), vec![seg(" ".repeat(self.width), bg(c().gap))]) };
        let lead = format!("{}{}", dim(), self.base.map_or(String::new(), bg));
        let style = if s == '\\' { dim() } else { String::new() };
        let nw = self.nw;
        let parts =
            cells(&self.spans, self.width.saturating_sub(nw + 1), &style, self.base, self.strong, self.wrap, left)
                .into_iter()
                .enumerate()
                .map(|(k, c)| {
                    let n = if k > 0 {
                        " ".repeat(nw + 1)
                    } else {
                        format!("{} ", num(if s == '\\' { None } else { no }, nw))
                    };
                    let mut row = vec![seg(n, lead.clone())];
                    row.extend(c);
                    row
                })
                .collect();
        (parts, vec![seg(" ".repeat(self.width), self.base.map_or(String::new(), bg))])
    }
}

fn join(l: &(Vec<Vec<Seg>>, Vec<Seg>), r: &(Vec<Vec<Seg>>, Vec<Seg>), k: usize) -> Vec<Seg> {
    let mut out = l.0.get(k).unwrap_or(&l.1).clone();
    out.push(seg("│", dim()));
    out.extend(r.0.get(k).unwrap_or(&r.1).iter().cloned());
    out
}

struct Ctx<'a> {
    lines: &'a [Line],
    fi: usize,
    hi: usize,
    width: usize,
    nw: usize,
    wrap: bool,
    left: usize,
}

fn spans_of(l: &Line, spans: Option<&Vec<Span>>) -> Vec<Span> {
    spans.cloned().unwrap_or_else(|| vec![(ui::clean_line(&l.text, 4, None), false)])
}

fn inline_rows(rows: &mut Vec<Row>, b: &Block, x: &Ctx) {
    let mut row = |li: usize, spans: Option<&Vec<Span>>, back: Option<Rgb>, strong: Option<Rgb>, sign_style: String| {
        let l = &x.lines[li];
        let nw = x.nw;
        let gutter =
            if l.sign == '\\' { " ".repeat(nw * 2 + 2) } else { format!("{} {} ", num(l.old, nw), num(l.new, nw)) };
        let spec = Rc::new(InlineSpec {
            sign: l.sign,
            gutter,
            spans: spans_of(l, spans),
            rest: x.width.saturating_sub(nw * 2 + 4),
            style: if l.sign == '\\' { dim() } else { String::new() },
            back,
            strong,
            sign_style,
            wrap: x.wrap,
        });
        for (k, c) in spec.make(x.left).into_iter().enumerate() {
            let mut segs = spec.lead(k);
            segs.extend(c);
            let redo: Option<Redo> = (!x.wrap).then(|| {
                let spec = spec.clone();
                Rc::new(move |left: usize| {
                    let mut segs = spec.lead(0);
                    segs.extend(spec.make(left).into_iter().next().unwrap_or_default());
                    segs
                }) as Redo
            });
            rows.push(Row {
                segs,
                back,
                place: if l.sign == '\\' { 0 } else { l.at },
                more: k > 0,
                lines: if k > 0 { Vec::new() } else { vec![(x.fi, x.hi, li)] },
                redo,
                ..Row::default()
            });
        }
    };
    match b {
        Block::Line(i) => row(*i, None, None, None, dim()),
        Block::Change { dels, adds, words } => {
            for (k, &d) in dels.iter().enumerate() {
                let w = words.get(k).and_then(|w| w.as_ref()).map(|w| &w.0);
                row(d, w, Some(c().removed), Some(c().removed_word), red());
            }
            for (k, &a) in adds.iter().enumerate() {
                let w = words.get(k).and_then(|w| w.as_ref()).map(|w| &w.1);
                row(a, w, Some(c().added), Some(c().added_word), green());
            }
        }
    }
}

/// Each side wraps on its own: the shorter one is padded with blank rows, on its background.
fn split_rows(rows: &mut Vec<Row>, b: &Block, x: &Ctx) {
    let half = x.width.saturating_sub(1) / 2;
    let right = x.width.saturating_sub(1 + half);
    let side = |li: Option<usize>, width: usize, spans: Option<&Vec<Span>>, base, strong, is_new: bool| {
        let l = li.map(|i| &x.lines[i]);
        SideSpec {
            line: l.map(|l| (l.sign, if l.sign == '+' || (l.sign == ' ' && is_new) { l.new } else { l.old })),
            width,
            spans: l.map_or_else(Vec::new, |l| spans_of(l, spans)),
            base,
            strong,
            nw: x.nw,
            wrap: x.wrap,
        }
    };
    let mut push = |pair: (SideSpec, SideSpec), place: i64, lines: Vec<usize>| {
        let pair = Rc::new(pair);
        let (l, r) = (pair.0.build(x.left), pair.1.build(x.left));
        for k in 0..l.0.len().max(r.0.len()).max(1) {
            let redo: Option<Redo> = (!x.wrap).then(|| {
                let pair = pair.clone();
                Rc::new(move |left: usize| join(&pair.0.build(left), &pair.1.build(left), 0)) as Redo
            });
            rows.push(Row {
                segs: join(&l, &r, k),
                place,
                more: k > 0,
                lines: if k > 0 { Vec::new() } else { lines.iter().map(|&i| (x.fi, x.hi, i)).collect() },
                redo,
                ..Row::default()
            });
        }
    };
    match b {
        Block::Line(i) => {
            let l = &x.lines[*i];
            let pair = (side(Some(*i), half, None, None, None, false), side(Some(*i), right, None, None, None, true));
            push(pair, if l.sign == '\\' { 0 } else { l.at }, vec![*i]);
        }
        Block::Change { dels, adds, words } => {
            for k in 0..dels.len().max(adds.len()) {
                let w = words.get(k).and_then(|w| w.as_ref());
                let (d, a) = (dels.get(k).copied(), adds.get(k).copied());
                let pair = (
                    side(d, half, w.map(|w| &w.0), Some(c().removed), Some(c().removed_word), false),
                    side(a, right, w.map(|w| &w.1), Some(c().added), Some(c().added_word), true),
                );
                let place = x.lines[a.or(d).expect("a line")].at;
                push(pair, place, d.into_iter().chain(a).collect());
            }
        }
    }
}

/// Rows of a parsed diff for a width. mode: "inline", or "split" (old version on the left, new on the right).
/// titled: the caller's title already names the only file, whose own row is left out. A file of a section
/// (changes_of) has it in its refs.
pub fn render(d: &Diff, width: usize, mode: &str, titled: bool, view: &View) -> Vec<Row> {
    let mut rows = Vec::new();
    let single = titled && d.pre.is_empty() && d.files.len() == 1;
    for l in &d.pre {
        rows.push(pre_row(&ui::clean_line(l, 4, None)));
    }
    for (fi, f) in d.files.iter().enumerate() {
        let file = name_of(f);
        let r = |from, to| Some(Ref { file: file.clone(), from, to, sec: f.sec.clone() });
        if !rows.is_empty() {
            rows.push(Row::default());
        }
        let folded = view.viewed.is_some_and(|v| v(f));
        if !single || !f.sec.is_empty() || folded {
            rows.push(Row { hunk: f.hunks.is_empty(), r#ref: r(0, 0), f: Some(fi), ..file_row(f, width, folded) });
        }
        if folded {
            continue;
        }
        for note in &f.notes {
            rows.push(Row { segs: vec![seg(format!("  {note}"), dim())], ..Row::default() });
        }
        if f.hunks.is_empty() && f.notes.is_empty() && f.status != "renamed" {
            rows.push(Row { segs: vec![seg("  (no content change)", dim())], ..Row::default() });
        }
        let mut last = 1;
        for h in &f.hunks {
            for l in &h.lines {
                last = last.max(l.old.unwrap_or(0)).max(l.new.unwrap_or(0));
            }
        }
        let nw = last.to_string().len();
        for (hi, h) in f.hunks.iter().enumerate() {
            let places: Vec<i64> = h.lines.iter().filter(|l| l.sign != '\\').map(|l| l.at).collect();
            let (from, to) = (places.iter().min().copied().unwrap_or(0), places.iter().max().copied().unwrap_or(0));
            rows.push(Row {
                segs: vec![seg(format!(" {}", ui::clean_line(&h.head, 4, None)), cyan())],
                back: Some(c().section),
                hunk: true,
                h: Some((fi, hi)),
                r#ref: r(from, to),
                ..Row::default()
            });
            let x = Ctx { lines: &h.lines, fi, hi, width, nw, wrap: view.wrap, left: view.left };
            for b in blocks(&h.lines) {
                if rows.len() > MAX_ROWS {
                    rows.push(Row { segs: vec![seg(" … diff too long, truncated", dim())], ..Row::default() });
                    return rows;
                }
                let at = rows.len();
                if mode == "split" {
                    split_rows(&mut rows, &b, &x);
                } else {
                    inline_rows(&mut rows, &b, &x);
                }
                for row in &mut rows[at..] {
                    if row.place != 0 {
                        row.r#ref = r(row.place, row.place);
                    }
                }
            }
        }
    }
    rows
}

/// The text of a note's lines in the version a diff shows, its new side (the index for a staged line, b for a
/// range), so that the note's anchor is not read from the file on disk, which may differ; None when the diff does
/// not hold them all (lines between two hunks).
pub fn shown_lines(parsed: &Diff, r: &Ref) -> Option<Vec<String>> {
    let f = parsed.files.iter().find(|x| name_of(x) == r.file && x.sec == r.sec)?;
    let mut by_line = std::collections::HashMap::new();
    for h in &f.hunks {
        for l in &h.lines {
            if l.sign != '\\'
                && let Some(n) = l.new
            {
                by_line.insert(n, l.text.clone());
            }
        }
    }
    (r.from..=r.to).map(|n| by_line.get(&n).cloned()).collect()
}

/// Where the lines from to to of the index (a staged note's ref) are in the file on disk, through the unstaged
/// changes of the same path: the same lines without unstaged changes; None when some were changed or removed since
/// they were staged.
pub fn disk_lines(parsed: &Diff, r: &Ref) -> Option<(i64, i64)> {
    let Some(f) = parsed.files.iter().find(|x| name_of(x) == r.file && x.sec == "unstaged") else {
        return Some((r.from, r.to));
    };
    let head = Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))?", "").expect("head");
    let at = |i: i64| -> Option<i64> {
        let mut delta = 0;
        for h in &f.hunks {
            let m = head.exec(&h.head)?;
            let n = |k: usize, missing: i64| m.get(k).map_or(missing, |v| v.parse::<i64>().unwrap_or(0));
            let (a, b, c, d) = (n(1, 0), n(2, 1), n(3, 0), n(4, 1));
            if b != 0 && i >= a && i < a + b {
                let l = h.lines.iter().find(|x| x.old == Some(i));
                return l.filter(|l| l.sign == ' ').and_then(|l| l.new);
            }
            // The first line after the hunk, on each side (after line a when it holds none).
            let (old_next, new_next) = (if b != 0 { a + b } else { a + 1 }, if d != 0 { c + d } else { c + 1 });
            if old_next <= i {
                delta = new_next - old_next;
            } else {
                break;
            }
        }
        Some(i + delta)
    };
    let from = at(r.from)?;
    for i in r.from + 1..=r.to {
        if at(i) != Some(from + i - r.from) {
            return None;
        }
    }
    Some((from, from + r.to - r.from))
}

pub fn default_mode(width: usize) -> &'static str {
    if width >= SPLIT_MIN { "split" } else { "inline" }
}

/// The layout chosen with v, kept in the settings for the next diffs (source control and the preview included), by
/// width: one for narrow diffs (under SPLIT_MIN columns) and one for wide ones, so that each keeps its own.
/// diffLayout: { narrow, wide }, each "inline" or "split", unset to choose by width; a single value holds for both
/// until v chooses one.
fn size_of(width: usize) -> &'static str {
    if width >= SPLIT_MIN { "wide" } else { "narrow" }
}

pub fn saved_mode(width: usize) -> Option<&'static str> {
    let s = exec::read_settings();
    let m = s.get("diffLayout");
    let v = match m {
        Some(o @ Value::Obj(_)) => o.get(size_of(width)),
        other => other,
    };
    match v.and_then(Value::as_str) {
        Some("inline") => Some("inline"),
        Some("split") => Some("split"),
        _ => None,
    }
}

pub fn save_mode(width: usize, mode: &str) {
    let mut s = exec::read_settings();
    let mut both = match s.get("diffLayout") {
        Some(o @ Value::Obj(_)) => o.clone(),
        Some(old) if old.truthy() => crate::obj! { "narrow" => old.clone(), "wide" => old.clone() },
        _ => Value::Obj(Vec::new()),
    };
    both.set(size_of(width), Value::Str(mode.into()));
    s.set("diffLayout", both);
    exec::write_settings(&s);
}

/// The layout of a diff of that width: the one chosen for its size, else by width.
pub fn layout_of(width: usize) -> &'static str {
    saved_mode(width).unwrap_or_else(|| default_mode(width))
}

/// The width of the longest line of parsed changes, the most ← → can scroll.
pub fn widest_of(parsed: &Diff) -> usize {
    let mut w = 0;
    for f in &parsed.files {
        for h in &f.hunks {
            for l in &h.lines {
                w = w.max(ui::text_width(&ui::clean_line(&l.text, 4, None)));
            }
        }
    }
    w
}
