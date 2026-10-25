//! `localeCompare` as Node's ICU does it with the root collation, approached: the order of ASCII's punctuation and of
//! the Latin letters comes from Node (tools/unicode/collation.js); the rest follows the same plan by code point:
//! white space and punctuation, then other symbols, then currency signs, then digits, then Latin letters, then the
//! other scripts' letters. Accents count after the letters, case after the accents, lowercase first; control characters
//! do not count. A letter followed by combining accents sorts as the letter composed when Latin has one.

use super::collation::{AFTER, COMPOSE, CURRENCY, FOLD, PUNCT, ZEROS};
use std::cmp::Ordering;

/// One collation element: its weight at each level ICU compares in turn (0 primary: an accent of its own).
struct Element {
    primary: u32,
    secondary: u32,
    tertiary: u8,
}

const SYMBOL: u32 = 0x100;
const MONEY: u32 = 0x18_0000;
const DIGIT: u32 = 0x20_0000;
const NUMBER: u32 = DIGIT + 0x10;
const LATIN: u32 = 0x30_0000;
const LETTER: u32 = 0x40_0000;

fn latin(letter: u8) -> u32 {
    LATIN + (letter - b'a') as u32 * 0x1000
}

/// The value of a decimal digit, of any script.
fn digit(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        return Some(c as u32 - '0' as u32);
    }
    let i = ZEROS.partition_point(|&z| z <= c as u32);
    let value = c as u32 - ZEROS.get(i.checked_sub(1)?)?;
    (value < 10).then_some(value)
}

/// The characters, each letter and the combining accents after it composed when Latin has the letter composed.
fn composed(s: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(s.len());
    for c in s.chars() {
        if let Some(&last) = out.last()
            && let Ok(i) = COMPOSE.binary_search_by_key(&(last as u32, c as u32), |&(a, b, _)| (a, b))
        {
            *out.last_mut().unwrap() = char::from_u32(COMPOSE[i].2).unwrap_or(last);
            continue;
        }
        out.push(c);
    }
    out
}

fn elements(s: &str, numeric: bool, out: &mut Vec<Element>) {
    let mut chars = composed(s).into_iter().peekable();
    while let Some(c) = chars.next() {
        let upper = c.is_uppercase() as u8;
        let lower = c.to_lowercase().next().unwrap_or(c);
        let mut push = |primary, secondary| out.push(Element { primary, secondary, tertiary: upper });
        if let Some(d) = digit(c) {
            if !numeric {
                push(DIGIT + d, 0);
                continue;
            }
            // A run of digits counts as its number: its length without leading zeros first, then its digits.
            let mut digits = vec![d];
            while let Some(d) = chars.peek().and_then(|&c| digit(c)) {
                digits.push(d);
                chars.next();
            }
            let start = digits.iter().position(|&d| d != 0).unwrap_or(digits.len());
            push(NUMBER + (digits.len() - start).min(0xF_FFFF) as u32, 0);
            for &d in &digits[start..] {
                push(DIGIT + d, 0);
            }
        } else if let Some(i) = CURRENCY.iter().position(|&m| m == c as u32) {
            push(MONEY + i as u32, 0);
        } else if c.is_ascii_alphabetic() {
            push(latin(lower as u8), 0);
        } else if c.is_ascii() || c == '\u{85}' {
            if let Ok(i) = PUNCT.binary_search_by_key(&(c as u8), |&(c, _)| c) {
                push(PUNCT[i].1 as u32, 0);
            }
            // The other controls do not count.
        } else if let Ok(i) = FOLD.binary_search_by_key(&(c as u32), |&(c, _, _)| c) {
            let (_, plain, accent) = FOLD[i];
            for (n, b) in plain.bytes().enumerate() {
                push(latin(b), if n == 0 { accent as u32 } else { 0 });
            }
        } else if let Ok(i) = AFTER.binary_search_by_key(&(lower as u32), |&(c, _)| c) {
            let after = AFTER[i].1 as u8;
            push(latin(after) + 1 + i as u32, 0);
        } else if super::is_mark(c) {
            push(0, 0x100 + c as u32);
        } else if super::is_letter(c) {
            push(LETTER + lower as u32, 0);
        } else if super::is_space(c) {
            // Another space sorts as the space, after it.
            let space = PUNCT.binary_search_by_key(&b' ', |&(c, _)| c).map_or(0, |i| PUNCT[i].1 as u32);
            out.push(Element { primary: space, secondary: 0, tertiary: 2 });
        } else if c.is_control() {
            // Does not count.
        } else {
            push(SYMBOL + c as u32, 0);
        }
    }
}

/// `a.localeCompare(b)` (the default, every level), or with `{ numeric: true, sensitivity: 'base' }` (`numeric_base`:
/// runs of digits compare as numbers, only the letters themselves count).
pub fn locale_compare(a: &str, b: &str, numeric_base: bool) -> Ordering {
    let (mut x, mut y) = (Vec::new(), Vec::new());
    elements(a, numeric_base, &mut x);
    elements(b, numeric_base, &mut y);
    let primary = |v: &[Element]| v.iter().filter(|e| e.primary != 0).map(|e| e.primary).collect::<Vec<_>>();
    let order = primary(&x).cmp(&primary(&y));
    if order != Ordering::Equal || numeric_base {
        return order;
    }
    let secondary = |v: &[Element]| v.iter().map(|e| e.secondary).collect::<Vec<_>>();
    let order = secondary(&x).cmp(&secondary(&y));
    if order != Ordering::Equal {
        return order;
    }
    let tertiary = |v: &[Element]| v.iter().filter(|e| e.primary != 0).map(|e| e.tertiary).collect::<Vec<_>>();
    tertiary(&x).cmp(&tertiary(&y))
}
