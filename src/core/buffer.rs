//! The text of a file edited in the preview (its edit mode, i): lines, a cursor and a selection, moves over wide
//! characters, combining marks and tabs, edits with undo and redo, find and replace, the lines changed since the
//! file was opened, and the file read and written back as it was (line endings, final newline, byte order mark).
//! Offsets in a line are in bytes, always at the start of a character.

use super::exec::{io_code, walk_realpath};
use super::paths;
use super::ui::cols;
use crate::unicode;
use std::collections::{BTreeSet, HashMap};
use std::fs::Metadata;

/// Larger files are not edited.
pub const MAX_BYTES: usize = 2 << 20;
/// Columns between tab stops.
pub const TAB: usize = 4;

// --- the file

/// A file's text as lines to edit. eol: the line ending most lines have ("\n" or "\r\n"), written back on every
/// line; final_: whether the text ends with one; mixed: both kinds were found.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub lines: Vec<String>,
    pub eol: &'static str,
    pub final_: bool,
    pub bom: bool,
    pub mixed: bool,
}

/// A file's bytes as lines to edit, or why not: too large, binary (a NUL byte) or not UTF-8.
pub fn parse(buf: &[u8]) -> Result<Doc, String> {
    if buf.len() > MAX_BYTES {
        return Err(format!("over {} MB", MAX_BYTES >> 20));
    }
    if buf.contains(&0) {
        return Err("binary file".into());
    }
    let text = std::str::from_utf8(buf).map_err(|_| "not UTF-8".to_string())?;
    let bom = text.starts_with('\u{feff}');
    let text = if bom { &text[3..] } else { text };
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    let final_ = text.ends_with('\n');
    // text.split(/\r?\n/)
    let mut lines: Vec<String> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
    // The last part has no line break after it: its \r is its own.
    if let Some(last) = text.rsplit('\n').next() {
        *lines.last_mut().unwrap() = last.to_string();
    }
    if final_ {
        lines.pop();
    }
    Ok(Doc { lines, eol: if crlf > lf { "\r\n" } else { "\n" }, final_, bom, mixed: crlf > 0 && lf > 0 })
}

/// The text to write: the lines joined with the file's line ending, its final one and byte order mark kept.
pub fn serialize(lines: &[String], eol: &str, final_: bool, bom: bool) -> String {
    format!("{}{}{}", if bom { "\u{feff}" } else { "" }, lines.join(eol), if final_ { eol } else { "" })
}

impl Doc {
    pub fn serialize(&self) -> String {
        serialize(&self.lines, self.eol, self.final_, self.bom)
    }
}

/// The file read whole, as parse() gives it, with its stat; or why not (the error's code, io_code).
pub fn read(file: &str) -> Result<(Doc, Metadata), String> {
    use std::io::Read;
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    // A folder opens too (FILE_FLAG_BACKUP_SEMANTICS): it is then not a regular file.
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(&mut opts, 0x0200_0000);
    let mut f = opts.open(file).map_err(|e| io_code(&e))?;
    let stat = f.metadata().map_err(|e| io_code(&e))?;
    if !stat.is_file() {
        return Err("not a regular file".into());
    }
    if stat.len() > MAX_BYTES as u64 {
        return Err(format!("over {} MB", MAX_BYTES >> 20));
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| io_code(&e))?;
    Ok((parse(&buf)?, stat))
}

/// Whether the file can be replaced by a new one: one name only and, but on Windows, owned by the user.
fn mine(real: &str, stat: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = real;
        stat.nlink() <= 1 && stat.uid() == crate::sys::getuid()
    }
    #[cfg(windows)]
    {
        let _ = stat;
        crate::sys::links(real) <= 1
    }
}

/// n in base 36, lowercase digits.
fn base36(mut n: u128) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// The new file beside the one replaced, with its permissions and group, renamed over it.
fn replace_file(real: &str, tmp: &str, text: &str, stat: Option<&Metadata>) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    // wx: never through a file or link already there.
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(stat.map_or(0o666, |s| s.permissions().mode() & 0o7777));
    }
    std::io::Write::write_all(&mut opts.open(tmp)?, text.as_bytes())?;
    if let Some(stat) = stat {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            // Its group too (EPERM when not one of the user's: written in place), before chmod as chown clears
            // setgid.
            if std::fs::metadata(tmp)?.gid() != stat.gid() {
                std::os::unix::fs::chown(tmp, Some(stat.uid()), Some(stat.gid()))?;
            }
            std::fs::set_permissions(tmp, std::fs::Permissions::from_mode(stat.permissions().mode() & 0o7777))?;
        }
        #[cfg(not(unix))]
        std::fs::set_permissions(tmp, stat.permissions())?;
    }
    std::fs::rename(tmp, real)
}

