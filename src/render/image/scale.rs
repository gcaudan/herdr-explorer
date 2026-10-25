//! Scaling: the image boxed down to the cells or pixels it is shown in.

use super::jpeg::int32;
use super::place::describe;
use super::{Image, Info, MAX_BYTES, MAX_PIXELS, decode, info, round};

pub(super) const ICON: f64 = 32.0; // small images are enlarged up to this many pixels on their longer side

// The size of an image scaled to fit max_w × max_h: never enlarged, except small icons by a whole factor (up to 4)
// until their longer side reaches icon.
pub(super) fn fit(w: f64, h: f64, max_w: f64, max_h: f64, icon: f64) -> (usize, usize) {
    let mut scale = (max_w / w).min(max_h / h);
    if scale >= 1.0 {
        scale = scale.floor().min((icon / w.max(h)).floor()).clamp(1.0, 4.0);
    }
    (round(w * scale).min(max_w).max(1.0) as usize, round(h * scale).min(max_h).max(1.0) as usize)
}

// The sums over the source area of each target pixel of a row (box filter): colours weighted by alpha, and alpha;
// with the number of source pixels.
pub(super) struct Boxes {
    pub(super) x0: Vec<usize>,
    pub(super) x1: Vec<usize>,
    pub(super) acc: Vec<f64>,
}

impl Boxes {
    pub(super) fn new(w: usize, tw: usize) -> Boxes {
        let x0: Vec<usize> = (0..tw).map(|x| x * w / tw).collect();
        let x1 = (0..tw).map(|x| (x0[x] + 1).max((x + 1) * w / tw)).collect();
        Boxes { x0, x1, acc: vec![0.0; tw * 4] }
    }

    // Row y of th from an image h rows high: the sums, and the source rows (y0, y1).
    pub(super) fn row(&mut self, img: &Image, y: usize, th: usize) -> usize {
        let (w, h) = (img.width, img.height);
        let y0 = y * h / th;
        let y1 = (y0 + 1).max((y + 1) * h / th);
        self.acc.fill(0.0);
        for sy in y0..y1 {
            let row = sy * w * 4;
            for x in 0..self.x0.len() {
                let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
                for p in (row + self.x0[x] * 4..row + self.x1[x] * 4).step_by(4) {
                    let al = img.data[p + 3] as f64;
                    r += img.data[p] as f64 * al;
                    g += img.data[p + 1] as f64 * al;
                    b += img.data[p + 2] as f64 * al;
                    a += al;
                }
                let q = x * 4;
                self.acc[q] += r;
                self.acc[q + 1] += g;
                self.acc[q + 2] += b;
                self.acc[q + 3] += a;
            }
        }
        y1 - y0
    }
}

// RGBA pixels of a decoded image at tw × th: each target pixel averages its source area (box filter), colours
// weighted by alpha.
fn scale(img: &Image, tw: usize, th: usize) -> Vec<u8> {
    let mut boxes = Boxes::new(img.width, tw);
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let rows = boxes.row(img, y, th);
        for x in 0..tw {
            let (q, o) = (x * 4, (y * tw + x) * 4);
            let n = ((boxes.x1[x] - boxes.x0[x]) * rows) as f64;
            let a = boxes.acc[q + 3];
            if a != 0.0 {
                for k in 0..3 {
                    out[o + k] = round(boxes.acc[q + k] / a) as u8;
                }
            }
            out[o + 3] = round(a / n) as u8;
        }
    }
    out
}

/// An image scaled for the Kitty graphics protocol: RGBA pixels, and the size of the image itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
    pub image_width: usize,
    pub image_height: usize,
}

// The checks before decoding, and the decoded image; Err with what to show instead.
pub(super) fn decoded(buffer: &[u8]) -> Result<(Option<Info>, Image), String> {
    let meta = info(buffer);
    if buffer.is_empty() {
        return Err("Empty file".into());
    }
    if buffer.len() > MAX_BYTES {
        return Err(describe(meta.as_ref(), "file too large for preview"));
    }
    if meta.as_ref().is_some_and(|m| m.width * m.height > MAX_PIXELS) {
        return Err(describe(meta.as_ref(), "too large for preview"));
    }
    match decode(buffer) {
        Some(img) => Ok((meta, img)),
        None => Err(describe(meta.as_ref(), "no preview")),
    }
}

/// The image scaled to fit max_w × max_h pixels, for the Kitty graphics protocol; Err with what to show when it
/// cannot be.
pub fn pixels(buffer: &[u8], max_w: f64, max_h: f64) -> Result<Pixels, String> {
    let (_, img) = decoded(buffer)?;
    let (w, h) = (img.width, img.height);
    let (tw, th) = fit(w as f64, h as f64, (int32(max_w) as f64).max(1.0), (int32(max_h) as f64).max(1.0), 128.0);
    let data = if tw == w && th == h { img.data } else { scale(&img, tw, th) };
    Ok(Pixels { width: tw, height: th, data, image_width: w, image_height: h })
}
