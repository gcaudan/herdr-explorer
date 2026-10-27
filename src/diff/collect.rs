//! The changes read from git: of a scope (collect), of a commit, stash or tag (show), the lines of a file
//! changed since HEAD (line_changes), and the options every diff's View menu starts with.

use super::{Diff, parse};
use crate::core::buffer::{LineDiff, LineKind};
use crate::core::exec::{self, Env, Opts, env_set, git, git_env, run};
use crate::core::paths;
use crate::core::regex::Regex;
use crate::core::scope::{self, Scope};
use crate::core::ui::{self, MenuItem, c, fg, seg};
use crate::json::Value;
use std::cell::RefCell;
use std::collections::BTreeSet;

pub const MAX_UNTRACKED: usize = 200;

/// Options of every git diff, show or stash show: no colours, external tools or textconv filters (from the
/// repository config, they could run commands), and the a/ b/ prefixes whatever diff.noprefix or diff.mnemonicPrefix
/// say, so that the parser finds the paths.
pub const DIFF_OPTS: [&str; 6] =
    ["--no-color", "--no-ext-diff", "--no-textconv", "--ignore-submodules=all", "--src-prefix=a/", "--dst-prefix=b/"];

const SHOW: [&str; 3] = ["--stat", "-p", "--diff-merges=first-parent"];

/// git's environment for a diff: a file named '*' or '[id]' means itself.
fn diff_env(cwd: &str) -> Env {
    let mut env = git_env(cwd);
    env_set(&mut env, "GIT_LITERAL_PATHSPECS", "1");
    env
}

fn git_args<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["-c", "core.quotePath=false"];
    all.extend_from_slice(args);
    all
}

/// A commit, a stash or a tag with its changes, as source control and the preview show them: the git arguments, or
/// None for a request not well formed. what: { kind, ref, paths }, paths limiting a commit to a file's history.
pub fn show_args(what: &Value) -> Option<Vec<String>> {
    let r = what.get("ref").and_then(Value::as_str).filter(|r| !r.is_empty() && !r.starts_with('-'))?;
    let paths: Vec<String> = match what.get("paths") {
        // The default [] only for a missing one: null is not an array.
        None => Vec::new(),
        Some(Value::Arr(a)) => {
            a.iter().map(|p| p.as_str().filter(|p| !p.is_empty()).map(String::from)).collect::<Option<_>>()?
        }
        Some(_) => return None,
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match what.get("kind").and_then(Value::as_str) {
        Some("commit") => {
            let mut a = s(&["show"]);
            a.extend(s(&DIFF_OPTS));
            a.extend(s(&SHOW));
            a.extend(s(&["--format=fuller", "--decorate=short", r]));
            if !paths.is_empty() {
                a.push("--".into());
                a.extend(paths);
            }
            Some(a)
        }
        Some("stash") => {
            let mut a = s(&["stash", "show"]);
            a.extend(s(&DIFF_OPTS));
            a.extend(s(&["--stat", "-p", "--include-untracked", r]));
            Some(a)
        }
        Some("tag") => {
            let mut a = s(&["show"]);
            a.extend(s(&DIFF_OPTS));
            a.extend(s(&SHOW));
            a.push(format!("refs/tags/{r}"));
            Some(a)
        }
        _ => None,
    }
}

/// The number of a pull request to show ({ kind: "pull", ref }), None for another request.
pub fn pull_of(what: &Value) -> Option<u64> {
    if what.get("kind").and_then(Value::as_str) != Some("pull") {
        return None;
    }
    what.get("ref").and_then(Value::as_str)?.parse().ok().filter(|n| *n > 0)
}

/// The changes of show_args, parsed, run from cwd: None when git fails. A pull request's come from gh, after its
/// header and description.
pub fn show(cwd: &str, what: &Value) -> Option<Diff> {
    if let Some(n) = pull_of(what) {
        return crate::core::gh::show(cwd, n).map(|text| parse(&text));
    }
    let args = show_args(what)?;
    git()?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let opts = Opts { cwd: Some(cwd.into()), env: Some(diff_env(cwd)), ..Default::default() };
    run(git(), &git_args(&args), &opts).map(|out| parse(&out))
}

/// The options every diff's View menu starts with (⚙ or ,), each changing in place, the menu kept open: layout,
/// "inline" or "split"; wrap, the Wrap lines setting.
pub fn view_items<T>(layout: &str, wrap: bool, switch_layout: T, toggle_wrap: T) -> Vec<MenuItem<T>> {
    vec![
        MenuItem {
            label: "Layout".into(),
            key: "v".into(),
            value: Some(seg(layout, fg(c().blue))),
            danger: false,
            keep: true,
            run: switch_layout,
        },
        MenuItem {
            label: "Wrap lines".into(),
            key: "w".into(),
            value: Some(ui::on_off(wrap)),
            danger: false,
            keep: true,
            run: toggle_wrap,
        },
    ]
}

/// The default branch: the remote's HEAD (origin/main), else main, master, origin/main or origin/master, the first
/// that exists; "" when none does. out(args): git's trimmed output for args ("" on a failure).
pub fn default_branch(out: impl Fn(&[&str]) -> String) -> String {
    let name = out(&["rev-parse", "--abbrev-ref", "-q", "--verify", "refs/remotes/origin/HEAD"]);
    if !name.is_empty() {
        return name;
    }
    for b in ["main", "master", "origin/main", "origin/master"] {
        if !out(&["rev-parse", "--verify", "-q", &format!("{b}^{{commit}}")]).is_empty() {
            return b.into();
        }
    }
    String::new()
}

/// What collect found: the diff's text; top, the repository's folder, which the diff's paths are relative to;
/// base_name, the default branch ("" when none was found, or when compared with it but no common commit); error, why
/// the scope could not be compared (nothing then compared); staged, for HEAD, the staged changes, text then holding
/// the others (changes_of parses them together).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collected {
    pub text: String,
    pub staged: Option<String>,
    pub top: String,
    pub base_name: String,
    pub error: Option<String>,
}

