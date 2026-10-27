//! Cards over a pane: buttons, the confirmation card and the question with a field.

use super::hints::{key_label, put};
use super::input::Mouse;
use super::message::wrap;
use super::text::{ellipsis, line, text_width};
use super::theme::c;
use super::{Rgb, Seg, bg, fg, seg};

/// A tone of a card's button: danger (red) or go (green).
fn button(tone: &str) -> Option<Rgb> {
    match tone {
        "danger" => Some(c().danger),
        "go" => Some(c().go),
        _ => None,
    }
}

fn button_text(tone: &str) -> Option<Rgb> {
    match tone {
        "danger" => Some(c().danger_text),
        "go" => Some(c().go_text),
        _ => None,
    }
}

/// A question in a card over the pane, answered yes or no.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Confirm {
    pub title: String,
    pub name: String,
    pub where_: Option<String>,
    pub note: Option<String>,
    pub action: String,
    /// 'danger' (the default) or 'go'.
    pub tone: String,
    /// 'yes' or 'no'.
    pub focus: String,
    /// The buttons' columns for clicks, from the last draw.
    pub zones: Option<ConfirmZones>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmZones {
    pub y: i64,
    pub yes: (i64, i64),
    pub no: (i64, i64),
}

/// The rows of a card's frame drawn into lines, at column x and row y, rows past the pane's last left out.
#[allow(clippy::too_many_arguments)]
fn card(lines: &mut Vec<String>, w: i64, h: i64, x: i64, y: i64, inner: i64, text: &[Vec<Seg>]) {
    let edge = fg(c().dim);
    let left = " ".repeat(x as usize);
    let right = " ".repeat((w - x - inner - 2).max(0) as usize);
    let width = inner as usize;
    let mut row = |i: i64, l: String| {
        if i < h {
            put(lines, i as usize, l);
        }
    };
    row(y, format!("{left}{}{right}", line(&[seg(format!("╭{}╮", "─".repeat(width)), &edge)], width + 2, None)));
    for (k, segs) in text.iter().enumerate() {
        let mut all = vec![seg("│", &edge)];
        all.extend(segs.iter().cloned());
        let l = format!("{left}{}{}{right}", line(&all, width + 1, None), line(&[seg("│", &edge)], 1, None));
        row(y + 1 + k as i64, l);
    }
    let bottom = format!("{left}{}{right}", line(&[seg(format!("╰{}╯", "─".repeat(width)), &edge)], width + 2, None));
    row(y + text.len() as i64 + 1, bottom);
}

/// The card's place: its left column and top row (from 0).
fn card_at(w: i64, h: i64, inner: i64, rows: i64) -> (i64, i64) {
    let x = (w - inner - 2).div_euclid(2).max(0);
    let y = (h - rows).min((h - rows).div_euclid(3)).max(1);
    (x, y)
}

/// A yes or no question drawn over the pane: its title, the name concerned and where it is, a note, then the
/// buttons, the action in red (green for tone 'go'), the focused one lit.
pub fn confirm(lines: &mut Vec<String>, w: i64, h: i64, q: &mut Confirm) {
    q.zones = None;
    // No room for the card's edges: keys only.
    if w < 2 {
        return;
    }
    // A narrow pane: the card takes its width, less a column on each side.
    let inner = 44.min((w - 4).max(12.min(w - 2)).max(0));
    let room = (inner - 2).max(1);
    let mut names: Vec<Vec<Seg>> = wrap(&q.name, room).into_iter().map(|l| vec![seg(format!(" {l}"), "")]).collect();
    let mut where_: Vec<Vec<Seg>> = q
        .where_
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(c().dim))])
        .collect();
    let tone_note = if q.tone == "go" { c().dim } else { c().red };
    let mut note: Vec<Vec<Seg>> = q
        .note
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(tone_note))])
        .collect();
    // A low pane: the blank lines go first, then where, the name's next lines, the note, the name and the title, so
    // that the buttons stay in it, under its first line.
    let (mut gaps, mut title) = (2, true);
    let fit = (h - 3).max(1);
    let rows = |title: bool, gaps: i64, names: usize, where_: usize, note: usize| {
        title as i64 + 1 + gaps + (names + where_ + note) as i64
    };
    macro_rules! over {
        () => {
            rows(title, gaps, names.len(), where_.len(), note.len()) > fit
        };
    }
    while over!() && gaps > 0 {
        gaps -= 1;
    }
    if over!() {
        where_.clear();
    }
    while over!() && names.len() > 1 {
        names.pop();
    }
    if over!() {
        note.clear();
    }
    if over!() {
        names.clear();
    }
    if over!() {
        title = false;
    }
    let mut text: Vec<Vec<Seg>> = Vec::new();
    if title {
        text.push(vec![seg(format!(" {}", ellipsis(&q.title, room)), "\x1b[1m")]);
    }
    if gaps > 1 {
        text.push(Vec::new());
    }
    text.extend(names);
    text.extend(where_);
    text.extend(note);
    let yes = format!(" y {} ", q.action);
    let no = " n Cancel ";
    let (yw, nw) = (text_width(&yes) as i64, text_width(no) as i64);
    let gap = (inner - 3 - yw - nw).clamp(1, 3);
    let tone = if button(&q.tone).is_some() { q.tone.as_str() } else { "danger" };
    let lit = format!("\x1b[1m{}", fg([255, 255, 255]));
    let focused = q.focus == "yes";
    let yes_style = if focused {
        format!("{lit}{}", bg(button(tone).unwrap_or_default()))
    } else {
        format!("{}{}", fg(button_text(tone).unwrap_or_default()), bg(c().button_off))
    };
    let no_style =
        if focused { format!("{}{}", fg(c().dim), bg(c().button_off)) } else { format!("{lit}{}", bg(c().button_on)) };
    if gaps > 0 {
        text.push(Vec::new());
    }
    text.push(vec![seg("  ", ""), seg(&yes, yes_style), seg(" ".repeat(gap as usize), ""), seg(no, no_style)]);
    let rows_h = text.len() as i64 + 2;
    let (x, y) = card_at(w, h, inner, rows_h);
    card(lines, w, h, x, y, inner, &text);
    // Screen columns from 1: the card's edge, then 2 spaces before the first button; a button cut by the edge is
    // clickable up to it.
    let (bx, end) = (x + 4, x + inner + 1);
    if y + rows_h - 2 < h {
        q.zones = Some(ConfirmZones {
            y: y + rows_h - 2,
            yes: (bx, (bx + yw - 1).min(end)),
            no: (bx + yw + gap, (bx + yw + gap + nw - 1).min(end)),
        });
    }
}

