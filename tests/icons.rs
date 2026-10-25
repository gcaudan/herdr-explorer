//! test/icons.test.js: file icons in each icon set: every one and its space take the same columns, two (three for
//! emoji, two columns wide), so that the names line up.

use herdr_explorer::core::{icons, ui};
use std::sync::Mutex;

/// Held by the tests that change the icon set, which every test of this binary shares.
static SET: Mutex<()> = Mutex::new(());

const NAMES: &[&str] = &[
    "a.js",
    "b.ts",
    "c.json",
    "d.md",
    "e.py",
    "f.html",
    "g.css",
    "h.sh",
    "i.ps1",
    "j.png",
    "k.pdf",
    "l.zip",
    "m.csv",
    "n.docx",
    "o.lock",
    "p.toml",
    "q.sql",
    "r.rs",
    "s.cs",
    "t.java",
    "u.vue",
    "v.txt",
    "w.go",
    ".gitignore",
    "package.json",
    "Dockerfile",
    "README.md",
    "noext",
    "x.unknown",
];

#[test]
fn icons_take_the_same_width_within_a_set() {
    let _set = SET.lock().unwrap_or_else(|e| e.into_inner());
    for set in ["nerd", "emoji", "unicode", "ascii"] {
        ui::use_glyphs(set);
        let all = NAMES.iter().map(|&n| (n, false, false)).chain([("src", true, false), ("src", true, true)]);
        for (name, dir, open) in all {
            // The colour is always one (an array in the JS).
            let (text, _rgb) = icons::icon(None, name, dir, open);
            assert_eq!(ui::text_width(&text), if set == "emoji" { 3 } else { 2 }, "{set} {name}: {text:?}");
        }
        let g = ui::g();
        for (name, glyph) in
            [("closed", g.closed), ("open", g.open), ("gear", g.gear), ("search", g.search), ("branch", g.branch)]
        {
            assert_eq!(ui::text_width(glyph), 1, "{set} {name}");
        }
    }
    ui::use_glyphs("nerd");
}

#[test]
fn an_unknown_icon_set_falls_back_to_nerd_font() {
    let _set = SET.lock().unwrap_or_else(|e| e.into_inner());
    ui::use_glyphs("nope");
    assert_eq!(ui::g().set, "nerd");
    assert_eq!(ui::g().gear, "\u{f013}");
}
