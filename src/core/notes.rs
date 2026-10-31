//! Review notes of a tab: lines of a file (or a whole file) with a remark, taken in the preview and the Changes pane,
//! then typed together into the agent's prompt next to the explorer, without submitting it.
//! Stored in notes-<tab id>.json in the state folder, so that every pane of the tab sees the same list. Each note keeps
//! the text of its lines (anchor), so that it follows them when lines are added or removed above, and is marked stale
//! once they are gone (current). A note stays the JSON object written, with any field it holds.

use super::actions::relative_to;
use super::exec::{real_path, tab_file, write_atomic};
use crate::json::{self, Value};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

/// The kinds a note can have, chosen with Tab while it is typed; "" for a plain remark.
pub const KINDS: [&str; 5] = ["", "fix", "question", "nit", "verify"];

fn file_of(tab: &str) -> Option<String> {
    tab_file("notes", tab)
}

/// The notes of a tab: [{ file, from, to, kind, text, t, anchor, stale }], from and to 1-based lines, 0 for the whole
/// file; anchor, the text of those lines when the note was taken (none past MAX_ANCHOR lines); stale, they are no
/// longer in the file.
pub fn read(tab: &str) -> Vec<Value> {
    let parsed = file_of(tab).and_then(|f| super::exec::read_text(f).ok()).and_then(|t| json::parse(&t).ok());
    let Some(Value::Arr(list)) = parsed else { return Vec::new() };
    let is_str = |n: &Value, k: &str| matches!(n.get(k), Some(Value::Str(_)));
    list.into_iter().filter(|n| matches!(n, Value::Obj(_)) && is_str(n, "file") && is_str(n, "text")).collect()
}

/// Written to a temporary file then renamed, so that another pane never reads it half-written. An empty list removes
/// the file.
pub fn write(tab: &str, list: &[Value]) -> bool {
    let Some(file) = file_of(tab) else { return false };
    if list.is_empty() {
        let _ = std::fs::remove_file(&file);
        return true;
    }
    write_atomic(&file, json::stringify(&Value::Arr(list.to_vec())))
}

/// A note's text keeps no control character, which could submit the prompt once typed.
pub fn clean(text: &str) -> String {
    let spaced: String =
        text.chars().map(|c| if matches!(c, '\0'..='\x1f' | '\x7f'..='\u{9f}') { ' ' } else { c }).collect();
    spaced.trim_matches(crate::unicode::is_space).to_string()
}

const MAX_ANCHOR: u64 = 200; // lines kept as a note's anchor
const MAX_SIZE: u64 = 4 << 20; // a larger file is not read to place its notes

/// The lines of a file, or why they are not there.
#[derive(Debug, Clone, PartialEq)]
pub enum Lines {
    /// The file is gone.
    Gone,
    /// Not known (too large, unreadable for now).
    Unknown,
    Lines(Vec<String>),
}

/// The lines of a file.
pub fn lines_of(file: &str) -> Lines {
    let read = || -> std::io::Result<Lines> {
        if std::fs::metadata(file)?.len() > MAX_SIZE {
            return Ok(Lines::Unknown);
        }
        let text = String::from_utf8_lossy(&std::fs::read(file)?).into_owned();
        // The '' after a last line break is not a line: a note on it would be anchored on any blank line.
        let mut lines: Vec<String> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        Ok(Lines::Lines(lines))
    };
    read().unwrap_or_else(|e| match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => Lines::Gone,
        _ => Lines::Unknown,
    })
}

/// A note placed in the file's lines now: where its anchor is, the nearest place to where it was when the text is in
/// several; stale when nowhere. None when nothing changes (the same note), and when the lines are not known.
pub fn locate(n: &Value, lines: &Lines) -> Option<Value> {
    let anchor = match n.get("anchor") {
        Some(Value::Arr(a)) if !a.is_empty() => a,
        _ => return None,
    };
    if !n.get("from").is_some_and(Value::truthy) || *lines == Lines::Unknown {
        return None;
    }
    let len = anchor.len();
    let was = json::num(n.get("from")) - 1.0;
    let lines = match lines {
        Lines::Lines(l) => Some(l),
        _ => None,
    };
    // The anchor is at line index i (None off the list).
    let at = |lines: &[String], i: f64| {
        i >= 0.0
            && i.fract() == 0.0
            && anchor
                .iter()
                .enumerate()
                .all(|(k, l)| lines.get(i as usize + k).is_some_and(|line| l.as_str() == Some(line.as_str())))
    };
    let mut best: f64 = -1.0;
    if let Some(lines) = lines {
        if at(lines, was) {
            best = was;
        } else {
            for i in 0..(lines.len() + 1).saturating_sub(len) {
                let i = i as f64;
                if at(lines, i) && (best < 0.0 || (i - was).abs() < (best - was).abs()) {
                    best = i;
                }
            }
        }
    }
    let stale = n.get("stale").is_some_and(Value::truthy);
    if best < 0.0 {
        if stale {
            return None;
        }
        let mut moved = n.clone();
        moved.set("stale", Value::Bool(true));
        return Some(moved);
    }
    if best == was && !stale {
        return None;
    }
    let mut moved = n.clone();
    moved.remove("stale");
    moved.set("from", Value::Num(best + 1.0));
    moved.set("to", Value::Num(best + len as f64));
    Some(moved)
}

