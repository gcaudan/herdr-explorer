//! The terminal's replies kept apart from the keys, the line a note set sends, notes following
//! their lines, the files viewed, the diff rows. The image cases marked case 9 are placeholders, not written yet.

mod support;

use herdr_explorer::core::notes::{self, Lines};
use herdr_explorer::core::scope::{self, Scope};
use herdr_explorer::core::ui::{self, Event};
use herdr_explorer::core::{exec, paths, viewed};
use herdr_explorer::diff::{self, Collected, Ref, Row, View};
use herdr_explorer::json::Value;
use herdr_explorer::obj;

#[test]
fn terminal_replies_never_read_as_keys() {
    let (mut keys, mut replies) = (Vec::new(), Vec::new());
    for e in ui::decode("a\x1b_Gi=31;OK\x1b\\b\x1b[6;16;8tc") {
        match e {
            Event::Key(k) => keys.push(k),
            Event::Reply(r) => replies.push(r),
            e => panic!("{e:?}"),
        }
    }
    assert_eq!(keys, ["a", "b", "c"]);
    assert_eq!(replies, ["\x1b_Gi=31;OK\x1b\\", "\x1b[6;16;8t"]);
    // Alone, ESC ] and ESC _ are alt+] and alt+_; with more after them, a cut answer.
    assert!(matches!(&ui::decode("\x1b]")[..], [Event::Key(k)] if k == "alt+]"));
    assert!(matches!(&ui::decode("\x1b_")[..], [Event::Key(k)] if k == "alt+_"));
    assert!(matches!(&ui::decode("\x1b]1")[0], Event::Reply(r) if r == "\x1b]"));
    assert_eq!(ui::wrap("abcd ef", 4), ["abcd", "ef"]);
    let mouse: Vec<_> = ui::decode("\x1b[<0;5;7M")
        .into_iter()
        .filter_map(|e| if let Event::Mouse(m) = e { Some(m) } else { None })
        .collect();
    assert_eq!((mouse[0].x, mouse[0].y, mouse[0].press), (5, 7, true));
}

fn strs(v: &[&str]) -> Value {
    Value::Arr(v.iter().map(|&s| s.into()).collect())
}

fn lines(v: &[&str]) -> Lines {
    Lines::Lines(v.iter().map(|s| s.to_string()).collect())
}

fn num(n: &Value, k: &str) -> Option<f64> {
    n.get(k).and_then(Value::as_f64)
}

#[test]
fn notes_go_on_one_line_sorted_relative_to_the_terminal() {
    let list = [
        obj! { "file" => "/r/src/b.js", "from" => 3.0, "to" => 3.0, "kind" => "", "text" => "why", "t" => 2.0 },
        obj! { "file" => "/r/src/a.js", "from" => 12.0, "to" => 14.0, "kind" => "fix", "text" => "rename\nit", "t" => 1.0 },
        obj! { "file" => "/r/README.md", "from" => 0.0, "to" => 0.0, "kind" => "", "text" => "add an example", "t" => 3.0 },
    ];
    let line = notes::format(&list, Some("/r"), "");
    assert_eq!(line, "Review notes: README.md: add an example; src/a.js:12-14 (fix): rename it; src/b.js:3: why ");
    assert!(!line.chars().any(|c| c < '\x20'));
    assert_eq!(notes::where_(&list[0], Some("/elsewhere"), false), "/r/src/b.js:3");
}

