//! Image preview: format and size read from the header (PNG, JPEG, GIF, BMP, WebP, TIFF, ICO, SVG), decoders for PNG,
//! JPEG, GIF, BMP, TIFF and ICO, the image drawn in half blocks or scaled for the Kitty graphics protocol, and the part
//! of an image that shows in scrolled text. WebP goes through dwebp, when installed, which turns it into a PNG.
//! Files are untrusted: sizes are checked before allocating, every loop is bounded and nothing panics. Where the JS
//! reads past the end of a buffer (undefined, 0 in bitwise operations), the reads here give 0.

use crate::codec::inflate::inflate_zlib;
use crate::core::exec::{self, Opts};
use crate::core::regex::Regex;
use crate::core::ui::{self, Rgb, Seg};
use crate::core::{js, paths, which};
use crate::json;
use std::sync::LazyLock;
use std::time::Duration;

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

// --- header

/// What the header tells: the format, the size in pixels and a note (`note` absent where the JS has no key).
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
        let unit = match js::lower(m.get(2).unwrap_or("")).as_str() {
            "pt" => 4.0 / 3.0,
            "pc" => 16.0,
            "mm" => 96.0 / 25.4,
            "cm" => 96.0 / 2.54,
            "in" => 96.0,
            _ => 1.0,
        };
        let x = js::string_to_number(m.get(1).unwrap_or("")) * unit;
        if x.is_nan() || x == 0.0 { 0.0 } else { x }
    };
    let (mut w, mut h) = (length(attr(0)), length(attr(1)));
    let vb_text = attr(2).unwrap_or_default();
    let vb: Vec<f64> = SVG_SEP
        .split(vb_text.trim_matches(js::is_space))
        .into_iter()
        .map(|s| js::string_to_number(s.unwrap_or("")))
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

// The tags read from a TIFF, and how many values they may hold together: the JS keeps every tag, each up to 2^24
// values, enough to fill memory with a small file.
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

// --- decoding

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

// --- PNG

const ADAM7: [[usize; 4]; 7] =
    [[0, 0, 8, 8], [4, 0, 8, 8], [0, 4, 4, 8], [2, 0, 4, 4], [0, 2, 2, 4], [1, 0, 2, 2], [0, 1, 1, 2]];

