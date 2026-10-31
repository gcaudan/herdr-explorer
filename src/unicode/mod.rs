//! The Unicode classes of the regular expressions (`\p{L}`, `\p{N}`, `\p{M}`, `\p{Emoji}`,
//! `\p{Emoji_Presentation}`, `\p{Regional_Indicator}`, `\s`), from generated tables (Unicode 17.0). And
//! the natural order of names, for the tree and the lists.

#[rustfmt::skip]
mod tables;
use std::cmp::Ordering;

fn within(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(a, b)| {
            if b < c {
                Ordering::Less
            } else if a > c {
                Ordering::Greater
            } else {
                Ordering::Equal
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

/// `\s`: white space and line terminators.
pub fn is_space(c: char) -> bool {
    within(tables::SPACE, c)
}

/// `[\p{L}\p{N}]`
pub fn is_alnum(c: char) -> bool {
    is_letter(c) || is_number(c)
}

/// The order of names: runs of ASCII digits compare as numbers, the rest character by character with case and
/// Latin-1's accents ignored. Names equal so far then put the unaccented first, then lowercase before uppercase and
/// fewer leading zeros first, and last the bytes decide, so that two different names never tie.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    let (mut accent, mut tie) = (Ordering::Equal, Ordering::Equal);
    loop {
        let (c, d) = match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return accent.then(tie).then_with(|| a.cmp(b)),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) => (c, d),
        };
        if c.is_ascii_digit() && d.is_ascii_digit() {
            let (m, n) = (digits(&mut x), digits(&mut y));
            let (m0, n0) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
            let order = m0.len().cmp(&n0.len()).then_with(|| m0.cmp(n0));
            if order != Ordering::Equal {
                return order;
            }
            tie = tie.then(m.len().cmp(&n.len()));
        } else {
            x.next();
            y.next();
            let order = fold(c).cmp(&fold(d));
            if order != Ordering::Equal {
                return order;
            }
            let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
            accent = accent.then((lower(c) != fold(c)).cmp(&(lower(d) != fold(d))));
            tie = tie.then(c.is_uppercase().cmp(&d.is_uppercase()));
        }
    }
}

fn digits(it: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut run = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        run.push(c);
    }
    run
}

fn fold(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        c => c,
    }
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

    #[test]
    fn natural_order() {
        let mut names =
            ["file10", "File2", "file2", "file1", "file01", "b", "é", "E", "e", "A", "a", "f", "_x", "10", "9"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            ["9", "10", "_x", "a", "A", "b", "e", "E", "é", "f", "file1", "file01", "file2", "File2", "file10"]
        );
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
    }
}
