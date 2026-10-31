//! A commit, a stash or a tag shown by the preview's Diff view (diff::show), from git.

mod support;

use herdr_explorer::diff;
use herdr_explorer::json::Value;
use std::process::Command;

fn what(kind: &str, r: &str) -> Value {
    json(&format!(r#"{{"kind":"{kind}","ref":"{r}"}}"#))
}

fn json(text: &str) -> Value {
    herdr_explorer::json::parse(text).unwrap()
}

#[test]
fn a_commit_a_stash_and_a_tag_give_their_git_arguments_anything_else_none() {
    let args = |text: &str| diff::show_args(&json(text));
    let commit = args(r#"{"kind":"commit","ref":"abc","paths":["a.txt"]}"#).unwrap();
    assert_eq!(commit[commit.len() - 3..], ["abc", "--", "a.txt"]);
    assert_eq!(args(r#"{"kind":"stash","ref":"stash@{0}"}"#).unwrap()[..2], ["stash", "show"]);
    assert_eq!(args(r#"{"kind":"tag","ref":"v1"}"#).unwrap().last().unwrap(), "refs/tags/v1");
    // A ref read as an option, an unknown kind, paths not strings: refused, as the request file is the preview's input.
    assert!(args(r#"{"kind":"commit","ref":"--output=/tmp/x"}"#).is_none());
    assert!(args(r#"{"kind":"log","ref":"abc"}"#).is_none());
    assert!(args(r#"{"kind":"commit","ref":"abc","paths":[1]}"#).is_none());
}

#[test]
fn a_commit_shows_its_changes() {
    let tmp = support::TempDir::new("show");
    let dir = tmp.path.to_str().unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    std::fs::write(tmp.path.join("a.txt"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "first"]);
    std::fs::write(tmp.path.join("a.txt"), "one\ntwo\n").unwrap();
    git(&["commit", "-qam", "second"]);
    let head = git(&["rev-parse", "HEAD"]);
    let d = diff::show(dir, &what("commit", &head)).expect("the commit's diff");
    assert_eq!(d.files.len(), 1);
    assert_eq!(d.add, 1);
    assert!(d.pre.iter().any(|l| l.contains("second")), "{:?}", d.pre);
    assert!(diff::show(dir, &what("commit", "nope")).is_none());
}