fn decode_png(b: &[u8]) -> Option<Image> {
    if b.len() < 33 || ascii(b, 12, 4) != "IHDR" {
        return None;
    }
    let (w, h, depth, kind, interlace) = (u32be(b, 16) as usize, u32be(b, 20) as usize, b[24], b[25], b[28]);
    let channels = match kind {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    // Bit depths allowed by the specification for each colour type.
    if ![1, 2, 4, 8, 16].contains(&depth) || (if kind == 3 { depth > 8 } else { kind != 0 && depth < 8 }) {
        return None;
    }
    if bad(w as f64, h as f64) {
        return None;
    }
    let (mut plte, mut trns): (Option<&[u8]>, Option<&[u8]>) = (None, None);
    let mut idat = Vec::new();
    let mut any = false;
    let mut i = 8usize;
    while i + 8 <= b.len() {
        let len = u32be(b, i) as usize;
        let data = &b[i + 8..b.len().min((i + 8).saturating_add(len))]; // truncated: what is there
        match ascii(b, i + 4, 4).as_str() {
            "IDAT" => {
                idat.extend_from_slice(data);
                any = true;
            }
            "PLTE" => plte = Some(data),
            "tRNS" => trns = Some(data),
            "IEND" => break,
            _ => {}
        }
        i = i.saturating_add(12).saturating_add(len);
    }
    if !any || (kind == 3 && plte.is_none()) {
        return None;
    }

    let bpp = channels * depth as usize; // bits per pixel
    let passes: Vec<[usize; 7]> = (if interlace != 0 { &ADAM7[..] } else { &[[0, 0, 1, 1]][..] })
        .iter()
        .map(|&[x0, y0, dx, dy]| {
            let pw = w.saturating_sub(x0).div_ceil(dx);
            let ph = h.saturating_sub(y0).div_ceil(dy);
            [x0, y0, dx, dy, pw, ph, (pw * bpp).div_ceil(8)]
        })
        .collect();
    let size: usize = passes.iter().map(|p| if p[4] > 0 && p[5] > 0 { p[5] * (1 + p[6]) } else { 0 }).sum();
    if size > MAX_RAW {
        return None;
    }
    // Sync flush: a truncated file still gives its first rows.
    let raw = inflate_zlib(&idat, size + 1024).ok()?;

    // Palette, or the transparent colour, as RGBA.
    let mut pal = [0u8; 256 * 4];
    if let Some(p) = plte {
        for k in 0..256 {
            if k * 3 + 2 >= p.len() {
                break;
            }
            let a = trns.and_then(|t| t.get(k).copied()).unwrap_or(255);
            pal[k * 4..k * 4 + 4].copy_from_slice(&[p[k * 3], p[k * 3 + 1], p[k * 3 + 2], a]);
        }
    }
    let key: Option<Vec<u32>> = match trns {
        Some(t) if kind == 0 && t.len() >= 2 => Some(vec![u16be(t, 0)]),
        Some(t) if kind == 2 && t.len() >= 6 => Some(vec![u16be(t, 0), u16be(t, 2), u16be(t, 4)]),
        _ => None,
    };
    let keyed = |vals: &[u32]| key.as_ref().is_some_and(|k| k.iter().zip(vals).all(|(a, b)| a == b));

    let mut out = vec![0u8; w * h * 4];
    let dist = (bpp >> 3).max(1);
    let mask = (1u32 << depth) - 1;
    let mut pos = 0;
    for &[x0, y0, dx, dy, pw, ph, stride] in &passes {
        if pw == 0 || ph == 0 {
            continue;
        }
        let (mut prev, mut cur) = (vec![0u8; stride], vec![0u8; stride]);
        let mut y = 0;
        while y < ph && pos < raw.len() {
            let f = raw[pos];
            cur.fill(0);
            let row = &raw[(pos + 1).min(raw.len())..raw.len().min(pos + 1 + stride)];
            cur[..row.len()].copy_from_slice(row);
            unfilter(f, &mut cur, &prev, dist);
            let line = (y0 + y * dy) * w;
            for x in 0..pw {
                let o = (line + x0 + x * dx) * 4;
                let px = &mut out[o..o + 4];
                if depth == 8 {
                    let s = x * channels;
                    match kind {
                        6 => px.copy_from_slice(&cur[s..s + 4]),
                        3 => px.copy_from_slice(&pal[cur[s] as usize * 4..cur[s] as usize * 4 + 4]),
                        2 => {
                            px[..3].copy_from_slice(&cur[s..s + 3]);
                            px[3] = if keyed(&[cur[s] as u32, cur[s + 1] as u32, cur[s + 2] as u32]) { 0 } else { 255 };
                        }
                        _ => {
                            px[..3].fill(cur[s]);
                            px[3] = if kind == 4 {
                                cur[s + 1]
                            } else if keyed(&[cur[s] as u32]) {
                                0
                            } else {
                                255
                            };
                        }
                    }
                } else if depth == 16 {
                    let s = x * channels * 2;
                    let v = |k: usize| (cur[s + 2 * k] as u32) << 8 | cur[s + 2 * k + 1] as u32;
                    if kind == 0 || kind == 4 {
                        px[..3].fill(cur[s]);
                        px[3] = if kind == 4 {
                            cur[s + 2]
                        } else if keyed(&[v(0)]) {
                            0
                        } else {
                            255
                        };
                    } else {
                        px[..3].copy_from_slice(&[cur[s], cur[s + 2], cur[s + 4]]);
                        px[3] = if kind == 6 {
                            cur[s + 6]
                        } else if keyed(&[v(0), v(1), v(2)]) {
                            0
                        } else {
                            255
                        };
                    }
                } else {
                    // 1, 2 or 4 bits: grey or palette.
                    let bit = x * depth as usize;
                    let v = (cur[bit >> 3] as u32) >> (8 - depth as usize - (bit & 7)) & mask;
                    if kind == 3 {
                        px.copy_from_slice(&pal[v as usize * 4..v as usize * 4 + 4]);
                    } else {
                        px[..3].fill((v * 255 / mask) as u8);
                        px[3] = if keyed(&[v]) { 0 } else { 255 };
                    }
                }
            }
            std::mem::swap(&mut prev, &mut cur);
            y += 1;
            pos += stride + 1;
        }
    }
    Some(Image { width: w, height: h, data: out })
}

fn unfilter(f: u8, cur: &mut [u8], prev: &[u8], d: usize) {
    let n = cur.len();
    match f {
        1 => {
            for i in d..n {
                cur[i] = cur[i].wrapping_add(cur[i - d]);
            }
        }
        2 => {
            for i in 0..n {
                cur[i] = cur[i].wrapping_add(prev[i]);
            }
        }
        3 => {
            for i in 0..n {
                let a = if i >= d { cur[i - d] as u32 } else { 0 };
                cur[i] = cur[i].wrapping_add(((a + prev[i] as u32) >> 1) as u8);
            }
        }
        4 => {
            for i in 0..n {
                let a = if i >= d { cur[i - d] as i32 } else { 0 };
                let (b, c) = (prev[i] as i32, if i >= d { prev[i - d] as i32 } else { 0 });
                let (pa, pb, pc) = ((b - c).abs(), (a - c).abs(), (a + b - 2 * c).abs());
                let p = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                cur[i] = cur[i].wrapping_add(p as u8);
            }
        }
        _ => {}
    }
}

// --- GIF (first frame)

fn decode_gif(b: &[u8]) -> Option<Image> {
    let (mut sw, mut sh) = (le16(b, 6) as usize, le16(b, 8) as usize);
    let mut pos = 13;
    let mut gct: Option<&[u8]> = None;
    let mut transparent: Option<u8> = None;
    let table = |pos: usize, flags: u8| &b[pos.min(b.len())..b.len().min(pos + (3 << ((flags & 7) + 1)))];
    if b[10] & 0x80 != 0 {
        let t = table(pos, b[10]);
        pos += t.len();
        gct = Some(t);
    }
    while pos < b.len() {
        let t = b[pos];
        pos += 1;
        if t == 0x21 {
            if at(b, pos) == 0xf9 && at(b, pos + 1) >= 4 && at(b, pos + 2) & 1 != 0 {
                transparent = b.get(pos + 5).copied();
            }
            pos = skip_blocks(b, pos + 1);
        } else if t == 0x2c {
            let (fx, fy, fw, fh) = (
                le16(b, pos) as usize,
                le16(b, pos + 2) as usize,
                le16(b, pos + 4) as usize,
                le16(b, pos + 6) as usize,
            );
            let flags = at(b, pos + 8) as u8;
            pos += 9;
            let mut pal = gct;
            if flags & 0x80 != 0 {
                let t = table(pos, flags);
                pos += t.len();
                pal = Some(t);
            }
            // A frame larger than the screen enlarges it, as browsers do.
            sw = sw.max(fx + fw);
            sh = sh.max(fy + fh);
            if bad(sw as f64, sh as f64) {
                return None;
            }
            // No code size: the JS makes an array of negative length, and fails.
            let min = *b.get(pos)? as u32;
            pos += 1;
            let end = skip_blocks(b, pos);
            let mut data = vec![0u8; end - pos];
            let mut n = 0;
            let mut p = pos;
            while p < end && b[p] != 0 {
                let len = b[p] as usize;
                let block = &b[p + 1..b.len().min(p + 1 + len)];
                data[n..n + block.len()].copy_from_slice(block);
                n += len;
                p += 1 + len;
            }
            let mut idx = vec![0u8; fw * fh];
            let count = lzw(&data[..n.min(data.len())], min, &mut idx);
            let mut rows = vec![0usize; fh];
            if flags & 0x40 != 0 {
                let mut r = 0;
                for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                    for y in (start..fh).step_by(step) {
                        rows[r] = y;
                        r += 1;
                    }
                }
            } else {
                for (y, row) in rows.iter_mut().enumerate() {
                    *row = y;
                }
            }
            let mut out = vec![0u8; sw * sh * 4];
            for (i, &k) in idx[..count].iter().enumerate() {
                let c = k as usize * 3;
                let Some(pal) = pal.filter(|p| Some(k) != transparent && c + 2 < p.len()) else { continue };
                let o = ((fy + rows[i / fw]) * sw + fx + i % fw) * 4;
                out[o..o + 4].copy_from_slice(&[pal[c], pal[c + 1], pal[c + 2], 255]);
            }
            return Some(Image { width: sw, height: sh, data: out });
        } else {
            return None;
        }
    }
    None
}

fn skip_blocks(b: &[u8], mut pos: usize) -> usize {
    while pos < b.len() {
        let len = b[pos] as usize;
        pos += 1 + len;
        if len == 0 {
            break;
        }
    }
    pos.min(b.len())
}

// Decodes into out; returns the number of pixels written.
fn lzw(data: &[u8], min: u32, out: &mut [u8]) -> usize {
    if !(1..=11).contains(&min) {
        return 0;
    }
    let (clear, eoi, count) = (1usize << min, (1usize << min) + 1, out.len());
    let mut prefix = vec![0u16; 4096];
    let mut lens = vec![0u16; 4096];
    let mut suffix = vec![0u8; 4096];
    let mut first = vec![0u8; 4096];
    for k in 0..clear {
        suffix[k] = k as u8;
        first[k] = k as u8;
        lens[k] = 1;
    }
    let (mut size, mut next, mut prev, mut acc, mut bits, mut o) = (min + 1, eoi + 1, None::<usize>, 0u32, 0u32, 0);
    let emit = |c: usize, o: &mut usize, out: &mut [u8], prefix: &[u16], suffix: &[u8], lens: &[u16]| {
        let l = lens[c] as usize;
        let mut k = c;
        for j in (0..l).rev() {
            if *o + j < count {
                out[*o + j] = suffix[k];
            }
            k = prefix[k] as usize;
        }
        *o += l;
    };
    let mut i = 0;
    while i < data.len() && o < count {
        acc |= (data[i] as u32) << bits;
        i += 1;
        bits += 8;
        while bits >= size && o < count {
            let code = (acc & ((1 << size) - 1)) as usize;
            acc >>= size;
            bits -= size;
            if code == clear {
                size = min + 1;
                next = eoi + 1;
                prev = None;
                continue;
            }
            if code == eoi {
                return o.min(count);
            }
            let Some(p) = prev else {
                if code >= clear {
                    return o.min(count);
                }
                out[o] = code as u8;
                o += 1;
                prev = Some(code);
                continue;
            };
            if code > next {
                return o.min(count);
            }
            let fc = if code < next { first[code] } else { first[p] };
            if code < next {
                emit(code, &mut o, out, &prefix, &suffix, &lens);
            } else {
                emit(p, &mut o, out, &prefix, &suffix, &lens);
                if o < count {
                    out[o] = fc;
                }
                o += 1;
            }
            if next < 4096 {
                prefix[next] = p as u16;
                suffix[next] = fc;
                first[next] = first[p];
                lens[next] = lens[p] + 1;
                next += 1;
                if next == 1 << size && size < 12 {
                    size += 1;
                }
            }
            prev = Some(code);
        }
    }
    o.min(count)
}

