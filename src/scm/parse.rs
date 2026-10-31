//! git's output turned into data, for the source control pane: status, history, stashes, refs,
//! remotes, worktrees, branch comparisons, config scopes and error messages. Pure functions.

use crate::core::exec::Exec;
use crate::core::paths;
use crate::unicode::natural_cmp;

/// The change of a status code: a type change shows as modified, a copy as added.
pub fn code(c: char) -> char {
    match c {
        'M' | 'T' => 'M',
        'A' | 'C' => 'A',
        'D' => 'D',
        'R' => 'R',
        _ => 'M',
    }
}

/// What a conflict's two codes mean.
pub fn conflict(xy: &str) -> Option<&'static str> {
    Some(match xy {
        "DD" => "both deleted",
        "AU" => "added by us",
        "UD" => "deleted by them",
        "UA" => "added by them",
        "DU" => "deleted by us",
        "AA" => "both added",
        "UU" => "both modified",
        _ => return None,
    })
}

/// The repository's folders: its top, its git folder, the folder shared by its worktrees.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dirs {
    pub top: String,
    pub gitdir: String,
    pub common: String,
}

/// git rev-parse --show-toplevel --absolute-git-dir --git-common-dir, run in root: None without a top folder (the
/// common folder can be relative to root).
pub fn parse_repo(out: &str, root: &str) -> Option<Dirs> {
    let mut lines = out.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
    let top = lines.next().filter(|t| !t.is_empty())?;
    let (gitdir, common) = (lines.next().unwrap_or(""), lines.next().unwrap_or(""));
    Some(Dirs {
        top: paths::resolve(&[top]),
        gitdir: paths::resolve(&[gitdir]),
        common: paths::resolve(&[root, common]),
    })
}

/// A file of the status: its path (and former path, for a rename staged), its change (M, A, D, R, U untracked, C
/// conflict), what a conflict is, and its section: "conflicts", "staged" or "changes".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub path: String,
    pub orig: Option<String>,
    pub code: char,
    pub label: Option<&'static str>,
    pub untracked: bool,
    pub sec: &'static str,
}

/// The branch, its upstream, and the conflicts, staged and unstaged changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub branch: String,
    pub oid: String,
    pub upstream: String,
    pub ahead: f64,
    pub behind: f64,
    pub conflicts: Vec<Item>,
    pub staged: Vec<Item>,
    pub changes: Vec<Item>,
    pub detached: bool,
    pub has_head: bool,
}

/// `/\+(\d+) -(\d+)/`: the counts of `# branch.ab +2 -1`.
fn ahead_behind(e: &str) -> Option<(f64, f64)> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let mut from = 0;
    while let Some(i) = e[from..].find('+') {
        let at = from + i + 1;
        let n = digits(&e[at..]);
        if n > 0 && e[at + n..].starts_with(" -") {
            let m = digits(&e[at + n + 2..]);
            if m > 0 {
                let num = |s: &str| s.parse().unwrap_or(f64::NAN);
                return Some((num(&e[at..at + n]), num(&e[at + n + 2..at + n + 2 + m])));
            }
        }
        from = at;
    }
    None
}

