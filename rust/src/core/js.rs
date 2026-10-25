//! What the JS took from the language itself and the port needs as it behaved: parseInt, the line terminators that
//! a regex's `.` stops at, `\s`, toLowerCase, String(x) on a JSON value, UTF-16 lengths.

use crate::json::{self, Value};

/// `parseInt(s, 10)`: leading white space, a sign, then the longest run of digits; None (NaN) without one.
pub fn parse_int(s: &str) -> Option<f64> {
    let s = s.trim_start_matches(is_space);
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let n: f64 = rest[..digits].parse().ok()?;
    Some(if neg { -n } else { n })
}

/// `parseInt(s, 10) || 0` as an integer, as the JS uses it for counts.
pub fn int_or_zero(s: &str) -> f64 {
    match parse_int(s) {
        Some(n) if n != 0.0 => n,
        _ => 0.0,
    }
}

/// The characters a regex's `.` does not match.
pub fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// A regex's `\s`.
pub fn is_space(c: char) -> bool {
    crate::unicode::is_space(c)
}

/// `toLowerCase()` (Rust's lowercase mapping; the Unicode version can lag Node's by one).
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// How many UTF-16 units the text takes: JS's `length`.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// The byte offset of the UTF-16 unit `units` of s (its length past the end); None inside a surrogate pair.
pub fn unit_at(s: &str, units: usize) -> Option<usize> {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= units {
            return (n == units).then_some(i);
        }
        n += c.len_utf16();
    }
    (n <= units).then_some(s.len())
}

pub use crate::unicode::locale_compare;

/// `x === y` on fields of JSON values (None: undefined): an object or an array is never equal to another.
pub fn strict_eq(x: Option<&Value>, y: Option<&Value>) -> bool {
    match (x, y) {
        (None, None) => true,
        (Some(Value::Obj(_) | Value::Arr(_)), _) | (_, Some(Value::Obj(_) | Value::Arr(_))) => false,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// `Number(x)` of a field (None: undefined), as `a - b` reads its operands: NaN for what is not a number.
pub fn number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Num(n)) => *n,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => *b as u8 as f64,
        Some(Value::Str(s)) => string_to_number(s),
        Some(v @ Value::Arr(_)) => string_to_number(&string(v)),
        _ => f64::NAN,
    }
}

/// A string read as a number, as JS's `Number(s)`: white space around it, Infinity, 0x, 0o and 0b literals, else a
/// decimal literal; NaN for anything else.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_space);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| {
        let digits = t.get(2..).filter(|_| t[..2].eq_ignore_ascii_case(p))?;
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(r)) {
            return Some(f64::NAN);
        }
        Some(digits.chars().fold(0.0, |n, c| n * r as f64 + c.to_digit(r).unwrap_or(0) as f64))
    };
    if t.len() > 2
        && let Some(n) = radix("0x", 16).or_else(|| radix("0o", 8)).or_else(|| radix("0b", 2))
    {
        return n;
    }
    let (sign, body) = match t.as_bytes()[0] {
        b'-' => (-1.0, &t[1..]),
        b'+' => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    // [digits][.digits][e[+-]digits], with a digit before or after the point.
    let b = body.as_bytes();
    let digits = |i: usize| b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(0);
    let mut i = int;
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        frac = digits(i + 1);
        i += 1 + frac;
    }
    if int + frac == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp = digits(j);
        if exp == 0 {
            return f64::NAN;
        }
        i = j + exp;
    }
    if i != b.len() {
        return f64::NAN;
    }
    body.parse::<f64>().map_or(f64::NAN, |n| sign * n)
}

/// `x > y` on fields of JSON values (None: undefined): two strings (objects and arrays become theirs) compared by
/// UTF-16 units, anything else as numbers.
pub fn greater(x: Option<&Value>, y: Option<&Value>) -> bool {
    let text = |v: Option<&Value>| match v {
        Some(Value::Str(s)) => Some(s.clone()),
        Some(v @ (Value::Arr(_) | Value::Obj(_))) => Some(string(v)),
        _ => None,
    };
    match (text(x), text(y)) {
        (Some(a), Some(b)) => a.encode_utf16().cmp(b.encode_utf16()) == std::cmp::Ordering::Greater,
        (a, b) => {
            let num = |t: Option<String>, v: Option<&Value>| t.map_or_else(|| number(v), |t| string_to_number(&t));
            num(a, x) > num(b, y)
        }
    }
}