// --- BMP and the bitmaps inside ICO files

fn shift_of(m: u32) -> u32 {
    if m == 0 { 32 } else { m.trailing_zeros() }
}

// off: start of the DIB header; data: start of the pixels (None: right after the palette). ico: the height counts
// the colour bitmap and the 1-bit transparency mask that follows it.
fn decode_dib(b: &[u8], off: usize, data: Option<usize>, ico: bool) -> Option<Image> {
    let hs = le32(b, off) as usize;
    let (w, mut h, bpp, comp, used) = if hs == 12 {
        (le16(b, off + 4) as i64, le16(b, off + 6) as i64, le16(b, off + 10), 0, 0)
    } else if (40..=256).contains(&hs) {
        (sle32(b, off + 4) as i64, sle32(b, off + 8) as i64, le16(b, off + 14), le32(b, off + 16), le32(b, off + 32))
    } else {
        return None;
    };
    if ico {
        h /= 2;
    }
    let top_down = h < 0;
    h = h.abs();
    if bad(w as f64, h as f64) || ![1, 2, 4, 8, 16, 24, 32].contains(&bpp) {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let mut masks: Option<[u32; 4]> = None;
    let mut alpha = false;
    let mut extra = 0;
    if comp == 3 || comp == 6 {
        // Masks in the header (V2 and later) or right after the 40-byte one.
        let m = if hs >= 52 { off + 40 } else { off + hs };
        let a = if hs >= 56 || comp == 6 { le32(b, m + 12) } else { 0 };
        masks = Some([le32(b, m), le32(b, m + 4), le32(b, m + 8), a]);
        if hs < 52 {
            extra = if comp == 6 { 16 } else { 12 };
        }
        alpha = a != 0;
    } else if comp != 0 {
        return None; // RLE, embedded JPEG / PNG
    } else if bpp == 16 {
        masks = Some([0x7c00, 0x3e0, 0x1f, 0]);
    } else if bpp == 32 {
        masks = Some([0xff0000, 0xff00, 0xff, 0xff000000]);
        alpha = ico;
    }
    let entry = if hs == 12 { 3 } else { 4 };
    let pal = off + hs + extra;
    let colours = if bpp <= 8 {
        let all = 1usize << bpp;
        if used == 0 { all } else { (used as usize).min(all) }
    } else {
        0
    };
    let data = data.unwrap_or(pal + colours * entry);
    let stride = (w * bpp as usize).div_ceil(32) * 4;
    let mut out = vec![0u8; w * h * 4];
    // Each mask, its shift and its largest value, as the JS computes them in 32 bits.
    let chan = masks.map(|ms| {
        ms.map(|m| {
            let max = (1i32.wrapping_shl(m.count_ones()) as i64) - 1;
            (m, shift_of(m) % 32, if max == 0 { 1 } else { max })
        })
    });
    let value = |v: u32, (m, s, max): (u32, u32, i64)| byte(((v & m) >> s) as f64 * 255.0 / max as f64);
    let mut any_alpha = false;
    for y in 0..h {
        let p = data + (if top_down { y } else { h - 1 - y }) * stride;
        if p + stride > b.len() {
            continue;
        }
        for x in 0..w {
            let o = (y * w + x) * 4;
            if bpp <= 8 {
                let bit = x * bpp as usize;
                let k = (b[p + (bit >> 3)] as usize) >> (8 - bpp as usize - (bit & 7)) & ((1 << bpp) - 1);
                let c = pal + k * entry;
                if k < colours && c + 2 < b.len() {
                    out[o..o + 3].copy_from_slice(&[b[c + 2], b[c + 1], b[c]]);
                }
                out[o + 3] = 255;
            } else if bpp == 24 {
                let q = p + x * 3;
                out[o..o + 4].copy_from_slice(&[b[q + 2], b[q + 1], b[q], 255]);
            } else {
                let chan = chan?;
                let v = if bpp == 16 { le16(b, p + x * 2) } else { le32(b, p + x * 4) };
                for k in 0..3 {
                    out[o + k] = value(v, chan[k]);
                }
                if !alpha {
                    out[o + 3] = 255;
                    continue;
                }
                out[o + 3] = value(v, chan[3]);
                any_alpha |= out[o + 3] > 0;
            }
        }
    }
    // An alpha channel that is all zero is unused, not a fully transparent image.
    if alpha && !any_alpha {
        for a in out.iter_mut().skip(3).step_by(4) {
            *a = 255;
        }
    }
    if ico && !any_alpha {
        let ms = w.div_ceil(32) * 4;
        let mask = data + stride * h;
        for y in 0..h {
            let p = mask + (if top_down { y } else { h - 1 - y }) * ms;
            if p + ms > b.len() {
                continue;
            }
            for x in 0..w {
                if b[p + (x >> 3)] >> (7 - (x & 7)) & 1 != 0 {
                    out[(y * w + x) * 4 + 3] = 0;
                }
            }
        }
    }
    Some(Image { width: w, height: h, data: out })
}

fn decode_ico(b: &[u8]) -> Option<Image> {
    let e = ico_entry(b)?;
    let off = e.off as usize;
    if off >= b.len() {
        return None;
    }
    let d = &b[off..b.len().min(off + e.size as usize)];
    if is_png(d) { decode_png(d) } else { decode_dib(d, 0, None, true) }
}

// --- JPEG
//
// Baseline, extended sequential and progressive Huffman, 8 bits; grey, YCbCr, RGB, CMYK and YCCK.

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63,
];

/// ToInt32: a number stored in an Int32Array, or before a bitwise operator.
fn int32(x: f64) -> i32 {
    if x.is_finite() { x.trunc().rem_euclid(4294967296.0) as u32 as i32 } else { 0 }
}

/// A number stored in a Uint8ClampedArray: NaN and below 0 give 0, halves go to the even neighbour.
fn clamped(x: f64) -> u8 {
    if x.is_nan() || x <= 0.0 {
        0
    } else if x >= 255.0 {
        255
    } else {
        x.round_ties_even() as u8
    }
}

// Codes of up to 9 bits are found in one lookup (length << 8 | value); longer ones by length. The values may be fewer
// than the counts say (a cut file): the missing ones read 0.
struct Huffman {
    fast: Vec<u16>,
    max: [i32; 17],
    min: [i32; 17],
    ptr: [i32; 17],
    values: Vec<u8>,
}

