//! Image preview: format and size read from the header (PNG, JPEG, GIF, BMP, WebP, TIFF, ICO, SVG), decoders for PNG,
//! JPEG, GIF, BMP, TIFF and ICO, the image drawn in half blocks or scaled for the Kitty graphics protocol, and the part
//! of an image that shows in scrolled text. WebP goes through dwebp, when installed, which turns it into a PNG.
//! Files are untrusted: sizes are checked before allocating, every loop is bounded and nothing panics. A read
//! past the end of a buffer gives 0.

mod bmp;
mod gif;
mod jpeg;
mod place;
mod png;
mod scale;
mod tiff;

pub use place::{Crop, Halves, Pic, Placed, crop, render, shown};
pub use scale::{Pixels, pixels};

use crate::core::exec::{self, Opts};
use crate::core::regex::Regex;
use crate::core::{paths, which};
use bmp::{decode_dib, decode_ico};
use gif::decode_gif;
use jpeg::decode_jpeg;
use png::decode_png;
use std::sync::LazyLock;
use std::time::Duration;
use tiff::decode_tiff;

pub const FORMATS: [&str; 15] =
    ["png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "bmp", "dib", "ico", "cur", "webp", "tif", "tiff", "svg"];

pub const MAX_BYTES: usize = 50 * 1024 * 1024;

const MAX_PIXELS: f64 = 50e6;

const MAX_RAW: usize = 256 * 1024 * 1024; // inflated PNG data: a few bytes of zlib can claim gigabytes

fn bad(w: f64, h: f64) -> bool {
    !(w > 0.0 && h > 0.0) || w * h > MAX_PIXELS
}

fn at(b: &[u8], i: usize) -> u32 {
    b.get(i).map_or(0, |&x| x as u32)
}

fn u16be(b: &[u8], i: usize) -> u32 {
    at(b, i) << 8 | at(b, i + 1)
}

fn u32be(b: &[u8], i: usize) -> u32 {
    at(b, i) << 24 | at(b, i + 1) << 16 | at(b, i + 2) << 8 | at(b, i + 3)
}

fn le16(b: &[u8], i: usize) -> u32 {
    at(b, i) | at(b, i + 1) << 8
}

fn le32(b: &[u8], i: usize) -> u32 {
    at(b, i) | at(b, i + 1) << 8 | at(b, i + 2) << 16 | at(b, i + 3) << 24
}

fn sle32(b: &[u8], i: usize) -> i32 {
    le32(b, i) as i32
}

// The bytes from i to i + n (fewer at the end), one character each.
fn ascii(b: &[u8], i: usize, n: usize) -> String {
    b.get(i..b.len().min(i.saturating_add(n))).unwrap_or(&[]).iter().map(|&c| c as char).collect()
}

fn is_png(b: &[u8]) -> bool {
    b.len() > 8 && b[0] == 0x89 && ascii(b, 1, 3) == "PNG"
}

fn is_tiff(b: &[u8]) -> bool {
    b.len() > 8 && (b[..4] == [0x49, 0x49, 42, 0] || b[..4] == [0x4d, 0x4d, 0, 42])
}

fn is_webp(b: &[u8]) -> bool {
    b.len() > 12 && ascii(b, 0, 4) == "RIFF" && ascii(b, 8, 4) == "WEBP"
}

/// Math.round: halves up.
fn round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// A number stored in a Uint8Array: truncated, modulo 256 (NaN and infinities give 0).
fn byte(x: f64) -> u8 {
    if x.is_finite() { (x.trunc() as i64).rem_euclid(256) as u8 } else { 0 }
}

/// What the header tells: the format, the size in pixels and a note (None when there is none).
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub format: &'static str,
    pub width: f64,
    pub height: f64,
    pub note: Option<String>,
}

// JPEG frame types other than baseline, extended sequential and progressive Huffman, which are not decoded.
fn jpeg_note(m: u32) -> Option<&'static str> {
    Some(match m {
        0xc3 | 0xc7 | 0xcb | 0xcf => "lossless",
        0xc5 | 0xcd => "hierarchical",
        0xc6 | 0xca | 0xce => "progressive",
        0xc9 => "arithmetic",
        _ => return None,
    })
}

