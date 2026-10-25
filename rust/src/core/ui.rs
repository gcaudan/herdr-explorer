//! Terminal helpers shared by the plugin's panes: colours, text width, lines, input decoding, a one-line text
//! field, cards, menus and scrollbars. Offsets in a text are byte offsets, where the JS counted UTF-16 units.

use super::exec;
use super::js;
use crate::unicode;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// The Kitty graphics protocol query (a 1×1 image, not kept).
pub const KITTY_QUERY: &str = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";

/// Whether a reply starts as the answer of a terminal that has the Kitty graphics protocol.
pub fn kitty_ok(reply: &str) -> bool {
    reply.starts_with("\x1b_Gi=31;OK")
}

pub type Rgb = [u8; 3];

/// A piece of a line: its text and the style sequence written before it.
pub type Seg = (String, String);

pub fn seg(text: impl Into<String>, style: impl Into<String>) -> Seg {
    (text.into(), style.into())
}

pub fn fg(rgb: Rgb) -> String {
    format!("\x1b[38;2;{};{};{}m", rgb[0], rgb[1], rgb[2])
}

pub fn bg(rgb: Rgb) -> String {
    format!("\x1b[48;2;{};{};{}m", rgb[0], rgb[1], rgb[2])
}

/// `Math.round`: halves rounded up (x + 0.5 floored would round 0.49999999999999994 up).
fn round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// A shade of a colour close to the theme's background, for a pill's background.
pub fn shade(rgb: Rgb) -> Rgb {
    let canvas = c().canvas;
    let at = |i: usize| round(canvas[i] as f64 + (rgb[i] as f64 - canvas[i] as f64) * 0.25) as u8;
    [at(0), at(1), at(2)]
}

pub fn glyph(code: u32) -> String {
    char::from_u32(code).map(String::from).unwrap_or_default()
}

// --- glyphs

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

// --- colours

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
    exec::state_dir().map(|d| super::paths::join(&[&d, "background"]))
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
    let text = background_file().and_then(|f| super::exec::read_text(f).ok());
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

// --- text

/// The C0 and C1 control characters, never written to the terminal from file names or contents.
pub fn is_control(ch: char) -> bool {
    matches!(ch, '\x00'..='\x1f' | '\x7f'..='\u{9f}')
}

pub fn clean(text: &str) -> String {
    text.chars().map(|ch| if is_control(ch) { '?' } else { ch }).collect()
}

/// Tabs expanded to spaces, other control characters removed: for file contents. at: receives, for each byte of
/// text and its end, where it lands in the cleaned text.
pub fn clean_line(text: &str, tab: usize, mut at: Option<&mut Vec<usize>>) -> String {
    if at.is_none() && !text.chars().any(is_control) {
        return text.to_string();
    }
    let mut s = String::new();
    let mut count = 0;
    for ch in text.chars() {
        if let Some(at) = at.as_deref_mut() {
            at.extend(std::iter::repeat_n(s.len(), ch.len_utf8()));
        }
        if ch == '\t' {
            // A tab stop of 0: the JS repeats NaN times, nothing.
            let n = if tab == 0 { 0 } else { tab - count % tab };
            s.extend(std::iter::repeat_n(' ', n));
            count += n;
        } else if !is_control(ch) {
            s.push(ch);
            count += 1;
        }
    }
    if let Some(at) = at {
        at.push(s.len());
    }
    s
}

/// Wide characters: CJK, emoji presentation, and the trigrams ☰ (wide since Unicode 16, as herdr counts them); not
/// the regional indicators.
fn wide(ch: char) -> bool {
    if unicode::is_regional_indicator(ch) {
        return false;
    }
    matches!(ch,
        '\u{1100}'..='\u{115f}' | '\u{2e80}'..='\u{303e}' | '\u{3041}'..='\u{33ff}' | '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}' | '\u{a000}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7a3}' | '\u{f900}'..='\u{faff}'
        | '\u{fe30}'..='\u{fe4f}' | '\u{ff00}'..='\u{ff60}' | '\u{ffe0}'..='\u{ffe6}' | '\u{20000}'..='\u{3fffd}'
        | '\u{2630}'..='\u{2637}' | '\u{268a}'..='\u{268f}')
        || unicode::is_emoji_presentation(ch)
}

/// Terminal columns of a character: none for combining marks, two for wide ones. prev: the character before, for
/// the emoji selector U+FE0F, which makes a text symbol such as ⚠ an emoji two columns wide.
pub fn cols(ch: char, prev: Option<char>) -> usize {
    if ch == '\u{fe0f}' && prev.is_some_and(|p| unicode::is_emoji(p) && !wide(p)) {
        1
    } else if unicode::is_mark(ch) || matches!(ch, '\u{200b}' | '\u{200d}' | '\u{fe0f}') {
        0
    } else if wide(ch) {
        2
    } else {
        1
    }
}

/// The longest start of a text that fits in max columns, and its width. prev: the character before the text.
pub fn fit(text: &str, max: usize, mut prev: Option<char>) -> (&str, usize) {
    let mut w = 0;
    for (i, ch) in text.char_indices() {
        let c = cols(ch, prev);
        if w + c > max {
            return (&text[..i], w);
        }
        w += c;
        prev = Some(ch);
    }
    (text, w)
}

pub fn text_width(text: &str) -> usize {
    fit(text, usize::MAX, None).1
}

/// Cut to max columns with an ellipsis when too long.
pub fn ellipsis(text: &str, max: i64) -> String {
    if (text_width(text) as i64) <= max {
        return text.to_string();
    }
    format!("{}…", fit(text, (max - 1).max(0) as usize, None).0)
}

/// A screen line of exactly width columns: the segments' text truncated to the width, control characters replaced;
/// a style may set a background, reset after its segment. back: background colour of the whole line.
pub fn line(segments: &[Seg], width: usize, back: Option<Rgb>) -> String {
    let back = back.map(bg).unwrap_or_default();
    let mut s = back.clone();
    let mut used = 0;
    let mut prev = None;
    for (text, style) in segments {
        let text = clean(text);
        let (part, w) = fit(&text, width - used, prev);
        if part.is_empty() {
            continue;
        }
        prev = part.chars().last();
        s += style;
        s += part;
        s += "\x1b[0m";
        s += &back;
        used += w;
    }
    s.extend(std::iter::repeat_n(' ', width - used));
    s + "\x1b[0m"
}

// --- key hints

/// Key hints: the segments, and the columns of each pair from their start, to click them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hints {
    pub segs: Vec<Seg>,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub x0: usize,
    pub x1: usize,
    pub key: String,
}

/// Each key in bold, its action dimmed, as many pairs as fit in max columns. pairs: (key, action).
pub fn keys<K: AsRef<str>, A: AsRef<str>>(pairs: &[(K, A)], max: usize) -> Hints {
    let mut out = Hints::default();
    let mut used = 0;
    for (k, action) in pairs {
        let (k, action) = (k.as_ref(), action.as_ref());
        let lead = if out.segs.is_empty() { " " } else { "   " };
        let w = text_width(&format!("{lead}{k} {action}"));
        if used + w > max {
            break;
        }
        out.segs.push(seg(format!("{lead}{k}"), format!("\x1b[1m{}", fg(c().text))));
        out.segs.push(seg(format!(" {action}"), fg(c().dim)));
        if let Some(key) = key_of(k) {
            out.zones.push(Zone { x0: used + lead.len(), x1: used + w, key });
        }
        used += w;
    }
    out
}

/// The key a click on a hint sends: its first key, by its name for the key handlers; none for a range of keys (↑↓,
/// 1-4) or arrows.
fn key_of(label: &str) -> Option<String> {
    let k = label.split(' ').next().unwrap_or("");
    let b = k.as_bytes();
    let range = b.len() == 3 && b[0].is_ascii_digit() && b[1] == b'-' && b[2].is_ascii_digit();
    if k.contains(['↑', '↓', '←', '→']) || range {
        return None;
    }
    let sent = match k {
        "⏎" | "↵" => "enter",
        "esc" => "escape",
        "del" => "delete",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        _ => k,
    };
    // An empty key sends nothing.
    (!sent.is_empty()).then(|| sent.to_string())
}

/// The key of the hint at column x (0-based) of a line whose hints start at column from.
pub fn key_at(hints: &Hints, x: i64, from: i64) -> Option<String> {
    hints.zones.iter().find(|z| x >= from + z.x0 as i64 && x < from + z.x1 as i64).map(|z| z.key.clone())
}

/// A key name as shown to the user: arrows and Enter as symbols, the long names shortened.
pub fn key_label(k: &str) -> String {
    let name = k.rsplit('+').next().unwrap_or(k);
    let short = match name {
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "enter" => "⏎",
        "escape" => "esc",
        "pageup" => "pgup",
        "pagedown" => "pgdn",
        _ => return k.to_string(),
    };
    format!("{}{short}", &k[..k.len() - name.len()])
}

// --- messages, empty areas, wrapping

