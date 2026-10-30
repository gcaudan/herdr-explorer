//! The file written back as it was read, the cursor's columns over wide characters, marks and
//! tabs, undo steps, find and replace, and the lines marked as changed. Offsets are in bytes.

mod support;

use herdr_explorer::core::buffer::{self, Editor, LineKind, Match, Pos};
use std::fs;
use support::TempDir;

fn doc(text: &str) -> buffer::Doc {
    buffer::parse(text.as_bytes()).unwrap()
}

fn editor(text: &str) -> Editor {
    Editor::new(&doc(text))
}

fn round(text: &str) -> String {
    doc(text).serialize()
}

#[test]
fn line_endings_final_newline_and_bom_are_kept() {
    for t in ["", "a", "a\n", "a\nb", "a\r\nb\r\n", "a\r\nb", "\n", "\n\n", "\u{feff}x\r\n", "a\rb\n"] {
        assert_eq!(round(t), t, "{t:?}");
    }
    assert_eq!(doc("a\r\nb\r\n").eol, "\r\n");
    assert_eq!(doc("a\nb\n").eol, "\n");
    // Mixed: the line ending most lines have, on every line.
    let mixed = doc("a\r\nb\r\nc\n");
    assert_eq!((mixed.eol, mixed.mixed, mixed.serialize().as_str()), ("\r\n", true, "a\r\nb\r\nc\r\n"));
    // An edit keeps them.
    let mut ed = editor("one\r\ntwo");
    ed.place(Pos::new(1, 3), false);
    ed.newline();
    ed.type_("three", "");
    assert_eq!(ed.serialize(), "one\r\ntwo\r\nthree");
}

#[test]
fn binary_other_encodings_and_large_files_are_refused() {
    assert!(buffer::parse(b"a\0b").is_err());
    assert!(buffer::parse(&[0x61, 0xe9, 0x62]).is_err(), "Latin-1");
    assert!(buffer::parse(&vec![0x61; buffer::MAX_BYTES + 1]).is_err());
    assert!(buffer::parse("héllo ✓ 中文".as_bytes()).is_ok());
}

/// A scratch folder holding a.sh, removed after the test.
fn scratch(f: impl FnOnce(&std::path::Path, &str)) {
    let dir = TempDir::new("edit");
    let file = dir.path.join("a.sh");
    fs::write(&file, "old").unwrap();
    f(&dir.path, file.to_str().unwrap());
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

#[test]
fn the_save_replaces_the_file_whole_leaving_no_temporary_file() {
    scratch(|dir, file| {
        let stat = buffer::save(file, "new").unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "new");
        assert_eq!(stat.len(), 3);
        assert_eq!(names(dir), ["a.sh"], "no temporary file left");
        assert_eq!(buffer::read(file).unwrap().0.lines, ["new"]);
    });
}

#[test]
fn a_read_only_file_is_not_saved() {
    scratch(|_, file| {
        let mut perm = fs::metadata(file).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(file, perm.clone()).unwrap();
        let before = fs::read(file).unwrap();
        assert_eq!(buffer::save(file, "new").unwrap_err().to_string(), "read-only");
        assert_eq!(fs::read(file).unwrap(), before);
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        fs::set_permissions(file, perm).unwrap();
    });
}

// Windows' chmod only knows read-only.
#[cfg(unix)]
#[test]
fn the_save_keeps_the_mode() {
    use std::os::unix::fs::PermissionsExt;
    scratch(|_, file| {
        fs::set_permissions(file, fs::Permissions::from_mode(0o750)).unwrap();
        buffer::save(file, "new").unwrap();
        assert_eq!(fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o750);
    });
}

#[test]
fn the_save_writes_through_a_link_keeping_it() {
    scratch(|dir, file| {
        let link = dir.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink("a.sh", &link).unwrap();
        #[cfg(windows)]
        if let Err(e) = std::os::windows::fs::symlink_file("a.sh", &link) {
            // Windows without developer mode refuses links.
            if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(1314) {
                return;
            }
            panic!("{e}");
        }
        buffer::save(link.to_str().unwrap(), "new").unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "new");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "still a link");
        assert_eq!(names(dir), ["a.sh", "link"], "no temporary file left");
    });
}

