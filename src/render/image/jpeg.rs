//! JPEG decoding, baseline and progressive: the frame, the Huffman tables and scans, the inverse DCT and the
//! colours.

use super::{Image, ascii, at, bad, u16be};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63,
];

/// ToInt32: a number stored in an Int32Array, or before a bitwise operator.
pub(super) fn int32(x: f64) -> i32 {
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

pub(super) type Table = [i32; 64];

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

pub(super) fn decode_jpeg(b: &[u8]) -> Option<Image> {
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
        // The prediction is unbounded: its product goes through a double.
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
        // A band cut short (bounds missing) reads nothing.
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

// Integer IDCT (the jidctint algorithm, as in stb_image): fixed point with 12 fractional bits, in
// i64 (exact below 2^53); wrapped to 32 bits (int32) where it stores or shifts.
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