#[test]
fn notes_follow_their_lines_or_go_stale_once_they_are_gone() {
    let n = obj! { "file" => "/r/a.js", "from" => 3.0, "to" => 4.0, "text" => "x", "anchor" => strs(&["c", "d"]) };
    assert_eq!(notes::locate(&n, &lines(&["a", "b", "c", "d"])), None, "in place: the same note");
    let moved = notes::locate(&n, &lines(&["new", "a", "b", "c", "d"])).unwrap();
    assert_eq!((num(&moved, "from"), num(&moved, "to")), (Some(4.0), Some(5.0)));
    // In several places: the nearest to where it was.
    let twice = lines(&["c", "d", "x", "x", "x", "c", "d"]);
    assert_eq!(num(&notes::locate(&n, &twice).unwrap(), "from"), Some(1.0));
    let mut later = n.clone();
    later.set("from", Value::Num(5.0));
    later.set("to", Value::Num(6.0));
    assert_eq!(num(&notes::locate(&later, &twice).unwrap(), "from"), Some(6.0));
    let stale = notes::locate(&n, &lines(&["a", "b", "c!", "d"])).unwrap();
    assert_eq!(stale.get("stale"), Some(&Value::Bool(true)));
    assert_eq!(num(&stale, "from"), Some(3.0));
    // Back once the lines are: no longer stale.
    let back = notes::locate(&stale, &lines(&["c", "d"])).unwrap();
    assert_eq!((num(&back, "from"), back.get("stale")), (Some(1.0), None));
    assert_eq!(notes::locate(&n, &Lines::Gone).unwrap().get("stale"), Some(&Value::Bool(true)), "the file gone");
    assert_eq!(notes::locate(&n, &Lines::Unknown), None, "the lines not known: as it was");
    let whole = obj! { "file" => "/r/a.js", "from" => 0.0, "to" => 0.0, "text" => "x" };
    assert_eq!(notes::locate(&whole, &lines(&[])), None);
    let mut fix = stale.clone();
    fix.set("kind", "fix".into());
    assert_eq!(notes::format(&[fix], Some("/r"), " Fix these:"), "Fix these: Review notes: a.js:3-4 (fix, stale): x ");
}

#[test]
fn a_file_stays_viewed_until_its_content_changes() {
    let dir = support::TempDir::new("viewed");
    let file = paths::join(&[&dir.path.to_string_lossy(), "a.txt"]);
    std::fs::write(&file, "one\n").unwrap();
    let map = Value::Obj(vec![(file.clone(), viewed::sign(&file).into())]);
    assert!(viewed::is(&map, &file));
    std::fs::write(&file, "two\n").unwrap();
    assert!(!viewed::is(&map, &file));
    std::fs::remove_file(&file).unwrap();
    assert_eq!(viewed::sign(&file), "gone");
}

/// Rewritten with the same size and times right after it was hashed (a coarse clock): hashed again all the same.
#[test]
fn a_file_changed_within_2_s_of_its_hash_is_hashed_again() {
    let dir = support::TempDir::new("racy");
    let file = paths::join(&[&dir.path.to_string_lossy(), "a.txt"]);
    std::fs::write(&file, "one\n").unwrap();
    let times = std::fs::metadata(&file).unwrap().modified().unwrap();
    let before = viewed::sign(&file);
    std::fs::write(&file, "two\n").unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(times).unwrap();
    assert_ne!(viewed::sign(&file), before);
}

#[test]
fn notes_stay_relative_from_a_folder_reached_through_a_link() {
    let dir = support::TempDir::new("notes");
    let real = dir.path.to_string_lossy().into_owned();
    std::fs::create_dir(dir.path.join("src")).unwrap();
    let link = format!("{real}-link");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&real, &link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&real, &link);
    if made.is_err() {
        return; // links not allowed (Windows without developer mode)
    }
    let file = exec::real_path(&paths::join(&[&link, "src", "a.js"]));
    assert_eq!(file, paths::join(&[&real, "src", "a.js"]));
    let n = obj! { "file" => file, "from" => 12.0, "to" => 12.0 };
    assert_eq!(notes::where_(&n, Some(&link), false), "src/a.js:12");
    let _ = std::fs::remove_file(&link);
    #[cfg(windows)]
    let _ = std::fs::remove_dir(&link);
}

#[test]
fn untracked_files_of_a_subfolder_are_named_from_the_repository() {
    let t = support::TempDir::new("diff");
    if !git_in(&t.path, &["init", "-q"]).status.success() {
        return;
    }
    std::fs::create_dir_all(t.path.join("app/src")).unwrap();
    std::fs::write(t.path.join("app/src/new.js"), "x\n").unwrap();
    let res = diff::collect(&t.path.join("app").to_string_lossy(), "", "head", None, None);
    assert_eq!(names(&res.text), ["app/src/new.js"]);
}

