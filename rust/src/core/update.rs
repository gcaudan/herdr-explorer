//! New releases of the plugin (src/core/update.js): a check of GitHub once a day (from the hook, never from the
//! panes), the version found kept in the state folder, and the update itself: the clone moved to the release's tag
//! and the release's binary put in bin/, checked against its SHA256SUMS. The explorer and preview panes restart on the
//! new code by themselves when the version on disk changes (`VersionWatch`).
//!
//! As the JS, but: the update follows the release tags instead of the head of the
//! default branch; it downloads and checks the binary before touching the clone, and refuses when it cannot; it holds
//! bin/.fetch.lock (scripts/fetch-bin.sh's rule) from the move of the clone until the binary is put. GitHub is asked
//! through curl, as fetch-bin does (no crate).

use crate::core::exec::{self, Opts, WIN};
use crate::core::regex::Regex;
use crate::core::watch::Watch;
use crate::core::{js, paths, which};
use crate::json::{self, Value};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant};

/// The GitHub repository the releases come from when the clone's origin is not on GitHub.
pub const REPO: &str = "gcaudan/herdr-explorer";
const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
/// A bin/.fetch.lock this old is a dead fetch's (scripts/fetch-bin.sh's rule).
const STALE: Duration = Duration::from_secs(2 * 60);
/// An update.lock this old is a dead update's: longer than an update can take (two downloads of 3 min, the wait for
/// bin/.fetch.lock, git's fetch and move).
const UPDATE_STALE: Duration = Duration::from_secs(15 * 60);

fn re(src: &str, flags: &str) -> Regex {
    Regex::new(src, flags).expect("a valid pattern")
}

/// The plugin's folder, a git clone: the one above the binary's bin/ when it holds the manifest, else
/// HERDR_PLUGIN_ROOT (a binary run from elsewhere, cargo's target among them). None: no update.
pub fn dir() -> Option<&'static str> {
    static DIR: OnceLock<Option<String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let has = |d: &str| std::fs::metadata(paths::join(&[d, "herdr-plugin.toml"])).is_ok_and(|m| m.is_file());
        let exe = std::env::current_exe().ok().map(|p| exec::strip_verbatim(p.to_string_lossy().into_owned()));
        let bin = exe.map(|e| paths::dirname(&e)).filter(|b| paths::basename(b, None) == "bin");
        bin.map(|b| paths::dirname(&b))
            .filter(|d| has(d))
            .or_else(|| exec::env_var("HERDR_PLUGIN_ROOT").filter(|d| paths::is_absolute(d) && has(d)))
    })
    .as_deref()
}

fn manifest() -> Option<String> {
    dir().map(|d| paths::join(&[d, "herdr-plugin.toml"]))
}

/// The command that updates a plugin herdr installed.
pub fn herdr_update() -> String {
    format!("herdr plugin update {}", exec::plugin())
}

static REPO_URL: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(?:https:\/\/|ssh:\/\/git@|git@)github\.com[:/]([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$", ""));
static DOTS: LazyLock<Regex> = LazyLock::new(|| re(r"^\.+$", ""));

/// The GitHub repository of a clone's origin URL ("owner/name"), None for another host or a malformed URL: a fork
/// follows its own releases.
pub fn repo_of(url: &str) -> Option<String> {
    let m = REPO_URL.exec(url.trim_matches(js::is_space))?;
    let (owner, name) = (m.get(1)?, m.get(2)?);
    (!DOTS.test(owner) && !DOTS.test(name)).then(|| format!("{owner}/{name}"))
}

static VERSION: LazyLock<Regex> = LazyLock::new(|| re(r#"^version\s*=\s*"([^"]+)""#, "m"));

/// The version in the manifest on disk.
pub fn read_version() -> Option<String> {
    version_in(dir()?)
}

fn version_in(dir: &str) -> Option<String> {
    let text = exec::read_text(paths::join(&[dir, "herdr-plugin.toml"])).ok()?;
    VERSION.exec(&text).and_then(|m| m.get(1)).map(String::from)
}

/// The version of the code this process runs: the manifest's when the process started (`init`).
pub fn current() -> Option<&'static str> {
    static CURRENT: OnceLock<Option<String>> = OnceLock::new();
    CURRENT.get_or_init(read_version).as_deref()
}

/// Reads the version this process runs, at its start, as the JS's module did when loaded.
pub fn init() {
    let _ = current();
}

/// { checkedAt, repo, latest, notes, url, skip, updated, finish } in the state folder: the repository checked, the
/// latest release, its changes and page, the version the user skipped, the last update done here ({ from, to, t }),
/// and the one to finish.
pub fn file() -> Option<String> {
    exec::state_dir().map(|d| paths::join(&[&d, "update.json"]))
}

fn read() -> Value {
    read_at(file().as_deref())
}

fn read_at(file: Option<&str>) -> Value {
    let v = file.and_then(|f| exec::read_text(f).ok()).and_then(|t| json::parse(&t).ok());
    match v {
        Some(v @ Value::Obj(_)) => v,
        // `JSON.parse(…) || {}`: an array or another truthy value is kept, its fields then missing.
        _ => Value::Obj(Vec::new()),
    }
}

fn write(data: &Value) {
    if let Some(f) = file() {
        exec::write_atomic(&f, json::stringify(data));
    }
}

/// `{ ...read(), ...fields }`.
fn write_with(fields: Vec<(&str, Value)>) {
    write_at(file().as_deref(), fields);
}

fn write_at(file: Option<&str>, fields: Vec<(&str, Value)>) {
    let Some(file) = file else { return };
    let mut data = read_at(Some(file));
    for (k, v) in fields {
        data.set(k, v);
    }
    exec::write_atomic(file, json::stringify(&data));
}

static ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*[*-]\s+(.+)$", ""));
static COMMIT_LINK: LazyLock<Regex> = LazyLock::new(|| re(r"\s*\(\[[^\]]*\]\([^)]*\)\)", "g"));
static LINK: LazyLock<Regex> = LazyLock::new(|| re(r"\[([^\]]*)\]\([^)]*\)", "g"));
static BOLD: LazyLock<Regex> = LazyLock::new(|| re(r"\*\*([^*]+)\*\*", "g"));
static MARKS: LazyLock<Regex> = LazyLock::new(|| re(r"[`*_]", "g"));
static CONTROLS: LazyLock<Regex> = LazyLock::new(|| re(r"[\x00-\x1f\x7f-\x9f]", "g"));

/// The changes listed in a release's text (release-please: "* **scope:** text ([#12](…)) ([abc123](…))"), as plain
/// lines: the links and the emphasis dropped, the scope kept, 20 at most.
pub fn release_notes(body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for l in body.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        let Some(t) = ITEM.exec(l).and_then(|m| m.get(1)) else { continue };
        let t = COMMIT_LINK.replace_with(t, true, "");
        let first = |m: &crate::core::regex::Match| m.get(1).unwrap_or("").to_string();
        let t = LINK.replace(&t, true, first);
        let t = BOLD.replace(&t, true, first);
        let t = MARKS.replace_with(&t, true, "");
        let t = CONTROLS.replace_with(&t, true, " ");
        let t = t.trim_matches(js::is_space);
        if !t.is_empty() {
            lines.push(t.to_string());
        }
    }
    lines.truncate(20);
    lines
}

/// curl from PATH (curl.exe on Windows: PowerShell's curl is another program).
fn curl() -> Option<String> {
    which::which(if WIN { "curl.exe" } else { "curl" })
}

static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"^\d+(\.\d+)*$", ""));
static PAGE: LazyLock<Regex> = LazyLock::new(|| re(r"^https:\/\/github\.com\/[\w./%-]+$", ""));

