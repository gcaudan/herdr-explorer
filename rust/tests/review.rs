//! test/review.test.js: the line a note set sends, notes following their lines, the files viewed. The terminal's
//! replies, the diff rows and the image decoders wait for the steps that port them.

mod support;

use herdr_explorer::core::notes::{self, Lines};
use herdr_explorer::core::{exec, paths, viewed};
use herdr_explorer::json::Value;
use herdr_explorer::obj;

#[test]
#[ignore = "case 6a"]
fn terminal_replies_never_read_as_keys() {}

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
#[ignore = "case 15"]
fn untracked_files_of_a_subfolder_are_named_from_the_repository() {}

#[test]
#[ignore = "case 15"]
fn diff_scopes_a_commit_a_range_a_branch_the_last_agent_turn() {}

#[test]
#[ignore = "case 15"]
fn diff_rows_know_the_lines_they_show() {}

#[test]
#[ignore = "case 15"]
fn diff_rows_wrap_long_lines_or_scroll_them_from_a_column() {}

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
#[ignore = "case 15"]
fn the_lines_a_note_takes_from_a_diff_are_those_of_the_version_shown() {}

#[test]
#[ignore = "case 15"]
fn a_diff_row_cut_from_another_column_is_made_again_alike_redo() {}

#[test]
#[ignore = "case 15"]
fn a_staged_note_is_placed_where_its_lines_are_on_disk() {}
