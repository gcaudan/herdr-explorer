//! The explorer's tree, apart from the terminal (src/explorer/tree.js): the folders read (cache), the git status of the
//! root, the rows of a view (folders expanded, filter, changed files only) and the order of the entries. `Tree::new`
//! makes the tree of a root; the explorer changes `root` (Set as root) and `settings` as they change.

use crate::core::{exec, js, paths};
use crate::unicode::locale_compare;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Entries read at most while filtering.
pub const FILTER_BUDGET: i64 = 20000;

/// The strength of a change, the strongest one marking the folders above it.
pub fn rank(c: char) -> u8 {
    match c {
        'U' => 1,
        'A' | 'R' => 2,
        'M' => 3,
        'D' => 4,
        'C' => 5,
        _ => 0,
    }
}

/// An entry of a folder, and a row of the tree once placed: its depth, the filter's match in its name (byte offsets),
/// whether it is unfolded. A compact folder row is its chain's last folder: `from` the first one's rel, `chain` the
/// names before it ("src/main/"). `gone`: a deleted entry, no longer on disk, shown among the changed files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub rel: String,
    pub link: bool,
    pub gone: bool,
    pub from: Option<String>,
    pub chain: Option<String>,
    pub depth: usize,
    pub hit: Option<(usize, usize)>,
    pub open: bool,
}

/// A Map of the JS: lookups by key, iteration in the order keys were first set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ordered<V> {
    keys: Vec<String>,
    map: HashMap<String, V>,
}

impl<V: Clone> Ordered<V> {
    pub fn get(&self, k: &str) -> Option<&V> {
        self.map.get(k)
    }

    pub fn has(&self, k: &str) -> bool {
        self.map.contains_key(k)
    }

    pub fn set(&mut self, k: &str, v: V) {
        if self.map.insert(k.to_string(), v).is_none() {
            self.keys.push(k.to_string());
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.keys.iter().map(|k| (k.as_str(), &self.map[k]))
    }
}

impl Ordered<()> {
    pub fn add(&mut self, k: &str) {
        self.set(k, ());
    }
}

/// The git status of the root's repository: its top folder (None outside one), the root's path in it, the changed
/// files, each folder with the strongest change it holds, the untracked folders, the ignored entries, and the
/// deleted files no longer on disk. A root in no repository has instead the repositories of its folders (repos,
/// their names), their statuses merged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Git {
    pub top: Option<String>,
    pub prefix: String,
    pub repos: Vec<String>,
    pub files: Ordered<char>,
    pub dirs: Ordered<char>,
    pub untracked: Ordered<()>,
    pub ignored: Ordered<()>,
    pub gone: Vec<String>,
}

impl Git {
    /// Whether the tree has a git status: the root in a repository, or repositories in its folders.
    pub fn on(&self) -> bool {
        self.top.is_some() || !self.repos.is_empty()
    }

    /// The repository holding rel, as the folder of the root it is in: "" for the root's own, None outside any.
    pub fn repo_of(&self, rel: &str) -> Option<&str> {
        if self.top.is_some() {
            return Some("");
        }
        let name = rel.split('/').next().unwrap_or("");
        self.repos.iter().find(|r| *r == name).map(String::as_str)
    }
}

/// What git status says, as parse_status reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub files: Ordered<char>,
    pub dirs: Ordered<char>,
    pub untracked: Ordered<()>,
    pub ignored: Ordered<()>,
}

/// The parent of a relative path: "" for a name of the root, None for the root.
pub fn parent_of(rel: &str) -> Option<&str> {
    if rel.is_empty() { None } else { Some(rel.rfind('/').map_or("", |i| &rel[..i])) }
}

/// Whether a row shows the entry rel: its own, or a folder of its compact chain.
pub fn holds(row: &Entry, rel: &str) -> bool {
    row.rel == rel
        || row.from.as_deref().is_some_and(|from| {
            !from.is_empty()
                && (rel == from || rel.starts_with(&format!("{from}/")))
                && row.rel.starts_with(&format!("{rel}/"))
        })
}

/// Names in the tree's order, numbers in their order (localeCompare, numeric, base sensitivity).
pub fn name_order(a: &str, b: &str) -> Ordering {
    locale_compare(a, b, true)
}

/// Folders first, then names, numbers in their order.
pub fn entry_order(a: &Entry, b: &Entry) -> Ordering {
    b.dir.cmp(&a.dir).then_with(|| name_order(&a.name, &b.name))
}