/// The text written over the file at once: to a new temporary file beside it, with the file's permissions and group,
/// then renamed over it. A link keeps pointing to it: its target is written. A file with other names (hard links),
/// owned by another user, or in a folder that cannot be written, is written in place instead, which keeps all of
/// that. Returns the file's new stat.
pub fn save(file: &str, text: &str) -> std::io::Result<Metadata> {
    let (real, stat) = match walk_realpath(file) {
        Ok(real) => {
            let stat = std::fs::metadata(&real).ok();
            (real, stat)
        }
        Err(_) => (file.to_string(), None),
    };
    if stat.as_ref().is_none_or(|s| mine(&real, s)) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let name = format!(".{}.{}-{}.tmp", paths::basename(&real, None), std::process::id(), base36(now));
        let tmp = paths::join(&[&paths::dirname(&real), &name]);
        match replace_file(&real, &tmp, text, stat.as_ref()) {
            Ok(()) => return std::fs::metadata(&real),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                if stat.is_none() || !["EACCES", "EPERM", "EROFS", "EXDEV", "EBUSY"].contains(&io_code(&e).as_str()) {
                    return Err(e);
                }
            }
        }
    }
    std::fs::write(&real, text)?;
    std::fs::metadata(&real)
}

// --- characters and columns

/// What joins the character before it, for the cursor: combining marks, zero-width spaces and joiners, variation
/// selectors, skin tones.
fn joins(c: char) -> bool {
    unicode::is_mark(c) || matches!(c, '\u{200b}'..='\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{1f3fb}'..='\u{1f3ff}')
}

fn flag_part(c: char) -> bool {
    ('\u{1f1e6}'..='\u{1f1ff}').contains(&c)
}

fn is_control(c: char) -> bool {
    matches!(c, '\x00'..='\x1f' | '\x7f'..='\u{9f}')
}

fn char_at(text: &str, i: usize) -> char {
    text[i..].chars().next().unwrap_or('\0')
}

/// Whether the character ending at j is a zero-width joiner.
fn after_zwj(text: &str, j: usize) -> bool {
    text[..j].ends_with('\u{200d}')
}

/// Where the character after the one at i starts (i itself at the end). A flag's two halves go together, and any
/// character after a zero-width joiner joins (👩‍💻).
pub fn next_at(text: &str, i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    let c = char_at(text, i);
    let mut j = i + c.len_utf8();
    if flag_part(c) && j < text.len() && flag_part(char_at(text, j)) {
        j += 4;
    }
    while j < text.len() {
        let ch = char_at(text, j);
        if !joins(ch) && !after_zwj(text, j) {
            break;
        }
        j += ch.len_utf8();
    }
    j
}

/// A character's start before i (0 at most), from which the boundaries are known: one that joins nothing before it,
/// and no flag half, whose pairing depends on the halves before.
fn safe_start(text: &str, i: usize) -> usize {
    let mut end = i.min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    for (k, c) in text[..end].char_indices().rev() {
        if k == 0 {
            break;
        }
        if !joins(c) && !flag_part(c) && !after_zwj(text, k) {
            return k;
        }
    }
    0
}

/// Where the character before the one at i starts: found from a known boundary, not from the start of the line, so
/// that a long line (minified code) stays fast.
pub fn prev_at(text: &str, i: usize) -> usize {
    let k = safe_start(text, i);
    let mut p = k;
    let mut q = next_at(text, k);
    while q < i {
        p = q;
        q = next_at(text, q);
    }
    p.min(i)
}

/// The start of the character holding i (i itself when it is one).
pub fn snap(text: &str, i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    let mut p = 0;
    let mut q = safe_start(text, i + 1);
    while q <= i {
        p = q;
        q = next_at(text, q);
    }
    p
}

/// A line as the screen shows it: tabs as spaces to the next stop, other control characters as ? (one column).
/// map[i]: where the character at byte i starts in the shown text; col[i]: its column; both with an entry for the
/// end.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub text: String,
    pub map: Vec<usize>,
    pub col: Vec<usize>,
    pub width: usize,
}

