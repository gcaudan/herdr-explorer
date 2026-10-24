//! Inflate (RFC 1951) in a zlib wrapper (RFC 1950), as the JS calls it: `zlib.inflateSync(data, { finishFlush:
//! Z_SYNC_FLUSH, maxOutputLength })`. A stream cut short gives what it decoded so far; more output than allowed, a
//! bad header, bad codes or a wrong checksum are errors. Node writes the output in chunks of 16 KiB and checks its
//! size after each one: a data error met before the chunk holding the byte past the limit is full wins over it.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// More than the allowed output (zlib's "Cannot create a Buffer larger than…").
    TooLarge,
    /// A header, block or code zlib refuses, or a wrong Adler-32.
    Data(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooLarge => f.write_str("output too large"),
            Error::Data(what) => f.write_str(what),
        }
    }
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    buf: u64,
    count: u32,
}

/// The input ran out: what was decoded is kept.
struct End;

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<(), End> {
        while self.count < n {
            let Some(&b) = self.data.get(self.at) else { return Err(End) };
            self.at += 1;
            self.buf |= (b as u64) << self.count;
            self.count += 8;
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> Result<u32, End> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.count -= n;
        Ok(v)
    }

    /// Whatever bits are left of the current byte are dropped.
    fn align(&mut self) {
        let drop = self.count % 8;
        self.buf >>= drop;
        self.count -= drop;
    }

    /// Fills the bit buffer as far as the input goes (up to 32 bits), for a table lookup.
    fn fill(&mut self) {
        while self.count <= 56 {
            let Some(&b) = self.data.get(self.at) else { return };
            self.at += 1;
            self.buf |= (b as u64) << self.count;
            self.count += 8;
        }
    }
}

/// A canonical Huffman code as a lookup table indexed by the next `bits` input bits (LSB first, reversed codes):
/// each entry the symbol << 4 | its length, 0 for no code.
struct Huffman {
    table: Vec<u32>,
    bits: u32,
}

/// What a code is for: zlib refuses an incomplete code of code lengths, and an incomplete literal/length or distance
/// code but for a single code of length 1 (or none at all).
#[derive(PartialEq)]
enum Kind {
    CodeLengths,
    Symbols,
}

impl Huffman {
    fn new(lengths: &[u8], kind: Kind) -> Result<Huffman, Error> {
        let bits = lengths.iter().copied().max().unwrap_or(0) as u32;
        let mut count = [0u32; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        // Over-subscribed codes are refused, as zlib does, and incomplete ones as it does (Kind).
        let mut left: i64 = 1;
        for &c in &count[1..] {
            left = left * 2 - c as i64;
            if left < 0 {
                return Err(Error::Data("over-subscribed code"));
            }
        }
        if bits > 0 && left > 0 && (kind == Kind::CodeLengths || bits != 1) {
            return Err(Error::Data("incomplete code"));
        }
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for len in 1..16 {
            code = (code + count[len - 1]) << 1;
            next[len] = code;
        }
        let size = 1usize << bits.max(1);
        let mut table = vec![0u32; size];
        for (sym, &l) in lengths.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let l = l as u32;
            let c = next[l as usize];
            next[l as usize] += 1;
            let rev = c.reverse_bits() >> (32 - l);
            let mut i = rev as usize;
            while i < size {
                table[i] = (sym as u32) << 4 | l;
                i += 1 << l;
            }
        }
        Ok(Huffman { table, bits: bits.max(1) })
    }

    fn decode(&self, b: &mut Bits) -> Result<Result<u32, Error>, End> {
        b.fill();
        let entry = self.table[(b.buf & ((1u64 << self.bits) - 1)) as usize];
        let len = entry & 15;
        if len == 0 {
            // No code here: either the input ran out before a whole code, or the code is invalid.
            if b.count < self.bits {
                return Err(End);
            }
            return Ok(Err(Error::Data("invalid code")));
        }
        if b.count < len {
            return Err(End);
        }
        b.buf >>= len;
        b.count -= len;
        Ok(Ok(entry >> 4))
    }
}

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

/// Node's output chunk (zlib's Z_DEFAULT_CHUNK): the output size is checked each time one is full.
const CHUNK: usize = 16 * 1024;

/// The output, bounded as Node bounds it.
struct Out {
    data: Vec<u8>,
    max: usize,
}

impl Out {
    fn push(&mut self, b: u8) -> Result<(), Error> {
        self.data.push(b);
        if self.data.len().is_multiple_of(CHUNK) && self.data.len() > self.max {
            return Err(Error::TooLarge);
        }
        Ok(())
    }

    /// The stream decoded (or cut short): the last chunk's size check.
    fn done(self) -> Result<Vec<u8>, Error> {
        if self.data.len() > self.max { Err(Error::TooLarge) } else { Ok(self.data) }
    }
}

