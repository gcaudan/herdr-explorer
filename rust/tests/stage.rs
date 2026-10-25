//! test/stage.test.js: staging, unstaging and discarding parts of a diff. The tests that read a repository's diff
//! (diff.collect, changesOf, render) wait for the step that ports them.

mod support;

use herdr_explorer::core::stage::{self, Mode, Target};
use herdr_explorer::diff::{Diff, File, Hunk, Line};
use std::collections::HashSet;

#[test]
#[ignore = "case 15"]
fn a_hunk_then_a_line_go_to_the_index_the_rest_stays_unstaged() {}

#[test]
#[ignore = "case 15"]
fn a_staged_line_goes_back_an_unstaged_one_is_discarded() {}

#[test]
#[ignore = "case 15"]
fn part_of_an_untracked_file_is_staged_a_whole_one_is_discarded_from_the_disk() {}

#[test]
#[ignore = "case 15"]
fn a_file_added_with_git_add_n_keeps_its_entry_when_staged_in_part_loses_it_when_discarded() {}

#[test]
#[ignore = "case 15"]
fn lines_ending_with_crlf_keep_it_in_the_patch() {}

#[test]
fn paths_with_quotes_or_backslashes_are_quoted_as_git_does() {
    assert_eq!(stage::quote("a/plain name.txt"), "a/plain name.txt");
    assert_eq!(stage::quote("a/say \"hi\"\\.txt"), "\"a/say \\\"hi\\\"\\\\.txt\"");
    assert_eq!(stage::quote("a/tab\there"), "\"a/tab\\there\"");
}

#[test]
#[ignore = "case 15"]
fn a_line_added_after_a_last_line_without_newline_is_staged_on_a_line_of_its_own() {}

#[test]
#[ignore = "case 15"]
fn a_last_line_without_newline_discarded_alone_comes_back_before_the_line_added() {}

#[test]
#[ignore = "case 15"]
fn part_of_a_staged_deletion_goes_back_to_the_index() {}

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
#[ignore = "case 15"]
fn part_of_a_replaced_block_keeps_the_lines_in_their_order() {}

#[test]
#[ignore = "case 15"]
fn the_last_line_added_to_a_file_without_newline_is_taken_out_without_adding_one() {}

#[test]
#[ignore = "case 15"]
fn discarding_for_good_goes_on_after_a_file_it_cannot_delete() {}

#[test]
#[ignore = "case 15"]
fn whole_files_failing_after_a_patch_say_what_was_staged() {}

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
#[ignore = "case 15"]
fn the_lines_changed_since_head_staged_or_not_as_the_file_view_marks_them() {}

#[test]
#[ignore = "case 16"]
fn the_fork_point_of_the_history_where_the_branch_leaves_main_none_on_main() {}
