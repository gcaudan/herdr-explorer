//! State of the source control pane: git run in the repository, the loaders that fill S (status,
//! history, stashes, refs, the branch picker, diffs and branch comparisons), and the view switches. The loaders
//! block, run on the pane's worker (`load`), and their result is applied to
//! S in the loop (`apply`). The refresh, the watch of the git folder and what is drawn are the pane's.

use super::context::{Cmp, DiffView, Hist, Pick, PickItem, Picker, Repo, RepoState, Row, State};
use super::parse::{self, Branch, Dirs, Item, Refs, Stash, error_of};
use crate::core::exec::{self, Env, Exec, Opts};
use crate::core::paths;
use crate::diff::{self, DIFF_OPTS, Diff};
use crate::json::Value;
use std::time::Duration;

// --- running git

/// Config keys that run a program and that the repository could set in its own config: for the commands that write
/// (commit, switch, stash, fetch, pull, push), each one the repository sets is replaced by the user's global value or
/// the default. Hooks, fsmonitor and filters are already disabled by git_env.
const RISKY: &str = "^(core\\.(sshcommand|askpass|gitproxy)|gpg\\.(.+\\.)?program|gpg\\.ssh\\.defaultkeycommand|credential\\.(.+\\.)?helper|remote\\..+\\.(uploadpack|receivepack)|merge\\..+\\.driver)$";
const DEFAULTS: [(&str, &str); 7] = [
    ("core.sshcommand", "ssh"),
    ("core.askpass", ""),
    ("gpg.program", "gpg"),
    ("gpg.openpgp.program", "gpg"),
    ("gpg.x509.program", "gpgsm"),
    ("gpg.ssh.program", "ssh-keygen"),
    ("gpg.ssh.defaultkeycommand", ""),
];

/// The command-line options replacing upload-pack and receive-pack programs set by the repository (the first value
/// given wins for these, so the config cannot override them).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pack {
    pub fetch: Vec<String>,
    pub push: Vec<String>,
}

/// The hardened environment of the commands that write, and their pack options.
pub fn safe_env(cwd: &str) -> (Env, Pack) {
    let mut env = exec::git_env(cwd);
    let res = exec::exec(
        exec::git(),
        &["config", "-z", "--includes", "--show-scope", "--get-regexp", RISKY],
        &Opts { cwd: Some(cwd.into()), env: Some(env.clone()), ..Default::default() },
    );
    let keys = parse::parse_scopes(&res.out);
    // ext:: URLs run a command; a local path or file:// remote runs git-receive-pack on that repository, with its
    // hooks, and git leaves out the environment config (core.hooksPath) for a local transport.
    let mut extra: Vec<(String, String)> =
        vec![("protocol.ext.allow".into(), "never".into()), ("protocol.file.allow".into(), "never".into())];
    let mut pack = Pack::default();
    let user_scope = |s: &str| s == "system" || s == "global";
    for (key, list) in keys {
        if list.iter().all(|(scope, _)| user_scope(scope) || scope == "command") {
            continue;
        }
        let user: Vec<&String> = list.iter().filter(|(scope, _)| user_scope(scope)).map(|(_, v)| v).collect();
        if key.ends_with(".uploadpack") {
            pack.fetch = vec!["--upload-pack=git-upload-pack".into()];
        } else if key.ends_with(".receivepack") {
            pack.push = vec!["--receive-pack=git-receive-pack".into()];
        } else if key == "core.gitproxy" {
            // The first core.gitProxy wins too, but the variable takes precedence over the config.
            exec::env_set(&mut env, "GIT_PROXY_COMMAND", "");
        } else if key.ends_with("helper") {
            // credential helpers form a list, reset by an empty value.
            extra.push((key.clone(), String::new()));
            extra.extend(user.iter().map(|v| (key.clone(), v.to_string())));
        } else {
            let value = match user.last() {
                Some(v) => v.to_string(),
                None => DEFAULTS
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map_or("git merge-file %A %O %B".into(), |(_, v)| v.to_string()),
            };
            extra.push((key, value));
        }
    }
    let pairs: Vec<(&str, &str)> = extra.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    exec::add_config(&mut env, &pairs);
    (env, pack)
}

