//! test/stage.test.js: staging, unstaging and discarding parts of a diff: the patches made from the rows selected,
//! applied in a repository.

mod support;

use herdr_explorer::core::stage::{self, Mode, Sel, Target};
use herdr_explorer::diff::{self, Diff, File, Hunk, Line, Row, View};
use std::collections::HashSet;
use std::process::Command;
use support::TempDir;

/// A repository with one commit of the files given.
struct Repo {
    dir: TempDir,
    top: String,
}

impl Repo {
    fn new(files: &[(&str, &str)]) -> Option<Repo> {
        let dir = TempDir::new("stage");
        let top = dir.path.to_string_lossy().into_owned();
        let r = Repo { dir, top };
        if !r.git(&["init", "-q"]).status.success() {
            return None;
        }
        // The patches applied to the working tree too, whatever the global config.
        r.git(&["config", "core.autocrlf", "false"]);
        for (f, text) in files {
            r.write(f, text);
        }
        r.git(&["add", "-A"]);
        r.git(&["commit", "-q", "-m", "init"]);
        Some(r)
    }

    fn git(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "core.autocrlf=false"])
            .args(args)
            .current_dir(&self.top)
            .output()
            .expect("git")
    }

    fn out(&self, args: &[&str]) -> String {
        String::from_utf8_lossy(&self.git(args).stdout).into_owned()
    }

    fn path(&self, f: &str) -> std::path::PathBuf {
        self.dir.path.join(f)
    }

    fn write(&self, f: &str, text: &str) {
        std::fs::write(self.path(f), text).unwrap();
    }

    fn read(&self, f: &str) -> String {
        std::fs::read_to_string(self.path(f)).unwrap()
    }

    fn changes(&self) -> Diff {
        diff::changes_of(&diff::collect(&self.top, "", "head", None, None))
    }

    /// apply, with a trash that must not be reached (the tracked lines only).
    fn apply(&self, parsed: &Diff, sel: &Sel, mode: Mode) -> String {
        stage::apply(&self.top, parsed, sel, mode, &|abs| panic!("trash {abs}"), None)
    }
}

fn render(parsed: &Diff, mode: &str) -> Vec<Row> {
    diff::render(parsed, 200, mode, false, &View::default())
}

fn plain(r: &Row) -> String {
    r.segs.iter().map(|s| s.0.as_str()).collect()
}

/// What the rows whose text includes one of the strings given select, in a section.
fn pick(list: &[Row], sec: &str, texts: &[&str]) -> Sel {
    stage::picked(
        list.iter()
            .filter(|r| r.r#ref.as_ref().is_some_and(|x| x.sec == sec) && texts.iter().any(|t| plain(r).contains(t))),
    )
}

/// The rows of a file's name.
fn file_rows<'a>(parsed: &Diff, list: &'a [Row], names: &[&str], sec: Option<&str>) -> Vec<&'a Row> {
    list.iter()
        .filter(|r| {
            r.f.is_some_and(|f| {
                let f = &parsed.files[f];
                f.to.as_deref().is_some_and(|t| names.contains(&t)) && sec.is_none_or(|s| f.sec == s)
            })
        })
        .collect()
}

/// The lines of a hunk matching, as a selection.
fn lines_sel(parsed: &Diff, sec: &str, keep: impl Fn(&Line) -> bool, first: bool) -> Sel {
    let fi = parsed.files.iter().position(|x| x.sec == sec).expect("section");
    let mut sel = Sel::default();
    for (li, l) in parsed.files[fi].hunks[0].lines.iter().enumerate() {
        if keep(l) {
            sel.lines.insert((fi, 0, li));
            if first {
                break;
            }
        }
    }
    sel
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

/// chmod does not stop root, nor Windows.
fn no_perms(dir: &TempDir) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let probe = dir.path.join(".probe");
        std::fs::write(&probe, "x").unwrap();
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o0)).unwrap();
        let readable = std::fs::read(&probe).is_ok();
        let _ = std::fs::remove_file(&probe);
        readable
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        true
    }
}

