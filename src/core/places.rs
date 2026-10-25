//! The places of peek's argument: file[:places], places separated by commas, each a line (12), lines (12-14), or
//! columns of a line (12:5-18, counted in characters from 1); 12:5 is line 12, as compilers and grep write it.

use super::paths;

/// A part of a file to show: lines from..to, and columns a..b of a single line (0 when none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub from: u64,
    pub to: u64,
    pub a: u64,
    pub b: u64,
}

/// A number of the argument (`+m[1]`): digits, saturated past u64.
fn num(digits: &str) -> u64 {
    digits.parse().unwrap_or(if digits.is_empty() { 0 } else { u64::MAX })
}

/// /^(\d+)(?:-(\d+))?(?::(\d+)(?:-(\d+))?)?$/: the digits of each group, "" when absent.
fn place(p: &str) -> Option<[&str; 4]> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let mut groups = [""; 4];
    let mut rest = p;
    let n = digits(rest);
    if n == 0 {
        return None;
    }
    (groups[0], rest) = rest.split_at(n);
    if let Some(r) = rest.strip_prefix('-') {
        let n = digits(r);
        if n == 0 {
            return None;
        }
        (groups[1], rest) = r.split_at(n);
    }
    if let Some(r) = rest.strip_prefix(':') {
        let n = digits(r);
        if n == 0 {
            return None;
        }
        (groups[2], rest) = r.split_at(n);
        if let Some(r) = rest.strip_prefix('-') {
            let n = digits(r);
            if n == 0 {
                return None;
            }
            (groups[3], rest) = r.split_at(n);
        }
    }
    rest.is_empty().then_some(groups)
}

/// A name that exists as given is taken whole, so that a colon in it is not read as places, and so is one whose tail
/// does not read as places. Returns the file and its marks; an error for a place that reads as one but cannot be
/// (12-3, 0, 12:9-5).
pub fn target(arg: &str, cwd: &str) -> Result<(String, Vec<Mark>), String> {
    let whole = paths::resolve(&[cwd, arg]);
    let b = arg.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let at = arg[if drive { 2 } else { 0 }..].find(':').map(|i| i + if drive { 2 } else { 0 });
    let Some(at) = at.filter(|&i| i >= 1) else { return Ok((whole, Vec::new())) };
    if std::path::Path::new(&whole).exists() {
        return Ok((whole, Vec::new()));
    }
    let mut marks = Vec::new();
    for p in arg[at + 1..].split(',') {
        let Some(m) = place(p) else { return Ok((whole, Vec::new())) };
        let from = num(m[0]);
        let to = if m[1].is_empty() { from } else { num(m[1]) };
        // Columns only on one line, a range of them only (a single column is a position, not a part).
        let cols = m[1].is_empty() && !m[3].is_empty();
        if from == 0 || to < from || (cols && (num(m[2]) == 0 || num(m[3]) < num(m[2]))) {
            return Err(format!("wrong place: {p}"));
        }
        marks.push(Mark { from, to, a: if cols { num(m[2]) } else { 0 }, b: if cols { num(m[3]) } else { 0 } });
    }
    Ok((paths::resolve(&[cwd, &arg[..at]]), marks))
}

/// A link clicked in a terminal (HERDR_PLUGIN_CLICKED_URL), as peek's argument: a file:// URL (an OSC 8 hyperlink),
/// its line from a #L12, #L12-L14 or #12-14 fragment, or the text clicked, path[:places], as it is. A file:// URL on
/// another host is refused (on Windows it would reach a network share); so is any other scheme. Returns the argument,
/// or None.
pub fn clicked(url: &str, hostname: &str) -> Option<String> {
    let url = url.trim_matches(super::js::is_space);
    if !has_scheme(url) {
        return (!url.is_empty()).then(|| url.to_string());
    }
    let u = FileUrl::parse(url)?;
    if !u.host.is_empty() && u.host != "localhost" && u.host.to_lowercase() != hostname.to_lowercase() {
        return None;
    }
    let file = u.to_path()?;
    match fragment_lines(&u.hash) {
        Some((a, Some(b))) => Some(format!("{file}:{a}-{b}")),
        Some((a, None)) => Some(format!("{file}:{a}")),
        None => Some(file),
    }
}

/// The machine's name, for `clicked`.
pub fn hostname() -> String {
    crate::sys::hostname()
}

/// /^[a-z][a-z0-9+.-]*:\/\//i
fn has_scheme(url: &str) -> bool {
    let b = url.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return false;
    }
    let n = b.iter().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-')).count();
    url[n..].starts_with("://")
}

/// /^#L?(\d+)(?:-L?(\d+))?$/i
fn fragment_lines(hash: &str) -> Option<(&str, Option<&str>)> {
    let rest = hash.strip_prefix('#')?;
    let rest = rest.strip_prefix(['L', 'l']).unwrap_or(rest);
    let n = rest.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    let (a, rest) = rest.split_at(n);
    if rest.is_empty() {
        return Some((a, None));
    }
    let rest = rest.strip_prefix('-')?;
    let rest = rest.strip_prefix(['L', 'l']).unwrap_or(rest);
    let n = rest.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0 && n == rest.len()).then_some((a, Some(rest)))
}

