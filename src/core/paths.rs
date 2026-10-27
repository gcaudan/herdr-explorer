//! Paths handled as strings: `posix` and `win32` are both here (tested on every
//! system), the functions at the top are the running system's. Indices are bytes: the separators, dots and drive
//! letters these functions look at are ASCII, so every slice falls on a character boundary.

#[cfg(not(windows))]
pub use posix::{basename, dirname, extname, is_absolute, join, normalize, relative, resolve, root};
#[cfg(windows)]
pub use win32::{basename, dirname, extname, is_absolute, join, normalize, relative, resolve, root};

#[cfg(not(windows))]
pub const SEP: char = '/';
#[cfg(windows)]
pub const SEP: char = '\\';
#[cfg(not(windows))]
pub const DELIMITER: char = ':';
#[cfg(windows)]
pub const DELIMITER: char = ';';

/// The process's working directory.
pub fn cwd() -> String {
    std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".into())
}

/// The path with `..` and `.` resolved and separators merged: `sep` joins the parts kept.
fn normalize_string(path: &str, above_root: bool, sep: &str, is_sep: fn(u8) -> bool) -> String {
    let b = path.as_bytes();
    let mut res = String::new();
    let mut last_segment = 0usize;
    let mut last_slash: isize = -1;
    let mut dots: i32 = 0;
    let mut code = 0u8;
    for i in 0..=b.len() {
        if i < b.len() {
            code = b[i];
        } else if is_sep(code) {
            break;
        } else {
            code = b'/';
        }
        if is_sep(code) {
            if last_slash == i as isize - 1 || dots == 1 {
                // an empty part or '.'
            } else if dots == 2 {
                if !(res.len() >= 2 && last_segment == 2 && res.ends_with("..")) {
                    if res.len() > 2 {
                        match res.rfind(sep) {
                            None => {
                                res.clear();
                                last_segment = 0;
                            }
                            Some(at) => {
                                res.truncate(at);
                                last_segment = res.len() - res.rfind(sep).map_or(0, |x| x + 1);
                            }
                        }
                        last_slash = i as isize;
                        dots = 0;
                        continue;
                    } else if !res.is_empty() {
                        res.clear();
                        last_segment = 0;
                        last_slash = i as isize;
                        dots = 0;
                        continue;
                    }
                }
                if above_root {
                    if !res.is_empty() {
                        res.push_str(sep);
                    }
                    res.push_str("..");
                    last_segment = 2;
                }
            } else {
                if !res.is_empty() {
                    res.push_str(sep);
                }
                res.push_str(&path[(last_slash + 1) as usize..i]);
                last_segment = i - (last_slash + 1) as usize;
            }
            last_slash = i as isize;
            dots = 0;
        } else if code == b'.' && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
    }
    res
}