/// The format and size from the first bytes, or None when not a known image or its size is unknown.
pub fn info(b: &[u8]) -> Option<Info> {
    sniff(b).filter(|m| m.width > 0.0 && m.height > 0.0)
}

fn sniff(b: &[u8]) -> Option<Info> {
    let n = b.len();
    if n < 12 {
        return None;
    }
    let info = |format, w: f64, h: f64| Some(Info { format, width: w, height: h, note: None });
    if is_png(b) {
        return if n < 24 { None } else { info("PNG", u32be(b, 16) as f64, u32be(b, 20) as f64) };
    }
    if b[0] == 0xff && b[1] == 0xd8 {
        let mut i = 2;
        while i + 9 < n {
            if b[i] != 0xff || b[i + 1] == 0xff {
                i += 1;
                continue;
            }
            let m = b[i + 1] as u32;
            if m == 0xd8 || m == 0x01 || (0xd0..=0xd7).contains(&m) {
                i += 2;
                continue;
            }
            if m == 0xd9 || m == 0xda {
                break;
            }
            if (0xc0..=0xcf).contains(&m) && m != 0xc4 && m != 0xc8 && m != 0xcc {
                let note = match jpeg_note(m) {
                    Some(n) => n.to_string(),
                    None if b[i + 4] != 8 => format!("{}-bit", b[i + 4]),
                    None => String::new(),
                };
                return Some(Info {
                    format: "JPEG",
                    width: u16be(b, i + 7) as f64,
                    height: u16be(b, i + 5) as f64,
                    note: Some(note),
                });
            }
            i += 2 + u16be(b, i + 2) as usize;
        }
        return None;
    }
    if ascii(b, 0, 4) == "GIF8" {
        return info("GIF", le16(b, 6) as f64, le16(b, 8) as f64);
    }
    if is_tiff(b) {
        let t = tiff_tags(b)?;
        let first = |tag| t.get(tag).map_or(0.0, |v| v.first().copied().unwrap_or(f64::NAN));
        return info("TIFF", first(256), first(257));
    }
    if b[0] == 0x42 && b[1] == 0x4d && n >= 26 {
        return if le32(b, 14) == 12 {
            info("BMP", le16(b, 18) as f64, le16(b, 20) as f64)
        } else {
            info("BMP", sle32(b, 18) as f64, (sle32(b, 22) as f64).abs())
        };
    }
    if ascii(b, 0, 4) == "RIFF" && ascii(b, 8, 4) == "WEBP" && n >= 30 {
        return match ascii(b, 12, 4).as_str() {
            "VP8 " => info("WebP", (le16(b, 26) & 0x3fff) as f64, (le16(b, 28) & 0x3fff) as f64),
            "VP8L" if b[20] == 0x2f => {
                let bits = le32(b, 21);
                info("WebP", ((bits & 0x3fff) + 1) as f64, ((bits >> 14 & 0x3fff) + 1) as f64)
            }
            "VP8X" => Some(Info {
                format: "WebP",
                width: (1 + (at(b, 24) | at(b, 25) << 8 | at(b, 26) << 16)) as f64,
                height: (1 + (at(b, 27) | at(b, 28) << 8 | at(b, 29) << 16)) as f64,
                note: Some(if b[20] & 2 != 0 { "animated" } else { "" }.into()),
            }),
            _ => None,
        };
    }
    if b[0] == 0 && b[1] == 0 && (b[2] == 1 || b[2] == 2) && b[3] == 0 {
        let e = ico_entry(b)?;
        return info(if b[2] == 1 { "ICO" } else { "CUR" }, e.w as f64, e.h as f64);
    }
    svg_size(b)
}

fn re(src: &str, flags: &str) -> Regex {
    Regex::new(src, flags).unwrap_or_else(|e| panic!("{src}: {e}"))
}

static SVG_START: LazyLock<Regex> = LazyLock::new(|| re("^(\u{ef}\u{bb}\u{bf})?\\s*<", ""));

