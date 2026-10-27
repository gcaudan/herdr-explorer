//! What the search finds: the files of the folder, the fuzzy match of Go to file, the globs and the
//! matches of Search in files. Matched positions and ranges are byte offsets into the path or line.

use crate::core::exec::{self, Opts};
use crate::core::paths;
use crate::core::regex::Regex;
use std::collections::HashSet;

/// Walk without git.
pub const MAX_FILES: usize = 50000;
/// Larger files are not searched.
const MAX_SIZE: u64 = 2 << 20;
pub const MAX_MATCHES: usize = 5000;
/// Go to file results kept.
pub const MAX_ROWS: usize = 1000;
/// Files read at once.
pub const BATCH: usize = 24;

/// The byte offset of the n-th character of s, or its length.
pub fn char_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(b, _)| b)
}

// --- file list

/// Tracked and untracked files not ignored, relative to root (git lists only the folder it runs in, even below the
/// top of the repository), minus those deleted from the working tree.
pub fn git_files(root: &str) -> Option<Vec<String>> {
    let git = exec::git()?;
    let opts = Opts { cwd: Some(root.into()), env: Some(exec::git_env(root)), ..Default::default() };
    let all = exec::run(Some(git), &["ls-files", "-z", "-co", "--exclude-standard"], &opts);
    let deleted = exec::run(Some(git), &["ls-files", "-z", "-d"], &opts);
    let all = all.filter(|a| !a.is_empty())?;
    let gone: HashSet<&str> = deleted.as_deref().unwrap_or("").split('\0').collect();
    let mut seen = HashSet::new();
    Some(all.split('\0').filter(|f| seen.insert(*f) && !f.is_empty() && !gone.contains(f)).map(String::from).collect())
}

/// Without git, or to include ignored and hidden files: the folder itself, without .git (nor node_modules and
/// dot-folders unless hidden), symbolic links followed once (real paths remembered against loops), capped. The
/// folders read one after the other, the list sorted after.
pub fn walk(root: &str, hidden: bool) -> (Vec<String>, bool) {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = std::collections::VecDeque::from([String::new()]);
    while let Some(rel) = queue.pop_front() {
        if files.len() >= MAX_FILES {
            break;
        }
        let abs = paths::join(&[root, &rel]);
        let Ok(real) = exec::walk_realpath(&abs) else { continue };
        if !seen.insert(real) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&abs) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let Ok(kind) = e.file_type() else { continue };
            let mut dir = kind.is_dir();
            // A link to a network share is left out, never followed (see remote_link).
            if kind.is_symlink() {
                let full = paths::join(&[&abs, &name]);
                if exec::remote_link(&full, root) {
                    continue;
                }
                match std::fs::metadata(&full) {
                    Ok(m) => dir = m.is_dir(),
                    Err(_) => continue,
                }
            }
            if !dir {
                if files.len() < MAX_FILES {
                    files.push(r);
                }
                continue;
            }
            if name == ".git" || (!hidden && (name == "node_modules" || name.starts_with('.'))) {
                continue;
            }
            queue.push_back(r);
        }
    }
    let capped = files.len() >= MAX_FILES;
    (files, capped)
}

// --- fuzzy matching

// Subsequence match scored like fzf and VS Code: each matched character scores, more at the start of the path, of a
// segment or a word, and in the file name; consecutive runs score more, gaps cost. Dynamic programming over (query
// character, path position), keeping the best alignment.
const SCORE: i64 = 16;
const CONSEC: i64 = 12;
const GAP_OPEN: i64 = 3;
const GAP: i64 = 1;
const SEGMENT: i64 = 10;
const WORD: i64 = 7;
const CAMEL: i64 = 6;
const NAME: i64 = 4;
const CASE: i64 = 1;
const NEG: i64 = -100_000_000;

/// The character is its own lower case (a missing one never is).
fn is_lower(c: Option<char>) -> bool {
    c.is_some_and(|ch| {
        let mut l = ch.to_lowercase();
        l.next() == Some(ch) && l.next().is_none()
    })
}

fn bonus(s: &[char], j: usize, base: usize) -> i64 {
    let p = if j > 0 { s.get(j - 1).copied() } else { Some('/') };
    let mut b = if j >= base { NAME } else { 0 };
    if p == Some('/') || p == Some('\\') {
        b += SEGMENT;
    } else if !p.is_some_and(|ch| crate::unicode::is_letter(ch) || crate::unicode::is_number(ch)) {
        // None (a path shorter than its lower case) counts as a letter.
        if p.is_some() {
            b += WORD;
        }
    } else if is_lower(p) && !is_lower(s.get(j).copied()) {
        b += CAMEL;
    }
    b
}