/// `Array.prototype.sort` with a comparator giving a JS number, as V8 sorts: stable, and with V8's result for a
/// comparator that is not a total order (NaN, inconsistent answers) on lists shorter than 64 items, which V8 sorts as
/// one run: its first ascending or strictly descending run, then binary insertion. Longer lists are merged from runs
/// of 32 so sorted, which V8's TimSort only approaches with such a comparator.
pub fn sort_by<T>(v: &mut Vec<T>, mut cmp: impl FnMut(&T, &T) -> f64) {
    fn run<T>(v: &mut [T], cmp: &mut impl FnMut(&T, &T) -> f64) {
        if v.len() < 2 {
            return;
        }
        // CountAndMakeRun.
        let descending = cmp(&v[1], &v[0]) < 0.0;
        let mut len = 2;
        while len < v.len() {
            let order = cmp(&v[len], &v[len - 1]);
            if if descending { order >= 0.0 } else { order < 0.0 } {
                break;
            }
            len += 1;
        }
        if descending {
            v[..len].reverse();
        }
        // BinaryInsertionSort.
        for i in len..v.len() {
            let (mut left, mut right) = (0, i);
            while left < right {
                let mid = left + ((right - left) >> 1);
                if cmp(&v[i], &v[mid]) < 0.0 { right = mid } else { left = mid + 1 }
            }
            v[left..=i].rotate_right(1);
        }
    }
    if v.len() < 64 {
        run(v, &mut cmp);
        return;
    }
    for chunk in v.chunks_mut(32) {
        run(chunk, &mut cmp);
    }
    let mut width = 32;
    while width < v.len() {
        let mut out: Vec<T> = Vec::with_capacity(v.len());
        let mut items = std::mem::take(v).into_iter();
        loop {
            let a: Vec<T> = items.by_ref().take(width).collect();
            if a.is_empty() {
                break;
            }
            let b: Vec<T> = items.by_ref().take(width).collect();
            let (mut a, mut b) = (a.into_iter().peekable(), b.into_iter().peekable());
            while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
                // Stable: the left one first unless the right one is less.
                let next = if cmp(y, x) < 0.0 { b.next() } else { a.next() };
                out.extend(next);
            }
            out.extend(a);
            out.extend(b);
        }
        *v = out;
        width *= 2;
    }
}

/// `x || y` on numbers: y when x is 0 or NaN.
pub fn or(x: f64, y: impl FnOnce() -> f64) -> f64 {
    if x != 0.0 && !x.is_nan() { x } else { y() }
}

/// `String(x)` of a JSON value: what a template literal or a String() call writes.
pub fn string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) if n.is_nan() => "NaN".into(),
        Value::Num(n) if n.is_infinite() => if *n > 0.0 { "Infinity" } else { "-Infinity" }.into(),
        Value::Num(n) => json::number(*n),
        Value::Str(s) => s.clone(),
        Value::Arr(items) => {
            items.iter().map(|x| if x.is_null() { String::new() } else { string(x) }).collect::<Vec<_>>().join(",")
        }
        Value::Obj(_) => "[object Object]".into(),
    }
}

/// Every character not kept replaced by `_`, once per UTF-16 unit, as `s.replace(/[^…]/g, '_')` does.
pub fn replace_chars(s: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if keep(c) {
            out.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                out.push('_');
            }
        }
    }
    out
}