/// git status --porcelain=v2 -z --branch: the branch, its upstream, and the conflicts, staged and unstaged changes,
/// each sorted by path. A rename (type 2) is followed by its former path in the next field.
pub fn parse_status(out: &str) -> Status {
    let mut info = Status::default();
    let f: Vec<&str> = out.split('\0').collect();
    let mut i = 0;
    while i < f.len() {
        let e = f[i];
        let parts: Vec<&str> = e.split(' ').collect();
        let first = e.chars().next();
        if let Some(v) = e.strip_prefix("# branch.oid ") {
            info.oid = v.into();
        } else if let Some(v) = e.strip_prefix("# branch.head ") {
            info.branch = v.into();
        } else if let Some(v) = e.strip_prefix("# branch.upstream ") {
            info.upstream = v.into();
        } else if e.starts_with("# branch.ab ") {
            if let Some((a, b)) = ahead_behind(e) {
                (info.ahead, info.behind) = (a, b);
            }
        } else if first == Some('1') || first == Some('2') {
            let xy: Vec<char> = parts.get(1).map_or(Vec::new(), |x| x.chars().collect());
            let skip = if first == Some('1') { 8 } else { 9 };
            let p = parts.get(skip..).map_or(String::new(), |r| r.join(" "));
            let orig = if first == Some('2') {
                i += 1;
                f.get(i).map(|s| s.to_string())
            } else {
                None
            };
            // A missing code (a field XY shorter than 2) is not '.': a modification. Without the field
            // at all, the record is left out.
            if parts.len() > 1 {
                let x = xy.first().copied();
                if x != Some('.') {
                    let orig = if x == Some('R') { orig } else { None };
                    let c = x.map_or('M', code);
                    info.staged.push(Item { path: p.clone(), orig, code: c, sec: "staged", ..Item::default() });
                }
                let y = xy.get(1).copied();
                if y != Some('.') {
                    info.changes.push(Item { path: p, code: y.map_or('M', code), sec: "changes", ..Item::default() });
                }
            }
        } else if first == Some('u') {
            info.conflicts.push(Item {
                path: parts.get(10..).map_or(String::new(), |r| r.join(" ")),
                code: 'C',
                label: parts.get(1).and_then(|xy| conflict(xy)),
                sec: "conflicts",
                ..Item::default()
            });
        } else if first == Some('?') {
            let path = e.get(2..).unwrap_or("").to_string();
            info.changes.push(Item { path, code: 'U', untracked: true, sec: "changes", ..Item::default() });
        }
        i += 1;
    }
    for list in [&mut info.conflicts, &mut info.staged, &mut info.changes] {
        list.sort_by(|a, b| natural_cmp(&a.path, &b.path));
    }
    info.detached = info.branch == "(detached)";
    info.has_head = info.oid != "(initial)";
    info
}

/// The names a file had, from git log --follow --name-only -z --format=.
pub fn parse_names(out: &str) -> Vec<String> {
    out.split(['\0', '\n']).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// A decoration of a commit: a branch ("heads", HEAD alone when detached), a remote branch or a tag.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoration {
    pub name: String,
    pub kind: &'static str,
}

/// A commit of git log: hash, short hash, subject, author, age, decorations, whether it is HEAD, parents.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    pub refs: Vec<Decoration>,
    pub head: bool,
    pub parents: Vec<String>,
}

/// `+field`: a missing field is NaN.
fn number(f: Option<&str>) -> f64 {
    f.and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN)
}

/// `/(^|, )HEAD($|,| ->)/.test(refs)`.
fn is_head(refs: &str) -> bool {
    refs.match_indices("HEAD").any(|(i, _)| {
        let (before, after) = (&refs[..i], &refs[i + 4..]);
        (before.is_empty() || before.ends_with(", "))
            && (after.is_empty() || after.starts_with(',') || after.starts_with(" ->"))
    })
}

/// git log --decorate=full --format=%H%x1f%h%x1f%s%x1f%an%x1f%at%x1f%D%x1f%P: one commit per line.
pub fn parse_history(out: &str) -> Vec<Commit> {
    out.split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            let s = |i: usize| f.get(i).map_or(String::new(), |x| x.to_string());
            let refs = f.get(5).copied().unwrap_or("");
            Commit {
                hash: s(0),
                short: s(1),
                subject: s(2),
                author: s(3),
                date: ago(number(f.get(4).copied())),
                refs: ref_list(refs),
                head: f.get(5).is_some_and(|r| is_head(r)),
                parents: f
                    .get(6)
                    .map_or(Vec::new(), |p| p.split(' ').filter(|x| !x.is_empty()).map(String::from).collect()),
            }
        })
        .collect()
}

/// Short age of a commit: "5 min", "3 h", "2 d", "4 mo", "1 y".
pub fn ago(time: f64) -> String {
    ago_at(time, crate::core::clock::now_ms())
}