/// A key for the question: ← → (and tab, h, l) move between the buttons ('move'), Enter presses the focused one, y
/// confirms ('yes'); any other key cancels ('no').
pub fn confirm_key(k: &str, q: &mut Confirm) -> &'static str {
    if matches!(k, "left" | "right" | "tab" | "shift+tab" | "h" | "l") {
        q.focus = if q.focus == "yes" { "no" } else { "yes" }.into();
        return "move";
    }
    if k == "y" || (k == "enter" && q.focus == "yes") { "yes" } else { "no" }
}

/// A click: on the action's button 'yes', elsewhere 'no'; None for what is not a click.
pub fn confirm_mouse(m: &Mouse, q: &Confirm) -> Option<&'static str> {
    if !m.press || m.drag || m.wheel != 0 {
        return None;
    }
    let on = q.zones.as_ref().is_some_and(|z| m.button == 0 && m.y - 1 == z.y && m.x >= z.yes.0 && m.x <= z.yes.1);
    Some(if on { "yes" } else { "no" })
}

/// A button of an ask card: its key, label and tone ('go', 'danger', else grey).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Button {
    pub key: String,
    pub label: String,
    pub tone: String,
}

/// A question in a card with more answers than confirm's two.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ask {
    pub title: String,
    pub name: String,
    pub where_: Option<String>,
    pub note: Option<String>,
    /// Lines shown as bullets under the name.
    pub list: Vec<String>,
    pub buttons: Vec<Button>,
    /// The index of the button lit.
    pub focus: usize,
    /// From the last draw: the buttons' columns, (from, to, key, row), and the card's box (x0, x1, y0, y1).
    pub zones: Option<Vec<(i64, i64, String, i64)>>,
    pub boxed: Option<(i64, i64, i64, i64)>,
}

