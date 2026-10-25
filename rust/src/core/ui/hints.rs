//! The key hints of a footer, clickable, and the key help drawn over a pane.

use super::input::{Field, Mouse, printable};
use super::message::empty;
use super::text::{ellipsis, line, seg_width, text_width};
use super::theme::{c, g};
use super::{Seg, fg, seg};
use crate::core::js;

/// Key hints: the segments, and the columns of each pair from their start, to click them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hints {
    pub segs: Vec<Seg>,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub x0: usize,
    pub x1: usize,
    pub key: String,
}

/// Each key in bold, its action dimmed, as many pairs as fit in max columns. pairs: (key, action).
pub fn keys<K: AsRef<str>, A: AsRef<str>>(pairs: &[(K, A)], max: usize) -> Hints {
    let mut out = Hints::default();
    let mut used = 0;
    for (k, action) in pairs {
        let (k, action) = (k.as_ref(), action.as_ref());
        let lead = if out.segs.is_empty() { " " } else { "   " };
        let w = text_width(&format!("{lead}{k} {action}"));
        if used + w > max {
            break;
        }
        out.segs.push(seg(format!("{lead}{k}"), format!("\x1b[1m{}", fg(c().text))));
        out.segs.push(seg(format!(" {action}"), fg(c().dim)));
        if let Some(key) = key_of(k) {
            out.zones.push(Zone { x0: used + lead.len(), x1: used + w, key });
        }
        used += w;
    }
    out
}

/// The key a click on a hint sends: its first key, by its name for the key handlers; none for a range of keys (↑↓,
/// 1-4) or arrows.
fn key_of(label: &str) -> Option<String> {
    let k = label.split(' ').next().unwrap_or("");
    let b = k.as_bytes();
    let range = b.len() == 3 && b[0].is_ascii_digit() && b[1] == b'-' && b[2].is_ascii_digit();
    if k.contains(['↑', '↓', '←', '→']) || range {
        return None;
    }
    let sent = match k {
        "⏎" | "↵" => "enter",
        "esc" => "escape",
        "del" => "delete",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        _ => k,
    };
    // An empty key sends nothing.
    (!sent.is_empty()).then(|| sent.to_string())
}

/// The key of the hint at column x (0-based) of a line whose hints start at column from.
pub fn key_at(hints: &Hints, x: i64, from: i64) -> Option<String> {
    hints.zones.iter().find(|z| x >= from + z.x0 as i64 && x < from + z.x1 as i64).map(|z| z.key.clone())
}

/// A key name as shown to the user: arrows and Enter as symbols, the long names shortened.
pub fn key_label(k: &str) -> String {
    let name = k.rsplit('+').next().unwrap_or(k);
    let short = match name {
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "enter" => "⏎",
        "escape" => "esc",
        "pageup" => "pgup",
        "pagedown" => "pgdn",
        _ => return k.to_string(),
    };
    format!("{}{short}", &k[..k.len() - name.len()])
}

/// Lines set from row i, the list grown when shorter.
pub(super) fn put(lines: &mut Vec<String>, i: usize, l: String) {
    if lines.len() <= i {
        lines.resize(i + 1, String::new());
    }
    lines[i] = l;
}

/// A group of the key help: its title and its (key, action) pairs.
pub type Group = (String, Vec<(String, String)>);

/// What an open key help keeps: its first row shown, its find field (None when closed), and from the last draw the
/// rows the list gets, how many there are, and the pane's size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpState {
    pub top: i64,
    pub find: Option<Field>,
    pub n: i64,
    pub count: i64,
    pub w: Option<usize>,
    pub h: Option<usize>,
}

const HELP_KEYS: [(&str, &str); 3] = [("↑↓", "scroll"), ("/", "find"), ("esc", "close")];

/// The rows of the help: a group's title alone, or a key and its action.
fn help_rows(groups: &[Group], state: &HelpState) -> Vec<Vec<String>> {
    let q = state.find.as_ref().map(|f| js::lower(&f.value)).unwrap_or_default();
    let mut out = Vec::new();
    for (g, pairs) in groups {
        let kept: Vec<Vec<String>> = pairs
            .iter()
            .map(|(k, a)| vec![k.split(' ').map(key_label).collect::<Vec<_>>().join(" "), a.clone()])
            .filter(|r| q.is_empty() || js::lower(&format!("{} {}", r[0], r[1])).contains(&q))
            .collect();
        if !kept.is_empty() {
            out.push(vec![g.clone()]);
            out.extend(kept);
        }
    }
    out
}