/// The notes of a tab placed again in their files as they are now (locate), saved when one moved, for every pane.
/// only: the real path of the one file to read (a pane showing it, which saw it change), the other notes as saved.
pub fn current(tab: &str, only: Option<&str>) -> Vec<Value> {
    let list = read(tab);
    let mut files: HashMap<String, Lines> = HashMap::new();
    let mut placed = Vec::with_capacity(list.len());
    let mut moved = Vec::new();
    for n in &list {
        let file = n.get("file").and_then(Value::as_str).unwrap_or("");
        let skip = !n.get("from").is_some_and(Value::truthy)
            || !n.get("anchor").is_some_and(Value::truthy)
            || only.is_some_and(|o| !o.is_empty() && o != file);
        let lines = if skip { None } else { Some(files.entry(file.to_string()).or_insert_with(|| lines_of(file))) };
        match lines.and_then(|lines| locate(n, lines)) {
            Some(m) => {
                moved.push(m.clone());
                placed.push(m);
            }
            None => placed.push(n.clone()),
        }
    }
    if moved.is_empty() { placed } else { update(tab, &moved) }
}

/// The same note in two readings of the list: taken at the same time, on the same file, with the same text.
fn same(a: &Value, b: &Value) -> bool {
    ["t", "file", "text"].iter().all(|k| a.get(k) == b.get(k))
}

/// Writes the notes moved into the list read again just before, so that a note taken, sent or removed meanwhile by
/// another pane is kept; read back after, to write again if another pane wrote over it meanwhile. Returns the list.
fn update(tab: &str, moved: &[Value]) -> Vec<Value> {
    let mut list = read(tab);
    for _ in 0..3 {
        list = list.into_iter().map(|n| moved.iter().find(|m| same(m, &n)).cloned().unwrap_or(n)).collect();
        write(tab, &list);
        let back = read(tab);
        let kept = |m: &Value| {
            !back.iter().any(|n| same(m, n)) || back.iter().any(|n| json::stringify(n) == json::stringify(m))
        };
        if moved.iter().all(kept) {
            return back;
        }
        list = back;
    }
    list
}

/// A note to take: its file, its lines (0 for the whole file), its kind and text; lines: the text from to to of the
/// version shown, as a diff of the index or of a commit shows it (Some(None) when it is not known, for no anchor; None
/// to read them from the file on disk); stale: those lines are no longer on disk.
#[derive(Debug, Clone, Default)]
pub struct New {
    pub file: String,
    pub from: u64,
    pub to: Option<u64>,
    pub kind: String,
    pub text: String,
    pub lines: Option<Option<Vec<String>>>,
    pub stale: bool,
}

/// Adds a note, its file named by its real path (real_path), so that the preview (from the explorer's folder, maybe
/// through a symbolic link such as /home → /var/home) and the diff (from the repository's real folder) agree.
pub fn add(tab: &str, note: New) -> String {
    add_at(tab, note, crate::core::clock::now_ms())
}

/// `add`, the note taken at t.
pub fn add_at(tab: &str, note: New, t: f64) -> String {
    let text = clean(&note.text);
    if text.is_empty() || note.file.is_empty() {
        return "Empty note".into();
    }
    let (mut from, mut to) = (note.from, note.to.unwrap_or(note.from));
    if from > to {
        (from, to) = (to, from);
    }
    let mut list = read(tab);
    let real = real_path(&note.file);
    let mut anchor = None;
    if from != 0 && to - from < MAX_ANCHOR {
        anchor = match note.lines {
            Some(shown) => shown.filter(|s| s.len() as u64 == to - from + 1),
            None => match lines_of(&real) {
                Lines::Lines(lines) if to as usize <= lines.len() => {
                    Some(lines[from as usize - 1..to as usize].to_vec())
                }
                _ => None,
            },
        };
    }
    let kind = if KINDS.contains(&note.kind.as_str()) { note.kind } else { String::new() };
    let mut fields = vec![
        ("file".to_string(), Value::Str(real)),
        ("from".to_string(), Value::Num(from as f64)),
        ("to".to_string(), Value::Num(to as f64)),
        ("kind".to_string(), Value::Str(kind)),
        ("text".to_string(), Value::Str(text)),
        ("t".to_string(), Value::Num(t)),
    ];
    if let Some(anchor) = anchor {
        fields.push(("anchor".to_string(), Value::Arr(anchor.into_iter().map(Value::Str).collect())));
        if note.stale {
            fields.push(("stale".to_string(), Value::Bool(true)));
        }
    }
    list.push(Value::Obj(fields));
    if write(tab, &list) { format!("Note added ({})", list.len()) } else { "Cannot save the note".into() }
}

