//! Messages of the footer, their kind and time shown, empty areas, and wrapping.

use super::hints::keys;
use super::text::{ellipsis, fit, line, seg_width, text_width};
use super::theme::c;
use super::{Seg, fg, seg};

/// An empty area of h lines, width w: what it lacks, dimmed and centred a third of the way down, and under it the
/// keys that go on from there.
pub fn empty<K: AsRef<str>, A: AsRef<str>>(w: usize, h: usize, text: &str, pairs: &[(K, A)]) -> Vec<String> {
    let mut body: Vec<Vec<Seg>> = vec![vec![seg(ellipsis(text, w as i64 - 2), fg(c().dim))]];
    if !pairs.is_empty() {
        body.push(Vec::new());
        body.push(keys(pairs, usize::MAX).segs);
    }
    let n = body.len() as i64;
    let at = (h as i64 - n).min((h as i64 - n).div_euclid(3)).max(0);
    (0..h as i64)
        .map(|i| match usize::try_from(i - at).ok().and_then(|k| body.get(k)) {
            Some(segs) => {
                let pad = (w as i64 - seg_width(segs) as i64).div_euclid(2).max(0) as usize;
                let mut all = vec![seg(" ".repeat(pad), "")];
                all.extend(segs.iter().cloned());
                line(&all, w, None)
            }
            None => line(&[], w, None),
        })
        .collect()
}

/// What a message reads as: ✓ done, ✕ error, ⋯ going on, · nothing to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Ok,
    Error,
    Busy,
    Info,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Ok => "ok",
            Kind::Error => "error",
            Kind::Busy => "busy",
            Kind::Info => "info",
        }
    }
}

/// A word of the message, with \b on both sides.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(crate::core::regex::is_word) && !after.is_some_and(crate::core::regex::is_word)
    })
}

/// The kind of a message, guessed from its text.
pub fn message_kind(text: &str) -> Kind {
    const STARTS: [&str; 11] = [
        "Cannot",
        "Can't",
        "Could not",
        "Not ",
        "No editor",
        "No terminal",
        "Action failed",
        "Update failed",
        "Already exists",
        "VS Code not found",
        "Unknown ",
    ];
    let error = STARTS.iter().any(|s| text.starts_with(s))
        || has_word(text, "not found")
        || has_word(text, "unavailable")
        || text.ends_with(": already bound")
        || text.contains("; cannot ");
    if text.ends_with('…') {
        Kind::Busy
    } else if error {
        Kind::Error
    } else if text.starts_with("No ") || text.starts_with("Nothing") {
        Kind::Info
    } else {
        Kind::Ok
    }
}

/// How long a message shows, in milliseconds: an error twice as long.
pub fn message_time(kind: Kind) -> u64 {
    if kind == Kind::Error { 4000 } else { 2000 }
}

/// A message after an action, with its mark. kind: None to guess it from the text.
pub fn message(text: &str, kind: Option<Kind>) -> Vec<Seg> {
    match kind.unwrap_or_else(|| message_kind(text)) {
        Kind::Error => vec![seg(" ✕ ", fg(c().red)), seg(text, fg(c().red))],
        Kind::Busy => vec![seg(format!(" ⋯ {text}"), fg(c().dim))],
        Kind::Info => vec![seg(" · ", fg(c().dim)), seg(text, "")],
        Kind::Ok => vec![seg(" ✓ ", fg(c().green)), seg(text, "")],
    }
}

/// Splits a plain text into lines of at most width columns, at spaces when possible.
pub fn wrap(text: &str, width: i64) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        let mut w = 0;
        // The words, each with the white space that ends it.
        let mut words = Vec::new();
        let mut start = 0;
        for (i, ch) in para.char_indices() {
            if crate::unicode::is_space(ch) {
                words.push(&para[start..i + ch.len_utf8()]);
                start = i + ch.len_utf8();
            }
        }
        words.push(&para[start..]);
        for word in words {
            let ww = text_width(word);
            if w + ww <= width {
                cur += word;
                w += ww;
                continue;
            }
            if !cur.is_empty() {
                lines.push(cur.trim_end_matches(crate::unicode::is_space).to_string());
                cur.clear();
                w = 0;
            }
            // White space at a break goes with it: the next line never starts with it.
            if word.chars().all(crate::unicode::is_space) {
                continue;
            }
            let mut rest = word;
            // The white space ending the word never breaks it: dropped with the line's end.
            while text_width(rest.trim_end_matches(crate::unicode::is_space)) > width {
                let (mut part, _) = fit(rest, width, None);
                // A character wider than the width never fits: it goes alone on its line.
                if part.is_empty() {
                    part = &rest[..rest.chars().next().map_or(0, char::len_utf8)];
                }
                lines.push(part.to_string());
                rest = &rest[part.len()..];
            }
            cur = rest.to_string();
            w = text_width(rest);
        }
        lines.push(cur.trim_end_matches(crate::unicode::is_space).to_string());
    }
    lines
}