/// A file URL as the WHATWG parser reads it (`new URL`), as far as `clicked` needs: the scheme, the host (lowercased,
/// "localhost" kept for the caller), the path's segments with `.` and `..` resolved (percent escapes left as typed),
/// and the fragment.
struct FileUrl {
    host: String,
    path: Vec<String>,
    hash: String,
}

/// A segment that is a Windows drive letter: C: or C|.
fn drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && (b[1] == b':' || b[1] == b'|')
}

impl FileUrl {
    fn parse(url: &str) -> Option<FileUrl> {
        // Leading and trailing C0 controls and spaces go, and tabs and line breaks anywhere.
        let url: String =
            url.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
        let colon = url.find(':')?;
        if !url[..colon].eq_ignore_ascii_case("file") {
            return None; // another scheme: refused (new URL may accept it, the caller refuses it then)
        }
        let rest = &url[colon + 1..];
        let (rest, hash) = match rest.find('#') {
            Some(i) => (&rest[..i], rest[i..].to_string()),
            None => (rest, String::new()),
        };
        let rest = rest.find('?').map_or(rest, |i| &rest[..i]);
        let is_sep = |c: char| c == '/' || c == '\\';
        let rest = rest.strip_prefix(is_sep).and_then(|r| r.strip_prefix(is_sep))?;
        let host_end = rest.find(is_sep).unwrap_or(rest.len());
        let (mut host, mut path) = (rest[..host_end].to_string(), &rest[host_end..]);
        let mut first = String::new();
        if drive_letter(&host) {
            // file://C:/x is the path /C:/x.
            first = std::mem::take(&mut host);
        } else if host.bytes().any(|b| b" #%/:<>?@[\\]^|".contains(&b)) {
            if !host.contains('%') {
                return None;
            }
            host = String::from_utf8(decode(&host)?).ok()?;
            if host.bytes().any(|b| b" #%/:<>?@[\\]^|".contains(&b) || b < 0x20) {
                return None;
            }
        }
        host = host.to_ascii_lowercase();
        let mut segments: Vec<String> = Vec::new();
        let mut parts: Vec<&str> = Vec::new();
        if !first.is_empty() {
            parts.push(&first);
        }
        if !path.is_empty() {
            path = &path[1..];
            parts.extend(path.split(is_sep));
        } else if parts.is_empty() {
            parts.push("");
        }
        let n = parts.len();
        for (i, part) in parts.into_iter().enumerate() {
            let last = i + 1 == n;
            let lower = part.to_ascii_lowercase();
            if matches!(lower.as_str(), ".." | ".%2e" | "%2e." | "%2e%2e") {
                // A drive letter alone stays.
                if !(segments.len() == 1 && drive_letter(&segments[0])) {
                    segments.pop();
                }
                if last {
                    segments.push(String::new());
                }
            } else if matches!(lower.as_str(), "." | "%2e") {
                if last {
                    segments.push(String::new());
                }
            } else if segments.is_empty() && drive_letter(part) {
                segments.push(format!("{}:", &part[..1]));
            } else {
                segments.push(part.to_string());
            }
        }
        Some(FileUrl { host, path: segments, hash: if hash == "#" { String::new() } else { hash } })
    }

    /// `url.fileURLToPath` once the host is cleared: the path decoded; None for an encoded separator, bad escapes or
    /// (on Windows) a path without a drive.
    fn to_path(&self) -> Option<String> {
        let pathname = format!("/{}", self.path.join("/"));
        let lower = pathname.to_ascii_lowercase();
        if lower.contains("%2f") || (cfg!(windows) && lower.contains("%5c")) {
            return None;
        }
        if cfg!(windows) {
            let p = String::from_utf8(decode(&pathname.replace('/', "\\"))?).ok()?;
            let b = p.as_bytes();
            if b.len() < 3 || !(b[1] | 0x20).is_ascii_lowercase() || b[2] != b':' {
                return None;
            }
            return Some(p[1..].to_string());
        }
        String::from_utf8(decode(&pathname)?).ok()
    }
}

/// decodeURIComponent's escapes: None for a % without two hex digits.
fn decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn reads_file_urls() {
        let c = |u: &str| clicked(u, "me");
        assert_eq!(c("file:///a/b/../c/./d.js#L3").as_deref(), Some("/a/c/d.js:3"));
        assert_eq!(c("file:///a/%2e%2E/b").as_deref(), Some("/b"));
        assert_eq!(c("file:///a%2Fb").as_deref(), None);
        assert_eq!(c("file:///a%zz").as_deref(), None);
        assert_eq!(c("file:///a\tb\n.js").as_deref(), Some("/ab.js"));
        assert_eq!(c("file://ME/x").as_deref(), Some("/x"));
        assert_eq!(c("file://other/x").as_deref(), None);
        assert_eq!(c("file://a@b/x").as_deref(), None);
        assert_eq!(c("file:///x#L1-L2").as_deref(), Some("/x:1-2"));
        assert_eq!(c("file:///x#1-").as_deref(), Some("/x"));
    }

    #[cfg(windows)]
    #[test]
    fn reads_windows_file_urls() {
        let c = |u: &str| clicked(u, "me");
        assert_eq!(c("file:///C:/a/../b/d.js#L3").as_deref(), Some("C:\\b\\d.js:3"));
        assert_eq!(c("file:///a/b").as_deref(), None);
        assert_eq!(c("file:///C:/a%5Cb").as_deref(), None);
    }
}