static SVG_ATTRS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    ["width", "height", "viewBox"].map(|name| re(&format!(r#"\s{name}\s*=\s*["']([^"']*)["']"#), "i"))
});

static SVG_LENGTH: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*([\d.]+(?:e[+-]?\d+)?)\s*(px|pt|pc|mm|cm|in|)\s*$", "i"));

static SVG_SEP: LazyLock<Regex> = LazyLock::new(|| re(r"[\s,]+", ""));

// The first match of /<svg\b[^>]*>/i, found by hand: the regex engine would retry from each "<svg" to the end of the
// text. When the first "<svg" has no ">" after it, none further on has.
fn svg_tag(head: &str) -> Option<&str> {
    let b = head.as_bytes();
    let word = |i: usize| b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
    let start = (0..b.len()).find(|&i| {
        b[i] == b'<' && b.get(i + 1..i + 4).is_some_and(|s| s.eq_ignore_ascii_case(b"svg")) && !word(i + 4)
    })?;
    let end = start + head[start..].find('>')?;
    Some(&head[start..=end])
}

// SVG size from the width / height attributes of the root element, else its viewBox.
fn svg_size(b: &[u8]) -> Option<Info> {
    let head: String = b[..b.len().min(65536)].iter().map(|&c| c as char).collect();
    if !SVG_START.test(&head) {
        return None;
    }
    let tag = svg_tag(&head)?;
    let attr = |k: usize| SVG_ATTRS[k].exec(tag).and_then(|m| m.get(1).map(str::to_string));
    let length = |v: Option<String>| {
        let v = v.unwrap_or_default();
        let Some(m) = SVG_LENGTH.exec(&v) else { return 0.0 };
        let unit = match m.get(2).unwrap_or("").to_lowercase().as_str() {
            "pt" => 4.0 / 3.0,
            "pc" => 16.0,
            "mm" => 96.0 / 25.4,
            "cm" => 96.0 / 2.54,
            "in" => 96.0,
            _ => 1.0,
        };
        let x = m.get(1).unwrap_or("").trim().parse::<f64>().unwrap_or(f64::NAN) * unit;
        if x.is_nan() || x == 0.0 { 0.0 } else { x }
    };
    let (mut w, mut h) = (length(attr(0)), length(attr(1)));
    let vb_text = attr(2).unwrap_or_default();
    let vb: Vec<f64> = SVG_SEP
        .split(vb_text.trim_matches(crate::unicode::is_space))
        .into_iter()
        .map(|s| s.unwrap_or("").trim().parse::<f64>().unwrap_or(f64::NAN))
        .collect();
    if vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0 {
        if w == 0.0 && h == 0.0 {
            (w, h) = (vb[2], vb[3]);
        } else if w == 0.0 {
            w = h * vb[2] / vb[3];
        } else if h == 0.0 {
            h = w * vb[3] / vb[2];
        }
    }
    Some(Info { format: "SVG", width: round(w), height: round(h), note: None })
}

struct IcoEntry {
    w: u32,
    h: u32,
    bpp: u32,
    size: u32,
    off: u32,
}

// The largest image of an ICO / CUR directory (then the deepest colours).
fn ico_entry(b: &[u8]) -> Option<IcoEntry> {
    let count = le16(b, 4) as usize;
    let mut best: Option<IcoEntry> = None;
    let mut k = 0;
    while k < count && 6 + 16 * k + 16 <= b.len() {
        let e = 6 + 16 * k;
        let w = if b[e] == 0 { 256 } else { b[e] as u32 };
        let h = if b[e + 1] == 0 { 256 } else { b[e + 1] as u32 };
        let bpp = le16(b, e + 6);
        if best.as_ref().is_none_or(|x| w * h > x.w * x.h || (w * h == x.w * x.h && bpp > x.bpp)) {
            best = Some(IcoEntry { w, h, bpp, size: le32(b, e + 8), off: le32(b, e + 12) });
        }
        k += 1;
    }
    best
}

// The tags read from a TIFF, and how many values they may hold together: every tag kept, each up to 2^24
// values, would fill memory with a small file.
const TIFF_TAGS: [u32; 13] = [256, 257, 258, 259, 262, 273, 277, 278, 279, 284, 317, 320, 338];

const MAX_TIFF_VALUES: usize = 1 << 22;

/// The tags of a TIFF's first directory that are read, in the order first met (a tag met again keeps its place, as in
/// a Map).
pub(crate) struct Tags {
    pub tags: Vec<(u32, Vec<f64>)>,
    pub le: bool,
}

impl Tags {
    pub fn get(&self, tag: u32) -> Option<&Vec<f64>> {
        self.tags.iter().find(|(t, _)| *t == tag).map(|(_, v)| v)
    }
}

pub(crate) fn tiff_tags(b: &[u8]) -> Option<Tags> {
    let le = b[0] == 0x49;
    let r16 = |i: usize| if le { le16(b, i) } else { u16be(b, i) };
    let r32 = |i: usize| if le { le32(b, i) } else { u32be(b, i) };
    let dir = r32(4) as usize;
    let count = if dir + 2 <= b.len() { r16(dir) as usize } else { 0 };
    if count == 0 || count > 1000 {
        return None;
    }
    let mut tags: Vec<(u32, Vec<f64>)> = Vec::new();
    for k in 0..count {
        let e = dir + 2 + k * 12;
        if e + 12 > b.len() {
            break;
        }
        let (tag, kind, n) = (r16(e), r16(e + 2), r32(e + 4) as usize);
        let size = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 => 4,
            5 => 8,
            _ => continue,
        };
        let kept: usize = tags.iter().filter(|(t, _)| *t != tag).map(|(_, v)| v.len()).sum();
        if n > 1 << 24 || !TIFF_TAGS.contains(&tag) || kept + n > MAX_TIFF_VALUES {
            continue;
        }
        let start = if n * size <= 4 { e + 8 } else { r32(e + 8) as usize };
        if start + n * size > b.len() {
            continue;
        }
        let values = (0..n)
            .map(|i| {
                let p = start + i * size;
                match size {
                    1 => b[p] as f64,
                    2 => r16(p) as f64,
                    4 => r32(p) as f64,
                    _ => r32(p) as f64 / (if r32(p + 4) == 0 { 1 } else { r32(p + 4) }) as f64,
                }
            })
            .collect();
        match tags.iter_mut().find(|(t, _)| *t == tag) {
            Some(slot) => slot.1 = values,
            None => tags.push((tag, values)),
        }
    }
    Some(Tags { tags, le })
}