/// Paths on stdin: no command-line length limit.
pub const FROM_FILE: [&str; 2] = ["--pathspec-from-file=-", "--pathspec-file-nul"];

/// Messages in English, like the pane, and recognisable (not fully merged); the character set is kept.
pub fn english(env: &mut Env) {
    if let Some(all) = exec::env_get(env, "LC_ALL").filter(|v| !v.is_empty()).map(String::from) {
        exec::env_set(env, "LC_CTYPE", &all);
        exec::env_remove(env, "LC_ALL");
    }
    exec::env_remove(env, "LANGUAGE");
    exec::env_set(env, "LC_MESSAGES", "C");
}

/// How git runs: safe, the hardened environment, for the commands that write; env, variables added; config, [key,
/// value] pairs after the repository's config, as GIT_CONFIG_KEY_n (-c would split a key holding '=', which a branch
/// name can); input, its standard input; timeout and detached, as exec's; top, the repository an op runs in (the
/// one loaded when None).
#[derive(Debug, Clone, Default)]
pub struct GitOpts {
    pub top: Option<String>,
    pub safe: bool,
    pub env: Vec<(String, String)>,
    pub config: Vec<(String, String)>,
    pub input: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    pub detached: bool,
}

/// git in the repository (cwd: its top, the pane's folder before it is known). The paths given are literal
/// pathspecs: a file named '*.js' or ':x' means itself (not for the other commands: stash -u relies on pathspec
/// magic internally).
pub fn git(cwd: &str, args: &[&str], opts: &GitOpts) -> Exec {
    let safe = opts.safe.then(|| safe_env(cwd));
    let mut env = match &safe {
        Some((env, _)) => env.clone(),
        None => exec::git_env(cwd),
    };
    if args.contains(&"--") || args.contains(&FROM_FILE[0]) {
        exec::env_set(&mut env, "GIT_LITERAL_PATHSPECS", "1");
    }
    for (k, v) in &opts.env {
        exec::env_set(&mut env, k, v);
    }
    english(&mut env);
    let pairs: Vec<(&str, &str)> = opts.config.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    exec::add_config(&mut env, &pairs);
    let first = args.first().copied().unwrap_or("");
    let pack: &[String] = match &safe {
        Some((_, pack)) if first == "push" => &pack.push,
        Some((_, pack)) if first == "fetch" || first == "pull" => &pack.fetch,
        _ => &[],
    };
    let mut full: Vec<&str> = vec!["-c", "core.quotePath=false", first];
    full.extend(pack.iter().map(String::as_str));
    full.extend(args.iter().skip(1));
    exec::exec(
        exec::git(),
        &full,
        &Opts {
            cwd: Some(cwd.into()),
            env: Some(env),
            input: opts.input.clone(),
            timeout: opts.timeout,
            detached: opts.detached,
            ..Default::default()
        },
    )
}

/// git's output for args, without options.
fn run(cwd: &str, args: &[&str]) -> Exec {
    git(cwd, args, &GitOpts::default())
}

/// Network settings the repository could set in its own config to send the user's credentials elsewhere (another
/// address or proxy, a certificate of its own, no TLS check) or to rewrite the remote URL; and
/// core.alternateRefsCommand, a shell command git runs while negotiating a fetch; remote.<name>.vcs, a remote helper
/// program that can be a relative path into the repository.
const NETWORK: &str =
    "^(http\\..+|url\\..+\\.(insteadof|pushinsteadof)|remote\\..+\\.(proxy|vcs)|core\\.alternaterefscommand)$";

pub fn repo_network_keys(cwd: &str) -> Vec<String> {
    parse::repo_keys(&run(cwd, &["config", "-z", "--includes", "--show-scope", "--get-regexp", NETWORK]).out)
}

/// The environment of fetch, pull and push: no password prompt in the terminal, only the usual transports (no remote
/// helper, local path or ext:: command).
pub fn net_env() -> Vec<(String, String)> {
    vec![("GIT_TERMINAL_PROMPT".into(), "0".into()), ("GIT_ALLOW_PROTOCOL".into(), "https:http:ssh:git".into())]
}