/// The change of a status line's two codes.
pub fn code(x: char, y: char) -> char {
    if x == '?' {
        return 'U';
    }
    if x == '!' {
        return '!';
    }
    if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        return 'C';
    }
    if x == 'D' || y == 'D' {
        return 'D';
    }
    if x == 'R' || y == 'R' {
        return 'R';
    }
    if x == 'A' || x == 'C' {
        return 'A';
    }
    'M'
}

/// The output of git status --porcelain=v1 -z, its paths taken relative to the folder prefix of the repository: the
/// changed files, each folder with the strongest change it holds (rank), the untracked folders and the ignored
/// entries.
pub fn parse_status(status: &str, prefix: &str) -> Status {
    let mut git = Status::default();
    parse_into(&mut git, status, prefix, "");
    git
}

/// parse_status into git, its paths put under the folder base ("" for none): the status of a repository in a folder
/// of the root, the folders above its changes marked with those of the other repositories.
fn parse_into(git: &mut Status, status: &str, prefix: &str, base: &str) {
    let parts: Vec<&str> = status.split('\0').collect();
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        i += 1;
        if p.encode_utf16().count() < 4 {
            continue;
        }
        let units: Vec<u16> = p.encode_utf16().take(2).collect();
        let unit = |u: u16| char::from_u32(u as u32).unwrap_or('\u{fffd}');
        let (x, y) = (unit(units[0]), unit(units[1]));
        let c = code(x, y);
        // The original path follows (renamed in the index or the tree).
        if "RC".contains(x) || "RC".contains(y) {
            i += 1;
        }
        // From the 4th UTF-16 unit, as slice(3); a cut inside a surrogate pair skips the line.
        let Some(mut rel) = unit_at(p, 3).map(|at| &p[at..]) else { continue };
        if !prefix.is_empty() {
            let Some(r) = rel.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')) else { continue };
            rel = r;
        }
        let is_dir = rel.ends_with('/');
        let rel = if is_dir { &rel[..rel.len() - 1] } else { rel };
        let full = if base.is_empty() { rel.to_string() } else { format!("{base}/{rel}") };
        let rel = full.as_str();
        if c == '!' {
            git.ignored.add(rel);
            continue;
        }
        if is_dir {
            git.untracked.add(rel);
        } else {
            git.files.set(rel, c);
        }
        let mut a = parent_of(rel);
        while let Some(dir) = a {
            if git.dirs.get(dir).is_none_or(|&d| rank(c) > rank(d)) {
                git.dirs.set(dir, c);
            }
            a = parent_of(dir);
        }
    }
}

/// Whether a comes before b in the tree; a_dir and b_dir: whether each is a folder.
pub fn tree_before(a: &str, a_dir: bool, b: &str, b_dir: bool) -> bool {
    let (x, y): (Vec<&str>, Vec<&str>) = (a.split('/').collect(), b.split('/').collect());
    for i in 0..x.len().min(y.len()) {
        if x[i] == y[i] {
            continue;
        }
        let xd = i < x.len() - 1 || a_dir;
        let yd = i < y.len() - 1 || b_dir;
        if xd != yd {
            return xd;
        }
        return name_order(x[i], y[i]) == Ordering::Less;
    }
    x.len() < y.len()
}

/// What the tree shows, read at each call: dot files, git-ignored entries, compact folders.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Settings {
    pub hidden: bool,
    pub ignored: bool,
    pub compact: bool,
}

/// What the rows of compact chains are made of.
#[derive(Clone, Copy)]
enum List {
    Visible,
    Changed,
}

/// The tree of a folder.
pub struct Tree {
    pub root: String,
    pub settings: Settings,
    pub git: Git,
    /// Relative folder → its sorted entries.
    pub cache: HashMap<String, Vec<Entry>>,
    /// After a change on disk (stale), a folder read is read again only when its modification time moved: the
    /// compact folders look into every folder shown, collapsed ones included, at each build of the rows. A time too
    /// close to the reading (within the 2 s of the coarsest file systems) is not trusted (None).
    stamps: HashMap<String, Option<f64>>,
    /// Folders read, to compare with their modification time before reuse.
    check: HashSet<String>,
}

