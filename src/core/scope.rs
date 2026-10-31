//! The diff scope of a tab: what the Changes pane and the preview's Diff view compare the files with, chosen with b
//! (or Compare in source control) and shared by every pane of the tab through scope-<tab id>.json in the state folder.
//! A scope is { kind, rev }:
//!   head    the last commit (the default)
//!   branch  where the branch left the default one (merge-base): every change of the branch, committed or not
//!   turn    the working tree when the agent of the tab last started working (turn-<tab id>.json, see agent_status)
//!   rev     a commit (its own changes) or a range, a..b or a...b; rev the text typed
//!   ref     a branch (or any commit) compared with the working tree; rev its name
//!
//! The menu b opens comes with the ui layer.

use super::exec::{self, Env, Opts, env_remove, env_set, git, run, tab_file, write_atomic};
use super::paths;
use super::ui::{self, Field, Menu, MenuItem};
use crate::codec::sha;
use crate::json::{self, Value};
use std::time::Duration;

pub const KINDS: [&str; 5] = ["head", "branch", "turn", "rev", "ref"];

/// A scope as read from its file or made by a pane: the object itself, its other fields kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Scope(pub Value);

impl Scope {
    pub fn head() -> Scope {
        Scope(crate::obj! { "kind" => "head" })
    }

    pub fn new(kind: &str, rev: Option<&str>) -> Scope {
        let mut v = crate::obj! { "kind" => kind };
        if let Some(r) = rev {
            v.set("rev", Value::Str(r.into()));
        }
        Scope(v)
    }

    pub fn kind(&self) -> &str {
        self.0.get("kind").and_then(Value::as_str).unwrap_or("")
    }

    /// The commit, range or branch; "" when none.
    pub fn rev(&self) -> &str {
        self.0.get("rev").and_then(Value::as_str).unwrap_or("")
    }
}

pub fn read(tab: &str) -> Scope {
    let parsed = tab_file("scope", tab).and_then(|f| super::exec::read_text(f).ok()).and_then(|t| json::parse(&t).ok());
    if let Some(s) = parsed {
        let kind = s.get("kind").and_then(Value::as_str);
        if let Some(k) = kind
            && KINDS.contains(&k)
            && (!matches!(k, "rev" | "ref") || s.get("rev").and_then(Value::as_str).is_some_and(|r| !r.is_empty()))
        {
            return Scope(s);
        }
    }
    Scope::head()
}

/// HEAD removes the file, the default.
pub fn write(tab: &str, scope: &Scope) -> bool {
    let Some(file) = tab_file("scope", tab) else { return false };
    if scope.kind() == "head" {
        let _ = std::fs::remove_file(&file);
        return true;
    }
    let mut v = Value::Obj(Vec::new());
    if let Some(k) = scope.0.get("kind") {
        v.set("kind", k.clone());
    }
    if let Some(r) = scope.0.get("rev").filter(|r| r.truthy()) {
        v.set("rev", r.clone());
    }
    write_atomic(&file, json::stringify(&v))
}

/// Watches the tab's scope (another pane chose one); None without a state folder.
pub fn watch(tab: &str) -> Option<super::watch::Watch> {
    tab_file("scope", tab).map(|f| super::watch::Watch::new(&f, Duration::from_millis(500)))
}

/// The same kind and the same rev, a missing rev as an empty one.
pub fn same(a: &Scope, b: &Scope) -> bool {
    let rev = |s: &Scope| s.0.get("rev").filter(|r| r.truthy()).cloned().unwrap_or(Value::Str(String::new()));
    a.0.get("kind") == b.0.get("kind") && rev(a) == rev(b)
}

/// The scope in a few words, for a header: HEAD, main base, last turn, abc123, a..b, vs feature.
pub fn label(scope: &Scope, base_name: &str) -> String {
    match scope.kind() {
        "branch" => format!("{} base", if base_name.is_empty() { "main" } else { base_name }),
        "turn" => "last turn".into(),
        // The full names source control writes.
        "rev" => strip_ref_prefixes(scope.rev()),
        "ref" => format!("vs {}", scope.rev()),
        _ => "HEAD".into(),
    }
}

