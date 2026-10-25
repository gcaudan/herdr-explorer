//! The terminal's input decoded into keys, mouse events, pastes and replies; the one-line text field.

use super::text::{is_control, text_width};
use super::{Seg, seg};

/// The terminal's escape sequences and their key names.
const SEQ: &[(&str, &str)] = &[
    ("\x1b[A", "up"),
    ("\x1b[B", "down"),
    ("\x1b[C", "right"),
    ("\x1b[D", "left"),
    ("\x1bOA", "up"),
    ("\x1bOB", "down"),
    ("\x1bOC", "right"),
    ("\x1bOD", "left"),
    ("\x1b[H", "home"),
    ("\x1b[F", "end"),
    ("\x1bOH", "home"),
    ("\x1bOF", "end"),
    ("\x1b[1~", "home"),
    ("\x1b[4~", "end"),
    ("\x1b[7~", "home"),
    ("\x1b[8~", "end"),
    ("\x1b[2~", "insert"),
    ("\x1b[3~", "delete"),
    ("\x1b[5~", "pageup"),
    ("\x1b[6~", "pagedown"),
    ("\x1b[Z", "shift+tab"),
    ("\x1b[29~", "menu"),
    ("\x1b[21;2~", "shift+f10"),
    ("\x1b[1;5A", "ctrl+up"),
    ("\x1b[1;5B", "ctrl+down"),
    ("\x1b[1;5C", "ctrl+right"),
    ("\x1b[1;5D", "ctrl+left"),
    ("\x1b[1;2A", "shift+up"),
    ("\x1b[1;2B", "shift+down"),
    ("\x1b[1;2C", "shift+right"),
    ("\x1b[1;2D", "shift+left"),
    ("\x1b[1;3A", "alt+up"),
    ("\x1b[1;3B", "alt+down"),
    ("\x1b[1;3C", "alt+right"),
    ("\x1b[1;3D", "alt+left"),
    ("\x1b[1;5H", "ctrl+home"),
    ("\x1b[1;5F", "ctrl+end"),
    ("\x1b[5;5~", "ctrl+pageup"),
    ("\x1b[6;5~", "ctrl+pagedown"),
    // With shift, for a selection in a text being edited; ctrl+shift+z where the terminal tells it from ctrl+z.
    ("\x1b[1;2H", "shift+home"),
    ("\x1b[1;2F", "shift+end"),
    ("\x1b[5;2~", "shift+pageup"),
    ("\x1b[6;2~", "shift+pagedown"),
    ("\x1b[1;6C", "ctrl+shift+right"),
    ("\x1b[1;6D", "ctrl+shift+left"),
    ("\x1b[1;6H", "ctrl+shift+home"),
    ("\x1b[1;6F", "ctrl+shift+end"),
    ("\x1b[3;5~", "ctrl+delete"),
    ("\x1b[122;6u", "ctrl+shift+z"),
    ("\x1b[90;6u", "ctrl+shift+z"),
    ("\x1b[27;6;90~", "ctrl+shift+z"),
    ("\x1b[27;6;122~", "ctrl+shift+z"),
    ("\x1bOP", "f1"),
    ("\x1bOQ", "f2"),
    ("\x1bOR", "f3"),
    ("\x1bOS", "f4"),
    ("\x1b[11~", "f1"),
    ("\x1b[12~", "f2"),
    ("\x1b[15~", "f5"),
    ("\r", "enter"),
    ("\n", "enter"),
    ("\t", "tab"),
    ("\x7f", "backspace"),
    ("\x08", "backspace"),
    ("\x1b", "escape"),
    ("\x1b\x7f", "alt+backspace"),
    ("\x1b\r", "alt+enter"),
    (" ", "space"),
];

/// A token's key name: printable characters are their own name; control characters are ctrl+a … ctrl+z; ESC
/// followed by a character is alt+<character>; None for an unknown sequence.
pub fn key_name(k: &str) -> Option<String> {
    if let Some((_, name)) = SEQ.iter().find(|(s, _)| *s == k) {
        return Some(name.to_string());
    }
    let mut chars = k.chars();
    let (first, second, third) = (chars.next(), chars.next(), chars.next());
    if let (Some(ch), None) = (first, second)
        && ch < ' '
    {
        return Some(format!("ctrl+{}", char::from(ch as u8 + 96)));
    }
    // ESC and one UTF-16 unit.
    if let (Some('\x1b'), Some(ch), None) = (first, second, third)
        && ch.len_utf16() == 1
    {
        return Some(format!("alt+{ch}"));
    }
    if k.starts_with('\x1b') {
        return None;
    }
    Some(k.to_string())
}

