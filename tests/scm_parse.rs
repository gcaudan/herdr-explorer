//! test/scm-parse.test.js: the parsers of the source control pane, on output taken from git.

use herdr_explorer::core::exec::Exec;
use herdr_explorer::core::{js, paths};
use herdr_explorer::scm::parse::{self, Decoration, Dirs, Item, Label, Remote, Stash, Worktree};

const OID: &str = "5de5c897f4428a5692f5231cb6fc7d24c977d804";

fn z(lines: &[&str]) -> String {
    lines.join("\0") + "\0"
}

fn zeros() -> String {
    "0".repeat(40)
}

fn dec(name: &str, kind: &'static str) -> Decoration {
    Decoration { name: name.into(), kind }
}

#[test]
fn status_empty_repository() {
    let s = parse::parse_status(&z(&["# branch.oid (initial)", "# branch.head main"]));
    assert_eq!(s.branch, "main");
    assert!(!s.has_head);
    assert!(!s.detached);
    assert!(s.conflicts.is_empty() && s.staged.is_empty() && s.changes.is_empty());
    assert_eq!(s.upstream, "");
    assert_eq!((s.ahead, s.behind), (0.0, 0.0));
}

#[test]
fn status_upstream_staged_and_unstaged_changes_of_one_file_sorted_by_path() {
    let s = parse::parse_status(&z(&[
        &format!("# branch.oid {OID}"),
        "# branch.head feat/x",
        "# branch.upstream origin/feat/x",
        "# branch.ab +2 -1",
        &format!("1 MM N... 100644 100644 100644 {OID} {OID} src/b.js"),
        &format!("1 A. N... 000000 100644 100644 {} {OID} a.txt", zeros()),
        &format!("1 .D N... 100644 100644 000000 {OID} {OID} Z.md"),
        &format!("1 .T N... 100644 100644 120000 {OID} {OID} link"),
    ]));
    assert_eq!(s.branch, "feat/x");
    assert_eq!(s.oid, OID);
    assert_eq!(s.upstream, "origin/feat/x");
    assert_eq!((s.ahead, s.behind), (2.0, 1.0));
    assert!(s.has_head);
    let staged: Vec<(&str, char, Option<&str>)> =
        s.staged.iter().map(|i| (i.path.as_str(), i.code, i.orig.as_deref())).collect();
    assert_eq!(staged, [("a.txt", 'A', None), ("src/b.js", 'M', None)]);
    let changes: Vec<(&str, char)> = s.changes.iter().map(|i| (i.path.as_str(), i.code)).collect();
    assert_eq!(changes, [("link", 'M'), ("src/b.js", 'M'), ("Z.md", 'D')]);
    assert!(s.changes.iter().all(|i| i.sec == "changes") && s.staged.iter().all(|i| i.sec == "staged"));
}

#[test]
fn status_z_rename_to_a_unicode_path_with_spaces_conflict_untracked_names_kept_literal() {
    let s = parse::parse_status(&z(&[
        &format!("# branch.oid {OID}"),
        "# branch.head main",
        &format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 dir/new é.js"),
        "old.js",
        &format!("u UU N... 100644 100644 100644 100644 {OID} {OID} {OID} f.txt"),
        &format!("u DU N... 100644 000000 100644 100644 {OID} {} {OID} gone by us.txt", zeros()),
        "? tab\tq.txt",
        "? ünï cødé.md",
        "? \"quoted\".txt",
    ]));
    assert_eq!(
        s.staged,
        [Item {
            path: "dir/new é.js".into(),
            orig: Some("old.js".into()),
            code: 'R',
            sec: "staged",
            ..Item::default()
        }]
    );
    let conflicts: Vec<(&str, char, Option<&str>)> =
        s.conflicts.iter().map(|i| (i.path.as_str(), i.code, i.label)).collect();
    assert_eq!(conflicts, [("f.txt", 'C', Some("both modified")), ("gone by us.txt", 'C', Some("deleted by us"))]);
    let changes: Vec<(&str, char, bool)> = s.changes.iter().map(|i| (i.path.as_str(), i.code, i.untracked)).collect();
    assert_eq!(changes, [("\"quoted\".txt", 'U', true), ("tab\tq.txt", 'U', true), ("ünï cødé.md", 'U', true)]);
}

#[test]
fn status_a_copy_is_shown_as_added_without_its_source() {
    let s = parse::parse_status(&z(&[&format!("2 C. N... 100644 100644 100644 {OID} {OID} C75 copy.js"), "orig.js"]));
    assert_eq!(s.staged, [Item { path: "copy.js".into(), orig: None, code: 'A', sec: "staged", ..Item::default() }]);
}

#[test]
fn status_detached_head() {
    let s = parse::parse_status(&z(&[&format!("# branch.oid {OID}"), "# branch.head (detached)"]));
    assert!(s.detached);
    assert!(s.has_head);
}