/// An empty area of h lines, width w: what it lacks, dimmed and centred a third of the way down, and under it the
/// keys that go on from there.
pub fn empty<K: AsRef<str>, A: AsRef<str>>(w: usize, h: usize, text: &str, pairs: &[(K, A)]) -> Vec<String> {
    let mut body: Vec<Vec<Seg>> = vec![vec![seg(ellipsis(text, w as i64 - 2), fg(c().dim))]];
    if !pairs.is_empty() {
        body.push(Vec::new());
        body.push(keys(pairs, usize::MAX).segs);
    }
    let n = body.len() as i64;
    let at = (h as i64 - n).min((h as i64 - n).div_euclid(3)).max(0);
    (0..h as i64)
        .map(|i| match usize::try_from(i - at).ok().and_then(|k| body.get(k)) {
            Some(segs) => {
                let pad = (w as i64 - seg_width(segs) as i64).div_euclid(2).max(0) as usize;
                let mut all = vec![seg(" ".repeat(pad), "")];
                all.extend(segs.iter().cloned());
                line(&all, w, None)
            }
            None => line(&[], w, None),
        })
        .collect()
}

/// What a message reads as: ✓ done, ✕ error, ⋯ going on, · nothing to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Ok,
    Error,
    Busy,
    Info,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Ok => "ok",
            Kind::Error => "error",
            Kind::Busy => "busy",
            Kind::Info => "info",
        }
    }
}

/// A word of the message, with \b on both sides.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(js::is_word) && !after.is_some_and(js::is_word)
    })
}

/// The kind of a message, guessed from its text.
pub fn message_kind(text: &str) -> Kind {
    const STARTS: [&str; 11] = [
        "Cannot",
        "Can't",
        "Could not",
        "Not ",
        "No editor",
        "No terminal",
        "Action failed",
        "Update failed",
        "Already exists",
        "VS Code not found",
        "Unknown ",
    ];
    let error = STARTS.iter().any(|s| text.starts_with(s))
        || has_word(text, "not found")
        || has_word(text, "unavailable")
        || text.ends_with(": already bound")
        || text.contains("; cannot ");
    if text.ends_with('…') {
        Kind::Busy
    } else if error {
        Kind::Error
    } else if text.starts_with("No ") || text.starts_with("Nothing") {
        Kind::Info
    } else {
        Kind::Ok
    }
}

/// How long a message shows, in milliseconds: an error twice as long.
pub fn message_time(kind: Kind) -> u64 {
    if kind == Kind::Error { 4000 } else { 2000 }
}

/// A message after an action, with its mark. kind: None to guess it from the text.
pub fn message(text: &str, kind: Option<Kind>) -> Vec<Seg> {
    match kind.unwrap_or_else(|| message_kind(text)) {
        Kind::Error => vec![seg(" ✕ ", fg(c().red)), seg(text, fg(c().red))],
        Kind::Busy => vec![seg(format!(" ⋯ {text}"), fg(c().dim))],
        Kind::Info => vec![seg(" · ", fg(c().dim)), seg(text, "")],
        Kind::Ok => vec![seg(" ✓ ", fg(c().green)), seg(text, "")],
    }
}

/// Splits a plain text into lines of at most width columns, at spaces when possible.
pub fn wrap(text: &str, width: i64) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        let mut w = 0;
        // The words, each with the white space that ends it.
        let mut words = Vec::new();
        let mut start = 0;
        for (i, ch) in para.char_indices() {
            if js::is_space(ch) {
                words.push(&para[start..i + ch.len_utf8()]);
                start = i + ch.len_utf8();
            }
        }
        words.push(&para[start..]);
        for word in words {
            let ww = text_width(word);
            if w + ww <= width {
                cur += word;
                w += ww;
                continue;
            }
            if !cur.is_empty() {
                lines.push(cur.trim_end_matches(js::is_space).to_string());
                cur.clear();
            }
            let mut rest = word;
            while text_width(rest) > width {
                let (mut part, _) = fit(rest, width, None);
                // A character wider than the width never fits: the JS loops forever there; it
                // goes alone on its line.
                if part.is_empty() {
                    part = &rest[..rest.chars().next().map_or(0, char::len_utf8)];
                }
                lines.push(part.to_string());
                rest = &rest[part.len()..];
            }
            cur = rest.to_string();
            w = text_width(rest);
        }
        lines.push(cur.trim_end_matches(js::is_space).to_string());
    }
    lines
}

// --- segments

pub fn seg_text(segs: &[Seg]) -> String {
    segs.iter().map(|(t, _)| t.as_str()).collect()
}

pub fn seg_width(segs: &[Seg]) -> usize {
    text_width(&seg_text(segs))
}

/// The columns [from, from + width) of a segment list, for horizontal scrolling. A wide character cut by the left
/// edge leaves a space, so that the text keeps its columns.
pub fn slice(segs: &[Seg], from: usize, width: usize) -> Vec<Seg> {
    let mut res = Vec::new();
    let (mut col, mut prev, mut pw) = (0, None, 0);
    for (text, style) in segs {
        let mut part = String::new();
        for ch in text.chars() {
            let c = cols(ch, prev);
            // An emoji selector whose character was cut by the left edge: a space in its place.
            let lone = c > 0 && ch == '\u{fe0f}' && col < from + pw;
            prev = Some(ch);
            pw = c;
            if col >= from && col + c <= from + width {
                part.push(if lone { ' ' } else { ch });
            } else if col < from && col + c > from {
                part.extend(std::iter::repeat_n(' ', (col + c - from).min(width)));
            }
            col += c;
            if col >= from + width {
                break;
            }
        }
        if !part.is_empty() {
            res.push((part, style.clone()));
        }
        if col >= from + width {
            break;
        }
    }
    res
}

/// A segment list cut into lines of at most width columns.
pub fn split(segs: &[Seg], width: usize) -> Vec<Vec<Seg>> {
    let mut lines: Vec<Vec<Seg>> = vec![Vec::new()];
    let (mut w, mut prev, mut pw): (usize, Option<char>, usize) = (0, None, 0);
    for (text, style) in segs {
        let mut part = String::new();
        for ch in text.chars() {
            let c = cols(ch, prev);
            // The emoji selector widens the character before it (pw columns, maybe in the previous segment): both go
            // to the next line.
            let base = if c > 0 && ch == '\u{fe0f}' { prev } else { None };
            let bw = if base.is_some() { pw } else { 0 };
            if w + c > width && w > bw {
                let mut moved = Vec::new();
                let mut keep = String::new();
                if let Some(b) = base {
                    if part.ends_with(b) {
                        part.truncate(part.len() - b.len_utf8());
                        keep.push(b);
                    } else {
                        let last_line = lines.last_mut().expect("a line");
                        if let Some(last) = last_line.last_mut()
                            && last.0.ends_with(b)
                        {
                            last.0.truncate(last.0.len() - b.len_utf8());
                            let style = last.1.clone();
                            if last.0.is_empty() {
                                last_line.pop();
                            }
                            moved.push((b.to_string(), style));
                        }
                    }
                }
                if !part.is_empty() {
                    lines.last_mut().expect("a line").push((std::mem::take(&mut part), style.clone()));
                }
                w = if !moved.is_empty() || !keep.is_empty() { bw } else { 0 };
                lines.push(moved);
                part = keep;
            }
            part.push(ch);
            w += c;
            prev = Some(ch);
            pw = c;
        }
        if !part.is_empty() {
            lines.last_mut().expect("a line").push((part, style.clone()));
        }
    }
    lines
}

/// The byte ranges [start, end) of the segments' text restyled with style added (search matches).
pub fn mark(segs: &[Seg], ranges: &[(usize, usize)], style: &str) -> Vec<Seg> {
    if ranges.is_empty() {
        return segs.to_vec();
    }
    let mut res = Vec::new();
    let mut pos = 0;
    for (text, base) in segs {
        let mut cut = 0;
        let end = pos + text.len();
        // A bound inside a character takes the whole character: its halves are never parted.
        let inside = |p: usize| p > pos && p < end;
        let ranges: Vec<(usize, usize)> = ranges
            .iter()
            .map(|&(a, b)| {
                let a = if inside(a) { text.floor_char_boundary(a - pos) + pos } else { a };
                (a, if inside(b) { text.ceil_char_boundary(b - pos) + pos } else { b })
            })
            .collect();
        let mut points: Vec<usize> = Vec::new();
        for &(a, b) in &ranges {
            for p in [a, b] {
                if inside(p) && !points.contains(&(p - pos)) {
                    points.push(p - pos);
                }
            }
        }
        points.sort_unstable();
        points.push(text.len());
        for p in points {
            if p <= cut {
                continue;
            }
            let at = pos + cut;
            let hit = ranges.iter().any(|&(a, b)| at >= a && at < b);
            res.push((text[cut..p].to_string(), if hit { format!("{base}{style}") } else { base.clone() }));
            cut = p;
        }
        pos = end;
    }
    res
}

// --- segmented switch, drawn lines

/// An option's value as the explorer's settings show it.
pub fn on_off(on: bool) -> Seg {
    if on { seg("● on", fg(c().green)) } else { seg("○ off", fg(c().dim)) }
}

pub fn switch_style(on: bool) -> String {
    if on { format!("{}{}\x1b[1m", bg(c().select), fg(c().text)) } else { format!("{}{}", bg(c().switch), fg(c().dim)) }
}

/// A choice of a segmented switch: its id and the columns it takes from 0, for clicks.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub from: usize,
    pub to: usize,
}