/// `s.replace(/refs\/(heads|remotes)\//g, '')`, in one pass.
fn strip_ref_prefixes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("refs/heads/").or_else(|| rest.strip_prefix("refs/remotes/")) {
            rest = r;
            continue;
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// What an item of the scope menus does: a scope chosen at once, or one that needs a commit or range ("rev") or a
/// branch ("ref") asked first: the pane hides the menu, then prompts from the rev typed before (prompt_rev), or shows
/// the menu of the branches (branch_menu).
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Scope(Scope),
    Ask(&'static str),
}

/// The items of the menu b opens, as ui::Menu takes them.
pub fn items(scope: &Scope, base_name: &str) -> Vec<Option<MenuItem<Pick>>> {
    let mark = |s: &Scope| {
        (same(scope, s) || (scope.kind() == s.kind() && ["rev", "ref"].contains(&s.kind()))).then(|| ui::seg("●", ""))
    };
    let item = |label: String, value, run| {
        Some(MenuItem { label, key: String::new(), value, danger: false, keep: false, run })
    };
    let base = if base_name.is_empty() { "main" } else { base_name };
    // A scope without its rev shows "undefined".
    let rev = scope.0.get("rev").map_or("undefined".into(), json::text);
    let typed = |kind: &str| if scope.kind() == kind { format!("  {rev}") } else { String::new() };
    let kind = |k: &str| Scope::new(k, None);
    vec![
        item("HEAD".into(), mark(&Scope::head()), Pick::Scope(Scope::head())),
        item(format!("Merge base with {base}"), mark(&kind("branch")), Pick::Scope(kind("branch"))),
        item("Last agent turn".into(), mark(&kind("turn")), Pick::Scope(kind("turn"))),
        None,
        item(format!("Commit or range…{}", typed("rev")), mark(&kind("rev")), Pick::Ask("rev")),
        item(format!("Branch…{}", typed("ref")), mark(&kind("ref")), Pick::Ask("ref")),
    ]
}

/// The menu b opens, at the right edge of a screen w columns wide.
pub fn menu(scope: &Scope, base_name: &str, w: i64) -> Menu<Pick> {
    Menu::new("Compare with", items(scope, base_name), 1, w, 1)
}

/// The rev Commit or range… starts from: the one typed before.
pub fn prompt_rev(scope: &Scope) -> &str {
    if scope.kind() == "rev" { scope.rev() } else { "" }
}

/// The menu of Branch…, in place of the first, from the repository's branches (branches); None without any (the
/// pane says "No branch").
pub fn branch_menu(scope: &Scope, list: &[String], w: i64) -> Option<Menu<Pick>> {
    if list.is_empty() {
        return None;
    }
    let items = list
        .iter()
        .map(|b| {
            Some(MenuItem {
                label: b.clone(),
                key: String::new(),
                value: (scope.kind() == "ref" && scope.rev() == b).then(|| ui::seg("●", "")),
                danger: false,
                keep: false,
                run: Pick::Scope(Scope::new("ref", Some(b))),
            })
        })
        .collect();
    Some(Menu::new("Branch", items, 1, w, 1))
}

/// A key in the field of Commit or range….
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevKey {
    /// Enter (the rev, "" when empty) or escape ("").
    Done(String),
    /// Whether the field took the key.
    Changed(bool),
}

pub fn rev_key(field: &mut Field, k: &str) -> RevKey {
    if k == "escape" || k == "ctrl+c" {
        return RevKey::Done(String::new());
    }
    if k == "enter" {
        return RevKey::Done(field.value.trim_matches(crate::unicode::is_space).to_string());
    }
    RevKey::Changed(field.key(k))
}

/// The branches of the repository holding cwd, local then remote, the latest commit first, for Branch….
pub fn branches(cwd: &str) -> Vec<String> {
    let opts = Opts { cwd: Some(cwd.into()), env: Some(exec::git_env(cwd)), ..Default::default() };
    let out = run(
        git(),
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%00%(symref)%00%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
        &opts,
    )
    .unwrap_or_default();
    // origin/HEAD, whose short name is origin, left out: a remote's symbolic ref (whatever the remote's name), or its
    // HEAD by name when not symbolic. A local symbolic ref (master as an alias of main) stays.
    out.split('\n')
        .filter_map(|l| {
            let mut f = l.split('\0');
            let (full, sym, short) = (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
            let remote_head = full.strip_prefix("refs/remotes/").is_some_and(|r| {
                !sym.is_empty() || r.strip_suffix("/HEAD").is_some_and(|r| !r.is_empty() && !r.contains('/'))
            });
            (!short.is_empty() && !remote_head).then(|| short.trim_matches(crate::unicode::is_space).to_string())
        })
        .collect()
}

// --- the agent's last turn

/// Untracked files above this size stay out of the snapshot (a dataset or a build output not ignored): hashed, each
/// would take its size again in the repository's objects until git gc.
pub const MAX_SNAPSHOT_FILE: u64 = 10 << 20;

/// A tree object of the repository's working tree as it is: tracked files with their changes, and untracked ones
/// that are not ignored (up to MAX_SNAPSHOT_FILE), added to a copy of the index (the real one left alone), so that git
/// rehashes only the files changed since. Its blobs stay in the repository's objects until git gc prunes them. Returns
/// the tree's id, or None.
/// keep: the copy is kept in the state folder (index/, one per index file of a repository) for the next call, so that
/// the untracked files too are rehashed only once changed (the Diff view reloaded); copied again when the real index
/// changes. Not in the git folder: a repository received with it (an archive) could hold a symbolic link there, which
/// the copy would follow to overwrite any file of the user.
/// only: a file or folder, the one path the caller compares, the only one added (the rest of the tree is then as last
/// added, not as it is now).
pub fn worktree(top: &str, env: &Env, keep: bool, only: Option<&str>) -> Option<String> {
    // The exclusions below need the pathspec magic that GIT_LITERAL_PATHSPECS turns off: the paths are made literal
    // one by one instead.
    let mut plain = env.clone();
    env_remove(&mut plain, "GIT_LITERAL_PATHSPECS");
    let git_in = |args: &[&str], index: Option<&str>| {
        let mut e = plain.clone();
        if let Some(i) = index {
            env_set(&mut e, "GIT_INDEX_FILE", i);
        }
        run(git(), args, &Opts { cwd: Some(top.into()), env: Some(e), ..Default::default() })
    };
    let index = git_in(&["rev-parse", "--git-path", "index"], None).unwrap_or_default();
    let index = index.trim_matches(crate::unicode::is_space);
    if index.is_empty() {
        return None;
    }
    let scope_path = match only {
        Some(o) => format!(":(literal){o}"),
        None => ".".into(),
    };
    // The large untracked files left out: the last turn shows them as new.
    let listing = git_in(&["ls-files", "-o", "--exclude-standard", "-z", "--", &scope_path], None).unwrap_or_default();
    let big: Vec<String> = listing
        .split('\0')
        .filter(|f| {
            !f.is_empty() && std::fs::metadata(paths::join(&[top, f])).is_ok_and(|m| m.len() > MAX_SNAPSHOT_FILE)
        })
        .map(|f| format!(":(exclude,literal){f}"))
        .collect();
    // Added to the kept copy while still small: taken out, else its old content stays in the snapshot.
    let mut drop_big: Vec<&str> = vec!["rm", "--cached", "-q", "--ignore-unmatch", "--"];
    let literal: Vec<String> = big.iter().map(|f| f.replacen(":(exclude,literal)", ":(literal)", 1)).collect();
    drop_big.extend(literal.iter().map(String::as_str));
    let mut add: Vec<&str> = vec!["add", "-A", "--", &scope_path];
    add.extend(big.iter().map(String::as_str));
    let real_index = paths::resolve(&[top, index]);
    let kept = if keep { kept_index(&real_index) } else { None };
    if let Some(kept) = kept {
        remove_old_copies(&paths::dirname(&real_index));
        let from_of = |st: &Option<std::fs::Metadata>| match st {
            Some(m) => format!(
                "{}:{}",
                json::number(m.modified().map(crate::core::clock::ms).unwrap_or(0.0)),
                json::number(m.len() as f64)
            ),
            None => "none".into(),
        };
        let fresh = |from: &str| {
            super::exec::read_text(format!("{kept}.from")).is_ok_and(|t| t == from)
                && std::path::Path::new(&kept).exists()
        };
        if !fresh(&from_of(&std::fs::metadata(&real_index).ok())) {
            // Copied while holding git's own lock of the copy: a git add of another pane running on it would otherwise
            // put back the old copy when it ends, while .from says it is fresh. Taken already: a copy of its own.
            let (lock, tmp) = (format!("{kept}.lock"), format!("{kept}.{}", std::process::id()));
            let locked = std::fs::create_dir_all(paths::dirname(&kept))
                .and_then(|()| std::fs::OpenOptions::new().write(true).create_new(true).open(&lock))
                .map(drop);
            // Never removed here, whatever its age: git add can hold it for minutes on a large repository, and nothing
            // tells whose it is. One left by a process that died goes with the weekly sweep (sweep_state).
            if locked.is_err() {
                return worktree(top, env, false, only);
            }
            let copied = (|| -> std::io::Result<bool> {
                // Another pane may have copied it while this one waited: copied again, the files its git add hashed
                // since would be hashed again. The index read again: it may have changed meanwhile, and .from must
                // say what is copied.
                let cur = std::fs::metadata(&real_index).ok();
                let cur_from = from_of(&cur);
                if fresh(&cur_from) {
                    return Ok(true);
                }
                match &cur {
                    Some(st) => copy_index(&real_index, &kept, st, &tmp)?,
                    None => match std::fs::remove_file(&kept) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                        _ => {}
                    },
                }
                Ok(write_atomic(&format!("{kept}.from"), &cur_from))
            })();
            if !matches!(copied, Ok(true)) {
                let _ = std::fs::remove_file(&tmp);
                let _ = std::fs::remove_file(&lock);
                return worktree(top, env, false, only);
            }
            let _ = std::fs::remove_file(&lock);
        }
        if (big.is_empty() || git_in(&drop_big, Some(&kept)).is_some()) && git_in(&add, Some(&kept)).is_some() {
            let tree = git_in(&["write-tree"], Some(&kept)).unwrap_or_default();
            let tree = tree.trim_matches(crate::unicode::is_space);
            // Marked used through .from (sweep_state removes the copy a week after it): git rewrites neither file
            // when nothing changed, and the copy's own mtime is git's to judge its racily clean entries.
            if let Ok(f) = std::fs::File::options().write(true).open(format!("{kept}.from")) {
                let now = std::time::SystemTime::now();
                let _ = f.set_times(std::fs::FileTimes::new().set_accessed(now).set_modified(now));
            }
            if !tree.is_empty() {
                return Some(tree.to_string());
            }
        }
        // Another pane updating it at the same time (its lock taken): a copy of its own.
        return worktree(top, env, false, only);
    }
    let dir = exec::mkdtemp("herdr-explorer-index-").ok()?;
    let tmp = paths::join(&[&dir, "index"]);
    if let Ok(st) = std::fs::metadata(&real_index) {
        let _ = copy_index(&real_index, &tmp, &st, &tmp);
    }
    let tree = git_in(&add, Some(&tmp)).and_then(|_| git_in(&["write-tree"], Some(&tmp))).and_then(|t| {
        let t = t.trim_matches(crate::unicode::is_space);
        (!t.is_empty()).then(|| t.to_string())
    });
    let _ = std::fs::remove_dir_all(&dir);
    tree
}

/// Copies the index file src (st, its metadata, read before) to dst through tmp, with its dates: git trusts the stat
/// data of the files changed in the same tick as the index was written (racily clean) only if they are older than it,
/// not the copy. Dated once renamed: sweep_state judges a leftover tmp by its own date.
fn copy_index(src: &str, dst: &str, st: &std::fs::Metadata, tmp: &str) -> std::io::Result<()> {
    std::fs::copy(src, tmp)?;
    if tmp != dst {
        std::fs::rename(tmp, dst)?;
    }
    let mut times = std::fs::FileTimes::new();
    if let Ok(a) = st.accessed() {
        times = times.set_accessed(a);
    }
    if let Ok(m) = st.modified() {
        times = times.set_modified(m);
    }
    std::fs::File::options().write(true).open(dst)?.set_times(times)
}

/// The copy that versions before 0.8.0 kept in the git folder, beside the index, with its .from, and the .<pid>
/// copies and git's .lock of a process that died: removed once per git folder and process.
fn remove_old_copies(git_dir: &str) {
    static DONE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    {
        let mut done = DONE.lock().unwrap_or_else(|p| p.into_inner());
        if done.iter().any(|d| d == git_dir) {
            return;
        }
        done.push(git_dir.to_string());
    }
    static OLD: std::sync::LazyLock<super::regex::Regex> = std::sync::LazyLock::new(|| {
        super::regex::Regex::new(r"^herdr-explorer-index(\.from(\.\d+)?|\.lock|\.\d+)?$", "").expect("a valid pattern")
    });
    for name in exec::list_dir(git_dir).into_iter().filter(|n| OLD.test(n)) {
        let _ = std::fs::remove_file(paths::join(&[git_dir, &name]));
    }
}

/// The copy of an index file kept by worktree: index/<hash of its path> in the state folder, None without one. Those
/// not used for a week are removed by sweep_state (exec). Kept per state folder and index, as its path is resolved.
pub fn kept_index(index: &str) -> Option<String> {
    static KEPT: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
    let dir = exec::state_dir()?;
    let key = format!("{dir}\0{index}");
    let mut kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, path)) = kept.iter().find(|(k, _)| *k == key) {
        return Some(path.clone());
    }
    let hash = sha::hex(&sha::sha1(exec::real_path(index).as_bytes()));
    let path = paths::join(&[&dir, "index", &hash]);
    kept.push((key, path.clone()));
    Some(path)
}