/// `name.toLowerCase().indexOf(query)`, as byte offsets in name: the JS took the UTF-16 offsets of the lowered name
/// for the name itself.
fn hit_of(name: &str, query: &str) -> Option<(usize, usize)> {
    let lower = js::lower(name);
    let at = lower.find(query)?;
    let from = lower[..at].encode_utf16().count();
    let to = from + query.encode_utf16().count();
    let byte = |units: usize| {
        let mut n = 0;
        for (i, c) in name.char_indices() {
            if n >= units {
                return i;
            }
            n += c.len_utf16();
        }
        name.len()
    };
    Some((byte(from), byte(to)))
}

/// The byte offset of the UTF-16 unit `units` of s (its length past the end); None inside a surrogate pair.
pub(crate) fn unit_at(s: &str, units: usize) -> Option<usize> {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= units {
            return (n == units).then_some(i);
        }
        n += c.len_utf16();
    }
    (n <= units).then_some(s.len())
}

impl Tree {
    pub fn new(root: &str, settings: Settings) -> Tree {
        Tree {
            root: root.to_string(),
            settings,
            git: Git::default(),
            cache: HashMap::new(),
            stamps: HashMap::new(),
            check: HashSet::new(),
        }
    }

    fn full(&self, rel: &str) -> String {
        paths::join(&[&self.root, rel])
    }

    fn mtime(&self, rel: &str) -> Option<f64> {
        std::fs::metadata(self.full(rel)).ok()?.modified().ok().map(js::time_ms)
    }

    /// Whether a link leads to a folder; a link to a network share stays a plain entry, never followed (remote_link).
    fn is_dir(&self, full: &str) -> bool {
        !exec::remote_link(full, &self.root) && std::fs::metadata(full).is_ok_and(|m| m.is_dir())
    }

