//! Menus: their items, keys, mouse and drawing.

use super::input::Mouse;
use super::text::{ellipsis, head, line, tail, text_width};
use super::theme::c;
use super::{Seg, fg, seg};

/// An item of a menu: its label; key, the key that does the same outside the menu, dimmed after the label ("" for
/// none); value, (text, style) on the right, as in the explorer's settings (on_off gives ● on / ○ off); danger, in red;
/// keep, the menu stays open after it runs (an option changed in place); run, what the caller runs.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem<T> {
    pub label: String,
    pub key: String,
    pub value: Option<Seg>,
    pub danger: bool,
    pub keep: bool,
    pub run: T,
}

/// What a key or a click chose: close the menu, or run an item (keep: show the menu again after, at the same row).
#[derive(Debug, Clone, PartialEq)]
pub enum Choice<T> {
    Close,
    Run { run: T, keep: bool },
}

/// A menu in a frame over a pane: items, None for a rule. at: the screen line (from 0) of its top border; right: its
/// right edge's column (from 1), else (0) it starts at the column left (from 1). The caller closes the menu on a
/// choice and runs the item, then shows the menu again at the same row when it keeps it.
#[derive(Debug, Clone)]
pub struct Menu<T> {
    pub title: String,
    pub items: Vec<Option<MenuItem<T>>>,
    pub at: i64,
    pub right: i64,
    pub left: i64,
    pub sel: usize,
    pub off: usize,
    pub x: i64,
    pub inner: i64,
}

impl<T: Clone> Menu<T> {
    pub fn new(title: &str, items: Vec<Option<MenuItem<T>>>, at: i64, right: i64, left: i64) -> Self {
        let sel = items.iter().position(Option::is_some).unwrap_or(0);
        Menu { title: title.into(), items, at, right, left, sel, off: 0, x: left, inner: 0 }
    }

    fn choice(it: &MenuItem<T>) -> Choice<T> {
        Choice::Run { run: it.run.clone(), keep: it.keep }
    }

    pub fn rows(&self, h: i64) -> usize {
        1.max((self.items.len() as i64).min(h - 2 - self.at)) as usize
    }

    /// Draws the menu over the lines of a screen w × h, what they showed kept on both sides of its frame.
    pub fn draw(&mut self, lines: &mut Vec<String>, w: i64, h: i64) {
        let n = self.rows(h);
        let edge = fg(c().dim);
        // An item: a space, its label, its key after two spaces, at least one space (two before a value), its value,
        // a space.
        let side = |it: &MenuItem<T>| {
            (if it.key.is_empty() { 0 } else { text_width(&it.key) + 2 })
                + it.value.as_ref().map_or(0, |v| text_width(&v.0) + 1)
        };
        let want = self
            .items
            .iter()
            .map(|it| it.as_ref().map_or(0, |it| text_width(&it.label) + side(it) + 3))
            .fold(text_width(&self.title) + 4, usize::max) as i64;
        let inner = (w - 2 - if self.right != 0 { 1 } else { self.left }).min(want);
        self.inner = inner;
        // From its left column x (from 1): the frame takes inner + 2 columns after a space.
        let x = if self.right != 0 { 1.max(self.right - inner - 2) } else { self.left };
        self.x = x;
        let cols = |v: i64| v.max(0) as usize;
        // On both sides of the frame, what the line showed.
        let keep = |lines: &mut Vec<String>, i: usize| {
            if lines.len() <= i {
                lines.resize(i + 1, String::new());
            }
            // `lines[i] || line([], W)`: an empty line too.
            let blank;
            let l = if lines[i].is_empty() {
                blank = line(&[], cols(w), None);
                &blank
            } else {
                &lines[i]
            };
            let pad = if x > 1 { head(l, cols(x - 1)) } else { String::new() };
            (pad, tail(l, cols(x + inner + 1)))
        };
        let border = |pad: &str, rest: &str, l: char, r: char, t: &str, more: Option<char>| {
            let rule = "─".repeat(cols(inner - text_width(t) as i64 - if more.is_some() { 2 } else { 0 }));
            let more = more.map_or(String::new(), |m| format!("{m}─"));
            format!(
                "{pad}{}{rest}",
                line(&[seg(format!("{l}{t}{rule}{more}{r}"), edge.clone())], cols(inner + 2), None)
            )
        };
        let at = self.at.max(0) as usize;
        let (pad, rest) = keep(lines, at);
        let title = format!(" {} ", ellipsis(&self.title, 1.max(inner - 4)));
        lines[at] = border(&pad, &rest, '╭', '╮', &title, (self.off > 0).then_some('▴'));
        let shown: Vec<(usize, Option<MenuItem<T>>)> =
            self.items.iter().cloned().enumerate().skip(self.off).take(n).collect();
        for (k, (i, it)) in shown.into_iter().enumerate() {
            let row = at + 1 + k;
            let (pad, rest) = keep(lines, row);
            let Some(it) = it else {
                lines[row] = format!(
                    "{pad}{}{rest}",
                    line(&[seg(format!("├{}┤", "─".repeat(cols(inner))), edge.clone())], cols(inner + 2), None)
                );
                continue;
            };
            let key = if it.key.is_empty() { String::new() } else { format!("  {}", it.key) };
            let (value, style) = it.value.clone().unwrap_or_default();
            let room =
                inner - 3 - text_width(&key) as i64 - if value.is_empty() { 0 } else { text_width(&value) as i64 + 1 };
            let label = ellipsis(&it.label, 1.max(room));
            let gap =
                " ".repeat(
                    1.max(inner - 2 - (text_width(&label) + text_width(&key) + text_width(&value)) as i64) as usize
                );
            let segs = [
                seg(format!(" {label}"), if it.danger { fg(c().red) } else { String::new() }),
                seg(key, fg(c().dim)),
                seg(gap, ""),
                seg(format!("{value} "), style),
            ];
            let back = if i == self.sel { c().select } else { c().select_blur };
            lines[row] = format!("{pad}{edge}│\x1b[39m{}{edge}│\x1b[0m{rest}", line(&segs, cols(inner), Some(back)));
        }
        let last = at + n + 1;
        let (pad, rest) = keep(lines, last);
        lines[last] = border(&pad, &rest, '╰', '╯', "", (self.off + n < self.items.len()).then_some('▾'));
    }

