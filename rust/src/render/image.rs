//! Image preview: format and size read from the header (PNG, JPEG, GIF, BMP, WebP, TIFF, ICO, SVG), decoders for PNG,
//! GIF, BMP and ICO (JPEG and TIFF come next). WebP goes through dwebp, when installed, which turns it into a PNG.
//! Files are untrusted: sizes are checked before allocating, every loop is bounded and nothing panics. Where the JS
//! reads past the end of a buffer (undefined, 0 in bitwise operations), the reads here give 0.

use crate::codec::inflate::inflate_zlib;
use crate::core::exec::{self, Opts};
use crate::core::regex::Regex;
use crate::core::{js, paths, which};
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
    Some(Tags { tags })
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
    if ascii(b, 0, 4) == "GIF8" {
        return decode_gif(b);
    }
    if b[0] == 0x42 && b[1] == 0x4d {
        return decode_dib(b, 14, Some(le32(b, 10) as usize), false);
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