/// The changes to show, as git diff writes them, for a scope (core::scope): those of the working tree compared with
/// the last commit ("head"), where the branch left the default one ("branch") or a branch ("ref"), untracked files in
/// full; those since the agent's last turn started ("turn", turn: its snapshot { top, tree }); those of a commit or a
/// range ("rev"). file: a file or folder to limit them to; scope None: base's kind.
pub fn collect(cwd: &str, file: &str, base: &str, scope: Option<&Scope>, turn: Option<&Value>) -> Collected {
    let kind = scope.map_or(base, Scope::kind);
    let env = diff_env(cwd);
    let target: Vec<&str> = if file.is_empty() { Vec::new() } else { vec!["--", file] };
    // stdout even on a non-zero exit code: diff --no-index returns 1 when the files differ.
    let git_in = |dir: &str, args: &[&str]| -> String {
        if git().is_none() {
            return String::new();
        }
        exec::exec(
            git(),
            &git_args(args),
            &Opts { cwd: Some(dir.into()), env: Some(env.clone()), ..Default::default() },
        )
        .out
    };
    let g = |args: &[&str]| git_in(cwd, args);
    let diff = |args: &[&str]| {
        let mut all = vec!["diff"];
        all.extend_from_slice(&DIFF_OPTS);
        all.extend_from_slice(args);
        all.extend_from_slice(&target);
        g(&all)
    };
    let top =
        Some(g(&["rev-parse", "--show-toplevel"]).trim().to_string()).filter(|t| !t.is_empty()).unwrap_or(cwd.into());
    let mut base_name = String::new();
    let mut from = (!g(&["rev-parse", "--verify", "-q", "HEAD"]).trim().is_empty()).then(|| "HEAD".to_string());
    // The default branch, named whatever the base, for the switch: the remote's HEAD, else main or master.
    if from.is_some() {
        base_name = default_branch(|args| g(args).trim().to_string());
    }
    if from.is_some() && kind == "branch" {
        let fork =
            if base_name.is_empty() { String::new() } else { g(&["merge-base", "HEAD", &base_name]).trim().into() };
        if fork.is_empty() {
            base_name = String::new();
        } else {
            from = Some(fork);
        }
    }
    let fail =
        |top: String, base_name: String, e: String| Collected { top, base_name, error: Some(e), ..Default::default() };
    let done = |text: String, top: String, base_name: String| Collected { text, top, base_name, ..Default::default() };
    // A commit the user typed: its id, or None. --end-of-options keeps a name starting with - from reading as an
    // option.
    let commit = |rev: &str| -> Option<String> {
        if rev.is_empty() {
            return None;
        }
        Some(g(&["rev-parse", "--verify", "-q", "--end-of-options", &format!("{rev}^{{commit}}")]).trim().to_string())
            .filter(|c| !c.is_empty())
    };
    if kind == "turn" {
        let turn_top = turn.and_then(|t| t.get("top")).map(crate::json::text);
        if turn_top.is_none_or(|t| paths::resolve(&[&t]) != paths::resolve(&[&top])) {
            return fail(top, base_name, "No agent turn recorded in this repository".into());
        }
        let tree = turn.and_then(|t| t.get("tree")).filter(|t| t.truthy()).map(crate::json::text);
        let Some(tree) = tree else { return fail(top, base_name, "The agent turn is still being recorded".into()) };
        // Only the file compared added: the preview reloads at each save of it.
        let Some(now) = scope::worktree(&top, &env, true, (!file.is_empty()).then_some(file)) else {
            return fail(top, base_name, "Cannot read the working tree".into());
        };
        return done(diff(&[&tree, &now]), top, base_name);
    }
    let rev = scope.map_or("", Scope::rev);
    if kind == "rev" {
        // a..b, a...b (from where they parted), or one commit: its own changes, against its first parent. A side
        // left out is HEAD, as for git (main.., ..main).
        let range = Regex::new(r"^(.*?)(\.\.\.?)(.*)$", "").expect("range");
        if let Some(m) = range.exec(rev) {
            let side = |i| Some(m.get(i).unwrap_or("")).filter(|s| !s.is_empty()).unwrap_or("HEAD").to_string();
            let (x, y) = (side(1), side(3));
            let (Some(a), Some(b)) = (commit(&x), commit(&y)) else {
                let bad = if commit(&x).is_none() { x } else { y };
                return fail(top, base_name, format!("Unknown commit: {bad}"));
            };
            let start = if m.get(2) == Some("...") { g(&["merge-base", &a, &b]).trim().to_string() } else { a };
            if start.is_empty() {
                return fail(top, base_name, "No common commit".into());
            }
            return done(diff(&[&start, &b]), top, base_name);
        }
        let Some(c) = commit(rev) else { return fail(top, base_name, format!("Unknown commit: {rev}")) };
        // No parent: compared with the empty tree, whose id depends on the hash (SHA-1 or SHA-256).
        let parent = commit(&format!("{c}^")).unwrap_or_else(|| {
            let opts =
                Opts { cwd: Some(cwd.into()), env: Some(env.clone()), input: Some(Vec::new()), ..Default::default() };
            exec::exec(git(), &git_args(&["hash-object", "-t", "tree", "--stdin"]), &opts).out.trim().to_string()
        });
        return done(diff(&[&parent, &c]), top, base_name);
    }
    if kind == "ref" {
        from = commit(rev);
        if from.is_none() {
            return fail(top, base_name, format!("Unknown branch: {rev}"));
        }
    }
    // HEAD (or the empty tree, before any commit): the staged changes apart, without renames so that each path
    // stands for itself when a part of it is staged or unstaged (core::stage).
    let sections = kind == "head" || from.is_none();
    let staged = sections.then(|| diff(&["--cached", "--no-renames"]));
    let mut text = if sections { diff(&["--no-renames"]) } else { diff(&[from.as_deref().unwrap_or("")]) };
    // Untracked files named from top, as git diff names the others (ls-files and diff --no-index name them from cwd).
    let mut ls = vec!["ls-files", "--others", "--exclude-standard", "--full-name", "-z"];
    ls.extend_from_slice(&target);
    let listed = g(&ls);
    let untracked: Vec<&str> = listed.split('\0').filter(|s| !s.is_empty()).collect();
    for f in untracked.iter().take(MAX_UNTRACKED) {
        let mut args = vec!["diff"];
        args.extend_from_slice(&DIFF_OPTS);
        args.extend_from_slice(&["--no-index", "--", "/dev/null", f]);
        text += &git_in(&top, &args);
    }
    if untracked.len() > MAX_UNTRACKED {
        text += &format!("\n… and {} more untracked files\n", untracked.len() - MAX_UNTRACKED);
    }
    Collected { text, staged, top, base_name, error: None }
}