/// A mouse report: x and y from 1; wheel: -1 up, 1 down; drag: motion with a button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mouse {
    pub button: i64,
    pub mods: i64,
    pub x: i64,
    pub y: i64,
    pub press: bool,
    pub drag: bool,
    pub wheel: i64,
}

/// What the terminal sent, decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// 'up', 'enter', 'ctrl+p', 'alt+x', or the typed character itself ('a', 'É').
    Key(String),
    Mouse(Mouse),
    /// Focus reports (mode 1004).
    Focus(bool),
    /// Bracketed paste (mode 2004), control characters removed; raw: as pasted, line breaks as \n.
    Paste {
        text: String,
        raw: String,
    },
    /// The terminal's answer to a query: a Kitty graphics reply (\x1b_G…), an OSC answer or a size report.
    Reply(String),
}

/// `+digits`, then ToInt32, as the JS's `b & 64` reads a button number.
fn int32(digits: &str) -> i64 {
    let n = digits.bytes().fold(0.0, |n, d| n * 10.0 + (d - b'0') as f64);
    (n % 4294967296.0) as u32 as i32 as i64
}

/// `+digits`, saturated.
fn number(digits: &str) -> i64 {
    digits.parse().unwrap_or(i64::MAX)
}

/// One token of the input at its start, as the JS's TOKEN regex reads it, and the event it gives.
fn token(s: &str) -> (usize, Option<Event>) {
    let digits = |t: &str| t.bytes().take_while(u8::is_ascii_digit).count();
    if let Some(body) = s.strip_prefix("\x1b[200~")
        && let Some(end) = body.find("\x1b[201~")
    {
        let raw = body[..end].replace("\r\n", "\n").replace('\r', "\n");
        let text = raw.chars().filter(|&ch| !is_control(ch) || ch == '\n').collect();
        return (6 + end + 6, Some(Event::Paste { text, raw }));
    }
    if let Some(body) = s.strip_prefix("\x1b_") {
        let n = body.find('\x1b').unwrap_or(body.len());
        if body[n..].starts_with("\x1b\\") {
            return (2 + n + 2, Some(Event::Reply(s[..2 + n + 2].to_string())));
        }
    }
    if let Some(body) = s.strip_prefix("\x1b]") {
        let n = body.find(['\x07', '\x1b']).unwrap_or(body.len());
        let end = if body[n..].starts_with('\x07') { Some(1) } else { body[n..].starts_with("\x1b\\").then_some(2) };
        if let Some(e) = end {
            return (2 + n + e, Some(Event::Reply(s[..2 + n + e].to_string())));
        }
    }
    if let Some(body) = s.strip_prefix("\x1b[") {
        // \x1b[(\d+(?:;\d+)*)t: a size report.
        let mut i = digits(body);
        if i > 0 {
            while body[i..].starts_with(';') && digits(&body[i + 1..]) > 0 {
                i += 1 + digits(&body[i + 1..]);
            }
            if body[i..].starts_with('t') {
                return (2 + i + 1, Some(Event::Reply(s[..2 + i + 1].to_string())));
            }
        }
        // \x1b[<b;x;y(M|m): a mouse report.
        if let Some(m) = body.strip_prefix('<') {
            let mut fields = [""; 3];
            let mut at = 0;
            let mut ok = true;
            for (k, f) in fields.iter_mut().enumerate() {
                let n = digits(&m[at..]);
                let sep = if k < 2 { m[at + n..].starts_with(';') } else { m[at + n..].starts_with(['M', 'm']) };
                if n == 0 || !sep {
                    ok = false;
                    break;
                }
                *f = &m[at..at + n];
                at += n + 1;
            }
            if ok {
                let b = int32(fields[0]);
                let press = m.as_bytes()[at - 1] == b'M';
                let wheel = if b & 64 != 0 {
                    match b & 3 {
                        0 => -1,
                        1 => 1,
                        _ => 0,
                    }
                } else {
                    0
                };
                let mouse = Mouse {
                    button: b & 3,
                    mods: b & 28,
                    x: number(fields[1]),
                    y: number(fields[2]),
                    press,
                    drag: b & 32 != 0,
                    wheel,
                };
                return (3 + at, Some(Event::Mouse(mouse)));
            }
        }
        // \x1b[I and \x1b[O: focus.
        if body.starts_with(['I', 'O']) {
            return (3, Some(Event::Focus(body.starts_with('I'))));
        }
        // \x1b[[\d;]*[A-Za-z~]
        let n = body.bytes().take_while(|b| b.is_ascii_digit() || *b == b';').count();
        if body.as_bytes().get(n).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'~') {
            return key_token(s, 2 + n + 1);
        }
    }
    if let Some(body) = s.strip_prefix("\x1bO")
        && body.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
    {
        return key_token(s, 3);
    }
    if let Some(body) = s.strip_prefix('\x1b')
        && let Some(ch) = body.chars().next()
        && ch != '\x1b'
    {
        // ESC _ and ESC ] cut from the rest of their answer still read as one.
        if ch == '_' || ch == ']' {
            return (2, Some(Event::Reply(s[..2].to_string())));
        }
        return key_token(s, 1 + ch.len_utf8());
    }
    key_token(s, s.chars().next().map_or(s.len(), char::len_utf8))
}