impl Huffman {
    fn new(counts: &[u8], values: &[u8]) -> Huffman {
        let mut fast = vec![0u16; 512];
        let (mut max, mut min, mut ptr) = ([-1i32; 17], [0i32; 17], [0i32; 17]);
        let (mut code, mut k) = (0i32, 0usize);
        for len in 1..=16 {
            ptr[len] = k as i32;
            min[len] = code;
            for _ in 0..counts[len - 1] {
                if len <= 9 && code < 1 << len {
                    let base = (code << (9 - len)) as usize;
                    let v = values.get(k).map_or(0, |&v| v as u16);
                    fast[base..base + (1 << (9 - len))].fill((len as u16) << 8 | v);
                }
                k += 1;
                code += 1;
            }
            if counts[len - 1] != 0 {
                max[len] = code - 1;
            }
            code <<= 1;
        }
        Huffman { fast, max, min, ptr, values: values.to_vec() }
    }
}

type Table = [i32; 64];

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    stride: usize,
    plane: Vec<u8>,
    // Blocks of a scan with this component alone: no padding to whole MCUs.
    bw: usize,
    bh: usize,
    across: usize,
    // Progressive: the coefficients of every block, built up scan after scan, then transformed (jpeg_finish).
    coefs: Vec<i16>,
    pred: i64,
}

struct Frame {
    w: usize,
    h: usize,
    comps: Vec<Comp>,
    hmax: usize,
    vmax: usize,
    mx: usize,
    my: usize,
    progressive: bool,
}

// A component of a scan: its index in the frame, its tables.
struct ScanComp<'a> {
    c: usize,
    dc: Option<&'a Huffman>,
    ac: Option<&'a Huffman>,
    q: Table,
}

// A progressive scan: a band ss..se (None where the segment is cut short), ah and al the bits of its refinement.
struct Band {
    ss: Option<u32>,
    se: Option<u32>,
    ah: u32,
    al: u32,
}

fn decode_jpeg(b: &[u8]) -> Option<Image> {
    let n = b.len();
    let mut qt: [Option<Table>; 4] = [None; 4];
    let (mut dcs, mut acs): ([Option<Huffman>; 4], [Option<Huffman>; 4]) = Default::default();
    let mut frame: Option<Frame> = None;
    let mut restart = 0usize;
    // The transform of the Adobe segment: -1 without one, or when the segment is cut before it.
    let mut adobe = -1i32;
    let mut i = 2;
    while i + 4 <= n {
        if b[i] != 0xff || b[i + 1] == 0xff {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if matches!(m, 0xd8 | 0x01 | 0x00 | 0xd0..=0xd7) {
            i += 2;
            continue;
        }
        if m == 0xd9 {
            break;
        }
        let len = u16be(b, i + 2) as usize;
        let (s, e) = (i + 4, (i + 2 + len).min(n));
        match m {
            0xdb => {
                let mut p = s;
                while p < e {
                    let (wide, id) = (b[p] >> 4 != 0, (b[p] & 3) as usize);
                    p += 1;
                    let mut t = [0i32; 64];
                    for (k, &z) in ZIGZAG.iter().enumerate() {
                        t[z] = if wide { u16be(b, p + 2 * k) } else { at(b, p + k) } as i32;
                    }
                    p += if wide { 128 } else { 64 };
                    qt[id] = Some(t);
                }
            }
            0xc4 => {
                let mut p = s;
                while p + 17 <= e {
                    let counts = &b[p + 1..p + 17];
                    let total: usize = counts.iter().map(|&c| c as usize).sum();
                    let table = Huffman::new(counts, &b[p + 17..n.min(p + 17 + total)]);
                    let tables = if b[p] >> 4 != 0 { &mut acs } else { &mut dcs };
                    tables[(b[p] & 3) as usize] = Some(table);
                    p += 17 + total;
                }
            }
            0xdd => restart = u16be(b, s) as usize,
            0xee if ascii(b, s, 5) == "Adobe" => adobe = b.get(s + 11).map_or(-1, |&x| x as i32),
            0xc0..=0xc2 => {
                if frame.is_some() || at(b, s) != 8 {
                    return None;
                }
                frame = Some(jpeg_frame(b, s, m == 0xc2)?);
            }
            0xc3..=0xcf if m != 0xc8 && m != 0xcc => return None,
            0xda => {
                let f = frame.as_mut()?;
                let count = at(b, s) as usize;
                let get = |k: usize| b.get(k).map(|&x| x as u32);
                let a = at(b, s + 3 + 2 * count);
                let (ss, se) = (get(s + 1 + 2 * count), get(s + 2 + 2 * count));
                let band = f.progressive.then_some(Band { ss, se, ah: a >> 4, al: a & 15 });
                // A progressive scan holds either the DC coefficients (of one or more components) or a band of AC
                // ones (of one component), and needs only that table.
                let gt = |x: Option<u32>, y: u32| x.is_some_and(|x| x > y);
                if band.as_ref().is_some_and(|sc| {
                    gt(se, 63)
                        || ss.zip(se).is_some_and(|(ss, se)| ss > se)
                        || (ss == Some(0) && se != Some(0))
                        || (gt(ss, 0) && count != 1)
                        || sc.al > 13
                }) {
                    return None;
                }
                let mut comps = Vec::new();
                for k in 0..count {
                    let id = b.get(s + 1 + 2 * k).copied();
                    let c = f.comps.iter().position(|x| Some(x.id) == id)?;
                    let sel = at(b, s + 2 + 2 * k) as usize;
                    let (dc, ac) =
                        (dcs.get(sel >> 4).and_then(Option::as_ref), acs.get(sel & 15).and_then(Option::as_ref));
                    let q = qt[f.comps[c].tq]?;
                    let ok = match &band {
                        Some(sc) if ss == Some(0) => dc.is_some() || sc.ah != 0,
                        Some(_) => ac.is_some(),
                        None => dc.is_some() && ac.is_some(),
                    };
                    if !ok {
                        return None;
                    }
                    comps.push(ScanComp { c, dc, ac, q });
                }
                if comps.is_empty() {
                    return None;
                }
                i = match &band {
                    Some(sc) => jpeg_progressive(b, e, f, &comps, restart, sc),
                    None => jpeg_scan(b, e, f, &comps, restart),
                };
                continue;
            }
            _ => {}
        }
        i += 2 + len;
    }
    let mut frame = frame?;
    if frame.progressive {
        jpeg_finish(&mut frame, &qt);
    }
    Some(jpeg_colours(&frame, adobe))
}

fn jpeg_frame(b: &[u8], s: usize, progressive: bool) -> Option<Frame> {
    let (h, w, count) = (u16be(b, s + 1) as usize, u16be(b, s + 3) as usize, at(b, s + 5) as usize);
    if bad(w as f64, h as f64) || ![1, 3, 4].contains(&count) {
        return None;
    }
    let mut comps: Vec<Comp> = (0..count)
        .map(|k| {
            let p = s + 6 + 3 * k;
            Comp {
                id: at(b, p) as u8,
                h: (at(b, p + 1) >> 4) as usize,
                v: (at(b, p + 1) & 15) as usize,
                tq: (at(b, p + 2) & 3) as usize,
                stride: 0,
                plane: Vec::new(),
                bw: 0,
                bh: 0,
                across: 0,
                coefs: Vec::new(),
                pred: 0,
            }
        })
        .collect();
    if comps.iter().any(|c| !(1..=4).contains(&c.h) || !(1..=4).contains(&c.v)) {
        return None;
    }
    let hmax = comps.iter().map(|c| c.h).max()?;
    let vmax = comps.iter().map(|c| c.v).max()?;
    let (mx, my) = (w.div_ceil(8 * hmax), h.div_ceil(8 * vmax));
    for c in &mut comps {
        c.stride = mx * c.h * 8;
        c.plane = vec![0; c.stride * my * c.v * 8];
        c.bw = (w * c.h).div_ceil(hmax * 8);
        c.bh = (h * c.v).div_ceil(vmax * 8);
        c.across = mx * c.h;
        if progressive {
            c.coefs = vec![0; c.across * my * c.v * 64];
        }
    }
    Some(Frame { w, h, comps, hmax, vmax, mx, my, progressive })
}

// The entropy-coded data of a scan, read bit by bit from pos. Byte stuffing (FF 00) is undone; at a marker or the
// end of the file, zeros come instead.
struct Bits<'a> {
    b: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
    marker: bool,
    eobrun: i64,
}

