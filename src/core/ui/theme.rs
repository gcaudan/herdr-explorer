//! Glyphs and colours: the Nerd Font or Unicode glyph set, the dark and light palettes and git's status colours,
//! the theme setting and the terminal's background.

use super::Rgb;
use crate::core::exec;
use crate::core::js;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// The glyphs of the panes' controls in an icon set: `search` is the magnifier before every find and filter field;
/// `remote` marks a branch whose remote one is at the same commit; `set` the set's name.
pub struct Glyphs {
    pub closed: &'static str,
    pub open: &'static str,
    pub gear: &'static str,
    pub search: &'static str,
    pub branch: &'static str,
    pub remote: &'static str,
    pub set: &'static str,
}

const UNICODE_GLYPHS: [&str; 6] = ["▸", "▾", "⚙", "⌕", "⎇", "☁"];

const fn glyphs(g: [&'static str; 6], set: &'static str) -> Glyphs {
    Glyphs { closed: g[0], open: g[1], gear: g[2], search: g[3], branch: g[4], remote: g[5], set }
}

static GLYPHS: [Glyphs; 4] = [
    glyphs(["\u{eab6}", "\u{eab4}", "\u{f013}", "\u{f002}", "\u{e0a0}", "\u{f0c2}"], "nerd"),
    glyphs(UNICODE_GLYPHS, "unicode"),
    glyphs([">", "v", "=", "/", "*", "^"], "ascii"),
    glyphs(UNICODE_GLYPHS, "emoji"),
];

static GLYPH_SET: AtomicU8 = AtomicU8::new(0);

/// The icon set chosen in the settings: nerd, unicode, ascii or emoji (nerd for anything else).
pub fn use_glyphs(set: &str) {
    let i = GLYPHS.iter().position(|g| g.set == set).unwrap_or(0);
    GLYPH_SET.store(i as u8, Ordering::Relaxed);
}

/// The glyphs of the set in use (`G` in the JS).
pub fn g() -> &'static Glyphs {
    &GLYPHS[GLYPH_SET.load(Ordering::Relaxed) as usize]
}

macro_rules! palette {
    ($($field:ident $name:literal),* $(,)?) => {
        /// The colours of a theme, from VS Code Dark+ (git decorations) and Seti (icons) for the dark one, Light+ for
        /// the light one.
        pub struct Palette { $(pub $field: Rgb),* }
        impl Palette {
            /// A colour by its name in the JS (`C[name]`).
            pub fn get(&self, name: &str) -> Option<Rgb> {
                match name { $($name => Some(self.$field),)* _ => None }
            }
        }
    };
}

palette! {
    yellow "yellow", blue "blue", orange "orange", green "green", purple "purple", red "red", grey "grey",
    folder "folder", dim "dim", text "text", select "select", select_blur "selectBlur", header "header",
    added "added", removed "removed", find "find", added_word "addedWord", removed_word "removedWord", gap "gap",
    section "section", rail "rail", thumb "thumb", switch "switch", field "field", panel "panel", menu "menu",
    hit "hit", current "current", found "found", warn "warn", pulse "pulse", strong "strong", canvas "canvas",
    button_on "buttonOn", button_off "buttonOff", danger "danger", go "go", danger_text "dangerText",
    go_text "goText",
}

pub static DARK: Palette = Palette {
    yellow: [203, 203, 65],
    blue: [81, 154, 186],
    orange: [227, 121, 51],
    green: [141, 193, 73],
    purple: [160, 116, 196],
    red: [204, 62, 68],
    grey: [109, 128, 134],
    folder: [220, 182, 122],
    dim: [140, 140, 140],
    text: [212, 212, 212],
    select: [4, 57, 94],
    select_blur: [55, 55, 61],
    header: [37, 37, 38],
    added: [35, 60, 36],
    removed: [72, 30, 30],
    find: [120, 90, 20],
    added_word: [46, 105, 52],
    removed_word: [125, 42, 42],
    gap: [30, 30, 30],
    section: [30, 36, 44],
    rail: [60, 60, 60],
    thumb: [130, 130, 130],
    switch: [58, 58, 62],
    field: [60, 60, 60],
    panel: [49, 49, 49],
    menu: [30, 30, 30],
    hit: [24, 163, 255],
    current: [200, 140, 30],
    found: [120, 70, 20],
    warn: [64, 48, 16],
    pulse: [32, 58, 40],
    strong: [255, 255, 255],
    canvas: [30, 30, 30],
    button_on: [96, 96, 96],
    button_off: [45, 45, 45],
    danger: [138, 47, 42],
    go: [46, 110, 58],
    danger_text: [229, 120, 109],
    go_text: [111, 191, 142],
};