/// Two or more ways to see the same thing, the active one filled: (id, label) choices. Returns the segments, their
/// width and the columns of each choice.
pub fn segmented(choices: &[(&str, &str)], active: &str) -> (Vec<Seg>, usize, Vec<Hit>) {
    let (mut segs, mut hits, mut x) = (Vec::new(), Vec::new(), 0);
    for &(id, label) in choices {
        let text = format!(" {label} ");
        let w = text_width(&text);
        segs.push(seg(text, switch_style(id == active)));
        hits.push(Hit { id: id.to_string(), from: x, to: (x + w).wrapping_sub(1) });
        x += w;
    }
    (segs, x, hits)
}

/// The tokens of a drawn line: its CSI sequences and its characters.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let ch = rest.chars().next()?;
        let mut n = ch.len_utf8();
        if let Some(seq) = rest.strip_prefix("\x1b[") {
            let params = seq.bytes().take_while(|b| b.is_ascii_digit() || *b == b';' || *b == b'?').count();
            if seq.as_bytes().get(params).is_some_and(u8::is_ascii_alphabetic) {
                n = 2 + params + 1;
            }
        }
        let (tok, tail) = rest.split_at(n);
        rest = tail;
        Some(tok)
    })
}

/// The first n columns of a drawn line (line()'s output), its colours kept, padded with spaces when shorter.
pub fn head(text: &str, n: usize) -> String {
    let (mut out, mut w, mut prev) = (String::new(), 0, None);
    for tok in tokens(text) {
        if tok.starts_with('\x1b') {
            out += tok;
            continue;
        }
        let ch = tok.chars().next();
        let c = cols(ch.unwrap_or(' '), prev);
        if w + c > n {
            break;
        }
        out += tok;
        w += c;
        prev = ch;
    }
    format!("{out}\x1b[0m{}", " ".repeat(n.saturating_sub(w)))
}

/// A drawn line from column n (from 0), its colours kept: the styles met before n are kept in front; a wide
/// character cut by n leaves spaces.
pub fn tail(text: &str, n: usize) -> String {
    let (mut pre, mut out, mut w, mut prev) = (String::new(), String::new(), 0, None);
    for tok in tokens(text) {
        if tok.starts_with('\x1b') {
            if w >= n {
                out += tok
            } else {
                pre += tok
            }
            continue;
        }
        let ch = tok.chars().next();
        let c = cols(ch.unwrap_or(' '), prev);
        prev = ch;
        if w >= n {
            out += tok;
        } else if w + c > n {
            out += &" ".repeat(w + c - n);
        }
        w += c;
    }
    format!("{pre}{out}\x1b[0m")
}

// --- input

/// The terminal's escape sequences and their key names.
const SEQ: &[(&str, &str)] = &[
    ("\x1b[A", "up"),
    ("\x1b[B", "down"),
    ("\x1b[C", "right"),
    ("\x1b[D", "left"),
    ("\x1bOA", "up"),
    ("\x1bOB", "down"),
    ("\x1bOC", "right"),
    ("\x1bOD", "left"),
    ("\x1b[H", "home"),
    ("\x1b[F", "end"),
    ("\x1bOH", "home"),
    ("\x1bOF", "end"),
    ("\x1b[1~", "home"),
    ("\x1b[4~", "end"),
    ("\x1b[7~", "home"),
    ("\x1b[8~", "end"),
    ("\x1b[2~", "insert"),
    ("\x1b[3~", "delete"),
    ("\x1b[5~", "pageup"),
    ("\x1b[6~", "pagedown"),
    ("\x1b[Z", "shift+tab"),
    ("\x1b[29~", "menu"),
    ("\x1b[21;2~", "shift+f10"),
    ("\x1b[1;5A", "ctrl+up"),
    ("\x1b[1;5B", "ctrl+down"),
    ("\x1b[1;5C", "ctrl+right"),
    ("\x1b[1;5D", "ctrl+left"),
    ("\x1b[1;2A", "shift+up"),
    ("\x1b[1;2B", "shift+down"),
    ("\x1b[1;2C", "shift+right"),
    ("\x1b[1;2D", "shift+left"),
    ("\x1b[1;3A", "alt+up"),
    ("\x1b[1;3B", "alt+down"),
    ("\x1b[1;3C", "alt+right"),
    ("\x1b[1;3D", "alt+left"),
    ("\x1b[1;5H", "ctrl+home"),
    ("\x1b[1;5F", "ctrl+end"),
    ("\x1b[5;5~", "ctrl+pageup"),
    ("\x1b[6;5~", "ctrl+pagedown"),
    // With shift, for a selection in a text being edited; ctrl+shift+z where the terminal tells it from ctrl+z.
    ("\x1b[1;2H", "shift+home"),
    ("\x1b[1;2F", "shift+end"),
    ("\x1b[5;2~", "shift+pageup"),
    ("\x1b[6;2~", "shift+pagedown"),
    ("\x1b[1;6C", "ctrl+shift+right"),
    ("\x1b[1;6D", "ctrl+shift+left"),
    ("\x1b[1;6H", "ctrl+shift+home"),
    ("\x1b[1;6F", "ctrl+shift+end"),
    ("\x1b[3;5~", "ctrl+delete"),
    ("\x1b[122;6u", "ctrl+shift+z"),
    ("\x1b[90;6u", "ctrl+shift+z"),
    ("\x1b[27;6;90~", "ctrl+shift+z"),
    ("\x1b[27;6;122~", "ctrl+shift+z"),
    ("\x1bOP", "f1"),
    ("\x1bOQ", "f2"),
    ("\x1bOR", "f3"),
    ("\x1bOS", "f4"),
    ("\x1b[11~", "f1"),
    ("\x1b[12~", "f2"),
    ("\x1b[15~", "f5"),
    ("\r", "enter"),
    ("\n", "enter"),
    ("\t", "tab"),
    ("\x7f", "backspace"),
    ("\x08", "backspace"),
    ("\x1b", "escape"),
    ("\x1b\x7f", "alt+backspace"),
    ("\x1b\r", "alt+enter"),
    (" ", "space"),
];

/// A token's key name: printable characters are their own name; control characters are ctrl+a … ctrl+z; ESC
/// followed by a character is alt+<character>; None for an unknown sequence.
pub fn key_name(k: &str) -> Option<String> {
    if let Some((_, name)) = SEQ.iter().find(|(s, _)| *s == k) {
        return Some(name.to_string());
    }
    let mut chars = k.chars();
    let (first, second, third) = (chars.next(), chars.next(), chars.next());
    if let (Some(ch), None) = (first, second)
        && ch < ' '
    {
        return Some(format!("ctrl+{}", char::from(ch as u8 + 96)));
    }
    // ESC and one UTF-16 unit.
    if let (Some('\x1b'), Some(ch), None) = (first, second, third)
        && ch.len_utf16() == 1
    {
        return Some(format!("alt+{ch}"));
    }
    if k.starts_with('\x1b') {
        return None;
    }
    Some(k.to_string())
}

/// A mouse report: x and y from 1; wheel: -1 up, 1 down; drag: motion with a button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mouse {
    pub button: i64,
    pub mods: i64,
    pub x: i64,
    pub y: i64,
    pub press: bool,
    pub drag: bool,
    pub wheel: i64,
}

/// What the terminal sent, decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// 'up', 'enter', 'ctrl+p', 'alt+x', or the typed character itself ('a', 'É').
    Key(String),
    Mouse(Mouse),
    /// Focus reports (mode 1004).
    Focus(bool),
    /// Bracketed paste (mode 2004), control characters removed; raw: as pasted, line breaks as \n.
    Paste {
        text: String,
        raw: String,
    },
    /// The terminal's answer to a query: a Kitty graphics reply (\x1b_G…), an OSC answer or a size report.
    Reply(String),
}

/// `+digits`, then ToInt32, as the JS's `b & 64` reads a button number.
fn int32(digits: &str) -> i64 {
    let n = digits.bytes().fold(0.0, |n, d| n * 10.0 + (d - b'0') as f64);
    (n % 4294967296.0) as u32 as i32 as i64
}

/// `+digits`, saturated.
fn number(digits: &str) -> i64 {
    digits.parse().unwrap_or(i64::MAX)
}