#[test]
fn columns_over_tabs_wide_characters_combining_marks_and_emoji() {
    let s = buffer::shown("a\tb中e\u{301}x");
    assert_eq!(s.text, "a   b中e\u{301}x");
    // The bytes where a, \t, b, 中, e, the mark, x and the end start.
    assert_eq!([0, 1, 2, 3, 6, 7, 9, 10].map(|i| s.col[i]), [0, 1, 4, 5, 7, 8, 8, 9]);
    // The cursor steps over a character with its marks, a flag, a joined emoji.
    let t = "e\u{301}🇫🇷👩\u{200d}💻z";
    let mut stops = Vec::new();
    let mut i = 0;
    while i < t.len() {
        stops.push(i);
        i = buffer::next_at(t, i);
    }
    assert_eq!(stops, [0, 3, 11, 22]);
    assert_eq!(buffer::prev_at(t, 22), 11);
    assert_eq!(buffer::prev_at(t, 11), 3);
    // A column inside a wide character is the character.
    assert_eq!(buffer::at_col("中文x", 1), 0);
    assert_eq!(buffer::at_col("中文x", 3), 3);
    assert_eq!(buffer::at_col("中文x", 9), 7);
    assert_eq!(buffer::at_col("\tx", 2), 0);
    assert_eq!(buffer::at_col("\tx", 4), 1);
}

#[test]
fn moves_arrows_keep_their_column_words_home() {
    let mut ed = editor("  hello world\n中文\nabcdefgh");
    ed.place(Pos::new(0, 6), false);
    ed.move_("down", false, 10);
    assert_eq!(ed.cur, Pos::new(1, 6), "column 6 is past 中文 (4 columns)");
    ed.move_("down", false, 10);
    assert_eq!(ed.cur, Pos::new(2, 6), "the column is kept");
    ed.place(Pos::new(0, 13), false);
    ed.move_("wordleft", false, 10);
    assert_eq!(ed.cur.at, 8);
    ed.move_("wordleft", false, 10);
    assert_eq!(ed.cur.at, 2);
    ed.move_("wordright", false, 10);
    assert_eq!(ed.cur.at, 7);
    ed.move_("home", false, 10);
    assert_eq!(ed.cur.at, 2);
    ed.move_("home", false, 10);
    assert_eq!(ed.cur.at, 0);
    ed.move_("end", true, 10);
    assert_eq!(ed.selected(), "  hello world");
    ed.move_("left", false, 10);
    assert_eq!((ed.cur.at, ed.selection()), (0, None), "← folds the selection to its start");
}

#[test]
fn editing_enter_keeps_the_indentation_backspace_joins_lines_tab_indents() {
    let mut ed = editor("    if (x) {\n}\n");
    ed.place(Pos::new(0, 12), false);
    ed.newline();
    ed.indent_lines(1);
    ed.type_("y", "");
    assert_eq!(ed.serialize(), "    if (x) {\n        y\n}\n");
    assert_eq!(ed.indent, "    ");
    ed.select_all();
    ed.indent_lines(-1);
    assert_eq!(ed.serialize(), "if (x) {\n    y\n}\n");
    ed.place(Pos::new(1, 0), false);
    ed.erase(-1, false);
    assert_eq!(ed.lines[0], "if (x) {    y");
    ed.place(Pos::new(0, 13), false);
    ed.erase(-1, true);
    assert_eq!(ed.lines[0], "if (x) {    ");
    let tabs = editor("a\n\tb\n\tc\n");
    assert_eq!(tabs.indent, "\t");
}

#[test]
fn tab_over_a_selection_on_one_line_reaches_the_stop_from_its_start() {
    let mut ed = editor("    x\nab cdef\n");
    ed.place(Pos::new(1, 1), false);
    ed.place(Pos::new(1, 3), true);
    ed.indent_lines(1);
    assert_eq!(ed.lines[1], "a   cdef");
}

#[test]
fn backspace_after_a_selection_shrunk_to_nothing_erases_before_the_cursor() {
    // shift+→ then shift+← leave the selection's other end at the cursor: the erase must not take it as a selection.
    let mut ed = editor("abcdef");
    ed.place(Pos::new(0, 3), false);
    ed.place(Pos::new(0, 4), true);
    ed.place(Pos::new(0, 3), true);
    ed.erase(-1, false);
    ed.erase(-1, false);
    assert_eq!(ed.serialize(), "adef");
    // Nor once the line it was on is gone.
    let mut joined = editor("a\nb");
    joined.place(Pos::new(1, 0), false);
    joined.place(Pos::new(1, 0), true);
    joined.erase(-1, false);
    joined.erase(-1, false);
    assert_eq!(joined.serialize(), "b");
}

