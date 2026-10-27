//! Base64: written padded with =; read with the URL-safe alphabet too, skipping characters outside the alphabet (any non-ASCII one included) and stops at the
//! first =.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn value(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => return None,
    } as u32)
}

pub fn decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut n, mut k) = (0u32, 0);
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = value(c) else { continue };
        n = n << 6 | v;
        k += 1;
        if k == 4 {
            out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
            (n, k) = (0, 0);
        }
    }
    match k {
        2 => out.push((n >> 4) as u8),
        3 => out.extend_from_slice(&[(n >> 10) as u8, (n >> 2) as u8]),
        _ => {}
    }
    out
}