/// One token of the input at its start, as the JS's TOKEN regex reads it, and the event it gives.
fn token(s: &str) -> (usize, Option<Event>) {
    let digits = |t: &str| t.bytes().take_while(u8::is_ascii_digit).count();
    if let Some(body) = s.strip_prefix("\x1b[200~")
        && let Some(end) = body.find("\x1b[201~")
    {
        let raw = body[..end].replace("\r\n", "\n").replace('\r', "\n");
        let text = raw.chars().filter(|&ch| !is_control(ch) || ch == '\n').collect();
        return (6 + end + 6, Some(Event::Paste { text, raw }));
    }
    if let Some(body) = s.strip_prefix("\x1b_") {
        let n = body.find('\x1b').unwrap_or(body.len());
        if body[n..].starts_with("\x1b\\") {
            return (2 + n + 2, Some(Event::Reply(s[..2 + n + 2].to_string())));
        }
    }
    if let Some(body) = s.strip_prefix("\x1b]") {
        let n = body.find(['\x07', '\x1b']).unwrap_or(body.len());
        let end = if body[n..].starts_with('\x07') { Some(1) } else { body[n..].starts_with("\x1b\\").then_some(2) };
        if let Some(e) = end {
            return (2 + n + e, Some(Event::Reply(s[..2 + n + e].to_string())));
        }
    }
    if let Some(body) = s.strip_prefix("\x1b[") {
        // \x1b[(\d+(?:;\d+)*)t: a size report.
        let mut i = digits(body);
        if i > 0 {
            while body[i..].starts_with(';') && digits(&body[i + 1..]) > 0 {
                i += 1 + digits(&body[i + 1..]);
            }
            if body[i..].starts_with('t') {
                return (2 + i + 1, Some(Event::Reply(s[..2 + i + 1].to_string())));
            }
        }
        // \x1b[<b;x;y(M|m): a mouse report.
        if let Some(m) = body.strip_prefix('<') {
            let mut fields = [""; 3];
            let mut at = 0;
            let mut ok = true;
            for (k, f) in fields.iter_mut().enumerate() {
                let n = digits(&m[at..]);
                let sep = if k < 2 { m[at + n..].starts_with(';') } else { m[at + n..].starts_with(['M', 'm']) };
                if n == 0 || !sep {
                    ok = false;
                    break;
                }
                *f = &m[at..at + n];
                at += n + 1;
            }
            if ok {
                let b = int32(fields[0]);
                let press = m.as_bytes()[at - 1] == b'M';
                let wheel = if b & 64 != 0 {
                    match b & 3 {
                        0 => -1,
                        1 => 1,
                        _ => 0,
                    }
                } else {
                    0
                };
                let mouse = Mouse {
                    button: b & 3,
                    mods: b & 28,
                    x: number(fields[1]),
                    y: number(fields[2]),
                    press,
                    drag: b & 32 != 0,
                    wheel,
                };
                return (3 + at, Some(Event::Mouse(mouse)));
            }
        }
        // \x1b[I and \x1b[O: focus.
        if body.starts_with(['I', 'O']) {
            return (3, Some(Event::Focus(body.starts_with('I'))));
        }
        // \x1b[[\d;]*[A-Za-z~]
        let n = body.bytes().take_while(|b| b.is_ascii_digit() || *b == b';').count();
        if body.as_bytes().get(n).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'~') {
            return key_token(s, 2 + n + 1);
        }
    }
    if let Some(body) = s.strip_prefix("\x1bO")
        && body.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
    {
        return key_token(s, 3);
    }
    if let Some(body) = s.strip_prefix('\x1b')
        && let Some(ch) = body.chars().next()
        && ch != '\x1b'
    {
        // ESC _ and ESC ] cut from the rest of their answer still read as one.
        if ch == '_' || ch == ']' {
            return (2, Some(Event::Reply(s[..2].to_string())));
        }
        return key_token(s, 1 + ch.len_utf8());
    }
    key_token(s, s.chars().next().map_or(s.len(), char::len_utf8))
}

fn key_token(s: &str, n: usize) -> (usize, Option<Event>) {
    (n, key_name(&s[..n]).map(Event::Key))
}

/// Decodes terminal input into events.
pub fn decode(data: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (n, event) = token(rest);
        out.extend(event);
        rest = &rest[n..];
    }
    out
}

/// Whether a key name types a character.
pub fn printable(name: &str) -> bool {
    let mut chars = name.chars();
    if chars.next().is_some() && chars.next().is_none() && name != " " { true } else { name == "space" }
}

/// A one-line text field: value and cursor (in characters), edited with the usual keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    pub value: String,
    pub cursor: usize,
}

impl Field {
    pub fn new(value: &str) -> Self {
        Field { value: value.to_string(), cursor: value.chars().count() }
    }

    pub fn set(&mut self, value: &str) {
        *self = Field::new(value);
    }

    /// Returns true when the key changed the field or its cursor.
    pub fn key(&mut self, name: &str) -> bool {
        let mut chars: Vec<char> = self.value.chars().collect();
        let at = self.cursor;
        if printable(name) {
            let ch = if name == "space" { ' ' } else { name.chars().next().unwrap_or(' ') };
            chars.insert(at.min(chars.len()), ch);
            self.cursor += 1;
        } else {
            match name {
                "backspace" => {
                    if at == 0 {
                        return true;
                    }
                    if at - 1 < chars.len() {
                        chars.remove(at - 1);
                    }
                    self.cursor -= 1;
                }
                "delete" | "ctrl+d" => {
                    if at < chars.len() {
                        chars.remove(at);
                    }
                }
                "left" | "ctrl+b" => self.cursor = at.saturating_sub(1),
                "right" | "ctrl+f" => self.cursor = chars.len().min(at + 1),
                "home" => self.cursor = 0,
                "end" => self.cursor = chars.len(),
                "ctrl+u" => {
                    chars.drain(..at.min(chars.len()));
                    self.cursor = 0;
                }
                "ctrl+k" => chars.truncate(at),
                "ctrl+w" | "alt+backspace" => {
                    let mut i = at.min(chars.len());
                    while i > 0 && chars[i - 1] == ' ' {
                        i -= 1;
                    }
                    while i > 0 && chars[i - 1] != ' ' {
                        i -= 1;
                    }
                    chars.drain(i..at.min(chars.len()));
                    self.cursor = i;
                }
                _ => return false,
            }
        }
        self.value = chars.into_iter().collect();
        true
    }

    /// A text typed in, its line breaks as spaces.
    pub fn insert(&mut self, text: &str) {
        for ch in text.chars() {
            let ch = if ch == '\n' { ' ' } else { ch };
            if ch == ' ' {
                self.key("space")
            } else {
                self.key(ch.encode_utf8(&mut [0; 4]))
            };
        }
    }

    /// Segments for line(): the visible part of the value in width columns, the cursor shown in reverse video.
    pub fn segments(&self, width: i64, style: &str) -> Vec<Seg> {
        let chars: Vec<char> = self.value.chars().collect();
        let part = |a: usize, b: usize| -> String { chars[a.min(chars.len())..b.min(chars.len())].iter().collect() };
        let mut start = 0;
        while start < self.cursor && (text_width(&part(start, self.cursor + 1)) as i64) > width - 1 {
            start += 1;
        }
        let under = chars.get(self.cursor).map_or(" ".to_string(), |c| c.to_string());
        vec![
            seg(part(start, self.cursor), style),
            seg(under, "\x1b[7m"),
            seg(part(self.cursor + 1, chars.len()), style),
        ]
    }
}

// --- key help

/// Lines set from row i, the list grown when shorter.
fn put(lines: &mut Vec<String>, i: usize, l: String) {
    if lines.len() <= i {
        lines.resize(i + 1, String::new());
    }
    lines[i] = l;
}

/// A group of the key help: its title and its (key, action) pairs.
pub type Group = (String, Vec<(String, String)>);

/// What an open key help keeps: its first row shown, its find field (None when closed), and from the last draw the
/// rows the list gets, how many there are, and the pane's size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpState {
    pub top: i64,
    pub find: Option<Field>,
    pub n: i64,
    pub count: i64,
    pub w: Option<usize>,
    pub h: Option<usize>,
}

const HELP_KEYS: [(&str, &str); 3] = [("↑↓", "scroll"), ("/", "find"), ("esc", "close")];

/// The rows of the help: a group's title alone, or a key and its action.
fn help_rows(groups: &[Group], state: &HelpState) -> Vec<Vec<String>> {
    let q = state.find.as_ref().map(|f| js::lower(&f.value)).unwrap_or_default();
    let mut out = Vec::new();
    for (g, pairs) in groups {
        let kept: Vec<Vec<String>> = pairs
            .iter()
            .map(|(k, a)| vec![k.split(' ').map(key_label).collect::<Vec<_>>().join(" "), a.clone()])
            .filter(|r| q.is_empty() || js::lower(&format!("{} {}", r[0], r[1])).contains(&q))
            .collect();
        if !kept.is_empty() {
            out.push(vec![g.clone()]);
            out.extend(kept);
        }
    }
    out
}