/// The key help of a pane (? or f1), over the whole pane: the title, a find field once / is pressed, the keys by
/// group, then a rule and the keys of the help.
pub fn help(lines: &mut [String], w: usize, title: &str, groups: &[Group], state: &mut HelpState) {
    let h = lines.len();
    let rows = help_rows(groups, state);
    let at = if state.find.is_some() { 2 } else { 1 };
    let n = (h as i64 - at - 2).max(1);
    state.top = state.top.min(rows.len() as i64 - n).max(0);
    state.n = n;
    state.count = rows.len() as i64;
    state.w = Some(w);
    state.h = Some(h);
    // The keys' column: wide enough for most, a longer list of keys cut.
    let widest = rows.iter().filter(|r| r.len() == 2).map(|r| text_width(&r[0]) + 2).fold(6, usize::max);
    let kw = widest.min(11).min(w / 2);
    let title = if title.is_empty() { String::new() } else { format!(" · {title}") };
    let mut out = vec![line(&[seg(" ‹ ", fg(c().dim)), seg("Keys", "\x1b[1m"), seg(title, fg(c().dim))], w, None)];
    if let Some(find) = &state.find {
        let count = if find.value.is_empty() {
            String::new()
        } else {
            format!(" {}", rows.iter().filter(|r| r.len() == 2).count())
        };
        let mut segs = vec![seg(format!(" {} ", g().search), fg(c().dim))];
        let field = find.segments((w as i64 - 4 - count.len() as i64).max(1), "");
        let pad = (w as i64 - 3 - seg_width(&field) as i64).max(0) as usize;
        segs.extend(field);
        segs.push(seg(format!("{count:>pad$}"), fg(c().dim)));
        out.push(line(&segs, w, Some(c().header)));
    }
    for r in rows.iter().skip(state.top as usize).take(n as usize) {
        if r.len() == 1 {
            out.push(line(&[seg(format!(" {}", r[0]), format!("\x1b[1m{}", fg(c().blue)))], w, None));
            continue;
        }
        let k = ellipsis(&r[0], kw as i64 - 2);
        let pad = " ".repeat((kw as i64 - text_width(&k) as i64).max(1) as usize);
        let segs = [seg(format!("  {k}{pad}"), format!("\x1b[1m{}", fg(c().text))), seg(r[1].clone(), fg(c().dim))];
        out.push(line(&segs, w, None));
    }
    if rows.is_empty() {
        out.extend(empty(w, n as usize, "No match", &[("esc", "clear")]));
    }
    while out.len() + 2 < h {
        out.push(line(&[], w, None));
    }
    let more =
        format!("{}{}", if state.top > 0 { "▴" } else { "" }, if state.top + n < rows.len() as i64 { "▾" } else { "" });
    let rule = "─".repeat((w as i64 - more.chars().count() as i64 - 1).max(0) as usize);
    out.push(line(&[seg(rule + &more, fg(c().dim))], w, None));
    out.push(line(&keys(&HELP_KEYS, w).segs, w, Some(c().header)));
    for (i, l) in out.into_iter().take(h).enumerate() {
        lines[i] = l;
    }
}

fn help_scroll(state: &mut HelpState, by: i64) {
    let n = if state.n == 0 { 1 } else { state.n };
    state.top = (state.top.saturating_add(by)).min(state.count - n).max(0);
}

/// Keys of an open help: / opens the find, where characters are typed (Escape closes it); the arrows, page keys,
/// home and end scroll. Returns false when the help is to close: Escape, q, ? or f1.
pub fn help_key(k: &str, state: &mut HelpState) -> bool {
    if let Some(find) = &mut state.find {
        if k == "escape" {
            state.find = None;
            return true;
        }
        if printable(k) || k == "space" || k == "backspace" || k == "delete" {
            if find.key(k) {
                state.top = 0;
            }
            return true;
        }
    } else if k == "/" {
        state.find = Some(Field::new(""));
        state.top = 0;
        return true;
    } else if matches!(k, "escape" | "q" | "?" | "f1") {
        return false;
    }
    let page = ((if state.n == 0 { 10 } else { state.n }) - 1).max(1);
    let half = (page + 1) / 2;
    let by = match k {
        "up" | "k" => -1,
        "down" | "j" => 1,
        "pageup" => -page,
        "pagedown" => page,
        "ctrl+u" => -half,
        "ctrl+d" => half,
        "home" | "g" => i64::MIN,
        "end" | "G" => i64::MAX,
        _ => 0,
    };
    if by != 0 {
        help_scroll(state, by);
    }
    true
}

/// Mouse on an open help: the wheel scrolls, a click on a key hint acts as its key. Returns false when the help is
/// to close: a click on the title, or on esc.
pub fn help_mouse(m: &Mouse, state: &mut HelpState) -> bool {
    if m.wheel != 0 {
        help_scroll(state, m.wheel * 3);
        return true;
    }
    if !m.press || m.drag || m.button != 0 {
        return true;
    }
    if m.y == 1 {
        return false;
    }
    let k = if state.h.is_some_and(|h| m.y == h as i64) {
        key_at(&keys(&HELP_KEYS, state.w.unwrap_or(usize::MAX)), m.x - 1, 0)
    } else {
        None
    };
    match k {
        Some(k) => help_key(&k, state),
        None => true,
    }
}