/// Score of the query q (ql in lower case, both as characters) in the path s (sl), whose name starts at byte base,
/// or None; with positions: also the byte offsets in s of the matched characters.
pub fn fuzzy(q: &[char], ql: &[char], s: &str, sl: &str, base: usize, positions: bool) -> Option<(i64, Vec<usize>)> {
    let mut want = ql.iter().peekable();
    for c in sl.chars() {
        match want.peek() {
            None => break,
            Some(&&w) if w == c => {
                want.next();
            }
            _ => {}
        }
    }
    if want.peek().is_some() {
        return None;
    }
    let path = s;
    let base = path[..base].chars().count();
    let (s, sl): (Vec<char>, Vec<char>) = (s.chars().collect(), sl.chars().collect());
    let (m, n) = (ql.len(), sl.len());
    let mut cells = vec![0i64; m * n];
    let mut from = vec![0u8; m * n];
    for (i, &qc) in ql.iter().enumerate() {
        let row = i * n;
        // best previous-row cell at least two columns back, minus the gap cost
        let mut gap = NEG;
        for j in 0..n {
            if i > 0 && j >= 2 {
                gap = (gap - GAP).max(cells[row - n + j - 2] - GAP_OPEN);
            }
            let (mut v, mut f) = (NEG, 0);
            if sl[j] == qc {
                let b = SCORE + bonus(&s, j, base) + if s.get(j) == q.get(i) { CASE } else { 0 };
                if i == 0 {
                    v = b;
                } else {
                    let diag = if j > 0 { cells[row - n + j - 1] } else { NEG };
                    if diag > NEG / 2 && diag + CONSEC >= gap {
                        v = diag + CONSEC + b;
                        f = 1;
                    } else if gap > NEG / 2 {
                        v = gap + b;
                        f = 2;
                    }
                }
            }
            cells[row + j] = v;
            from[row + j] = f;
        }
    }
    let (mut best, mut end) = (NEG, -1i64);
    let last = (m - 1) * n;
    for j in 0..n {
        if cells[last + j] > best {
            best = cells[last + j];
            end = j as i64;
        }
    }
    if best <= NEG / 2 {
        return None;
    }
    if !positions {
        return Some((best, Vec::new()));
    }
    let mut pos = vec![end as usize];
    for i in (1..m).rev() {
        let up = (i - 1) * n;
        if end >= 0 && from[i * n + end as usize] == 1 {
            end -= 1;
        } else {
            let (mut k, mut kv) = (-1i64, NEG);
            for c in 0..(end - 1).max(0) {
                let v = cells[up + c as usize] - GAP_OPEN - (end - c - 2) * GAP;
                if v > kv {
                    kv = v;
                    k = c;
                }
            }
            end = k;
        }
        pos.push(end.max(0) as usize);
    }
    pos.reverse();
    let at: Vec<usize> = path.char_indices().map(|(b, _)| b).chain([path.len()]).collect();
    Some((best, pos.into_iter().map(|j| at[j.min(s.len())]).collect()))
}

/// `path:line` (or path:line:column) jumps to a line: the query without blanks, / for \, and the line (0: none).
pub fn parse_query(text: &str) -> (String, f64) {
    static LINE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^(.*?):(\d+)(?::\d+)?$", "").expect("regex"));
    static BLANKS: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"\s+", "g").expect("regex"));
    let trimmed = text.trim_matches(|c| crate::unicode::is_space(c) || crate::core::regex::is_line_terminator(c));
    let m = LINE.exec(trimmed);
    let q = m.as_ref().and_then(|m| m.get(1)).unwrap_or(text);
    let q = BLANKS.replace_with(q, true, "").replace('\\', "/");
    let line = m.as_ref().and_then(|m| m.get(2)).map_or(0.0, |s| s.parse().unwrap_or(0.0));
    (q, line)
}

/// A query as characters, and in lower case.
pub fn query_chars(q: &str) -> (Vec<char>, Vec<char>) {
    (q.chars().collect(), q.to_lowercase().chars().collect())
}

// --- search in files