#[test]
fn undo_groups_typed_characters_by_word_redo_comes_back() {
    let mut ed = editor("");
    for ch in "hello world".chars() {
        ed.type_(&ch.to_string(), "type");
    }
    assert_eq!(ed.text(), "hello world");
    assert!(ed.dirty());
    ed.undo();
    assert_eq!(ed.text(), "hello");
    ed.undo();
    assert_eq!(ed.text(), "");
    assert!(!ed.dirty(), "back as opened");
    ed.redo();
    ed.redo();
    assert_eq!(ed.text(), "hello world");
    // A move starts a new step; so does a save.
    ed.move_("home", false, 10);
    ed.type_(">", "type");
    ed.move_("end", false, 10);
    ed.type_("!", "type");
    ed.undo();
    assert_eq!(ed.text(), ">hello world");
    ed.mark_saved();
    ed.type_("?", "type");
    ed.undo();
    assert!(!ed.dirty());
    // Backspaces join; a typed character after them is a step of its own.
    ed.erase(-1, false);
    ed.erase(-1, false);
    ed.undo();
    assert_eq!(ed.text(), ">hello world");
    assert_eq!(ed.cur, Pos::new(0, 12));
}

#[test]
fn a_paste_keeps_its_text_tabs_and_lines_as_one_step() {
    let mut ed = editor("ab");
    ed.place(Pos::new(0, 1), false);
    ed.type_("x\ty\n\tz", "");
    assert_eq!(ed.lines, ["ax\ty", "\tzb"]);
    assert_eq!(ed.cur, Pos::new(1, 2));
    ed.undo();
    assert_eq!(ed.lines, ["ab"]);
}

#[test]
fn find_replace_one_and_all() {
    let mut ed = editor("foo Foo\nbar foo");
    assert_eq!(buffer::matches(&ed.lines, "foo").len(), 3, "any case");
    assert_eq!(buffer::matches(&ed.lines, "Foo").len(), 1, "a capital: exact case");
    ed.place(Pos::new(0, 1), false);
    let m = |line, a, b| Some(Match { line, a, b });
    assert_eq!(ed.find("foo", 1, false), m(0, 4, 7));
    assert_eq!(ed.find("foo", 1, false), m(1, 4, 7));
    assert_eq!(ed.find("foo", 1, false), m(0, 0, 3), "round");
    assert_eq!(ed.find("foo", -1, false), m(1, 4, 7));
    assert!(ed.replace_one("foo", "qux"));
    assert_eq!(ed.text(), "foo Foo\nbar qux");
    assert_eq!(ed.replace_all("foo", "x"), 2);
    assert_eq!(ed.text(), "x x\nbar qux");
    ed.undo();
    assert_eq!(ed.text(), "foo Foo\nbar qux");
}

fn lines(s: &str) -> Vec<String> {
    s.split(' ').map(str::to_string).collect()
}

#[test]
fn changed_lines_added_modified_and_where_lines_were_removed() {
    let kinds = |a: &str, b: &str| {
        let d = buffer::line_diff(&lines(a), &lines(b), 1000);
        let k: String = d.kinds.iter().map(|k| k.name().chars().next().unwrap_or('.')).collect();
        (k, d.deleted.into_iter().collect::<Vec<_>>())
    };
    assert_eq!(kinds("a b c", "a b c"), ("...".into(), vec![]));
    assert_eq!(kinds("a b c", "a x b c"), (".a..".into(), vec![]));
    assert_eq!(kinds("a b c", "a B c"), (".m.".into(), vec![]));
    assert_eq!(kinds("a b c d", "a d"), ("..".into(), vec![1]));
    assert_eq!(kinds("a b c", "a b"), ("..".into(), vec![2]));
    assert_eq!(kinds("a b c d e", "x b c y e"), ("m..m.".into(), vec![]));
    assert_eq!(kinds("a b c d e f", "b c X e f g"), ("..m..a".into(), vec![0]));
    // Past the limit, the whole middle counts as changed.
    let d = buffer::line_diff(&lines("a 1 2 z"), &lines("a 2 1 z"), 0);
    assert_eq!(d.kinds, [LineKind::Same, LineKind::Mod, LineKind::Mod, LineKind::Same]);
    // A large file with a change at each end stays quick.
    let big: Vec<String> = (0..50000).map(|i| format!("line {i}")).collect();
    let mut edited = big.clone();
    edited[0] = "first".into();
    edited.remove(40000);
    let t = std::time::Instant::now();
    let r = buffer::line_diff(&big, &edited, 1000);
    assert!(t.elapsed().as_millis() < 1000);
    let changed = r.kinds.iter().filter(|&&k| k != LineKind::Same).count();
    assert_eq!((r.kinds[0], changed, r.deleted.into_iter().collect::<Vec<_>>()), (LineKind::Mod, 1, vec![40000]));
}
