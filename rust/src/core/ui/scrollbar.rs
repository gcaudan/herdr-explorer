//! The scrollbar of a list: its marks, its ▴ ▾ buttons held to repeat, and dragging its thumb.

use super::input::Mouse;
use super::theme::c;
use super::{Rgb, fg, round};
use std::time::{Duration, Instant};

/// A row to show on a scrollbar: a cell of the rail or thumb takes the colour of the heaviest mark of the rows it
/// stands for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarMark {
    pub row: i64,
    pub color: Rgb,
    pub weight: f64,
}

/// The bar as last set: rows shown, the largest top, the top, whether it has ▴ ▾ buttons (1 or 0), the rows of its
/// track, the thumb's size and row, the mark of each track row.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub height: i64,
    pub max: i64,
    pub top: i64,
    pub arrows: i64,
    pub track: i64,
    pub size: i64,
    pub at: i64,
    pub tints: Vec<Option<BarMark>>,
}

/// What the mouse holds: a button (step -1 or 1), repeating from due, or the thumb, grabbed on row k at top.
#[derive(Debug, Clone, PartialEq)]
pub enum Held {
    Step { k: i64, step: i64, last: Option<i64>, due: Option<Instant> },
    Thumb { k: i64, top: i64 },
}

/// What a mouse event did to the bar: not its own, taken, or taken with the list to scroll to a top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHit {
    Ignored,
    Took,
    Scroll(i64),
}

/// A held button repeats 400 ms after the press, then every 50 ms.
const REPEAT_AFTER: Duration = Duration::from_millis(450);

const REPEAT_EVERY: Duration = Duration::from_millis(50);

/// The scrollbar of a list shown height rows at a time, in a column on its right, hidden while the list fits: a thin
/// rail, its thumb, and ▴ ▾ buttons at its ends when there is room. One per list, set again at each render; it keeps a
/// drag or a held button going from one mouse event to the next. The list scrolls by what mouse and tick return; the
/// pane lets go (stop) when it loses the focus.
#[derive(Debug, Clone, Default)]
pub struct Scrollbar {
    pub bar: Option<Bar>,
    pub held: Option<Held>,
}

impl Scrollbar {
    pub fn new() -> Self {
        Self::default()
    }

    /// The list's length, the rows shown and the first shown; returns the columns the bar takes: 1, or 0 when hidden.
    pub fn set(&mut self, total: i64, height: i64, top: i64, marks: &[BarMark]) -> usize {
        self.bar = None;
        if height < 2 || total <= height {
            return 0;
        }
        let max = total - height;
        let arrows = if height >= 4 { 1 } else { 0 };
        let track = height - 2 * arrows;
        let size = 1.max((track - 1).min(round((track * height) as f64 / total as f64) as i64));
        let top = top.min(max).max(0);
        let mut tints: Vec<Option<BarMark>> = vec![None; track as usize];
        for m in marks {
            let t = (track - 1).min((m.row as f64 * track as f64 / total as f64).floor() as i64);
            // A mark before the list: tints[-1] is a property the JS's array never shows.
            let Some(cell) = usize::try_from(t).ok().and_then(|t| tints.get_mut(t)) else { continue };
            if cell.is_none_or(|c| c.weight < m.weight) {
                *cell = Some(*m);
            }
        }
        let at = arrows + round(((track - size) * top) as f64 / max as f64) as i64;
        self.bar = Some(Bar { height, max, top, arrows, track, size, at, tints });
        1
    }

    /// The bar's cell on row k of the list (from 0), to add after the row's line; nothing when hidden.
    pub fn cell(&self, k: i64) -> String {
        let Some(b) = &self.bar else { return String::new() };
        if b.arrows == 1 && k == 0 {
            return format!("{}▴\x1b[0m", fg(c().dim));
        }
        if b.arrows == 1 && k == b.height - 1 {
            return format!("{}▾\x1b[0m", fg(c().dim));
        }
        let thumb = k >= b.at && k < b.at + b.size;
        let tint = usize::try_from(k - b.arrows).ok().and_then(|i| b.tints.get(i)).copied().flatten();
        match (tint, thumb) {
            (Some(t), _) => format!("{}{}\x1b[0m", fg(t.color), if thumb { '┃' } else { '│' }),
            (None, true) => format!("{}┃\x1b[0m", fg(c().thumb)),
            (None, false) => format!("{}│\x1b[0m", fg(c().rail)),
        }
    }

    /// The top that puts the thumb at row r, kept within the list.
    pub fn top_at(&self, r: i64) -> i64 {
        let Some(b) = &self.bar else { return 0 };
        0.max(b.max.min(round(((r - b.arrows) * b.max) as f64 / 1.max(b.track - b.size) as f64) as i64))
    }

    /// The next top of a held button, or None when the repeat stops: at an end of the list, or when the list did not
    /// take the last move (gone from the screen).
    fn step(&mut self) -> Option<i64> {
        let Some(Held::Step { step, last, .. }) = &mut self.held else { return None };
        let b = self.bar.as_ref()?;
        let top = 0.max(b.max.min(b.top + *step));
        if Some(top) == *last || top == b.top {
            return None;
        }
        *last = Some(top);
        Some(top)
    }