impl Bits<'_> {
    fn new(b: &[u8], pos: usize) -> Bits<'_> {
        Bits { b, pos, buf: 0, cnt: 0, marker: false, eobrun: 0 }
    }

    // Keeps at least 25 bits in buf.
    fn fill(&mut self) {
        while self.cnt <= 24 {
            let mut v = 0;
            if !self.marker && self.pos < self.b.len() {
                v = self.b[self.pos] as u32;
                if v != 0xff {
                    self.pos += 1;
                } else if self.b.get(self.pos + 1) == Some(&0) {
                    self.pos += 2;
                } else {
                    self.marker = true;
                    v = 0;
                }
            }
            self.buf = self.buf << 8 | v;
            self.cnt += 8;
        }
    }

    fn bits(&mut self, s: u32) -> u32 {
        if s == 0 {
            return 0;
        }
        if self.cnt < s {
            self.fill();
        }
        let v = (self.buf >> (self.cnt - s)) & ((1 << s) - 1);
        self.cnt -= s;
        v
    }

    fn symbol(&mut self, t: &Huffman) -> u32 {
        if self.cnt < 16 {
            self.fill();
        }
        let f = t.fast[((self.buf >> (self.cnt - 9)) & 511) as usize];
        if f != 0 {
            self.cnt -= (f >> 8) as u32;
            return (f & 255) as u32;
        }
        for len in 10..=16u32 {
            let code = ((self.buf >> (self.cnt - len)) & ((1 << len) - 1)) as i32;
            let l = len as usize;
            if code <= t.max[l] {
                self.cnt -= len;
                let k = t.ptr[l] as i64 + code as i64 - t.min[l] as i64;
                return usize::try_from(k).ok().and_then(|k| t.values.get(k)).map_or(0, |&v| v as u32);
            }
        }
        self.cnt -= 16;
        0
    }

    // A value of s bits, its sign given by its first bit.
    fn receive(&mut self, s: u32) -> i64 {
        let v = self.bits(s) as i64;
        if s != 0 && v < 1 << (s - 1) { v - (1 << s) + 1 } else { v }
    }

    fn is_marker(&self, k: usize, rst: bool) -> bool {
        let next = self.b.get(k + 1).copied();
        self.b.get(k) == Some(&0xff)
            && next != Some(0)
            && next != Some(0xff)
            && (rst || !matches!(next, Some(0xd0..=0xd7)))
    }

    // Resynchronises on the next RST marker; false when another marker comes first, which ends the scan.
    fn restart(&mut self) -> bool {
        while self.pos + 1 < self.b.len() && !self.is_marker(self.pos, true) {
            self.pos += 1;
        }
        if !matches!(self.b.get(self.pos + 1), Some(0xd0..=0xd7)) {
            return false;
        }
        self.pos += 2;
        self.marker = false;
        self.buf = 0;
        self.cnt = 0;
        true
    }

    // The position of the marker that ends the scan.
    fn end(&mut self) -> usize {
        while self.pos + 1 < self.b.len() && !self.is_marker(self.pos, false) {
            self.pos += 1;
        }
        self.pos
    }
}

// Decodes one scan of a baseline image; returns the position of the marker that follows it.
fn jpeg_scan(b: &[u8], pos: usize, f: &mut Frame, comps: &[ScanComp], restart: usize) -> usize {
    let mut r = Bits::new(b, pos);
    let (mut blk, mut tmp) = ([0i32; 64], [0i32; 64]);
    each_block(f, comps, restart, &mut r, |r, s, c, bx, by| {
        let (Some(dc), Some(ac)) = (s.dc, s.ac) else { return };
        blk.fill(0);
        let t = r.symbol(dc) & 15;
        c.pred += r.receive(t);
        // The prediction is unbounded: its product goes through a double, as in the JS.
        blk[0] = int32(c.pred as f64 * s.q[0] as f64);
        let mut k = 1;
        while k < 64 {
            let rs = r.symbol(ac);
            let (size, run) = (rs & 15, (rs >> 4) as usize);
            if size == 0 {
                if run != 15 {
                    break;
                }
                k += 16;
                continue;
            }
            k += run;
            if k > 63 {
                break;
            }
            let z = ZIGZAG[k];
            k += 1;
            blk[z] = (r.receive(size) * s.q[z] as i64) as i32;
        }
        idct(&blk, &mut tmp, &mut c.plane, by * 8 * c.stride + bx * 8, c.stride);
    });
    r.end()
}

// Calls block(scan component, component, x, y) for every block of a scan, in its order: MCU after MCU when it
// interleaves components, row after row of the component's own blocks otherwise. Predictions (and the progressive
// end-of-band run) start again at each restart marker.
fn each_block(
    f: &mut Frame,
    comps: &[ScanComp],
    restart: usize,
    r: &mut Bits,
    mut block: impl FnMut(&mut Bits, &ScanComp, &mut Comp, usize, usize),
) {
    let single = comps.len() == 1;
    let total = if single { f.comps[comps[0].c].bw * f.comps[comps[0].c].bh } else { f.mx * f.my };
    let reset = |f: &mut Frame, r: &mut Bits<'_>| {
        for s in comps {
            f.comps[s.c].pred = 0;
        }
        r.eobrun = 0;
    };
    reset(f, r);
    for m in 0..total {
        if restart != 0 && m != 0 && m % restart == 0 {
            if !r.restart() {
                break;
            }
            reset(f, r);
        }
        if single {
            let c = &mut f.comps[comps[0].c];
            let bw = c.bw;
            block(r, &comps[0], c, m % bw, m / bw);
            continue;
        }
        let (mx, my) = (m % f.mx, m / f.mx);
        for s in comps {
            let c = &mut f.comps[s.c];
            let (ch, cv) = (c.h, c.v);
            for v in 0..cv {
                for h in 0..ch {
                    block(r, s, c, mx * ch + h, my * cv + v);
                }
            }
        }
    }
}