/// The key help of a pane (? or f1), over the whole pane: the title, a find field once / is pressed, the keys by
/// group, then a rule and the keys of the help.
pub fn help(lines: &mut [String], w: usize, title: &str, groups: &[Group], state: &mut HelpState) {
    let h = lines.len();
    let rows = help_rows(groups, state);
    let at = if state.find.is_some() { 2 } else { 1 };
    let n = (h as i64 - at - 2).max(1);
    state.top = state.top.min(rows.len() as i64 - n).max(0);
    state.n = n;
    state.count = rows.len() as i64;
    state.w = Some(w);
    state.h = Some(h);
    // The keys' column: wide enough for most, a longer list of keys cut.
    let widest = rows.iter().filter(|r| r.len() == 2).map(|r| text_width(&r[0]) + 2).fold(6, usize::max);
    let kw = widest.min(11).min(w / 2);
    let title = if title.is_empty() { String::new() } else { format!(" · {title}") };
    let mut out = vec![line(&[seg(" ‹ ", fg(c().dim)), seg("Keys", "\x1b[1m"), seg(title, fg(c().dim))], w, None)];
    if let Some(find) = &state.find {
        let count = if find.value.is_empty() {
            String::new()
        } else {
            format!(" {}", rows.iter().filter(|r| r.len() == 2).count())
        };
        let mut segs = vec![seg(format!(" {} ", g().search), fg(c().dim))];
        let field = find.segments((w as i64 - 4 - count.len() as i64).max(1), "");
        let pad = (w as i64 - 3 - seg_width(&field) as i64).max(0) as usize;
        segs.extend(field);
        segs.push(seg(format!("{count:>pad$}"), fg(c().dim)));
        out.push(line(&segs, w, Some(c().header)));
    }
    for r in rows.iter().skip(state.top as usize).take(n as usize) {
        if r.len() == 1 {
            out.push(line(&[seg(format!(" {}", r[0]), format!("\x1b[1m{}", fg(c().blue)))], w, None));
            continue;
        }
        let k = ellipsis(&r[0], kw as i64 - 2);
        let pad = " ".repeat((kw as i64 - text_width(&k) as i64).max(1) as usize);
        let segs = [seg(format!("  {k}{pad}"), format!("\x1b[1m{}", fg(c().text))), seg(r[1].clone(), fg(c().dim))];
        out.push(line(&segs, w, None));
    }
    if rows.is_empty() {
        out.extend(empty(w, n as usize, "No match", &[("esc", "clear")]));
    }
    while out.len() + 2 < h {
        out.push(line(&[], w, None));
    }
    let more =
        format!("{}{}", if state.top > 0 { "▴" } else { "" }, if state.top + n < rows.len() as i64 { "▾" } else { "" });
    let rule = "─".repeat((w as i64 - more.chars().count() as i64 - 1).max(0) as usize);
    out.push(line(&[seg(rule + &more, fg(c().dim))], w, None));
    out.push(line(&keys(&HELP_KEYS, w).segs, w, Some(c().header)));
    for (i, l) in out.into_iter().take(h).enumerate() {
        lines[i] = l;
    }
}

fn help_scroll(state: &mut HelpState, by: i64) {
    let n = if state.n == 0 { 1 } else { state.n };
    state.top = (state.top.saturating_add(by)).min(state.count - n).max(0);
}

/// Keys of an open help: / opens the find, where characters are typed (Escape closes it); the arrows, page keys,
/// home and end scroll. Returns false when the help is to close: Escape, q, ? or f1.
pub fn help_key(k: &str, state: &mut HelpState) -> bool {
    if let Some(find) = &mut state.find {
        if k == "escape" {
            state.find = None;
            return true;
        }
        if printable(k) || k == "space" || k == "backspace" || k == "delete" {
            if find.key(k) {
                state.top = 0;
            }
            return true;
        }
    } else if k == "/" {
        state.find = Some(Field::new(""));
        state.top = 0;
        return true;
    } else if matches!(k, "escape" | "q" | "?" | "f1") {
        return false;
    }
    let page = ((if state.n == 0 { 10 } else { state.n }) - 1).max(1);
    let half = (page + 1) / 2;
    let by = match k {
        "up" | "k" => -1,
        "down" | "j" => 1,
        "pageup" => -page,
        "pagedown" => page,
        "ctrl+u" => -half,
        "ctrl+d" => half,
        "home" | "g" => i64::MIN,
        "end" | "G" => i64::MAX,
        _ => 0,
    };
    if by != 0 {
        help_scroll(state, by);
    }
    true
}

/// Mouse on an open help: the wheel scrolls, a click on a key hint acts as its key. Returns false when the help is
/// to close: a click on the title, or on esc.
pub fn help_mouse(m: &Mouse, state: &mut HelpState) -> bool {
    if m.wheel != 0 {
        help_scroll(state, m.wheel * 3);
        return true;
    }
    if !m.press || m.drag || m.button != 0 {
        return true;
    }
    if m.y == 1 {
        return false;
    }
    let k = if state.h.is_some_and(|h| m.y == h as i64) {
        key_at(&keys(&HELP_KEYS, state.w.unwrap_or(usize::MAX)), m.x - 1, 0)
    } else {
        None
    };
    match k {
        Some(k) => help_key(&k, state),
        None => true,
    }
}

// --- cards

/// A tone of a card's button: danger (red) or go (green).
fn button(tone: &str) -> Option<Rgb> {
    match tone {
        "danger" => Some(c().danger),
        "go" => Some(c().go),
        _ => None,
    }
}

fn button_text(tone: &str) -> Option<Rgb> {
    match tone {
        "danger" => Some(c().danger_text),
        "go" => Some(c().go_text),
        _ => None,
    }
}

/// A question in a card over the pane, answered yes or no.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Confirm {
    pub title: String,
    pub name: String,
    pub where_: Option<String>,
    pub note: Option<String>,
    pub action: String,
    /// 'danger' (the default) or 'go'.
    pub tone: String,
    /// 'yes' or 'no'.
    pub focus: String,
    /// The buttons' columns for clicks, from the last draw.
    pub zones: Option<ConfirmZones>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmZones {
    pub y: i64,
    pub yes: (i64, i64),
    pub no: (i64, i64),
}

/// The rows of a card's frame drawn into lines, at column x and row y, rows past the pane's last left out.
#[allow(clippy::too_many_arguments)]
fn card(lines: &mut Vec<String>, w: i64, h: i64, x: i64, y: i64, inner: i64, text: &[Vec<Seg>]) {
    let edge = fg(c().dim);
    let left = " ".repeat(x as usize);
    let right = " ".repeat((w - x - inner - 2).max(0) as usize);
    let width = inner as usize;
    let mut row = |i: i64, l: String| {
        if i < h {
            put(lines, i as usize, l);
        }
    };
    row(y, format!("{left}{}{right}", line(&[seg(format!("╭{}╮", "─".repeat(width)), &edge)], width + 2, None)));
    for (k, segs) in text.iter().enumerate() {
        let mut all = vec![seg("│", &edge)];
        all.extend(segs.iter().cloned());
        let l = format!("{left}{}{}{right}", line(&all, width + 1, None), line(&[seg("│", &edge)], 1, None));
        row(y + 1 + k as i64, l);
    }
    let bottom = format!("{left}{}{right}", line(&[seg(format!("╰{}╯", "─".repeat(width)), &edge)], width + 2, None));
    row(y + text.len() as i64 + 1, bottom);
}

/// The card's place: its left column and top row (from 0).
fn card_at(w: i64, h: i64, inner: i64, rows: i64) -> (i64, i64) {
    let x = (w - inner - 2).div_euclid(2).max(0);
    let y = (h - rows).min((h - rows).div_euclid(3)).max(1);
    (x, y)
}

/// A yes or no question drawn over the pane: its title, the name concerned and where it is, a note, then the
/// buttons, the action in red (green for tone 'go'), the focused one lit.
pub fn confirm(lines: &mut Vec<String>, w: i64, h: i64, q: &mut Confirm) {
    q.zones = None;
    // No room for the card's edges: keys only.
    if w < 2 {
        return;
    }
    // A narrow pane: the card takes its width, less a column on each side.
    let inner = 44.min((w - 4).max(12.min(w - 2)).max(0));
    let room = (inner - 2).max(1);
    let mut names: Vec<Vec<Seg>> = wrap(&q.name, room).into_iter().map(|l| vec![seg(format!(" {l}"), "")]).collect();
    let mut where_: Vec<Vec<Seg>> = q
        .where_
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(c().dim))])
        .collect();
    let tone_note = if q.tone == "go" { c().dim } else { c().red };
    let mut note: Vec<Vec<Seg>> = q
        .note
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(tone_note))])
        .collect();
    // A low pane: the blank lines go first, then where, the name's next lines, the note, the name and the title, so
    // that the buttons stay in it, under its first line.
    let (mut gaps, mut title) = (2, true);
    let fit = (h - 3).max(1);
    let rows = |title: bool, gaps: i64, names: usize, where_: usize, note: usize| {
        title as i64 + 1 + gaps + (names + where_ + note) as i64
    };
    macro_rules! over {
        () => {
            rows(title, gaps, names.len(), where_.len(), note.len()) > fit
        };
    }
    while over!() && gaps > 0 {
        gaps -= 1;
    }
    if over!() {
        where_.clear();
    }
    while over!() && names.len() > 1 {
        names.pop();
    }
    if over!() {
        note.clear();
    }
    if over!() {
        names.clear();
    }
    if over!() {
        title = false;
    }
    let mut text: Vec<Vec<Seg>> = Vec::new();
    if title {
        text.push(vec![seg(format!(" {}", ellipsis(&q.title, room)), "\x1b[1m")]);
    }
    if gaps > 1 {
        text.push(Vec::new());
    }
    text.extend(names);
    text.extend(where_);
    text.extend(note);
    let yes = format!(" y {} ", q.action);
    let no = " n Cancel ";
    let (yw, nw) = (text_width(&yes) as i64, text_width(no) as i64);
    let gap = (inner - 3 - yw - nw).clamp(1, 3);
    let tone = if button(&q.tone).is_some() { q.tone.as_str() } else { "danger" };
    let lit = format!("\x1b[1m{}", fg([255, 255, 255]));
    let focused = q.focus == "yes";
    let yes_style = if focused {
        format!("{lit}{}", bg(button(tone).unwrap_or_default()))
    } else {
        format!("{}{}", fg(button_text(tone).unwrap_or_default()), bg(c().button_off))
    };
    let no_style =
        if focused { format!("{}{}", fg(c().dim), bg(c().button_off)) } else { format!("{lit}{}", bg(c().button_on)) };
    if gaps > 0 {
        text.push(Vec::new());
    }
    text.push(vec![seg("  ", ""), seg(&yes, yes_style), seg(" ".repeat(gap as usize), ""), seg(no, no_style)]);
    let rows_h = text.len() as i64 + 2;
    let (x, y) = card_at(w, h, inner, rows_h);
    card(lines, w, h, x, y, inner, &text);
    // Screen columns from 1: the card's edge, then 2 spaces before the first button; a button cut by the edge is
    // clickable up to it.
    let (bx, end) = (x + 4, x + inner + 1);
    if y + rows_h - 2 < h {
        q.zones = Some(ConfirmZones {
            y: y + rows_h - 2,
            yes: (bx, (bx + yw - 1).min(end)),
            no: (bx + yw + gap, (bx + yw + gap + nw - 1).min(end)),
        });
    }
}