    /// A mouse event on row k of the list (from 0); on: whether it is on the bar's column; now, its time. A press on
    /// the thumb grabs it, the drag going on wherever the mouse goes until the release; elsewhere on the rail, the
    /// thumb first jumps under the mouse. A button scrolls one row, then again while held on it (tick).
    pub fn mouse(&mut self, m: &Mouse, k: i64, on: bool, now: Instant) -> BarHit {
        if m.wheel != 0 {
            return BarHit::Ignored;
        }
        if self.held.is_some() && !m.press {
            self.stop();
            return BarHit::Took;
        }
        if m.drag
            && let Some(h) = &mut self.held
        {
            match h {
                Held::Step { k: hk, due, .. } => {
                    if !on || k != *hk {
                        *due = None;
                    }
                }
                Held::Thumb { k: hk, top } => {
                    if let Some(b) = &self.bar {
                        let moved = round(((k - *hk) * b.max) as f64 / 1.max(b.track - b.size) as f64) as i64;
                        return BarHit::Scroll(0.max(b.max.min(*top + moved)));
                    }
                }
            }
            return BarHit::Took;
        }
        self.stop();
        let Some(b) = &self.bar else { return BarHit::Ignored };
        if !on || !m.press || m.drag || m.button != 0 || k < 0 || k >= b.height {
            return BarHit::Ignored;
        }
        let step = if b.arrows == 1 && k == 0 {
            -1
        } else if b.arrows == 1 && k == b.height - 1 {
            1
        } else {
            0
        };
        if step != 0 {
            self.held = Some(Held::Step { k, step, last: None, due: Some(now + REPEAT_AFTER) });
            return self.step().map_or(BarHit::Took, BarHit::Scroll);
        }
        let top = if k >= b.at && k < b.at + b.size { b.top } else { self.top_at(k - b.size / 2) };
        let was = b.top;
        self.held = Some(Held::Thumb { k, top });
        if top != was { BarHit::Scroll(top) } else { BarHit::Took }
    }

    /// When a held button repeats next, for the pane's wait.
    pub fn due(&self) -> Option<Instant> {
        match &self.held {
            Some(Held::Step { due, .. }) => *due,
            _ => None,
        }
    }

    /// A held button's repeat, once due: the top to scroll to, or None (not due, or stopped).
    pub fn tick(&mut self, now: Instant) -> Option<i64> {
        if self.due().is_none_or(|d| d > now) {
            return None;
        }
        let top = self.step();
        if let Some(Held::Step { due, .. }) = &mut self.held {
            *due = top.map(|_| now + REPEAT_EVERY);
        }
        top
    }

    /// Lets go of what the mouse held.
    pub fn stop(&mut self) {
        self.held = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(press: bool, drag: bool) -> Mouse {
        Mouse { press, drag, ..Mouse::default() }
    }

    /// A held ▾ scrolls at once, again 450 ms later (the JS's 400 ms timeout, then its first 50 ms interval), then
    /// every 50 ms, until the release; the list not taking a move stops it.
    #[test]
    fn a_held_button_repeats_until_released() {
        let mut bar = Scrollbar::new();
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        bar.set(100, 10, 0, &[]);
        assert_eq!(bar.mouse(&press(true, false), 9, true, t), BarHit::Scroll(1));
        bar.set(100, 10, 1, &[]);
        assert_eq!(bar.due(), Some(ms(450)));
        assert_eq!(bar.tick(ms(449)), None);
        assert_eq!(bar.tick(ms(450)), Some(2));
        bar.set(100, 10, 2, &[]);
        assert_eq!(bar.tick(ms(499)), None);
        assert_eq!(bar.tick(ms(500)), Some(3));
        // Not scrolled (the list gone from the screen): the repeat stops.
        assert_eq!(bar.tick(ms(550)), None);
        assert_eq!(bar.due(), None);
        assert_eq!(bar.mouse(&press(false, false), 9, true, ms(600)), BarHit::Took);
        assert_eq!(bar.held, None);
    }

    #[test]
    fn a_button_left_while_held_stops_repeating() {
        let mut bar = Scrollbar::new();
        let t = Instant::now();
        bar.set(100, 10, 50, &[]);
        assert_eq!(bar.mouse(&press(true, false), 0, true, t), BarHit::Scroll(49));
        bar.set(100, 10, 49, &[]);
        assert_eq!(bar.mouse(&press(true, true), 1, true, t), BarHit::Took);
        assert_eq!(bar.due(), None);
        assert_eq!(bar.tick(t + Duration::from_secs(1)), None);
        // Back on the button: still stopped, as the JS's cleared timer.
        assert_eq!(bar.mouse(&press(true, true), 0, true, t), BarHit::Took);
        assert_eq!(bar.due(), None);
    }

    #[test]
    fn the_repeat_stops_at_an_end_of_the_list() {
        let mut bar = Scrollbar::new();
        let t = Instant::now();
        bar.set(11, 10, 1, &[]);
        assert_eq!(bar.mouse(&press(true, false), 9, true, t), BarHit::Took);
        assert_eq!(bar.tick(t + Duration::from_millis(450)), None);
        assert_eq!(bar.due(), None);
    }
}