pub fn shown(text: &str) -> Shown {
    let (mut map, mut col) = (vec![0; text.len() + 1], vec![0; text.len() + 1]);
    let (mut out, mut w, mut prev) = (String::new(), 0, None);
    for (i, ch) in text.char_indices() {
        for k in i..i + ch.len_utf8() {
            map[k] = out.len();
            col[k] = w;
        }
        if ch == '\t' {
            let n = TAB - w % TAB;
            out += &" ".repeat(n);
            w += n;
            prev = None;
        } else if is_control(ch) {
            out.push('?');
            w += 1;
            prev = None;
        } else {
            out.push(ch);
            w += cols(ch, prev);
            prev = Some(ch);
        }
    }
    map[text.len()] = out.len();
    col[text.len()] = w;
    Shown { text: out, map, col, width: w }
}

pub fn col_of(text: &str, at: usize) -> usize {
    shown(text).col[at.min(text.len())]
}

/// The character under a column (its start), or the end of the line past it.
pub fn at_col(text: &str, want: usize) -> usize {
    let s = shown(text);
    let mut i = 0;
    while i < text.len() {
        let j = next_at(text, i);
        if s.col[j] > want {
            break;
        }
        i = j;
    }
    i
}

/// Words for ctrl+← →, double-clicks: letters, digits and _ (1); spaces (0); anything else (2).
fn kind(c: char) -> u8 {
    if unicode::is_letter(c) || unicode::is_number(c) || unicode::is_mark(c) || c == '_' || c == '$' {
        1
    } else if crate::unicode::is_space(c) {
        0
    } else {
        2
    }
}

/// The end of the word after at, spaces before it skipped.
pub fn word_end(text: &str, mut at: usize) -> usize {
    while at < text.len() && kind(char_at(text, at)) == 0 {
        at = next_at(text, at);
    }
    let k = (at < text.len()).then(|| kind(char_at(text, at)));
    while at < text.len() && Some(kind(char_at(text, at))) == k {
        at = next_at(text, at);
    }
    at
}

/// The start of the word before at, spaces after it skipped.
pub fn word_start(text: &str, mut at: usize) -> usize {
    let before = |at: usize| kind(char_at(text, prev_at(text, at)));
    while at > 0 && before(at) == 0 {
        at = prev_at(text, at);
    }
    let k = (at > 0).then(|| before(at));
    while at > 0 && Some(before(at)) == k {
        at = prev_at(text, at);
    }
    at
}

/// The indentation a file uses: a tab when most indented lines start with one, else spaces, as many as the smallest
/// indentation of two or more (4 when none).
pub fn indent_of(lines: &[String]) -> String {
    let (mut tabs, mut spaces, mut least) = (0, 0, usize::MAX);
    for l in lines.iter().take(5000) {
        if l.starts_with('\t') {
            tabs += 1;
        } else if l.starts_with(' ') {
            spaces += 1;
            if let Some(n) = l.bytes().position(|b| b != b' ')
                && n >= 2
                && n < least
            {
                least = n;
            }
        }
    }
    if tabs > spaces { "\t".into() } else { " ".repeat(if least <= 8 { least } else { 4 }) }
}

// --- lines changed

/// How a line differs from the file as opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Same,
    Add,
    Mod,
}

impl LineKind {
    pub fn name(self) -> &'static str {
        match self {
            LineKind::Same => "",
            LineKind::Add => "add",
            LineKind::Mod => "mod",
        }
    }
}

/// The lines of b changed from a, and deleted: the lines of b before which lines of a were removed (b's length: at
/// the end).
#[derive(Debug, Clone, PartialEq)]
pub struct LineDiff {
    pub kinds: Vec<LineKind>,
    pub deleted: BTreeSet<usize>,
}

/// The lines both start and end with are skipped, then the rest compared line by line (myers), at most limit
/// differences: past it, all of it counts as changed.
pub fn line_diff(a: &[String], b: &[String], limit: usize) -> LineDiff {
    let mut kinds = vec![LineKind::Same; b.len()];
    let mut deleted = BTreeSet::new();
    let (mut s, mut ea, mut eb) = (0, a.len(), b.len());
    while s < ea && s < eb && a[s] == b[s] {
        s += 1;
    }
    while ea > s && eb > s && a[ea - 1] == b[eb - 1] {
        ea -= 1;
        eb -= 1;
    }
    // Lines as numbers, quicker to compare.
    let mut ids: HashMap<&str, usize> = HashMap::new();
    let (x, y) = (numbered(&mut ids, &a[s..ea]), numbered(&mut ids, &b[s..eb]));
    let mut same = myers(&x, &y, limit).unwrap_or_default();
    same.push((x.len(), y.len()));
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in same {
        let (add, del, at) = (pj - j, pi - i, s + j);
        for k in 0..add {
            kinds[at + k] = if k < del { LineKind::Mod } else { LineKind::Add };
        }
        if del > add {
            deleted.insert(at + add);
        }
        i = pi + 1;
        j = pj + 1;
    }
    LineDiff { kinds, deleted }
}

