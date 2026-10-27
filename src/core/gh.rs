//! GitHub's pull requests through the GitHub CLI (gh), for source control's Pull requests view and the preview: run
//! without a shell, with the repository's git environment (its hooks and filters off for the git gh runs), without a
//! prompt; their text cleaned of control characters, since titles and descriptions are anyone's.

use super::exec::{self, Opts};
use super::paths;
use super::ui::clean;
use super::which::which;
use crate::json::{self, Value};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

/// A pull request of the list: its checks "pass", "fail", "pending" or "" (none); its review, gh's reviewDecision
/// ("APPROVED", "CHANGES_REQUESTED", "REVIEW_REQUIRED" or "").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pull {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub branch: String,
    pub draft: bool,
    pub checks: &'static str,
    pub review: String,
    pub url: String,
}

const LIST_FIELDS: &str = "number,title,author,headRefName,isDraft,reviewDecision,statusCheckRollup,url";

/// gh with args in the repository top: its output, or the reason it failed, said in one line.
pub fn run(top: &str, args: &[&str]) -> Result<String, String> {
    run_in(top, args, exec::git_env(top), TIMEOUT)
}

/// run with the git environment given (a hardened one for the git that fetches: gh pr checkout) and a time limit.
pub fn run_in(top: &str, args: &[&str], mut env: exec::Env, timeout: Duration) -> Result<String, String> {
    let Some(gh) = which("gh") else { return Err("GitHub CLI (gh) not found".into()) };
    for (k, v) in [
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1"),
        ("GH_SPINNER_DISABLED", "1"),
        ("GH_PAGER", ""),
        ("NO_COLOR", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_ALLOW_PROTOCOL", "https:http:ssh:git"),
    ] {
        exec::env_set(&mut env, k, v);
    }
    let opts =
        Opts { cwd: Some(top.into()), env: Some(env), timeout: Some(timeout), detached: true, ..Default::default() };
    let res = exec::exec(Some(&gh), args, &opts);
    if res.code == Some(0) {
        return Ok(res.out);
    }
    Err(reason(&res.err))
}

/// gh's error in one line: logging in when that is what it asks.
fn reason(err: &str) -> String {
    if err.contains("gh auth login") {
        return "gh is not logged in: run gh auth login".into();
    }
    let line = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("gh failed");
    clean(line)
}

/// The open pull requests of the repository, kept in the state folder for the next opening (cached).
pub fn list(top: &str) -> Result<Vec<Pull>, String> {
    let out = run(top, &["pr", "list", "--state", "open", "--limit", "100", "--json", LIST_FIELDS])?;
    let pulls = parse_list(&out).ok_or_else(|| "Unexpected answer from gh".to_string())?;
    if let Some(file) = cache_file(top) {
        let _ = std::fs::create_dir_all(paths::dirname(&file));
        exec::write_atomic(&file, out);
    }
    Ok(pulls)
}

/// The list of the last opening, shown while gh answers.
pub fn cached(top: &str) -> Option<Vec<Pull>> {
    parse_list(&exec::read_text(cache_file(top)?).ok()?)
}

/// pulls/<FNV-1a of the repository's folder>.json in the state folder.
fn cache_file(top: &str) -> Option<String> {
    let dir = exec::state_dir()?;
    let hash = top.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    Some(paths::join(&[&dir, "pulls", &format!("{hash:016x}.json")]))
}

fn str_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).map(clean).unwrap_or_default()
}

/// The checks of statusCheckRollup in one word: a failure first, then one still running, else passed; "" for none.
fn checks(rollup: Option<&Value>) -> &'static str {
    let Some(items) = rollup.and_then(Value::as_array).filter(|a| !a.is_empty()) else { return "" };
    let outcome = |c: &Value| {
        // A check run has a conclusion (empty while it runs), a commit status a state.
        let word = c.get("conclusion").and_then(Value::as_str).filter(|s| !s.is_empty());
        let word = word.or_else(|| c.get("state").and_then(Value::as_str)).unwrap_or("");
        match word {
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => "fail",
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => "pass",
            _ => "pending",
        }
    };
    let all: Vec<&str> = items.iter().map(outcome).collect();
    ["fail", "pending"].into_iter().find(|w| all.contains(w)).unwrap_or("pass")
}

/// gh pr list's JSON: None when it is not a list.
pub fn parse_list(out: &str) -> Option<Vec<Pull>> {
    let v = json::parse(out).ok()?;
    let items = v.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|p| {
                let number = p.get("number").and_then(Value::as_f64).filter(|n| *n >= 1.0 && n.fract() == 0.0)?;
                Some(Pull {
                    number: number as u64,
                    title: str_at(p, "title"),
                    author: p.get("author").map(|a| str_at(a, "login")).unwrap_or_default(),
                    branch: str_at(p, "headRefName"),
                    draft: p.get("isDraft") == Some(&Value::Bool(true)),
                    checks: checks(p.get("statusCheckRollup")),
                    review: str_at(p, "reviewDecision"),
                    url: str_at(p, "url"),
                })
            })
            .collect(),
    )
}