// Decodes one scan of a progressive image into the coefficients: the DC ones or a band ss..se of AC ones, first
// (ah 0) or one more bit of them (refinement), each scaled by 2^al. As libjpeg's jdphuff.c.
fn jpeg_progressive(b: &[u8], pos: usize, f: &mut Frame, comps: &[ScanComp], restart: usize, band: &Band) -> usize {
    let mut r = Bits::new(b, pos);
    let (p1, m1) = (1i32 << band.al, -1i32 << band.al);
    // A refinement bit for a coefficient already non-zero: away from zero.
    let refine = |r: &mut Bits<'_>, co: &mut [i16], z: usize| {
        if r.bits(1) != 0 && (co[z] as i32 & p1) == 0 {
            co[z] = (co[z] as i32 + if co[z] >= 0 { p1 } else { m1 }) as i16;
        }
    };
    each_block(f, comps, restart, &mut r, |r, s, c, bx, by| {
        let o = (by * c.across + bx) * 64;
        if o + 64 > c.coefs.len() {
            return;
        }
        if band.ss == Some(0) {
            if band.ah == 0 {
                let t = s.dc.map_or(0, |t| r.symbol(t)) & 15;
                c.pred += r.receive(t);
                c.coefs[o] = (c.pred * p1 as i64) as i16;
            } else if r.bits(1) != 0 {
                c.coefs[o] |= p1 as i16;
            }
            return;
        }
        // A band cut short reads nothing (undefined bounds in the JS).
        let (Some(ss), Some(se), Some(ac)) = (band.ss, band.se, s.ac) else { return };
        let (mut k, se) = (ss as usize, se as usize);
        let co = &mut c.coefs[o..o + 64];
        if band.ah == 0 {
            if r.eobrun > 0 {
                r.eobrun -= 1;
                return;
            }
            while k <= se {
                let rs = r.symbol(ac);
                let (run, size) = (rs >> 4, rs & 15);
                if size == 0 {
                    if run < 15 {
                        r.eobrun = (1 << run) - 1 + r.bits(run) as i64;
                        break;
                    }
                    k += 16;
                    continue;
                }
                k += run as usize;
                if k > 63 {
                    break;
                }
                co[ZIGZAG[k]] = (r.receive(size) * p1 as i64) as i16;
                k += 1;
            }
            return;
        }
        if r.eobrun <= 0 {
            while k <= se {
                let rs = r.symbol(ac);
                let mut run = (rs >> 4) as i32;
                let mut value = 0;
                if rs & 15 != 0 {
                    value = if r.bits(1) != 0 { p1 } else { m1 };
                } else if run < 15 {
                    r.eobrun = (1 << run) + r.bits(run as u32) as i64;
                    break;
                }
                // Zeros to skip (run of them, or 16), refining the non-zero coefficients met on the way.
                while k <= se {
                    let z = ZIGZAG[k];
                    if co[z] != 0 {
                        refine(r, co, z);
                    } else {
                        run -= 1;
                        if run < 0 {
                            break;
                        }
                    }
                    k += 1;
                }
                if value != 0 && k <= se {
                    co[ZIGZAG[k]] = value as i16;
                }
                k += 1;
            }
        }
        if r.eobrun > 0 {
            while k <= se {
                if co[ZIGZAG[k]] != 0 {
                    refine(r, co, ZIGZAG[k]);
                }
                k += 1;
            }
            r.eobrun -= 1;
        }
    });
    r.end()
}

// A progressive image once its scans are read: every block's coefficients dequantised and transformed.
fn jpeg_finish(f: &mut Frame, qt: &[Option<Table>; 4]) {
    let (mut blk, mut tmp) = ([0i32; 64], [0i32; 64]);
    for c in &mut f.comps {
        let Some(q) = qt[c.tq] else { continue };
        for by in 0..c.bh {
            for bx in 0..c.bw {
                let o = (by * c.across + bx) * 64;
                for k in 0..64 {
                    blk[k] = c.coefs.get(o + k).map_or(0, |&x| (x as i64 * q[k] as i64) as i32);
                }
                idct(&blk, &mut tmp, &mut c.plane, by * 8 * c.stride + bx * 8, c.stride);
            }
        }
        c.coefs = Vec::new();
    }
}

// Integer IDCT (the jidctint algorithm, as in stb_image): fixed point with 12 fractional bits. The JS computes in
// doubles, exact here (below 2^53) as in i64; ToInt32 where it stores or shifts.
const K: [i64; 12] = [2217, -7568, 3135, 4816, 1223, 8410, 12586, 6149, -3686, -10498, -8035, -1598];

fn idct(d: &[i32; 64], v: &mut [i32; 64], out: &mut [u8], off: usize, stride: usize) {
    for pass in 0..2 {
        for i in 0..8 {
            // First pass on columns (step 8), second on rows (step 1).
            let (a, st) = if pass == 1 { (i * 8, 1) } else { (i, 8) };
            let s: [i64; 8] = std::array::from_fn(|k| (if pass == 1 { v[a + k * st] } else { d[a + k * st] }) as i64);
            if pass == 0 && s[1..].iter().all(|&x| x == 0) {
                let dc = (s[0] * 4) as i32;
                for k in 0..8 {
                    v[a + k * 8] = dc;
                }
                continue;
            }
            let p1 = (s[2] + s[6]) * K[0];
            let (t2, t3) = (p1 + s[6] * K[1], p1 + s[2] * K[2]);
            let (t0, t1) = ((s[0] + s[4]) * 4096, (s[0] - s[4]) * 4096);
            let (mut x0, mut x3, mut x1, mut x2) = (t0 + t3, t0 - t3, t1 + t2, t1 - t2);
            let (mut t0, mut t1, mut t2, mut t3) = (s[7], s[5], s[3], s[1]);
            let (mut p3, mut p4) = (t0 + t2, t1 + t3);
            let (mut p1, mut p2) = (t0 + t3, t1 + t2);
            let p5 = (p3 + p4) * K[3];
            t0 *= K[4];
            t1 *= K[5];
            t2 *= K[6];
            t3 *= K[7];
            p1 = p5 + p1 * K[8];
            p2 = p5 + p2 * K[9];
            p3 *= K[10];
            p4 *= K[11];
            t3 += p1 + p4;
            t2 += p2 + p3;
            t1 += p2 + p4;
            t0 += p1 + p3;
            if pass == 0 {
                let sh = |x: i64| (x as i32) >> 10;
                (x0, x1, x2, x3) = (x0 + 512, x1 + 512, x2 + 512, x3 + 512);
                v[a] = sh(x0 + t3);
                v[a + 56] = sh(x0 - t3);
                v[a + 8] = sh(x1 + t2);
                v[a + 48] = sh(x1 - t2);
                v[a + 16] = sh(x2 + t1);
                v[a + 40] = sh(x2 - t1);
                v[a + 24] = sh(x3 + t0);
                v[a + 32] = sh(x3 - t0);
            } else {
                // Rounding, the 1 << 17 scale of both passes, and the +128 level shift.
                let r = 65536 + (128 << 17);
                (x0, x1, x2, x3) = (x0 + r, x1 + r, x2 + r, x3 + r);
                let o = off + i * stride;
                let mut put = |k: usize, x: i64| {
                    if let Some(p) = out.get_mut(o + k) {
                        *p = if x <= 0 { 0 } else { (x / 131072).min(255) as u8 };
                    }
                };
                put(0, x0 + t3);
                put(7, x0 - t3);
                put(1, x1 + t2);
                put(6, x1 - t2);
                put(2, x2 + t1);
                put(5, x2 - t1);
                put(3, x3 + t0);
                put(4, x3 - t0);
            }
        }
    }
}