fn numbered<'a>(ids: &mut HashMap<&'a str, usize>, lines: &'a [String]) -> Vec<usize> {
    lines
        .iter()
        .map(|l| {
            let n = ids.len();
            *ids.entry(l.as_str()).or_insert(n)
        })
        .collect()
}

/// Myers' diff of two lists of numbers: the pairs (i, j) of equal items it keeps, in order; None past limit
/// differences.
fn myers(a: &[usize], b: &[usize], limit: usize) -> Option<Vec<(usize, usize)>> {
    let (n, m) = (a.len() as i64, b.len() as i64);
    if n == 0 || m == 0 {
        return Some(Vec::new());
    }
    let max = n + m;
    let off = max + 1;
    let mut v = vec![0i64; (2 * max + 3) as usize];
    let mut trace: Vec<Vec<i64>> = Vec::new();
    let mut d = 0;
    while d <= max && d <= limit as i64 {
        // The diagonals d can reach from, kept for the way back.
        trace.push(v[(off - d - 1) as usize..(off + d + 2) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let at = |k: i64| v[(off + k) as usize];
            let mut x = if k == -d || (k != d && at(k - 1) < at(k + 1)) { at(k + 1) } else { at(k - 1) + 1 };
            let mut y = x - k;
            while x < n && y >= 0 && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(off + k) as usize] = x;
            if x >= n && y >= m {
                return Some(back(&trace, n, m));
            }
            k += 2;
        }
        d += 1;
    }
    None
}

fn back(trace: &[Vec<i64>], n: i64, m: i64) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as i64).rev() {
        let t = &trace[d as usize];
        let at = |k: i64| t[(k + d + 1) as usize];
        let k = x - y;
        let pk = if k == -d || (k != d && at(k - 1) < at(k + 1)) { k + 1 } else { k - 1 };
        let px = at(pk);
        let py = px - pk;
        while x > px && y > py && x > 0 && y > 0 {
            pairs.push(((x - 1) as usize, (y - 1) as usize));
            x -= 1;
            y -= 1;
        }
        x = px;
        y = py;
    }
    pairs.reverse();
    pairs
}

// --- find

/// A match of a find: its line, its start and end (bytes) in the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub a: usize,
    pub b: usize,
}

/// A character as a regular expression with the i flag (and no u) compares it: its upper case when that is in
/// the BMP, never a non-ASCII character made ASCII; a character past the BMP as it is.
fn canon(c: char) -> char {
    if c as u32 > 0xffff {
        return c;
    }
    let mut up = c.to_uppercase();
    let u = up.next().unwrap_or(c);
    if up.next().is_some() || u as u32 > 0xffff || (c as u32 >= 128 && (u as u32) < 128) {
        return c;
    }
    u
}

/// The matches of a query in the lines. Plain text, ignoring case unless the query has a capital letter, as the
/// preview's find.
pub fn matches(lines: &[String], query: &str) -> Vec<Match> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let exact = query != query.to_lowercase();
    let canon = |c: char| if exact { c } else { canon(c) };
    let q: Vec<char> = query.chars().map(canon).collect();
    for (line, l) in lines.iter().enumerate() {
        let chars: Vec<(usize, char)> = l.char_indices().map(|(i, c)| (i, canon(c))).collect();
        let mut i = 0;
        while i + q.len() <= chars.len() {
            if chars[i..i + q.len()].iter().map(|&(_, c)| c).eq(q.iter().copied()) {
                let b = chars.get(i + q.len()).map_or(l.len(), |&(k, _)| k);
                out.push(Match { line, a: chars[i].0, b });
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

// --- the editor

/// A place in the text: a line (from 0) and an offset in its text, always at the start of a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub at: usize,
}

impl Pos {
    pub fn new(line: usize, at: usize) -> Pos {
        Pos { line, at }
    }
}

/// Where a text put at p ends.
fn end_of(p: Pos, text: &str) -> Pos {
    match text.rfind('\n') {
        None => Pos::new(p.line, p.at + text.len()),
        Some(i) => Pos::new(p.line + text.matches('\n').count(), text.len() - i - 1),
    }
}

/// An edit: at from, removed replaced by inserted.
#[derive(Debug, Clone, PartialEq)]
struct Change {
    from: Pos,
    removed: String,
    inserted: String,
}

/// The cursor and the selection's other end.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Place {
    cur: Pos,
    anchor: Option<Pos>,
}

/// A step of the history: its id, kind ("type", "back", "del", or "" for one that joins nothing), its edits, and
/// the places before and after it.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    id: u64,
    kind: &'static str,
    changes: Vec<Change>,
    before: Place,
    after: Place,
}