#[test]
fn rev_parse_the_repository_folders_the_common_one_relative_to_the_folder() {
    let root = paths::resolve(&["/repo/sub"]);
    let r = parse::parse_repo(
        &format!("{}\n{}\n../.git\n", paths::resolve(&["/repo"]), paths::resolve(&["/repo/.git"])),
        &root,
    );
    assert_eq!(
        r,
        Some(Dirs {
            top: paths::resolve(&["/repo"]),
            gitdir: paths::resolve(&["/repo/.git"]),
            common: paths::resolve(&["/repo/.git"])
        })
    );
    assert_eq!(parse::parse_repo("", &root), None);
}

#[test]
fn log_follow_names() {
    assert_eq!(parse::parse_names("\nsrc/new.js\0\nsrc/old.js\0"), ["src/new.js", "src/old.js"]);
}

#[test]
fn history_decorations_head_parents_merge_commits() {
    let now = (js::now_ms() / 1000.0).floor();
    let (a, b, c, d) = ("a".repeat(40), "b".repeat(40), "c".repeat(40), "d".repeat(40));
    let rows = [
        [
            a.as_str(),
            "aaaaaaa",
            "Merge branch 'feat'",
            "Zoë",
            &(now - 120.0).to_string(),
            "HEAD -> refs/heads/main, refs/remotes/origin/main, refs/remotes/origin/HEAD, tag: refs/tags/v1.0",
            &format!("{b} {c}"),
        ]
        .join("\x1f"),
        [b.as_str(), "bbbbbbb", "fix: x | y", "T", &(now - 3.0 * 86400.0).to_string(), "", &d].join("\x1f"),
        [d.as_str(), "ddddddd", "feat: initial", "T", &(now - 400.0 * 86400.0).to_string(), "HEAD", ""].join("\x1f"),
    ];
    let h = parse::parse_history(&(rows.join("\n") + "\n"));
    assert_eq!(h.len(), 3);
    assert_eq!(h[0].refs, [dec("main", "heads"), dec("origin/main", "remotes"), dec("v1.0", "tags")]);
    assert!(h[0].head);
    assert_eq!(h[0].parents, [b.clone(), c]);
    assert_eq!(h[0].date, "2 min");
    assert_eq!(h[1].subject, "fix: x | y");
    assert_eq!((h[1].refs.is_empty(), h[1].head, h[1].date.as_str()), (true, false, "3 d"));
    // Detached HEAD: HEAD alone.
    assert_eq!(h[2].refs, [dec("HEAD", "heads")]);
    assert!(h[2].head);
    assert!(h[2].parents.is_empty());
    assert_eq!(h[2].date, "1 y");
    assert!(parse::parse_history("").is_empty());
}

#[test]
fn ago() {
    let now = js::now_ms() / 1000.0;
    assert_eq!(parse::ago(now), "1 min");
    assert_eq!(parse::ago(now - 7200.0), "2 h");
    assert_eq!(parse::ago(now - 60.0 * 86400.0), "2 mo");
    assert_eq!(parse::ago(now + 999.0), "1 min");
}