/// `ago` at a time (ms).
pub fn ago_at(time: f64, now: f64) -> String {
    let d = now / 1000.0 - time;
    // Math.max: NaN when either is.
    let s = if d.is_nan() { f64::NAN } else { d.max(0.0) };
    let n = |x: f64| fmt_num(x.floor());
    if s < 3600.0 {
        format!("{} min", fmt_num(1f64.max((s / 60.0).floor())))
    } else if s < 86400.0 {
        format!("{} h", n(s / 3600.0))
    } else if s < 30.0 * 86400.0 {
        format!("{} d", n(s / 86400.0))
    } else if s < 365.0 * 86400.0 {
        format!("{} mo", n(s / (30.0 * 86400.0)))
    } else {
        format!("{} y", n(s / (365.0 * 86400.0)))
    }
}

/// An integer as text (NaN included).
fn fmt_num(x: f64) -> String {
    if x.is_nan() { "NaN".into() } else { crate::json::number(x) }
}

/// A string with no line terminator, which `.` does not match.
fn one_line(s: &str) -> bool {
    !s.chars().any(crate::core::regex::is_line_terminator)
}

/// `/^refs\/(heads|remotes|tags)\/(.+)$/`: the kind and the name.
fn split_ref(r: &str) -> Option<(&'static str, &str)> {
    let rest = r.strip_prefix("refs/")?;
    for kind in ["heads", "remotes", "tags"] {
        if let Some(name) = rest.strip_prefix(kind).and_then(|x| x.strip_prefix('/'))
            && !name.is_empty()
            && one_line(name)
        {
            return Some((kind, name));
        }
    }
    None
}

/// Decorations of a commit (%D with full names): branches, remote branches (without their HEAD) and tags.
pub fn ref_list(d: &str) -> Vec<Decoration> {
    let mut out = Vec::new();
    for r in d.split(", ").filter(|r| !r.is_empty()) {
        let r = r.strip_prefix("HEAD -> ").unwrap_or(r);
        if r == "HEAD" {
            out.push(Decoration { name: "HEAD".into(), kind: "heads" });
        } else if let Some((kind, name)) = split_ref(r.strip_prefix("tag: ").unwrap_or(r))
            && !(kind == "remotes" && name.ends_with("/HEAD"))
        {
            out.push(Decoration { name: name.into(), kind });
        }
    }
    out
}

/// A label of a commit in the history: a branch (remote: one of the same name at the same commit; current: the one
/// checked out), a remote branch alone, or a tag.
#[derive(Debug, Clone, PartialEq)]
pub enum Label {
    Local { name: String, remote: bool, current: bool },
    Remote { name: String },
    Tag { name: String },
}

/// A commit's labels (ref_list) in the order the history shows them: a branch and the remote ones of the same name
/// at the same commit as one, the branch checked out first (head: the commit is HEAD, branch: the branch checked out,
/// "" when detached), then the other branches, the remote ones alone and the tags.
pub fn labels(refs: &[Decoration], head: bool, branch: &str) -> Vec<Label> {
    let heads: Vec<&Decoration> = refs.iter().filter(|r| r.kind == "heads").collect();
    let remotes: Vec<&Decoration> = refs.iter().filter(|r| r.kind == "remotes").collect();
    let mut joined = vec![false; remotes.len()];
    let local: Vec<(String, bool, bool)> = heads
        .iter()
        .map(|h| {
            let mut mine = false;
            for (k, r) in remotes.iter().enumerate() {
                let short = r.name.find('/').map_or(r.name.as_str(), |i| &r.name[i + 1..]);
                if short == h.name {
                    joined[k] = true;
                    mine = true;
                }
            }
            (h.name.clone(), mine, head && (h.name == "HEAD" || h.name == branch))
        })
        .collect();
    let mut out: Vec<Label> = Vec::new();
    for want in [true, false] {
        out.extend(local.iter().filter(|l| l.2 == want).map(|(name, remote, current)| Label::Local {
            name: name.clone(),
            remote: *remote,
            current: *current,
        }));
    }
    out.extend(remotes.iter().zip(&joined).filter(|(_, j)| !**j).map(|(r, _)| Label::Remote { name: r.name.clone() }));
    out.extend(refs.iter().filter(|r| r.kind == "tags").map(|t| Label::Tag { name: t.name.clone() }));
    out
}

