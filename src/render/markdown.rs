//! Markdown rendered for the terminal: headings, paragraphs with inline styles, lists, quotes, code blocks
//! highlighted by `render::highlight`, GFM tables and task lists, simple Mermaid flowcharts. A small CommonMark-like
//! parser, not a complete one. Output: lines of (text, style) segments that fit the width, for `ui::line`.

use crate::core::regex::Regex;
use crate::core::ui::{self, Rgb, Seg};
use crate::render::highlight;
use crate::unicode;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;

const BOLD: &str = "\x1b[1m";
const ITALIC: &str = "\x1b[3m";
const UNDER: &str = "\x1b[4m";
const STRIKE: &str = "\x1b[9m";

// Colours by theme (`ui::use_theme`), and the alerts' ([!NOTE]…).
struct Palette {
    h: [Rgb; 6],
    link: Rgb,
    image: Rgb,
    code: Rgb,
    code_bg: Rgb,
    block_bg: Rgb,
    border: Rgb,
    dim: Rgb,
    marker: Rgb,
    green: Rgb,
    alerts: [(&'static str, &'static str, Rgb); 5],
}

const DARK: Palette = Palette {
    h: [[86, 156, 214], [78, 201, 176], [220, 220, 170], [197, 134, 192], [156, 220, 254], [160, 160, 160]],
    link: [55, 148, 255],
    image: [197, 134, 192],
    code: [206, 145, 120],
    code_bg: [50, 50, 50],
    block_bg: [36, 36, 36],
    border: [90, 90, 90],
    dim: [140, 140, 140],
    marker: [86, 156, 214],
    green: [129, 184, 139],
    alerts: [
        ("note", "ℹ Note", [55, 148, 255]),
        ("tip", "✔ Tip", [129, 184, 139]),
        ("important", "❖ Important", [197, 134, 192]),
        ("warning", "⚠ Warning", [220, 180, 90]),
        ("caution", "⛔ Caution", [244, 135, 113]),
    ],
};

const LIGHT: Palette = Palette {
    h: [[0, 0, 255], [38, 127, 153], [121, 94, 38], [175, 0, 219], [0, 16, 128], [100, 100, 100]],
    link: [0, 102, 204],
    image: [175, 0, 219],
    code: [163, 21, 21],
    code_bg: [236, 236, 236],
    block_bg: [245, 245, 245],
    border: [190, 190, 190],
    dim: [110, 110, 110],
    marker: [0, 0, 255],
    green: [9, 134, 88],
    alerts: [
        ("note", "ℹ Note", [9, 105, 218]),
        ("tip", "✔ Tip", [26, 127, 55]),
        ("important", "❖ Important", [130, 80, 223]),
        ("warning", "⚠ Warning", [154, 103, 0]),
        ("caution", "⛔ Caution", [207, 34, 46]),
    ],
};

fn col() -> &'static Palette {
    if ui::theme_name() == "light" { &LIGHT } else { &DARK }
}

fn link_style(p: &Palette) -> String {
    ui::fg(p.link) + UNDER
}

fn code_style(p: &Palette) -> String {
    ui::fg(p.code) + &ui::bg(p.code_bg)
}

fn dim() -> String {
    ui::fg(col().dim)
}

fn border() -> String {
    ui::fg(col().border)
}

const BULLETS: [&str; 3] = ["•", "◦", "▪"];
const MAX_INLINE: usize = 20000; // longer paragraphs are shown without inline markup
const MAX_DEPTH: usize = 16; // nested inline spans
const MAX_NEST: usize = 100; // nested quotes and lists

/// What the caller hangs on a line of an image (a picture placed in pixels over blank lines), kept whatever prefix a
/// list or a quote adds.
pub type Figure = Rc<dyn Any>;

/// A rendered line: its segments, the slug of the heading it starts, the image it carries.
#[derive(Clone, Default)]
pub struct Line {
    pub segs: Vec<Seg>,
    pub anchor: Option<String>,
    pub figure: Option<Figure>,
}

impl Line {
    fn of(segs: Vec<Seg>) -> Line {
        Line { segs, ..Line::default() }
    }
}

fn seg(text: impl Into<String>, style: impl Into<String>) -> Seg {
    (text.into(), style.into())
}

// --- segments

// Appends a segment, merged with the previous one when the style is the same.
fn push(out: &mut Vec<Seg>, text: &str, style: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.1 == style => last.0.push_str(text),
        _ => out.push(seg(text, style)),
    }
}

fn plain(segs: &[Seg]) -> String {
    segs.iter().map(|s| s.0.as_str()).collect()
}

fn width_of(segs: &[Seg]) -> usize {
    segs.iter().map(|s| ui::text_width(&s.0)).sum()
}

// Prefixes the first line and the others (hanging indent); keeps heading anchors and figures.
fn prefixed(lines: Vec<Line>, first: &[Seg], rest: &[Seg]) -> Vec<Line> {
    lines
        .into_iter()
        .enumerate()
        .map(|(k, l)| {
            let mut segs = if k > 0 { rest.to_vec() } else { first.to_vec() };
            segs.extend(l.segs);
            // An empty anchor (a heading of punctuation only) is dropped.
            Line { segs, anchor: l.anchor.filter(|a| !a.is_empty()), figure: l.figure }
        })
        .collect()
}

// Cuts segments into lines of at most width columns, without looking for spaces (code).
fn chop(segs: &[Seg], width: usize) -> Vec<Vec<Seg>> {
    let mut lines = Vec::new();
    let mut cur = Vec::new();
    let mut w = 0;
    for (t, s) in segs {
        let mut rest = t.as_str();
        while !rest.is_empty() {
            let (part, pw) = ui::fit(rest, width - w, None);
            if part.is_empty() {
                if w == 0 {
                    // Wider than the whole line: dropped.
                    rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
                    continue;
                }
                lines.push(std::mem::take(&mut cur));
                w = 0;
                continue;
            }
            push(&mut cur, part, s);
            w += pw;
            rest = &rest[part.len()..];
        }
    }
    lines.push(cur);
    lines
}

static PIECES: LazyLock<Regex> = LazyLock::new(|| re(r"(\n| +)", ""));