/// Removes the last note taken.
pub fn undo(tab: &str) -> String {
    let mut list = read(tab);
    let Some(last) = list.pop() else { return "No note".into() };
    if write(tab, &list) {
        format!("Note removed: {}", last.get("text").and_then(Value::as_str).unwrap_or(""))
    } else {
        "Cannot remove the note".into()
    }
}

/// Where a note points, relative to a folder: src/app.js:12, src/app.js:12-14, or the file alone. The folder is
/// resolved as the note's file is, so that one reached through a symbolic link still gives a relative path (resolved:
/// it already is).
pub fn where_(n: &Value, dir: Option<&str>, resolved: bool) -> String {
    let dir = dir.filter(|d| !d.is_empty());
    let real = dir.filter(|_| !resolved).map(real_path);
    let file = n.get("file").and_then(Value::as_str).unwrap_or("");
    let reference = relative_to(real.as_deref().or(dir), file);
    let from = n.get("from");
    if !from.is_some_and(Value::truthy) {
        return reference;
    }
    let field = |v: Option<&Value>| v.map_or("undefined".into(), json::text);
    let range =
        if json::num(n.get("to")) > json::num(from) { format!("-{}", field(n.get("to"))) } else { String::new() };
    format!("{reference}:{}{range}", field(from))
}

/// Each note as "src/a.js:12-14 (fix): rename it", in file and line order; "(stale)" when its lines are gone.
/// The folder is resolved once for all of them.
pub fn items(list: &[Value], dir: Option<&str>) -> Vec<String> {
    let at = dir.filter(|d| !d.is_empty()).map(real_path);
    let mut sorted: Vec<&Value> = list.iter().collect();
    let text = |n: &Value, k: &str| n.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    sorted.sort_by(|a, b| {
        let num = |n: &Value, k: &str| json::num(n.get(k));
        crate::unicode::natural_cmp(&text(a, "file"), &text(b, "file"))
            .then_with(|| num(a, "from").partial_cmp(&num(b, "from")).unwrap_or(Ordering::Equal))
            .then_with(|| num(a, "t").partial_cmp(&num(b, "t")).unwrap_or(Ordering::Equal))
    });
    sorted
        .into_iter()
        .map(|n| {
            let mut tags = Vec::new();
            if let Some(kind) = n.get("kind").filter(|k| k.truthy()) {
                tags.push(json::text(kind));
            }
            if n.get("stale").is_some_and(Value::truthy) {
                tags.push("stale".into());
            }
            let tags = if tags.is_empty() { String::new() } else { format!(" ({})", tags.join(", ")) };
            format!("{}{tags}: {}", clean(&where_(n, at.as_deref(), true)), clean(&text(n, "text")))
        })
        .collect()
}

/// One line for the agent: "Review notes: src/a.js:12-14 (fix): rename it; …", after the reviewPrefix setting.
pub fn format(list: &[Value], dir: Option<&str>, prefix: &str) -> String {
    let prefix = clean(prefix);
    let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix} ") };
    format!("{prefix}Review notes: {} ", items(list, dir).join("; "))
}

/// The tab's notes to the clipboard, as a list under "Review notes:".
pub fn copy(tab: &str, dir: Option<&str>) -> String {
    let list = current(tab, None);
    if list.is_empty() {
        return "No note to copy".into();
    }
    let lines: String = items(&list, dir).iter().map(|i| format!("- {i}\n")).collect();
    let n = list.len();
    super::actions::copy(
        &format!("Review notes:\n{lines}"),
        &format!("{n} note{} copied", if n > 1 { "s" } else { "" }),
    )
}

/// Types every note of the tab into the terminal next to the explorer, then clears them.
pub fn send(tab: &str, pane: &str) -> String {
    let list = current(tab, None);
    if list.is_empty() {
        return "No note to send".into();
    }
    let settings = super::exec::read_settings();
    let prefix = settings.get("reviewPrefix").filter(|p| p.truthy()).map_or(String::new(), json::text);
    let text = |dir: Option<&str>| format(&list, dir, &prefix);
    if let Some(err) = super::actions::type_text(super::actions::Line::Of(&text), pane) {
        return err;
    }
    // Notes taken while sending are kept.
    let sent: Vec<Option<&Value>> = list.iter().map(|n| n.get("t")).collect();
    let kept: Vec<Value> = read(tab).into_iter().filter(|n| !sent.iter().any(|t| same_value(*t, n.get("t")))).collect();
    write(tab, &kept);
    let n = list.len();
    format!("{n} note{} sent", if n > 1 { "s" } else { "" })
}

/// Equal values, NaN equal to itself; an object read again never the same.
fn same_value(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::Num(x)), Some(Value::Num(y))) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

/// Watches the tab's notes (another pane took or sent one); None without a state folder.
pub fn watch(tab: &str) -> Option<super::watch::Watch> {
    file_of(tab).map(|f| super::watch::Watch::new(&f, Duration::from_millis(500)))
}