/// A key for the question: ← → (and tab, h, l) move between the buttons ('move'), Enter presses the focused one, y
/// confirms ('yes'); any other key cancels ('no').
pub fn confirm_key(k: &str, q: &mut Confirm) -> &'static str {
    if matches!(k, "left" | "right" | "tab" | "shift+tab" | "h" | "l") {
        q.focus = if q.focus == "yes" { "no" } else { "yes" }.into();
        return "move";
    }
    if k == "y" || (k == "enter" && q.focus == "yes") { "yes" } else { "no" }
}

/// A click: on the action's button 'yes', elsewhere 'no'; None for what is not a click.
pub fn confirm_mouse(m: &Mouse, q: &Confirm) -> Option<&'static str> {
    if !m.press || m.drag || m.wheel != 0 {
        return None;
    }
    let on = q.zones.as_ref().is_some_and(|z| m.button == 0 && m.y - 1 == z.y && m.x >= z.yes.0 && m.x <= z.yes.1);
    Some(if on { "yes" } else { "no" })
}

/// A button of an ask card: its key, label and tone ('go', 'danger', else grey).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Button {
    pub key: String,
    pub label: String,
    pub tone: String,
}

/// A question in a card with more answers than confirm's two.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ask {
    pub title: String,
    pub name: String,
    pub where_: Option<String>,
    pub note: Option<String>,
    /// Lines shown as bullets under the name.
    pub list: Vec<String>,
    pub buttons: Vec<Button>,
    /// The index of the button lit.
    pub focus: usize,
    /// From the last draw: the buttons' columns, (from, to, key, row), and the card's box (x0, x1, y0, y1).
    pub zones: Option<Vec<(i64, i64, String, i64)>>,
    pub boxed: Option<(i64, i64, i64, i64)>,
}

/// An ask card drawn over the pane as confirm draws its card, its buttons on as many rows as its width needs.
pub fn ask(lines: &mut Vec<String>, w: i64, h: i64, q: &mut Ask) {
    q.zones = None;
    q.boxed = None;
    if w < 2 {
        return;
    }
    let inner = 48.min((w - 4).max(12.min(w - 2)).max(0));
    let room = (inner - 2).max(1);
    let mut names: Vec<Vec<Seg>> = if q.name.is_empty() {
        Vec::new()
    } else {
        wrap(&q.name, room).into_iter().take(3).map(|l| vec![seg(format!(" {l}"), "")]).collect()
    };
    let mut list: Vec<Vec<Seg>> =
        q.list.iter().map(|l| vec![seg(" • ", fg(c().dim)), seg(ellipsis(l, (room - 3).max(1)), "")]).collect();
    let mut where_: Vec<Vec<Seg>> = q
        .where_
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(c().dim))])
        .collect();
    let mut note: Vec<Vec<Seg>> = q
        .note
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| vec![seg(format!(" {}", ellipsis(s, room)), fg(c().dim))])
        .collect();
    // The buttons in rows that fit the card, 2 spaces apart (1 when that keeps them on one row).
    let labels: Vec<String> = q.buttons.iter().map(|b| format!(" {} {} ", key_label(&b.key), b.label)).collect();
    let widths: Vec<i64> = labels.iter().map(|l| text_width(l) as i64).collect();
    let total = |g: i64| widths.iter().sum::<i64>() + g * (labels.len() as i64 - 1);
    let gap = if total(2) <= inner - 2 || total(1) > inner - 2 { 2 } else { 1 };
    let mut bands: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0;
    for (i, &lw) in widths.iter().enumerate() {
        let band = bands.last_mut().expect("a band");
        if !band.is_empty() && used + gap + lw > inner - 2 {
            bands.push(vec![i]);
            used = lw;
        } else {
            used += if band.is_empty() { 0 } else { gap } + lw;
            band.push(i);
        }
    }
    // A low pane: the list goes first, then the same order as confirm's to leave things out, the buttons last.
    let (mut gaps, mut title) = (2, true);
    let fit = (h - 3).max(1);
    let nb = bands.len() as i64;
    macro_rules! over {
        () => {
            title as i64 + nb + gaps + (names.len() + list.len() + where_.len() + note.len()) as i64 > fit
        };
    }
    while over!() && !list.is_empty() {
        list.pop();
    }
    while over!() && gaps > 0 {
        gaps -= 1;
    }
    if over!() {
        where_.clear();
    }
    while over!() && names.len() > 1 {
        names.pop();
    }
    if over!() {
        note.clear();
    }
    if over!() {
        names.clear();
    }
    if over!() {
        title = false;
    }
    let mut text: Vec<Vec<Seg>> = Vec::new();
    if title {
        text.push(vec![seg(format!(" {}", ellipsis(&q.title, room)), "\x1b[1m")]);
    }
    if gaps > 1 {
        text.push(Vec::new());
    }
    text.extend(names);
    text.extend(list);
    text.extend(where_);
    text.extend(note);
    let lit = format!("\x1b[1m{}", fg([255, 255, 255]));
    if gaps > 0 {
        text.push(Vec::new());
    }
    for band in &bands {
        let mut row = vec![seg("  ", "")];
        for (k, &i) in band.iter().enumerate() {
            let tone = &q.buttons[i].tone;
            let style = if i == q.focus {
                format!("{lit}{}", bg(button(tone).unwrap_or(c().button_on)))
            } else {
                format!("{}{}", fg(button_text(tone).unwrap_or(c().dim)), bg(c().button_off))
            };
            row.push(seg(if k > 0 { " ".repeat(gap as usize) } else { String::new() }, ""));
            row.push(seg(&labels[i], style));
        }
        text.push(row);
    }
    let rows_h = text.len() as i64 + 2;
    let (x, y) = card_at(w, h, inner, rows_h);
    card(lines, w, h, x, y, inner, &text);
    // Screen columns from 1: the card's edge, then 2 spaces before the first button; a button cut by the edge is
    // clickable up to it, one past it not at all.
    let end = x + inner + 1;
    q.boxed = Some((x + 1, x + inner + 2, y + 1, y + rows_h));
    let mut zones = Vec::new();
    for (r, band) in bands.iter().enumerate() {
        let by = y + rows_h - 1 - nb + r as i64;
        let mut bx = x + 4;
        if by < h {
            for &i in band {
                if bx <= end {
                    zones.push((bx, (bx + widths[i] - 1).min(end), q.buttons[i].key.clone(), by));
                }
                bx += widths[i] + gap;
            }
        }
    }
    q.zones = Some(zones);
}

/// A key for the question: ← → (and tab) move between the buttons ('move'), Enter and space press the focused one, a
/// button's key presses it, escape and ctrl+c give 'escape'; other keys nothing (None), the card staying.
pub fn ask_key(k: &str, q: &mut Ask) -> Option<String> {
    let n = q.buttons.len();
    if matches!(k, "left" | "right" | "tab" | "shift+tab") {
        // No buttons: the JS's focus becomes NaN.
        if n > 0 {
            q.focus = (q.focus + if k == "left" || k == "shift+tab" { n - 1 } else { 1 }) % n;
        }
        return Some("move".into());
    }
    if k == "enter" || k == "space" {
        return q.buttons.get(q.focus).map(|b| b.key.clone());
    }
    if k == "escape" || k == "ctrl+c" {
        return Some("escape".into());
    }
    q.buttons.iter().find(|b| b.key == k).map(|b| b.key.clone())
}

/// A click: on a button, its key; out of the card, 'escape'; None for the rest.
pub fn ask_mouse(m: &Mouse, q: &Ask) -> Option<String> {
    if !m.press || m.drag || m.wheel != 0 || m.button != 0 {
        return None;
    }
    let hit = q.zones.iter().flatten().find(|(a, b, _, y)| m.y - 1 == *y && m.x >= *a && m.x <= *b);
    if let Some((_, _, key, _)) = hit {
        return Some(key.clone());
    }
    match q.boxed {
        Some((x0, x1, y0, y1)) if m.x >= x0 && m.x <= x1 && m.y >= y0 && m.y <= y1 => None,
        _ => Some("escape".into()),
    }
}

// --- scrollbar

/// A row to show on a scrollbar: a cell of the rail or thumb takes the colour of the heaviest mark of the rows it
/// stands for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarMark {
    pub row: i64,
    pub color: Rgb,
    pub weight: f64,
}

/// The bar as last set: rows shown, the largest top, the top, whether it has ▴ ▾ buttons (1 or 0), the rows of its
/// track, the thumb's size and row, the mark of each track row.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub height: i64,
    pub max: i64,
    pub top: i64,
    pub arrows: i64,
    pub track: i64,
    pub size: i64,
    pub at: i64,
    pub tints: Vec<Option<BarMark>>,
}