// Word wrap of styled segments: breaks at spaces, '\n' forces a line break, words longer than the width are cut.
fn wrap(segs: &[Seg], width: usize) -> Vec<Vec<Seg>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Seg>> = Vec::new();
    let mut cur: Vec<Seg> = Vec::new();
    let mut w = 0;
    let mut space: Option<Seg> = None;
    let mut word: Vec<Seg> = Vec::new();
    let mut ww = 0;
    fn br(lines: &mut Vec<Vec<Seg>>, cur: &mut Vec<Seg>, w: &mut usize, space: &mut Option<Seg>) {
        lines.push(std::mem::take(cur));
        *w = 0;
        *space = None;
    }
    let place = |lines: &mut Vec<Vec<Seg>>,
                 cur: &mut Vec<Seg>,
                 w: &mut usize,
                 space: &mut Option<Seg>,
                 word: &mut Vec<Seg>,
                 ww: &mut usize| {
        if word.is_empty() {
            return;
        }
        let sw = space.as_ref().map_or(0, |s| ui::text_width(&s.0));
        if *w > 0 && *w + sw + *ww > width {
            br(lines, cur, w, space);
        }
        if *w > 0
            && let Some((t, s)) = space.as_ref()
        {
            push(cur, t, s);
            *w += sw;
        }
        *space = None;
        if *w + *ww <= width {
            for (t, s) in word.iter() {
                push(cur, t, s);
                *w += ui::text_width(t);
            }
        } else {
            for (k, p) in chop(word, width).into_iter().enumerate() {
                if k > 0 {
                    br(lines, cur, w, space);
                }
                for (t, s) in &p {
                    push(cur, t, s);
                }
                *w = width_of(&p);
            }
        }
        word.clear();
        *ww = 0;
    };
    for (t, s) in segs {
        for piece in PIECES.split(t).into_iter().flatten() {
            if piece.is_empty() {
                continue;
            }
            if piece == "\n" {
                place(&mut lines, &mut cur, &mut w, &mut space, &mut word, &mut ww);
                br(&mut lines, &mut cur, &mut w, &mut space);
            } else if piece.starts_with(' ') {
                place(&mut lines, &mut cur, &mut w, &mut space, &mut word, &mut ww);
                if w > 0 {
                    space = Some(match space.take() {
                        Some((st, ss)) => (st + piece, ss),
                        None => seg(piece, s.as_str()),
                    });
                }
            } else {
                word.push(seg(piece, s.as_str()));
                ww += ui::text_width(piece);
            }
        }
    }
    place(&mut lines, &mut cur, &mut w, &mut space, &mut word, &mut ww);
    lines.push(cur);
    lines
}

// Pads a line of segments to the width with a background.
fn fill(mut segs: Vec<Seg>, width: usize, back: &str) -> Vec<Seg> {
    let pad = width.saturating_sub(width_of(&segs));
    segs.push(seg(" ".repeat(pad), back));
    segs
}

// --- inline

const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", "\u{a0}"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("mdash", "—"),
    ("ndash", "–"),
    ("hellip", "…"),
    ("larr", "←"),
    ("rarr", "→"),
    ("uarr", "↑"),
    ("darr", "↓"),
    ("times", "×"),
    ("middot", "·"),
    ("bull", "•"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("deg", "°"),
    ("check", "✓"),
    ("emsp", "\u{2003}"),
    ("ensp", "\u{2002}"),
    ("thinsp", "\u{2009}"),
];

fn re(src: &str, flags: &str) -> Regex {
    Regex::new(src, flags).expect("a valid pattern")
}

static SPECIAL: LazyLock<Regex> = LazyLock::new(|| re(r"[\\`*_~[!<&\n]|\bhttps?:\/\/", "g"));
static PUNCT: LazyLock<Regex> = LazyLock::new(|| re(r"[!-/:-@[-`{-~]", ""));
static ENTITY: LazyLock<Regex> = LazyLock::new(|| re(r"&(#\d{1,7}|#[xX][\da-fA-F]{1,6}|[A-Za-z]{2,8});", "y"));
static AUTOLINK: LazyLock<Regex> =
    LazyLock::new(|| re(r"<((?:https?|ftp|mailto):[^\s<>]*|[^\s<>@]+@[^\s<>@]+\.\w+)>", "y"));
