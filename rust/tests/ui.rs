//! test/ui.test.js: the terminal width of text and the segment helpers built on it (a row must take exactly the
//! columns the terminal gives it), the cards, the menus and the scrollbar.

use herdr_explorer::core::ui::{self, Ask, BarMark, Button, Choice, Confirm, Menu, MenuItem, Mouse, Scrollbar, Seg};
use std::sync::Mutex;

/// Held by every test: the theme is the process's, and one test changes it.
static THEME: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    THEME.lock().unwrap_or_else(|e| e.into_inner())
}

/// The text without its colours: `s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, '')`.
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\x1b[") {
        out += &rest[..i];
        let after = &rest[i + 2..];
        let n = after.bytes().take_while(|b| b.is_ascii_digit() || *b == b';' || *b == b'?').count();
        match after.as_bytes().get(n) {
            Some(b) if b.is_ascii_alphabetic() => rest = &after[n + 1..],
            _ => {
                out += "\x1b[";
                rest = after;
            }
        }
    }
    out + rest
}

fn text(segs: &[Seg]) -> String {
    ui::seg_text(segs)
}

fn s(t: &str) -> Seg {
    ui::seg(t, "")
}

#[test]
fn character_widths() {
    let _l = lock();
    let cases = [
        ("a", 1),
        ("中", 2),
        ("😀", 2),
        ("✅", 2),
        ("☰", 2),
        ("≡", 1),
        ("⚠", 1),
        ("⚠️", 2),
        ("🇫🇷", 2),
        ("é", 1),
        ("1️⃣", 2),
    ];
    for (t, w) in cases {
        assert_eq!(ui::text_width(t), w, "{t:?}");
    }
}

#[test]
fn the_emoji_selector_counts_across_segments() {
    let _l = lock();
    assert_eq!(ui::seg_width(&[s("⚠"), s("\u{fe0f}c")]), 3);
    assert_eq!(ui::text_width(&plain(&ui::line(&[s("⚠"), s("\u{fe0f}c")], 4, None))), 4);
}

#[test]
fn split_keeps_a_character_with_its_emoji_selector() {
    let _l = lock();
    let texts = |lines: Vec<Vec<Seg>>| lines.iter().map(|l| text(l)).collect::<Vec<_>>();
    assert_eq!(texts(ui::split(&[s("ab⚠️cd")], 3)), ["ab", "⚠️c", "d"]);
    assert_eq!(texts(ui::split(&[ui::seg("ab⚠", "x"), ui::seg("\u{fe0f}", "y")], 3)), ["ab", "⚠️"]);
    assert_eq!(texts(ui::split(&[s("a🌡️")], 2)), ["a", "🌡️"]);
}

#[test]
fn slice_puts_a_space_for_a_selector_whose_character_is_cut() {
    let _l = lock();
    assert_eq!(text(&ui::slice(&[s("a⚠️bc")], 2, 3)), " bc");
    assert_eq!(text(&ui::slice(&[s("a⚠️bc")], 1, 3)), "⚠️b");
}

/// Random segment lists: every helper keeps the text and the width it promises.
#[test]
fn random_text_keeps_its_width() {
    let _l = lock();
    let parts = ["a", "b", " ", "中", "😀", "✅", "☰", "⚠", "\u{fe0f}", "🇫", "🇷", "é", "❤"];
    let mut seed: u64 = 42;
    let mut rand = |n: usize| {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        (seed % n as u64) as usize
    };
    for _ in 0..3000 {
        let count = 1 + rand(4);
        let segs: Vec<Seg> = (0..count)
            .map(|_| {
                let len = rand(12);
                s(&(0..len).map(|_| parts[rand(parts.len())]).collect::<String>())
            })
            .collect();
        let width = 2 + rand(20);
        let all = text(&segs);
        let lines = ui::split(&segs, width);
        assert_eq!(lines.iter().map(|l| text(l)).collect::<String>(), all, "split keeps the text");
        for l in &lines {
            assert!(ui::seg_width(l) <= width, "split line over {width}: {l:?}");
        }
        assert_eq!(ui::text_width(&plain(&ui::line(&segs, width, None))), width, "line fills {width}: {segs:?}");
        let from = rand(10);
        assert!(ui::seg_width(&ui::slice(&segs, from, width)) <= width, "slice over {width}: {segs:?}");
    }
}