/// An ask card drawn over the pane as confirm draws its card, its buttons on as many rows as its width needs.
pub fn ask(lines: &mut Vec<String>, w: i64, h: i64, q: &mut Ask) {
    q.zones = None;
    q.boxed = None;
    if w < 2 {
        return;
    }
    let inner = 48.min((w - 4).max(12.min(w - 2)).max(0));
    let room = (inner - 2).max(1);
    let mut names: Vec<Vec<Seg>> = if q.name.is_empty() {
        Vec::new()
    } else {
        wrap(&q.name, room).into_iter().take(3).map(|l| vec![seg(format!(" {l}"), "")]).collect()
    };
    let mut list: Vec<Vec<Seg>> =
        q.list.iter().map(|l| vec![seg(" • ", fg(c().dim)), seg(ellipsis(l, (room - 3).max(1)), "")]).collect();
    let mut where_: Vec<Vec<Seg>> = q
        .where_
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(c().dim))])
        .collect();
    let mut note: Vec<Vec<Seg>> = q
        .note
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(c().dim))])
        .collect();
    // The buttons in rows that fit the card, 2 spaces apart (1 when that keeps them on one row).
    let labels: Vec<String> = q.buttons.iter().map(|b| format!(" {} {} ", key_label(&b.key), b.label)).collect();
    let widths: Vec<i64> = labels.iter().map(|l| text_width(l) as i64).collect();
    let total = |g: i64| widths.iter().sum::<i64>() + g * (labels.len() as i64 - 1);
    let gap = if total(2) <= inner - 2 || total(1) > inner - 2 { 2 } else { 1 };
    let mut bands: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0;
    for (i, &lw) in widths.iter().enumerate() {
        let band = bands.last_mut().expect("a band");
        if !band.is_empty() && used + gap + lw > inner - 2 {
            bands.push(vec![i]);
            used = lw;
        } else {
            used += if band.is_empty() { 0 } else { gap } + lw;
            band.push(i);
        }
    }
    // A low pane: the list goes first, then the same order as confirm's to leave things out, the buttons last.
    let (mut gaps, mut title) = (2, true);
    let fit = (h - 3).max(1);
    let nb = bands.len() as i64;
    macro_rules! over {
        () => {
            title as i64 + nb + gaps + (names.len() + list.len() + where_.len() + note.len()) as i64 > fit
        };
    }
    while over!() && !list.is_empty() {
        list.pop();
    }
    while over!() && gaps > 0 {
        gaps -= 1;
    }
    if over!() {
        where_.clear();
    }
    while over!() && names.len() > 1 {
        names.pop();
    }
    if over!() {
        note.clear();
    }
    if over!() {
        names.clear();
    }
    if over!() {
        title = false;
    }
    let mut text: Vec<Vec<Seg>> = Vec::new();
    if title {
        text.push(vec![seg(format!(" {}", ellipsis(&q.title, room)), "\x1b[1m")]);
    }
    if gaps > 1 {
        text.push(Vec::new());
    }
    text.extend(names);
    text.extend(list);
    text.extend(where_);
    text.extend(note);
    let lit = format!("\x1b[1m{}", fg([255, 255, 255]));
    if gaps > 0 {
        text.push(Vec::new());
    }
    for band in &bands {
        let mut row = vec![seg("  ", "")];
        for (k, &i) in band.iter().enumerate() {
            let tone = &q.buttons[i].tone;
            let style = if i == q.focus {
                format!("{lit}{}", bg(button(tone).unwrap_or(c().button_on)))
            } else {
                format!("{}{}", fg(button_text(tone).unwrap_or(c().dim)), bg(c().button_off))
            };
            row.push(seg(if k > 0 { " ".repeat(gap as usize) } else { String::new() }, ""));
            row.push(seg(&labels[i], style));
        }
        text.push(row);
    }
    let rows_h = text.len() as i64 + 2;
    let (x, y) = card_at(w, h, inner, rows_h);
    card(lines, w, h, x, y, inner, &text);
    // Screen columns from 1: the card's edge, then 2 spaces before the first button; a button cut by the edge is
    // clickable up to it, one past it not at all.
    let end = x + inner + 1;
    q.boxed = Some((x + 1, x + inner + 2, y + 1, y + rows_h));
    let mut zones = Vec::new();
    for (r, band) in bands.iter().enumerate() {
        let by = y + rows_h - 1 - nb + r as i64;
        let mut bx = x + 4;
        if by < h {
            for &i in band {
                if bx <= end {
                    zones.push((bx, (bx + widths[i] - 1).min(end), q.buttons[i].key.clone(), by));
                }
                bx += widths[i] + gap;
            }
        }
    }
    q.zones = Some(zones);
}

/// A key for the question: ← → (and tab) move between the buttons ('move'), Enter and space press the focused one, a
/// button's key presses it, escape and ctrl+c give 'escape'; other keys nothing (None), the card staying.
pub fn ask_key(k: &str, q: &mut Ask) -> Option<String> {
    let n = q.buttons.len();
    if matches!(k, "left" | "right" | "tab" | "shift+tab") {
        // No buttons: no focus to move.
        if n > 0 {
            q.focus = (q.focus + if k == "left" || k == "shift+tab" { n - 1 } else { 1 }) % n;
        }
        return Some("move".into());
    }
    if k == "enter" || k == "space" {
        return q.buttons.get(q.focus).map(|b| b.key.clone());
    }
    if k == "escape" || k == "ctrl+c" {
        return Some("escape".into());
    }
    q.buttons.iter().find(|b| b.key == k).map(|b| b.key.clone())
}

/// A click: on a button, its key; out of the card, 'escape'; None for the rest.
pub fn ask_mouse(m: &Mouse, q: &Ask) -> Option<String> {
    if !m.press || m.drag || m.wheel != 0 || m.button != 0 {
        return None;
    }
    let hit = q.zones.iter().flatten().find(|(a, b, _, y)| m.y - 1 == *y && m.x >= *a && m.x <= *b);
    if let Some((_, _, key, _)) = hit {
        return Some(key.clone());
    }
    match q.boxed {
        Some((x0, x1, y0, y1)) if m.x >= x0 && m.x <= x1 && m.y >= y0 && m.y <= y1 => None,
        _ => Some("escape".into()),
    }
}