    /// A folder's entries, sorted, .git left out; from the cache unless stale and changed.
    pub fn read_dir(&mut self, rel: &str) -> Vec<Entry> {
        if let Some(cached) = self.cache.get(rel) {
            if !self.check.contains(rel) {
                return cached.clone();
            }
            self.check.remove(rel);
            let was = self.stamps.get(rel).copied().flatten();
            // A link keeps its folder's time when its target changes: it is looked at again.
            if was.is_some()
                && self.mtime(rel) == was
                && cached.iter().all(|e| !e.link || self.is_dir(&self.full(&e.rel)) == e.dir)
            {
                return cached.clone();
            }
        }
        let t = self.mtime(rel);
        self.stamps.insert(rel.to_string(), t.filter(|t| js::now_ms() - t > 2000.0));
        let mut entries = Vec::new();
        if let Ok(list) = std::fs::read_dir(self.full(rel)) {
            for e in list.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let Ok(kind) = e.file_type() else { continue };
                let full = paths::join(&[&self.root, rel, &name]);
                let link = kind.is_symlink();
                let dir = if link { self.is_dir(&full) } else { kind.is_dir() };
                let rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                entries.push(Entry { name, dir, rel, link, ..Entry::default() });
            }
            // Node reads the names sorted byte by byte (libuv's scandir): names equal for the tree keep that order.
            entries.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
            entries.sort_by(entry_order);
        }
        self.cache.insert(rel.to_string(), entries.clone());
        entries
    }

    /// Entries shown with the current settings: dot files and git-ignored ones can be hidden.
    pub fn visible(&mut self, rel: &str) -> Vec<Entry> {
        let s = self.settings;
        let list = self.read_dir(rel);
        list.into_iter()
            .filter(|e| {
                (s.hidden || !e.name.starts_with('.')) && (s.ignored || self.decoration(&e.rel, e.dir) != Some('!'))
            })
            .collect()
    }

    /// A changed entry: a file with a git change, or a folder holding one.
    pub fn is_changed(&self, e: &Entry) -> bool {
        self.decoration(&e.rel, e.dir).is_some_and(|d| d != '!')
    }

    /// The entries of a folder among the changed files: those on disk, and the deleted files and folders no longer
    /// there (gone), in the tree's order.
    pub fn with_gone(&mut self, rel: &str) -> Vec<Entry> {
        let mut list = self.visible(rel);
        let pre = if rel.is_empty() { String::new() } else { format!("{rel}/") };
        let mut gone: Ordered<bool> = Ordered::default();
        for g in &self.git.gone {
            let Some(rest) = g.strip_prefix(&pre) else { continue };
            let (name, dir) = match rest.find('/') {
                Some(i) => (&rest[..i], true),
                None => (rest, false),
            };
            if !list.iter().any(|e| e.name == name) && (self.settings.hidden || !name.starts_with('.')) {
                gone.set(name, dir);
            }
        }
        if gone.is_empty() {
            return list;
        }
        list.extend(gone.iter().map(|(name, &dir)| Entry {
            name: name.to_string(),
            dir,
            rel: format!("{pre}{name}"),
            gone: true,
            ..Entry::default()
        }));
        list.sort_by(entry_order);
        list
    }

    fn list(&mut self, kind: List, rel: &str) -> Vec<Entry> {
        match kind {
            List::Visible => self.visible(rel),
            List::Changed => {
                let all = self.with_gone(rel);
                all.into_iter().filter(|e| self.is_changed(e)).collect()
            }
        }
    }

    /// Compact folders: a folder whose only entry (in list) is a folder shows on one row with it, src/main/java, as
    /// the last one (its rel, expanded or not), chain the names before it ("src/main/"), from the first one's rel. At
    /// most 32 folders, as a link to a folder above would make the chain endless. unfolded, for the tree as it is
    /// unfolded (unfolded_of): a folder folded (nothing of it unfolded) is chained only through folders already read,
    /// so that the rows never read a folder they do not show; it shows its chain once unfolded.
    fn compact(&mut self, e: Entry, kind: List, unfolded: Option<&HashSet<String>>) -> Entry {
        if !self.settings.compact || !e.dir {
            return e;
        }
        let lazy = unfolded.is_some_and(|u| !u.contains(&e.rel));
        let mut last = e.clone();
        let mut names = Vec::new();
        for _ in 0..32 {
            if lazy && !self.cache.contains_key(&last.rel) {
                break;
            }
            let kids = self.list(kind, &last.rel);
            if kids.len() != 1 || !kids[0].dir {
                break;
            }
            names.push(last.name.clone());
            last = kids[0].clone();
        }
        if names.is_empty() {
            return e;
        }
        Entry { from: Some(e.rel), chain: Some(format!("{}/", names.join("/"))), ..last }
    }

    /// The rows of a view: query, the filter's text in lower case (no compact folders); changed, the changed files
    /// only; expanded, the folders unfolded.
    pub fn rows(&mut self, query: Option<&str>, changed: bool, expanded: &[String]) -> Vec<Entry> {
        let mut rows = Vec::new();
        let has = |rel: &str| expanded.iter().any(|e| e == rel);
        if let Some(query) = query.filter(|q| !q.is_empty()) {
            // Every folder is read, except ignored ones (node_modules, build output), within a budget.
            let mut budget = FILTER_BUDGET;
            rows = self.filter_walk("", 0, query, changed, &mut budget);
        } else if changed {
            // Only the changed entries, every folder unfolded, within the filter's budget.
            let mut budget = FILTER_BUDGET;
            self.changed_walk("", 0, &mut budget, &mut rows);
        } else {
            let unfolded = self.settings.compact.then(|| unfolded_of(expanded));
            self.walk("", 0, &has, unfolded.as_ref(), &mut rows);
        }
        rows
    }

    fn filter_walk(&mut self, rel: &str, depth: usize, query: &str, changed: bool, budget: &mut i64) -> Vec<Entry> {
        let mut found = Vec::new();
        let list = if changed { self.with_gone(rel) } else { self.visible(rel) };
        for e in list {
            *budget -= 1;
            if *budget < 0 {
                break;
            }
            if changed && !self.is_changed(&e) {
                continue;
            }
            let hit = hit_of(&e.name, query);
            let kids = if e.dir && !self.git.ignored.has(&e.rel) {
                self.filter_walk(&e.rel, depth + 1, query, changed, budget)
            } else {
                Vec::new()
            };
            if hit.is_some() || !kids.is_empty() {
                let open = !kids.is_empty();
                found.push(Entry { depth, hit, open, ..e });
                found.extend(kids);
            }
        }
        found
    }

    fn changed_walk(&mut self, rel: &str, depth: usize, budget: &mut i64, rows: &mut Vec<Entry>) {
        for entry in self.with_gone(rel) {
            *budget -= 1;
            if *budget < 0 {
                return;
            }
            if !self.is_changed(&entry) {
                continue;
            }
            let e = self.compact(entry, List::Changed, None);
            let (dir, next) = (e.dir, e.rel.clone());
            rows.push(Entry { depth, open: dir, ..e });
            if dir {
                self.changed_walk(&next, depth + 1, budget, rows);
            }
        }
    }

    fn walk(
        &mut self,
        rel: &str,
        depth: usize,
        has: &dyn Fn(&str) -> bool,
        unfolded: Option<&HashSet<String>>,
        rows: &mut Vec<Entry>,
    ) {
        for entry in self.visible(rel) {
            let e = self.compact(entry, List::Visible, unfolded);
            // A chain is open when its last folder is, or its first one (unfolded while it showed alone).
            let open = e.dir && (has(&e.rel) || e.from.as_deref().is_some_and(has));
            let next = e.rel.clone();
            rows.push(Entry { depth, open, ..e });
            if open {
                self.walk(&next, depth + 1, has, unfolded, rows);
            }
        }
    }

    /// A new git status (parse_status), and the deleted files no longer on disk.
    pub fn apply_status(&mut self, status: &str) {
        self.apply_statuses(&[("", status)]);
    }

    /// The statuses of the repositories of the root's folders, each with its folder ("" for the root's own
    /// repository), merged as apply_status applies one.
    pub fn apply_statuses(&mut self, list: &[(&str, &str)]) {
        let mut s = Status::default();
        for (base, status) in list {
            let prefix = if base.is_empty() { self.git.prefix.as_str() } else { "" };
            parse_into(&mut s, status, prefix, base);
        }
        self.git.files = s.files;
        self.git.dirs = s.dirs;
        self.git.untracked = s.untracked;
        self.git.ignored = s.ignored;
        let root = self.root.clone();
        self.git.gone = self
            .git
            .files
            .iter()
            .filter(|(rel, c)| **c == 'D' && std::fs::metadata(paths::join(&[&root, rel])).is_err())
            .map(|(rel, _)| rel.to_string())
            .collect();
    }

    /// Decoration of an entry: that of an ignored or untracked folder extends to all its contents.
    pub fn decoration(&self, rel: &str, dir: bool) -> Option<char> {
        let mut a = Some(rel);
        while let Some(at) = a.filter(|a| !a.is_empty()) {
            if self.git.ignored.has(at) {
                return Some('!');
            }
            if self.git.untracked.has(at) {
                return Some('U');
            }
            a = parent_of(at);
        }
        // A symbolic link to a folder is a file for git.
        dir.then(|| self.git.dirs.get(rel).copied()).flatten().or_else(|| self.git.files.get(rel).copied())
    }

    /// A change inside a collapsed ignored folder (node_modules, bin, obj…) is not visible: skipped.
    pub fn hidden(&self, rel: &str, expanded: &[String]) -> bool {
        let mut a = parent_of(rel);
        while let Some(at) = a.filter(|a| !a.is_empty()) {
            if self.git.ignored.has(at) && !expanded.iter().any(|e| e == at) {
                return true;
            }
            a = parent_of(at);
        }
        false
    }

    /// The changed files in the tree's order (folders first, then names, as read_dir sorts them).
    pub fn changed_files(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let mut budget = FILTER_BUDGET;
        self.changed_files_walk("", &mut budget, &mut out);
        out
    }

    fn changed_files_walk(&mut self, rel: &str, budget: &mut i64, out: &mut Vec<String>) {
        for e in self.with_gone(rel) {
            *budget -= 1;
            if *budget < 0 {
                return;
            }
            if !self.is_changed(&e) {
                continue;
            }
            if e.dir {
                self.changed_files_walk(&e.rel, budget, out);
            } else {
                out.push(e.rel);
            }
        }
    }

    /// A change on disk: the folders read are checked again before reuse.
    pub fn stale(&mut self) {
        self.check = self.cache.keys().cloned().collect();
    }

    /// Another root (Set as root): nothing read, no git status.
    pub fn reset(&mut self, root: &str) {
        self.root = root.to_string();
        self.cache.clear();
        self.git = Git::default();
    }
}

/// The folders expanded and their parents: those with something unfolded.
fn unfolded_of(expanded: &[String]) -> HashSet<String> {
    let mut out = HashSet::new();
    for rel in expanded {
        let mut r = rel.as_str();
        while !r.is_empty() && !out.contains(r) {
            out.insert(r.to_string());
            r = &r[..r.rfind('/').unwrap_or(0)];
        }
    }
    out
}
