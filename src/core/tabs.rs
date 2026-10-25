//! The preview's tabs, as VS Code's editors: a list of tabs and the index of the one shown. A file opened by a click
//! or by following the selection replaces the temporary tab (kept false, shown in italics); a double-click, Enter, an
//! edit or t keeps it. Each function returns new tabs, leaving its input alone; the other fields of a tab (its
//! scroll, view, marks: `extra`) are the preview's.

use super::paths;

/// At most this many tabs: past it, the one furthest from the tab opened goes.
pub const MAX: usize = 50;
/// The columns of the arrow (‹ ›) on a side of the strip with hidden tabs.
pub const MORE: i64 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Tab<T> {
    pub file: String,
    pub kept: bool,
    pub extra: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tabs<T> {
    pub list: Vec<Tab<T>>,
    pub current: usize,
}

/// What open gives: the tabs, and fresh: the tab is new or replaced, its view to start over.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened<T> {
    pub tabs: Tabs<T>,
    pub fresh: bool,
}

/// A file shown: in its tab when one has it (kept when asked), else in the temporary tab, else in a new tab after the
/// current one. busy(tab): a tab never dropped past MAX (unsaved edits), the list growing past it when every other
/// one is.
pub fn open<T: Clone + Default>(tabs: &Tabs<T>, file: &str, keep: bool, busy: impl Fn(&Tab<T>) -> bool) -> Opened<T> {
    let list = &tabs.list;
    if let Some(at) = list.iter().position(|t| t.file == file) {
        let list = list
            .iter()
            .enumerate()
            .map(|(i, t)| if i == at && keep && !t.kept { Tab { kept: true, ..t.clone() } } else { t.clone() })
            .collect();
        return Opened { tabs: Tabs { list, current: at }, fresh: false };
    }
    let tab = Tab { file: file.to_string(), kept: keep, extra: T::default() };
    if let Some(temp) = list.iter().position(|t| !t.kept) {
        let mut list = list.clone();
        list[temp] = tab;
        return Opened { tabs: Tabs { list, current: temp }, fresh: true };
    }
    let i = list.len().min(tabs.current + 1);
    let mut next = list.clone();
    next.insert(i, tab);
    let mut shown = i;
    if next.len() > MAX {
        let mut far: Vec<usize> = (0..next.len()).filter(|&k| k != shown && !busy(&next[k])).collect();
        // The furthest first, the later one of two as far.
        far.sort_by(|&a, &b| (b.abs_diff(shown)).cmp(&a.abs_diff(shown)).then(b.cmp(&a)));
        if let Some(&drop) = far.first() {
            next.remove(drop);
            if drop < shown {
                shown -= 1;
            }
        }
    }
    Opened { tabs: Tabs { list: next, current: shown }, fresh: true }
}

/// The tab i closed: the one on its right shown in its place, else the one on its left. The last tab stays (the
/// preview closes instead).
pub fn close<T: Clone>(tabs: &Tabs<T>, i: usize) -> Tabs<T> {
    if tabs.list.len() <= 1 || i >= tabs.list.len() {
        return tabs.clone();
    }
    let mut list = tabs.list.clone();
    list.remove(i);
    let current = if i < tabs.current { tabs.current - 1 } else { tabs.current.min(list.len() - 1) };
    Tabs { list, current }
}

/// The next tab (dir 1) or the previous one (-1), going round.
pub fn step<T: Clone>(tabs: &Tabs<T>, dir: i64) -> Tabs<T> {
    let n = tabs.list.len() as i64;
    let current = if n > 0 { (tabs.current as i64 + dir + n).rem_euclid(n) as usize } else { 0 };
    Tabs { list: tabs.list.clone(), current }
}

/// The tab i kept: the next file opened by a click no longer replaces it.
pub fn keep<T: Clone>(tabs: &Tabs<T>, i: usize) -> Tabs<T> {
    let mut list = tabs.list.clone();
    if let Some(t) = list.get_mut(i) {
        t.kept = true;
    }
    Tabs { list, current: tabs.current }
}

/// The names on the tabs: the file name, with its folder when another tab has the same name (src/index.js,
/// test/index.js).
pub fn labels(files: &[String]) -> Vec<String> {
    let names: Vec<String> = files.iter().map(|f| paths::basename(f, None)).collect();
    files
        .iter()
        .zip(&names)
        .map(|(f, name)| {
            if names.iter().filter(|n| *n == name).count() > 1 {
                format!("{}/{name}", paths::basename(&paths::dirname(f), None))
            } else {
                name.clone()
            }
        })
        .collect()
}

/// The tabs that fit in room columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    /// The first and last tabs shown (last -1 for no tab).
    pub first: i64,
    pub last: i64,
    /// The columns that the current tab, alone and too wide, must lose.
    pub cut: i64,
}

/// The tabs that fit in room columns, the current one always among them: widths, the columns of each tab; first, the
/// first tab shown before, so that the strip moves only when the current tab would leave it. A side with hidden tabs
/// takes MORE columns for its arrow.
pub fn strip(widths: &[i64], current: i64, room: i64, first: i64) -> Strip {
    let n = widths.len() as i64;
    if n == 0 {
        return Strip { first: 0, last: -1, cut: 0 };
    }
    let need = |a: i64, b: i64| -> i64 {
        let sum: i64 = (a.max(0)..=b.min(n - 1)).map(|k| widths[k as usize]).sum();
        sum + if a > 0 { MORE } else { 0 } + if b < n - 1 { MORE } else { 0 }
    };
    let mut first = first.min(current).max(0);
    let mut last = current;
    while first < current && need(first, last) > room {
        first += 1;
    }
    while last + 1 < n && need(first, last + 1) <= room {
        last += 1;
    }
    while first > 0 && need(first - 1, last) <= room {
        first -= 1;
    }
    Strip { first, last, cut: (need(first, last) - room).max(0) }
}