/// The lines of a file being edited, its cursor and selection, its history.
#[derive(Debug, Clone)]
pub struct Editor {
    pub lines: Vec<String>,
    pub eol: &'static str,
    pub final_: bool,
    pub bom: bool,
    pub mixed: bool,
    /// The lines as opened, for changed().
    pub original: Vec<String>,
    pub indent: String,
    pub cur: Pos,
    /// The selection's other end, None when none.
    pub anchor: Option<Pos>,
    /// The column ↑ ↓ keep, from the first of them.
    pub goal: Option<usize>,
    undos: Vec<Step>,
    redos: Vec<Step>,
    /// The last step's id.
    id: u64,
    /// The id of the step saved last (0: as opened).
    saved: u64,
    /// The next typing starts a step of its own.
    sealed: bool,
    /// Changes at each edit, for the caches.
    pub version: u64,
    diff: Option<(u64, LineDiff)>,
    changes: Option<Vec<Change>>,
}

impl Editor {
    pub fn new(doc: &Doc) -> Editor {
        let mut ed = Editor {
            lines: Vec::new(),
            eol: "\n",
            final_: false,
            bom: false,
            mixed: false,
            original: Vec::new(),
            indent: String::new(),
            cur: Pos::default(),
            anchor: None,
            goal: None,
            undos: Vec::new(),
            redos: Vec::new(),
            id: 0,
            saved: 0,
            sealed: true,
            version: 0,
            diff: None,
            changes: None,
        };
        ed.reset(doc);
        ed
    }

    /// The text replaced by doc's (a reload): the cursor stays where it can, the history and changes start over.
    pub fn reset(&mut self, doc: &Doc) {
        self.lines = if doc.lines.is_empty() { vec![String::new()] } else { doc.lines.clone() };
        (self.eol, self.final_, self.bom, self.mixed) = (doc.eol, doc.final_, doc.bom, doc.mixed);
        self.original = self.lines.clone();
        self.indent = indent_of(&self.lines);
        self.cur = self.clamp(self.cur);
        self.anchor = None;
        self.goal = None;
        self.undos.clear();
        self.redos.clear();
        self.id = 0;
        self.saved = 0;
        self.sealed = true;
        self.version += 1;
        self.diff = None;
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn serialize(&self) -> String {
        serialize(&self.lines, self.eol, self.final_, self.bom)
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        Pos::new(line, snap(text, p.at.min(text.len())))
    }

    /// The id of the step the text is at: equal to saved when nothing changed since the save.
    fn top(&self) -> u64 {
        self.undos.last().map_or(0, |s| s.id)
    }

    pub fn dirty(&self) -> bool {
        self.top() != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.top();
        self.sealed = true;
    }

    /// The selection's ends in order, or None.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor.filter(|&a| a != self.cur)?;
        Some(if anchor < self.cur { (anchor, self.cur) } else { (self.cur, anchor) })
    }

    pub fn text_of(&self, a: Pos, b: Pos) -> String {
        if a.line == b.line {
            return self.lines[a.line][a.at..b.at].to_string();
        }
        let mut parts = vec![&self.lines[a.line][a.at..]];
        parts.extend(self.lines[a.line + 1..b.line].iter().map(String::as_str));
        parts.push(&self.lines[b.line][..b.at]);
        parts.join("\n")
    }

    pub fn selected(&self) -> String {
        self.selection().map_or(String::new(), |(a, b)| self.text_of(a, b))
    }

    /// The text between a and b replaced, without history: returns where the new text ends.
    fn splice(&mut self, a: Pos, b: Pos, text: &str) -> Pos {
        let head = &self.lines[a.line][..a.at];
        let tail = &self.lines[b.line][b.at..];
        let mut parts: Vec<String> = text.split('\n').map(str::to_string).collect();
        parts[0] = format!("{head}{}", parts[0]);
        parts.last_mut().unwrap().push_str(tail);
        let end = end_of(a, text);
        self.lines.splice(a.line..=b.line, parts);
        self.version += 1;
        end
    }