/// The paths of items on stdin, NUL-separated: a rename's former path before its own.
pub fn nul(items: &[Item]) -> Vec<u8> {
    let mut out = Vec::new();
    for i in items {
        // `i.orig ?`: an empty former path is none.
        if let Some(o) = i.orig.as_ref().filter(|o| !o.is_empty()) {
            out.extend_from_slice(o.as_bytes());
            out.push(0);
        }
        out.extend_from_slice(i.path.as_bytes());
        out.push(0);
    }
    if items.is_empty() {
        out.push(0);
    }
    out
}

// --- repository state

/// The status of the repository found (its folders): None when git fails there.
pub fn load_status(dirs: &Dirs) -> Option<Repo> {
    let res = run(&dirs.top, &["status", "--porcelain=v2", "-z", "--branch", "--ignore-submodules=all", "-uall"]);
    if res.code != Some(0) {
        return None;
    }
    Some(Repo {
        top: dirs.top.clone(),
        gitdir: dirs.gitdir.clone(),
        common: dirs.common.clone(),
        status: parse::parse_status(&res.out),
        state: exec::git_operation(&dirs.gitdir),
    })
}

/// The commit in the graph: its lane crossed by a heavy bar. Box-drawing characters reach the top and bottom of the
/// cell in every terminal, so the commits of a lane join up from one row to the next (● and the Nerd Font circles do
/// not, where the terminal keeps glyphs at their size). A branch tip has no line above, the first commit none below.
pub fn node(above: bool, below: bool) -> char {
    match (above, below) {
        (true, true) => '┿',
        (false, true) => '┯',
        (true, false) => '┷',
        _ => '━',
    }
}

/// The file whose history is shown, relative to the repository (git runs there), through symbolic links (/home and
/// /var/home).
pub fn hist_name(top: &str, file: &str) -> String {
    paths::relative(&exec::real_path(top), &exec::real_path(file)).split(paths::SEP).collect::<Vec<_>>().join("/")
}

/// The file's former names, shown after its name: " ← old.js", " ← 2 names".
pub fn renamed(names: &[String]) -> String {
    match names.len() {
        0 | 1 => String::new(),
        2 => format!(" ← {}", paths::posix::basename(&names[1], None)),
        n => format!(" ← {} names", n - 1),
    }
}

/// What the History view asks for: one file's history (its absolute path) or every file's, every branch or the
/// current one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryAsk {
    pub file: Option<String>,
    pub all_branches: bool,
}

/// The History view: its commits with their graph, and, for one file, the names it had (its current one first).
pub fn load_history(repo: &Repo, ask: &HistoryAsk) -> (Vec<Hist>, Vec<String>) {
    let top = repo.top.as_str();
    // One file: every name it had (--follow sees its renames, but on one line of commits only, without the graph),
    // whose commits the log below then lists.
    let mut names: Vec<String> = Vec::new();
    if let Some(file) = &ask.file {
        let name = hist_name(top, file);
        let res = run(top, &["log", "--follow", "--name-only", "-z", "--format=", "-n", "2000", "HEAD", "--", &name]);
        let found = if res.code == Some(0) { parse::parse_names(&res.out) } else { Vec::new() };
        names.push(name);
        for n in found {
            if !names.contains(&n) {
                names.push(n);
            }
        }
    }
    // Children before their parents, by date, as the graph needs. For one file, --parents links each commit to the
    // previous one that changed it, so that the graph joins them up.
    let mut args = vec![
        "log",
        "--no-show-signature",
        "--decorate=full",
        "--date-order",
        "-n",
        "500",
        "--format=%H%x1f%h%x1f%s%x1f%an%x1f%at%x1f%D%x1f%P",
    ];
    if ask.file.is_some() {
        args.push("--parents");
    }
    if ask.all_branches {
        if repo.status.has_head {
            args.push("HEAD");
        }
        args.extend(["--branches", "--remotes"]);
    }
    args.push("--");
    args.extend(names.iter().map(String::as_str));
    let (res, fork) = std::thread::scope(|s| {
        let fork = s.spawn(|| fork_point(top, repo.status.has_head));
        (run(top, &args), fork.join().ok().flatten())
    });
    let commits = if res.code != Some(0) { Vec::new() } else { parse::parse_history(&res.out) };
    let pairs: Vec<(&str, Vec<&str>)> =
        commits.iter().map(|c| (c.hash.as_str(), c.parents.iter().map(String::as_str).collect())).collect();
    let rows = crate::render::graph::layout(&pairs, fork.as_deref());
    let history = commits
        .iter()
        .zip(rows)
        .map(|(c, g)| Hist { commit: c.clone(), node: node(g.above, g.below), lane: g.color, graph: g.cells })
        .collect();
    (history, names)
}

