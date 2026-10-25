//! The Unicode classes of the JS's regular expressions (`\p{L}`, `\p{N}`, `\p{M}`, `\p{Emoji}`,
//! `\p{Emoji_Presentation}`, `\p{Regional_Indicator}`, `\s`), from tables generated with the node the JS ran on
//! (tools/unicode/gen.js, removed at the switch to Rust: see the git history before it), so that both agree on every
//! character.

#[rustfmt::skip]
mod tables;
#[rustfmt::skip]
mod collation;

pub use collation_order::locale_compare;
mod collation_order;

fn within(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(a, b)| {
            if b < c {
                std::cmp::Ordering::Less
            } else if a > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `\p{L}`
pub fn is_letter(c: char) -> bool {
    within(tables::LETTER, c)
}

/// `\p{N}`
pub fn is_number(c: char) -> bool {
    within(tables::NUMBER, c)
}

/// `\p{M}`
pub fn is_mark(c: char) -> bool {
    within(tables::MARK, c)
}

/// `\p{Emoji}`
pub fn is_emoji(c: char) -> bool {
    within(tables::EMOJI, c)
}

/// `\p{Emoji_Presentation}`
pub fn is_emoji_presentation(c: char) -> bool {
    within(tables::EMOJI_PRESENTATION, c)
}

/// `\p{Regional_Indicator}`
pub fn is_regional_indicator(c: char) -> bool {
    within(tables::REGIONAL_INDICATOR, c)
}

/// `\s`: JS's white space and line terminators.
pub fn is_space(c: char) -> bool {
    within(tables::SPACE, c)
}

/// `[\p{L}\p{N}]`
pub fn is_alnum(c: char) -> bool {
    is_letter(c) || is_number(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert!(is_letter('é') && is_letter('界') && !is_letter('1') && !is_letter('_'));
        assert!(is_number('٣') && is_number('½') && !is_number('a'));
        assert!(is_mark('\u{301}') && !is_mark('a'));
        assert!(is_emoji('#') && is_emoji('😀') && !is_emoji_presentation('#') && is_emoji_presentation('😀'));
        assert!(is_regional_indicator('🇫') && !is_regional_indicator('F'));
        assert!(is_space('\u{feff}') && is_space('\u{a0}') && is_space('\n') && !is_space('\u{85}'));
        for table in [tables::LETTER, tables::NUMBER, tables::MARK, tables::EMOJI, tables::SPACE] {
            assert!(table.windows(2).all(|w| w[0].1 < w[1].0 && w[0].0 <= w[0].1));
        }
    }
}