/// The latest GitHub release, (latest, notes, url), or None (offline, timeout, no release, curl missing). Redirects
/// are not followed, as Node's https.get.
fn latest_release(repo: &str) -> Option<(String, Vec<String>, String)> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let args = ["-fsS", "--max-time", "3", "-A", "herdr-explorer", "-H", "Accept: application/vnd.github+json", &url];
    let body = exec::run(curl().as_deref(), &args, &Opts { timeout: Some(Duration::from_secs(5)), ..Opts::default() })?;
    let release = json::parse(&body).ok().filter(Value::truthy).unwrap_or(Value::Obj(Vec::new()));
    let field = |k: &str| release.get(k).map_or_else(|| "undefined".to_string(), js::string);
    let tag = field("tag_name");
    let tag = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let page = field("html_url");
    let url = if PAGE.test(&page) { page } else { format!("https://github.com/{repo}/releases") };
    let body = release.get("body").filter(|b| b.truthy()).map(js::string).unwrap_or_default();
    TAG.test(&tag).then(|| (tag, release_notes(&body), url))
}

/// At most once a day, and not when the setting is off. A failed check is not retried before the next day either.
pub fn check() {
    if file().is_none() || exec::read_settings().get("updateCheck") == Some(&Value::Bool(false)) {
        return;
    }
    let at = read().get("checkedAt").filter(|v| v.truthy()).map_or(0.0, |v| js::number(Some(v)));
    if js::now_ms() - at < DAY {
        return;
    }
    let origin = git(&["config", "--get", "remote.origin.url"], Duration::from_secs(10), &[]);
    let repo = origin.ok.then(|| repo_of(&origin.out)).flatten().unwrap_or_else(|| REPO.into());
    let release = latest_release(&repo);
    let mut fields = vec![("checkedAt", Value::from(js::now_ms())), ("repo", Value::from(repo.as_str()))];
    if let Some((latest, notes, url)) = release {
        fields.extend([("latest", latest.into()), ("notes", notes.into()), ("url", url.into())]);
    }
    write_with(fields);
}

/// The newer release found, (latest, skipped), or None: compared with the version on disk, so that it goes away once
/// installed. A clone ahead of the release (development) shows nothing. skipped: the one the user skipped (skip).
fn newer_release() -> Option<(String, bool)> {
    file()?;
    let data = read();
    let latest = data.get("latest").filter(|v| v.truthy())?;
    let current = read_version()?;
    let text = js::string(latest);
    exec::newer(&text, &current).then(|| (text, js::strict_eq(data.get("skip"), Some(latest))))
}

/// The newer release to show, unless skipped.
pub fn available() -> Option<String> {
    newer_release().filter(|r| !r.1).map(|r| r.0)
}

/// The newer release the user skipped, still installable from the settings.
pub fn skipped() -> Option<String> {
    newer_release().filter(|r| r.1).map(|r| r.0)
}

/// The user skips a version: no ↑ for it, in every explorer; the next one shows again.
pub fn skip(version: &str) {
    write_with(vec![("skip", version.into())]);
}

/// The latest release's changes and page; without a page, the releases of the repository checked.
pub fn release() -> (Vec<String>, String) {
    let data = if file().is_some() { read() } else { Value::Obj(Vec::new()) };
    let repo = data.get("repo").and_then(Value::as_str).and_then(|r| repo_of(&format!("https://github.com/{r}")));
    let releases = format!("https://github.com/{}/releases", repo.as_deref().unwrap_or(REPO));
    let notes =
        data.get("notes").and_then(Value::as_array).map_or_else(Vec::new, |n| n.iter().map(js::string).collect());
    let url = data.get("url").and_then(Value::as_str).map_or(releases, String::from);
    (notes, url)
}

