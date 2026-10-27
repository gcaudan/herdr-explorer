//! GIF decoding, of the first frame: its blocks and the LZW data.

use super::{Image, at, bad, le16};

pub(super) fn decode_gif(b: &[u8]) -> Option<Image> {
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
            // No code size: no image.
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
