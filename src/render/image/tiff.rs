//! TIFF decoding: strips uncompressed, PackBits or LZW.

use super::jpeg::int32;
use super::{Image, MAX_RAW, at, bad, byte, round, tiff_tags};
use crate::codec::inflate::inflate_zlib;

// A whole number, or None: sizes, offsets and rows per strip that are not (rationals, empty tags) give absurd images
// in the JS (NaN lengths, fractional indices), none here.
fn whole(x: f64) -> Option<usize> {
    (x >= 0.0 && x.fract() == 0.0 && x < 9007199254740992.0).then_some(x as usize)
}

pub(super) fn decode_tiff(b: &[u8]) -> Option<Image> {
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