#[test]
fn diff_scopes_a_commit_a_range_a_branch_the_last_agent_turn() {
    let t = support::TempDir::new("scope");
    let top = t.path.to_string_lossy().into_owned();
    let git = |args: &[&str]| git_in(&t.path, args);
    if !git(&["init", "-q", "-b", "main"]).status.success() {
        return;
    }
    let write = |f: &str, text: &str| std::fs::write(t.path.join(f), text).unwrap();
    let commit = |f: &str, text: &str| {
        write(f, text);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", f]);
    };
    commit("a.txt", "one\n");
    commit("b.txt", "two\n");
    git(&["switch", "-q", "-c", "feature"]);
    commit("c.txt", "three\n");
    let collect = |file: &str, sc: Scope, turn: Option<&Value>| diff::collect(&top, file, "head", Some(&sc), turn);
    let names_of = |kind: &str, rev: &str| names(&collect("", Scope::new(kind, Some(rev)), None).text);
    assert_eq!(names_of("rev", "main"), ["b.txt"]);
    assert_eq!(names_of("rev", "main...feature"), ["c.txt"]);
    assert_eq!(names_of("rev", "HEAD~2..HEAD"), ["b.txt", "c.txt"]);
    write("new.txt", "x\n");
    assert_eq!(names_of("ref", "main"), ["c.txt", "new.txt"]);
    assert!(collect("", Scope::new("rev", Some("nope")), None).error.is_some());
    // The turn: what changed since its snapshot, untracked files included; the index stays as it was.
    let tree = scope::worktree(&top, &exec::process_env(), false, None).expect("tree");
    assert_eq!(String::from_utf8_lossy(&git(&["status", "--porcelain"]).stdout), "?? new.txt\n");
    write("a.txt", "one!\n");
    write("other.txt", "y\n");
    let turn = obj! { "top" => top.clone(), "tree" => tree };
    let res = collect("", Scope::new("turn", None), Some(&turn));
    assert!(res.error.is_none(), "{:?}", res.error);
    assert_eq!(names(&res.text), ["a.txt", "other.txt"]);
    // Limited to one file, which alone is added again: a new one untracked too.
    write("third.txt", "z\n");
    let file = |f: &str| t.path.join(f).to_string_lossy().into_owned();
    let one = collect(&file("third.txt"), Scope::new("turn", None), Some(&turn));
    assert_eq!(names(&one.text), ["third.txt"]);
    write("a.txt", "one!!\n");
    let again = collect(&file("a.txt"), Scope::new("turn", None), Some(&turn));
    assert!(again.text.split('\n').any(|l| l == "+one!!"), "{}", again.text);
    assert!(collect("", Scope::new("turn", None), None).error.is_some());
}

