//! Text on the terminal: control characters cleaned, display width, cut and ellipsis, lines padded to a width;
//! segments sliced, split and marked; the segmented switch and the drawn lines.

use super::theme::c;
use super::{Rgb, Seg, bg, fg, seg};
use crate::unicode;

/// The C0 and C1 control characters, never written to the terminal from file names or contents.
pub fn is_control(ch: char) -> bool {
    matches!(ch, '\x00'..='\x1f' | '\x7f'..='\u{9f}')
}

pub fn clean(text: &str) -> String {
    text.chars().map(|ch| if is_control(ch) { '?' } else { ch }).collect()
}

/// Tabs expanded to spaces, other control characters removed: for file contents. at: receives, for each byte of
/// text and its end, where it lands in the cleaned text.
pub fn clean_line(text: &str, tab: usize, mut at: Option<&mut Vec<usize>>) -> String {
    if at.is_none() && !text.chars().any(is_control) {
        return text.to_string();
    }
    let mut s = String::new();
    let mut count = 0;
    for ch in text.chars() {
        if let Some(at) = at.as_deref_mut() {
            at.extend(std::iter::repeat_n(s.len(), ch.len_utf8()));
        }
        if ch == '\t' {
            // A tab stop of 0: nothing.
            let n = if tab == 0 { 0 } else { tab - count % tab };
            s.extend(std::iter::repeat_n(' ', n));
            count += n;
        } else if !is_control(ch) {
            s.push(ch);
            count += 1;
        }
    }
    if let Some(at) = at {
        at.push(s.len());
    }
    s
}

/// Wide characters: CJK, emoji presentation, and the trigrams ☰ (wide since Unicode 16, as herdr counts them); not
/// the regional indicators.
fn wide(ch: char) -> bool {
    if unicode::is_regional_indicator(ch) {
        return false;
    }
    matches!(ch,
        '\u{1100}'..='\u{115f}' | '\u{2e80}'..='\u{303e}' | '\u{3041}'..='\u{33ff}' | '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}' | '\u{a000}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7a3}' | '\u{f900}'..='\u{faff}'
        | '\u{fe30}'..='\u{fe4f}' | '\u{ff00}'..='\u{ff60}' | '\u{ffe0}'..='\u{ffe6}' | '\u{20000}'..='\u{3fffd}'
        | '\u{2630}'..='\u{2637}' | '\u{268a}'..='\u{268f}')
        || unicode::is_emoji_presentation(ch)
}

/// Terminal columns of a character: none for combining marks, two for wide ones. prev: the character before, for
/// the emoji selector U+FE0F, which makes a text symbol such as ⚠ an emoji two columns wide.
pub fn cols(ch: char, prev: Option<char>) -> usize {
    if ch == '\u{fe0f}' && prev.is_some_and(|p| unicode::is_emoji(p) && !wide(p)) {
        1
    } else if unicode::is_mark(ch) || matches!(ch, '\u{200b}' | '\u{200d}' | '\u{fe0f}') {
        0
    } else if wide(ch) {
        2
    } else {
        1
    }
}

/// The longest start of a text that fits in max columns, and its width. prev: the character before the text.
pub fn fit(text: &str, max: usize, mut prev: Option<char>) -> (&str, usize) {
    let mut w = 0;
    for (i, ch) in text.char_indices() {
        let c = cols(ch, prev);
        if w + c > max {
            return (&text[..i], w);
        }
        w += c;
        prev = Some(ch);
    }
    (text, w)
}

pub fn text_width(text: &str) -> usize {
    fit(text, usize::MAX, None).1
}

/// Cut to max columns with an ellipsis when too long.
pub fn ellipsis(text: &str, max: i64) -> String {
    if (text_width(text) as i64) <= max {
        return text.to_string();
    }
    format!("{}…", fit(text, (max - 1).max(0) as usize, None).0)
}

/// A screen line of exactly width columns: the segments' text truncated to the width, control characters replaced;
/// a style may set a background, reset after its segment. back: background colour of the whole line.
pub fn line(segments: &[Seg], width: usize, back: Option<Rgb>) -> String {
    let back = back.map(bg).unwrap_or_default();
    let mut s = back.clone();
    let mut used = 0;
    let mut prev = None;
    for (text, style) in segments {
        let text = clean(text);
        let (part, w) = fit(&text, width - used, prev);
        if part.is_empty() {
            continue;
        }
        prev = part.chars().last();
        s += style;
        s += part;
        s += "\x1b[0m";
        s += &back;
        used += w;
    }
    s.extend(std::iter::repeat_n(' ', width - used));
    s + "\x1b[0m"
}