// Planes to RGBA: chroma upsampled by repetition, then YCbCr, RGB, CMYK or YCCK (Adobe) to RGB.
fn jpeg_colours(f: &Frame, adobe: i32) -> Image {
    let (w, h, comps) = (f.w, f.h, &f.comps);
    let mut out = vec![0u8; w * h * 4];
    let cols: Vec<Vec<usize>> = comps.iter().map(|c| (0..w).map(|x| x * c.h / f.hmax).collect()).collect();
    let ids: Vec<u8> = comps.iter().map(|c| c.id).collect();
    let rgb = comps.len() == 3 && (adobe == 0 || ids == [82, 71, 66]);
    let ycc = if comps.len() == 3 { !rgb } else { adobe == 2 };
    for y in 0..h {
        let rows: Vec<usize> = comps.iter().map(|c| y * c.v / f.vmax * c.stride).collect();
        let px = |k: usize, x: usize| comps[k].plane.get(rows[k] + cols[k][x]).map_or(0.0, |&v| v as f64);
        for x in 0..w {
            let o = (y * w + x) * 4;
            let a = px(0, x);
            if comps.len() == 1 {
                out[o..o + 3].fill(clamped(a));
                out[o + 3] = 255;
                continue;
            }
            let (mut r, mut g, mut bl) = (a, px(1, x), px(2, x));
            if ycc {
                let (cb, cr) = (g - 128.0, bl - 128.0);
                r = a + 1.402 * cr;
                g = a - 0.344136 * cb - 0.714136 * cr;
                bl = a + 1.772 * cb;
            }
            if comps.len() == 4 {
                // Adobe stores CMYK inverted: each channel already is 255 - ink.
                let k = px(3, x) / 255.0;
                if ycc {
                    (r, g, bl) = (255.0 - r, 255.0 - g, 255.0 - bl);
                }
                (r, g, bl) = (r * k, g * k, bl * k);
            }
            out[o..o + 4].copy_from_slice(&[clamped(r), clamped(g), clamped(bl), 255]);
        }
    }
    Image { width: w, height: h, data: out }
}

// --- TIFF
//
// Baseline images: one strip after another (no tiles), one plane, 1 to 16 bits per sample; grey, RGB(A), palette or
// CMYK; uncompressed, PackBits, LZW or Deflate, with the horizontal predictor. Only the first image of the file.

// A whole number, or None: sizes, offsets and rows per strip that are not (rationals, empty tags) give absurd images
// in the JS (NaN lengths, fractional indices), none here.
fn whole(x: f64) -> Option<usize> {
    (x >= 0.0 && x.fract() == 0.0 && x < 9007199254740992.0).then_some(x as usize)
}

fn decode_tiff(b: &[u8]) -> Option<Image> {
    let t = tiff_tags(b)?;
    // A tag's first value, d when the tag is absent; NaN when it has no value (undefined in the JS).
    let one = |tag: u32, d: f64| t.get(tag).map_or(d, |v| v.first().copied().unwrap_or(f64::NAN));
    let (w, h, spp, bps) = (one(256, 0.0), one(257, 0.0), one(277, 1.0), one(258, 1.0));
    let (comp, photo, pred) = (one(259, 1.0), one(262, 1.0), one(317, 1.0));
    let (offsets, counts) = (t.get(273)?, t.get(279)?);
    if bad(w, h) || one(284, 1.0) != 1.0 || ![1.0, 2.0, 4.0, 8.0, 16.0].contains(&bps) || spp < 1.0 || spp > 5.0 {
        return None;
    }
    if ![0.0, 1.0, 2.0, 3.0, 5.0].contains(&photo)
        || (photo == 2.0 && spp < 3.0)
        || (photo == 5.0 && spp < 4.0)
        || (bps < 8.0 && spp > 1.0)
    {
        return None;
    }
    let (w, h, spp) = (whole(w)?, whole(h)?, whole(spp)?);
    let rps = whole(one(278, h as f64))?.min(h);
    let (bps, photo) = (bps as usize, photo as usize);
    let stride = (w * spp * bps).div_ceil(8);
    if stride * h > MAX_RAW {
        return None;
    }
    let mut raw = vec![0u8; stride * h];
    let (mut s, mut y) = (0, 0);
    while s < offsets.len() && y < h {
        let off = whole(offsets[s])?;
        let count = whole(counts.get(s).copied().unwrap_or(0.0))?;
        let start = off.min(b.len());
        let data = &b[start..off.saturating_add(count).clamp(start, b.len())];
        let want = rps.min(h - y) * stride;
        let owned;
        let part: &[u8] = if comp == 1.0 {
            data
        } else {
            owned = if comp == 32773.0 {
                pack_bits(data, want)
            } else if comp == 5.0 {
                tiff_lzw(data, want)
            } else if comp == 8.0 || comp == 32946.0 {
                inflate_zlib(data, want + 1024).ok()?
            } else {
                return None;
            };
            &owned
        };
        let n = part.len().min(want);
        raw[y * stride..y * stride + n].copy_from_slice(&part[..n]);
        s += 1;
        y += rps;
    }
    // Horizontal predictor: each sample stored as the difference with the one on its left (8 and 16 bits).
    if pred == 2.0 && bps >= 8 {
        let bytes = bps / 8;
        let step = spp * bytes;
        for y in 0..h {
            let row = y * stride;
            if bytes == 1 {
                for i in row + step..row + stride {
                    raw[i] = raw[i].wrapping_add(raw[i - step]);
                }
                continue;
            }
            let get = |raw: &[u8], i: usize| {
                let (x, y) = (raw[i] as u32, raw[i + 1] as u32);
                if t.le { x | y << 8 } else { x << 8 | y }
            };
            let mut i = row + step;
            while i + 1 < row + stride {
                let v = (get(&raw, i) + get(&raw, i - step)) & 0xffff;
                let (lo, hi) = (v as u8, (v >> 8) as u8);
                raw[i..i + 2].copy_from_slice(&if t.le { [lo, hi] } else { [hi, lo] });
                i += 2;
            }
        }
    }
    // A sample as 8 bits: the high byte of 16, smaller depths spread over 0-255.
    let max = (1u32 << bps.min(8)) - 1;
    let byte_at = |i: usize| raw.get(i).map_or(0, |&x| x as u32);
    let sample = |y: usize, k: usize| -> f64 {
        match bps {
            8 => byte_at(y * stride + k) as f64,
            16 => byte_at(y * stride + k * 2 + t.le as usize) as f64,
            _ => {
                let bit = k * bps;
                let v = byte_at(y * stride + (bit >> 3)) >> (8 - bps - (bit & 7)) & max;
                if photo == 3 { v as f64 } else { round(v as f64 * 255.0 / max as f64) }
            }
        }
    };
    let map = t.get(320);
    let extra = t.get(338).and_then(|v| v.first().copied()).unwrap_or(f64::NAN);
    let has_extra = extra != 0.0 && !extra.is_nan();
    // A colour of the map, as the JS reads it: 0 past its end or at a fractional index.
    let colour = |i: f64| {
        let v = map.and_then(|m| (i.fract() == 0.0).then(|| m.get(i as usize)).flatten());
        v.map_or(0.0, |&v| (int32(v) >> 8) as f64)
    };
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (o, k) = ((y * w + x) * 4, x * spp);
            let (mut r, mut g, mut bl, mut a);
            a = 255.0;
            if photo == 3 {
                let (i, n) = (sample(y, k), map.map_or(0.0, |m| m.len() as f64 / 3.0));
                (r, g, bl) = if i >= n { (0.0, 0.0, 0.0) } else { (colour(i), colour(n + i), colour(2.0 * n + i)) };
            } else if photo == 2 {
                (r, g, bl) = (sample(y, k), sample(y, k + 1), sample(y, k + 2));
                if spp > 3 && has_extra {
                    a = sample(y, k + 3);
                }
            } else if photo == 5 {
                let kk = 255.0 - sample(y, k + 3);
                r = (255.0 - sample(y, k)) * kk / 255.0;
                g = (255.0 - sample(y, k + 1)) * kk / 255.0;
                bl = (255.0 - sample(y, k + 2)) * kk / 255.0;
            } else {
                let v = sample(y, k);
                r = if photo == 0 { 255.0 - v } else { v };
                (g, bl) = (r, r);
                if spp > 1 && has_extra {
                    a = sample(y, k + 1);
                }
            }
            // Associated alpha (1): colours already multiplied by it.
            if a < 255.0 && extra == 1.0 && a != 0.0 {
                (r, g, bl) = (r * 255.0 / a, g * 255.0 / a, bl * 255.0 / a);
            }
            out[o..o + 4].copy_from_slice(&[byte(r), byte(g), byte(bl), byte(a)]);
        }
    }
    Some(Image { width: w, height: h, data: out })
}