#[test]
fn a_hunk_then_a_line_go_to_the_index_the_rest_stays_unstaged() {
    let Some(r) = Repo::new(&[("a.txt", &lines(30))]) else { return };
    r.write(
        "a.txt",
        &r.read("a.txt").replacen("line 2\n", "line two\n", 1).replacen("line 28\n", "line 28 changed\nline 28b\n", 1),
    );
    let parsed = r.changes();
    // The second hunk, from its @@ row.
    let list = render(&parsed, "inline");
    let hunks: Vec<&Row> =
        list.iter().filter(|x| x.h.is_some() && x.r#ref.as_ref().is_some_and(|f| f.sec == "unstaged")).collect();
    assert_eq!(hunks.len(), 2);
    assert_eq!(r.apply(&parsed, &stage::picked([hunks[1]]), Mode::Stage), "Staged 3 lines");
    let cached = r.out(&["diff", "--cached"]);
    assert!(cached.contains("+line 28b"), "{cached}");
    assert!(!cached.contains("line two"), "{cached}");
    // One added line of the first hunk: the removed one stays in the index.
    let p2 = r.changes();
    let list = render(&p2, "inline");
    assert_eq!(r.apply(&p2, &pick(&list, "unstaged", &["line two"]), Mode::Stage), "Staged 1 line");
    let index = r.out(&["show", ":a.txt"]);
    assert!(index.contains("line 1\nline 2\nline two\nline 3\n"), "{index}");
    assert!(index.contains("line 28 changed\nline 28b\n"), "{index}");
}

#[test]
fn a_staged_line_goes_back_an_unstaged_one_is_discarded() {
    let Some(r) = Repo::new(&[("a.txt", &lines(5))]) else { return };
    r.write("a.txt", "line 1\nnew A\nline 2\nline 3\nnew B\nline 4\nline 5\n");
    r.git(&["add", "a.txt"]);
    let parsed = r.changes();
    let list = render(&parsed, "split");
    assert_eq!(r.apply(&parsed, &pick(&list, "staged", &["new A"]), Mode::Unstage), "Unstaged 1 line");
    assert_eq!(r.out(&["show", ":a.txt"]), "line 1\nline 2\nline 3\nnew B\nline 4\nline 5\n");
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    assert_eq!(stage::count(&parsed, &pick(&list, "unstaged", &["new A"]), Mode::Discard), "1 line");
    assert_eq!(r.apply(&parsed, &pick(&list, "unstaged", &["new A"]), Mode::Discard), "Discarded 1 line");
    assert_eq!(r.read("a.txt"), "line 1\nline 2\nline 3\nnew B\nline 4\nline 5\n");
    // Nothing of the other section.
    assert_eq!(r.apply(&parsed, &pick(&list, "staged", &["new B"]), Mode::Stage), "Nothing unstaged selected");
}

#[test]
fn part_of_an_untracked_file_is_staged_a_whole_one_is_discarded_from_the_disk() {
    let Some(r) = Repo::new(&[("a.txt", "x\n")]) else { return };
    r.write("new.txt", "one\ntwo\nthree\n");
    r.write("gone.txt", "bye\n");
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    assert_eq!(r.apply(&parsed, &pick(&list, "unstaged", &["+ two"]), Mode::Stage), "Staged 1 line");
    assert_eq!(r.out(&["show", ":new.txt"]), "two\n");
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    let file = stage::picked(file_rows(&parsed, &list, &["gone.txt"], None));
    // To the trash; without one, kept unless deleted for good once asked.
    let trashed = std::cell::RefCell::new(Vec::new());
    let trash = |abs: &str| {
        trashed.borrow_mut().push(abs.to_string());
        std::fs::remove_file(abs).unwrap();
        true
    };
    assert_eq!(stage::apply(&r.top, &parsed, &file, Mode::Discard, &trash, None), "Discarded 1 line");
    assert_eq!(*trashed.borrow(), [r.path("gone.txt").to_string_lossy().into_owned()]);
    r.write("gone.txt", "bye\n");
    let no = |_: &[String]| false;
    let yes = |_: &[String]| true;
    assert_eq!(
        stage::apply(&r.top, &parsed, &file, Mode::Discard, &|_| false, Some(&no)),
        "1 untracked file kept: no trash"
    );
    assert!(r.path("gone.txt").exists());
    assert_eq!(stage::apply(&r.top, &parsed, &file, Mode::Discard, &|_| false, Some(&yes)), "Discarded 1 line");
    assert!(!r.path("gone.txt").exists());
}

#[test]
fn a_file_added_with_git_add_n_keeps_its_entry_when_staged_in_part_loses_it_when_discarded() {
    let Some(r) = Repo::new(&[("a.txt", "x\n")]) else { return };
    r.write("n.txt", "one\ntwo\n");
    r.git(&["add", "-N", "n.txt"]);
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    assert_eq!(r.apply(&parsed, &pick(&list, "unstaged", &["+ one"]), Mode::Stage), "Staged 1 line");
    assert_eq!(r.out(&["show", ":n.txt"]), "one\n");
    r.git(&["reset", "-q", "--", "n.txt"]);
    r.git(&["add", "-N", "n.txt"]);
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    let file = stage::picked(file_rows(&parsed, &list, &["n.txt"], Some("unstaged")));
    let trash = |abs: &str| std::fs::remove_file(abs).is_ok();
    assert_eq!(stage::apply(&r.top, &parsed, &file, Mode::Discard, &trash, None), "Discarded 2 lines");
    assert_eq!(r.out(&["status", "--porcelain"]), "");
}

#[test]
fn lines_ending_with_crlf_keep_it_in_the_patch() {
    let Some(r) = Repo::new(&[("w.txt", "a\r\nb\r\nc\r\n")]) else { return };
    r.write("w.txt", "a\r\nB\r\nc\r\nd\r\n");
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    assert_eq!(r.apply(&parsed, &pick(&list, "unstaged", &["+ d"]), Mode::Stage), "Staged 1 line");
    assert_eq!(r.out(&["show", ":w.txt"]), "a\r\nb\r\nc\r\nd\r\n");
}

#[test]
fn paths_with_quotes_or_backslashes_are_quoted_as_git_does() {
    assert_eq!(stage::quote("a/plain name.txt"), "a/plain name.txt");
    assert_eq!(stage::quote("a/say \"hi\"\\.txt"), "\"a/say \\\"hi\\\"\\\\.txt\"");
    assert_eq!(stage::quote("a/tab\there"), "\"a/tab\\there\"");
}

#[test]
fn a_line_added_after_a_last_line_without_newline_is_staged_on_a_line_of_its_own() {
    let Some(r) = Repo::new(&[("f.txt", "a\nb")]) else { return };
    r.write("f.txt", "a\nb\nc\n");
    let parsed = r.changes();
    let sel = lines_sel(&parsed, "unstaged", |l| l.sign == '+' && l.text == "c", true);
    assert_eq!(r.apply(&parsed, &sel, Mode::Stage), "Staged 1 line");
    assert_eq!(r.out(&["show", ":f.txt"]), "a\nb\nc\n");
}

#[test]
fn a_last_line_without_newline_discarded_alone_comes_back_before_the_line_added() {
    for now in ["a\nnew\n", "a\nnew"] {
        let Some(r) = Repo::new(&[("f.txt", "a\nold")]) else { return };
        r.write("f.txt", now);
        let parsed = r.changes();
        let sel = lines_sel(&parsed, "unstaged", |l| l.sign == '-' && l.text == "old", true);
        assert_eq!(r.apply(&parsed, &sel, Mode::Discard), "Discarded 1 line");
        assert_eq!(r.read("f.txt"), format!("a\nold\n{}", &now[2..]));
    }
}

#[test]
fn part_of_a_staged_deletion_goes_back_to_the_index() {
    let Some(r) = Repo::new(&[("g.txt", "x\ny\nz\n")]) else { return };
    r.git(&["rm", "-q", "g.txt"]);
    let parsed = r.changes();
    let sel = lines_sel(&parsed, "staged", |l| l.text == "y", true);
    assert_eq!(r.apply(&parsed, &sel, Mode::Unstage), "Unstaged 1 line");
    assert_eq!(r.out(&["show", ":g.txt"]), "y\n");
}

fn line(sign: char, text: &str, old: Option<i64>, new: Option<i64>) -> Line {
    Line { sign, text: text.into(), old, new, at: new.unwrap_or(1), cr: false }
}

#[test]
fn a_whole_file_takes_its_mode_change_with_it() {
    let f = File {
        from: Some("h.sh".into()),
        to: Some("h.sh".into()),
        status: "modified".into(),
        old_mode: Some("100644".into()),
        new_mode: Some("100755".into()),
        hunks: vec![Hunk {
            head: "@@ -1 +1,2 @@".into(),
            context: String::new(),
            lines: vec![line(' ', "m", Some(1), Some(1)), line('+', "n", None, Some(2))],
        }],
        ..File::default()
    };
    let parsed = Diff { files: vec![f], ..Diff::default() };
    let keep: HashSet<(usize, usize)> = [(0, 1)].into();
    let all = stage::patch_of(&parsed, &Target { f: 0, keep: keep.clone(), all: true }, Mode::Stage).unwrap();
    assert!(all.contains("\nold mode 100644\nnew mode 100755\n--- a/h.sh"), "{all}");
    let part = stage::patch_of(&parsed, &Target { f: 0, keep, all: false }, Mode::Stage).unwrap();
    assert!(!part.contains("old mode"), "{part}");
}

#[test]
fn part_of_a_replaced_block_keeps_the_lines_in_their_order() {
    for (mode, pick_text, index, tree) in [
        (Mode::Discard, "c", "a\nb\nc\nz\n", "a\nB\nc\nz\n"),
        (Mode::Stage, "b", "a\nB\nc\nz\n", "a\nB\nC\nz\n"),
        (Mode::Unstage, "c", "a\nB\nc\nz\n", "a\nB\nC\nz\n"),
    ] {
        let Some(r) = Repo::new(&[("f.txt", "a\nb\nc\nz\n")]) else { return };
        r.write("f.txt", "a\nB\nC\nz\n");
        if mode == Mode::Unstage {
            r.git(&["add", "f.txt"]);
        }
        let parsed = r.changes();
        let sel = lines_sel(&parsed, mode.section(), |l| l.text.to_lowercase() == pick_text, false);
        let verb = if mode.section() == "staged" {
            "Unstaged"
        } else if mode == Mode::Stage {
            "Staged"
        } else {
            "Discarded"
        };
        assert_eq!(r.apply(&parsed, &sel, mode), format!("{verb} 2 lines"));
        assert_eq!(r.out(&["show", ":f.txt"]), index);
        assert_eq!(r.read("f.txt"), tree);
    }
}

#[test]
fn the_last_line_added_to_a_file_without_newline_is_taken_out_without_adding_one() {
    for mode in [Mode::Discard, Mode::Unstage] {
        let Some(r) = Repo::new(&[("f.txt", "a\nb")]) else { return };
        r.write("f.txt", "a\nb\nc");
        if mode == Mode::Unstage {
            r.git(&["add", "f.txt"]);
        }
        let parsed = r.changes();
        let sel = lines_sel(&parsed, mode.section(), |l| l.sign == '+' && l.text == "c", true);
        r.apply(&parsed, &sel, mode);
        assert_eq!(if mode == Mode::Discard { r.read("f.txt") } else { r.out(&["show", ":f.txt"]) }, "a\nb");
    }
}

#[test]
fn discarding_for_good_goes_on_after_a_file_it_cannot_delete() {
    let Some(r) = Repo::new(&[("a.txt", "x\n")]) else { return };
    if no_perms(&r.dir) {
        return;
    }
    std::fs::create_dir(r.path("ro")).unwrap();
    r.write("ro/a.txt", "a\n");
    r.write("b.txt", "b\n");
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    let files = stage::picked(file_rows(&parsed, &list, &["ro/a.txt", "b.txt"], None));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |m| std::fs::set_permissions(r.path("ro"), std::fs::Permissions::from_mode(m)).unwrap();
        mode(0o555);
        let yes = |_: &[String]| true;
        let said = stage::apply(&r.top, &parsed, &files, Mode::Discard, &|_| false, Some(&yes));
        mode(0o755);
        assert_eq!(said, "Discarded 1 line; cannot delete ro/a.txt: EACCES");
        assert!(r.path("ro/a.txt").exists());
        assert!(!r.path("b.txt").exists());
    }
}