/// The update done here that this code comes from, for a minute after it: the version it went to.
pub fn just_updated() -> Option<String> {
    file()?;
    let data = read();
    let updated = data.get("updated").filter(|u| u.truthy())?;
    let to = updated.get("to").and_then(Value::as_str)?;
    let t = js::number(updated.get("t"));
    (Some(to) == current() && js::now_ms() - t < 60.0 * 1000.0).then(|| to.to_string())
}

/// What git gave: whether it succeeded, its stdout and stderr.
struct Git {
    ok: bool,
    out: String,
    err: String,
}

/// git in the plugin's folder. detached (not on Windows): a session without terminal, so that ssh or a credential
/// helper can never prompt in the pane. GCM_INTERACTIVE: Git Credential Manager (Windows) otherwise opens its sign-in
/// window, GIT_TERMINAL_PROMPT not reaching it, and the update waits for it until the timeout.
fn git(args: &[&str], timeout: Duration, extra: &[(&str, &str)]) -> Git {
    git_in(dir(), args, timeout, extra)
}

fn git_in(dir: Option<&str>, args: &[&str], timeout: Duration, extra: &[(&str, &str)]) -> Git {
    let (Some(git), Some(dir)) = (exec::git(), dir) else {
        return Git { ok: false, out: String::new(), err: "git not found".into() };
    };
    let mut env = exec::process_env();
    for (k, v) in [("GIT_TERMINAL_PROMPT", "0"), ("GCM_INTERACTIVE", "never")].iter().chain(extra) {
        exec::env_set(&mut env, k, v);
    }
    let opts =
        Opts { cwd: Some(dir.into()), env: Some(env), timeout: Some(timeout), detached: !WIN, ..Opts::default() };
    let r = exec::exec(Some(git), args, &opts);
    Git { ok: r.code == Some(0), out: r.out, err: r.err }
}

fn same_path(a: &str, b: &str) -> bool {
    if WIN { a.to_lowercase() == b.to_lowercase() } else { a == b }
}

/// One update at a time, across the explorers of every tab: a lock file in the state folder, taken over after 15 min
/// (a process killed during the update; 2 min in the JS, shorter than an update with downloads). Without a state folder, or when it cannot be written, no lock.
fn lock_file() -> Option<String> {
    file().map(|f| paths::join(&[&paths::dirname(&f), "update.lock"]))
}

fn lock() -> bool {
    let Some(lock) = lock_file() else { return true };
    let _ = std::fs::create_dir_all(paths::dirname(&lock));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
        Ok(_) => return true,
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return true,
        Err(_) => {}
    }
    match std::fs::metadata(&lock).and_then(|m| m.modified()) {
        Ok(t) if t.elapsed().unwrap_or_default() < UPDATE_STALE => false,
        Ok(_) => std::fs::write(&lock, "").is_ok(),
        Err(_) => false,
    }
}

/// Installed by `herdr plugin install` rather than linked, as herdr reports it (source kind "github", "local" when
/// linked): a checkout herdr manages (detached HEAD), which only `herdr plugin update` may move; that update installs
/// into a new folder, so the open panes stay on this code. false when herdr does not answer.
pub fn managed() -> bool {
    let plugin = exec::plugin();
    let opts = Opts { timeout: Some(Duration::from_secs(5)), ..Opts::default() };
    let out = exec::run(exec::herdr(), &["plugin", "list", "--plugin", &plugin, "--json"], &opts);
    let res = exec::result(out.as_deref());
    let found = res.get("plugins").and_then(Value::as_array).and_then(|list| {
        list.iter().find(|p| p.get("plugin_id").and_then(Value::as_str) == Some(plugin.as_str())).cloned()
    });
    found.and_then(|p| p.get("source")?.get("kind").cloned()) == Some(Value::from("github"))
}

/// The command that runs the installer again, to finish an update that changed the manifest or the installer.
pub fn installer() -> String {
    let dir = dir().unwrap_or("");
    if WIN {
        format!("powershell -ExecutionPolicy Bypass -File \"{}\"", paths::join(&[dir, "install.ps1"]))
    } else {
        format!("\"{}\"", paths::join(&[dir, "install.sh"]))
    }
}

/// The version whose update the installer must finish, or None: kept in update.json, so that every explorer shows it
/// after the restart, until the command is taken (finished).
pub fn finishing() -> Option<String> {
    file()?;
    read().get("finish").filter(|f| f.truthy()).map(js::string)
}

pub fn finished() {
    let mut data = read();
    if data.remove("finish").is_some_and(|f| f.truthy()) {
        write(&data);
    }
}

/// What an update gives: done, its message, and the command that does it in a terminal when that may succeed (a
/// passphrase to type, the network back); a refusal has none.
pub struct Outcome {
    pub ok: bool,
    pub message: String,
    pub retry: Option<String>,
}

fn refused(message: impl Into<String>) -> Outcome {
    Outcome { ok: false, message: message.into(), retry: None }
}

/// The clone moved to the tag of the release `to`, with its binary: see the module. The installer is not run again:
/// it unlinks and links the plugin, with an unknown effect on the panes running; the user does it when it changed.
pub fn install(to: &str) -> Outcome {
    if !lock() {
        return refused("Update in progress");
    }
    let outcome = match dir() {
        // The plugin's own clone, not a repository holding a copy of it (dotfiles), whose update would run its hooks.
        None => refused("Not a git clone"),
        Some(dir) => update(&Ctx { dir: dir.into(), curl: curl(), file: file() }, to),
    };
    if let Some(lock) = lock_file() {
        let _ = std::fs::remove_file(lock);
    }
    outcome
}

/// The release asset of the binary for this platform: Windows on ARM64 runs the x86_64 one.
fn asset() -> Option<&'static str> {
    if WIN {
        Some("herdr-explorer-x86_64-pc-windows-msvc.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("herdr-explorer-x86_64-unknown-linux-musl")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("herdr-explorer-aarch64-unknown-linux-musl")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("herdr-explorer-x86_64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("herdr-explorer-aarch64-apple-darwin")
    } else {
        None
    }
}

