//! JSON read and written: `parse` keeps an object's keys in the order first seen (a repeated key takes the last
//! value), `stringify` and `stringify_pretty` write it compact or indented, numbers in their shortest form. State files and settings keep their format through it.

use std::fmt::Write as _;

/// A JSON value. Objects keep their keys in insertion order, array indexes aside (insert).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// The value of an object's key, `None` for a missing key or a value that is not an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets an object's key, in place when it exists, else at the end; a value that is not an object becomes one.
    pub fn set(&mut self, key: &str, value: Value) {
        if !matches!(self, Value::Obj(_)) {
            *self = Value::Obj(Vec::new());
        }
        if let Value::Obj(fields) = self {
            insert(fields, key.to_string(), value);
        }
    }

    /// Removes an object's key; returns its value.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        match self {
            Value::Obj(fields) => fields.iter().position(|(k, _)| k == key).map(|i| fields.remove(i).1),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Vec<(String, Value)>> {
        match self {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Truthiness: false, 0, NaN, "" and null are falsy.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Arr(_) | Value::Obj(_) => true,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Num(n)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Num(n as f64)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::Arr(v.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

/// Builds an object from `key => value` pairs: `obj! { "kind" => "rev", "rev" => s }`.
#[macro_export]
macro_rules! obj {
    ($($k:expr => $v:expr),* $(,)?) => {
        $crate::json::Value::Obj(vec![$(($k.to_string(), $crate::json::Value::from($v))),*])
    };
}

/// Sets a key of an object's fields: in place when it exists; else an array index ("0", "17")
/// among the other indexes in numeric order, before every other key, and any other key at the end.
pub fn insert(fields: &mut Vec<(String, Value)>, key: String, value: Value) {
    if let Some((_, v)) = fields.iter_mut().find(|(k, _)| *k == key) {
        *v = value;
        return;
    }
    match index(&key) {
        Some(n) => {
            let at = fields.iter().position(|(k, _)| index(k).is_none_or(|m| m > n)).unwrap_or(fields.len());
            fields.insert(at, (key, value));
        }
        None => fields.push((key, value)),
    }
}

/// The array index a key stands for: digits without a leading zero, below 2^32 - 1.
fn index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|n| *n < u32::MAX)
}

// --- parse

/// Parses a JSON text; `Err` holds a short reason.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), at: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.at < p.s.len() {
        return Err(format!("unexpected character at {}", p.at));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.s.get(self.at) {
            self.at += 1;
        }
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.s[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    fn fail<T>(&self) -> Result<T, String> {
        Err(match self.s.get(self.at) {
            Some(_) => format!("unexpected character at {}", self.at),
            None => "unexpected end of JSON input".into(),
        })
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        // A deeper text is refused rather than overflowing the stack.
        if depth > 512 {
            return Err("nested too deeply".into());
        }
        match self.s.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut fields: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(Value::Obj(fields));
                }
                loop {
                    self.ws();
                    if self.s.get(self.at) != Some(&b'"') {
                        return self.fail();
                    }
                    let key = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return self.fail();
                    }
                    self.ws();
                    let v = self.value(depth + 1)?;
                    insert(&mut fields, key, v);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(Value::Obj(fields));
                    }
                    return self.fail();
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(Value::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(Value::Arr(items));
                    }
                    return self.fail();
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') if self.eat("true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Value::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.fail(),
        }
    }

    fn digits(&mut self) -> usize {
        let from = self.at;
        while self.s.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        self.at - from
    }

    fn number(&mut self) -> Result<Value, String> {
        let from = self.at;
        self.eat("-");
        match self.s.get(self.at) {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return self.fail(),
        }
        if self.eat(".") && self.digits() == 0 {
            return self.fail();
        }
        if let Some(b'e' | b'E') = self.s.get(self.at) {
            self.at += 1;
            if let Some(b'+' | b'-') = self.s.get(self.at) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return self.fail();
            }
        }
        let text = std::str::from_utf8(&self.s[from..self.at]).unwrap_or("");
        text.parse::<f64>().map(Value::Num).map_err(|e| e.to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let Some(h) = self.s.get(self.at..self.at + 4) else {
            return self.fail();
        };
        let h = std::str::from_utf8(h).map_err(|e| e.to_string())?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return self.fail();
        }
        self.at += 4;
        u32::from_str_radix(h, 16).map_err(|e| e.to_string())
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1; // the opening quote
        let mut out = String::new();
        loop {
            let from = self.at;
            while let Some(&b) = self.s.get(self.at) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.at += 1;
            }
            // The input is a &str: a run of bytes that stops at an ASCII byte is whole characters.
            out.push_str(std::str::from_utf8(&self.s[from..self.at]).map_err(|e| e.to_string())?);
            match self.s.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let c = match self.s.get(self.at) {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.at += 1;
                            let hi = self.hex4()?;
                            // A surrogate pair makes one character; a lone surrogate, which a Rust string cannot
                            // hold, becomes U+FFFD.
                            let c = if (0xD800..0xDC00).contains(&hi) && self.s[self.at..].starts_with(b"\\u") {
                                let back = self.at;
                                self.at += 2;
                                let lo = self.hex4()?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
                                } else {
                                    self.at = back;
                                    None
                                }
                            } else {
                                char::from_u32(hi)
                            };
                            out.push(c.unwrap_or('\u{FFFD}'));
                            continue;
                        }
                        _ => return self.fail(),
                    };
                    self.at += 1;
                    out.push(c);
                }
                _ => return self.fail(),
            }
        }
    }
}

