//! New releases of the plugin: a check of GitHub once a day (from the hook, never from the panes), the version found
//! kept in the state folder, for the explorer's ↑, which copies herdr's update command: herdr's plugin index is the
//! only channel, its build step (scripts/fetch-bin.*) putting the release's binary in bin/. The explorer and preview
//! panes restart on the new code by themselves when the version on disk changes (`VersionWatch`: a linked clone
//! moved by git), or a development build is put (bin/.dev written again by scripts/dev-build). GitHub is asked through curl, as fetch-bin does (no crate).

use crate::core::exec::{self, Opts, WIN};
use crate::core::regex::Regex;
use crate::core::watch::Watch;
use crate::core::{paths, which};
use crate::json::{self, Value};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant};

/// The GitHub repository the releases come from when the clone's origin is not on GitHub.
pub const REPO: &str = "gcaudan/herdr-explorer";
const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

fn re(src: &str, flags: &str) -> Regex {
    Regex::new(src, flags).expect("a valid pattern")
}

/// The plugin's folder: the one above the binary's bin/ when it holds the manifest, else
/// HERDR_PLUGIN_ROOT (a binary run from elsewhere, cargo's target among them).
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
    let m = REPO_URL.exec(url.trim_matches(crate::unicode::is_space))?;
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

/// The development build's mark (bin/.dev), which scripts/dev-build writes again once each build is put.
fn dev_file() -> Option<String> {
    dir().map(|d| paths::join(&[d, "bin", ".dev"]))
}

fn read_dev() -> Option<String> {
    std::fs::read_to_string(dev_file()?).ok()
}

/// The development build this process runs: the mark when the process started (`init`), None for a release binary.
fn current_dev() -> Option<&'static str> {
    static CURRENT: OnceLock<Option<String>> = OnceLock::new();
    CURRENT.get_or_init(read_dev).as_deref()
}

/// Reads the version and the development build this process runs, at its start.
pub fn init() {
    let _ = current();
    let _ = current_dev();
}

/// { checkedAt, repo, latest, notes, url, skip, seen, updated } in the state folder: the repository checked, the
/// latest release, its changes and page, the version the user skipped, the version the explorers last ran, and the
/// last update seen ({ from, to, t }).
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
        // Unparsable or falsy: an empty object; an array or another truthy value is kept, its fields then missing.
        _ => Value::Obj(Vec::new()),
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
        let t = t.trim_matches(crate::unicode::is_space);
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
/// are not followed.
fn latest_release(repo: &str) -> Option<(String, Vec<String>, String)> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let args = ["-fsS", "--max-time", "3", "-A", "herdr-explorer", "-H", "Accept: application/vnd.github+json", &url];
    let body = exec::run(curl().as_deref(), &args, &Opts { timeout: Some(Duration::from_secs(5)), ..Opts::default() })?;
    let release = json::parse(&body).ok().filter(Value::truthy).unwrap_or(Value::Obj(Vec::new()));
    let field = |k: &str| release.get(k).map_or_else(|| "undefined".to_string(), json::text);
    let tag = field("tag_name");
    let tag = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let page = field("html_url");
    let url = if PAGE.test(&page) { page } else { format!("https://github.com/{repo}/releases") };
    let body = release.get("body").filter(|b| b.truthy()).map(json::text).unwrap_or_default();
    TAG.test(&tag).then(|| (tag, release_notes(&body), url))
}

/// At most once a day, and not when the setting is off. A failed check is not retried before the next day either.
pub fn check() {
    if file().is_none() || exec::read_settings().get("updateCheck") == Some(&Value::Bool(false)) {
        return;
    }
    let at = read().get("checkedAt").filter(|v| v.truthy()).map_or(0.0, |v| json::num(Some(v)));
    if crate::core::clock::now_ms() - at < DAY {
        return;
    }
    let repo = origin().and_then(|o| repo_of(&o)).unwrap_or_else(|| REPO.into());
    let release = latest_release(&repo);
    let mut fields =
        vec![("checkedAt", Value::from(crate::core::clock::now_ms())), ("repo", Value::from(repo.as_str()))];
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
    let text = json::text(latest);
    exec::newer(&text, &current).then(|| (text, data.get("skip") == Some(latest)))
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
        data.get("notes").and_then(Value::as_array).map_or_else(Vec::new, |n| n.iter().map(json::text).collect());
    let url = data.get("url").and_then(Value::as_str).map_or(releases, String::from);
    (notes, url)
}