/// Where the current branch leaves the default branch (diff::default_branch, the Diff view's main base): the
/// history's graph changes colour there. None on the default branch, or a branch it already has.
pub fn fork_point(top: &str, has_head: bool) -> Option<String> {
    if !has_head {
        return None;
    }
    let out = |args: &[&str]| {
        let r = run(top, args);
        if r.code == Some(0) { r.out.trim_matches(crate::unicode::is_space).to_string() } else { String::new() }
    };
    let name = diff::default_branch(out);
    if name.is_empty() {
        return None;
    }
    let (fork, head) = std::thread::scope(|s| {
        let fork = s.spawn(|| out(&["merge-base", "HEAD", &name]));
        let head = out(&["rev-parse", "HEAD"]);
        (fork.join().unwrap_or_default(), head)
    });
    (!fork.is_empty() && fork != head).then_some(fork)
}

pub fn load_stashes(top: &str) -> Vec<Stash> {
    let res = run(top, &["stash", "list", "--format=%gd%x1f%s%x1f%cr"]);
    if res.code != Some(0) { Vec::new() } else { parse::parse_stashes(&res.out) }
}

pub fn load_refs(top: &str) -> Refs {
    let (refs, remotes, trees) = std::thread::scope(|s| {
        let remotes = s.spawn(|| run(top, &["remote", "-v"]));
        let trees = s.spawn(|| run(top, &["worktree", "list", "--porcelain"]));
        let refs = run(
            top,
            &[
                "for-each-ref",
                "--format=%(refname)%1f%(objectname:short)%1f%(upstream:short)%1f%(upstream:track)%1f%(committerdate:relative)%1f%(contents:subject)",
                "refs/heads",
                "refs/remotes",
                "refs/tags",
            ],
        );
        let out = |h: std::thread::ScopedJoinHandle<Exec>| h.join().map(|e| e.out).unwrap_or_default();
        (refs.out, out(remotes), out(trees))
    });
    parse::parse_refs(&refs, &remotes, &trees)
}

/// What a refresh needs from S, taken in the loop before the worker runs `load`.
#[derive(Debug, Clone, Default)]
pub struct LoadAsk {
    /// The pane's folder.
    pub root: String,
    /// The repository's folders once found (None while there is none: it is looked up again).
    pub dirs: Option<Dirs>,
    pub view: &'static str,
    pub history: HistoryAsk,
    pub picker: bool,
    /// The file of the open diff, which follows it, staged or not since: its path and section.
    pub follow: Option<(String, String)>,
}

/// What `load` found. repo: None outside a repository; found: the repository was just found (its git folder is then
/// watched); the views' data when loaded; followed: the key of the diff open when asked, the file it follows now and
/// its diff.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub repo: RepoState,
    pub no_git: bool,
    pub found: bool,
    pub history: Option<(Vec<Hist>, Vec<String>)>,
    pub stashes: Option<Vec<Stash>>,
    pub refs: Option<Refs>,
    pub followed: Option<(String, Item, Diff)>,
}

impl LoadAsk {
    pub fn of(s: &State, root: &str) -> LoadAsk {
        LoadAsk {
            root: root.into(),
            dirs: s.repo().map(|r| Dirs { top: r.top.clone(), gitdir: r.gitdir.clone(), common: r.common.clone() }),
            view: s.view,
            history: HistoryAsk { file: s.hist_file.clone(), all_branches: s.all_branches },
            picker: s.picker.is_some(),
            follow: s.diff.as_ref().and_then(|d| d.item.as_ref()).map(|i| (i.path.clone(), i.sec.to_string())),
        }
    }
}