/// What a pull request's review stands at, in words.
pub fn review_text(review: &str) -> &'static str {
    match review {
        "APPROVED" => "approved",
        "CHANGES_REQUESTED" => "changes requested",
        "REVIEW_REQUIRED" => "review required",
        _ => "",
    }
}

/// A pull request as the preview shows it: its header and description, then its diff (gh pr diff), as one text that
/// diff::parse reads, the header and description before the first file. None when gh fails.
pub fn show(top: &str, number: u64) -> Option<String> {
    let n = number.to_string();
    let fields = "number,title,author,state,isDraft,url,headRefName,baseRefName,body,reviewDecision,statusCheckRollup";
    let (view, diff) = std::thread::scope(|s| {
        let diff = s.spawn(|| run(top, &["pr", "diff", &n, "--color", "never"]));
        let view = run(top, &["pr", "view", &n, "--json", fields]);
        (view, diff.join().unwrap_or_else(|_| Err(String::new())))
    });
    let v = json::parse(&view.ok()?).ok()?;
    // GitHub gives no diff past 300 files: said in its place.
    let diff = diff.unwrap_or_else(|e| format!("No diff: {e}\n"));
    Some(format!("{}{diff}", header(&v)))
}

/// The header of show: the lines before the diff, the description indented as a commit message in git show.
fn header(v: &Value) -> String {
    let number = v.get("number").and_then(Value::as_f64).unwrap_or(0.0) as u64;
    let mut out = format!("PR #{number} {}\n", str_at(v, "title"));
    let draft = if v.get("isDraft") == Some(&Value::Bool(true)) { ", draft" } else { "" };
    let author = v.get("author").map(|a| str_at(a, "login")).unwrap_or_default();
    out += &format!("Author: {author} ({}{draft})\n", str_at(v, "state").to_lowercase());
    out += &format!("Branch: {} → {}\n", str_at(v, "headRefName"), str_at(v, "baseRefName"));
    let checks = match checks(v.get("statusCheckRollup")) {
        "" => "none",
        "pass" => "passed",
        "fail" => "failed",
        _ => "running",
    };
    out += &format!("Checks: {checks}\n");
    let review = review_text(&str_at(v, "reviewDecision"));
    if !review.is_empty() {
        out += &format!("Review: {review}\n");
    }
    out += &format!("URL:    {}\n\n", str_at(v, "url"));
    let body = v.get("body").and_then(Value::as_str).unwrap_or("").replace("\r\n", "\n");
    for line in body.trim_end().lines() {
        out += &format!("    {}\n", clean(line));
    }
    if !body.trim().is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_checks_reviews_and_cleans_the_text() {
        let out = r#"[
            {"number": 12, "title": "Fix \u001b[31mred", "author": {"login": "ann"}, "headRefName": "fix",
             "isDraft": false, "reviewDecision": "APPROVED", "url": "https://github.com/o/r/pull/12",
             "statusCheckRollup": [{"conclusion": "SUCCESS"}, {"conclusion": "", "status": "IN_PROGRESS"}]},
            {"number": 13, "title": "Draft", "author": {"login": "bob"}, "headRefName": "d", "isDraft": true,
             "reviewDecision": "", "url": "", "statusCheckRollup": [{"state": "FAILURE"}, {"conclusion": "SUCCESS"}]},
            {"number": 14, "title": "No checks", "statusCheckRollup": []},
            {"title": "no number"}
        ]"#;
        let pulls = parse_list(out).unwrap();
        assert_eq!(pulls.len(), 3);
        assert_eq!(pulls[0].title, "Fix ?[31mred");
        assert_eq!((pulls[0].checks, pulls[0].author.as_str(), pulls[0].draft), ("pending", "ann", false));
        assert_eq!((pulls[1].checks, pulls[1].draft), ("fail", true));
        assert_eq!((pulls[2].checks, pulls[2].author.as_str()), ("", ""));
        assert_eq!(parse_list("{}"), None);
    }

    #[test]
    fn the_header_comes_before_the_diff_and_its_description_is_indented() {
        let v = json::parse(
            r#"{"number": 7, "title": "T", "author": {"login": "a"}, "state": "OPEN", "isDraft": true,
                "headRefName": "h", "baseRefName": "main", "body": "line\r\n\u001b]0;x\u0007", "url": "u",
                "reviewDecision": "CHANGES_REQUESTED", "statusCheckRollup": []}"#,
        )
        .unwrap();
        let text = header(&v);
        assert_eq!(
            text,
            "PR #7 T\nAuthor: a (open, draft)\nBranch: h → main\nChecks: none\nReview: changes requested\nURL:    u\n\n    line\n    ?]0;x?\n\n"
        );
        let d = crate::diff::parse(&format!("{text}diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n"));
        assert_eq!(d.files.len(), 1);
        assert_eq!(d.pre.first().map(String::as_str), Some("PR #7 T"));
    }

    #[test]
    fn a_login_request_says_so() {
        assert_eq!(
            reason("To get started with GitHub CLI, please run:  gh auth login\n"),
            "gh is not logged in: run gh auth login"
        );
        assert_eq!(reason("\nno git remotes found\nmore"), "no git remotes found");
    }
}