#[test]
fn key_hints_keep_the_pairs_that_fit_whole() {
    let _l = lock();
    let pairs = [("s", "stage"), ("d", "discard"), ("q", "quit")];
    assert_eq!(text(&ui::keys(&pairs, usize::MAX).segs), " s stage   d discard   q quit");
    assert_eq!(text(&ui::keys(&pairs, 20).segs), " s stage   d discard");
    assert!(ui::keys(&pairs, usize::MAX).segs[0].1.contains("\x1b[1m"));
}

#[test]
fn a_click_on_a_hint_gives_its_key() {
    let _l = lock();
    let hints = ui::keys(&[("⏎", "open"), ("↑↓", "move"), ("space s", "stage")], usize::MAX);
    // " ⏎ open   ↑↓ move   space s stage"
    assert_eq!(ui::key_at(&hints, 1, 0).as_deref(), Some("enter"));
    assert_eq!(ui::key_at(&hints, 11, 0), None);
    assert_eq!(ui::key_at(&hints, 21, 0).as_deref(), Some("space"));
    assert_eq!(ui::key_at(&hints, 0, 0), None);
    assert_eq!(ui::key_at(&hints, 22, 1).as_deref(), Some("space"));
}

#[test]
fn a_question_card_fits_the_pane_buttons_included() {
    let _l = lock();
    for (w, h) in [(80, 30), (26, 12), (12, 8), (10, 5), (30, 4), (30, 3), (2, 10), (1, 10)] {
        let mut lines: Vec<String> = (0..h).map(|_| " ".repeat(w as usize)).collect();
        let mut q = Confirm {
            title: "Move to the trash?".into(),
            name: "a name long enough to wrap on a few lines.txt".into(),
            where_: Some("in src/".into()),
            note: Some("This cannot be undone.".into()),
            action: "Move".into(),
            focus: "no".into(),
            ..Confirm::default()
        };
        ui::confirm(&mut lines, w, h, &mut q);
        assert_eq!(lines.len(), h as usize, "{w}×{h}: rows");
        for l in &lines {
            assert_eq!(ui::text_width(&plain(l)), w as usize, "{w}×{h}: {}", plain(l));
        }
        match &q.zones {
            None => assert!(w < 2, "{w}×{h}: a card"),
            Some(z) => {
                assert!(w >= 2, "{w}×{h}: no card");
                assert!(z.y < h, "{w}×{h}: buttons out of the pane");
            }
        }
    }
}

#[test]
fn segmented_switch_the_choices_and_their_columns() {
    let _l = lock();
    let (segs, _, hits) = ui::segmented(&[("file", "File"), ("diff", "Diff")], "diff");
    assert_eq!(text(&segs), " File  Diff ");
    let hits: Vec<(&str, usize, usize)> = hits.iter().map(|h| (h.id.as_str(), h.from, h.to)).collect();
    assert_eq!(hits, [("file", 0, 5), ("diff", 6, 11)]);
}