/// The extension of a name's last part, from its last dot; none for a name starting with its only dot (`.bashrc`),
/// `.` or `..`. `start`: where the name may begin (past a drive).
fn extname_from(path: &str, start: usize, is_sep: fn(u8) -> bool) -> String {
    let b = path.as_bytes();
    let (mut start_dot, mut start_part, mut end): (isize, usize, isize) = (-1, start, -1);
    let mut matched_slash = true;
    let mut pre_dot_state = 0;
    let mut i = b.len() as isize - 1;
    while i >= start as isize {
        let code = b[i as usize];
        if is_sep(code) {
            if !matched_slash {
                start_part = i as usize + 1;
                break;
            }
            i -= 1;
            continue;
        }
        if end == -1 {
            matched_slash = false;
            end = i + 1;
        }
        if code == b'.' {
            if start_dot == -1 {
                start_dot = i;
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot != -1 {
            pre_dot_state = -1;
        }
        i -= 1;
    }
    if start_dot == -1
        || end == -1
        || pre_dot_state == 0
        || (pre_dot_state == 1 && start_dot == end - 1 && start_dot == start_part as isize + 1)
    {
        return String::new();
    }
    path[start_dot as usize..end as usize].to_string()
}

/// The last part of a path, trailing separators left out; `start`: where it may begin (past a drive).
fn basename_from(path: &str, start: usize, is_sep: fn(u8) -> bool) -> String {
    let b = path.as_bytes();
    let (mut from, mut end): (usize, isize) = (start, -1);
    let mut matched_slash = true;
    let mut i = b.len() as isize - 1;
    while i >= start as isize {
        if is_sep(b[i as usize]) {
            if !matched_slash {
                from = i as usize + 1;
                break;
            }
        } else if end == -1 {
            matched_slash = false;
            end = i + 1;
        }
        i -= 1;
    }
    if end == -1 { String::new() } else { path[from..end as usize].to_string() }
}

/// The basename with a suffix: taken off when the name ends with it and is not only it; nothing left when the
/// whole path is the suffix.
fn strip_suffix(path: &str, name: String, suffix: Option<&str>) -> String {
    match suffix {
        Some(s) if !s.is_empty() && s == path => String::new(),
        Some(s) if !s.is_empty() && name.len() > s.len() && name.ends_with(s) => {
            name[..name.len() - s.len()].to_string()
        }
        _ => name,
    }
}

pub mod posix {
    use super::{basename_from, extname_from, normalize_string, strip_suffix};

    fn is_sep(c: u8) -> bool {
        c == b'/'
    }

    pub fn is_absolute(path: &str) -> bool {
        path.starts_with('/')
    }

    /// The parts resolved to an absolute POSIX path: from the last absolute part (the working directory before them all).
    pub fn resolve(parts: &[&str]) -> String {
        let mut resolved = String::new();
        let mut absolute = false;
        let cwd = super::cwd();
        let mut i = parts.len() as isize - 1;
        while i >= -1 && !absolute {
            let p = if i >= 0 { parts[i as usize] } else { &cwd };
            i -= 1;
            if p.is_empty() {
                continue;
            }
            resolved = format!("{p}/{resolved}");
            absolute = p.starts_with('/');
        }
        let tail = normalize_string(&resolved, !absolute, "/", is_sep);
        if absolute {
            format!("/{tail}")
        } else if tail.is_empty() {
            ".".into()
        } else {
            tail
        }
    }

    pub fn normalize(path: &str) -> String {
        if path.is_empty() {
            return ".".into();
        }
        let absolute = path.starts_with('/');
        let trailing = path.ends_with('/');
        let mut p = normalize_string(path, !absolute, "/", is_sep);
        if p.is_empty() {
            if absolute {
                return "/".into();
            }
            return if trailing { "./".into() } else { ".".into() };
        }
        if trailing {
            p.push('/');
        }
        if absolute { format!("/{p}") } else { p }
    }

    pub fn join(parts: &[&str]) -> String {
        let kept: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
        if kept.is_empty() {
            return ".".into();
        }
        normalize(&kept.join("/"))
    }

    pub fn relative(from: &str, to: &str) -> String {
        if from == to {
            return String::new();
        }
        let (from, to) = (resolve(&[from]), resolve(&[to]));
        if from == to {
            return String::new();
        }
        let (f, t) = (from.as_bytes(), to.as_bytes());
        let (from_start, from_end) = (1usize, f.len());
        let from_len = from_end - from_start;
        let to_start = 1usize;
        let to_len = t.len() - to_start;
        let length = from_len.min(to_len);
        let mut last_common: isize = -1;
        let mut i = 0;
        while i < length {
            let c = f[from_start + i];
            if c != t[to_start + i] {
                break;
            } else if c == b'/' {
                last_common = i as isize;
            }
            i += 1;
        }
        if i == length {
            if to_len > length {
                if t[to_start + i] == b'/' {
                    return to[to_start + i + 1..].to_string();
                }
                if i == 0 {
                    return to[to_start + i..].to_string();
                }
            } else if from_len > length {
                if f[from_start + i] == b'/' {
                    last_common = i as isize;
                } else if i == 0 {
                    last_common = 0;
                }
            }
        }
        let mut out = String::new();
        #[allow(clippy::needless_range_loop)] // k reaches from_end, past the last index
        for k in (from_start as isize + last_common + 1) as usize..=from_end {
            if k == from_end || f[k] == b'/' {
                out.push_str(if out.is_empty() { ".." } else { "/.." });
            }
        }
        format!("{out}{}", &to[(to_start as isize + last_common) as usize..])
    }

    pub fn dirname(path: &str) -> String {
        let b = path.as_bytes();
        if b.is_empty() {
            return ".".into();
        }
        let has_root = b[0] == b'/';
        let mut end: isize = -1;
        let mut matched_slash = true;
        for i in (1..b.len()).rev() {
            if b[i] == b'/' {
                if !matched_slash {
                    end = i as isize;
                    break;
                }
            } else {
                matched_slash = false;
            }
        }
        if end == -1 {
            return if has_root { "/".into() } else { ".".into() };
        }
        if has_root && end == 1 {
            return "//".into();
        }
        path[..end as usize].to_string()
    }

    pub fn basename(path: &str, suffix: Option<&str>) -> String {
        strip_suffix(path, basename_from(path, 0, is_sep), suffix)
    }

    pub fn extname(path: &str) -> String {
        extname_from(path, 0, is_sep)
    }

    /// The root of a POSIX path: / or nothing.
    pub fn root(path: &str) -> String {
        if path.starts_with('/') { "/".into() } else { String::new() }
    }
}

pub mod win32 {
    use super::{basename_from, extname_from, normalize_string, strip_suffix};

    fn is_sep(c: u8) -> bool {
        c == b'/' || c == b'\\'
    }

    fn is_drive(b: &[u8]) -> bool {
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    }

    pub fn is_absolute(path: &str) -> bool {
        let b = path.as_bytes();
        !b.is_empty() && (is_sep(b[0]) || (b.len() > 2 && is_drive(b) && is_sep(b[2])))
    }

    /// The root of a path that starts with two separators: (device, root end), as resolve reads
    /// `\\server\share`, and `\\?\…`, `\\.\…` when `devices` (parse takes those as a server and a share too); None
    /// when it is not one.
    fn unc(path: &str, devices: bool) -> Option<(String, usize)> {
        let b = path.as_bytes();
        let len = b.len();
        let mut j = 2;
        let mut last = j;
        while j < len && !is_sep(b[j]) {
            j += 1;
        }
        if j < len && j != last {
            let first = &path[last..j];
            last = j;
            while j < len && is_sep(b[j]) {
                j += 1;
            }
            if j < len && j != last {
                last = j;
                while j < len && !is_sep(b[j]) {
                    j += 1;
                }
                if j == len || j != last {
                    if !devices || (first != "." && first != "?") {
                        return Some((format!("\\\\{first}\\{}", &path[last..j]), j));
                    }
                    return Some((format!("\\\\{first}"), 4));
                }
            }
        }
        None
    }

    /// The parts resolved to an absolute Windows path. The working directory of another drive comes from its `=C:`
    /// variable.
    pub fn resolve(parts: &[&str]) -> String {
        let mut device = String::new();
        let mut tail = String::new();
        let mut absolute = false;
        let mut i = parts.len() as isize - 1;
        while i >= -1 {
            let owned;
            let p: &str = if i >= 0 {
                let p = parts[i as usize];
                if p.is_empty() {
                    i -= 1;
                    continue;
                }
                p
            } else if device.is_empty() {
                owned = super::cwd();
                &owned
            } else {
                let mut p = std::env::var(format!("={device}")).unwrap_or_else(|_| super::cwd());
                if p.get(..2).map(|s| s.to_lowercase()) != Some(device.to_lowercase())
                    && p.as_bytes().get(2) == Some(&b'\\')
                {
                    p = format!("{device}\\");
                }
                owned = p;
                &owned
            };
            i -= 1;
            let b = p.as_bytes();
            let len = b.len();
            let (mut root_end, mut dev, mut abs) = (0usize, String::new(), false);
            if len == 1 {
                if is_sep(b[0]) {
                    root_end = 1;
                    abs = true;
                }
            } else if is_sep(b[0]) {
                abs = true;
                if is_sep(b[1]) {
                    if let Some((d, end)) = unc(p, true) {
                        dev = d;
                        root_end = end;
                    }
                } else {
                    root_end = 1;
                }
            } else if is_drive(b) {
                dev = p[..2].to_string();
                root_end = 2;
                if len > 2 && is_sep(b[2]) {
                    abs = true;
                    root_end = 3;
                }
            }
            if !dev.is_empty() {
                if !device.is_empty() {
                    if dev.to_lowercase() != device.to_lowercase() {
                        continue;
                    }
                } else {
                    device = dev;
                }
            }
            if absolute {
                if !device.is_empty() {
                    break;
                }
            } else {
                tail = format!("{}\\{tail}", &p[root_end..]);
                absolute = abs;
                if abs && !device.is_empty() {
                    break;
                }
            }
        }
        let tail = normalize_string(&tail, !absolute, "\\", is_sep);
        if absolute {
            format!("{device}\\{tail}")
        } else {
            let s = format!("{device}{tail}");
            if s.is_empty() { ".".into() } else { s }
        }
    }

    const RESERVED: [&str; 28] = [
        "CON",
        "PRN",
        "AUX",
        "NUL",
        "COM1",
        "COM2",
        "COM3",
        "COM4",
        "COM5",
        "COM6",
        "COM7",
        "COM8",
        "COM9",
        "LPT1",
        "LPT2",
        "LPT3",
        "LPT4",
        "LPT5",
        "LPT6",
        "LPT7",
        "LPT8",
        "LPT9",
        "COM\u{b9}",
        "COM\u{b2}",
        "COM\u{b3}",
        "LPT\u{b9}",
        "LPT\u{b2}",
        "LPT\u{b3}",
    ];

    /// Whether the start of a path up to its colon (at `colon`, a byte offset) is a reserved device name (CON,
    /// COM1…), uppercased. No colon (None) takes all but the last character.
    fn reserved(path: &str, colon: Option<usize>) -> bool {
        let part = match colon {
            Some(c) => &path[..c],
            None => match path.chars().next_back() {
                Some(ch) => &path[..path.len() - ch.len_utf8()],
                None => "",
            },
        };
        RESERVED.contains(&part.to_uppercase().as_str())
    }

    /// The Windows path normalized: reserved device names (CON:) kept as devices, a relative path never read as a
    /// drive.
    pub fn normalize(path: &str) -> String {
        let b = path.as_bytes();
        let len = b.len();
        if len == 0 {
            return ".".into();
        }
        if len == 1 {
            return if b[0] == b'/' { "\\".into() } else { path.into() };
        }
        let (mut root_end, mut device, mut absolute) = (0usize, None::<String>, false);
        if is_sep(b[0]) {
            absolute = true;
            if len > 1 && is_sep(b[1]) {
                let mut j = 2;
                let mut last = j;
                while j < len && !is_sep(b[j]) {
                    j += 1;
                }
                if j < len && j != last {
                    let first = &path[last..j];
                    last = j;
                    while j < len && is_sep(b[j]) {
                        j += 1;
                    }
                    if j < len && j != last {
                        last = j;
                        while j < len && !is_sep(b[j]) {
                            j += 1;
                        }
                        if first == "." || first == "?" {
                            // A device root (\\.\PHYSICALDRIVE0), or a reserved device (\\?\COM1:).
                            device = Some(format!("\\\\{first}"));
                            root_end = 4;
                            let end = path.find(':').map_or(0, |c| c + 1);
                            let possible = if end > 4 { &path[4..end] } else { "" };
                            if !possible.is_empty() && reserved(possible, Some(possible.len() - 1)) {
                                device = Some(format!("\\\\?\\{possible}"));
                                root_end = 4 + possible.len();
                            }
                        } else if j == len {
                            // A bare share (\\server\share) is the root alone, with its separator.
                            return format!("\\\\{first}\\{}\\", &path[last..]);
                        } else {
                            device = Some(format!("\\\\{first}\\{}", &path[last..j]));
                            root_end = j;
                        }
                    }
                }
            } else {
                root_end = 1;
            }
        } else if let Some(colon) = path.find(':').filter(|&c| c > 0) {
            if colon == 1 && b[0].is_ascii_alphabetic() {
                device = Some(path[..2].to_string());
                root_end = 2;
                if len > 2 && is_sep(b[2]) {
                    absolute = true;
                    root_end = 3;
                }
            } else if reserved(path, Some(colon)) {
                device = Some(path[..colon + 1].to_string());
                root_end = colon + 1;
            }
        }
        let mut tail =
            if root_end < len { normalize_string(&path[root_end..], !absolute, "\\", is_sep) } else { String::new() };
        if tail.is_empty() && !absolute {
            tail = ".".into();
        }
        if !tail.is_empty() && is_sep(b[len - 1]) {
            tail.push('\\');
        }
        // A relative path with a colon not tied to a drive must not read as one once normalized (CVE-2024-36139).
        if !absolute && device.is_none() && path.contains(':') {
            if is_drive(tail.as_bytes()) {
                return format!(".\\{tail}");
            }
            if path.match_indices(':').any(|(i, _)| i == len - 1 || is_sep(b[i + 1])) {
                return format!(".\\{tail}");
            }
        }
        if reserved(path, path.find(':')) {
            return format!(".\\{}{tail}", device.unwrap_or_default());
        }
        match device {
            None if absolute => format!("\\{tail}"),
            None => tail,
            Some(d) if absolute => format!("{d}\\{tail}"),
            Some(d) => format!("{d}{tail}"),
        }
    }

    pub fn join(parts: &[&str]) -> String {
        let kept: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
        let Some(first) = kept.first() else { return ".".into() };
        let mut joined = kept.join("\\");
        // A first part starting with exactly two separators keeps its UNC root; more than two are made one, so that
        // a join never makes a UNC path out of parts that were not one.
        let fb = first.as_bytes();
        let mut needs_replace = true;
        let mut slashes = 0;
        if is_sep(fb[0]) {
            slashes += 1;
            if fb.len() > 1 && is_sep(fb[1]) {
                slashes += 1;
                if fb.len() > 2 {
                    if is_sep(fb[2]) {
                        slashes += 1;
                    } else {
                        needs_replace = false;
                    }
                }
            }
        }
        if needs_replace {
            let jb = joined.as_bytes();
            while slashes < jb.len() && is_sep(jb[slashes]) {
                slashes += 1;
            }
            if slashes >= 2 {
                joined = format!("\\{}", &joined[slashes..]);
            }
        }
        // A part named after a reserved device (CON:, COM1:…): not normalized, its slashes turned.
        if joined.split('\\').any(|p| p.find(':').is_some_and(|c| reserved(p, Some(c)))) {
            return joined.replace('/', "\\");
        }
        normalize(&joined)
    }

    /// Case-insensitive, as Windows' file names: compared character by character on their lowercase (a character
    /// whose lowercase is several is kept as it is).
    pub fn relative(from: &str, to: &str) -> String {
        if from == to {
            return String::new();
        }
        let (from_orig, to_orig) = (resolve(&[from]), resolve(&[to]));
        if from_orig == to_orig {
            return String::new();
        }
        let lower = |s: &str| -> Vec<char> {
            s.chars()
                .map(|c| {
                    let mut l = c.to_lowercase();
                    match (l.next(), l.next()) {
                        (Some(x), None) => x,
                        _ => c,
                    }
                })
                .collect()
        };
        let (f, t) = (lower(&from_orig), lower(&to_orig));
        let to_chars: Vec<char> = to_orig.chars().collect();
        let slice = |a: usize, b: usize| -> String { to_chars[a..b].iter().collect() };
        if f == t {
            return String::new();
        }
        let mut from_start = 0;
        while from_start < f.len() && f[from_start] == '\\' {
            from_start += 1;
        }
        let mut from_end = f.len();
        while from_end > from_start + 1 && f[from_end - 1] == '\\' {
            from_end -= 1;
        }
        let from_len = from_end - from_start;
        let mut to_start = 0;
        while to_start < t.len() && t[to_start] == '\\' {
            to_start += 1;
        }
        let mut to_end = t.len();
        while to_end > to_start + 1 && t[to_end - 1] == '\\' {
            to_end -= 1;
        }
        let to_len = to_end - to_start;
        let length = from_len.min(to_len);
        let mut last_common: isize = -1;
        let mut i = 0;
        while i < length {
            let c = f[from_start + i];
            if c != t[to_start + i] {
                break;
            } else if c == '\\' {
                last_common = i as isize;
            }
            i += 1;
        }
        if i != length {
            if last_common == -1 {
                return to_orig;
            }
        } else {
            if to_len > length {
                if t[to_start + i] == '\\' {
                    return slice(to_start + i + 1, to_chars.len());
                }
                if i == 2 {
                    return slice(to_start + i, to_chars.len());
                }
            }
            if from_len > length {
                if f[from_start + i] == '\\' {
                    last_common = i as isize;
                } else if i == 2 {
                    last_common = 3;
                }
            }
            if last_common == -1 {
                last_common = 0;
            }
        }
        let mut out = String::new();
        #[allow(clippy::needless_range_loop)] // k reaches from_end, past the last index
        for k in (from_start as isize + last_common + 1) as usize..=from_end {
            if k == from_end || f[k] == '\\' {
                out.push_str(if out.is_empty() { ".." } else { "\\.." });
            }
        }
        let mut start = (to_start as isize + last_common) as usize;
        if !out.is_empty() {
            return format!("{out}{}", slice(start, to_end));
        }
        if to_chars.get(start) == Some(&'\\') {
            start += 1;
        }
        slice(start, to_end)
    }

    pub fn dirname(path: &str) -> String {
        let b = path.as_bytes();
        let len = b.len();
        if len == 0 {
            return ".".into();
        }
        if len == 1 {
            return if is_sep(b[0]) { path.into() } else { ".".into() };
        }
        let (mut root_end, mut offset): (isize, usize) = (-1, 0);
        if is_sep(b[0]) {
            root_end = 1;
            offset = 1;
            if is_sep(b[1]) {
                let mut j = 2;
                let mut last = j;
                while j < len && !is_sep(b[j]) {
                    j += 1;
                }
                if j < len && j != last {
                    last = j;
                    while j < len && is_sep(b[j]) {
                        j += 1;
                    }
                    if j < len && j != last {
                        last = j;
                        while j < len && !is_sep(b[j]) {
                            j += 1;
                        }
                        if j == len {
                            return path.into();
                        }
                        if j != last {
                            root_end = j as isize + 1;
                            offset = j + 1;
                        }
                    }
                }
            }
        } else if is_drive(b) {
            root_end = if len > 2 && is_sep(b[2]) { 3 } else { 2 };
            offset = root_end as usize;
        }
        let mut end: isize = -1;
        let mut matched_slash = true;
        let mut i = len as isize - 1;
        while i >= offset as isize {
            if is_sep(b[i as usize]) {
                if !matched_slash {
                    end = i;
                    break;
                }
            } else {
                matched_slash = false;
            }
            i -= 1;
        }
        if end == -1 {
            if root_end == -1 {
                return ".".into();
            }
            end = root_end;
        }
        path[..end as usize].to_string()
    }

    pub fn basename(path: &str, suffix: Option<&str>) -> String {
        let start = if is_drive(path.as_bytes()) { 2 } else { 0 };
        strip_suffix(path, basename_from(path, start, is_sep), suffix)
    }

    pub fn extname(path: &str) -> String {
        let start = if is_drive(path.as_bytes()) { 2 } else { 0 };
        extname_from(path, start, is_sep)
    }

    /// The root of a Windows path: a drive (`C:`, `C:\`), a share (`\\server\share\`), or a separator.
    pub fn root(path: &str) -> String {
        let b = path.as_bytes();
        if b.is_empty() {
            return String::new();
        }
        if is_sep(b[0]) {
            if b.len() > 1
                && is_sep(b[1])
                && let Some((_, end)) = unc(path, false)
            {
                let end = if end < b.len() && is_sep(b[end]) { end + 1 } else { end };
                return path[..end].to_string();
            }
            return path[..1].to_string();
        }
        if is_drive(b) {
            return path[..if b.len() > 2 && is_sep(b[2]) { 3 } else { 2 }].to_string();
        }
        String::new()
    }
}