/// The parsed changes of what collect returned: with the staged changes apart (HEAD), each file of either section
/// marked sec "staged" or "unstaged", and the files in path order, a path's staged changes before its unstaged ones.
pub fn changes_of(res: &Collected) -> Diff {
    let Some(staged) = &res.staged else { return parse(&res.text) };
    let (mut a, mut b) = (parse(staged), parse(&res.text));
    for f in &mut a.files {
        f.sec = "staged".into();
    }
    for f in &mut b.files {
        f.sec = "unstaged".into();
    }
    let name = |f: &super::File| f.to.clone().or_else(|| f.from.clone()).unwrap_or_default();
    let mut files: Vec<_> = a.files.into_iter().chain(b.files).collect();
    // By path; a stable sort.
    files.sort_by_key(name);
    let pre: Vec<String> = a.pre.into_iter().chain(b.pre).collect();
    // An empty section's blank line.
    let pre = if pre.iter().any(|l| !l.trim_matches(crate::unicode::is_space).is_empty()) { pre } else { Vec::new() };
    Diff { pre, files, add: a.add + b.add, del: a.del + b.del }
}

thread_local! {
    static LINE_CACHE: RefCell<Vec<(String, String, Option<LineDiff>)>> = const { RefCell::new(Vec::new()) };
}