/// The first error line of git's stderr, without its "fatal:", else what failed.
fn git_error(err: &str, fallback: &str) -> String {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^(fatal|error)\s*:\s*", ""));
    let lines: Vec<&str> =
        err.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).filter(|l| !l.is_empty()).collect();
    let error = lines.iter().find(|l| PREFIX.test(l)).or(lines.first());
    error.map_or_else(|| fallback.into(), |e| PREFIX.replace_with(e, false, ""))
}

/// Where an update works: the plugin's folder, curl, update.json.
struct Ctx {
    dir: String,
    curl: Option<String>,
    file: Option<String>,
}

impl Ctx {
    fn git(&self, args: &[&str], timeout: Duration, extra: &[(&str, &str)]) -> Git {
        git_in(Some(&self.dir), args, timeout, extra)
    }
}

fn update(ctx: &Ctx, to: &str) -> Outcome {
    let dir = ctx.dir.as_str();
    if exec::git().is_none() {
        return refused("git not found in PATH");
    }
    let t10 = Duration::from_secs(10);
    let top = ctx.git(&["rev-parse", "--show-toplevel"], t10, &[]);
    if !top.ok || !same_path(&exec::real_path(top.out.trim()), &exec::real_path(dir)) {
        return refused("Not a git clone");
    }
    let gitdir = ctx.git(&["rev-parse", "--absolute-git-dir"], t10, &[]);
    if !gitdir.ok || !exec::git_operation(gitdir.out.trim()).is_empty() {
        return refused("A git operation is in progress");
    }
    let status = ctx.git(&["status", "--porcelain", "--untracked-files=no"], t10, &[]);
    if !status.ok || !status.out.trim().is_empty() {
        return refused("Local changes in the plugin folder");
    }
    let before = ctx.git(&["rev-parse", "HEAD"], t10, &[]).out.trim().to_string();
    let from = version_in(dir);

    // The binary first, into a temporary file of bin/: without it, the clone is left alone. A development binary
    // (bin/.dev) is never replaced.
    let bin = paths::join(&[dir, "bin"]);
    let exe = paths::join(&[&bin, if WIN { "herdr-explorer.exe" } else { "herdr-explorer" }]);
    let dev = std::fs::metadata(paths::join(&[&bin, ".dev"])).is_ok();
    let has = exec::run(Some(&exe), &["--version"], &Opts { timeout: Some(t10), ..Opts::default() });
    let tmp = paths::join(&[&bin, &format!(".fetch-{}", std::process::id())]);
    let fetch = !dev && has.as_deref().map(str::trim) != Some(to);
    if fetch && let Err(message) = download(ctx, to, &bin, &tmp) {
        remove_temps(&tmp);
        return refused(message);
    }

    // From here, bin/.fetch.lock: a shim or fetch-bin started meanwhile waits, then finds the manifest and the binary
    // of the same version.
    let held = fetch_lock(&bin);
    let lock = paths::join(&[&bin, ".fetch.lock"]);
    let head = ctx.git(&["symbolic-ref", "-q", "--short", "HEAD"], t10, &[]);
    let moved = move_clone(ctx, to);
    // Refreshed: git's fetch and move may have taken longer than a fetch-bin waits before taking a lock over.
    if held && let Ok(f) = std::fs::OpenOptions::new().write(true).open(&lock) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    let placed = match &moved {
        Ok(_) if fetch => place(&tmp, &exe),
        _ => Ok(()),
    };
    // The binary not put: the clone moved back, so that the code and the binary stay of the same version (and the
    // panes do not restart on a version without its binary, hiding the failure).
    if moved.is_ok() && placed.is_err() && !before.is_empty() {
        let back: Vec<&str> = if head.ok && !head.out.trim().is_empty() {
            vec!["reset", "--keep", &before]
        } else {
            vec!["checkout", "--detach", &before]
        };
        let _ = ctx.git(&back, t10, &[]);
    }
    remove_temps(&tmp);
    if held {
        let _ = std::fs::remove_file(&lock);
    }
    if let Err((message, steps)) = moved {
        return Outcome { ok: false, message, retry: Some(manual(dir, steps)) };
    }
    if placed.is_err() {
        let message = format!("The {to} binary could not be put in bin");
        let steps = moved.unwrap_or_default();
        return Outcome { ok: false, message, retry: Some(manual(dir, steps)) };
    }

    let after = ctx.git(&["rev-parse", "HEAD"], t10, &[]).out.trim().to_string();
    if before.is_empty() || after == before {
        return Outcome { ok: true, message: "Already up to date".into(), retry: None };
    }
    // The manifest (other than its version) or the installer changed: herdr and the installer must take it.
    let diff =
        ctx.git(&["diff", "-U0", &before, &after, "--", "herdr-plugin.toml", "install.sh", "install.ps1"], t10, &[]);
    static VERSION_LINE: LazyLock<Regex> = LazyLock::new(|| re(r"^[+-]version\s*=", ""));
    let changed = !diff.ok
        || diff.out.split('\n').any(|l| {
            (l.starts_with('+') || l.starts_with('-'))
                && !(l.starts_with("+++") || l.starts_with("---"))
                && !VERSION_LINE.test(l)
        });
    let now = version_in(dir);
    // Rewritten even when unchanged, so that the explorers of the other tabs read it again and hide their ↑; updated
    // for the explorers that restart on the new code.
    if ctx.file.is_some() {
        let updated = Value::Obj(vec![
            ("from".into(), from.into()),
            ("to".into(), now.clone().into()),
            ("t".into(), js::now_ms().into()),
        ]);
        let mut fields = vec![("updated", updated)];
        if changed {
            fields.push(("finish", now.clone().into()));
        }
        write_at(ctx.file.as_deref(), fields);
    }
    let now = now.unwrap_or_else(|| "undefined".into());
    if !changed {
        return Outcome { ok: true, message: format!("Updated to {now}"), retry: None };
    }
    Outcome { ok: true, message: format!("Updated to {now}: rerun the installer"), retry: None }
}