    /// An edit of the current step (see step()): returns where the new text ends.
    fn replace(&mut self, a: Pos, b: Pos, text: &str) -> Pos {
        let removed = self.text_of(a, b);
        if removed.is_empty() && text.is_empty() {
            return a;
        }
        self.changes.get_or_insert_default().push(Change { from: a, removed, inserted: text.into() });
        self.splice(a, b, text)
    }

    /// f's edits as one step of the history. kind: typed characters ("type") and deletions ("back", "del") of the
    /// same kind join the step before them while the cursor stayed there; a space after a word starts a new one.
    fn step(&mut self, kind: &'static str, f: impl FnOnce(&mut Self)) -> bool {
        let before = Place { cur: self.cur, anchor: self.anchor };
        self.changes = Some(Vec::new());
        f(self);
        let changes = self.changes.take().unwrap_or_default();
        if changes.is_empty() {
            return false;
        }
        self.redos.clear();
        self.goal = None;
        let starts_space = changes[0].inserted.chars().next().is_some_and(crate::unicode::is_space);
        let joinable = self.undos.last().filter(|last| {
            let word = kind == "type"
                && starts_space
                && !last
                    .changes
                    .last()
                    .is_some_and(|c| c.inserted.chars().next_back().is_some_and(crate::unicode::is_space));
            !kind.is_empty() && last.kind == kind && !self.sealed && last.id != self.saved && !word
        });
        let after = Place { cur: self.cur, anchor: None };
        if joinable.is_some() {
            let last = self.undos.last_mut().unwrap();
            last.changes.extend(changes);
            last.after = after;
        } else {
            self.id += 1;
            self.undos.push(Step { id: self.id, kind, changes, before, after });
        }
        self.sealed = kind.is_empty();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undos.pop() else { return false };
        for c in s.changes.iter().rev() {
            self.splice(c.from, end_of(c.from, &c.inserted), &c.removed);
        }
        let before = s.before;
        self.redos.push(s);
        self.restore(before);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redos.pop() else { return false };
        for c in &s.changes {
            self.splice(c.from, end_of(c.from, &c.removed), &c.inserted);
        }
        let after = s.after;
        self.undos.push(s);
        self.restore(after);
        true
    }

    fn restore(&mut self, p: Place) {
        self.cur = self.clamp(p.cur);
        self.anchor = p.anchor.map(|a| self.clamp(a));
        self.goal = None;
        self.sealed = true;
    }

    // --- edits

    /// Text typed or pasted in place of the selection. kind: "type" for typed characters, which join in one step.
    pub fn type_(&mut self, text: &str, kind: &'static str) -> bool {
        self.step(kind, |ed| {
            let (a, b) = ed.selection().unwrap_or((ed.cur, ed.cur));
            ed.cur = ed.replace(a, b, text);
            ed.anchor = None;
        })
    }

    /// Enter: a new line indented as the one the cursor leaves (up to the cursor).
    pub fn newline(&mut self) -> bool {
        let a = self.selection().map_or(self.cur, |s| s.0);
        let line = &self.lines[a.line];
        let lead = line[..a.at.min(first_non_space(line))].to_string();
        self.type_(&format!("\n{lead}"), "")
    }

    /// Backspace (dir < 0) and delete: the selection, else the character before or after the cursor (or the word,
    /// with word), joining two lines at their ends.
    pub fn erase(&mut self, dir: i64, word: bool) -> bool {
        if self.selection().is_some() {
            return self.type_("", "");
        }
        self.anchor = None;
        let p = self.cur;
        let text = &self.lines[p.line];
        let q = if dir < 0 {
            if p.at > 0 {
                Some(Pos::new(p.line, if word { word_start(text, p.at) } else { prev_at(text, p.at) }))
            } else if p.line > 0 {
                Some(Pos::new(p.line - 1, self.lines[p.line - 1].len()))
            } else {
                None
            }
        } else if p.at < text.len() {
            Some(Pos::new(p.line, if word { word_end(text, p.at) } else { next_at(text, p.at) }))
        } else if p.line < self.lines.len() - 1 {
            Some(Pos::new(p.line + 1, 0))
        } else {
            None
        };
        let Some(q) = q else { return false };
        let kind = if word {
            ""
        } else if dir < 0 {
            "back"
        } else {
            "del"
        };
        self.step(kind, |ed| {
            let (a, b) = if dir < 0 { (q, p) } else { (p, q) };
            ed.cur = ed.replace(a, b, "");
        })
    }