pub static LIGHT: Palette = Palette {
    yellow: [148, 116, 0],
    blue: [0, 106, 177],
    orange: [196, 90, 0],
    green: [56, 128, 30],
    purple: [128, 60, 180],
    red: [205, 49, 49],
    grey: [110, 120, 125],
    folder: [184, 134, 60],
    dim: [110, 110, 110],
    text: [40, 40, 40],
    select: [173, 214, 255],
    select_blur: [228, 230, 236],
    header: [243, 243, 243],
    added: [221, 243, 221],
    removed: [252, 225, 225],
    find: [250, 222, 140],
    added_word: [172, 226, 170],
    removed_word: [245, 180, 180],
    gap: [240, 240, 240],
    section: [226, 234, 244],
    rail: [215, 215, 215],
    thumb: [160, 160, 160],
    switch: [225, 225, 228],
    field: [232, 232, 232],
    panel: [236, 236, 236],
    menu: [248, 248, 248],
    hit: [0, 102, 204],
    current: [240, 160, 40],
    found: [250, 200, 120],
    warn: [253, 236, 200],
    pulse: [214, 240, 214],
    strong: [0, 0, 0],
    canvas: [255, 255, 255],
    button_on: [110, 110, 110],
    button_off: [225, 225, 225],
    danger: [196, 43, 28],
    go: [46, 125, 50],
    danger_text: [180, 35, 24],
    go_text: [30, 120, 50],
};

/// The colours of git's statuses (M A U R D C !), dark then light.
const GIT_DARK: [(char, Rgb); 7] = [
    ('M', [226, 192, 141]),
    ('A', [129, 184, 139]),
    ('U', [115, 201, 145]),
    ('R', [115, 201, 145]),
    ('D', [199, 78, 57]),
    ('C', [228, 103, 107]),
    ('!', [140, 140, 140]),
];

const GIT_LIGHT: [(char, Rgb); 7] = [
    ('M', [137, 85, 3]),
    ('A', [88, 124, 12]),
    ('U', [0, 113, 0]),
    ('R', [0, 113, 0]),
    ('D', [173, 7, 7]),
    ('C', [173, 7, 7]),
    ('!', [142, 142, 144]),
];

static LIGHT_THEME: AtomicBool = AtomicBool::new(false);

fn light() -> bool {
    LIGHT_THEME.load(Ordering::Relaxed)
}

/// The theme's colours (`C`): read at each render, they follow the theme.
pub fn c() -> &'static Palette {
    if light() { &LIGHT } else { &DARK }
}

/// The colour of a git status in the theme (`GIT[status]`).
pub fn git(status: char) -> Option<Rgb> {
    let table = if light() { &GIT_LIGHT } else { &GIT_DARK };
    table.iter().find(|(s, _)| *s == status).map(|(_, rgb)| *rgb)
}

/// The theme: 'light', else 'dark'.
pub fn use_theme(name: &str) {
    LIGHT_THEME.store(name == "light", Ordering::Relaxed);
}

pub fn theme_name() -> &'static str {
    if light() { "light" } else { "dark" }
}

/// The background file of the state folder, written when a pane reads the terminal's background.
pub(crate) fn background_file() -> Option<String> {
    exec::state_dir().map(|d| crate::core::paths::join(&[&d, "background"]))
}

/// The theme setting: 'dark', 'light', or 'auto'.
pub fn theme_setting() -> &'static str {
    match exec::read_settings().get("theme").and_then(|t| t.as_str()) {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "auto",
    }
}

/// The terminal's background as last seen, dark until then.
pub fn seen_background() -> &'static str {
    let text = background_file().and_then(|f| crate::core::exec::read_text(f).ok());
    if text.is_some_and(|t| t.trim_matches(js::is_space) == "light") { "light" } else { "dark" }
}

/// The theme a setting stands for: 'auto' is the terminal's background as last seen.
pub fn setting_theme(t: Option<&str>) -> &'static str {
    match t.unwrap_or_else(|| theme_setting()) {
        "dark" => "dark",
        "light" => "light",
        "auto" => seen_background(),
        // The JS returns any other value as it is, which useTheme takes as dark.
        _ => "dark",
    }
}

/// The theme of the settings put in use, as the JS does when the module loads.
pub fn init_theme() {
    use_theme(setting_theme(None));
}

/// The terminal's background from its answer to OSC 11 (rgb:rrrr/gggg/bbbb): 'light' or 'dark', None when
/// unreadable.
pub fn background_of(reply: &str) -> Option<&'static str> {
    let rest = reply.strip_prefix("\x1b]11;")?;
    let rest = rest.get(..4).filter(|p| p.eq_ignore_ascii_case("rgb:")).map(|_| &rest[4..])?;
    let mut parts = [0.0; 3];
    let mut s = rest;
    for (i, part) in parts.iter_mut().enumerate() {
        if i > 0 {
            s = s.strip_prefix('/')?;
        }
        let n = s.bytes().take_while(u8::is_ascii_hexdigit).count();
        if n == 0 {
            return None;
        }
        let v = s[..n].chars().fold(0.0, |v, d| v * 16.0 + d.to_digit(16).unwrap_or(0) as f64);
        *part = v / (16f64.powi(n as i32) - 1.0);
        s = &s[n..];
    }
    Some(if 0.2126 * parts[0] + 0.7152 * parts[1] + 0.0722 * parts[2] > 0.5 { "light" } else { "dark" })
}
