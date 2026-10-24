//! Deflate (RFC 1951) in a zlib wrapper, for the Kitty graphics protocol (`zlib.deflateSync` in the JS): LZ77 over a
//! 32 KiB window with hash chains, fixed Huffman codes. The bytes differ from zlib's (registered gap); what they
//! inflate to is the same.

use super::inflate::adler32;

struct Writer {
    out: Vec<u8>,
    buf: u64,
    count: u32,
}

impl Writer {
    fn put(&mut self, value: u32, n: u32) {
        self.buf |= (value as u64) << self.count;
        self.count += n;
        while self.count >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.count -= 8;
        }
    }

    /// A Huffman code, written from its most significant bit.
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn flush(&mut self) {
        if self.count > 0 {
            self.out.push(self.buf as u8);
            self.buf = 0;
            self.count = 0;
        }
    }
}

/// The fixed code of a literal/length symbol: (code, length).
fn fixed(sym: u32) -> (u32, u32) {
    match sym {
        0..=143 => (0x30 + sym, 8),
        144..=255 => (0x190 + sym - 144, 9),
        256..=279 => (sym - 256, 7),
        _ => (0xC0 + sym - 280, 8),
    }
}

const LEN_BASE: [u32; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];

const LEN_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

fn length(w: &mut Writer, len: u32) {
    let k = LEN_BASE.iter().rposition(|&b| b <= len).unwrap();
    let (code, n) = fixed(257 + k as u32);
    w.code(code, n);
    w.put(len - LEN_BASE[k], LEN_EXTRA[k]);
}

fn distance(w: &mut Writer, d: u32) {
    let k = DIST_BASE.iter().rposition(|&b| b <= d).unwrap();
    w.code(k as u32, 5);
    let extra = if k < 2 { 0 } else { (k as u32 - 2) / 2 };
    w.put(d - DIST_BASE[k], extra);
}

const WINDOW: usize = 32768;
const HASH: usize = 1 << 15;
const MAX_CHAIN: usize = 64;

/// Deflates data in a zlib stream (header 78 9C, as zlib's default level writes).
pub fn deflate_zlib(data: &[u8]) -> Vec<u8> {
    let mut w = Writer { out: vec![0x78, 0x9c], buf: 0, count: 0 };
    w.put(1, 1); // the last block
    w.put(1, 2); // fixed codes
    let mut head = vec![usize::MAX; HASH];
    let mut prev = vec![usize::MAX; WINDOW];
    let hash = |i: usize| -> usize {
        ((data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize) & (HASH - 1)
    };
    let mut i = 0;
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + 2 < data.len() {
            let h = hash(i);
            prev[i % WINDOW] = head[h];
            head[h] = i;
        }
    };
    while i < data.len() {
        let mut best = (0usize, 0usize);
        if i + 2 < data.len() {
            let mut cand = head[hash(i)];
            let mut chain = 0;
            while cand != usize::MAX && i - cand < WINDOW && chain < MAX_CHAIN {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, i - cand);
                    if l == max {
                        break;
                    }
                }
                let next = prev[cand % WINDOW];
                if next == usize::MAX || next >= cand {
                    break;
                }
                cand = next;
                chain += 1;
            }
        }
        if best.0 >= 3 {
            length(&mut w, best.0 as u32);
            distance(&mut w, best.1 as u32);
            for j in i..i + best.0 {
                insert(&mut head, &mut prev, j);
            }
            i += best.0;
        } else {
            let (code, n) = fixed(data[i] as u32);
            w.code(code, n);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    let (code, n) = fixed(256);
    w.code(code, n);
    w.flush();
    w.out.extend_from_slice(&adler32(data).to_be_bytes());
    w.out
}

#[cfg(test)]
mod tests {
    use super::super::inflate::inflate_zlib;
    use super::*;

    #[test]
    fn round_trips() {
        let mut noise = Vec::new();
        let mut x = 12345u32;
        for _ in 0..100_000 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            noise.push((x >> 16) as u8 % 7);
        }
        // Every match length: a run of 300 distinct bytes, then each prefix of it again.
        let mut lengths: Vec<u8> = (0..300u32).map(|i| (i * 7 % 251) as u8).collect();
        for n in 3..=258 {
            lengths.extend_from_slice(&lengths[..300].to_vec()[..n]);
            lengths.push(255);
        }
        for data in [&b""[..], b"a", b"abcabcabcabcabcabc", &[0u8; 70_000][..], &noise, &lengths] {
            let packed = deflate_zlib(data);
            assert_eq!(inflate_zlib(&packed, usize::MAX).unwrap(), data);
        }
        assert!(deflate_zlib(&[0u8; 70_000]).len() < 1000);
    }
}
