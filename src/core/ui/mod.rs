//! Terminal helpers shared by the plugin's panes: colours, text width, lines, input decoding, a one-line text
//! field, cards, menus and scrollbars. Offsets in a text are byte offsets.

mod card;
mod hints;
mod input;
mod menu;
mod message;
mod scrollbar;
mod text;
mod theme;

pub use card::{Ask, Button, Confirm, ConfirmZones, ask, ask_key, ask_mouse, confirm, confirm_key, confirm_mouse};
pub use hints::{Group, HelpState, Hints, Zone, help, help_key, help_mouse, key_at, key_label, keys};
pub use input::{Event, Field, Mouse, decode, key_name, printable};
pub use menu::{Choice, Menu, MenuItem};
pub use message::{Kind, empty, message, message_kind, message_time, wrap};
pub use scrollbar::{Bar, BarHit, BarMark, Held, Scrollbar};
pub use text::{
    Hit, clean, clean_line, cols, ellipsis, fit, head, is_control, line, mark, on_off, seg_text, seg_width, segmented,
    slice, split, switch_style, tail, text_width,
};
pub(crate) use theme::background_file;
pub use theme::{
    DARK, Glyphs, LIGHT, Palette, background_of, c, g, git, init_theme, seen_background, setting_theme, theme_name,
    theme_setting, use_glyphs, use_theme,
};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_round_rounds_halves_up() {
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-0.5), 0.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(2.4), 2.0);
    }
}