/// A decoded image: its size and RGBA bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// The pixels of an image, or None when the format is not supported or the file is broken.
pub fn decode(b: &[u8]) -> Option<Image> {
    if b.len() < 12 || b.len() > MAX_BYTES {
        return None;
    }
    if is_png(b) {
        return decode_png(b);
    }
    if b[0] == 0xff && b[1] == 0xd8 {
        return decode_jpeg(b);
    }
    if ascii(b, 0, 4) == "GIF8" {
        return decode_gif(b);
    }
    if b[0] == 0x42 && b[1] == 0x4d {
        return decode_dib(b, 14, Some(le32(b, 10) as usize), false);
    }
    if is_tiff(b) {
        return decode_tiff(b);
    }
    if b[0] == 0 && b[1] == 0 && (b[2] == 1 || b[2] == 2) && b[3] == 0 {
        return decode_ico(b);
    }
    None
}

/// The image as a PNG made by dwebp (libwebp), when the file is a WebP and dwebp is installed; None otherwise. The
/// path is absolute, so that it never reads as an option.
pub fn convert(file: &str, buffer: &[u8]) -> Option<Vec<u8>> {
    if !is_webp(buffer) || !paths::is_absolute(file) {
        return None;
    }
    let dwebp = which::which("dwebp")?;
    let opts = Opts { timeout: Some(Duration::from_secs(20)), max_buffer: Some(MAX_RAW), ..Opts::default() };
    exec::run_bytes(Some(&dwebp), &["-quiet", file, "-o", "-"], &opts).filter(|out| is_png(out))
}