/// A regex's `\w`.
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `Math.round`: halves rounded up (x + 0.5 floored would round 0.49999999999999994 up).
pub fn round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `x.toFixed(digits)`, for |x| < 1e21. Rust rounds an exact tie to even, JS away from zero: such a tie (the exact
/// value ending with a 5 right after the digits kept) is moved off it, away from zero.
pub fn to_fixed(x: f64, digits: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let exact = format!("{:.*}", digits + 40, x.abs());
    let tail = &exact[exact.len() - 40..];
    let x = if tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0') {
        x + x.signum() * 0.25 * 10f64.powi(-(digits as i32))
    } else {
        x
    };
    // -0 is written without its sign; any other negative number keeps it, even rounded to zero.
    format!("{:.*}", digits, if x == 0.0 { 0.0 } else { x })
}

/// `Date.now()`: milliseconds since the epoch.
pub fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// A time as Node's `stat.mtimeMs` gives it: milliseconds, with the fraction.
pub fn time_ms(t: std::time::SystemTime) -> f64 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as f64 * 1e3 + d.subsec_nanos() as f64 / 1e6,
        Err(e) => -(e.duration().as_secs() as f64 * 1e3 + e.duration().subsec_nanos() as f64 / 1e6),
    }
}

/// `decodeURIComponent(s)`: its %XX escapes decoded as UTF-8; None where the JS throws (a % not followed by two hex
/// digits, bytes that are not UTF-8).
pub fn decode_uri_component(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok())?;
            if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_rounds_ties_away_from_zero() {
        assert_eq!(to_fixed(0.0078125, 6), "0.007813");
        assert_eq!(to_fixed(-0.0078125, 6), "-0.007813");
        assert_eq!(to_fixed(0.1234565, 6), "0.123456"); // below the tie in binary
        assert_eq!(to_fixed(0.05, 6), "0.050000");
        assert_eq!(to_fixed(-0.0000001, 6), "-0.000000");
        assert_eq!(to_fixed(-0.0, 6), "0.000000");
        assert_eq!(to_fixed(f64::NAN, 4), "NaN");
        assert_eq!(to_fixed(f64::NEG_INFINITY, 4), "-Infinity");
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
    }

    #[test]
    fn numbers_read_as_node_reads_them() {
        let cases: [(&str, f64); 24] = [
            (" 0x10 ", 16.0),
            ("0b11", 3.0),
            ("0o7", 7.0),
            ("inf", f64::NAN),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("+Infinity", f64::INFINITY),
            ("1e", f64::NAN),
            (".5", 0.5),
            ("5.", 5.0),
            ("+.5e-3", 0.0005),
            ("1_0", f64::NAN),
            ("-0x10", f64::NAN),
            ("0x", f64::NAN),
            ("", 0.0),
            (" \n", 0.0),
            ("1e+", f64::NAN),
            ("1.5E2", 150.0),
            ("00012", 12.0),
            ("--1", f64::NAN),
            (".", f64::NAN),
            ("e5", f64::NAN),
            ("0X1f", 31.0),
            ("\u{a0}1\u{2028}", 1.0),
        ];
        for (s, want) in cases {
            let got = string_to_number(s);
            assert!(got == want || (got.is_nan() && want.is_nan()), "{s:?}: {got} != {want}");
        }
    }

    #[test]
    fn greater_compares_strings_as_strings() {
        let s = |t: &str| Value::Str(t.into());
        assert!(!greater(Some(&s("10")), Some(&s("9"))));
        assert!(greater(Some(&Value::Num(10.0)), Some(&s("9"))));
        assert!(!greater(None, Some(&Value::Num(1.0))));
        assert!(greater(Some(&Value::Num(1.0)), Some(&Value::Null)));
    }

    #[test]
    fn decode_uri_component_throws_as_the_js() {
        assert_eq!(decode_uri_component("a%20b%C3%A9").as_deref(), Some("a bé"));
        assert_eq!(decode_uri_component("%5C%5Chost").as_deref(), Some(r"\\host"));
        assert_eq!(decode_uri_component("100%"), None);
        assert_eq!(decode_uri_component("%zz"), None);
        assert_eq!(decode_uri_component("%C3x"), None);
        assert_eq!(decode_uri_component("é%41").as_deref(), Some("éA"));
    }
}