/// What the mouse holds: a button (step -1 or 1), repeating from due, or the thumb, grabbed on row k at top.
#[derive(Debug, Clone, PartialEq)]
pub enum Held {
    Step { k: i64, step: i64, last: Option<i64>, due: Option<Instant> },
    Thumb { k: i64, top: i64 },
}

/// What a mouse event did to the bar: not its own, taken, or taken with the list to scroll to a top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHit {
    Ignored,
    Took,
    Scroll(i64),
}

/// A held button repeats 400 ms after the press, then every 50 ms.
const REPEAT_AFTER: Duration = Duration::from_millis(450);
const REPEAT_EVERY: Duration = Duration::from_millis(50);

/// The scrollbar of a list shown height rows at a time, in a column on its right, hidden while the list fits: a thin
/// rail, its thumb, and ▴ ▾ buttons at its ends when there is room. One per list, set again at each render; it keeps a
/// drag or a held button going from one mouse event to the next. The list scrolls by what mouse and tick return; the
/// pane lets go (stop) when it loses the focus.
#[derive(Debug, Clone, Default)]
pub struct Scrollbar {
    pub bar: Option<Bar>,
    pub held: Option<Held>,
}

impl Scrollbar {
    pub fn new() -> Self {
        Self::default()
    }

    /// The list's length, the rows shown and the first shown; returns the columns the bar takes: 1, or 0 when hidden.
    pub fn set(&mut self, total: i64, height: i64, top: i64, marks: &[BarMark]) -> usize {
        self.bar = None;
        if height < 2 || total <= height {
            return 0;
        }
        let max = total - height;
        let arrows = if height >= 4 { 1 } else { 0 };
        let track = height - 2 * arrows;
        let size = 1.max((track - 1).min(round((track * height) as f64 / total as f64) as i64));
        let top = top.min(max).max(0);
        let mut tints: Vec<Option<BarMark>> = vec![None; track as usize];
        for m in marks {
            let t = (track - 1).min((m.row as f64 * track as f64 / total as f64).floor() as i64);
            // A mark before the list: tints[-1] is a property the JS's array never shows.
            let Some(cell) = usize::try_from(t).ok().and_then(|t| tints.get_mut(t)) else { continue };
            if cell.is_none_or(|c| c.weight < m.weight) {
                *cell = Some(*m);
            }
        }
        let at = arrows + round(((track - size) * top) as f64 / max as f64) as i64;
        self.bar = Some(Bar { height, max, top, arrows, track, size, at, tints });
        1
    }

    /// The bar's cell on row k of the list (from 0), to add after the row's line; nothing when hidden.
    pub fn cell(&self, k: i64) -> String {
        let Some(b) = &self.bar else { return String::new() };
        if b.arrows == 1 && k == 0 {
            return format!("{}▴\x1b[0m", fg(c().dim));
        }
        if b.arrows == 1 && k == b.height - 1 {
            return format!("{}▾\x1b[0m", fg(c().dim));
        }
        let thumb = k >= b.at && k < b.at + b.size;
        let tint = usize::try_from(k - b.arrows).ok().and_then(|i| b.tints.get(i)).copied().flatten();
        match (tint, thumb) {
            (Some(t), _) => format!("{}{}\x1b[0m", fg(t.color), if thumb { '┃' } else { '│' }),
            (None, true) => format!("{}┃\x1b[0m", fg(c().thumb)),
            (None, false) => format!("{}│\x1b[0m", fg(c().rail)),
        }
    }

    /// The top that puts the thumb at row r, kept within the list.
    pub fn top_at(&self, r: i64) -> i64 {
        let Some(b) = &self.bar else { return 0 };
        0.max(b.max.min(round(((r - b.arrows) * b.max) as f64 / 1.max(b.track - b.size) as f64) as i64))
    }

    /// The next top of a held button, or None when the repeat stops: at an end of the list, or when the list did not
    /// take the last move (gone from the screen).
    fn step(&mut self) -> Option<i64> {
        let Some(Held::Step { step, last, .. }) = &mut self.held else { return None };
        let b = self.bar.as_ref()?;
        let top = 0.max(b.max.min(b.top + *step));
        if Some(top) == *last || top == b.top {
            return None;
        }
        *last = Some(top);
        Some(top)
    }

    /// A mouse event on row k of the list (from 0); on: whether it is on the bar's column; now, its time. A press on
    /// the thumb grabs it, the drag going on wherever the mouse goes until the release; elsewhere on the rail, the
    /// thumb first jumps under the mouse. A button scrolls one row, then again while held on it (tick).
    pub fn mouse(&mut self, m: &Mouse, k: i64, on: bool, now: Instant) -> BarHit {
        if m.wheel != 0 {
            return BarHit::Ignored;
        }
        if self.held.is_some() && !m.press {
            self.stop();
            return BarHit::Took;
        }
        if m.drag
            && let Some(h) = &mut self.held
        {
            match h {
                Held::Step { k: hk, due, .. } => {
                    if !on || k != *hk {
                        *due = None;
                    }
                }
                Held::Thumb { k: hk, top } => {
                    if let Some(b) = &self.bar {
                        let moved = round(((k - *hk) * b.max) as f64 / 1.max(b.track - b.size) as f64) as i64;
                        return BarHit::Scroll(0.max(b.max.min(*top + moved)));
                    }
                }
            }
            return BarHit::Took;
        }
        self.stop();
        let Some(b) = &self.bar else { return BarHit::Ignored };
        if !on || !m.press || m.drag || m.button != 0 || k < 0 || k >= b.height {
            return BarHit::Ignored;
        }
        let step = if b.arrows == 1 && k == 0 {
            -1
        } else if b.arrows == 1 && k == b.height - 1 {
            1
        } else {
            0
        };
        if step != 0 {
            self.held = Some(Held::Step { k, step, last: None, due: Some(now + REPEAT_AFTER) });
            return self.step().map_or(BarHit::Took, BarHit::Scroll);
        }
        let top = if k >= b.at && k < b.at + b.size { b.top } else { self.top_at(k - b.size / 2) };
        let was = b.top;
        self.held = Some(Held::Thumb { k, top });
        if top != was { BarHit::Scroll(top) } else { BarHit::Took }
    }

    /// When a held button repeats next, for the pane's wait.
    pub fn due(&self) -> Option<Instant> {
        match &self.held {
            Some(Held::Step { due, .. }) => *due,
            _ => None,
        }
    }

    /// A held button's repeat, once due: the top to scroll to, or None (not due, or stopped).
    pub fn tick(&mut self, now: Instant) -> Option<i64> {
        if self.due().is_none_or(|d| d > now) {
            return None;
        }
        let top = self.step();
        if let Some(Held::Step { due, .. }) = &mut self.held {
            *due = top.map(|_| now + REPEAT_EVERY);
        }
        top
    }

    /// Lets go of what the mouse held.
    pub fn stop(&mut self) {
        self.held = None;
    }
}

// --- menu

/// An item of a menu: its label; key, the key that does the same outside the menu, dimmed after the label ("" for
/// none); value, (text, style) on the right, as in the explorer's settings (on_off gives ● on / ○ off); danger, in red;
/// keep, the menu stays open after it runs (an option changed in place); run, what the caller runs.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem<T> {
    pub label: String,
    pub key: String,
    pub value: Option<Seg>,
    pub danger: bool,
    pub keep: bool,
    pub run: T,
}

/// What a key or a click chose: close the menu, or run an item (keep: show the menu again after, at the same row).
#[derive(Debug, Clone, PartialEq)]
pub enum Choice<T> {
    Close,
    Run { run: T, keep: bool },
}

/// A menu in a frame over a pane: items, None for a rule. at: the screen line (from 0) of its top border; right: its
/// right edge's column (from 1), else (0) it starts at the column left (from 1). The caller closes the menu on a
/// choice and runs the item, then shows the menu again at the same row when it keeps it.
#[derive(Debug, Clone)]
pub struct Menu<T> {
    pub title: String,
    pub items: Vec<Option<MenuItem<T>>>,
    pub at: i64,
    pub right: i64,
    pub left: i64,
    pub sel: usize,
    pub off: usize,
    pub x: i64,
    pub inner: i64,
}

impl<T: Clone> Menu<T> {
    pub fn new(title: &str, items: Vec<Option<MenuItem<T>>>, at: i64, right: i64, left: i64) -> Self {
        let sel = items.iter().position(Option::is_some).unwrap_or(0);
        Menu { title: title.into(), items, at, right, left, sel, off: 0, x: left, inner: 0 }
    }

    fn choice(it: &MenuItem<T>) -> Choice<T> {
        Choice::Run { run: it.run.clone(), keep: it.keep }
    }

    pub fn rows(&self, h: i64) -> usize {
        1.max((self.items.len() as i64).min(h - 2 - self.at)) as usize
    }