/// The repository looked up (while there is none: git init, clone in the folder), its status, and the data of the
/// view shown.
pub fn load(ask: &LoadAsk) -> Loaded {
    let mut out = Loaded::default();
    if exec::git().is_none() {
        out.repo = RepoState::None;
        out.no_git = true;
        return out;
    }
    let dirs = match &ask.dirs {
        Some(d) => d.clone(),
        None => {
            let res = exec::exec(
                exec::git(),
                &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--git-common-dir"],
                &Opts { cwd: Some(ask.root.clone()), env: Some(exec::git_env(&ask.root)), ..Default::default() },
            );
            if res.code.is_none() && (res.err.contains("ENOENT") || res.err.contains("not found")) {
                out.repo = RepoState::None;
                out.no_git = true;
                return out;
            }
            let Some(dirs) = (if res.code == Some(0) { parse::parse_repo(&res.out, &ask.root) } else { None }) else {
                out.repo = RepoState::None;
                return out;
            };
            out.found = true;
            dirs
        }
    };
    let Some(repo) = load_status(&dirs) else {
        out.repo = RepoState::None;
        return out;
    };
    match ask.view {
        "history" => out.history = Some(load_history(&repo, &ask.history)),
        "stashes" => out.stashes = Some(load_stashes(&repo.top)),
        "refs" => out.refs = Some(load_refs(&repo.top)),
        _ => {}
    }
    if ask.picker && out.refs.is_none() {
        out.refs = Some(load_refs(&repo.top));
    }
    // The open diff follows its file, staged or not since.
    if let Some((p, sec)) = &ask.follow {
        let s = &repo.status;
        let it = repo.section(sec).iter().chain(&s.conflicts).chain(&s.staged).chain(&s.changes).find(|i| &i.path == p);
        if let Some(it) = it {
            let args = item_diff_args(it, s.has_head);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            out.followed = Some((format!("{sec}:{p}"), it.clone(), diff::parse(&run(&repo.top, &args).out)));
        }
    }
    out.repo = RepoState::Some(Box::new(repo));
    out
}

/// `load`'s result applied to S.
pub fn apply(s: &mut State, l: Loaded) {
    s.no_git = s.no_git || l.no_git;
    s.repo = l.repo;
    if let Some((history, names)) = l.history {
        s.history = history;
        if s.hist_file.is_some() {
            s.hist_names = names;
        }
        s.hist_loaded = true;
    }
    if let Some(stashes) = l.stashes {
        s.stashes = stashes;
    }
    if let Some(refs) = l.refs {
        s.refs = Some(refs);
        set_picker_all(s);
    }
    if let Some((asked, it, parsed)) = l.followed {
        let key = format!("{}:{}", it.sec, it.path);
        // openItemDiff(it, true): only the diff still open on that file.
        if let Some(d) = s.diff.as_mut().filter(|d| d.item.is_some() && d.key == asked) {
            d.key = key;
            d.title = it.path.clone();
            d.tag = item_tag(&it);
            d.item = Some(it);
            d.parsed = Some(std::sync::Arc::new(parsed));
            d.cache.clear();
        }
    }
}

/// The file or folder selected in the explorer (abs), selected once the changes are loaded: the file, else the first
/// changed file of the folder. Err: the name of what has no changes.
pub fn reveal_item(repo: &Repo, abs: &str) -> Result<Item, String> {
    let rel = hist_name(&repo.top, abs);
    let inside = |p: &str| rel.is_empty() || p == rel || p.starts_with(&format!("{rel}/"));
    let s = &repo.status;
    let it = if rel.starts_with("..") {
        None
    } else {
        s.conflicts.iter().chain(&s.staged).chain(&s.changes).find(|i| inside(&i.path))
    };
    it.cloned().ok_or_else(|| {
        let b = super::context::base(&rel);
        if b.is_empty() { "the repository".into() } else { b.into() }
    })
}