    /// Tab (dir > 0) and shift+tab: without a selection over lines, tab puts the indentation at the cursor (spaces
    /// to the next stop); otherwise each line of the selection (or the cursor's) gets one more level, or one less.
    pub fn indent_lines(&mut self, dir: i64) -> bool {
        let s = self.selection();
        if dir > 0 && s.is_none_or(|(a, b)| a.line == b.line) {
            let unit = if self.indent == "\t" {
                "\t".to_string()
            } else {
                let n = self.indent.len();
                " ".repeat(n - col_of(&self.lines[self.cur.line], self.cur.at) % n)
            };
            return self.type_(&unit, "");
        }
        let (a, b) = s.unwrap_or((self.cur, self.cur));
        // A selection ending at the start of a line leaves that line out.
        let last = if b.line > a.line && b.at == 0 { b.line - 1 } else { b.line };
        let mut shift: HashMap<usize, i64> = HashMap::new();
        let done = self.step("", |ed| {
            for i in a.line..=last {
                let l = &ed.lines[i];
                if dir > 0 {
                    if l.is_empty() {
                        continue;
                    }
                    let indent = ed.indent.clone();
                    ed.replace(Pos::new(i, 0), Pos::new(i, 0), &indent);
                    shift.insert(i, indent.len() as i64);
                } else {
                    let n = if l.starts_with('\t') {
                        1
                    } else {
                        let lead = l.bytes().position(|b| b != b' ').unwrap_or(l.len());
                        lead.min(if ed.indent == "\t" { TAB } else { ed.indent.len() })
                    };
                    if n == 0 {
                        continue;
                    }
                    ed.replace(Pos::new(i, 0), Pos::new(i, n), "");
                    shift.insert(i, -(n as i64));
                }
            }
            for p in std::iter::once(&mut ed.cur).chain(ed.anchor.as_mut()) {
                p.at = (p.at as i64 + shift.get(&p.line).copied().unwrap_or(0)).max(0) as usize;
            }
        });
        if done {
            let after = Place { cur: self.cur, anchor: self.anchor };
            self.undos.last_mut().unwrap().after = after;
        }
        done
    }

    // --- moves

    /// The cursor to p; extend: the selection grows to it (shift), else it goes.
    pub fn place(&mut self, p: Pos, extend: bool) {
        let p = self.clamp(p);
        if extend && self.anchor.is_none() {
            self.anchor = Some(self.cur);
        }
        if !extend {
            self.anchor = None;
        }
        self.cur = p;
        self.sealed = true;
        self.goal = None;
    }