static TAG: LazyLock<Regex> = LazyLock::new(|| {
    re(r#"<(\/?)([A-Za-z][\w-]*)((?:\s+[^\s>/=<]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>"'<]+))?)*)\s*(\/?)>"#, "y")
});
static URL: LazyLock<Regex> = LazyLock::new(|| re(r"https?:\/\/[^\s<>]*", "y"));
static URL_END: LazyLock<Regex> = LazyLock::new(|| re(r#"[.,:;!?"'*_~]+$"#, ""));
static IMAGE_LABEL: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*!\[", ""));
static LINK_URL: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:<([^>]*)>|(\S*))", ""));

// The style of a formatting tag. The code and link styles are always the dark theme's,
// whatever theme is chosen.
fn tag_style(name: &str) -> Option<String> {
    Some(match name {
        "b" | "strong" | "summary" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => BOLD.into(),
        "i" | "em" => ITALIC.into(),
        "s" | "del" | "strike" => STRIKE.into(),
        "u" | "ins" => UNDER.into(),
        "code" | "kbd" | "tt" | "samp" => code_style(&DARK),
        "a" => link_style(&DARK),
        "mark" => "\x1b[7m".into(),
        "sup" | "sub" | "span" | "small" | "p" | "div" | "center" => String::new(),
        _ => return None,
    })
}

// The character at byte i, as `text[i]` reads it for the tests below: None past the end.
fn at(text: &str, i: usize) -> Option<char> {
    text.get(i..).and_then(|t| t.chars().next())
}

fn run(text: &str, i: usize, ch: u8) -> usize {
    text.as_bytes()[i.min(text.len())..].iter().take_while(|&&b| b == ch).count()
}

fn is_space(ch: Option<char>) -> bool {
    ch.is_none_or(crate::unicode::is_space)
}

// A letter or a digit.
fn is_word(ch: Option<char>) -> bool {
    ch.is_some_and(unicode::is_alnum)
}

fn char_before(text: &str, i: usize) -> Option<char> {
    text[..i].chars().next_back()
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let m = re(&format!(r#"(?:^|\s){name}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#), "i").exec(attrs)?;
    m.get(1).or(m.get(2)).or(m.get(3)).map(String::from)
}

// End of the code span opened at i (after its closing backticks), or 0.
fn close_ticks(text: &str, i: usize) -> usize {
    let n = run(text, i, b'`');
    let ticks = "`".repeat(n);
    let mut k = i + n;
    loop {
        let Some(j) = text.get(k..).and_then(|t| t.find(&ticks)).map(|j| k + j) else { return 0 };
        let r = run(text, j, b'`');
        if r == n {
            return j + n;
        }
        k = j + r;
    }
}

// The width of the character at byte j (1 past the end), to step over it.
fn step(text: &str, j: usize) -> usize {
    at(text, j).map_or(1, char::len_utf8)
}

/// The state of a rendering: link references, heading slugs taken, whether code is highlighted, the images' lines,
/// the images of the paragraph being rendered.
struct Ctx<'a> {
    refs: HashMap<String, String>,
    slugs: HashMap<String, i64>,
    hl: bool,
    image: Option<&'a mut ImageFn<'a>>,
    found: Option<Vec<String>>,
    // Blocks nested in the one being parsed, and whether the text went deeper than MAX_NEST.
    depth: usize,
    deep: bool,
}

type Span = (usize, Vec<Seg>);

// [text](url), [text][ref], [ref] and images; None when the brackets are not a link.
fn link(text: &str, i: usize, base: &str, ctx: &mut Ctx, depth: usize, image: bool) -> Option<Span> {
    let open = if image { i + 1 } else { i };
    let mut j = open;
    let mut d = 0i64;
    let b = text.as_bytes();
    while j < text.len() {
        match b[j] {
            // The escaped character with it (stepped over below).
            b'\\' => j += 1,
            b'`' => {
                let k = close_ticks(text, j);
                if k > 0 {
                    j = k - 1;
                }
            }
            b'[' => d += 1,
            b']' => {
                d -= 1;
                if d == 0 {
                    break;
                }
            }
            _ => {}
        }
        j += step(text, j);
    }
    if j >= text.len() {
        return None;
    }
    let label = &text[open + 1..j];
    let mut end = j + 1;
    let url = if b.get(end) == Some(&b'(') {
        let mut k = end;
        d = 0;
        while k < text.len() {
            match b[k] {
                b'\\' => k += 1,
                b'(' => d += 1,
                b')' => {
                    d -= 1;
                    if d == 0 {
                        break;
                    }
                }
                _ => {}
            }
            k += step(text, k);
        }
        if k >= text.len() {
            return None;
        }
        // The title is left out.
        let inner = &text[end + 1..k];
        let m = LINK_URL.exec(inner)?;
        let url = m.get(1).or(m.get(2)).unwrap_or("").to_string();
        end = k + 1;
        url
    } else {
        let mut r = label;
        if b.get(end) == Some(&b'[') {
            let k = text[end..].find(']').map(|k| end + k)?;
            r = &text[end + 1..k];
            if r.is_empty() {
                r = label;
            }
            end = k + 1;
        }
        ctx.refs.get(&r.trim_matches(crate::unicode::is_space).to_lowercase())?.clone()
    };
    if image {
        if let Some(found) = ctx.found.as_mut().filter(|_| !url.is_empty()) {
            found.push(url.clone());
        }
        let alt = plain(&inline(label, "", ctx, depth + 1));
        let alt = if alt.is_empty() { "image".into() } else { alt };
        return Some((end, vec![seg(format!("🖼 {alt}"), format!("{base}{}", ui::fg(col().image)))]));
    }
    let mut segs = inline(label, &format!("{base}{}", link_style(col())), ctx, depth + 1);
    let shown = plain(&segs);
    if !url.is_empty() && url != shown && url != format!("mailto:{shown}") && !IMAGE_LABEL.test(label) {
        push(&mut segs, &format!(" ({url})"), &dim());
    }
    Some((end, segs))
}

// **bold**, *italic*, ***both***, ~~strike~~ (also with _); the whole delimiter run when not closed.
fn emphasis(
    text: &str,
    i: usize,
    base: &str,
    ctx: &mut Ctx,
    depth: usize,
    unclosed: &mut HashSet<(u8, usize)>,
) -> Span {
    let b = text.as_bytes();
    let ch = b[i];
    let n = run(text, i, ch);
    let literal = (i + n, vec![seg((ch as char).to_string().repeat(n), base)]);
    if n > 3 || (ch == b'~' && n > 2) || is_space(at(text, i + n)) || (ch == b'_' && is_word(char_before(text, i))) {
        return literal;
    }
    // No closer after an earlier opener: none after this one either.
    if unclosed.contains(&(ch, n)) {
        return literal;
    }
    let mut j = i + n;
    while j < text.len() {
        let c = b[j];
        if c == b'\\' {
            j += 1 + step(text, j + 1);
            continue;
        }
        if c == b'`' {
            let k = close_ticks(text, j);
            if k > 0 {
                j = k;
                continue;
            }
        }
        if c != ch {
            j += step(text, j);
            continue;
        }
        let r = run(text, j, ch);
        if r == n && !is_space(char_before(text, j)) && !(ch == b'_' && is_word(at(text, j + r))) {
            let style = if ch == b'~' {
                STRIKE.to_string()
            } else {
                format!("{}{}", if n & 1 == 1 { ITALIC } else { "" }, if n >= 2 { BOLD } else { "" })
            };
            return (j + r, inline(&text[i + n..j], &format!("{base}{style}"), ctx, depth + 1));
        }
        j += r;
    }
    unclosed.insert((ch, n));
    literal
}

// Inline HTML: <br>, <img alt>, a few formatting tags; other tags are hidden, their contents kept.
fn tag(text: &str, i: usize, base: &str, ctx: &mut Ctx, depth: usize) -> Option<Span> {
    if text[i..].starts_with("<!--") {
        let k = text[i + 4..].find("-->").map(|k| i + 4 + k);
        return Some((k.map_or(text.len(), |k| k + 3), vec![]));
    }
    if let Some(m) = AUTOLINK.exec_at(text, i) {
        return Some((m.end(), vec![seg(m.get(1).unwrap_or(""), format!("{base}{}", link_style(col())))]));
    }
    let m = TAG.exec_at(text, i)?;
    let end = m.end();
    let name = m.get(2).unwrap_or("").to_lowercase();
    let attrs = m.get(3).unwrap_or("");
    if !m.get(1).unwrap_or("").is_empty() {
        return Some((end, vec![]));
    }
    if name == "br" {
        return Some((end, vec![seg("\n", "")]));
    }
    if name == "img" {
        if let (Some(found), Some(src)) = (ctx.found.as_mut(), attr(attrs, "src").filter(|s| !s.is_empty())) {
            found.push(src);
        }
        let alt = attr(attrs, "alt").filter(|a| !a.is_empty()).unwrap_or_else(|| "image".into());
        return Some((end, vec![seg(format!("🖼 {alt}"), format!("{base}{}", ui::fg(col().image)))]));
    }
    let Some(style) = tag_style(&name).filter(|_| m.get(4).unwrap_or("").is_empty()) else {
        return Some((end, vec![]));
    };
    let Some(close) = highlight::find_lower(text, &format!("</{name}"), end) else { return Some((end, vec![])) };
    let inner = &text[end..close];
    let after = text[close..].find('>').map_or(text.len(), |k| close + k + 1);
    let code = code_style(col());
    let mut segs = if style == code {
        vec![seg(inner.replace('\n', " "), code)]
    } else {
        inline(inner, &format!("{base}{style}"), ctx, depth + 1)
    };
    if name == "a"
        && let Some(href) = attr(attrs, "href").filter(|h| !h.is_empty() && *h != plain(&segs))
    {
        push(&mut segs, &format!(" ({href})"), &dim());
    }
    Some((after, segs))
}

// `&…;`: the character it stands for, cleaned, or None.
fn entity(e: &str) -> Option<String> {
    let ch = match e.strip_prefix('#') {
        None => ENTITIES.iter().find(|(k, _)| *k == e).map(|(_, v)| v.to_string()),
        Some(num) => {
            let cp = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse::<u32>().ok(),
            };
            cp.filter(|&cp| cp > 0).and_then(char::from_u32).map(String::from)
        }
    }?;
    Some(ui::clean_line(&ch, 4, None))
}

// Segments of a paragraph text; '\n' marks hard line breaks.
fn inline(text: &str, base: &str, ctx: &mut Ctx, depth: usize) -> Vec<Seg> {
    if depth > MAX_DEPTH || text.chars().count() > MAX_INLINE {
        return vec![seg(text, base)];
    }
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    let mut unclosed = HashSet::new();
    let b = text.as_bytes();
    while i < text.len() {
        let Some(m) = SPECIAL.exec_at(text, i) else {
            buf.push_str(&text[i..]);
            break;
        };
        buf.push_str(&text[i..m.start()]);
        i = m.start();
        let c = b[i];
        let r: Option<Span> = match c {
            b'\\' => at(text, i + 1)
                .filter(|n| PUNCT.test(&n.to_string()))
                .map(|n| (i + 1 + n.len_utf8(), vec![seg(n.to_string(), base)])),
            b'`' => {
                let k = close_ticks(text, i);
                let n = run(text, i, b'`');
                if k > 0 {
                    let mut code = text[i + n..k - n].replace('\n', " ");
                    if code.chars().count() > 2
                        && code.starts_with(' ')
                        && code.ends_with(' ')
                        && !code.trim_matches(crate::unicode::is_space).is_empty()
                    {
                        code = code[1..code.len() - 1].to_string();
                    }
                    Some((k, vec![seg(code, code_style(col()))]))
                } else {
                    Some((i + n, vec![seg("`".repeat(n), base)]))
                }
            }
            b'*' | b'_' | b'~' => Some(emphasis(text, i, base, ctx, depth, &mut unclosed)),
            b'[' => link(text, i, base, ctx, depth, false),
            b'!' => {
                if b.get(i + 1) == Some(&b'[') {
                    link(text, i, base, ctx, depth, true)
                } else {
                    None
                }
            }
            b'<' => tag(text, i, base, ctx, depth),
            b'\n' => Some((i + 1, vec![seg("\n", "")])),
            b'&' => ENTITY.exec_at(text, i).and_then(|k| Some((k.end(), vec![seg(entity(k.get(1)?)?, base)]))),
            b'h' => URL.exec_at(text, i).map(|k| {
                // Bare URL (GFM autolink): trailing punctuation and unbalanced ')' are not part of it.
                let mut url = URL_END.replace_with(k.as_str(), false, "");
                while url.ends_with(')') && url.matches('(').count() < url.matches(')').count() {
                    url.pop();
                }
                (i + url.len(), vec![seg(url.clone(), format!("{base}{}", link_style(col())))])
            }),
            _ => None,
        };
        match r {
            Some((end, segs)) => {
                push(&mut out, &std::mem::take(&mut buf), base);
                for (t, s) in &segs {
                    push(&mut out, t, s);
                }
                i = end;
            }
            None => {
                buf.push_str(m.as_str());
                i += m.as_str().len();
            }
        }
    }
    push(&mut out, &buf, base);
    out
}

// --- blocks

#[derive(Debug, Clone, Copy, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// A task list item's box: GFM's `[ ]` and `[x]`, and the statuses of Obsidian's themes: `[?]` to check, `[-]`
/// dropped, `[/]` or `[~]` in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Task {
    Open,
    Done,
    Check,
    Dropped,
    Doing,
}

#[derive(Debug, Clone)]
struct Item {
    task: Option<Task>,
    loose: bool,
    blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
enum Block {
    Code { lang: String, lines: Vec<String> },
    Heading { level: usize, text: String },
    Hr,
    Quote(Vec<Block>),
    List { ordered: bool, delim: char, start: u64, items: Vec<Item>, loose: bool },
    Table { head: Vec<String>, rows: Vec<Vec<String>>, align: Vec<Align> },
    Para(String),
    Front(Vec<String>),
}

static BLANK: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*$", ""));
static FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"^( {0,3})(`{3,}|~{3,})(.*)$", ""));
static ATX: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(#{1,6})(?:[ \t]+|$)(.*)$", ""));
static HR: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$", ""));
static SETEXT: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(=+|-+)[ \t]*$", ""));
static QUOTE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}> ?", ""));
static ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^( {0,3})([-+*]|\d{1,9}[.)])(?= |$)( *)", ""));
static TSEP: LazyLock<Regex> =
    LazyLock::new(|| re(r"^ {0,3}\|?(?:[ \t]*:?-+:?[ \t]*\|)*[ \t]*:?-+:?[ \t]*\|?[ \t]*$", ""));