// --- stringify

/// The value as compact JSON.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, None, 0);
    out
}

/// The value as JSON indented by `indent` spaces (compact when 0).
pub fn stringify_pretty(v: &Value, indent: usize) -> String {
    let mut out = String::new();
    write_value(&mut out, v, Some(indent), 0);
    out
}

fn write_value(out: &mut String, v: &Value, indent: Option<usize>, depth: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => out.push_str(&if n.is_finite() { number(*n) } else { "null".into() }),
        Value::Str(s) => quote(out, s),
        Value::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_value(out, item, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push(']');
        }
        Value::Obj(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                quote(out, k);
                out.push_str(if indent.is_some_and(|n| n > 0) { ": " } else { ":" });
                write_value(out, item, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(n) = indent.filter(|n| *n > 0) {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', n.min(10) * depth));
    }
}

/// A string quoted as JSON, control characters escaped.
pub fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The number a field holds: a number, or a string that reads as one (white space around it); NaN for anything else,
/// a missing field included.
pub fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Num(n)) => *n,
        // Not "inf" or "nan", which Rust reads and JS's Number() does not.
        Some(Value::Str(s)) if s.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') => f64::NAN,
        Some(Value::Str(s)) => s.trim().parse().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => *b as u8 as f64,
        _ => f64::NAN,
    }
}

/// A value as a message or a command line writes it: a string as it is, anything else as JSON.
pub fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        v => stringify(v),
    }
}

/// A finite number written as the shortest digits that read back the same, in
/// plain notation from 1e-7 to 1e21, else as d.ddde±x.
pub fn number(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    // Rust's {:e} gives the same shortest digits: d.ddde-x.
    let sci = format!("{:e}", n.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    let e: i64 = exp.parse().unwrap_or(0);
    let n_ = e + 1; // the position of the decimal point after the first digit, as the spec's n
    let mut out = String::new();
    if n < 0.0 {
        out.push('-');
    }
    if k <= n_ && n_ <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n_ - k) as usize));
    } else if 0 < n_ && n_ <= 21 {
        out.push_str(&digits[..n_ as usize]);
        out.push('.');
        out.push_str(&digits[n_ as usize..]);
    } else if -6 < n_ && n_ <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n_) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{}{}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_field_reads_as_a_number_as_js_number_would() {
        let n = |s: &str| num(Some(&Value::Str(s.into())));
        assert_eq!(n(" 12.5 "), 12.5);
        assert_eq!(n("1e3"), 1000.0);
        for s in ["inf", "-Infinity", "nan", "NaN", "12px"] {
            assert!(n(s).is_nan(), "{s}");
        }
    }

    #[test]
    fn numbers_as_js_writes_them() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-12.5, "-12.5"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (0.0000012, "0.0000012"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (1730000000000.0, "1730000000000"),
            (0.30000000000000004, "0.30000000000000004"),
        ];
        for (n, s) in cases {
            assert_eq!(number(n), s, "{n:e}");
        }
    }

    #[test]
    fn parses_and_writes_back() {
        let text = r#"{"a":[1,2.5,-3e2,true,null],"b":{"c":"x\"y\\z\n\u0001é😀"},"a":0}"#;
        let v = parse(text).unwrap();
        // The repeated key keeps its first place and takes the last value.
        assert_eq!(stringify(&v), r#"{"a":0,"b":{"c":"x\"y\\z\n\u0001é😀"}}"#);
        assert_eq!(parse(r#""😀""#).unwrap(), Value::from("😀"));
        assert_eq!(parse(r#""\ud83d""#).unwrap(), Value::from("\u{FFFD}"));
        assert_eq!(stringify_pretty(&parse("[]").unwrap(), 1), "[]");
        assert_eq!(stringify_pretty(&parse(r#"{"a":1}"#).unwrap(), 0), r#"{"a":1}"#);
        // Array indexes first, in numeric order.
        assert_eq!(
            stringify(&parse(r#"{"b":1,"10":2,"2":3,"01":4,"4294967295":5}"#).unwrap()),
            r#"{"2":3,"10":2,"b":1,"01":4,"4294967295":5}"#
        );
        assert_eq!(
            stringify_pretty(&parse(r#"{"a":[1,{}],"b":{}}"#).unwrap(), 1),
            "{\n \"a\": [\n  1,\n  {}\n ],\n \"b\": {}\n}"
        );
    }

    #[test]
    fn refuses_what_json_parse_refuses() {
        for bad in ["", "01", "1.", ".5", "+1", "[1,]", "{\"a\":1,}", "'a'", "\"\t\"", "nul", "[1] x", "{a:1}", "-"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(parse(" \n[ ]\r\t").unwrap(), Value::Arr(vec![]));
    }
}