/// What turn-<tab id>.json keeps: { pane, top, tree, t, ended }, the working tree of the repository of the pane whose
/// agent started working last, taken when it did (tree null while it is being taken); ended once that agent is idle
/// or done again. Null when there is none.
pub fn read_turn(tab: &str) -> Value {
    tab_file("turn", tab)
        .and_then(|f| super::exec::read_text(f).ok())
        .and_then(|t| json::parse(&t).ok())
        .unwrap_or(Value::Null)
}

/// Sets fields of the turn started at t, read again just before: the hooks of several events run at once, and a
/// write made from an older read would undo the other's (a tree taken back to null, an end lost). Read back after,
/// to write again if another hook wrote over it meanwhile. False when the turn is no longer that one.
pub fn update_turn(tab: &str, t: Option<&Value>, fields: &[(&str, Value)]) -> bool {
    let same_t = |v: &Value| v.get("t") == t;
    for _ in 0..3 {
        let now = read_turn(tab);
        if !now.truthy() || !same_t(&now) {
            return false;
        }
        let mut next = Value::Obj(super::exec::spread(&now));
        for (k, v) in fields {
            next.set(k, v.clone());
        }
        if let Some(f) = tab_file("turn", tab) {
            write_atomic(&f, json::stringify(&next));
        }
        let back = read_turn(tab);
        if back.truthy() && same_t(&back) && fields.iter().all(|(k, v)| back.get(k) == Some(v)) {
            return true;
        }
    }
    false
}