// PackBits: a count n, then n + 1 bytes as they are (n ≥ 0) or the next byte 1 - n times (n < 0).
fn pack_bits(src: &[u8], want: usize) -> Vec<u8> {
    let mut out = vec![0u8; want];
    let (mut i, mut o) = (0, 0);
    while i < src.len() && o < want {
        let n = src[i] as i8 as i32;
        i += 1;
        if n >= 0 {
            let end = (i + n as usize + 1).min(src.len());
            while i < end && o < want {
                out[o] = src[i];
                (o, i) = (o + 1, i + 1);
            }
        } else if n != -128 {
            let v = at(src, i) as u8;
            i += 1;
            let mut k = 0;
            while k < 1 - n && o < want {
                out[o] = v;
                (o, k) = (o + 1, k + 1);
            }
        }
    }
    out
}

// TIFF's LZW: codes of 9 to 12 bits, most significant bit first, 256 to clear the table and 257 to end; the code
// size grows one code earlier than in GIF.
fn tiff_lzw(src: &[u8], want: usize) -> Vec<u8> {
    struct Table {
        prefix: [u16; 4096],
        suffix: [u8; 4096],
        first: [u8; 4096],
        len: [u16; 4096],
    }
    let mut t = Box::new(Table { prefix: [0; 4096], suffix: [0; 4096], first: [0; 4096], len: [0; 4096] });
    for c in 0..256 {
        (t.suffix[c], t.first[c], t.len[c]) = (c as u8, c as u8, 1);
    }
    let mut out = vec![0u8; want];
    let (mut bits, mut next, mut prev, mut pos, mut o) = (9u32, 258usize, None::<usize>, 0usize, 0usize);
    let total = src.len() * 8;
    let write = |t: &Table, out: &mut [u8], o: &mut usize, c: usize| {
        let n = t.len[c] as usize;
        let mut x = c;
        for k in (0..n).rev() {
            if let Some(p) = out.get_mut(*o + k) {
                *p = t.suffix[x];
            }
            x = t.prefix[x] as usize;
        }
        *o += n;
    };
    while o < want && pos + bits as usize <= total {
        let i = pos >> 3;
        let word = at(src, i) << 16 | at(src, i + 1) << 8 | at(src, i + 2);
        let code = (word >> (24 - bits - (pos & 7) as u32) & ((1 << bits) - 1)) as usize;
        pos += bits as usize;
        if code == 257 {
            break;
        }
        if code == 256 {
            (bits, next, prev) = (9, 258, None);
            continue;
        }
        let Some(p) = prev else {
            if code > 255 {
                break;
            }
            write(&t, &mut out, &mut o, code);
            prev = Some(code);
            continue;
        };
        let head;
        if code < next {
            write(&t, &mut out, &mut o, code);
            head = t.first[code];
        } else if code == next {
            head = t.first[p];
            write(&t, &mut out, &mut o, p);
            if o < want {
                out[o] = head;
            }
            o += 1;
        } else {
            break;
        }
        if next < 4096 {
            (t.prefix[next], t.suffix[next], t.first[next], t.len[next]) = (p as u16, head, t.first[p], t.len[p] + 1);
            next += 1;
        }
        prev = Some(code);
        if next >= (1 << bits) - 1 && bits < 12 {
            bits += 1;
        }
    }
    out
}

// --- scaling

const ICON: f64 = 32.0; // small images are enlarged up to this many pixels on their longer side

// The size of an image scaled to fit max_w × max_h: never enlarged, except small icons by a whole factor (up to 4)
// until their longer side reaches icon.
fn fit(w: f64, h: f64, max_w: f64, max_h: f64, icon: f64) -> (usize, usize) {
    let mut scale = (max_w / w).min(max_h / h);
    if scale >= 1.0 {
        scale = scale.floor().min((icon / w.max(h)).floor()).clamp(1.0, 4.0);
    }
    (round(w * scale).min(max_w).max(1.0) as usize, round(h * scale).min(max_h).max(1.0) as usize)
}

// The sums over the source area of each target pixel of a row (box filter): colours weighted by alpha, and alpha;
// with the number of source pixels.
struct Boxes {
    x0: Vec<usize>,
    x1: Vec<usize>,
    acc: Vec<f64>,
}

impl Boxes {
    fn new(w: usize, tw: usize) -> Boxes {
        let x0: Vec<usize> = (0..tw).map(|x| x * w / tw).collect();
        let x1 = (0..tw).map(|x| (x0[x] + 1).max((x + 1) * w / tw)).collect();
        Boxes { x0, x1, acc: vec![0.0; tw * 4] }
    }

    // Row y of th from an image h rows high: the sums, and the source rows (y0, y1).
    fn row(&mut self, img: &Image, y: usize, th: usize) -> usize {
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
fn decoded(buffer: &[u8]) -> Result<(Option<Info>, Image), String> {
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

// --- rendering

// What shows instead of an image: its format and size when known, and why (an empty note counts as none).
fn describe(meta: Option<&Info>, why: &str) -> String {
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

// --- images in scrolled text
//
// An image placed in pixels over rows cells of a scrolled text (rendered Markdown) shows only where those rows do.

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

// --- WebP, through dwebp

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
