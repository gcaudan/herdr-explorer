//! The output of git diff, show or stash show, read into files, hunks and lines.

use crate::core::js;

/// A line of a hunk: its sign (' ', '+', '-', or '\\' for "\ No newline at end of file", whose text is then trimmed
/// and has no numbers), its text, its number in the old and the new version (None on the side it is not on), its
/// place (its number in the new version, or for a removed line that of the line now in its place), and whether it
/// ended with a carriage return, left out of text (core::stage puts it back in the patches it makes).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub sign: char,
    pub text: String,
    pub old: Option<i64>,
    pub new: Option<i64>,
    pub at: i64,
    pub cr: bool,
}

/// A hunk: its @@ line, the context git gives after it, its lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    pub head: String,
    pub context: String,
    pub lines: Vec<Line>,
}

/// A file of the diff: its paths (None on the side it does not exist, or when not known), its status (modified, added,
/// deleted, renamed, copied), notes (binary file, mode change), hunks, lines added and removed, the mode of an added
/// or deleted file, the old and new modes of a mode change; sec, the section of the Changes pane it is in, set by the
/// caller ("" for none).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct File {
    pub from: Option<String>,
    pub to: Option<String>,
    pub status: String,
    pub notes: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub add: usize,
    pub del: usize,
    pub mode: Option<String>,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub sec: String,
}

/// A diff: the lines before the first file (commit header and stat), the files, lines added and removed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Diff {
    pub pre: Vec<String>,
    pub files: Vec<File>,
    pub add: usize,
    pub del: usize,
}