    /// Moves the selection by dir, over the rules, round the ends.
    pub fn move_by(&mut self, dir: i64, h: i64) {
        if !self.items.iter().any(Option::is_some) {
            return;
        }
        let len = self.items.len() as i64;
        loop {
            self.sel = (self.sel as i64 + dir).rem_euclid(len) as usize;
            if self.items[self.sel].is_some() {
                break;
            }
        }
        let n = self.rows(h);
        if self.sel < self.off {
            self.off = self.sel;
        }
        if self.sel >= self.off + n {
            self.off = self.sel - n + 1;
        }
    }

    /// A key: up and down (and k j) move, enter and space run the selection, left and right (h l) an option's, the
    /// key shown beside an item runs it; escape, q and , (which opens the menus of the panes) close. None when the
    /// menu took the key, or nothing came of it.
    pub fn key(&mut self, k: &str, h: i64) -> Option<Choice<T>> {
        if matches!(k, "escape" | "q" | ",") {
            return Some(Choice::Close);
        }
        // ← → change the selected option, as in the explorer's settings; on an action (no value), they do nothing,
        // so that an arrow never pushes or checks out.
        if matches!(k, "left" | "right" | "h" | "l") {
            return self.items.get(self.sel).and_then(Option::as_ref).filter(|it| it.value.is_some()).map(Self::choice);
        }
        if k == "up" || k == "k" {
            self.move_by(-1, h);
            return None;
        }
        if k == "down" || k == "j" {
            self.move_by(1, h);
            return None;
        }
        if k == "enter" || k == "space" {
            return self.items.get(self.sel).and_then(Option::as_ref).map(Self::choice);
        }
        self.items.iter().flatten().find(|it| !it.key.is_empty() && it.key == k).map(Self::choice)
    }

    /// A click: on an item, it runs; elsewhere, the menu closes. The wheel moves the selection.
    pub fn mouse(&mut self, m: &Mouse, h: i64) -> Option<Choice<T>> {
        if m.wheel != 0 {
            self.move_by(m.wheel, h);
            return None;
        }
        if !m.press || m.drag {
            return None;
        }
        let k = m.y - 2 - self.at;
        if m.button == 0 && k >= 0 && k < self.rows(h) as i64 && m.x > self.x && m.x <= self.x + self.inner + 1 {
            return self.items.get(self.off + k as usize).and_then(Option::as_ref).map(Self::choice);
        }
        Some(Choice::Close)
    }
}