fn key_token(s: &str, n: usize) -> (usize, Option<Event>) {
    (n, key_name(&s[..n]).map(Event::Key))
}

/// Decodes terminal input into events.
pub fn decode(data: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (n, event) = token(rest);
        out.extend(event);
        rest = &rest[n..];
    }
    out
}

/// Whether a key name types a character.
pub fn printable(name: &str) -> bool {
    let mut chars = name.chars();
    if chars.next().is_some() && chars.next().is_none() && name != " " { true } else { name == "space" }
}

/// A one-line text field: value and cursor (in characters), edited with the usual keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    pub value: String,
    pub cursor: usize,
}

impl Field {
    pub fn new(value: &str) -> Self {
        Field { value: value.to_string(), cursor: value.chars().count() }
    }

    pub fn set(&mut self, value: &str) {
        *self = Field::new(value);
    }

    /// Returns true when the key changed the field or its cursor.
    pub fn key(&mut self, name: &str) -> bool {
        let mut chars: Vec<char> = self.value.chars().collect();
        let at = self.cursor;
        if printable(name) {
            let ch = if name == "space" { ' ' } else { name.chars().next().unwrap_or(' ') };
            chars.insert(at.min(chars.len()), ch);
            self.cursor += 1;
        } else {
            match name {
                "backspace" => {
                    if at == 0 {
                        return true;
                    }
                    if at - 1 < chars.len() {
                        chars.remove(at - 1);
                    }
                    self.cursor -= 1;
                }
                "delete" | "ctrl+d" => {
                    if at < chars.len() {
                        chars.remove(at);
                    }
                }
                "left" | "ctrl+b" => self.cursor = at.saturating_sub(1),
                "right" | "ctrl+f" => self.cursor = chars.len().min(at + 1),
                "home" => self.cursor = 0,
                "end" => self.cursor = chars.len(),
                "ctrl+u" => {
                    chars.drain(..at.min(chars.len()));
                    self.cursor = 0;
                }
                "ctrl+k" => chars.truncate(at),
                "ctrl+w" | "alt+backspace" => {
                    let mut i = at.min(chars.len());
                    while i > 0 && chars[i - 1] == ' ' {
                        i -= 1;
                    }
                    while i > 0 && chars[i - 1] != ' ' {
                        i -= 1;
                    }
                    chars.drain(i..at.min(chars.len()));
                    self.cursor = i;
                }
                _ => return false,
            }
        }
        self.value = chars.into_iter().collect();
        true
    }

    /// A text typed in, its line breaks as spaces.
    pub fn insert(&mut self, text: &str) {
        for ch in text.chars() {
            let ch = if ch == '\n' { ' ' } else { ch };
            if ch == ' ' {
                self.key("space")
            } else {
                self.key(ch.encode_utf8(&mut [0; 4]))
            };
        }
    }

    /// Segments for line(): the visible part of the value in width columns, the cursor shown in reverse video.
    pub fn segments(&self, width: i64, style: &str) -> Vec<Seg> {
        let chars: Vec<char> = self.value.chars().collect();
        let part = |a: usize, b: usize| -> String { chars[a.min(chars.len())..b.min(chars.len())].iter().collect() };
        let mut start = 0;
        while start < self.cursor && (text_width(&part(start, self.cursor + 1)) as i64) > width - 1 {
            start += 1;
        }
        let under = chars.get(self.cursor).map_or(" ".to_string(), |c| c.to_string());
        vec![
            seg(part(start, self.cursor), style),
            seg(under, "\x1b[7m"),
            seg(part(self.cursor + 1, chars.len()), style),
        ]
    }
}