#[test]
fn diff_rows_know_the_lines_they_show() {
    let text =
        ["diff --git a/x.js b/x.js", "--- a/x.js", "+++ b/x.js", "@@ -1,3 +1,3 @@", " one", "-two", "+2", " three", ""];
    let parsed = diff::parse(&text.join("\n"));
    for mode in ["inline", "split"] {
        let refs: Vec<String> = diff::render(&parsed, 100, mode, false, &View::default())
            .iter()
            .filter_map(|r| r.r#ref.as_ref().map(|x| format!("{}-{}", x.from, x.to)))
            .collect();
        // The file, the hunk, then its lines: the removed line is at the place of the added one.
        let want: &[&str] = if mode == "inline" {
            &["0-0", "1-3", "1-1", "2-2", "2-2", "3-3"]
        } else {
            &["0-0", "1-3", "1-1", "2-2", "3-3"]
        };
        assert_eq!(refs, want, "{mode}");
    }
}

#[test]
fn diff_rows_wrap_long_lines_or_scroll_them_from_a_column() {
    let long = "abcdefghij".repeat(6);
    let (del, add) = (format!("-{long}"), format!("+{long}!"));
    let text = ["diff --git a/x.js b/x.js", "--- a/x.js", "+++ b/x.js", "@@ -1,2 +1,2 @@", " short", &del, &add, ""];
    let parsed = diff::parse(&text.join("\n"));
    for mode in ["inline", "split"] {
        let rows = diff::render(&parsed, 40, mode, true, &View { wrap: true, ..View::default() });
        // Every row fills the width; a wrapped line goes on with rows marked more, at the same place.
        assert!(rows.iter().filter(|r| r.place != 0).all(|r| ui::seg_width(&r.segs) == 40), "{mode}");
        let more: Vec<&Row> = rows.iter().filter(|r| r.more).collect();
        assert!(more.len() >= 2 && more.iter().all(|r| r.place == 2), "{mode}");
        assert_eq!(
            diff::render(&parsed, 40, mode, true, &View::default()).iter().filter(|r| r.more).count(),
            0,
            "{mode}"
        );
    }
    let cut = diff::render(&parsed, 40, "inline", true, &View { left: 10, ..View::default() });
    // The hunk's header, then the lines: the gutter stays, the text starts at its 11th column.
    assert!(!plain(&cut[1]).contains("short"), "the short line scrolled out");
    assert!(plain(&cut[2]).contains(&format!(" {}", &long[10..30])));
    assert!(cut.iter().filter(|r| r.place != 0).all(|r| ui::seg_width(&r.segs) == 40));
}

#[test]
#[ignore = "case 9"]
fn tiff_strips_decode_whatever_their_compression() {}

#[test]
#[ignore = "case 9"]
fn images_scale_to_fit_never_above_their_size() {}

#[test]
#[ignore = "case 9"]
fn progressive_jpeg_decodes() {}

#[test]
fn the_lines_a_note_takes_from_a_diff_are_those_of_the_version_shown() {
    let staged = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
    let parsed = diff::changes_of(&Collected { staged: Some(staged.into()), ..Collected::default() });
    assert_eq!(diff::shown_lines(&parsed, &note_ref("f", "staged", 2, 3)), Some(vec!["B".into(), "c".into()]));
    assert_eq!(diff::shown_lines(&parsed, &note_ref("f", "staged", 3, 5)), None, "past the hunk");
    assert_eq!(diff::shown_lines(&parsed, &note_ref("f", "unstaged", 2, 2)), None, "another section");
}

#[test]
fn a_diff_row_cut_from_another_column_is_made_again_alike_redo() {
    let text = format!(
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n {}\n-{}\n+{}\n",
        "x".repeat(90),
        "old ".repeat(30),
        "new ".repeat(30)
    );
    let parsed = diff::parse(&text);
    for mode in ["inline", "split"] {
        let at0 = diff::render(&parsed, 80, mode, false, &View::default());
        let at16 = diff::render(&parsed, 80, mode, false, &View { left: 16, ..View::default() });
        assert_eq!(at0.len(), at16.len());
        for (i, r) in at0.iter().enumerate() {
            if let Some(redo) = &r.redo {
                assert_eq!(redo(16), at16[i].segs, "{mode} row {i}");
            }
        }
        assert!(at0.iter().any(|r| r.redo.is_some()));
    }
}

#[test]
fn a_staged_note_is_placed_where_its_lines_are_on_disk() {
    let staged = ["diff --git a/x b/x", "--- a/x", "+++ b/x", "@@ -1,2 +1,2 @@", " one", "-two", "+2", ""].join("\n");
    // On disk: a line added at the top, and line 2 of the index (2) changed.
    let unstaged =
        ["diff --git a/x b/x", "--- a/x", "+++ b/x", "@@ -0,0 +1 @@", "+zero", "@@ -2 +3 @@", "-2", "+deux", ""]
            .join("\n");
    let both = |text: &str| {
        diff::changes_of(&Collected { staged: Some(staged.clone()), text: text.into(), ..Collected::default() })
    };
    let parsed = both(&unstaged);
    assert_eq!(diff::disk_lines(&parsed, &note_ref("x", "staged", 1, 1)), Some((2, 2)));
    assert_eq!(diff::disk_lines(&parsed, &note_ref("x", "staged", 2, 2)), None);
    assert_eq!(diff::disk_lines(&parsed, &note_ref("x", "staged", 1, 2)), None);
    // No unstaged change: the same lines.
    assert_eq!(diff::disk_lines(&both(""), &note_ref("x", "staged", 2, 2)), Some((2, 2)));
}

fn git_in(top: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(top)
        .output()
        .expect("git")
}

fn names(text: &str) -> Vec<String> {
    diff::parse(text).files.into_iter().map(|f| f.to.unwrap_or_default()).collect()
}

fn plain(r: &Row) -> String {
    r.segs.iter().map(|s| s.0.as_str()).collect()
}

fn note_ref(file: &str, sec: &str, from: i64, to: i64) -> Ref {
    Ref { file: file.into(), from, to, sec: sec.into() }
}