/// The command that does the rest of a failed update in a terminal: the git steps from the one that failed, then
/// fetch-bin, each run once the one before succeeded. On Windows in PowerShell's syntax (Windows PowerShell 5.1 has no
/// &&).
fn manual(dir: &str, mut steps: Vec<String>) -> String {
    steps.push(if WIN {
        format!(
            "powershell -ExecutionPolicy Bypass -File \"{}\" \"{dir}\"",
            paths::join(&[dir, "scripts", "fetch-bin.ps1"])
        )
    } else {
        format!("sh \"{}\" \"{dir}\"", paths::join(&[dir, "scripts", "fetch-bin.sh"]))
    });
    chain(&steps, WIN)
}

fn chain(steps: &[String], win: bool) -> String {
    match steps {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] if win => format!("{first}; if ($?) {{ {} }}", chain(rest, win)),
        _ => steps.join(" && "),
    }
}

/// The git commands that move the clone to v<to>: fetch first when asked, then a fast-forward when on a branch the tag
/// follows, else a detached checkout.
fn move_steps(dir: &str, to: &str, fetch: bool, merge: bool) -> Vec<String> {
    let quoted = format!("git -C \"{dir}\"");
    let step =
        if merge { format!("{quoted} merge --ff-only v{to}") } else { format!("{quoted} checkout --detach v{to}") };
    if fetch { vec![format!("{quoted} fetch --tags"), step] } else { vec![step] }
}

fn remove_temps(tmp: &str) {
    let _ = std::fs::remove_file(tmp);
    let _ = std::fs::remove_file(format!("{tmp}.sums"));
}

/// The release's binary for this platform into tmp, checked against the release's SHA256SUMS; else why not.
fn download(ctx: &Ctx, to: &str, bin: &str, tmp: &str) -> Result<(), String> {
    let Some(asset) = asset() else { return Err("No release binary for this platform".into()) };
    let Some(curl) = ctx.curl.clone() else { return Err("curl not found in PATH".into()) };
    let origin = ctx.git(&["config", "--get", "remote.origin.url"], Duration::from_secs(10), &[]);
    let repo = origin.ok.then(|| repo_of(&origin.out)).flatten().unwrap_or_else(|| REPO.into());
    let url = format!("https://github.com/{repo}/releases/download/v{to}");
    std::fs::create_dir_all(bin).map_err(|_| format!("Cannot write in {bin}"))?;
    let opts = Opts { timeout: Some(Duration::from_secs(180)), ..Opts::default() };
    let get = |name: &str, out: &str| {
        exec::run(Some(&curl), &["-fsSL", "--max-time", "170", "-o", out, &format!("{url}/{name}")], &opts).is_some()
    };
    let sums = format!("{tmp}.sums");
    if !get(asset, tmp) || !get("SHA256SUMS", &sums) {
        return Err(format!("Cannot download the {to} binary"));
    }
    let want = exec::read_text(&sums).ok().and_then(|text| {
        text.lines().find_map(|l| {
            let mut parts = l.split_whitespace();
            let (hash, name) = (parts.next()?, parts.next()?);
            (name == asset || name.strip_prefix('*') == Some(asset)).then(|| hash.to_string())
        })
    });
    let want = want.filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    let Some(want) = want else { return Err(format!("{asset} is not in the {to} SHA256SUMS")) };
    let got = std::fs::read(tmp).map(|b| hex(&sha256(&b))).unwrap_or_default();
    if got != want {
        return Err(format!("The {to} binary does not match its SHA256SUMS"));
    }
    Ok(())
}

/// bin/.fetch.lock taken, as scripts/fetch-bin.sh takes it: waited for while another fetch holds it, taken over when
/// older than 2 min. false when it cannot be made (bin/ not writable): the update goes on without it.
fn fetch_lock(bin: &str) -> bool {
    let lock = paths::join(&[bin, ".fetch.lock"]);
    let _ = std::fs::create_dir_all(bin);
    let mut tries = 0;
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            // Another error with a lock there (on Windows, one being removed): waited for as a held one.
            Err(_) if std::fs::symlink_metadata(&lock).is_ok() => {}
            Err(_) => {
                tries += 1;
                if tries >= 3 {
                    return false;
                }
                continue;
            }
        }
        let old = |p: &str| {
            std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > STALE)
        };
        let taken = format!("{lock}.{}", std::process::id());
        if old(&lock) && std::fs::rename(&lock, &taken).is_ok() {
            // Still that old: the dead fetch's, removed; else another fetch's new lock, put back.
            if old(&taken) || std::fs::rename(&taken, &lock).is_err() {
                let _ = std::fs::remove_file(&taken);
            }
            continue;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// The clone moved to the tag v<to>: fast-forwarded when on a branch the tag follows, else checked out detached.
/// Ok: the git command that moved it. Err: the message and the git commands to run in a terminal.
fn move_clone(ctx: &Ctx, to: &str) -> Result<Vec<String>, (String, Vec<String>)> {
    let (dir, git) = (ctx.dir.as_str(), |args: &[&str], t, extra: &[(&str, &str)]| ctx.git(args, t, extra));
    let tag = format!("v{to}");
    // ssh in batch mode: GIT_TERMINAL_PROMPT does not reach it, and on Windows git shares the pane's console, where a
    // passphrase prompt would read the keys. Left alone when the user sets an ssh command.
    let own = exec::env_var("GIT_SSH_COMMAND").is_some()
        || exec::env_var("GIT_SSH").is_some()
        || git(&["config", "--get", "core.sshCommand"], Duration::from_secs(10), &[]).ok;
    let ssh: &[(&str, &str)] = if own { &[] } else { &[("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")] };
    let t60 = Duration::from_secs(60);
    let fetch = git(&["fetch", "--tags"], t60, ssh);
    let on_branch = git(&["symbolic-ref", "-q", "HEAD"], Duration::from_secs(10), &[]).ok;
    // A tag unknown here (the fetch failed) is taken as following the branch: the fast-forward, which refuses
    // otherwise, rather than a branch left for a detached HEAD.
    let t10 = Duration::from_secs(10);
    let known = git(&["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}^{{commit}}")], t10, &[]).ok;
    let follows = !known || git(&["merge-base", "--is-ancestor", "HEAD", &tag], t10, &[]).ok;
    let merge = on_branch && follows;
    if !fetch.ok {
        return Err((git_error(&fetch.err, "Update failed"), move_steps(dir, to, true, merge)));
    }
    let args = if merge { ["merge", "--ff-only", tag.as_str()] } else { ["checkout", "--detach", tag.as_str()] };
    let r = git(&args, t60, &[]);
    if !r.ok {
        return Err((git_error(&r.err, "Update failed"), move_steps(dir, to, false, merge)));
    }
    Ok(move_steps(dir, to, false, merge))
}

