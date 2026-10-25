//! PNG decoding: the chunks, the zlib data inflated within bounds, the filters undone and Adam7 interlacing.

use super::{Image, MAX_RAW, ascii, bad, u16be, u32be};
use crate::codec::inflate::inflate_zlib;

const ADAM7: [[usize; 4]; 7] =
    [[0, 0, 8, 8], [4, 0, 8, 8], [0, 4, 4, 8], [2, 0, 4, 4], [0, 2, 2, 4], [1, 0, 2, 2], [0, 1, 1, 2]];

pub(super) fn decode_png(b: &[u8]) -> Option<Image> {
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