pub fn seg_text(segs: &[Seg]) -> String {
    segs.iter().map(|(t, _)| t.as_str()).collect()
}

pub fn seg_width(segs: &[Seg]) -> usize {
    text_width(&seg_text(segs))
}

/// The columns [from, from + width) of a segment list, for horizontal scrolling. A wide character cut by the left
/// edge leaves a space, so that the text keeps its columns.
pub fn slice(segs: &[Seg], from: usize, width: usize) -> Vec<Seg> {
    let mut res = Vec::new();
    let (mut col, mut prev, mut pw) = (0, None, 0);
    for (text, style) in segs {
        let mut part = String::new();
        for ch in text.chars() {
            let c = cols(ch, prev);
            // An emoji selector whose character was cut by the left edge: a space in its place.
            let lone = c > 0 && ch == '\u{fe0f}' && col < from + pw;
            prev = Some(ch);
            pw = c;
            if col >= from && col + c <= from + width {
                part.push(if lone { ' ' } else { ch });
            } else if col < from && col + c > from {
                part.extend(std::iter::repeat_n(' ', (col + c - from).min(width)));
            }
            col += c;
            if col >= from + width {
                break;
            }
        }
        if !part.is_empty() {
            res.push((part, style.clone()));
        }
        if col >= from + width {
            break;
        }
    }
    res
}

/// A segment list cut into lines of at most width columns.
pub fn split(segs: &[Seg], width: usize) -> Vec<Vec<Seg>> {
    let mut lines: Vec<Vec<Seg>> = vec![Vec::new()];
    let (mut w, mut prev, mut pw): (usize, Option<char>, usize) = (0, None, 0);
    for (text, style) in segs {
        let mut part = String::new();
        for ch in text.chars() {
            let c = cols(ch, prev);
            // The emoji selector widens the character before it (pw columns, maybe in the previous segment): both go
            // to the next line.
            let base = if c > 0 && ch == '\u{fe0f}' { prev } else { None };
            let bw = if base.is_some() { pw } else { 0 };
            if w + c > width && w > bw {
                let mut moved = Vec::new();
                let mut keep = String::new();
                if let Some(b) = base {
                    if part.ends_with(b) {
                        part.truncate(part.len() - b.len_utf8());
                        keep.push(b);
                    } else {
                        let last_line = lines.last_mut().expect("a line");
                        if let Some(last) = last_line.last_mut()
                            && last.0.ends_with(b)
                        {
                            last.0.truncate(last.0.len() - b.len_utf8());
                            let style = last.1.clone();
                            if last.0.is_empty() {
                                last_line.pop();
                            }
                            moved.push((b.to_string(), style));
                        }
                    }
                }
                if !part.is_empty() {
                    lines.last_mut().expect("a line").push((std::mem::take(&mut part), style.clone()));
                }
                w = if !moved.is_empty() || !keep.is_empty() { bw } else { 0 };
                lines.push(moved);
                part = keep;
            }
            part.push(ch);
            w += c;
            prev = Some(ch);
            pw = c;
        }
        if !part.is_empty() {
            lines.last_mut().expect("a line").push((part, style.clone()));
        }
    }
    lines
}

/// The byte ranges [start, end) of the segments' text restyled with style added (search matches).
pub fn mark(segs: &[Seg], ranges: &[(usize, usize)], style: &str) -> Vec<Seg> {
    if ranges.is_empty() {
        return segs.to_vec();
    }
    let mut res = Vec::new();
    let mut pos = 0;
    for (text, base) in segs {
        let mut cut = 0;
        let end = pos + text.len();
        // A bound inside a character takes the whole character: its halves are never parted.
        let inside = |p: usize| p > pos && p < end;
        let ranges: Vec<(usize, usize)> = ranges
            .iter()
            .map(|&(a, b)| {
                let a = if inside(a) { text.floor_char_boundary(a - pos) + pos } else { a };
                (a, if inside(b) { text.ceil_char_boundary(b - pos) + pos } else { b })
            })
            .collect();
        let mut points: Vec<usize> = Vec::new();
        for &(a, b) in &ranges {
            for p in [a, b] {
                if inside(p) && !points.contains(&(p - pos)) {
                    points.push(p - pos);
                }
            }
        }
        points.sort_unstable();
        points.push(text.len());
        for p in points {
            if p <= cut {
                continue;
            }
            let at = pos + cut;
            let hit = ranges.iter().any(|&(a, b)| at >= a && at < b);
            res.push((text[cut..p].to_string(), if hit { format!("{base}{style}") } else { base.clone() }));
            cut = p;
        }
        pos = end;
    }
    res
}