/// The checked binary put in place by renames: the old one first set aside (Windows lets a running binary be renamed,
/// not removed; the binary removes these when it starts).
fn place(tmp: &str, exe: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    if std::fs::symlink_metadata(exe).is_ok() {
        let stamp = stamp(std::time::SystemTime::now());
        std::fs::rename(exe, format!("{exe}.old-{stamp}-{}", std::process::id()))?;
    }
    std::fs::rename(tmp, exe)
}

/// yyyymmddHHMMSS, in UTC.
fn stamp(t: std::time::SystemTime) -> String {
    let s = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, secs) = (s.div_euclid(86400), s.rem_euclid(86400));
    // Days to a civil date (Howard Hinnant's days_from_civil, reversed).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}{m:02}{d:02}{:02}{:02}{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (FIPS 180-4) of bytes.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

/// The version on disk watched (update.js watch): once it differs from the one this process runs (an update, or a
/// git pull in a terminal), the pane restarts on the new code, 2 s later so that every file is written, and when
/// idle allows it; asked again every second until then. The pane's loop calls `poll`.
pub struct VersionWatch {
    watch: Option<Watch>,
    attempt: Option<Instant>,
}

impl Default for VersionWatch {
    fn default() -> Self {
        VersionWatch::new()
    }
}

impl VersionWatch {
    pub fn new() -> VersionWatch {
        VersionWatch { watch: manifest().map(|m| Watch::new(&m, Duration::from_millis(2000))), attempt: None }
    }

    fn due_version() -> bool {
        read_version().is_some_and(|v| Some(v.as_str()) != current())
    }

    /// When the loop must look again.
    pub fn due(&self) -> Option<Instant> {
        let w = self.watch.as_ref()?;
        Some(self.attempt.map_or(w.due(), |a| a.min(w.due())))
    }

    /// Looks at the manifest and the attempt due: true when the pane must restart now (the watch then stops).
    pub fn poll(&mut self, idle: impl FnOnce() -> bool) -> bool {
        let Some(w) = &mut self.watch else { return false };
        if w.poll() && self.attempt.is_none() && Self::due_version() {
            self.attempt = Some(Instant::now() + Duration::from_millis(2000));
        }
        let Some(at) = self.attempt.filter(|a| *a <= Instant::now()) else { return false };
        self.attempt = None;
        if !Self::due_version() {
            return false;
        }
        if !idle() {
            self.attempt = Some(at.max(Instant::now()) + Duration::from_millis(1000));
            return false;
        }
        self.watch = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_is_read_from_a_github_origin_url() {
        assert_eq!(repo_of("git@github.com:gcaudan/herdr-explorer.git").as_deref(), Some("gcaudan/herdr-explorer"));
        assert_eq!(repo_of("https://github.com/alice/herdr-explorer").as_deref(), Some("alice/herdr-explorer"));
        assert_eq!(repo_of("https://github.com/alice/fork.git/").as_deref(), Some("alice/fork"));
        assert_eq!(repo_of("ssh://git@github.com/alice/fork.git\n").as_deref(), Some("alice/fork"));
    }

    #[test]
    fn another_host_or_a_malformed_url_gives_none() {
        for url in [
            "https://gitlab.com/alice/fork",
            "https://github.com/alice/fork/tree/main",
            "https://github.com/../fork",
            "https://github.com.evil.example/alice/fork",
            "",
        ] {
            assert_eq!(repo_of(url), None, "{url}");
        }
    }

    #[test]
    fn the_changes_of_a_release_please_text_as_plain_lines() {
        let body = "## [0.8.0](https://x) (2026-10-22)\n\n### Features\n\n* **diff:** stage hunks ([#12](https://g/12)) ([abc1234](https://g/c))\n* add a `light` theme ([d1e2f3a](https://g/d))\n\n### Bug Fixes\n\n- **scm:** keep [the list](https://l)\nplain text\n";
        assert_eq!(release_notes(body), ["diff: stage hunks", "add a light theme", "scm: keep the list"]);
        assert!(release_notes("").is_empty());
        let many: String = (0..30).map(|i| format!("* change {i}\r\n")).collect();
        assert_eq!(release_notes(&many).len(), 20);
        assert_eq!(release_notes("* a\u{7}b\u{85}c"), ["a b c"]);
    }

    #[test]
    fn sha256_of_known_texts() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(hex(&sha256(long)), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        assert_eq!(
            hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn stamps_in_utc() {
        let at = |s: u64| stamp(std::time::UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "19700101000000");
        assert_eq!(at(951_825_599), "20000229115959");
        assert_eq!(at(4_102_444_800), "21000101000000");
    }

    #[test]
    fn a_failed_update_gives_the_rest_to_run() {
        assert_eq!(git_error("hint: x\nfatal: unable to access 'u'\n", "Update failed"), "unable to access 'u'");
        assert_eq!(git_error("error : x\n", "Update failed"), "x");
        assert_eq!(git_error("something\n", "Update failed"), "something");
        assert_eq!(git_error("", "Update failed"), "Update failed");
        if !WIN {
            assert_eq!(manual("/p", vec![]), "sh \"/p/scripts/fetch-bin.sh\" \"/p\"");
            assert_eq!(
                manual("/p", move_steps("/p", "1.0.0", false, false)),
                "git -C \"/p\" checkout --detach v1.0.0 && sh \"/p/scripts/fetch-bin.sh\" \"/p\""
            );
        }
        let steps = ["a".to_string(), "b".into(), "c".into()];
        assert_eq!(chain(&steps, false), "a && b && c");
        assert_eq!(chain(&steps, true), "a; if ($?) { b; if ($?) { c } }");
        assert_eq!(chain(&steps[..1], true), "a");
    }

    /// The update in a sandbox: an origin repository with the tags v1.0.0 and v1.1.0, the plugin's clone of it on
    /// main at v1.0.0 with that version's binary, the release of v1.1.0 served by a fake curl from a folder. Nothing
    /// reaches the network nor the real clone.
    #[cfg(unix)]
    mod install {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};
        use std::process::Command;

        const CURL: &str = r#"#!/bin/sh
out= url=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) [ "$1" = --max-time ] && shift ;;
    *) url=$1 ;;
  esac
  shift
done
echo "$url" >>"$(dirname "$0")/curl.log"
src="$(dirname "$0")/releases/${url#*/releases/download/}"
[ -f "$src" ] || exit 22
cp "$src" "$out"
"#;

        struct Sandbox {
            root: PathBuf,
            plugin: PathBuf,
        }

        fn script(path: &Path, text: &str) {
            std::fs::write(path, text).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn git(dir: &Path, args: &[&str]) -> String {
            let out = Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "tag.gpgsign=false",
                ])
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        }