/// An agent's status changed (the pane.agent_status_changed event): a new turn starts when it works again after
/// having been idle or done, or for an agent other than the last one; blocked or unknown in between keep the turn.
/// cwd: the pane's folder. Returns what was done, for the log.
pub fn agent_status(tab: &str, pane: &str, status: &str, cwd: &str) -> &'static str {
    let turn = read_turn(tab);
    let mine = turn.truthy() && turn.get("pane").is_some_and(|p| p.as_str() == Some(pane));
    let ended = turn.get("ended").is_some_and(Value::truthy);
    if status != "working" {
        if mine && (status == "idle" || status == "done") && !ended {
            update_turn(tab, turn.get("t"), &[("ended", Value::Bool(true))]);
        }
        return "kept";
    }
    if mine && !ended {
        return "same turn";
    }
    let env = exec::git_env(cwd);
    let top = run(
        git(),
        &["rev-parse", "--show-toplevel"],
        &Opts { cwd: Some(cwd.into()), env: Some(env.clone()), ..Default::default() },
    )
    .unwrap_or_default();
    let top = top.trim_matches(crate::unicode::is_space);
    if top.is_empty() {
        return "not a repository";
    }
    // The turn is written at once, its tree once taken: the hooks of the events that follow, run while git hashes the
    // files, see the turn started (a working one keeps it, an idle one ends it), and the tree does not undo them.
    let t = Value::Num(crate::core::clock::now_ms());
    if let Some(f) = tab_file("turn", tab) {
        let turn = crate::obj! {
            "pane" => pane, "top" => paths::resolve(&[top]), "tree" => Value::Null, "t" => t.clone(), "ended" => false,
        };
        write_atomic(&f, json::stringify(&turn));
    }
    let tree = worktree(top, &env, true, None);
    if !update_turn(tab, Some(&t), &[("tree", Value::from(tree.clone()))]) {
        return "snapshot dropped: a newer turn";
    }
    if tree.is_some() { "snapshot" } else { "no snapshot" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Branch…'s menu: one item per branch, the one compared with marked; none without branches.
    #[test]
    fn the_branch_menu_marks_the_branch_compared_with() {
        let list = vec!["main".to_string(), "feat".to_string()];
        let menu = branch_menu(&Scope::new("ref", Some("feat")), &list, 80).unwrap();
        assert_eq!((menu.title.as_str(), menu.at, menu.right), ("Branch", 1, 80));
        let items: Vec<_> =
            menu.items.iter().flatten().map(|it| (it.label.as_str(), it.value.is_some(), &it.run)).collect();
        assert_eq!(
            items,
            [
                ("main", false, &Pick::Scope(Scope::new("ref", Some("main")))),
                ("feat", true, &Pick::Scope(Scope::new("ref", Some("feat"))))
            ]
        );
        assert!(branch_menu(&Scope::head(), &[], 80).is_none());
        assert_eq!(prompt_rev(&Scope::new("rev", Some("a..b"))), "a..b");
        assert_eq!(prompt_rev(&Scope::new("ref", Some("main"))), "");
    }
}