/// A path as git quotes it when it contains special characters: "a/tab\there" with octal escapes for bytes. Each
/// UTF-16 unit of a character past the BMP becomes U+FFFD, as Buffer.from(one unit) makes it.
pub fn unquote(p: &str) -> String {
    let units: Vec<u16> = p.encode_utf16().collect();
    if !p.starts_with('"') || !p.ends_with('"') || units.len() < 2 {
        return p.to_string();
    }
    let mut bytes = Vec::new();
    let unit_bytes = |u: u16, bytes: &mut Vec<u8>| match char::from_u32(u as u32) {
        Some(c) => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        None => bytes.extend_from_slice("\u{FFFD}".as_bytes()),
    };
    let end = units.len() - 1;
    let mut i = 1;
    while i < end {
        if units[i] != b'\\' as u16 {
            unit_bytes(units[i], &mut bytes);
            i += 1;
            continue;
        }
        i += 1;
        let n = units[i];
        if (b'0' as u16..=b'7' as u16).contains(&n) {
            // parseInt(p.substr(i, 3), 8): the octal digits among the next three units, a byte of the value.
            let value = units[i..(i + 3).min(units.len())]
                .iter()
                .take_while(|&&u| (b'0' as u16..=b'7' as u16).contains(&u))
                .fold(0u32, |v, &u| v * 8 + (u - b'0' as u16) as u32);
            bytes.push(value as u8);
            i += 3;
        } else {
            let esc = match n {
                0x61 => 7,
                0x62 => 8,
                0x74 => 9,
                0x6E => 10,
                0x76 => 11,
                0x66 => 12,
                0x72 => 13,
                _ => n as u32,
            };
            bytes.push(esc as u8);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A path of a --- or +++ line: unquoted, without its a/ or b/; None for /dev/null.
pub fn strip(p: &str) -> Option<String> {
    let p = unquote(p.strip_suffix('\t').unwrap_or(p));
    if p == "/dev/null" {
        return None;
    }
    Some(p.strip_prefix("a/").or_else(|| p.strip_prefix("b/")).unwrap_or(&p).to_string())
}

/// A hunk's @@ line: where it starts on each side, its count of lines there (None when not given), and what follows
/// the second @@.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Head<'a> {
    pub old: i64,
    pub old_count: Option<i64>,
    pub new: i64,
    pub new_count: Option<i64>,
    pub rest: &'a str,
}

/// A hunk's @@ line read as /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/ (no line terminator after the second
/// @@). Numbers past i64 saturate.
pub fn head(l: &str) -> Option<Head<'_>> {
    fn number(s: &str) -> Option<(i64, &str)> {
        let n = s.bytes().take_while(u8::is_ascii_digit).count();
        (n > 0).then(|| (s[..n].parse().unwrap_or(i64::MAX), &s[n..]))
    }
    fn range(s: &str) -> Option<(i64, Option<i64>, &str)> {
        let (start, rest) = number(s)?;
        match rest.strip_prefix(',') {
            Some(r) => {
                let (count, rest) = number(r)?;
                Some((start, Some(count), rest))
            }
            None => Some((start, None, rest)),
        }
    }
    let (o, oc, rest) = range(l.strip_prefix("@@ -")?)?;
    let (n, nc, rest) = range(rest.strip_prefix(" +")?)?;
    let rest = rest.strip_prefix(" @@")?;
    (!rest.chars().any(js::is_line_terminator)).then_some(Head { old: o, old_count: oc, new: n, new_count: nc, rest })
}

/// Parses the output of git diff, show or stash show.
pub fn parse(text: &str) -> Diff {
    let mut d = Diff::default();
    // The file and hunk being read (indices in d.files and its hunks), the lines left on each side.
    let mut in_hunk = false;
    let (mut old_left, mut new_left, mut o, mut n) = (0i64, 0i64, 0i64, 0i64);
    for raw in text.split('\n') {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        let sign = l.chars().next().unwrap_or('\0');
        if in_hunk && (old_left > 0 || new_left > 0 || sign == '\\') && matches!(sign, ' ' | '+' | '-' | '\\') {
            let f = d.files.last_mut().unwrap();
            let h = f.hunks.last_mut().unwrap();
            if sign == '\\' {
                let text = l[1..].trim_matches(js::is_space).to_string();
                h.lines.push(Line { sign, text, old: None, new: None, at: 0, cr: false });
                continue;
            }
            h.lines.push(Line {
                sign,
                text: l[1..].to_string(),
                old: (sign != '+').then_some(o),
                new: (sign != '-').then_some(n),
                at: n,
                cr: l.len() != raw.len(),
            });
            if sign != '+' {
                o += 1;
                old_left -= 1;
            }
            if sign != '-' {
                n += 1;
                new_left -= 1;
            }
            if sign == '+' {
                f.add += 1;
                d.add += 1;
            } else if sign == '-' {
                f.del += 1;
                d.del += 1;
            }
            continue;
        }
        in_hunk = false;
        if l.starts_with("diff ") {
            d.files.push(File { from: same_path(l), to: same_path(l), status: "modified".into(), ..File::default() });
            continue;
        }
        let Some(f) = d.files.last_mut() else {
            d.pre.push(l.to_string());
            continue;
        };
        if let Some(m) = head(l) {
            o = m.old;
            n = m.new;
            old_left = m.old_count.unwrap_or(1);
            new_left = m.new_count.unwrap_or(1);
            let context = m.rest.strip_prefix(' ').unwrap_or(m.rest).to_string();
            f.hunks.push(Hunk { head: l.to_string(), context, lines: Vec::new() });
            in_hunk = true;
        } else if let Some(p) = l.strip_prefix("--- ") {
            f.from = strip(p);
        } else if let Some(p) = l.strip_prefix("+++ ") {
            f.to = strip(p);
        } else if let Some(m) = l.strip_prefix("new file mode ") {
            f.status = "added".into();
            f.mode = Some(m.to_string());
        } else if let Some(m) = l.strip_prefix("deleted file mode ") {
            f.status = "deleted".into();
            f.mode = Some(m.to_string());
        } else if let Some(p) = l.strip_prefix("rename from ") {
            f.from = Some(unquote(p));
            f.status = "renamed".into();
        } else if let Some(p) = l.strip_prefix("rename to ") {
            f.to = Some(unquote(p));
        } else if let Some(p) = l.strip_prefix("copy from ") {
            f.from = Some(unquote(p));
            f.status = "copied".into();
        } else if let Some(p) = l.strip_prefix("copy to ") {
            f.to = Some(unquote(p));
        } else if l.starts_with("Binary files") {
            f.notes.push("Binary file".into());
        } else if let Some(m) = l.strip_prefix("old mode ") {
            f.old_mode = Some(m.to_string());
            f.notes.push(format!("mode {m}"));
        } else if let Some(m) = l.strip_prefix("new mode ") {
            f.new_mode = Some(m.to_string());
            // Without a note before it, the JS sets notes[-1], which no one reads.
            if let Some(last) = f.notes.last_mut() {
                last.push_str(&format!(" → {m}"));
            }
        } else if !l.trim_matches(js::is_space).is_empty()
            && !["index ", "similarity ", "dissimilarity ", "new file", "deleted file"].iter().any(|p| l.starts_with(p))
        {
            d.pre.push(l.to_string());
        }
    }
    for file in &mut d.files {
        if file.status == "added" {
            file.from = None;
        }
        if file.status == "deleted" {
            file.to = None;
        }
    }
    d
}

/// The path of a diff --git a/x b/x line, when the same on both sides (otherwise it comes from the --- +++ or rename
/// lines), measured in UTF-16 units as the JS does.
fn same_path(l: &str) -> Option<String> {
    // l.replace(/^diff --\S+ /, '')
    let rest = l
        .strip_prefix("diff --")
        .and_then(|r| {
            let n = r.find(js::is_space)?;
            (n > 0 && r[n..].starts_with(' ')).then(|| &r[n + 1..])
        })
        .unwrap_or(l);
    let units: Vec<u16> = rest.encode_utf16().collect();
    if !rest.starts_with("a/") {
        return None;
    }
    // half = (length - 1) / 2, and slice() truncates it.
    let len = units.len() as f64;
    let half = (len - 1.0) / 2.0;
    let slice = |from: f64, to: f64| {
        let clamp = |x: f64| (x.trunc().max(0.0) as usize).min(units.len());
        let (a, b) = (clamp(from), clamp(to));
        if a < b { &units[a..b] } else { &units[0..0] }
    };
    let first = slice(2.0, half);
    (first == slice(half + 3.0, len)).then(|| String::from_utf16_lossy(first))
}