    /// Draws the menu over the lines of a screen w × h, what they showed kept on both sides of its frame.
    pub fn draw(&mut self, lines: &mut Vec<String>, w: i64, h: i64) {
        let n = self.rows(h);
        let edge = fg(c().dim);
        // An item: a space, its label, its key after two spaces, at least one space (two before a value), its value,
        // a space.
        let side = |it: &MenuItem<T>| {
            (if it.key.is_empty() { 0 } else { text_width(&it.key) + 2 })
                + it.value.as_ref().map_or(0, |v| text_width(&v.0) + 1)
        };
        let want = self
            .items
            .iter()
            .map(|it| it.as_ref().map_or(0, |it| text_width(&it.label) + side(it) + 3))
            .fold(text_width(&self.title) + 4, usize::max) as i64;
        let inner = (w - 2 - if self.right != 0 { 1 } else { self.left }).min(want);
        self.inner = inner;
        // From its left column x (from 1): the frame takes inner + 2 columns after a space.
        let x = if self.right != 0 { 1.max(self.right - inner - 2) } else { self.left };
        self.x = x;
        let cols = |v: i64| v.max(0) as usize;
        // On both sides of the frame, what the line showed.
        let keep = |lines: &mut Vec<String>, i: usize| {
            if lines.len() <= i {
                lines.resize(i + 1, String::new());
            }
            // `lines[i] || line([], W)`: an empty line too.
            let blank;
            let l = if lines[i].is_empty() {
                blank = line(&[], cols(w), None);
                &blank
            } else {
                &lines[i]
            };
            let pad = if x > 1 { head(l, cols(x - 1)) } else { String::new() };
            (pad, tail(l, cols(x + inner + 1)))
        };
        let border = |pad: &str, rest: &str, l: char, r: char, t: &str, more: Option<char>| {
            let rule = "─".repeat(cols(inner - text_width(t) as i64 - if more.is_some() { 2 } else { 0 }));
            let more = more.map_or(String::new(), |m| format!("{m}─"));
            format!(
                "{pad}{}{rest}",
                line(&[seg(format!("{l}{t}{rule}{more}{r}"), edge.clone())], cols(inner + 2), None)
            )
        };
        let at = self.at.max(0) as usize;
        let (pad, rest) = keep(lines, at);
        let title = format!(" {} ", ellipsis(&self.title, 1.max(inner - 4)));
        lines[at] = border(&pad, &rest, '╭', '╮', &title, (self.off > 0).then_some('▴'));
        let shown: Vec<(usize, Option<MenuItem<T>>)> =
            self.items.iter().cloned().enumerate().skip(self.off).take(n).collect();
        for (k, (i, it)) in shown.into_iter().enumerate() {
            let row = at + 1 + k;
            let (pad, rest) = keep(lines, row);
            let Some(it) = it else {
                lines[row] = format!(
                    "{pad}{}{rest}",
                    line(&[seg(format!("├{}┤", "─".repeat(cols(inner))), edge.clone())], cols(inner + 2), None)
                );
                continue;
            };
            let key = if it.key.is_empty() { String::new() } else { format!("  {}", it.key) };
            let (value, style) = it.value.clone().unwrap_or_default();
            let room =
                inner - 3 - text_width(&key) as i64 - if value.is_empty() { 0 } else { text_width(&value) as i64 + 1 };
            let label = ellipsis(&it.label, 1.max(room));
            let gap =
                " ".repeat(
                    1.max(inner - 2 - (text_width(&label) + text_width(&key) + text_width(&value)) as i64) as usize
                );
            let segs = [
                seg(format!(" {label}"), if it.danger { fg(c().red) } else { String::new() }),
                seg(key, fg(c().dim)),
                seg(gap, ""),
                seg(format!("{value} "), style),
            ];
            let back = if i == self.sel { c().select } else { c().select_blur };
            lines[row] = format!("{pad}{edge}│\x1b[39m{}{edge}│\x1b[0m{rest}", line(&segs, cols(inner), Some(back)));
        }
        let last = at + n + 1;
        let (pad, rest) = keep(lines, last);
        lines[last] = border(&pad, &rest, '╰', '╯', "", (self.off + n < self.items.len()).then_some('▾'));
    }

    /// Moves the selection by dir, over the rules, round the ends.
    pub fn move_by(&mut self, dir: i64, h: i64) {
        if !self.items.iter().any(Option::is_some) {
            return;
        }
        let len = self.items.len() as i64;
        loop {
            self.sel = (self.sel as i64 + dir).rem_euclid(len) as usize;
            if self.items[self.sel].is_some() {
                break;
            }
        }
        let n = self.rows(h);
        if self.sel < self.off {
            self.off = self.sel;
        }
        if self.sel >= self.off + n {
            self.off = self.sel - n + 1;
        }
    }

    /// A key: up and down (and k j) move, enter and space run the selection, left and right (h l) an option's, the
    /// key shown beside an item runs it; escape, q and , (which opens the menus of the panes) close. None when the
    /// menu took the key, or nothing came of it.
    pub fn key(&mut self, k: &str, h: i64) -> Option<Choice<T>> {
        if matches!(k, "escape" | "q" | ",") {
            return Some(Choice::Close);
        }
        // ← → change the selected option, as in the explorer's settings; on an action (no value), they do nothing,
        // so that an arrow never pushes or checks out.
        if matches!(k, "left" | "right" | "h" | "l") {
            return self.items.get(self.sel).and_then(Option::as_ref).filter(|it| it.value.is_some()).map(Self::choice);
        }
        if k == "up" || k == "k" {
            self.move_by(-1, h);
            return None;
        }
        if k == "down" || k == "j" {
            self.move_by(1, h);
            return None;
        }
        if k == "enter" || k == "space" {
            return self.items.get(self.sel).and_then(Option::as_ref).map(Self::choice);
        }
        self.items.iter().flatten().find(|it| !it.key.is_empty() && it.key == k).map(Self::choice)
    }

    /// A click: on an item, it runs; elsewhere, the menu closes. The wheel moves the selection.
    pub fn mouse(&mut self, m: &Mouse, h: i64) -> Option<Choice<T>> {
        if m.wheel != 0 {
            self.move_by(m.wheel, h);
            return None;
        }
        if !m.press || m.drag {
            return None;
        }
        let k = m.y - 2 - self.at;
        if m.button == 0 && k >= 0 && k < self.rows(h) as i64 && m.x > self.x && m.x <= self.x + self.inner + 1 {
            return self.items.get(self.off + k as usize).and_then(Option::as_ref).map(Self::choice);
        }
        Some(Choice::Close)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(press: bool, drag: bool) -> Mouse {
        Mouse { press, drag, ..Mouse::default() }
    }

    /// A held ▾ scrolls at once, again 450 ms later (the JS's 400 ms timeout, then its first 50 ms interval), then
    /// every 50 ms, until the release; the list not taking a move stops it.
    #[test]
    fn a_held_button_repeats_until_released() {
        let mut bar = Scrollbar::new();
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        bar.set(100, 10, 0, &[]);
        assert_eq!(bar.mouse(&press(true, false), 9, true, t), BarHit::Scroll(1));
        bar.set(100, 10, 1, &[]);
        assert_eq!(bar.due(), Some(ms(450)));
        assert_eq!(bar.tick(ms(449)), None);
        assert_eq!(bar.tick(ms(450)), Some(2));
        bar.set(100, 10, 2, &[]);
        assert_eq!(bar.tick(ms(499)), None);
        assert_eq!(bar.tick(ms(500)), Some(3));
        // Not scrolled (the list gone from the screen): the repeat stops.
        assert_eq!(bar.tick(ms(550)), None);
        assert_eq!(bar.due(), None);
        assert_eq!(bar.mouse(&press(false, false), 9, true, ms(600)), BarHit::Took);
        assert_eq!(bar.held, None);
    }

    #[test]
    fn a_button_left_while_held_stops_repeating() {
        let mut bar = Scrollbar::new();
        let t = Instant::now();
        bar.set(100, 10, 50, &[]);
        assert_eq!(bar.mouse(&press(true, false), 0, true, t), BarHit::Scroll(49));
        bar.set(100, 10, 49, &[]);
        assert_eq!(bar.mouse(&press(true, true), 1, true, t), BarHit::Took);
        assert_eq!(bar.due(), None);
        assert_eq!(bar.tick(t + Duration::from_secs(1)), None);
        // Back on the button: still stopped, as the JS's cleared timer.
        assert_eq!(bar.mouse(&press(true, true), 0, true, t), BarHit::Took);
        assert_eq!(bar.due(), None);
    }

    #[test]
    fn the_repeat_stops_at_an_end_of_the_list() {
        let mut bar = Scrollbar::new();
        let t = Instant::now();
        bar.set(11, 10, 1, &[]);
        assert_eq!(bar.mouse(&press(true, false), 9, true, t), BarHit::Took);
        assert_eq!(bar.tick(t + Duration::from_millis(450)), None);
        assert_eq!(bar.due(), None);
    }

    #[test]
    fn math_round_rounds_halves_up() {
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-0.5), 0.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(2.4), 2.0);
    }

    /// A range ending inside a character (a column given for one) marks the whole character, without a panic.
    #[test]
    fn a_mark_never_cuts_a_character() {
        let segs = vec![("éa".to_string(), String::new())];
        let (marked, plain) = (|t: &str| (t.to_string(), "x".to_string()), |t: &str| (t.to_string(), String::new()));
        assert_eq!(mark(&segs, &[(0, 1)], "x"), [marked("é"), plain("a")]);
        assert_eq!(mark(&segs, &[(1, 3)], "x"), [marked("éa")]);
    }
}