/// A stash: its name (stash@{0}), subject and age.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stash {
    pub r#ref: String,
    pub subject: String,
    pub date: String,
}

/// git stash list --format=%gd%x1f%s%x1f%cr.
pub fn parse_stashes(out: &str) -> Vec<Stash> {
    out.split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            let s = |i: usize| f.get(i).map_or(String::new(), |x| x.to_string());
            Stash { r#ref: s(0), subject: s(1), date: s(2) }
        })
        .collect()
}

/// A branch, remote branch or tag of for-each-ref.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Branch {
    pub name: String,
    pub hash: String,
    pub upstream: String,
    pub track: String,
    pub date: String,
    pub subject: String,
    pub remote: bool,
    pub tag: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
}

/// Branches, remote branches, tags, remotes (their fetch URL) and worktrees.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Refs {
    pub heads: Vec<Branch>,
    pub remotes: Vec<Branch>,
    pub tags: Vec<Branch>,
    pub urls: Vec<Remote>,
    pub worktrees: Vec<Worktree>,
}

/// `/^(\S+)\s+(.*) \(fetch\)$/`: a remote and its fetch URL.
fn remote_line(l: &str) -> Option<Remote> {
    let rest = l.strip_suffix(" (fetch)")?;
    let name_end = rest.find(crate::unicode::is_space).unwrap_or(rest.len());
    let after = &rest[name_end..];
    let url = after.trim_start_matches(crate::unicode::is_space);
    if name_end == 0 || url.len() == after.len() || !one_line(url) {
        return None;
    }
    Some(Remote { name: rest[..name_end].into(), url: url.into() })
}

/// `/^<prefix>(.*)$/m`: the rest of the first line of a block that starts with prefix.
fn line_after<'a>(block: &'a str, prefix: &str) -> Option<&'a str> {
    block.split(crate::core::regex::is_line_terminator).find_map(|l| l.strip_prefix(prefix))
}

/// git for-each-ref (refname, short hash, upstream, track, date, subject, separated by %1f), git remote -v and git
/// worktree list --porcelain: branches, remote branches (without their HEAD), tags, remotes and worktrees.
pub fn parse_refs(refs: &str, remotes: &str, trees: &str) -> Refs {
    let mut r = Refs::default();
    for l in refs.split('\n').filter(|l| !l.is_empty()) {
        let f: Vec<&str> = l.split('\x1f').collect();
        let s = |i: usize| f.get(i).map_or(String::new(), |x| x.to_string());
        let Some((kind, name)) = split_ref(f[0]) else { continue };
        if kind == "remotes" && name.ends_with("/HEAD") {
            continue;
        }
        let b = Branch {
            name: name.into(),
            hash: s(1),
            upstream: s(2),
            track: s(3),
            date: s(4),
            subject: s(5),
            remote: kind == "remotes",
            tag: kind == "tags",
        };
        match kind {
            "heads" => r.heads.push(b),
            "remotes" => r.remotes.push(b),
            _ => r.tags.push(b),
        }
    }
    r.urls = remotes.split('\n').filter_map(remote_line).collect();
    for block in trees.split("\n\n") {
        if let Some(path) = line_after(block, "worktree ") {
            let branch = line_after(block, "branch refs/heads/").filter(|b| !b.is_empty()).unwrap_or("(detached)");
            r.worktrees.push(Worktree { path: path.into(), branch: branch.into() });
        }
    }
    r
}

/// A commit of one side of a branch comparison: same, the change is on both sides.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compared {
    pub same: bool,
    pub short: String,
    pub hash: String,
    pub subject: String,
    pub date: String,
}