/// A version the explorers had not run yet, noted at an explorer's start: an update ({ from, to, t }) when it is newer
/// than the one they ran last. None before (a first install): no update.
pub fn note_version() {
    let (Some(_), Some(now)) = (file(), current()) else { return };
    let data = read();
    let seen = data.get("seen").and_then(Value::as_str).map(String::from);
    if seen.as_deref() == Some(now) {
        return;
    }
    let mut fields = vec![("seen", Value::from(now))];
    if let Some(from) = seen.filter(|s| exec::newer(now, s)) {
        let updated = Value::Obj(vec![
            ("from".into(), from.into()),
            ("to".into(), now.into()),
            ("t".into(), crate::core::clock::now_ms().into()),
        ]);
        fields.push(("updated", updated));
    }
    write_with(fields);
}

/// The update this code comes from, for a minute after the first explorer started on it: the version it went to.
pub fn just_updated() -> Option<String> {
    file()?;
    let data = read();
    let updated = data.get("updated").filter(|u| u.truthy())?;
    let to = updated.get("to").and_then(Value::as_str)?;
    let t = json::num(updated.get("t"));
    (Some(to) == current() && crate::core::clock::now_ms() - t < 60.0 * 1000.0).then(|| to.to_string())
}

/// The clone's origin URL, None without git or an origin. detached (not on Windows): a session without terminal, so
/// that a credential helper can never prompt in the pane; GCM_INTERACTIVE: nor Git Credential Manager's window.
fn origin() -> Option<String> {
    let (git, dir) = (exec::git()?, dir()?);
    let mut env = exec::process_env();
    for (k, v) in [("GIT_TERMINAL_PROMPT", "0"), ("GCM_INTERACTIVE", "never")] {
        exec::env_set(&mut env, k, v);
    }
    let timeout = Some(Duration::from_secs(10));
    let opts = Opts { cwd: Some(dir.into()), env: Some(env), timeout, detached: !WIN, ..Opts::default() };
    let r = exec::exec(Some(git), &["config", "--get", "remote.origin.url"], &opts);
    (r.code == Some(0)).then_some(r.out)
}

/// The version on disk watched, and the development build's mark: once one differs from what this process runs (an
/// update, a git pull in a terminal, a development build put), the pane restarts on the new code, 2 s later so that every file is written, and when
/// idle allows it; asked again every second until then. The pane's loop calls `poll`.
pub struct VersionWatch {
    watch: Option<Watch>,
    dev: Option<Watch>,
    attempt: Option<Instant>,
}

impl Default for VersionWatch {
    fn default() -> Self {
        VersionWatch::new()
    }
}

impl VersionWatch {
    pub fn new() -> VersionWatch {
        let watch = |f: Option<String>| f.map(|f| Watch::new(&f, Duration::from_millis(2000)));
        VersionWatch { watch: watch(manifest()), dev: watch(dev_file()), attempt: None }
    }

    /// A new version, or a new development build (its mark there and changed: one removed brings no new code).
    fn due_version() -> bool {
        read_version().is_some_and(|v| Some(v.as_str()) != current())
            || read_dev().is_some_and(|d| Some(d.as_str()) != current_dev())
    }

    /// When the loop must look again.
    pub fn due(&self) -> Option<Instant> {
        let w = self.watch.as_ref()?;
        [self.attempt, w.due(), self.dev.as_ref().and_then(Watch::due)].into_iter().flatten().min()
    }

    /// Looks at the manifest and the attempt due: true when the pane must restart now (the watch then stops).
    pub fn poll(&mut self, idle: impl FnOnce() -> bool) -> bool {
        let Some(w) = &mut self.watch else { return false };
        // Both looked at, each keeping its own track.
        let changed = w.poll() | self.dev.as_mut().is_some_and(Watch::poll);
        if changed && self.attempt.is_none() && Self::due_version() {
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
        self.dev = None;
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
}