fn item(label: &str, key: &str, value: Option<Seg>, run: &'static str) -> Option<MenuItem<&'static str>> {
    Some(MenuItem { label: label.into(), key: key.into(), value, danger: false, keep: false, run })
}

fn ran(c: Option<Choice<&'static str>>) -> Option<&'static str> {
    match c {
        Some(Choice::Run { run, .. }) => Some(run),
        _ => None,
    }
}

#[test]
fn a_menu_drawn_on_the_right_keeps_what_is_left_of_it() {
    let _l = lock();
    let row = ui::line(&[ui::seg("abcdefghij", ui::fg(ui::c().red))], 20, None);
    let mut lines = vec![String::new(), row.clone(), row.clone(), row.clone(), row.clone(), row];
    let items = vec![item("A", "", Some(ui::on_off(true)), ""), None, item("B", "s", None, "b")];
    let mut menu = Menu::new("V", items, 1, 22, 1);
    menu.draw(&mut lines, 22, 24);
    assert_eq!(
        lines[1..].iter().map(|l| plain(l)).collect::<Vec<_>>(),
        [
            "abcdefghij╭ V ──────╮",
            "abcdefghij│ A  ● on │",
            "abcdefghij├─────────┤",
            "abcdefghij│ B  s    │",
            "abcdefghij╰─────────╯"
        ]
    );
    assert_eq!(ran(menu.key("s", 24)), Some("b"));
    assert_eq!(menu.key("escape", 24), Some(Choice::Close));
}

#[test]
fn tail_keeps_the_colours_before_its_column_and_spaces_a_cut_wide_character() {
    let _l = lock();
    let red = ui::fg(ui::c().red);
    let row = ui::line(&[ui::seg("abcdef", red.clone()), ui::seg("界ghij", "\x1b[1m")], 20, None);
    assert_eq!(plain(&ui::tail(&row, 7)), " ghij        ");
    assert!(ui::tail(&row, 7).starts_with(&format!("{red}\x1b[0m\x1b[1m")));
    assert_eq!(ui::text_width(&plain(&(ui::head(&row, 4) + &ui::tail(&row, 4)))), 20);
}

#[test]
fn a_menu_drawn_in_the_middle_keeps_what_is_right_of_it() {
    let _l = lock();
    let row = ui::line(&[s("abcdefghijklmnopqrstuvwxyz")], 26, None);
    let mut lines = vec![String::new(), row.clone(), row.clone(), row];
    Menu::new("M", vec![item("A", "", None, "")], 1, 0, 4).draw(&mut lines, 26, 24);
    assert_eq!(
        lines[1..].iter().map(|l| plain(l)).collect::<Vec<_>>(),
        ["abc╭ M ──╮klmnopqrstuvwxyz", "abc│ A   │klmnopqrstuvwxyz", "abc╰─────╯klmnopqrstuvwxyz"]
    );
}

#[test]
fn a_menu_item_with_a_key_and_no_value_keeps_its_whole_label() {
    let _l = lock();
    let mut lines = vec![String::new(); 3];
    Menu::new("M", vec![item("Rebase onto…", "R", None, "")], 0, 0, 1).draw(&mut lines, 40, 24);
    assert_eq!(plain(&lines[1]).trim_end(), "│ Rebase onto…  R  │");
}

#[test]
fn a_menu_from_a_left_column_keeps_what_is_left_of_it() {
    let _l = lock();
    let mut lines = vec![String::new(), "xyz".into(), "xyz".into(), "xyz".into()];
    let mut del = item("Delete…", "del", None, "d");
    del.as_mut().unwrap().danger = true;
    let mut menu = Menu::new("M", vec![del], 0, 0, 2);
    menu.draw(&mut lines, 30, 24);
    assert_eq!(
        lines[..3].iter().map(|l| plain(l).trim_end().to_string()).collect::<Vec<_>>(),
        [" ╭ M ────────────╮", "x│ Delete…  del  │", "x╰───────────────╯"]
    );
    let click = |x| Mouse { press: true, button: 0, x, y: 2, ..Mouse::default() };
    assert_eq!(ran(menu.mouse(&click(4), 24)), Some("d"));
    assert_eq!(menu.mouse(&click(20), 24), Some(Choice::Close));
}

#[test]
fn arrows_change_an_option_of_a_menu_never_run_an_action() {
    let _l = lock();
    let items = vec![item("Push", "P", None, "push"), item("Wrap", "", Some(ui::on_off(true)), "wrap")];
    let mut menu = Menu::new("M", items, 0, 0, 1);
    assert_eq!(menu.key("right", 24), None);
    assert_eq!(ran(menu.key("enter", 24)), Some("push"));
    menu.move_by(1, 24);
    assert_eq!(ran(menu.key("left", 24)), Some("wrap"));
}

#[test]
fn background_of_reads_the_terminal_background_from_its_osc_11_answer() {
    let _l = lock();
    assert_eq!(ui::background_of("\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\"), Some("dark"));
    assert_eq!(ui::background_of("\x1b]11;rgb:ffff/ffff/ffff\x07"), Some("light"));
    assert_eq!(ui::background_of("\x1b]11;rgb:fd/f6/e3\x07"), Some("light"));
    assert_eq!(ui::background_of("\x1b]10;rgb:ffff/ffff/ffff\x07"), None);
}

/// The JS test also checks that ui.C stays the same object and that onTheme tells the modules: the port has
/// neither, its styles are worked out from c() at each use.
#[test]
fn use_theme_swaps_the_colours() {
    let _l = lock();
    ui::use_theme("light");
    assert_eq!(ui::c().strong, [0, 0, 0]);
    assert_eq!(ui::theme_name(), "light");
    ui::use_theme("nope");
    assert_eq!(ui::c().strong, [255, 255, 255]);
    assert_eq!(ui::theme_name(), "dark");
}

#[test]
fn a_card_with_several_answers_wraps_its_buttons_to_fit_the_pane() {
    let _l = lock();
    for (w, h) in [(80, 30), (34, 24), (20, 12), (30, 5)] {
        let mut lines: Vec<String> = (0..h).map(|_| " ".repeat(w as usize)).collect();
        let button =
            |key: &str, label: &str, tone: &str| Button { key: key.into(), label: label.into(), tone: tone.into() };
        let mut q = Ask {
            title: "Update herdr-explorer".into(),
            name: "0.7.1 → 0.9.0".into(),
            list: vec!["a change".into(), "another change".into()],
            note: Some("and 2 more".into()),
            buttons: vec![
                button("y", "Update", "go"),
                button("r", "Notes", ""),
                button("s", "Skip version", ""),
                button("n", "Later", ""),
            ],
            focus: 3,
            ..Ask::default()
        };
        ui::ask(&mut lines, w, h, &mut q);
        for l in &lines {
            assert_eq!(ui::text_width(&plain(l)), w as usize, "{w}×{h}: {}", plain(l));
        }
        let zones = q.zones.clone().unwrap();
        let mut keys: Vec<&str> = zones.iter().map(|z| z.2.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["n", "r", "s", "y"], "{w}×{h}: every button clickable");
        for (x0, _, key, y) in &zones {
            let m = Mouse { press: true, button: 0, x: *x0, y: y + 1, ..Mouse::default() };
            assert_eq!(ui::ask_mouse(&m, &q).as_ref(), Some(key));
        }
    }
}

#[test]
fn the_scrollbar_takes_the_colour_of_the_heaviest_mark_of_the_rows_a_cell_stands_for() {
    let _l = lock();
    let mut bar = Scrollbar::new();
    let (red, green) = ([200, 0, 0], [0, 200, 0]);
    let mark = |row, color, weight| BarMark { row, color, weight };
    // 100 rows, 10 shown: 8 cells of track between the buttons, about 12 rows each.
    assert_eq!(bar.set(100, 10, 0, &[mark(50, green, 1.0), mark(51, red, 3.0), mark(99, green, 1.0)]), 1);
    let cells: Vec<String> = (0..10).map(|k| bar.cell(k)).collect();
    assert!(cells[5].contains(&ui::fg(red)) && plain(&cells[5]) == "│");
    assert!(cells[8].contains(&ui::fg(green)));
    assert!(!cells[2].contains(&ui::fg(red)) && !cells[2].contains(&ui::fg(green)));
    // On the thumb: its glyph, in the mark's colour.
    bar.set(100, 10, 45, &[mark(50, red, 3.0)]);
    assert_eq!(plain(&bar.cell(5)), "┃");
    assert!(bar.cell(5).contains(&ui::fg(red)));
}