/// An option's value as the explorer's settings show it.
pub fn on_off(on: bool) -> Seg {
    if on { seg("● on", fg(c().green)) } else { seg("○ off", fg(c().dim)) }
}

pub fn switch_style(on: bool) -> String {
    if on { format!("{}{}\x1b[1m", bg(c().select), fg(c().text)) } else { format!("{}{}", bg(c().switch), fg(c().dim)) }
}

/// A choice of a segmented switch: its id and the columns it takes from 0, for clicks.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub from: usize,
    pub to: usize,
}

/// Two or more ways to see the same thing, the active one filled: (id, label) choices. Returns the segments, their
/// width and the columns of each choice.
pub fn segmented(choices: &[(&str, &str)], active: &str) -> (Vec<Seg>, usize, Vec<Hit>) {
    let (mut segs, mut hits, mut x) = (Vec::new(), Vec::new(), 0);
    for &(id, label) in choices {
        let text = format!(" {label} ");
        let w = text_width(&text);
        segs.push(seg(text, switch_style(id == active)));
        hits.push(Hit { id: id.to_string(), from: x, to: (x + w).wrapping_sub(1) });
        x += w;
    }
    (segs, x, hits)
}

/// The tokens of a drawn line: its CSI sequences and its characters.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let ch = rest.chars().next()?;
        let mut n = ch.len_utf8();
        if let Some(seq) = rest.strip_prefix("\x1b[") {
            let params = seq.bytes().take_while(|b| b.is_ascii_digit() || *b == b';' || *b == b'?').count();
            if seq.as_bytes().get(params).is_some_and(u8::is_ascii_alphabetic) {
                n = 2 + params + 1;
            }
        }
        let (tok, tail) = rest.split_at(n);
        rest = tail;
        Some(tok)
    })
}

/// The first n columns of a drawn line (line()'s output), its colours kept, padded with spaces when shorter.
pub fn head(text: &str, n: usize) -> String {
    let (mut out, mut w, mut prev) = (String::new(), 0, None);
    for tok in tokens(text) {
        if tok.starts_with('\x1b') {
            out += tok;
            continue;
        }
        let ch = tok.chars().next();
        let c = cols(ch.unwrap_or(' '), prev);
        if w + c > n {
            break;
        }
        out += tok;
        w += c;
        prev = ch;
    }
    format!("{out}\x1b[0m{}", " ".repeat(n.saturating_sub(w)))
}

/// A drawn line from column n (from 0), its colours kept: the styles met before n are kept in front; a wide
/// character cut by n leaves spaces.
pub fn tail(text: &str, n: usize) -> String {
    let (mut pre, mut out, mut w, mut prev) = (String::new(), String::new(), 0, None);
    for tok in tokens(text) {
        if tok.starts_with('\x1b') {
            if w >= n {
                out += tok
            } else {
                pre += tok
            }
            continue;
        }
        let ch = tok.chars().next();
        let c = cols(ch.unwrap_or(' '), prev);
        prev = ch;
        if w >= n {
            out += tok;
        } else if w + c > n {
            out += &" ".repeat(w + c - n);
        }
        w += c;
    }
    format!("{pre}{out}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A range ending inside a character (a column given for one) marks the whole character, without a panic.
    #[test]
    fn a_mark_never_cuts_a_character() {
        let segs = vec![("éa".to_string(), String::new())];
        let (marked, plain) = (|t: &str| (t.to_string(), "x".to_string()), |t: &str| (t.to_string(), String::new()));
        assert_eq!(mark(&segs, &[(0, 1)], "x"), [marked("é"), plain("a")]);
        assert_eq!(mark(&segs, &[(1, 3)], "x"), [marked("éa")]);
    }
}