        fn manifest(v: &str) -> String {
            format!("id = \"gcaudan.explorer\"\nversion = \"{v}\"\n")
        }

        impl Sandbox {
            /// installer: the release also changes install.sh.
            fn new(name: &str, installer: bool) -> Sandbox {
                let root = std::env::temp_dir().join(format!("hx-update-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&root);
                let (src, plugin) = (root.join("src"), root.join("plugin"));
                std::fs::create_dir_all(&src).unwrap();
                git(&src, &["init", "-q", "-b", "main"]);
                std::fs::write(src.join("herdr-plugin.toml"), manifest("1.0.0")).unwrap();
                std::fs::write(src.join("install.sh"), "a\n").unwrap();
                git(&src, &["add", "."]);
                git(&src, &["commit", "-qm", "one"]);
                git(&src, &["tag", "v1.0.0"]);
                std::fs::write(src.join("herdr-plugin.toml"), manifest("1.1.0")).unwrap();
                if installer {
                    std::fs::write(src.join("install.sh"), "b\n").unwrap();
                }
                git(&src, &["commit", "-qam", "two"]);
                git(&src, &["tag", "v1.1.0"]);
                git(&root, &["clone", "-q", src.to_str().unwrap(), "plugin"]);
                git(&plugin, &["reset", "-q", "--hard", "v1.0.0"]);
                std::fs::create_dir_all(plugin.join("bin")).unwrap();
                script(&plugin.join("bin/herdr-explorer"), "#!/bin/sh\necho 1.0.0\n");
                let fake = root.join("fake");
                let release = fake.join("releases/v1.1.0");
                std::fs::create_dir_all(&release).unwrap();
                script(&fake.join("curl"), CURL);
                let name = asset().unwrap();
                script(&release.join(name), "#!/bin/sh\necho 1.1.0\n");
                let sum = hex(&sha256(&std::fs::read(release.join(name)).unwrap()));
                std::fs::write(release.join("SHA256SUMS"), format!("{sum}  {name}\n")).unwrap();
                Sandbox { root, plugin }
            }

            fn ctx(&self) -> Ctx {
                Ctx {
                    dir: self.plugin.to_str().unwrap().into(),
                    curl: Some(self.root.join("fake/curl").to_str().unwrap().into()),
                    file: Some(self.root.join("update.json").to_str().unwrap().into()),
                }
            }

            fn version(&self) -> String {
                let out = Command::new(self.plugin.join("bin/herdr-explorer")).output().unwrap();
                String::from_utf8(out.stdout).unwrap().trim().to_string()
            }

            fn head(&self) -> String {
                git(&self.plugin, &["rev-parse", "HEAD"])
            }

            fn tag(&self, t: &str) -> String {
                git(&self.plugin, &["rev-parse", &format!("{t}^{{commit}}")])
            }

            fn on_main(&self) -> bool {
                Command::new("git")
                    .args(["symbolic-ref", "-q", "HEAD"])
                    .current_dir(&self.plugin)
                    .output()
                    .unwrap()
                    .status
                    .success()
            }

            /// bin/'s files, the binaries set aside under one name.
            fn bin(&self) -> Vec<String> {
                let mut names: Vec<String> = std::fs::read_dir(self.plugin.join("bin"))
                    .unwrap()
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .map(|n| if n.starts_with("herdr-explorer.old-") { "herdr-explorer.old-*".into() } else { n })
                    .collect();
                names.sort();
                names
            }

            fn saved(&self) -> Value {
                read_at(Some(self.root.join("update.json").to_str().unwrap()))
            }

            fn curl_calls(&self) -> usize {
                std::fs::read_to_string(self.root.join("fake/curl.log")).map_or(0, |l| l.lines().count())
            }
        }

        impl Drop for Sandbox {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        #[test]
        fn moves_the_clone_to_the_tag_and_puts_its_checked_binary() {
            let b = Sandbox::new("ok", false);
            let o = update(&b.ctx(), "1.1.0");
            assert!(o.ok, "{}", o.message);
            assert_eq!(o.message, "Updated to 1.1.0");
            assert_eq!(b.head(), b.tag("v1.1.0"));
            assert!(b.on_main(), "fast-forwarded, still on main");
            assert_eq!(b.version(), "1.1.0");
            assert_eq!(b.bin(), ["herdr-explorer", "herdr-explorer.old-*"]);
            let saved = b.saved();
            let updated = saved.get("updated").unwrap();
            assert_eq!(updated.get("from"), Some(&Value::from("1.0.0")));
            assert_eq!(updated.get("to"), Some(&Value::from("1.1.0")));
            assert_eq!(saved.get("finish"), None);
            // Done already: nothing downloaded again.
            let o = update(&b.ctx(), "1.1.0");
            assert_eq!((o.ok, o.message.as_str()), (true, "Already up to date"));
            assert_eq!(b.curl_calls(), 2);
        }

        #[test]
        fn an_installer_changed_must_be_run_again() {
            let b = Sandbox::new("installer", true);
            let o = update(&b.ctx(), "1.1.0");
            assert_eq!((o.ok, o.message.as_str()), (true, "Updated to 1.1.0: rerun the installer"));
            assert_eq!(b.saved().get("finish"), Some(&Value::from("1.1.0")));
        }

        #[test]
        fn a_binary_that_does_not_match_leaves_everything_as_it_was() {
            let b = Sandbox::new("hash", false);
            std::fs::write(b.root.join("fake/releases/v1.1.0").join(asset().unwrap()), "other").unwrap();
            let o = update(&b.ctx(), "1.1.0");
            assert!(!o.ok && o.retry.is_none());
            assert_eq!(o.message, "The 1.1.0 binary does not match its SHA256SUMS");
            assert_eq!(b.head(), b.tag("v1.0.0"));
            assert_eq!(b.version(), "1.0.0");
            assert_eq!(b.bin(), ["herdr-explorer"]);
        }

        #[test]
        fn a_release_without_binary_or_curl_is_refused() {
            let b = Sandbox::new("missing", false);
            let o = update(&b.ctx(), "1.2.0");
            assert_eq!((o.ok, o.message.as_str()), (false, "Cannot download the 1.2.0 binary"));
            let o = update(&Ctx { curl: None, ..b.ctx() }, "1.1.0");
            assert_eq!((o.ok, o.message.as_str()), (false, "curl not found in PATH"));
            assert_eq!(b.head(), b.tag("v1.0.0"));
            assert_eq!(b.bin(), ["herdr-explorer"]);
        }

        #[test]
        fn local_changes_are_refused() {
            let b = Sandbox::new("dirty", false);
            std::fs::write(b.plugin.join("install.sh"), "mine\n").unwrap();
            let o = update(&b.ctx(), "1.1.0");
            assert_eq!((o.ok, o.message.as_str()), (false, "Local changes in the plugin folder"));
            assert_eq!(b.curl_calls(), 0);
        }

        #[test]
        fn a_development_binary_is_kept() {
            let b = Sandbox::new("dev", false);
            std::fs::write(b.plugin.join("bin/.dev"), "").unwrap();
            let o = update(&b.ctx(), "1.1.0");
            assert!(o.ok, "{}", o.message);
            assert_eq!(b.head(), b.tag("v1.1.0"));
            assert_eq!(b.version(), "1.0.0");
            assert_eq!(b.curl_calls(), 0);
        }

        #[test]
        fn a_branch_the_tag_does_not_follow_is_left_for_the_tag_detached() {
            let b = Sandbox::new("diverged", false);
            std::fs::write(b.plugin.join("mine.txt"), "x").unwrap();
            git(&b.plugin, &["add", "mine.txt"]);
            git(&b.plugin, &["commit", "-qm", "mine"]);
            let mine = b.head();
            let o = update(&b.ctx(), "1.1.0");
            assert!(o.ok, "{}", o.message);
            assert_eq!(b.head(), b.tag("v1.1.0"));
            assert!(!b.on_main());
            assert_eq!(git(&b.plugin, &["rev-parse", "main"]), mine, "the branch keeps its commit");
        }

        #[test]
        fn a_failed_git_step_gives_it_to_run() {
            let b = Sandbox::new("nofetch", false);
            git(&b.plugin, &["remote", "set-url", "origin", b.root.join("gone").to_str().unwrap()]);
            let o = update(&b.ctx(), "1.1.0");
            assert!(!o.ok);
            let dir = b.plugin.to_str().unwrap();
            let retry = o.retry.unwrap();
            // On main, which the tag follows, though the tag is unknown here: the fast-forward, not a detached HEAD.
            let steps = format!("git -C \"{dir}\" fetch --tags && git -C \"{dir}\" merge --ff-only v1.1.0 && ");
            assert!(retry.starts_with(&steps), "{retry}");
            assert!(retry.ends_with(&format!("sh \"{dir}/scripts/fetch-bin.sh\" \"{dir}\"")), "{retry}");
            assert_eq!(b.head(), b.tag("v1.0.0"));
            assert_eq!(b.version(), "1.0.0", "the binary waits for the clone");
            assert_eq!(b.bin(), ["herdr-explorer"]);
        }
    }
}