#[test]
fn whole_files_failing_after_a_patch_say_what_was_staged() {
    let Some(r) = Repo::new(&[("a.txt", "one\n")]) else { return };
    if no_perms(&r.dir) {
        return;
    }
    r.write("a.txt", "one\ntwo\n");
    r.write("u.txt", "u\n");
    let parsed = r.changes();
    let list = render(&parsed, "inline");
    let mut rows: Vec<&Row> = list
        .iter()
        .filter(|x| x.r#ref.as_ref().is_some_and(|f| f.sec == "unstaged") && plain(x).contains("+ two"))
        .collect();
    rows.extend(file_rows(&parsed, &list, &["u.txt"], None));
    let sel = stage::picked(rows);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |m| std::fs::set_permissions(r.path("u.txt"), std::fs::Permissions::from_mode(m)).unwrap();
        mode(0o0);
        let said = r.apply(&parsed, &sel, Mode::Stage);
        mode(0o644);
        assert!(said.starts_with("Staged 1 line; cannot stage u.txt: "), "{said}");
        assert!(r.out(&["diff", "--cached"]).contains("+two"));
    }
}

#[cfg(unix)]
#[test]
fn within_refuses_a_path_whose_folder_links_outside_the_repository() {
    use herdr_explorer::core::paths::join;
    let (t, o) = (support::TempDir::new("within"), support::TempDir::new("outside"));
    let top = t.path.to_string_lossy().into_owned();
    std::fs::create_dir(t.path.join("dir")).unwrap();
    std::os::unix::fs::symlink(&o.path, t.path.join("sub")).unwrap();
    assert!(stage::within(&join(&[&top, "a.txt"]), &top));
    assert!(stage::within(&join(&[&top, "dir/a.txt"]), &top));
    assert!(!stage::within(&join(&[&top, "sub/x"]), &top));
    assert!(!stage::within(&join(&[&top, "../x"]), &top));
}

