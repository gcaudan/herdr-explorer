//! The image drawn in half blocks, and the part of an image that shows in scrolled text.

use super::jpeg::int32;
use super::scale::{Boxes, ICON, decoded, fit};
use super::{Info, round};
use crate::core::ui::{self, Rgb, Seg};
use crate::json;

// What shows instead of an image: its format and size when known, and why (an empty note counts as none).
pub(super) fn describe(meta: Option<&Info>, why: &str) -> String {
    match meta {
        Some(m) => format!(
            "{} {}×{} ({}{why})",
            m.format,
            json::number(m.width),
            json::number(m.height),
            m.note.as_deref().filter(|n| !n.is_empty()).map_or(String::new(), |n| format!("{n}, "))
        ),
        None => format!("Unknown image format ({why})"),
    }
}

/// Half blocks of an image: its lines of segments, their size in cells, the size of the image itself, its format
/// ("" when the header says nothing).
#[derive(Debug, Clone, PartialEq)]
pub struct Halves {
    pub lines: Vec<Vec<Seg>>,
    pub width: usize,
    pub height: usize,
    pub image_width: usize,
    pub image_height: usize,
    pub format: &'static str,
}

/// Lines of half blocks fitting in cols × rows cells, over background (the theme's canvas by default); Err with
/// what to show when the image cannot be.
pub fn render(buffer: &[u8], cols: f64, rows: f64, background: Option<Rgb>) -> Result<Halves, String> {
    let (meta, img) = decoded(buffer)?;
    let (w, h) = (img.width, img.height);
    let (tw, th) = fit(w as f64, h as f64, (int32(cols) as f64).max(1.0), (int32(rows) as f64).max(1.0) * 2.0, ICON);

    // Box filter: each target pixel averages its source area, colours weighted by alpha, then blended over the
    // background.
    let [br, bgc, bb] = background.unwrap_or_else(|| ui::c().canvas);
    let mut boxes = Boxes::new(w, tw);
    let mut pix = vec![0u32; tw * th];
    for y in 0..th {
        let rows = boxes.row(&img, y, th);
        for x in 0..tw {
            let q = x * 4;
            let n = 255.0 * ((boxes.x1[x] - boxes.x0[x]) * rows) as f64;
            let rest = n - boxes.acc[q + 3];
            let mix = |k: usize, back: u8| round((boxes.acc[q + k] + back as f64 * rest) / n) as u32;
            pix[y * tw + x] = mix(0, br) << 16 | mix(1, bgc) << 8 | mix(2, bb);
        }
    }

    // One cell: top pixel as foreground of '▀', bottom pixel as background; equal neighbours merged.
    let back = (br as u32) << 16 | (bgc as u32) << 8 | bb as u32;
    let rgb = |c: i64| [(c >> 16 & 255) as u8, (c >> 8 & 255) as u8, (c & 255) as u8];
    let mut lines = Vec::new();
    for y in (0..th).step_by(2) {
        let mut segs: Vec<Seg> = Vec::new();
        let (mut run, mut top, mut bottom) = (0, -1i64, -1i64);
        for x in 0..=tw {
            let (t, u) = if x < tw {
                (pix[y * tw + x] as i64, if y + 1 < th { pix[(y + 1) * tw + x] } else { back } as i64)
            } else {
                (-1, -1)
            };
            if t == top && u == bottom {
                run += 1;
                continue;
            }
            if run != 0 {
                segs.push(("▀".repeat(run), ui::fg(rgb(top)) + &ui::bg(rgb(bottom))));
            }
            (top, bottom, run) = (t, u, 1);
        }
        lines.push(segs);
    }
    let height = lines.len();
    Ok(Halves { lines, width: tw, height, image_width: w, image_height: h, format: meta.map_or("", |m| m.format) })
}

/// The part of an image that shows: at the body row at (from 0), its rows from to from + count - 1, holding its
/// pixels y to y + h - 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub at: i64,
    pub from: i64,
    pub count: i64,
    pub y: i64,
    pub h: i64,
}

/// The part of an image rows cells high, its first row at row start of the text, that shows when the rows top to
/// top + height - 1 do (the rows stretch the image's pixels evenly, whatever the cell size); None when none shows.
pub fn crop(start: i64, rows: i64, pixels: i64, top: i64, height: i64) -> Option<Crop> {
    let (from, to) = ((top - start).max(0), rows.min(top.saturating_add(height).saturating_sub(start)));
    if rows <= 0 || to <= from {
        return None;
    }
    let y = round(from as f64 * pixels as f64 / rows as f64);
    let h = (round(to as f64 * pixels as f64 / rows as f64) - y).max(1.0);
    Some(Crop { at: start + from - top, from, count: to - from, y: y as i64, h: h as i64 })
}

/// An image placed over rows of a scrolled text: how many rows, how many pixels high.
pub trait Pic {
    fn rows(&self) -> i64;
    fn pixels(&self) -> i64;
}

/// An image showing among the rows of a text: where it starts in the text (which tells two copies apart), its crop.
pub struct Placed<'a, P> {
    pub pic: &'a P,
    pub start: i64,
    pub part: Crop,
}

/// The images showing among the rows top to top + height - 1 of a list, each row of an image carrying the image and
/// the row of it. Each image once, in the order met.
pub fn shown<'a, P: Pic>(list: &[Option<(&'a P, i64)>], top: i64, height: i64) -> Vec<Placed<'a, P>> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let end = (list.len() as i64).min(top.saturating_add(height));
    for i in top.max(0)..end {
        let Some((pic, row)) = list[i as usize] else { continue };
        let start = i - row;
        if seen.contains(&start) {
            continue;
        }
        seen.push(start);
        if let Some(part) = crop(start, pic.rows(), pic.pixels(), top, height) {
            out.push(Placed { pic, start, part });
        }
    }
    out
}