// --- operations in progress

/// An operation in progress, by repository state: continue (a merge ends with a commit instead) and abort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ongoing {
    pub name: &'static str,
    pub cont: Option<[&'static str; 2]>,
    pub abort: [&'static str; 2],
}

pub fn ongoing_of(state: &str) -> Option<Ongoing> {
    Some(match state {
        "MERGING" => Ongoing { name: "merge", cont: None, abort: ["merge", "--abort"] },
        "REBASING" => Ongoing { name: "rebase", cont: Some(["rebase", "--continue"]), abort: ["rebase", "--abort"] },
        "CHERRY-PICKING" => Ongoing {
            name: "cherry-pick",
            cont: Some(["cherry-pick", "--continue"]),
            abort: ["cherry-pick", "--abort"],
        },
        "REVERTING" => Ongoing { name: "revert", cont: Some(["revert", "--continue"]), abort: ["revert", "--abort"] },
        _ => return None,
    })
}

pub fn ongoing(s: &State) -> Option<Ongoing> {
    s.repo().and_then(|r| ongoing_of(r.state))
}

/// branch.<name>.mergeOptions, options the repository adds to every git merge on the branch, --abort included (a
/// strategy of its own, see unsafeToContinue): emptied.
pub fn merge_options(s: &State) -> Vec<(String, String)> {
    match s.repo() {
        Some(r) if !r.status.branch.is_empty() && !r.status.detached => {
            vec![(format!("branch.{}.mergeoptions", r.status.branch), String::new())]
        }
        _ => Vec::new(),
    }
}

/// Continue rather than commit: always for a rebase (git keeps each commit's message), without a message of the
/// user's for a cherry-pick or a revert (with one, a commit uses it).
pub fn continues(s: &State) -> bool {
    let Some(o) = ongoing(s) else { return false };
    o.cont.is_some()
        && !s.amend
        && (s.repo().is_some_and(|r| r.state == "REBASING")
            || s.field.value.trim_matches(crate::unicode::is_space).is_empty())
}

pub fn head_name(repo: &Repo) -> &str {
    if repo.status.detached { "HEAD" } else { &repo.status.branch }
}

// --- branches

/// The picker's branches, from the refs just loaded: the local ones (the current one marked) then the remote ones;
/// merge and rebase leave out the current one.
fn set_picker_all(s: &mut State) {
    let (Some(p), Some(refs)) = (s.picker.as_mut(), s.refs.as_ref()) else { return };
    let cur = s.repo.get().map_or("", |r| r.status.branch.as_str());
    let heads = refs.heads.iter().map(|b| Pick { branch: b.clone(), current: b.name == cur });
    let remotes = refs.remotes.iter().map(|b| Pick { branch: b.clone(), current: false });
    p.all = heads.chain(remotes).filter(|b| p.mode.is_none() || !b.current).collect();
    filter_picker(p);
}

pub fn filter_picker(p: &mut Picker) {
    let q = p.field.value.to_lowercase();
    let mut items = Vec::new();
    if p.mode.is_none() {
        items.push(PickItem::Create(if q.is_empty() {
            "New branch…".into()
        } else {
            format!("New branch \"{}\"…", p.field.value)
        }));
    }
    items.extend(
        p.all
            .iter()
            .filter(|b| q.is_empty() || b.branch.name.to_lowercase().contains(&q))
            .cloned()
            .map(PickItem::Branch),
    );
    p.items = items;
    p.sel = 0.max(p.sel.min(p.items.len() as i64 - 1));
}

/// The picker opened: mode "merge" or "rebase" (the menu's Merge a branch… and Rebase onto a branch…), else it
/// switches; its first branch selected once loaded (`picker_loaded`).
pub fn open_picker(s: &mut State, mode: Option<&'static str>, anchor: Option<(i64, i64)>) {
    if s.repo().is_none() {
        return;
    }
    let mut p = Picker { mode, anchor: mode.and(anchor), sel: first_pick(mode), ..Picker::default() };
    filter_picker(&mut p);
    s.picker = Some(p);
}

fn first_pick(mode: Option<&str>) -> i64 {
    if mode.is_some() { 0 } else { 1 }
}

/// The picker's refs loaded: its branches, and its first one selected (unless a name is typed in place meanwhile:
/// inline).
pub fn picker_loaded(s: &mut State, refs: Refs, inline: bool) {
    s.refs = Some(refs);
    set_picker_all(s);
    if let Some(p) = s.picker.as_mut().filter(|_| !inline) {
        p.sel = 0.max(first_pick(p.mode).min(p.items.len() as i64 - 1));
    }
}

// --- branch comparison (Refs)

/// Full names: a branch called like an option, or like a commit, stays a branch for git.
pub fn full_ref(b: &Branch) -> String {
    format!("refs/{}/{}", if b.remote { "remotes" } else { "heads" }, b.name)
}

pub fn base_ref(s: &State) -> String {
    s.compare_base.as_ref().map_or("HEAD".into(), full_ref)
}

pub fn base_name(s: &State) -> String {
    match (&s.compare_base, s.repo()) {
        (Some(b), _) => b.name.clone(),
        (None, Some(r)) => head_name(r).to_string(),
        (None, None) => String::new(),
    }
}

/// Commits listed on each side.
pub const MAX_COMPARE: usize = 300;

/// The comparison of a branch (row) opened: shown at once, filled by `compare`.
pub fn compare_view(s: &State, row: &Row) -> Option<DiffView> {
    let b = row.branch()?;
    Some(DiffView {
        key: row.key.clone(),
        compare: true,
        title: b.name.clone(),
        base: base_name(s),
        sel: -1,
        focus: s.diff.as_ref().is_some_and(|d| d.focus),
        ..DiffView::default()
    })
}

/// The commits only on the branch, those only on the base (= when the same change is on both sides, as after a
/// cherry-pick), and the files changed on the branch since it left the base. range: base...branch.
pub fn compare(top: &str, range: &str) -> Cmp {
    let max = MAX_COMPARE.to_string();
    let side = |only: &str| {
        run(
            top,
            &[
                "log",
                "--no-show-signature",
                "--no-ext-diff",
                "--no-textconv",
                only,
                "--cherry-mark",
                "-n",
                &max,
                "--format=%m%x1f%h%x1f%H%x1f%s%x1f%at",
                range,
                "--",
            ],
        )
    };
    let (count, mine, theirs, stat) = std::thread::scope(|sc| {
        let mine = sc.spawn(|| side("--right-only"));
        let theirs = sc.spawn(|| side("--left-only"));
        let stat = sc.spawn(|| {
            let mut args: Vec<&str> = vec!["diff"];
            args.extend(DIFF_OPTS);
            args.extend(["--numstat", "-M", range, "--"]);
            run(top, &args)
        });
        let count = run(top, &["rev-list", "--left-right", "--count", range, "--"]);
        let get = |h: std::thread::ScopedJoinHandle<Exec>| {
            h.join().unwrap_or(Exec { code: None, out: String::new(), err: String::new() })
        };
        (count, get(mine), get(theirs), get(stat))
    });
    let counts = parse::parse_count(&count.out);
    // `x || 0`: a missing or NaN count is 0.
    let n = |i: usize| counts.get(i).copied().filter(|x| !x.is_nan()).unwrap_or(0.0);
    Cmp {
        behind: n(0),
        ahead: n(1),
        mine: parse::parse_compare(&mine.out),
        theirs: parse::parse_compare(&theirs.out),
        files: parse::parse_numstat(&stat.out),
        failed: (count.code != Some(0)).then(|| error_of(&count)),
    }
}

// --- diffs

/// The git diff of a file of the status: staged, untracked (against nothing), a conflict (against HEAD, when there is
/// one) or unstaged.
pub fn item_diff_args(it: &Item, has_head: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["diff".into()];
    args.extend(DIFF_OPTS.iter().map(|s| s.to_string()));
    args.push("-M".into());
    if it.sec == "staged" {
        args.extend(["--cached".into(), "--".into()]);
        args.extend(it.orig.clone().filter(|o| !o.is_empty()));
    } else if it.untracked {
        args.extend(["--no-index".into(), "--".into(), "/dev/null".into()]);
    } else if it.sec == "conflicts" && has_head {
        args.extend(["HEAD".into(), "--".into()]);
    } else {
        args.push("--".into());
    }
    args.push(it.path.clone());
    args
}

/// The tag shown with a file's diff.
pub fn item_tag(it: &Item) -> &'static str {
    if it.sec == "staged" {
        "staged"
    } else if it.untracked {
        "untracked"
    } else if it.sec == "conflicts" {
        "conflict"
    } else {
        ""
    }
}

