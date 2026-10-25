//! BMP decoding, and the bitmaps inside ICO files.

use super::png::decode_png;
use super::{Image, bad, byte, ico_entry, is_png, le16, le32, sle32};

fn shift_of(m: u32) -> u32 {
    if m == 0 { 32 } else { m.trailing_zeros() }
}

// off: start of the DIB header; data: start of the pixels (None: right after the palette). ico: the height counts
// the colour bitmap and the 1-bit transparency mask that follows it.
pub(super) fn decode_dib(b: &[u8], off: usize, data: Option<usize>, ico: bool) -> Option<Image> {
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

pub(super) fn decode_ico(b: &[u8]) -> Option<Image> {
    let e = ico_entry(b)?;
    let off = e.off as usize;
    if off >= b.len() {
        return None;
    }
    let d = &b[off..b.len().min(off + e.size as usize)];
    if is_png(d) { decode_png(d) } else { decode_dib(d, 0, None, true) }
}