/// One side of a branch comparison, git log --cherry-mark --format=%m%x1f%h%x1f%H%x1f%s%x1f%at: '=' marks a change
/// that is on both sides.
pub fn parse_compare(out: &str) -> Vec<Compared> {
    out.split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            let s = |i: usize| f.get(i).map_or(String::new(), |x| x.to_string());
            Compared { same: f[0] == "=", short: s(1), hash: s(2), subject: s(3), date: ago(number(f.get(4).copied())) }
        })
        .collect()
}

/// git rev-list --left-right --count base...branch: [behind, ahead].
pub fn parse_count(out: &str) -> Vec<f64> {
    // trim().split(/\s+/): "" gives [""], that is [0].
    let t = out.trim_matches(crate::unicode::is_space);
    if t.is_empty() {
        return vec![0.0];
    }
    t.split(crate::unicode::is_space).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(f64::NAN)).collect()
}

/// A file of git diff --numstat: add and del "-" for a binary file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Numstat {
    pub add: String,
    pub del: String,
    pub path: String,
}

pub fn parse_numstat(out: &str) -> Vec<Numstat> {
    out.split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Numstat {
                add: f[0].into(),
                del: f.get(1).map_or(String::new(), |x| x.to_string()),
                path: f.get(2..).map_or(String::new(), |p| p.join("\t")),
            }
        })
        .collect()
}

/// git config -z --show-scope --get-regexp: the values of each key, with their scope, in order.
pub fn parse_scopes(out: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut keys: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let f: Vec<&str> = out.split('\0').collect();
    let mut i = 0;
    while i + 1 < f.len() {
        let (key, value) = match f[i + 1].find('\n') {
            None => (f[i + 1], ""),
            Some(nl) => (&f[i + 1][..nl], &f[i + 1][nl + 1..]),
        };
        let pair = (f[i].to_string(), value.to_string());
        match keys.iter_mut().find(|(k, _)| k == key) {
            Some((_, list)) => list.push(pair),
            None => keys.push((key.into(), vec![pair])),
        }
        i += 2;
    }
    keys
}

/// The same output: the keys the repository sets itself (not the system, user or command line).
pub fn repo_keys(out: &str) -> Vec<String> {
    let f: Vec<&str> = out.split('\0').collect();
    let mut keys = Vec::new();
    let mut i = 0;
    while i + 1 < f.len() {
        if !["system", "global", "command"].contains(&f[i]) {
            keys.push(f[i + 1].split('\n').next().unwrap_or("").to_string());
        }
        i += 2;
    }
    keys
}

/// `/^! \[(remote )?rejected\]/`: the length of the match.
fn rejected(l: &str) -> Option<usize> {
    let rest = l.strip_prefix("! [")?;
    let rest2 = rest.strip_prefix("remote ").unwrap_or(rest);
    rest2.strip_prefix("rejected]").map(|_| l.len() - rest2.len() + "rejected]".len())
}

/// The message of a failed git command: its first error line, without the "fatal: " prefix.
pub fn error_of(res: &Exec) -> String {
    let text = if !res.err.is_empty() { &res.err } else { &res.out };
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).trim_matches(crate::unicode::is_space))
        .filter(|l| !l.is_empty())
        .collect();
    // A refused push: the reason (non-fast-forward, fetch first…) rather than "failed to push some refs".
    if let Some((l, n)) = lines.iter().find_map(|l| rejected(l).map(|n| (*l, n))) {
        let after = &l[n..];
        let reason = after.trim_start_matches(crate::unicode::is_space);
        return if reason.len() < after.len() { format!("rejected {reason}") } else { l.to_string() };
    }
    let fatal = |x: &&&str| x.starts_with("fatal:") || x.starts_with("error:");
    let l = match lines.iter().find(fatal).or(lines.first()) {
        Some(l) => l.to_string(),
        None => return if res.code.is_none() { "timed out or not run" } else { "failed" }.into(),
    };
    for p in ["fatal: ", "error: "] {
        if let Some(rest) = l.strip_prefix(p) {
            return rest.into();
        }
    }
    l
}