/// Inflates a zlib stream; `max` bounds the output.
pub fn inflate_zlib(data: &[u8], max: usize) -> Result<Vec<u8>, Error> {
    if data.len() < 2 {
        return Ok(Vec::new());
    }
    let (cmf, flg) = (data[0], data[1]);
    if !(cmf as u32 * 256 + flg as u32).is_multiple_of(31) {
        return Err(Error::Data("incorrect header check"));
    }
    if cmf & 15 != 8 {
        return Err(Error::Data("unknown compression method"));
    }
    if cmf >> 4 > 7 {
        return Err(Error::Data("invalid window size"));
    }
    // A preset dictionary: zlib asks for it once its id is read, and waits for more input before.
    if flg & 0x20 != 0 {
        return if data.len() < 6 { Ok(Vec::new()) } else { Err(Error::Data("need dictionary")) };
    }
    let mut bits = Bits { data: &data[2..], at: 0, buf: 0, count: 0 };
    let mut out = Out { data: Vec::new(), max };
    if blocks(&mut bits, &mut out)? {
        bits.align();
        let mut sum = 0u32;
        for _ in 0..4 {
            match bits.take(8) {
                Ok(b) => sum = sum << 8 | b,
                Err(End) => return out.done(),
            }
        }
        if sum != adler32(&out.data) {
            return Err(Error::Data("incorrect data check"));
        }
    }
    out.done()
}

/// Inflates raw deflate blocks into out; true once the last block ended, false when the input ran out first.
fn blocks(bits: &mut Bits, out: &mut Out) -> Result<bool, Error> {
    loop {
        let Ok(head) = bits.take(3) else { return Ok(false) };
        let last = head & 1 == 1;
        let ended = match head >> 1 {
            0 => stored(bits, out)?,
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let lit = Huffman::new(&lengths, Kind::Symbols)?;
                // 32 codes, the last two invalid distances.
                let dist = Huffman::new(&[5u8; 32], Kind::Symbols)?;
                codes(bits, out, &lit, &dist)?
            }
            2 => match dynamic(bits)? {
                None => false,
                Some((lit, dist)) => codes(bits, out, &lit, &dist)?,
            },
            _ => return Err(Error::Data("invalid block type")),
        };
        if !ended {
            return Ok(false);
        }
        if last {
            return Ok(true);
        }
    }
}

fn stored(bits: &mut Bits, out: &mut Out) -> Result<bool, Error> {
    bits.align();
    let (Ok(len), Ok(nlen)) = (bits.take(16), bits.take(16)) else { return Ok(false) };
    if len != !nlen & 0xffff {
        return Err(Error::Data("invalid stored block lengths"));
    }
    // What the bit buffer holds is whole bytes now.
    for _ in 0..len {
        let Ok(b) = bits.take(8) else { return Ok(false) };
        out.push(b as u8)?;
    }
    Ok(true)
}

fn dynamic(bits: &mut Bits) -> Result<Option<(Huffman, Huffman)>, Error> {
    let Ok(h) = bits.take(14) else { return Ok(None) };
    let (nlen, ndist, ncode) = ((h & 31) as usize + 257, (h >> 5 & 31) as usize + 1, (h >> 10) as usize + 4);
    if nlen > 286 || ndist > 30 {
        return Err(Error::Data("too many length or distance symbols"));
    }
    let mut code_lengths = [0u8; 19];
    for &i in &ORDER[..ncode] {
        let Ok(l) = bits.take(3) else { return Ok(None) };
        code_lengths[i] = l as u8;
    }
    let code = Huffman::new(&code_lengths, Kind::CodeLengths)?;
    let mut lengths = vec![0u8; nlen + ndist];
    let mut i = 0;
    while i < nlen + ndist {
        let sym = match code.decode(bits) {
            Err(End) => return Ok(None),
            Ok(r) => r?,
        };
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                if i == 0 {
                    return Err(Error::Data("invalid bit length repeat"));
                }
                let Ok(r) = bits.take(2) else { return Ok(None) };
                (lengths[i - 1], 3 + r as usize)
            }
            17 => {
                let Ok(r) = bits.take(3) else { return Ok(None) };
                (0, 3 + r as usize)
            }
            _ => {
                let Ok(r) = bits.take(7) else { return Ok(None) };
                (0, 11 + r as usize)
            }
        };
        if i + repeat > nlen + ndist {
            return Err(Error::Data("invalid bit length repeat"));
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err(Error::Data("invalid code -- missing end-of-block"));
    }
    Ok(Some((Huffman::new(&lengths[..nlen], Kind::Symbols)?, Huffman::new(&lengths[nlen..], Kind::Symbols)?)))
}

fn codes(bits: &mut Bits, out: &mut Out, lit: &Huffman, dist: &Huffman) -> Result<bool, Error> {
    loop {
        let sym = match lit.decode(bits) {
            Err(End) => return Ok(false),
            Ok(r) => r?,
        };
        match sym {
            0..=255 => out.push(sym as u8)?,
            256 => return Ok(true),
            257..=285 => {
                let k = (sym - 257) as usize;
                let Ok(extra) = bits.take(LEN_EXTRA[k] as u32) else { return Ok(false) };
                let len = LEN_BASE[k] as usize + extra as usize;
                let d = match dist.decode(bits) {
                    Err(End) => return Ok(false),
                    Ok(r) => r? as usize,
                };
                if d >= 30 {
                    return Err(Error::Data("invalid distance code"));
                }
                let Ok(extra) = bits.take(DIST_EXTRA[d] as u32) else { return Ok(false) };
                let back = DIST_BASE[d] as usize + extra as usize;
                if back > out.data.len() {
                    return Err(Error::Data("invalid distance too far back"));
                }
                let from = out.data.len() - back;
                for j in 0..len {
                    out.push(out.data[from + j])?;
                }
            }
            _ => return Err(Error::Data("invalid literal/length code")),
        }
    }
}