/// The lines of a file changed since HEAD, for the preview's File view: kinds[i] add or mod for line i (from 0),
/// deleted the lines just under removed ones (the line count for the end); every line added for an untracked file of
/// count lines. None outside a repository, before the first commit, or when git fails; nothing changed for an ignored
/// file. Kept while the file and HEAD stay the same (cache_key), since the preview asks again at each focus and
/// reload.
pub fn line_changes(file: &str, count: usize) -> Option<LineDiff> {
    let abs = paths::resolve(&[file]);
    let key = cache_key(&abs, count);
    if let Some(k) = &key {
        let hit = LINE_CACHE.with(|c| c.borrow().iter().find(|e| e.0 == abs && &e.1 == k).map(|e| e.2.clone()));
        if let Some(res) = hit {
            return res;
        }
    }
    let res = read_line_changes(&abs, count);
    LINE_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|e| e.0 != abs);
        if let (Some(k), Some(r)) = (key, &res) {
            c.push((abs, k, Some(r.clone())));
        }
        if c.len() > 50 {
            c.remove(0);
        }
    });
    res
}

/// What line_changes depends on, read without git: the file's mtime and size, and HEAD (its text, and the times of
/// the branch it names and of packed-refs, which a commit or a reset changes). None outside a repository found as a
/// .git folder or file above the file.
fn cache_key(abs: &str, count: usize) -> Option<String> {
    let read = |f: &str| exec::read_text(f).ok();
    let time = |f: &str| std::fs::metadata(f).and_then(|m| m.modified()).map_or(0.0, crate::core::clock::ms);
    let st = std::fs::metadata(abs).ok()?;
    let mut d = paths::dirname(abs);
    loop {
        let dot = paths::join(&[&d, ".git"]);
        if let Ok(kind) = std::fs::metadata(&dot) {
            let mut gitdir = dot.clone();
            if kind.is_file() {
                let text = read(&dot).unwrap_or_default();
                let m = Regex::new(r"^gitdir: (.+)$", "m").expect("gitdir").exec(&text)?;
                gitdir = paths::resolve(&[&d, m.get(1).unwrap_or("").trim_matches(crate::unicode::is_space)]);
            }
            let head = read(&paths::join(&[&gitdir, "HEAD"]))?;
            let commondir = read(&paths::join(&[&gitdir, "commondir"])).unwrap_or_else(|| ".".into());
            let common = paths::resolve(&[&gitdir, commondir.trim_matches(crate::unicode::is_space)]);
            let r = Regex::new(r"^ref: (.+)$", "m").expect("ref").exec(&head).and_then(|m| m.get(1).map(String::from));
            let ref_time = r.map_or(0.0, |r| time(&paths::join(&[&common, r.trim_matches(crate::unicode::is_space)])));
            let mtime = st.modified().map_or(0.0, crate::core::clock::ms);
            return Some(
                [
                    abs.to_string(),
                    count.to_string(),
                    mtime.to_string(),
                    st.len().to_string(),
                    head.trim_matches(crate::unicode::is_space).to_string(),
                    ref_time.to_string(),
                    time(&paths::join(&[&common, "packed-refs"])).to_string(),
                ]
                .join("|"),
            );
        }
        let up = paths::dirname(&d);
        if up == d {
            return None;
        }
        d = up;
    }
}

fn read_line_changes(abs: &str, count: usize) -> Option<LineDiff> {
    let cwd = paths::dirname(abs);
    let opts = Opts { cwd: Some(cwd.clone()), env: Some(diff_env(&cwd)), ..Default::default() };
    let g = |args: &[&str]| run(git(), &git_args(args), &opts);
    let untracked = g(&["ls-files", "--others", "--exclude-standard", "-z", "--", abs])?;
    if !untracked.is_empty() {
        return Some(LineDiff { kinds: vec![LineKind::Add; count], deleted: BTreeSet::new() });
    }
    let mut args = vec!["diff"];
    args.extend_from_slice(&DIFF_OPTS);
    args.extend_from_slice(&["-U0", "--no-renames", "HEAD", "--", abs]);
    let text = g(&args)?;
    // kinds as long as the lines it names, at least count.
    let mut kinds = vec![LineKind::Same; count];
    let mut deleted = BTreeSet::new();
    let head = Regex::new(r"^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", "").expect("head");
    for l in text.split('\n') {
        let Some(m) = head.exec(l) else { continue };
        let n = |i: usize| m.get(i).map_or(1, |v| v.parse::<usize>().unwrap_or(0));
        let (del, add, start) = (n(1), n(3), m.get(2).map_or(0, |v| v.parse::<usize>().unwrap_or(0)));
        // Without new lines, the hunk names the line before the removed ones.
        let at = if add > 0 { start.saturating_sub(1) } else { start };
        if kinds.len() < at + add {
            kinds.resize(at + add, LineKind::Same);
        }
        for k in 0..add {
            kinds[at + k] = if k < del { LineKind::Mod } else { LineKind::Add };
        }
        if del > add {
            deleted.insert(at + add);
        }
    }
    Some(LineDiff { kinds, deleted })
}