    /// A move by name: left, right, up, down, home, end, pageup, pagedown (by page lines), wordleft, wordright, top,
    /// bottom. Without extend, ← → first fold a selection to its start or end.
    pub fn move_(&mut self, name: &str, extend: bool, page: usize) {
        let p = self.cur;
        let text = self.lines[p.line].clone();
        let s = self.selection();
        let last = Pos::new(self.lines.len() - 1, self.lines[self.lines.len() - 1].len());
        let to = match name {
            "left" => match s {
                Some((a, _)) if !extend => a,
                _ if p.at > 0 => Pos::new(p.line, prev_at(&text, p.at)),
                _ if p.line > 0 => Pos::new(p.line - 1, self.lines[p.line - 1].len()),
                _ => p,
            },
            "right" => match s {
                Some((_, b)) if !extend => b,
                _ if p.at < text.len() => Pos::new(p.line, next_at(&text, p.at)),
                _ if p.line < self.lines.len() - 1 => Pos::new(p.line + 1, 0),
                _ => p,
            },
            "wordleft" if p.at > 0 => Pos::new(p.line, word_start(&text, p.at)),
            "wordleft" if p.line > 0 => Pos::new(p.line - 1, self.lines[p.line - 1].len()),
            "wordleft" => p,
            "wordright" if p.at < text.len() => Pos::new(p.line, word_end(&text, p.at)),
            "wordright" if p.line < self.lines.len() - 1 => Pos::new(p.line + 1, 0),
            "wordright" => p,
            // Home: the first character that is not a space, then the start of the line.
            "home" => {
                let first = first_non_space(&text);
                Pos::new(p.line, if p.at == first { 0 } else { first })
            }
            "end" => Pos::new(p.line, text.len()),
            "top" => Pos::new(0, 0),
            "bottom" => last,
            "up" | "down" | "pageup" | "pagedown" => {
                let n = match name {
                    "up" => -1,
                    "down" => 1,
                    "pageup" => -(page as i64),
                    _ => page as i64,
                };
                let goal = *self.goal.get_or_insert_with(|| col_of(&text, p.at));
                let line = p.line as i64 + n;
                let to = if line < 0 {
                    Pos::new(0, 0)
                } else if line as usize >= self.lines.len() {
                    last
                } else {
                    Pos::new(line as usize, at_col(&self.lines[line as usize], goal))
                };
                self.place(to, extend);
                self.goal = Some(goal);
                return;
            }
            _ => return,
        };
        self.place(to, extend);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::new(0, 0));
        self.cur = Pos::new(self.lines.len() - 1, self.lines[self.lines.len() - 1].len());
        self.sealed = true;
    }

    /// The word (or run of spaces, or character) under p selected: a double-click.
    pub fn select_word(&mut self, p: Pos) {
        let p = self.clamp(p);
        let text = &self.lines[p.line];
        if p.at >= text.len() {
            return self.place(p, false);
        }
        let k = kind(char_at(text, p.at));
        let (mut a, mut b) = (p.at, next_at(text, p.at));
        if k != 2 {
            while a > 0 && kind(char_at(text, prev_at(text, a))) == k {
                a = prev_at(text, a);
            }
            while b < text.len() && kind(char_at(text, b)) == k {
                b = next_at(text, b);
            }
        }
        self.anchor = Some(Pos::new(p.line, a));
        self.cur = Pos::new(p.line, b);
        self.sealed = true;
    }

    /// The whole line under p, with its line break: a triple click.
    pub fn select_line(&mut self, p: Pos) {
        let p = self.clamp(p);
        self.anchor = Some(Pos::new(p.line, 0));
        self.cur = if p.line < self.lines.len() - 1 {
            Pos::new(p.line + 1, 0)
        } else {
            Pos::new(p.line, self.lines[p.line].len())
        };
        self.sealed = true;
    }

    /// Line n (from 1).
    pub fn goto(&mut self, line: usize) {
        self.place(Pos::new(line.saturating_sub(1), 0), false);
    }

    // --- find and replace

    /// The next match of query after the cursor (dir 1), or the one before it (-1), going round, selected. from:
    /// the cursor's own place counts (a find as the query is typed).
    pub fn find(&mut self, query: &str, dir: i64, from: bool) -> Option<Match> {
        let list = matches(&self.lines, query);
        let s = self.selection().unwrap_or((self.cur, self.cur));
        let start = |x: &Match| Pos::new(x.line, x.a);
        let m = if dir > 0 {
            let after = if from { s.0 } else { s.1 };
            list.iter().find(|x| start(x) >= after).or(list.first())
        } else {
            list.iter().rev().find(|x| start(x) < s.0).or(list.last())
        }
        .copied()?;
        self.anchor = Some(Pos::new(m.line, m.a));
        self.cur = Pos::new(m.line, m.b);
        self.sealed = true;
        self.goal = None;
        Some(m)
    }

    /// The selected match replaced by with, then the next one selected; without a match selected, the next one
    /// first. Returns whether one was replaced.
    pub fn replace_one(&mut self, query: &str, with: &str) -> bool {
        let on = self.selection().is_some_and(|(a, b)| {
            a.line == b.line
                && matches(std::slice::from_ref(&self.lines[a.line]), query).iter().any(|m| m.a == a.at && m.b == b.at)
        });
        if !on {
            self.find(query, 1, true);
            return false;
        }
        self.type_(with, "");
        self.find(query, 1, true);
        true
    }

    /// Every match replaced, as one step: returns how many.
    pub fn replace_all(&mut self, query: &str, with: &str) -> usize {
        let list = matches(&self.lines, query);
        if list.is_empty() {
            return 0;
        }
        let cur = self.cur;
        // From the last, so that the positions of the others still hold.
        self.step("", |ed| {
            for m in list.iter().rev() {
                ed.replace(Pos::new(m.line, m.a), Pos::new(m.line, m.b), with);
            }
        });
        self.place(cur, false);
        list.len()
    }

    /// The lines changed since the file was opened (line_diff), computed once per version.
    pub fn changed(&mut self) -> &LineDiff {
        if self.diff.as_ref().is_none_or(|(v, _)| *v != self.version) {
            self.diff = Some((self.version, line_diff(&self.original, &self.lines, 1000)));
        }
        &self.diff.as_ref().unwrap().1
    }
}

/// `line.search(/\S|$/)`: where the first character that is not a space starts, or the end.
fn first_non_space(line: &str) -> usize {
    line.char_indices().find(|&(_, c)| !crate::unicode::is_space(c)).map_or(line.len(), |(i, _)| i)
}