#[test]
fn the_lines_changed_since_head_staged_or_not_as_the_file_view_marks_them() {
    use herdr_explorer::core::buffer::LineKind;
    let Some(r) = Repo::new(&[("a.txt", &lines(10)), (".gitignore", "ignored.txt\n")]) else { return };
    let text = r.read("a.txt").replacen("line 2\n", "line two\n", 1).replacen("line 5\n", "", 1);
    r.write("a.txt", &text.replacen("line 10\n", "", 1).replacen("line 7\n", "line 7\nline 7b\n", 1));
    r.git(&["add", "a.txt"]);
    r.write("a.txt", &format!("line 0\n{}", r.read("a.txt")));
    let file = |f: &str| r.path(f).to_string_lossy().into_owned();
    let res = diff::line_changes(&file("a.txt"), 10).expect("changes");
    // line 0, line 1, line two, line 3, line 4, line 6, line 7, line 7b, line 8, line 9
    let marked: Vec<(usize, &str)> =
        res.kinds.iter().enumerate().filter(|(_, k)| **k != LineKind::Same).map(|(i, k)| (i, k.name())).collect();
    assert_eq!(marked, [(0, "add"), (2, "mod"), (7, "add")]);
    assert_eq!(res.deleted.iter().copied().collect::<Vec<_>>(), [5, 10]);
    r.write("new.txt", "a\nb\n");
    assert_eq!(diff::line_changes(&file("new.txt"), 2).unwrap().kinds, [LineKind::Add, LineKind::Add]);
    r.write("ignored.txt", "a\n");
    let ignored = diff::line_changes(&file("ignored.txt"), 1).unwrap();
    assert!(ignored.kinds.iter().all(|k| *k == LineKind::Same) && ignored.deleted.is_empty());
    let out = TempDir::new("nogit");
    std::fs::write(out.path.join("f.txt"), "a\n").unwrap();
    if !Command::new("git").arg("rev-parse").current_dir(&out.path).output().is_ok_and(|o| o.status.success()) {
        assert_eq!(diff::line_changes(&out.path.join("f.txt").to_string_lossy(), 1), None);
    }
}

#[test]
#[ignore = "case 16"]
fn the_fork_point_of_the_history_where_the_branch_leaves_main_none_on_main() {}