/// Glob to RegExp on '/' paths: * and ? within a segment, ** across segments, {a,b}, [abc]. A pattern without a
/// leading ./ or / matches at any depth, and a folder matches its contents (VS Code behaviour).
fn glob_source(glob: &str) -> String {
    let g = glob
        .trim_matches(|c| crate::unicode::is_space(c) || crate::core::regex::is_line_terminator(c))
        .replace('\\', "/");
    let g = if let Some(rest) = g.strip_prefix("./").or_else(|| g.strip_prefix('/')) {
        rest.to_string()
    } else if g.starts_with("**/") {
        g
    } else {
        format!("**/{g}")
    };
    let g: Vec<char> = g.trim_end_matches('/').chars().collect();
    let mut s = String::new();
    let mut depth = 0;
    let mut i = 0;
    while i < g.len() {
        let c = g[i];
        if c == '*' && g.get(i + 1) == Some(&'*') {
            i += 1;
            if g.get(i + 1) == Some(&'/') {
                i += 1;
                s += "(?:.*/)?";
            } else {
                s += ".*";
            }
        } else if c == '*' {
            s += "[^/]*";
        } else if c == '?' {
            s += "[^/]";
        } else if c == '{' {
            depth += 1;
            s += "(?:";
        } else if c == '}' && depth > 0 {
            depth -= 1;
            s += ")";
        } else if c == ',' && depth > 0 {
            s += "|";
        } else if c == '[' {
            let end = (i + 2..g.len()).find(|&k| g[k] == ']');
            let Some(end) = end else {
                s += "\\[";
                i += 1;
                continue;
            };
            let inner: String = g[i + 1..end].iter().collect();
            let inner = inner.strip_prefix('!').map_or(inner.clone(), |r| format!("^{r}"));
            s += &format!("[{}]", inner.replace('\\', "\\\\"));
            i = end;
        } else {
            if ".+^$()|\\".contains(c) {
                s.push('\\');
            }
            s.push(c);
        }
        i += 1;
    }
    s + &")".repeat(depth) + "(?:/.*)?"
}

/// Comma-separated globs (commas inside braces kept) as one RegExp, or None when empty.
pub fn globs(text: &str) -> Option<Regex> {
    let mut parts = Vec::new();
    let (mut cur, mut depth) = (String::new(), 0usize);
    for c in text.chars() {
        if c == ',' && depth == 0 {
            parts.push(std::mem::take(&mut cur));
            continue;
        }
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth = depth.saturating_sub(1);
        }
        cur.push(c);
    }
    parts.push(cur);
    let sources: Vec<String> = parts
        .iter()
        .filter(|p| {
            !p.trim_matches(|c| crate::unicode::is_space(c) || crate::core::regex::is_line_terminator(c)).is_empty()
        })
        .map(|p| glob_source(p))
        .collect();
    if sources.is_empty() {
        return None;
    }
    Regex::new(&format!("^(?:{})$", sources.join("|")), if exec::WIN { "i" } else { "" }).ok()
}

/// The text's matcher; the message shown when it is not a valid regular expression.
pub fn matcher(text: &str, case: bool, word: bool, regex: bool) -> Result<Regex, String> {
    let mut src = text.to_string();
    if !regex {
        src = src.chars().fold(String::new(), |mut s, c| {
            if ".*+?^${}()|[]\\".contains(c) {
                s.push('\\');
            }
            s.push(c);
            s
        });
    }
    if word {
        src = format!("\\b(?:{src})\\b");
    }
    Regex::new(&src, if case { "gm" } else { "gmi" }).map_err(|e| {
        // The engine's wording: "Invalid regular expression: /src/flags: Unterminated group", shown without its start.
        let msg = e.strip_prefix("Invalid regular expression: ").unwrap_or(&e);
        let mut chars = msg.chars();
        let msg = match chars.next() {
            Some(c) if !msg.starts_with("numbers") => c.to_uppercase().collect::<String>() + chars.as_str(),
            _ => msg.to_string(),
        };
        format!("Invalid regex: {msg}")
    })
}

/// Text of a regular file, or why it is not searchable: too large or binary (NUL in the first 8 kB). Stat first:
/// opening a FIFO would block. A file is read only when no link on its way below the root leads to a network share:
/// a crafted index could list a file under a linked folder.
pub fn read_text(abs: &str, root: &str) -> Result<String, &'static str> {
    if exec::remote_link(abs, root) {
        return Err("link to a network share");
    }
    let st = std::fs::metadata(abs).map_err(|_| "cannot read")?;
    if !st.is_file() {
        return Err("not a file");
    }
    if st.len() > MAX_SIZE {
        return Err("file too large");
    }
    let buf = std::fs::read(abs).map_err(|_| "cannot read")?;
    if buf[..buf.len().min(8192)].contains(&0) {
        return Err("binary file");
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Non-empty match byte ranges of re in a text, at most max.
pub fn ranges(re: &Regex, text: &str, max: usize) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut i = 0;
    while found.len() < max {
        let Some(m) = re.exec_at(text, i) else { break };
        if m.start() == m.end() {
            // lastIndex++: past the character.
            match text[m.end()..].chars().next() {
                Some(c) => i = m.end() + c.len_utf8(),
                None => break,
            }
            continue;
        }
        found.push((m.start(), m.end()));
        i = m.end();
    }
    found
}