#[test]
fn stash_list() {
    let s = parse::parse_stashes(
        "stash@{0}\x1fWIP on main: ae3311f chore: z\x1f2 minutes ago\nstash@{1}\x1fOn feat: été\x1f3 days ago\n",
    );
    let stash =
        |r: &str, subject: &str, date: &str| Stash { r#ref: r.into(), subject: subject.into(), date: date.into() };
    assert_eq!(
        s,
        [
            stash("stash@{0}", "WIP on main: ae3311f chore: z", "2 minutes ago"),
            stash("stash@{1}", "On feat: été", "3 days ago")
        ]
    );
    assert!(parse::parse_stashes("").is_empty());
}

#[test]
fn refs_branches_remote_branches_without_head_tags_remotes_worktrees() {
    let refs = [
        ["refs/heads/feat/x", "d5c4e81", "origin/feat/x", "[ahead 1, behind 2]", "2 days ago", "fix: readme"],
        ["refs/heads/main", "ae3311f", "", "", "31 seconds ago", "chore: z"],
        ["refs/remotes/origin/HEAD", "ae3311f", "", "", "1 day ago", "chore: z"],
        ["refs/remotes/origin/feat/x", "0520c69", "", "", "1 day ago", "feat: initial"],
        ["refs/tags/v1", "d5c4e81", "", "", "2 days ago", "fix: readme"],
    ]
    .map(|f| f.join("\x1f"))
    .join("\n")
        + "\n";
    let remotes = "origin\tgit@example.com:me/repo.git (fetch)\norigin\tgit@example.com:me/repo.git (push)\nup\thttps://example.com/r (fetch)\nup\thttps://example.com/r (push)\n";
    let trees = format!(
        "worktree /src/repo\nHEAD {OID}\nbranch refs/heads/main\n\nworktree /src/wt\nHEAD {OID}\ndetached\n\nworktree /src/wt é\nHEAD {OID}\nbranch refs/heads/feat/x\nlocked\n\n"
    );
    let r = parse::parse_refs(&refs, remotes, &trees);
    let heads: Vec<(&str, &str, &str, bool, bool)> =
        r.heads.iter().map(|b| (b.name.as_str(), b.upstream.as_str(), b.track.as_str(), b.remote, b.tag)).collect();
    assert_eq!(
        heads,
        [("feat/x", "origin/feat/x", "[ahead 1, behind 2]", false, false), ("main", "", "", false, false)]
    );
    let rem: Vec<(&str, bool)> = r.remotes.iter().map(|b| (b.name.as_str(), b.remote)).collect();
    assert_eq!(rem, [("origin/feat/x", true)]);
    let tags: Vec<(&str, bool, &str)> = r.tags.iter().map(|t| (t.name.as_str(), t.tag, t.subject.as_str())).collect();
    assert_eq!(tags, [("v1", true, "fix: readme")]);
    let url = |name: &str, url: &str| Remote { name: name.into(), url: url.into() };
    assert_eq!(r.urls, [url("origin", "git@example.com:me/repo.git"), url("up", "https://example.com/r")]);
    let wt = |path: &str, branch: &str| Worktree { path: path.into(), branch: branch.into() };
    assert_eq!(r.worktrees, [wt("/src/repo", "main"), wt("/src/wt", "(detached)"), wt("/src/wt é", "feat/x")]);
    assert_eq!(parse::parse_refs("", "", ""), parse::Refs::default());
}

#[test]
fn branch_comparison_cherry_marked_commits_counts_numstat_with_binary_and_tab_names() {
    let now = (js::now_ms() / 1000.0).floor();
    let c = parse::parse_compare(&format!(
        ">\x1fd5c4e81\x1f{}\x1ffix: readme\x1f{}\n=\x1fae3311f\x1f{}\x1fchore: z\x1f{}\n",
        "d".repeat(40),
        now - 60.0,
        "a".repeat(40),
        now - 60.0
    ));
    let got: Vec<(bool, &str, &str)> = c.iter().map(|x| (x.same, x.short.as_str(), x.subject.as_str())).collect();
    assert_eq!(got, [(false, "d5c4e81", "fix: readme"), (true, "ae3311f", "chore: z")]);
    assert_eq!(parse::parse_count("3\t1\n"), [3.0, 1.0]);
    let n = |add: &str, del: &str, path: &str| parse::Numstat { add: add.into(), del: del.into(), path: path.into() };
    assert_eq!(
        parse::parse_numstat("1\t0\tREADME.md\n-\t-\timg.png\n2\t2\ta\tb.txt\n"),
        [n("1", "0", "README.md"), n("-", "-", "img.png"), n("2", "2", "a\tb.txt")]
    );
}

#[test]
fn config_scopes_values_by_key_in_order_and_the_keys_the_repository_sets() {
    let out = "global\0credential.helper\nstore\0local\0credential.helper\nevil\0local\0core.sshcommand\nsh -c x\0worktree\0http.proxy\0";
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        parse::parse_scopes(out),
        [
            ("credential.helper".to_string(), vec![pair("global", "store"), pair("local", "evil")]),
            ("core.sshcommand".to_string(), vec![pair("local", "sh -c x")]),
            ("http.proxy".to_string(), vec![pair("worktree", "")]),
        ]
    );
    assert_eq!(parse::repo_keys(out), ["credential.helper", "core.sshcommand", "http.proxy"]);
    assert!(parse::repo_keys("").is_empty());
}

#[test]
fn error_of_fatal_and_error_lines_refused_pushes_no_output() {
    let e = |code: Option<i32>, err: &str, out: &str| parse::error_of(&Exec { code, err: err.into(), out: out.into() });
    assert_eq!(e(Some(128), "hint: x\nfatal: not a git repository\n", ""), "not a git repository");
    assert_eq!(e(Some(1), "error: pathspec 'x' did not match\n", ""), "pathspec 'x' did not match");
    assert_eq!(
        e(Some(1), "To x\n ! [rejected]        main -> main (non-fast-forward)\nerror: failed to push some refs\n", ""),
        "rejected main -> main (non-fast-forward)"
    );
    assert_eq!(e(Some(1), "", "nothing to commit\n"), "nothing to commit");
    assert_eq!(e(None, "", ""), "timed out or not run");
    assert_eq!(e(Some(1), "", ""), "failed");
}

#[test]
fn labels_a_branch_with_its_remote_one_as_one_the_current_branch_first_tags_last() {
    let refs = parse::ref_list(
        "tag: refs/tags/v1, refs/remotes/origin/old, refs/heads/main, HEAD -> refs/heads/feat, refs/remotes/origin/main, refs/remotes/origin/HEAD",
    );
    let local = |name: &str, remote: bool, current: bool| Label::Local { name: name.into(), remote, current };
    assert_eq!(
        parse::labels(&refs, true, "feat"),
        [
            local("feat", false, true),
            local("main", true, false),
            Label::Remote { name: "origin/old".into() },
            Label::Tag { name: "v1".into() }
        ]
    );
    // Not at HEAD: no current branch, even of the same name.
    assert!(parse::labels(&refs, false, "feat").iter().all(|l| !matches!(l, Label::Local { current: true, .. })));
    // Detached: HEAD is the current one.
    assert_eq!(parse::labels(&parse::ref_list("HEAD, refs/heads/main"), true, "")[0], local("HEAD", false, true));
}