static REF: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}\[([^\]]+)\]:[ \t]*<?(\S+?)>?(?:[ \t]+.*)?$", ""));
static COMMENT: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}<!--", ""));
static FENCE_LANG: LazyLock<Regex> = LazyLock::new(|| re(r"^\{\.?|\}$", "g"));
static LANG_END: LazyLock<Regex> = LazyLock::new(|| re(r"[\s,{]", ""));
static CLOSING_FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(`{3,}|~{3,})[ \t]*$", ""));
static INDENTED: LazyLock<Regex> = LazyLock::new(|| re(r"^ {4}", ""));
static ATX_END: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|[ \t]+)#+[ \t]*$", ""));
static HARD_BREAK: LazyLock<Regex> = LazyLock::new(|| re(r" {2,}$", ""));
static TASK: LazyLock<Regex> = LazyLock::new(|| re(r"^\[([ xX?/~-])\](?: +|$)", ""));

struct Fence {
    indent: usize,
    mark: String,
    lang: String,
}

fn fence(l: &str) -> Option<Fence> {
    let m = FENCE.exec(l)?;
    let (mark, info) = (m.get(2).unwrap_or(""), m.get(3).unwrap_or(""));
    if mark.starts_with('`') && info.contains('`') {
        return None;
    }
    let lang = FENCE_LANG.replace_with(info.trim_matches(crate::unicode::is_space), true, "");
    let lang = LANG_END.split(&lang).first().copied().flatten().unwrap_or("").to_lowercase();
    Some(Fence { indent: m.get(1).unwrap_or("").len(), mark: mark.into(), lang })
}

fn interrupts(l: &str) -> bool {
    fence(l).is_some()
        || ATX.test(l)
        || HR.test(l)
        || QUOTE.test(l)
        || COMMENT.test(l)
        || ITEM.exec(l).is_some_and(|m| !BLANK.test(&l[m.end()..]))
}

fn trim(s: &str) -> &str {
    s.trim_matches(crate::unicode::is_space)
}

// The text after its first n characters.
fn skip(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or("", |(i, _)| &s[i..])
}

// Cells of a table row, split at unescaped pipes.
fn cells(row: &str) -> Vec<String> {
    let mut s = trim(row);
    if let Some(r) = s.strip_prefix('|') {
        s = r;
    }
    if s.ends_with('|') && !s.ends_with("\\|") {
        s = &s[..s.len() - 1];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            out.push(trim(&cur).to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(trim(&cur).to_string());
    out
}

fn is_table(lines: &[String], i: usize) -> bool {
    i + 1 < lines.len()
        && lines[i].contains('|')
        && lines[i + 1].contains('|')
        && TSEP.test(&lines[i + 1])
        && cells(&lines[i]).len() == cells(&lines[i + 1]).len()
}

// Paragraph text: soft line breaks become spaces, hard ones (two trailing spaces or a backslash) '\n'.
fn join_lines(ls: &[String]) -> String {
    ls.iter()
        .enumerate()
        .map(|(k, l)| {
            let t = trim(l);
            if k == ls.len() - 1 {
                t.strip_suffix('\\').unwrap_or(t).to_string()
            } else if HARD_BREAK.test(l) {
                format!("{t}\n")
            } else if let Some(t) = t.strip_suffix('\\') {
                format!("{t}\n")
            } else {
                format!("{t} ")
            }
        })
        .collect()
}

fn digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

fn parse_list(lines: &[String], mut i: usize, ctx: &mut Ctx) -> (Block, usize) {
    let first = ITEM.exec(&lines[i]).expect("a list item");
    let marker0 = first.get(2).unwrap_or("");
    let ordered = digit(marker0);
    let delim = marker0.chars().last().unwrap_or('-');
    let start = if ordered { marker0[..marker0.len() - 1].parse::<u64>().unwrap_or(0) } else { 1 };
    let (mut items, mut loose) = (Vec::new(), false);
    while i < lines.len() {
        let Some(m) = ITEM.exec(&lines[i]) else { break };
        let mk = m.get(2).unwrap_or("");
        if digit(mk) != ordered || !mk.ends_with(delim) {
            break;
        }
        let marker = m.get(1).unwrap_or("").len() + mk.len();
        let pad = m.get(3).unwrap_or("").len();
        let rest = &lines[i][marker + pad..];
        let indent = if pad == 0 || pad > 4 || BLANK.test(rest) { marker + 1 } else { marker + pad };
        let mut body = vec![skip(&lines[i], indent).to_string()];
        let mut blank = false;
        let mut in_fence = fence(&body[0]).is_some();
        i += 1;
        while i < lines.len() {
            let l = &lines[i];
            if BLANK.test(l) {
                body.push(String::new());
                blank = true;
                i += 1;
                continue;
            }
            let ind = l.chars().count() - l.trim_start_matches(crate::unicode::is_space).chars().count();
            if ind >= indent {
                body.push(skip(l, indent).to_string());
                blank = false;
                if fence(skip(l, indent)).is_some() {
                    in_fence = !in_fence;
                }
                i += 1;
                continue;
            }
            if blank || in_fence || ITEM.test(l) || interrupts(l) {
                break;
            }
            // Lazy continuation of a paragraph.
            body.push(l.trim_start_matches(crate::unicode::is_space).to_string());
            i += 1;
        }
        let mut trailing = false;
        while body.len() > 1 && body.last().is_some_and(String::is_empty) {
            body.pop();
            trailing = true;
        }
        let item_loose = body.iter().any(String::is_empty);
        let mut task = None;
        if let Some(t) = TASK.exec(&body[0]) {
            task = Some(match t.get(1) {
                Some("x" | "X") => Task::Done,
                Some("?") => Task::Check,
                Some("-") => Task::Dropped,
                Some("/" | "~") => Task::Doing,
                _ => Task::Open,
            });
            let n = t.end();
            body[0] = body[0][n..].to_string();
        }
        let blocks = parse(&body, ctx);
        items.push(Item { task, loose: item_loose, blocks });
        if let Some(next) = lines.get(i).and_then(|l| ITEM.exec(l)) {
            let nm = next.get(2).unwrap_or("");
            if trailing && digit(nm) == ordered && nm.ends_with(delim) {
                loose = true;
            }
        }
    }
    (Block::List { ordered, delim, start, items, loose }, i)
}

fn parse(lines: &[String], ctx: &mut Ctx) -> Vec<Block> {
    if ctx.depth >= MAX_NEST {
        ctx.deep = true;
        return Vec::new();
    }
    ctx.depth += 1;
    let blocks = parse_blocks(lines, ctx);
    ctx.depth -= 1;
    blocks
}

fn parse_blocks(lines: &[String], ctx: &mut Ctx) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if BLANK.test(l) {
            i += 1;
            continue;
        }
        if let Some(f) = fence(l) {
            let mut code = Vec::new();
            let strip = re(&format!("^ {{0,{}}}", f.indent), "");
            i += 1;
            while i < lines.len() {
                let c = CLOSING_FENCE.exec(&lines[i]);
                if c.is_some_and(|c| {
                    let m = c.get(1).unwrap_or("");
                    m.as_bytes()[0] == f.mark.as_bytes()[0] && m.len() >= f.mark.len()
                }) {
                    i += 1;
                    break;
                }
                code.push(strip.replace_with(&lines[i], false, ""));
                i += 1;
            }
            blocks.push(Block::Code { lang: f.lang, lines: code });
        } else if INDENTED.test(l) {
            let mut code = Vec::new();
            while i < lines.len() && (INDENTED.test(&lines[i]) || BLANK.test(&lines[i])) {
                code.push(skip(&lines[i], 4).to_string());
                i += 1;
            }
            while code.last().is_some_and(|c| BLANK.test(c)) {
                code.pop();
            }
            blocks.push(Block::Code { lang: String::new(), lines: code });
        } else if let Some(m) = ATX.exec(l) {
            let text = ATX_END.replace_with(m.get(2).unwrap_or(""), false, "");
            blocks.push(Block::Heading { level: m.get(1).unwrap_or("#").len(), text: trim(&text).to_string() });
            i += 1;
        } else if HR.test(l) {
            blocks.push(Block::Hr);
            i += 1;
        } else if QUOTE.test(l) {
            let mut inner: Vec<String> = Vec::new();
            while i < lines.len() {
                if QUOTE.test(&lines[i]) {
                    inner.push(QUOTE.replace_with(&lines[i], false, ""));
                } else if !BLANK.test(&lines[i])
                    && inner.last().is_some_and(|last| !BLANK.test(last))
                    && !interrupts(&lines[i])
                {
                    // Lazy continuation.
                    inner.push(lines[i].clone());
                } else {
                    break;
                }
                i += 1;
            }
            blocks.push(Block::Quote(parse(&inner, ctx)));
        } else if ITEM.test(l) {
            let (list, next) = parse_list(lines, i, ctx);
            blocks.push(list);
            i = next;
        } else if COMMENT.test(l) {
            while i < lines.len() && !lines[i].contains("-->") {
                i += 1;
            }
            i += 1;
        } else if is_table(lines, i) {
            let head = cells(l);
            let align = cells(&lines[i + 1])
                .iter()
                .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
                    (true, true) => Align::Center,
                    (false, true) => Align::Right,
                    _ => Align::Left,
                })
                .collect();
            let mut rows = Vec::new();
            i += 2;
            while i < lines.len() && !BLANK.test(&lines[i]) && lines[i].contains('|') && !interrupts(&lines[i]) {
                rows.push(cells(&lines[i]));
                i += 1;
            }
            blocks.push(Block::Table { head, rows, align });
        } else if let Some(m) = REF.exec(l) {
            let key = trim(m.get(1).unwrap_or("")).to_lowercase();
            ctx.refs.entry(key).or_insert_with(|| m.get(2).unwrap_or("").to_string());
            i += 1;
        } else {
            let mut para = vec![l.clone()];
            let mut heading = None;
            i += 1;
            while i < lines.len() {
                let n = &lines[i];
                if BLANK.test(n) {
                    break;
                }
                if let Some(m) = SETEXT.exec(n) {
                    heading = Some(if m.get(1).unwrap_or("").starts_with('=') { 1 } else { 2 });
                    i += 1;
                    break;
                }
                if interrupts(n) || is_table(lines, i) {
                    break;
                }
                para.push(n.clone());
                i += 1;
            }
            let text = join_lines(&para);
            blocks.push(match heading {
                Some(level) => Block::Heading { level, text: trim(&text).to_string() },
                None => Block::Para(text),
            });
        }
    }
    blocks
}

// --- rendering

static SLUG_DROP: LazyLock<Regex> = LazyLock::new(|| re(r"[^\p{L}\p{N}\s_-]", "gu"));
static SLUG_SPACE: LazyLock<Regex> = LazyLock::new(|| re(r"\s", "g"));

fn slug(text: &str, ctx: &mut Ctx) -> String {
    let lowered = text.to_lowercase();
    let base = SLUG_DROP.replace_with(trim(&lowered), true, "");
    let base = SLUG_SPACE.replace_with(&base, true, "-");
    let n = ctx.slugs.entry(base.clone()).and_modify(|n| *n += 1).or_insert(0);
    if *n > 0 { format!("{base}-{n}") } else { base }
}

fn table(head: &[String], rows: &[Vec<String>], align: &[Align], width: usize, ctx: &mut Ctx) -> Vec<Line> {
    let n = head.len();
    let all: Vec<Vec<Vec<Seg>>> = std::iter::once(head)
        .chain(rows.iter().map(Vec::as_slice))
        .enumerate()
        .map(|(k, r)| {
            (0..n).map(|c| inline(r.get(c).map_or("", String::as_str), if k > 0 { "" } else { BOLD }, ctx, 0)).collect()
        })
        .collect();
    let border = border();
    let avail = width as i64 - 3 * n as i64 - 1;
    if avail < n as i64 * 2 {
        // Too narrow for columns: one "header: value" line per cell, rows separated by rules.
        let mut out = Vec::new();
        for (k, r) in all[1..].iter().enumerate() {
            if k > 0 {
                out.push(Line::of(vec![seg("─".repeat(width), border.as_str())]));
            }
            for (j, c) in r.iter().enumerate() {
                let mut segs = vec![seg(format!("{}: ", plain(&all[0][j])), format!("{BOLD}{}", dim()))];
                segs.extend(c.iter().cloned());
                out.extend(wrap(&segs, width).into_iter().map(Line::of));
            }
        }
        return out;
    }
    let column = |c: usize, f: &dyn Fn(&[Seg]) -> Vec<usize>| {
        all.iter().fold(1, |m, r| f(&r[c]).into_iter().fold(m, usize::max))
    };
    let natural: Vec<usize> =
        (0..n).map(|c| column(c, &|segs| wrap(segs, 1_000_000_000).iter().map(|l| width_of(l)).collect())).collect();
    // Longest word of each column: cut words only when the table cannot fit otherwise.
    let split = re(r"\s+", "");
    let word: Vec<usize> = natural
        .iter()
        .enumerate()
        .map(|(c, &x)| {
            x.min(column(c, &|segs| {
                split.split(&plain(segs)).into_iter().map(|p| ui::text_width(p.unwrap_or(""))).collect()
            }))
        })
        .collect();
    let mut w = natural.clone();
    // Shrinks the widest column that can still shrink, one column at a time, until the table fits.
    let mut floor = word;
    let mut total = w.iter().sum::<usize>() as i64;
    while total > avail {
        let mut k: Option<usize> = None;
        for (c, &x) in w.iter().enumerate() {
            if x > floor[c] && k.is_none_or(|k| x > w[k]) {
                k = Some(c);
            }
        }
        match k {
            None => {
                floor = vec![1; n];
                continue;
            }
            Some(k) => w[k] -= 1,
        }
        total -= 1;
    }
    let rule = |l: &str, mid: &str, r: &str| {
        let inner: Vec<String> = w.iter().map(|&x| "─".repeat(x + 2)).collect();
        Line::of(vec![seg(format!("{l}{}{r}", inner.join(mid)), border.as_str())])
    };
    let mut out = vec![rule("┌", "┬", "┐")];
    for (k, r) in all.iter().enumerate() {
        let wrapped: Vec<Vec<Vec<Seg>>> = r.iter().enumerate().map(|(j, c)| wrap(c, w[j])).collect();
        let h = wrapped.iter().map(Vec::len).max().unwrap_or(0);
        for y in 0..h {
            let mut line = vec![seg("│", border.as_str())];
            for (j, c) in wrapped.iter().enumerate() {
                let segs = c.get(y).cloned().unwrap_or_default();
                let gap = w[j] as i64 - width_of(&segs) as i64;
                let left = match align.get(j) {
                    Some(Align::Right) => gap,
                    Some(Align::Center) => gap >> 1,
                    _ => 0,
                };
                line.push(seg(" ".repeat((left + 1).max(0) as usize), ""));
                line.extend(segs);
                line.push(seg(" ".repeat((gap - left + 1).max(0) as usize), ""));
                line.push(seg("│", border.as_str()));
            }
            out.push(Line::of(line));
        }
        if k == 0 {
            out.push(rule("├", "┼", "┤"));
        }
    }
    out.push(rule("└", "┴", "┘"));
    out
}

// Mermaid flowchart (graph TD / LR): nodes in order of appearance, as boxes joined by arrows.
static M_NODE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"\s*([A-Za-z_]\w*)(?:\s*([[({>]+)\s*("[^"]*"|[^\])}"]*)\s*[\])}]+)?"#, "y"));
static M_ARROW: LazyLock<Regex> =
    LazyLock::new(|| re(r"\s*(?:--\s+([^-|>]+?)\s+-->|<?(?:--+|==+|-\.+-?)[>ox]?)\s*(?:\|([^|]*)\|)?", "y"));
static M_HEAD: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:flowchart|graph)\b", ""));
static M_SKIP: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*(?:subgraph|end|classDef|class|style|click|linkStyle|direction)\b", ""));
static M_COMMENT: LazyLock<Regex> = LazyLock::new(|| re(r"%%.*", ""));
static M_QUOTES: LazyLock<Regex> = LazyLock::new(|| re(r#"^"|"$"#, "g"));

fn mermaid(lines: &[String], width: usize) -> Option<Vec<Line>> {
    if !M_HEAD.test(lines.first().map_or("", String::as_str)) {
        return None;
    }
    let mut nodes: Vec<(String, String)> = Vec::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut node = |m: &crate::core::regex::Match| -> String {
        let label = M_QUOTES.replace_with(m.get(3).unwrap_or(""), true, "");
        let label = trim(&label).to_string();
        let id = m.get(1).unwrap_or("").to_string();
        match nodes.iter_mut().find(|(k, _)| *k == id) {
            None => nodes.push((id.clone(), if label.is_empty() { id.clone() } else { label })),
            Some(n) if !label.is_empty() => n.1 = label,
            Some(_) => {}
        }
        id
    };
    for raw in &lines[1.min(lines.len())..] {
        let raw = M_COMMENT.replace_with(raw, false, "");
        for st in raw.split(';') {
            if M_SKIP.test(st) || BLANK.test(st) {
                continue;
            }
            let Some(mut m) = M_NODE.exec_at(st, 0) else { continue };
            let mut from = node(&m);
            let mut at = m.end();
            while let Some(a) = M_ARROW.exec_at(st, at) {
                if trim(a.as_str()).is_empty() {
                    break;
                }
                let Some(next) = M_NODE.exec_at(st, a.end()) else { break };
                m = next;
                let to = node(&m);
                let label = a.get(1).filter(|s| !s.is_empty()).or(a.get(2)).unwrap_or("");
                edges.push((from, to.clone(), trim(label).to_string()));
                from = to;
                at = m.end();
            }
        }
    }
    if nodes.is_empty() {
        return None;
    }
    let border = border();
    let widest = nodes.iter().map(|(_, l)| ui::text_width(l) as i64).max().unwrap_or(0);
    let inner = (width as i64 - 4).min(widest).max(1) as usize;
    let box_w = inner + 4;
    let left = " ".repeat(((width.min(box_w + 20) as i64 - box_w as i64) >> 1).max(0) as usize);
    let mid = format!("{left}{}", " ".repeat(box_w >> 1));
    let mut out = Vec::new();
    for (k, (id, label)) in nodes.iter().enumerate() {
        let text = wrap(&[seg(label.as_str(), BOLD)], inner);
        out.push(Line::of(vec![seg(format!("{left}┌{}┐", "─".repeat(inner + 2)), border.as_str())]));
        for l in text {
            let pad = inner.saturating_sub(width_of(&l));
            let mut segs = vec![seg(format!("{left}│ "), border.as_str())];
            segs.extend(l);
            segs.push(seg(format!("{} │", " ".repeat(pad)), border.as_str()));
            out.push(Line::of(segs));
        }
        out.push(Line::of(vec![seg(format!("{left}└{}┘", "─".repeat(inner + 2)), border.as_str())]));
        let outs: Vec<&(String, String, String)> = edges.iter().filter(|e| e.0 == *id).collect();
        if outs.len() == 1 && nodes.get(k + 1).is_some_and(|n| n.0 == outs[0].1) {
            let label = if outs[0].2.is_empty() { String::new() } else { format!(" {}", outs[0].2) };
            out.push(Line::of(vec![seg(format!("{mid}│"), border.as_str()), seg(label, dim())]));
            out.push(Line::of(vec![seg(format!("{mid}▼"), border.as_str())]));
        } else {
            for (_, to, label) in outs {
                let name = nodes.iter().find(|n| n.0 == *to).map_or("", |n| n.1.as_str());
                out.push(Line::of(vec![
                    seg(format!("{left}  └─▶ "), border.as_str()),
                    seg(name, ""),
                    seg(if label.is_empty() { String::new() } else { format!(" ({label})") }, dim()),
                ]));
            }
            if k + 1 < nodes.len() {
                out.push(Line::default());
            }
        }
    }
    Some(out)
}

fn code(lang: &str, lines: &[String], width: usize, ctx: &Ctx) -> Vec<Line> {
    if lang == "mermaid"
        && let Some(m) = mermaid(lines, width)
    {
        return m;
    }
    let src: Vec<Vec<Seg>> = if ctx.hl {
        let id = if lang.is_empty() {
            None
        } else {
            highlight::language(lang).or_else(|| highlight::language(&format!("x.{lang}")))
        };
        highlight::highlight(&format!("{}\n", lines.join("\n")), id)
    } else {
        lines.iter().map(|l| vec![seg(l.as_str(), "")]).collect()
    };
    let back = ui::bg(col().block_bg);
    let inner = width.saturating_sub(2).max(1);
    let mut out: Vec<Vec<Seg>> = Vec::new();
    for segs in &src {
        for piece in chop(segs, inner) {
            let mut l = vec![seg(" ", back.as_str())];
            l.extend(piece.into_iter().map(|(t, s)| (t, format!("{back}{s}"))));
            out.push(fill(l, width, &back));
        }
    }
    // The language name at the right of the first line, when there is room.
    if !lang.is_empty()
        && let Some(first) = out.first_mut()
    {
        let head = &first[..first.len() - 1];
        if width_of(head) + ui::text_width(lang) + 2 <= width {
            let gap = width - width_of(head) - ui::text_width(lang) - 1;
            let mut l = head.to_vec();
            l.push(seg(" ".repeat(gap), back.as_str()));
            l.push(seg(lang, format!("{back}{}", dim())));
            l.push(seg(" ", back.as_str()));
            *first = l;
        }
    }
    out.into_iter().map(Line::of).collect()
}

fn block(b: &Block, width: usize, ctx: &mut Ctx, level: usize, tight: bool) -> Vec<Line> {
    let width = width.max(1);
    let p = col();
    match b {
        Block::Heading { level: h, text } => {
            let color = p.h[h - 1];
            let segs = inline(text, &format!("{BOLD}{}", ui::fg(color)), ctx, 0);
            let mut out: Vec<Line> = wrap(&segs, width).into_iter().map(Line::of).collect();
            out[0].anchor = Some(slug(&plain(&segs), ctx));
            if *h == 1 {
                out.push(Line::of(vec![seg("━".repeat(width), ui::fg(color))]));
            } else if *h == 2 {
                out.push(Line::of(vec![seg("─".repeat(width), border())]));
            }
            out
        }
        Block::Para(text) => {
            // The images of the paragraph, drawn under it when the caller can (ctx.image).
            ctx.found = ctx.image.is_some().then(Vec::new);
            let mut out: Vec<Line> = wrap(&inline(text, "", ctx, 0), width).into_iter().map(Line::of).collect();
            let found = ctx.found.take().unwrap_or_default();
            while out.last().is_some_and(|l| l.segs.is_empty()) {
                out.pop();
            }
            for url in found {
                if let Some(lines) = ctx.image.as_mut().and_then(|image| image(&url, width)) {
                    out.extend(lines);
                }
            }
            out
        }
        Block::Hr => vec![Line::of(vec![seg("─".repeat(width), border())])],
        Block::Code { lang, lines } => code(lang, lines, width, ctx),
        Block::Table { head, rows, align } => table(head, rows, align, width, ctx),
        Block::Front(lines) => {
            lines.iter().flat_map(|l| chop(&[seg(l.as_str(), dim())], width)).map(Line::of).collect()
        }
        Block::Quote(inner) => {
            static ALERT: LazyLock<Regex> = LazyLock::new(|| re(r"^\[!(\w+)\][ \t]*\n?", ""));
            let mut color = p.border;
            let mut inner = inner.clone();
            let mut alert = None;
            if let Some(Block::Para(text)) = inner.first()
                && let Some(a) = ALERT.exec(text)
            {
                let kind = a.get(1).unwrap_or("").to_lowercase();
                if let Some(&(_, title, rgb)) = p.alerts.iter().find(|(k, _, _)| *k == kind) {
                    color = rgb;
                    alert = Some(title);
                    inner[0] = Block::Para(text[a.end()..].to_string());
                }
            }
            let mut lines = blocks(&inner, width as i64 - 2, ctx, level, tight);
            if let Some(title) = alert {
                let mut head: Vec<Line> =
                    wrap(&[seg(title, format!("{BOLD}{}", ui::fg(color)))], width.saturating_sub(2))
                        .into_iter()
                        .map(Line::of)
                        .collect();
                head.extend(lines);
                lines = head;
            }
            let bar = [seg("│ ", ui::fg(color))];
            prefixed(lines, &bar, &bar)
        }
        Block::List { ordered, delim, start, items, loose } => {
            let marks: Vec<String> = (0..items.len())
                .map(|k| if *ordered { format!("{}{delim}", start + k as u64) } else { BULLETS[level % 3].to_string() })
                .collect();
            let mw = marks.iter().map(|m| ui::text_width(m)).max().unwrap_or(0);
            let mut out = Vec::new();
            for (k, it) in items.iter().enumerate() {
                let pad = mw.saturating_sub(ui::text_width(&marks[k]));
                let mark = if *ordered {
                    format!("{}{}", " ".repeat(pad), marks[k])
                } else {
                    format!("{}{}", marks[k], " ".repeat(pad))
                };
                let mut first = vec![seg(format!("{mark} "), ui::fg(p.marker))];
                if let Some(task) = it.task {
                    first.push(match task {
                        Task::Done => seg("☑ ", ui::fg(p.green)),
                        Task::Open => seg("☐ ", dim()),
                        Task::Check => seg("? ", ui::fg(p.alerts[3].2)),
                        Task::Dropped => seg("⊟ ", dim()),
                        Task::Doing => seg("◐ ", ui::fg(p.alerts[0].2)),
                    });
                }
                let indent = width_of(&first);
                let mut lines = blocks(&it.blocks, width as i64 - indent as i64, ctx, level + 1, !it.loose && !loose);
                // A dropped task's text struck through, its nested items with it.
                if it.task == Some(Task::Dropped) {
                    for (_, style) in lines.iter_mut().flat_map(|l| l.segs.iter_mut()) {
                        style.push_str(STRIKE);
                    }
                }
                if lines.is_empty() {
                    lines = vec![Line::default()];
                }
                if *loose && k > 0 {
                    out.push(Line::default());
                }
                out.extend(prefixed(lines, &first, &[seg(" ".repeat(indent), "")]));
            }
            out
        }
    }
}

fn blocks(list: &[Block], width: i64, ctx: &mut Ctx, level: usize, tight: bool) -> Vec<Line> {
    let width = width.max(1) as usize;
    let mut out = Vec::new();
    for b in list {
        let lines = block(b, width, ctx, level, tight);
        if lines.is_empty() {
            continue;
        }
        if !out.is_empty() && !tight {
            out.push(Line::default());
        }
        out.extend(lines);
    }
    out
}

// Truncates a line to the width (a last guard: blocks already fit, except in extremely narrow panes).
fn clamp(l: Line, width: usize) -> Line {
    let mut out = Vec::new();
    let mut w = 0;
    for (t, s) in &l.segs {
        let (part, pw) = ui::fit(t, width.saturating_sub(w), None);
        if !part.is_empty() {
            out.push(seg(part, s.as_str()));
        }
        w += pw;
        if part.len() < t.len() {
            break;
        }
    }
    Line { segs: out, anchor: l.anchor.filter(|a| !a.is_empty()), figure: l.figure }
}

/// Rendered lines, and the line of each heading by its slug (GitHub style: "## Some title" → "some-title"), slugs
/// that are array indexes ("1", "2024") first, in numeric order, then the others as they come.
pub struct Rendered {
    pub lines: Vec<Line>,
    pub anchors: Vec<(String, usize)>,
}

/// Draws an image: its url and the width, to the lines that show it.
pub type ImageFn<'a> = dyn FnMut(&str, usize) -> Option<Vec<Line>> + 'a;

/// How to render: code highlighted (highlight given, or not); image(url, width): the lines
/// drawing an image of the text, or None to show its name only; a line's figure is kept, whatever prefix a list or a
/// quote adds.
pub struct Options<'a> {
    pub highlight: bool,
    pub image: Option<&'a mut ImageFn<'a>>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { highlight: true, image: None }
    }
}

static FRONT: LazyLock<Regex> = LazyLock::new(|| re(r"^---\s*$", ""));
static FRONT_END: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:---|\.\.\.)\s*$", ""));
static NEWLINES: LazyLock<Regex> = LazyLock::new(|| re(r"\r\n?", "g"));

/// The lines of a Markdown text rendered for width columns (at least 1); None when its quotes and lists nest deeper
/// than MAX_NEST (the preview then shows the text plain).
pub fn render(text: &str, width: f64, opts: Options) -> Option<Rendered> {
    let width = if width.is_finite() && width.floor() >= 1.0 { width.floor() as usize } else { 1 };
    let mut ctx = Ctx {
        refs: HashMap::new(),
        slugs: HashMap::new(),
        hl: opts.highlight,
        image: opts.image,
        found: None,
        depth: 0,
        deep: false,
    };
    let mut src: Vec<String> =
        NEWLINES.replace_with(text, true, "\n").split('\n').map(|l| ui::clean_line(l, 4, None)).collect();
    let mut tree = Vec::new();
    // Front matter: a YAML block between --- lines at the very start.
    if FRONT.test(src.first().map_or("", String::as_str))
        && let Some(end) = src.iter().enumerate().position(|(k, l)| k > 0 && FRONT_END.test(l))
    {
        tree.push(Block::Front(src.drain(..=end).collect()));
    }
    tree.extend(parse(&src, &mut ctx));
    if ctx.deep {
        return None;
    }
    let lines: Vec<Line> =
        blocks(&tree, width as i64, &mut ctx, 0, false).into_iter().map(|l| clamp(l, width)).collect();
    // Array indexes ("1", "2024") first, in numeric order (json::insert).
    let mut keys: Vec<(String, crate::json::Value)> = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        if let Some(a) = l.anchor.as_ref().filter(|a| !a.is_empty())
            && !keys.iter().any(|(x, _)| x == a)
        {
            crate::json::insert(&mut keys, a.clone(), crate::json::Value::Num(k as f64));
        }
    }
    let anchors = keys.into_iter().map(|(a, k)| (a, k.as_f64().unwrap_or(0.0) as usize)).collect();
    Some(Rendered { lines, anchors })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_and_lists_nested_too_deep_give_none_instead_of_overflowing_the_stack() {
        for (unit, n) in [("> ", MAX_NEST), ("- ", MAX_NEST), (">", MAX_NEST)] {
            assert!(render(&format!("{}x", unit.repeat(n - 1)), 80.0, Options::default()).is_some(), "{unit:?}");
            assert!(render(&format!("{}x", unit.repeat(n + 1)), 80.0, Options::default()).is_none(), "{unit:?}");
        }
        assert!(render(&"> ".repeat(5000), 80.0, Options::default()).is_none());
    }

    #[test]
    fn task_statuses_have_their_own_box() {
        let text = "- [ ] open
- [x] done
- [?] check
- [-] dropped
- [/] doing
- [~] doing too
- [!] other";
        let r = render(text, 80.0, Options::default()).unwrap();
        let shown: Vec<String> =
            r.lines.iter().map(|l| l.segs.iter().map(|(t, _)| t.as_str()).collect::<String>()).collect();
        assert_eq!(
            shown,
            ["• ☐ open", "• ☑ done", "• ? check", "• ⊟ dropped", "• ◐ doing", "• ◐ doing too", "• [!] other"]
        );
        let struck = |k: usize| r.lines[k].segs.iter().skip(2).all(|(_, s)| s.contains(STRIKE));
        assert!(struck(3));
        assert!(!struck(2) && !struck(4));
    }
}