/// A matching line: its number, its text (long lines cut around the first match) and the byte ranges of the
/// matches in it; cut: its start left out.
#[derive(Clone, Debug)]
pub struct Hit {
    pub n: usize,
    pub text: String,
    pub cut: bool,
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct Group {
    pub rel: String,
    pub matches: Vec<Hit>,
    pub count: usize,
    pub collapsed: bool,
}

pub fn grep(root: &str, rel: &str, re: &Regex, room: usize) -> Option<Group> {
    let text = read_text(&paths::join(&[root, rel]), root).ok().filter(|t| !t.is_empty())?;
    // most files: no split needed
    if !re.test(&text) {
        return None;
    }
    let (mut matches, mut count) = (Vec::new(), 0);
    for (n, l) in text.split('\n').enumerate() {
        if count >= room {
            break;
        }
        let l = l.strip_suffix('\r').unwrap_or(l);
        let r = ranges(re, l, room - count);
        if r.is_empty() {
            continue;
        }
        count += r.len();
        // Long lines (minified files) kept around the first match only: 1000 characters from 100 before it.
        let (start, end) = if l.len() > 1000 && l.chars().count() > 1000 {
            let start = char_at(l, l[..r[0].0].chars().count().saturating_sub(100));
            (start, start + char_at(&l[start..], 1000))
        } else {
            (0, l.len())
        };
        matches.push(Hit {
            n: n + 1,
            text: l[start..end].to_string(),
            cut: start > 0,
            ranges: r.iter().filter(|&&(a, _)| a < end).map(|&(a, b)| (a - start, b.min(end) - start)).collect(),
        });
    }
    (!matches.is_empty()).then(|| Group { rel: rel.to_string(), matches, count, collapsed: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(q: &str, s: &str) -> Option<(i64, Vec<usize>)> {
        let base = s.rfind('/').map_or(0, |i| i + 1);
        let (q, ql) = query_chars(q);
        fuzzy(&q, &ql, s, &s.to_lowercase(), base, true)
    }

    #[test]
    fn fuzzy_scores() {
        assert_eq!(score("fb", "src/fooBar.js"), Some((53, vec![4, 7])));
        assert_eq!(score("fb", "fb.txt"), Some((64, vec![0, 1])));
        assert_eq!(score("ma", "beta/main.rs"), Some((64, vec![5, 6])));
        assert_eq!(score("bma", "beta/main.rs"), Some((85, vec![0, 5, 6])));
        assert_eq!(score("zz", "beta/main.rs"), None);
        assert_eq!(score("fb", "x/ä/fb-é.txt"), Some((64, vec![5, 6])));
        assert_eq!(score("fb", "Fab/FB.txt"), Some((62, vec![4, 5])));
        assert_eq!(score("ab", "aXbYab/cab"), Some((54, vec![8, 9])));
    }

    #[test]
    fn globs_match_names_and_folders() {
        let g = globs("*.rs,*.{ts,txt}").unwrap();
        assert!(g.test("src/a.rs") && g.test("e.ts") && g.test("d.txt") && !g.test("a.js"));
        let g = globs("src/deep,mod*").unwrap();
        assert!(g.test("src/deep/b.rs") && g.test("src/mod.rs") && !g.test("lib/c.rs"));
        assert!(globs(" , ").is_none());
        assert_eq!(glob_source("./a/[!x]?"), "a/[^x][^/](?:/.*)?");
    }

    #[test]
    fn query_with_a_line() {
        assert_eq!(parse_query(" a b:12:3 "), ("ab".into(), 12.0));
        assert_eq!(parse_query("a\\b"), ("a/b".into(), 0.0));
    }

    #[test]
    fn regex_errors_shown_short() {
        assert_eq!(matcher("(fo", false, false, true).err().unwrap(), "Invalid regex: Unterminated group");
        assert!(matcher("(fo", false, false, false).is_ok());
    }
}