/// A diff opened (showDiff): the one open kept when it has the same key (its scroll and focus), filled once git
/// answers (`show_diff_done`).
pub fn show_diff(s: &mut State, key: &str, title: &str, item: Option<Item>, tag: &'static str) {
    let focus = s.diff.as_ref().is_some_and(|d| d.focus);
    let mut d = match s.diff.take() {
        Some(d) if d.key == key => d,
        _ => DiffView { key: key.into(), focus, ..DiffView::default() },
    };
    d.item = item;
    d.title = title.into();
    d.tag = tag;
    s.diff = Some(d);
}

/// git's answer for the diff opened with key: dropped when another diff is open since.
pub fn show_diff_done(s: &mut State, key: &str, parsed: Diff) {
    if let Some(d) = s.diff.as_mut().filter(|d| d.key == key && !d.compare) {
        d.parsed = Some(std::sync::Arc::new(parsed));
        d.cache.clear();
    }
}

/// From SPLIT columns, the diff shows the selected file, commit, stash or tag (shortly after, when moving fast). A
/// branch other than the base shows its comparison with the base.
pub fn comparable(s: &State, row: Option<&Row>) -> bool {
    let Some(row) = row.filter(|r| r.kind == "branch") else { return false };
    let Some(b) = row.branch() else { return false };
    base_ref(s) != full_ref(b) && !(row.current && s.compare_base.is_none())
}

pub fn diffable(s: &State, row: Option<&Row>) -> bool {
    row.is_some_and(|r| ["file", "commit", "stash", "tag", "pull"].contains(&r.kind)) || comparable(s, row)
}

/// The diff on the right of the list stands for this row: its own, or a commit opened from its comparison.
pub fn showing(s: &State, row: &Row) -> bool {
    s.diff.as_ref().is_some_and(|d| d.key == row.key || d.back.as_ref().is_some_and(|b| b.key == row.key))
}

// --- views

/// Whether the view changed (the pane then draws and refreshes).
pub fn set_view(s: &mut State, id: &'static str) -> bool {
    if s.view == id {
        return false;
    }
    s.view = id;
    s.cursor = 0;
    s.top = 0;
    s.sel_key = None;
    s.diff = None;
    true
}

/// Back to the history of every file: the message to flash.
pub fn all_files(s: &mut State) -> &'static str {
    s.hist_file = None;
    s.hist_names.clear();
    s.hist_loaded = false;
    s.cursor = 0;
    s.top = 0;
    s.sel_key = None;
    s.diff = None;
    "History of every file"
}

/// A setting written for the next panes.
fn save(key: &str, on: bool) {
    let mut settings = exec::read_settings();
    settings.set(key, Value::Bool(on));
    exec::write_settings(&settings);
}

/// History of every branch or of the current one, kept for the next panes: the message to flash.
pub fn toggle_all_branches(s: &mut State) -> &'static str {
    s.all_branches = !s.all_branches;
    save("scmGraphAll", s.all_branches);
    if s.all_branches { "All branches" } else { "Current branch only" }
}

/// The list or the folder tree, kept for the next source control panes.
pub fn toggle_tree(s: &mut State) {
    s.tree = !s.tree;
    save("scmTree", s.tree);
}

/// A section or folder folded or unfolded.
pub fn toggle(s: &mut State, row: &Row, open: Option<bool>) {
    if open.unwrap_or(!row.open) {
        s.collapsed.remove(&row.key);
    } else {
        s.collapsed.insert(row.key.clone());
    }
}
