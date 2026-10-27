//! Syntax highlighting for the file viewer: a table-driven tokenizer per language (comments, strings, keywords,
//! numbers), not a parser. Strings and comments spanning lines carry a state from one line to the next.
//! Output: one list of (text, style) segments per source line, ready for `ui::line`.

use crate::core::regex::{Match, Regex};
use crate::core::ui::{self, Rgb, Seg};
use std::collections::HashSet;
use std::sync::{LazyLock, OnceLock};

/// Token colours of VS Code Dark+ and Light+, by theme (`ui::use_theme`).
pub const COLORS_DARK: &[(&str, Rgb)] = &[
    ("keyword", [86, 156, 214]),
    ("control", [197, 134, 192]),
    ("string", [206, 145, 120]),
    ("number", [181, 206, 168]),
    ("comment", [106, 153, 85]),
    ("type", [78, 201, 176]),
    ("function", [220, 220, 170]),
    ("variable", [156, 220, 254]),
    ("constant", [79, 193, 255]),
    ("regexp", [209, 105, 105]),
    ("escape", [215, 186, 125]),
    ("tag", [86, 156, 214]),
    ("attribute", [156, 220, 254]),
    ("punctuation", [128, 128, 128]),
    ("selector", [215, 186, 125]),
    ("heading", [86, 156, 214]),
    ("added", [129, 184, 139]),
    ("removed", [244, 135, 113]),
    ("meta", [155, 155, 155]),
];
pub const COLORS_LIGHT: &[(&str, Rgb)] = &[
    ("keyword", [0, 0, 255]),
    ("control", [175, 0, 219]),
    ("string", [163, 21, 21]),
    ("number", [9, 134, 88]),
    ("comment", [0, 128, 0]),
    ("type", [38, 127, 153]),
    ("function", [121, 94, 38]),
    ("variable", [0, 16, 128]),
    ("constant", [0, 112, 193]),
    ("regexp", [129, 31, 63]),
    ("escape", [238, 0, 0]),
    ("tag", [128, 0, 0]),
    ("attribute", [229, 0, 0]),
    ("punctuation", [110, 110, 110]),
    ("selector", [128, 0, 0]),
    ("heading", [128, 0, 0]),
    ("added", [9, 134, 88]),
    ("removed", [163, 21, 21]),
    ("meta", [110, 110, 110]),
];

/// The styles of a theme by name, the colours as escapes; heading in bold, bold, italic.
pub struct Styles(Vec<(&'static str, String)>);

impl Styles {
    fn of(colors: &[(&'static str, Rgb)]) -> Styles {
        let mut all: Vec<(&'static str, String)> = colors.iter().map(|&(k, rgb)| (k, ui::fg(rgb))).collect();
        for (k, s) in &mut all {
            if *k == "heading" {
                *s = format!("\x1b[1m{s}");
            }
        }
        all.push(("bold", "\x1b[1m".into()));
        all.push(("italic", "\x1b[3m".into()));
        Styles(all)
    }

    /// The style named, None for a name that is not one.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| *k == name).map(|(_, s)| s.as_str())
    }

    /// Every style, by name, in a fixed order.
    pub fn all(&self) -> &[(&'static str, String)] {
        &self.0
    }

    fn s(&self, name: &str) -> &str {
        self.get(name).unwrap_or("")
    }
}

static DARK: LazyLock<Styles> = LazyLock::new(|| Styles::of(COLORS_DARK));
static LIGHT: LazyLock<Styles> = LazyLock::new(|| Styles::of(COLORS_LIGHT));

/// The styles of the current theme.
pub fn styles() -> &'static Styles {
    if ui::theme_name() == "light" { &LIGHT } else { &DARK }
}

// A style of the language tables: a name of the styles, else the style itself ('').
fn named<'a>(s: &'a Styles, style: &'a str) -> &'a str {
    s.get(style).unwrap_or(style)
}

const LONG: usize = 2000; // longer lines are left plain: minified files would cost too much for nothing readable

fn words(lists: &str) -> HashSet<&str> {
    lists.split(' ').filter(|w| !w.is_empty()).collect()
}

// A delimiter as a regex source.
fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if ".*+?^${}()|[]\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// --- generic tokenizer

/// line (regex sources of line comments), block ((open, close) comments), str ((open, close, flags): m
/// multi-line, r raw i.e. no escapes), kw / ctl / consts / types (word lists), rules ((regex source without capture
/// groups, style)), tried before words), ci (case-insensitive words), ident (style of other identifiers), cap_type
/// (Capitalized words are types), keys (a string followed by ':' is a key), esc_char (string escape character).
struct Spec {
    line: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    str: &'static [(&'static str, &'static str, &'static str)],
    kw: &'static str,
    ctl: &'static str,
    consts: &'static str,
    types: &'static str,
    rules: &'static [(&'static str, &'static str)],
    ci: bool,
    ident: Option<&'static str>,
    cap_type: bool,
    keys: bool,
    esc_char: char,
    word: Option<&'static str>,
}

const BASE: Spec = Spec {
    line: &[],
    block: &[],
    str: &[],
    kw: "",
    ctl: "",
    consts: "",
    types: "",
    rules: &[],
    ci: false,
    ident: None,
    cap_type: false,
    keys: false,
    esc_char: '\\',
    word: None,
};

const NUMBER: &str = r"\b(?:0[xX][\da-fA-F_]+|0[bBoO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z]*";
const WORD: &str = r"[A-Za-z_$][\w$]*";

fn sticky(src: &str) -> Regex {
    Regex::new(src, "y").expect("a valid pattern")
}

static PAREN: LazyLock<Regex> = LazyLock::new(|| sticky(r"\s*\("));
static COLON: LazyLock<Regex> = LazyLock::new(|| sticky(r"\s*:"));

/// A string or comment opened: its closing delimiter, the name of its style, its escape character, whether it goes
/// on over lines, whether it ends at the line's end, whether it can be a key.
#[derive(Debug, Clone, PartialEq)]
struct Open {
    close: String,
    style: &'static str,
    esc: Option<char>,
    multi: bool,
    line: bool,
    keys: bool,
}

#[derive(Debug, Clone)]
enum Rule {
    Open(Open),
    Style(&'static str),
    Number,
    Word,
}

struct Compiled {
    spec: &'static Spec,
    rules: Vec<Rule>,
    re: Regex,
    kw: HashSet<&'static str>,
    ctl: HashSet<&'static str>,
    consts: HashSet<&'static str>,
    types: HashSet<&'static str>,
}

fn compile(spec: &'static Spec) -> Compiled {
    let mut opens: Vec<(String, Open)> = Vec::new();
    let open = |close: &str, style, esc, multi, line, keys| Open { close: close.into(), style, esc, multi, line, keys };
    for re in spec.line {
        opens.push((re.to_string(), open("", "comment", None, false, true, false)));
    }
    for (o, c) in spec.block {
        opens.push((esc(o), open(c, "comment", None, true, false, false)));
    }
    for &(o, c, flags) in spec.str {
        let e = if flags.contains('r') { None } else { Some(spec.esc_char) };
        opens.push((esc(o), open(c, "string", e, flags.contains('m'), false, spec.keys)));
    }
    // Longer delimiters first: """ before ", --[[ before --.
    opens.sort_by_key(|(re, _)| std::cmp::Reverse(re.chars().count()));
    let mut sources: Vec<String> = Vec::new();
    let mut rules = Vec::new();
    for (re, o) in opens {
        sources.push(re);
        rules.push(Rule::Open(o));
    }
    for &(re, style) in spec.rules {
        sources.push(re.into());
        rules.push(Rule::Style(style));
    }
    sources.push(NUMBER.into());
    rules.push(Rule::Number);
    sources.push(spec.word.unwrap_or(WORD).into());
    rules.push(Rule::Word);
    let src = sources.iter().map(|r| format!("({r})")).collect::<Vec<_>>().join("|");
    Compiled {
        spec,
        rules,
        re: Regex::new(&src, if spec.ci { "gi" } else { "g" }).expect("a valid pattern"),
        kw: words(spec.kw),
        ctl: words(spec.ctl),
        consts: words(spec.consts),
        types: words(spec.types),
    }
}

// Appends a segment, merged with the previous one when the style is the same.
fn add(out: &mut Vec<Seg>, text: &str, style: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.1 == style => last.0.push_str(text),
        _ => out.push((text.to_string(), style.to_string())),
    }
}

// Scans a string or comment from `from` to its closing delimiter; gives its end, and the state still open or None.
fn close(line: &str, from: usize, o: &Open, mut add: impl FnMut(&str, &str)) -> (usize, Option<Open>) {
    let style = named(styles(), o.style);
    let found = match o.esc {
        None => line[from..].find(&o.close).map(|j| from + j),
        Some(e) => {
            let mut j = from;
            loop {
                let Some(c) = line[j..].chars().next() else { break None };
                if c == e {
                    // The escaped character with it.
                    j += c.len_utf8();
                    j += line[j..].chars().next().map_or(0, char::len_utf8);
                } else if line[j..].starts_with(&o.close) {
                    break Some(j);
                } else {
                    j += c.len_utf8();
                }
            }
        }
    };
    match found {
        None => {
            add(&line[from..], style);
            (line.len(), Some(o.clone()))
        }
        Some(j) => {
            add(&line[from..j + o.close.len()], style);
            (j + o.close.len(), None)
        }
    }
}

fn word_style<'a>(c: &Compiled, s: &'a Styles, line: &str, w: &str, start: usize, end: usize) -> &'a str {
    let lowered;
    let k = if c.spec.ci {
        lowered = w.to_lowercase();
        lowered.as_str()
    } else {
        w
    };
    if c.ctl.contains(k) {
        return s.s("control");
    }
    if c.kw.contains(k) || c.consts.contains(k) {
        return s.s("keyword");
    }
    if c.types.contains(k) {
        return s.s("type");
    }
    if PAREN.exec_at(line, end).is_some() {
        return s.s("function");
    }
    if start > 0 && line.as_bytes()[start - 1] == b'.' && c.spec.ident.is_none() {
        return s.s("variable");
    }
    if c.spec.cap_type && w.as_bytes()[0].is_ascii_uppercase() && w.bytes().any(|b| b.is_ascii_lowercase()) {
        return s.s("type");
    }
    match c.spec.ident {
        Some(ident) => named(s, ident),
        None => s.s("variable"),
    }
}

// The first rule whose group matched.
fn rule_of(m: &Match) -> usize {
    (1..m.len()).find(|&g| m.get(g).is_some()).unwrap_or(1) - 1
}

fn generic(c: &Compiled, line: &str, st: Option<State>) -> (Vec<Seg>, Option<State>) {
    let s = styles();
    let mut out = Vec::new();
    let mut i = 0;
    let mut st = match st {
        Some(State::Open(o)) => {
            let (end, open) = close(line, 0, &o, |t, s| add(&mut out, t, s));
            i = end;
            open
        }
        _ => None,
    };
    while i < line.len() {
        let Some(m) = c.re.exec_at(line, i) else {
            add(&mut out, &line[i..], "");
            break;
        };
        add(&mut out, &line[i..m.start()], "");
        let end = m.end();
        match &c.rules[rule_of(&m)] {
            Rule::Open(o) => {
                if o.line {
                    add(&mut out, &line[m.start()..], named(s, o.style));
                    break;
                }
                // A key ("name": in JSON, YAML): the string is coloured as a property once closed.
                let mut keyed = String::new();
                (i, st) = close(line, end, o, |t, _| keyed.push_str(t));
                let key = o.keys && st.is_none() && COLON.exec_at(line, i).is_some();
                add(&mut out, &format!("{}{keyed}", m.as_str()), if key { s.s("variable") } else { named(s, o.style) });
                // An unterminated single-line string stops at the end of the line.
                if st.is_some() && !o.multi {
                    st = None;
                }
                continue;
            }
            _ if end == m.start() => {
                // An empty match: never loop forever.
                let ch = line[i..].chars().next().map_or(1, char::len_utf8);
                add(&mut out, &line[i..i + ch], "");
                i += ch;
                continue;
            }
            Rule::Word => {
                let style = word_style(c, s, line, m.as_str(), m.start(), end);
                add(&mut out, m.as_str(), style);
            }
            Rule::Style(style) => add(&mut out, m.as_str(), named(s, style)),
            Rule::Number => add(&mut out, m.as_str(), s.s("number")),
        }
        i = end;
    }
    (out, st.map(State::Open))
}

// --- line-based languages

fn re(src: &str, flags: &str) -> Regex {
    Regex::new(src, flags).expect("a valid pattern")
}

static DIFF_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(diff |index |--- |\+\+\+ |new file|deleted file|old mode|new mode|similarity|rename |Binary )", "")
});

fn diff(line: &str) -> (Vec<Seg>, Option<State>) {
    let s = styles();
    let style = if DIFF_HEAD.test(line) {
        s.s("bold")
    } else if line.starts_with("@@") {
        s.s("keyword")
    } else if line.starts_with('+') {
        s.s("added")
    } else if line.starts_with('-') {
        s.s("removed")
    } else if line.starts_with('\\') {
        s.s("comment")
    } else {
        ""
    };
    (vec![(line.into(), style.into())], None)
}

/// The state of a Markdown line: in an HTML comment, or in a code fence (its mark, the language highlighting it, and
/// that language's state).
#[derive(Debug, Clone, PartialEq, Default)]
struct Md {
    comment: bool,
    fence: Option<String>,
    lang: Option<&'static str>,
    sub: Option<Box<State>>,
}

static MD_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    let parts = [
        r"(`+)[^`]*\1",
        r"\*\*[^*]+\*\*|__[^_]+__|\*[^*\s][^*]*\*",
        r"!?\[[^\]]*\]\([^)\s]*\)",
        r"<https?:\/\/[^>\s]*>",
        r"<!--.*?(?:-->|$)",
        r"<\/?[A-Za-z][^>]*>",
    ];
    re(&parts.join("|"), "g")
});
static MD_FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(`{3,}|~{3,})\s*([\w+#.-]*)", ""));
static MD_HEADING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{0,3}#{1,6}(\s|$)", ""));
static MD_RULE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{0,3}([-*_])(\s*\1){2,}\s*$", ""));
static MD_QUOTE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:>\s?)+", ""));
static MD_ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:[-*+]|\d+[.)])(?:\s+\[[ xX]\])?(?=\s|$)", ""));

// Markdown source: block markers by line, a few inline spans, code fences highlighted in their language.
fn markdown(line: &str, st: Option<State>) -> (Vec<Seg>, Option<State>) {
    let s = styles();
    let mut st = match st {
        Some(State::Md(md)) => md,
        _ => Md::default(),
    };
    let mut out = Vec::new();
    let mut line = line;
    if st.comment {
        let Some(j) = line.find("-->") else {
            return (vec![(line.into(), s.s("comment").into())], Some(State::Md(st)));
        };
        add(&mut out, &line[..j + 3], s.s("comment"));
        line = &line[j + 3..];
        st = Md::default();
    }
    let fence = MD_FENCE.exec(line);
    if let Some(mark) = &st.fence {
        let closing = fence.as_ref().is_some_and(|f| {
            let f1 = f.get(1).unwrap_or("");
            f1.as_bytes()[0] == mark.as_bytes()[0] && f1.len() >= mark.len() && f.get(2).unwrap_or("").is_empty()
        });
        if closing {
            return (vec![(line.into(), s.s("string").into())], Some(State::Md(Md::default())));
        }
        let Some(lang) = st.lang else { return (vec![(line.into(), String::new())], Some(State::Md(st))) };
        let (segs, sub) = line_fn(lang, line, st.sub.take().map(|b| *b));
        st.sub = sub.map(Box::new);
        return (segs, Some(State::Md(st)));
    }
    if let Some(f) = fence {
        let name = f.get(2).unwrap_or("");
        let id = language(name).or_else(|| language(&format!("x.{name}")));
        let md = Md { fence: Some(f.get(1).unwrap_or("").into()), lang: id, ..Md::default() };
        return (vec![(line.into(), s.s("string").into())], Some(State::Md(md)));
    }
    if MD_HEADING.test(line) {
        return (vec![(line.into(), s.s("heading").into())], Some(State::Md(st)));
    }
    if MD_RULE.test(line) {
        return (vec![(line.into(), s.s("comment").into())], Some(State::Md(st)));
    }
    if let Some(m) = MD_QUOTE.exec(line).or_else(|| MD_ITEM.exec(line)) {
        let t = m.as_str();
        add(&mut out, t, if t.contains('>') { s.s("comment") } else { s.s("keyword") });
        line = &line[t.len()..];
    }
    let mut i = 0;
    for m in MD_INLINE.matches(line) {
        add(&mut out, &line[i..m.start()], "");
        let t = m.as_str();
        if t.starts_with("<!--") {
            add(&mut out, t, s.s("comment"));
            if !t.ends_with("-->") {
                st = Md { comment: true, ..Md::default() };
            }
        } else if t.starts_with('`') {
            add(&mut out, t, s.s("string"));
        } else if t.starts_with("**") || t.starts_with("__") {
            add(&mut out, t, s.s("bold"));
        } else if t.starts_with('*') {
            add(&mut out, t, s.s("italic"));
        } else if t.starts_with('<') && !t.starts_with("<http") {
            add(&mut out, t, s.s("tag"));
        } else if t.starts_with('<') {
            add(&mut out, t, s.s("string"));
        } else {
            let k = t.find("](").map_or(0, |k| k + 1);
            add(&mut out, &t[..k], s.s("string"));
            add(&mut out, &t[k..], s.s("keyword"));
        }
        i = m.end();
    }
    add(&mut out, &line[i..], "");
    (out, Some(State::Md(st)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Text,
    Comment,
    Cdata,
    Script,
    Style,
    Tag,
}

/// The state of an HTML line: what it is in (text, a comment, CDATA, a script or a style, a tag), the tag's name,
/// whether it is a closing tag, the quote of an attribute value open, the state of the script's or style's lines.
#[derive(Debug, Clone, PartialEq)]
struct Html {
    mode: Mode,
    tag: String,
    closing: bool,
    quote: Option<char>,
    sub: Option<Box<State>>,
}

impl Html {
    fn mode(mode: Mode) -> Html {
        Html { mode, tag: String::new(), closing: false, quote: None, sub: None }
    }
}

static TAG_TOKEN: LazyLock<Regex> = LazyLock::new(|| sticky(r#"\s+|[/?]?>|[^\s=>/"']+|=|["']|."#));
static OPEN_TAG: LazyLock<Regex> = LazyLock::new(|| sticky(r"<(\/?)([A-Za-z][\w:.-]*)"));
static DECL: LazyLock<Regex> = LazyLock::new(|| sticky(r"<[!?][\w-]*"));
static ENTITY: LazyLock<Regex> = LazyLock::new(|| re(r"&(?:#\d+|#x[\da-fA-F]+|\w+);", "g"));
static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| re(r"^[^\s=]", ""));

/// Where `needle` (ASCII, lower case) starts in the text from `from`, ignoring the ASCII case.
pub(crate) fn find_lower(text: &str, needle: &str, from: usize) -> Option<usize> {
    let (t, n) = (text.as_bytes(), needle.as_bytes());
    (from..=t.len().checked_sub(n.len())?).find(|&j| t[j..j + n.len()].eq_ignore_ascii_case(n))
}

// HTML / XML / Vue: tags and attributes, comments, CDATA; <script> and <style> contents in JavaScript / CSS.
fn html(line: &str, st: Option<State>) -> (Vec<Seg>, Option<State>) {
    let s = styles();
    let mut st = match st {
        Some(State::Html(h)) => h,
        _ => Html::mode(Mode::Text),
    };
    let mut out = Vec::new();
    let n = line.len();
    let mut i = 0;
    while i < n {
        match st.mode {
            Mode::Comment | Mode::Cdata => {
                let end_tok = if st.mode == Mode::Comment { "-->" } else { "]]>" };
                let j = line[i..].find(end_tok).map(|j| i + j);
                let e = j.map_or(n, |j| j + end_tok.len());
                add(&mut out, &line[i..e], if st.mode == Mode::Comment { s.s("comment") } else { s.s("string") });
                i = e;
                if j.is_some() {
                    st = Html::mode(Mode::Text);
                }
                continue;
            }
            Mode::Script | Mode::Style => {
                let (close, lang) = if st.mode == Mode::Script { ("</script", "js") } else { ("</style", "css") };
                let j = find_lower(line, close, i);
                let e = j.unwrap_or(n);
                if e > i {
                    let (segs, sub) = line_fn(lang, &line[i..e], st.sub.take().map(|b| *b));
                    for (t, style) in &segs {
                        add(&mut out, t, style);
                    }
                    st.sub = sub.map(Box::new);
                }
                i = e;
                if j.is_some() {
                    st = Html::mode(Mode::Text);
                }
                continue;
            }
            Mode::Tag => {
                if let Some(q) = st.quote {
                    let j = line[i..].find(q).map(|j| i + j);
                    let e = j.map_or(n, |j| j + 1);
                    add(&mut out, &line[i..e], s.s("string"));
                    i = e;
                    if j.is_some() {
                        st.quote = None;
                    }
                    continue;
                }
                // Never empty: '.' and \s+ take any character.
                let Some(t) = TAG_TOKEN.exec_at(line, i).map(|m| m.as_str()) else {
                    add(&mut out, &line[i..], "");
                    break;
                };
                if t == "\"" || t == "'" {
                    add(&mut out, t, s.s("string"));
                    st.quote = t.chars().next();
                } else if t.ends_with('>') {
                    add(&mut out, t, s.s("punctuation"));
                    let raw = !st.closing && t == ">" && (st.tag == "script" || st.tag == "style");
                    st = Html::mode(if !raw {
                        Mode::Text
                    } else if st.tag == "script" {
                        Mode::Script
                    } else {
                        Mode::Style
                    });
                } else {
                    add(&mut out, t, if ATTRIBUTE.test(t) { s.s("attribute") } else { "" });
                }
                i += t.len();
                continue;
            }
            Mode::Text => {}
        }
        // Text: entities, then the next tag.
        let j = line[i..].find('<').map(|j| i + j);
        let e = j.unwrap_or(n);
        let text = &line[i..e];
        let mut k = 0;
        for m in ENTITY.matches(text) {
            add(&mut out, &text[k..m.start()], "");
            add(&mut out, m.as_str(), s.s("constant"));
            k = m.end();
        }
        add(&mut out, &text[k..], "");
        i = e;
        if j.is_none() {
            break;
        }
        if line[i..].starts_with("<!--") {
            add(&mut out, "<!--", s.s("comment"));
            i += 4;
            st = Html::mode(Mode::Comment);
        } else if line[i..].starts_with("<![CDATA[") {
            add(&mut out, "<![CDATA[", s.s("string"));
            i += 9;
            st = Html::mode(Mode::Cdata);
        } else if let Some(m) = OPEN_TAG.exec_at(line, i) {
            let (slash, name) = (m.get(1).unwrap_or(""), m.get(2).unwrap_or(""));
            add(&mut out, &format!("<{slash}"), s.s("punctuation"));
            add(&mut out, name, s.s("tag"));
            i += m.as_str().len();
            st = Html { tag: name.to_lowercase(), closing: !slash.is_empty(), ..Html::mode(Mode::Tag) };
        } else if let Some(m) = DECL.exec_at(line, i) {
            add(&mut out, m.as_str(), s.s("keyword"));
            i += m.as_str().len();
            st = Html { closing: true, ..Html::mode(Mode::Tag) };
        } else {
            add(&mut out, "<", "");
            i += 1;
        }
    }
    (out, Some(State::Html(st)))
}

/// The state of a CSS line: the depth of braces, in a value, at the start of a declaration, in a comment, the quote
/// of a string open, after a property name.
#[derive(Debug, Clone, PartialEq)]
struct Css {
    depth: u32,
    value: bool,
    start: bool,
    comment: bool,
    quote: Option<char>,
    prop: bool,
}

static CSS_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    let parts = [
        r"\/\*",
        r"(?<![:\w(])\/\/",
        r#"["']"#,
        r"@[\w-]+",
        r"#[\da-fA-F]{3,8}\b",
        r"-?(?:\d+\.?\d*|\.\d+)(?:[a-zA-Z%]+)?",
        "!important",
        r"[\w-]+",
        "[{};:]",
        r"\s+",
        ".",
    ];
    re(&parts.join("|"), "gy")
});
static CSS_NUMBER: LazyLock<Regex> = LazyLock::new(|| re(r"^[-.\d#]", ""));
static CSS_DIGIT: LazyLock<Regex> = LazyLock::new(|| re(r"\d|#", ""));
static CSS_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"^[\w-]", ""));
static CSS_SELECTOR: LazyLock<Regex> = LazyLock::new(|| re(r"^#|^-?\d", ""));
static CSS_SPACE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s", ""));
static CSS_PUNCT: LazyLock<Regex> = LazyLock::new(|| re(r"^[{};\s]", ""));

// A ':' after white space, within 40 characters.
fn colon_after(line: &str, end: usize) -> bool {
    for c in line[end..].chars().take(40) {
        if c == ':' {
            return true;
        }
        if !crate::unicode::is_space(c) {
            return false;
        }
    }
    false
}

// CSS / SCSS / Less: selectors, properties and values told apart by braces, ':' and ';'.
fn css(line: &str, st: Option<State>) -> (Vec<Seg>, Option<State>) {
    let s = styles();
    let mut st = match st {
        Some(State::Css(c)) => c,
        _ => Css { depth: 0, value: false, start: true, comment: false, quote: None, prop: false },
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if st.comment || st.quote.is_some() {
            let end_tok = if st.comment { "*/".to_string() } else { st.quote.map(String::from).unwrap_or_default() };
            let j = line[i..].find(&end_tok).map(|j| i + j);
            let e = j.map_or(line.len(), |j| j + end_tok.len());
            add(&mut out, &line[i..e], if st.comment { s.s("comment") } else { s.s("string") });
            i = e;
            if j.is_some() {
                st.comment = false;
                st.quote = None;
            }
            continue;
        }
        let t = CSS_TOKEN.exec_at(line, i).map_or("", |m| m.as_str());
        if t.is_empty() {
            // Nothing matches (never: '.' and \s+ take any character): the rest as it is.
            add(&mut out, &line[i..], "");
            break;
        }
        let end = i + t.len();
        let next = line[end..].chars().next();
        let mut style = "";
        if t == "/*" {
            st.comment = true;
            style = s.s("comment");
        } else if t == "//" {
            add(&mut out, &line[i..], s.s("comment"));
            break;
        } else if t == "\"" || t == "'" {
            st.quote = t.chars().next();
            style = s.s("string");
        } else if t.starts_with('@') {
            style = s.s("control");
            st.start = false;
        } else if t == "{" {
            st.depth += 1;
            st.value = false;
            st.start = true;
        } else if t == "}" {
            st.depth = st.depth.saturating_sub(1);
            st.value = false;
            st.start = true;
        } else if t == ";" {
            st.value = false;
            st.start = true;
        } else if t == ":" && st.depth > 0 && st.prop {
            st.value = true;
            st.prop = false;
        } else if CSS_SPACE.test(t) {
            add(&mut out, t, "");
            i = end;
            continue;
        } else if st.value {
            style = if t == "!important" {
                s.s("keyword")
            } else if CSS_NUMBER.test(t) && CSS_DIGIT.test(t) {
                s.s("number")
            } else if next == Some('(') {
                s.s("function")
            } else if CSS_WORD.test(t) {
                s.s("string")
            } else {
                ""
            };
        } else if CSS_WORD.test(t)
            && st.depth > 0
            && st.start
            && colon_after(line, end)
            && (!line[end..].contains('{') || line[end..].contains(';'))
        {
            style = s.s("variable");
            st.prop = true;
        } else if CSS_WORD.test(t) || t == "#" || t == "." {
            style = if next == Some('(') { s.s("function") } else { s.s("selector") };
        } else if CSS_SELECTOR.test(t) {
            style = s.s("selector");
        }
        if !CSS_PUNCT.test(t) && t != ":" {
            st.start = false;
        }
        add(&mut out, t, style);
        i = end;
    }
    (out, Some(State::Css(st)))
}

// --- languages

/// The state a language carries from one line to the next.
#[derive(Debug, Clone, PartialEq)]
enum State {
    Open(Open),
    Md(Md),
    Html(Html),
    Css(Css),
}

const FNS: &[&str] = &["diff", "markdown", "html", "xml", "css"];

static COMPILED: [OnceLock<Compiled>; SPECS.len()] = [const { OnceLock::new() }; SPECS.len()];

// Whether a language id has a line function.
fn known(id: &str) -> bool {
    FNS.contains(&id) || SPECS.iter().any(|(k, _)| *k == id)
}

// The segments of a line in the language id (one it knows), and the state it leaves for the next line.
fn line_fn(id: &str, line: &str, st: Option<State>) -> (Vec<Seg>, Option<State>) {
    match id {
        "diff" => diff(line),
        "markdown" => markdown(line, st),
        "html" | "xml" => html(line, st),
        "css" => css(line, st),
        _ => match SPECS.iter().position(|(k, _)| *k == id) {
            Some(k) => generic(COMPILED[k].get_or_init(|| compile(SPECS[k].1)), line, st),
            None => (vec![(line.into(), String::new())], None),
        },
    }
}

fn lookup(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// The language id of a file name (or of a Markdown fence name such as "javascript"), None when unknown.
pub fn language(file_name: &str) -> Option<&'static str> {
    if file_name.is_empty() {
        return None;
    }
    let base = file_name.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
    if let Some(id) = lookup(NAMES, &base) {
        return Some(id);
    }
    if base.starts_with(".env.") || base.starts_with("dockerfile.") {
        return Some(if base.as_bytes()[1] == b'e' { "ini" } else { "dockerfile" });
    }
    match base.rfind('.') {
        None => lookup(ALIAS, &base),
        Some(dot) => lookup(EXT, &base[dot + 1..]),
    }
}

/// The segments of each line of text (a final newline adds no empty line). lang: an id from `language`, None for
/// none.
pub fn highlight(text: &str, lang: Option<&str>) -> Vec<Vec<Seg>> {
    let mut raw: Vec<&str> = text.split('\n').collect();
    if raw.len() > 1 && raw.last() == Some(&"") {
        raw.pop();
    }
    let lang = lang.filter(|id| known(id));
    let mut st = None;
    raw.into_iter()
        .map(|l| {
            let line = ui::clean_line(l, 4, None);
            let Some(id) = lang else { return vec![(line, String::new())] };
            if line.chars().count() > LONG {
                return vec![(line, String::new())];
            }
            let segs;
            (segs, st) = line_fn(id, &line, st.take());
            if segs.is_empty() { vec![(String::new(), String::new())] } else { segs }
        })
        .collect()
}

// --- language tables

const JS_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", ""), ("'", "'", ""), ("`", "`", "m")],
    kw: "var let const function class extends new delete typeof instanceof in of void this super async static get set debugger with as interface type enum namespace declare readonly abstract implements private protected public keyof infer is asserts satisfies override module require",
    ctl: "if else for while do switch case default break continue return goto try catch throw finally import export from default await yield",
    consts: "true false null undefined NaN Infinity",
    types: "string number boolean any unknown never object symbol bigint",
    rules: &[
        (
            r"\/(?![*/])(?<=(?:^|[(,=:\[!&|?{};+\-*%<>~^]|\breturn|\btypeof)\s*\/)(?:\\.|\[(?:\\.|[^\]\\])*\]|[^/\\[\n])+\/[dgimsuyv]*",
            "regexp",
        ),
        (r"@[\w.]+", "function"),
        (r"#[A-Za-z_]\w*", "variable"),
    ],
    cap_type: true,
    ..BASE
};
const JSON_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", "")],
    consts: "true false null",
    ident: Some(""),
    keys: true,
    ..BASE
};
const PYTHON_: Spec = Spec {
    line: &["#"],
    str: &[("\"\"\"", "\"\"\"", "m"), ("'''", "'''", "m"), ("\"", "\"", ""), ("'", "'", "")],
    kw: "def class lambda and or not in is global nonlocal pass del async None True False",
    ctl: "if elif else for while break continue return try except finally raise with as import from yield assert match case await",
    types: "int float str bool list dict set tuple bytes object type",
    rules: &[(r"@[\w.]+", "function"), (r#"\b[rRbBfFuU]{1,2}(?=["'])"#, "keyword")],
    cap_type: true,
    ..BASE
};
const SHELL_: Spec = Spec {
    line: &[r"(?<![^\s;|&(])#"],
    str: &[("\"", "\"", "m"), ("'", "'", "mr")],
    kw: "function local export declare readonly typeset unset alias source set shift trap eval exec end begin not and or",
    ctl: "if then else elif fi for while until do done case esac in select return exit break continue time switch",
    rules: &[(r"\$\{[^}\s]*\}|\$(?:[A-Za-z_]\w*|[\d@#?$!*-])", "variable"), (r"(?<=\s|^)--?[\w-]+", "constant")],
    ident: Some(""),
    word: Some(r"[A-Za-z_][\w.-]*"),
    ..BASE
};
const POWERSHELL_: Spec = Spec {
    line: &["#"],
    block: &[("<#", "#>")],
    str: &[("@\"", "\"@", "m"), ("@'", "'@", "mr"), ("\"", "\"", ""), ("'", "'", "r")],
    kw: "function param begin process end filter class enum using module dynamicparam",
    ctl: "if elseif else switch foreach for while do until break continue return try catch finally throw trap exit in",
    consts: "$true $false $null",
    rules: &[
        (r"\$(?:true|false|null)\b", "keyword"),
        (r"\$[\w:]+|\$\{[^}]*\}", "variable"),
        (r"\b[A-Za-z]+-[A-Za-z]\w*", "function"),
        (
            r"-(?:eq|ne|gt|ge|lt|le|like|notlike|match|notmatch|contains|notcontains|in|notin|and|or|not|xor|is|isnot|as|replace|split|join|f)\b",
            "keyword",
        ),
        (r"\[[\w.]+\]", "type"),
    ],
    ci: true,
    ident: Some(""),
    esc_char: '`',
    ..BASE
};
const RUST_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", "m")],
    kw: "fn let mut const static struct enum trait impl type mod pub use crate self Self super where as ref move unsafe extern dyn async",
    ctl: "if else match loop while for in return break continue yield await",
    consts: "true false None Some Ok Err",
    types: "i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str String Vec Option Result Box",
    rules: &[
        (r"b?'(?:\\.|[^\\'])'", "string"),
        (r"'[A-Za-z_]\w*", "keyword"),
        (r"#!?\[[^\]]*\]", "meta"),
        (r"\w+!", "function"),
    ],
    cap_type: true,
    ..BASE
};
const GO_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", ""), ("'", "'", ""), ("`", "`", "mr")],
    kw: "package import func var const type struct interface map chan go defer select",
    ctl: "if else for range switch case default break continue return goto fallthrough",
    consts: "true false nil iota",
    types: "int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 uintptr float32 float64 complex64 complex128 string bool byte rune error any",
    ..BASE
};
const C_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", ""), ("'", "'", "")],
    kw: "int char float double void long short signed unsigned struct union enum typedef static extern const volatile register inline auto sizeof bool",
    ctl: "if else for while do switch case default break continue return goto try catch throw finally",
    consts: "true false NULL nullptr",
    types: "size_t ssize_t ptrdiff_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t FILE",
    rules: &[(r"^\s*#\s*\w+", "control"), (r"(?<=#\s*include\s*)<[^>]*>", "string")],
    cap_type: true,
    ..BASE
};
const CPP_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", ""), ("'", "'", "")],
    kw: "int char float double void long short signed unsigned struct union enum typedef static extern const volatile register inline auto sizeof bool class namespace template typename public private protected virtual override final friend operator new delete this using constexpr consteval noexcept explicit mutable decltype static_cast dynamic_cast reinterpret_cast const_cast concept requires co_await co_return co_yield",
    ctl: "if else for while do switch case default break continue return goto try catch throw finally",
    consts: "true false NULL nullptr",
    types: "size_t ssize_t ptrdiff_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t FILE std string vector map unique_ptr shared_ptr",
    rules: &[(r"^\s*#\s*\w+", "control"), (r"(?<=#\s*include\s*)<[^>]*>", "string")],
    cap_type: true,
    ..BASE
};
const CSHARP_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("@\"", "\"", "mr"), ("\"\"\"", "\"\"\"", "m"), ("\"", "\"", ""), ("'", "'", "")],
    kw: "using namespace class struct interface enum record public private protected internal static readonly const void var new this base override virtual abstract sealed async partial get set init value int string bool double float decimal long short byte char object dynamic out ref in params is as typeof nameof delegate event operator implicit explicit unsafe fixed lock checked unchecked where required",
    ctl: "if else for while do switch case default break continue return goto try catch throw finally foreach yield await when",
    consts: "true false null",
    rules: &[(r"^\s*#\s*\w+", "control")],
    cap_type: true,
    ..BASE
};
const JAVA_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"\"\"", "\"\"\"", "m"), ("\"", "\"", ""), ("'", "'", "")],
    kw: "package import class interface enum record extends implements public private protected static final abstract native synchronized transient volatile strictfp new this super void int long short byte char boolean float double var instanceof throws assert permits sealed",
    ctl: "if else for while do switch case default break continue return goto try catch throw finally yield",
    consts: "true false null",
    rules: &[(r"@\w+", "type")],
    cap_type: true,
    ..BASE
};
const KOTLIN_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"\"\"", "\"\"\"", "mr"), ("\"", "\"", ""), ("'", "'", "")],
    kw: "package import class interface object fun val var typealias constructor init companion data sealed enum annotation open override abstract final private protected public internal inline suspend operator infix lateinit const vararg reified this super is as in out by get set where",
    ctl: "if else when for while do break continue return throw try catch finally",
    consts: "true false null",
    rules: &[(r"@\w+", "type")],
    cap_type: true,
    ..BASE
};
const SWIFT_: Spec = Spec {
    line: &["//"],
    block: &[("/*", "*/")],
    str: &[("\"\"\"", "\"\"\"", "m"), ("\"", "\"", "")],
    kw: "import class struct enum protocol extension func var let init deinit subscript typealias associatedtype static public private fileprivate internal open final override mutating nonmutating lazy weak unowned inout some any self Self super is as where in throws rethrows async convenience required indirect operator",
    ctl: "if else guard switch case default for while repeat break continue return throw do try catch defer fallthrough await",
    consts: "true false nil",
    rules: &[(r"@\w+", "type")],
    cap_type: true,
    ..BASE
};
const PHP_: Spec = Spec {
    line: &["//", r"#(?!\[)"],
    block: &[("/*", "*/")],
    str: &[("\"", "\"", "m"), ("'", "'", "m")],
    kw: "function class interface trait extends implements namespace use public private protected static final abstract const new clone instanceof echo print var global fn readonly enum",
    ctl: "if else elseif endif for foreach endforeach while endwhile do switch case default break continue return throw try catch finally yield include require include_once require_once as match",
    consts: "true false null",
    rules: &[(r"<\?(?:php|=)?|\?>", "meta"), (r"\$\w+", "variable")],
    ci: true,
    cap_type: true,
    ..BASE
};
const RUBY_: Spec = Spec {
    line: &["#"],
    block: &[("=begin", "=end")],
    str: &[("\"", "\"", "m"), ("'", "'", "m"), ("`", "`", "m")],
    kw: "def class module self nil true false and or not alias undef super lambda proc attr_reader attr_accessor attr_writer require require_relative include extend private protected public",
    ctl: "if elsif else unless case when while until for in do end begin rescue ensure raise return break next redo retry yield then",
    rules: &[(r"(?<![:\w]):[A-Za-z_]\w*[?!]?", "constant"), (r"@@?\w+|\$\w+", "variable")],
    cap_type: true,
    word: Some(r"[A-Za-z_]\w*[?!]?"),
    ..BASE
};
const LUA_: Spec = Spec {
    line: &["--"],
    block: &[("--[[", "]]")],
    str: &[("\"", "\"", ""), ("'", "'", ""), ("[[", "]]", "mr")],
    kw: "local function and or not nil true false self",
    ctl: "if then else elseif end for in while do repeat until return break goto",
    ..BASE
};
const SQL_: Spec = Spec {
    line: &["--"],
    block: &[("/*", "*/")],
    str: &[("'", "'", "mr"), ("\"", "\"", ""), ("`", "`", "r")],
    kw: "select from where insert into values update set delete create table drop alter add column index view join inner left right outer full cross on as and or not null is in exists between like ilike limit offset order by group having distinct union all intersect except case when then else end primary key foreign references unique default check constraint begin commit rollback transaction with recursive returning if replace trigger function procedure declare cast asc desc database schema grant revoke int integer varchar text boolean date time timestamp serial bigint smallint real numeric decimal char float double true false over partition window language returns",
    ci: true,
    ident: Some(""),
    ..BASE
};
const YAML_: Spec = Spec {
    line: &[r"(?<!\S)#"],
    str: &[("\"", "\"", ""), ("'", "'", "r")],
    consts: "true false null yes no on off",
    rules: &[
        (r#"^\s*(?:-\s+)*[^\s#:'"\-][^:#]*(?=:(?:\s|$))"#, "keyword"),
        (r"^(?:---|\.\.\.)(?=\s|$)", "control"),
        (r"[&*][\w-]+", "type"),
        (r"!!?[\w-]*", "type"),
        (r"^\s*-(?=\s|$)", ""),
        (r"[|>][+-]?(?=\s*$)", "control"),
    ],
    ci: true,
    ident: Some("string"),
    keys: true,
    word: Some(r"[A-Za-z_~][\w.-]*"),
    ..BASE
};
const INI_: Spec = Spec {
    line: &[r"(?<!\S)[#;]"],
    str: &[("\"\"\"", "\"\"\"", "m"), ("'''", "'''", "mr"), ("\"", "\"", ""), ("'", "'", "r")],
    consts: "true false",
    rules: &[
        (r"^\s*\[[^\]]*\]\]?", "type"),
        (r"^\s*(?:export\s+)?[\w.\-]+(?=\s*[=:])", "variable"),
        (r"\$\{[^}\s]*\}", "variable"),
    ],
    ident: Some(""),
    ..BASE
};
const DOCKERFILE_: Spec = Spec {
    line: &[r"^\s*#"],
    str: &[("\"", "\"", ""), ("'", "'", "")],
    rules: &[
        (
            r"^\s*(?:from|run|cmd|label|maintainer|expose|env|add|copy|entrypoint|volume|user|workdir|arg|onbuild|stopsignal|healthcheck|shell)\b",
            "keyword",
        ),
        (r"\bas\b(?=\s+\w+\s*$)", "keyword"),
        (r"\$\{[^}\s]*\}|\$\w+", "variable"),
        (r"(?<=\s)--[\w-]+", "constant"),
    ],
    ci: true,
    ident: Some(""),
    ..BASE
};
const MAKEFILE_: Spec = Spec {
    line: &["#"],
    str: &[("\"", "\"", ""), ("'", "'", "")],
    ctl: "ifeq ifneq ifdef ifndef else endif include define endef export override unexport vpath",
    rules: &[
        (r"\$\([^)\s]*\)|\$\{[^}\s]*\}|\$[@<^+?*%$]", "variable"),
        (r"^[\w.\-]+(?=\s*[:+?!]?=)", "variable"),
        (r"^[^\s:#=$][^:#=]*(?=:(?!=))", "function"),
    ],
    ident: Some(""),
    word: Some(r"[A-Za-z_][\w-]*"),
    ..BASE
};

const SPECS: &[(&str, &Spec)] = &[
    ("js", &JS_),
    ("ts", &JS_),
    ("json", &JSON_),
    ("python", &PYTHON_),
    ("shell", &SHELL_),
    ("powershell", &POWERSHELL_),
    ("rust", &RUST_),
    ("go", &GO_),
    ("c", &C_),
    ("cpp", &CPP_),
    ("csharp", &CSHARP_),
    ("java", &JAVA_),
    ("kotlin", &KOTLIN_),
    ("swift", &SWIFT_),
    ("php", &PHP_),
    ("ruby", &RUBY_),
    ("lua", &LUA_),
    ("sql", &SQL_),
    ("yaml", &YAML_),
    ("ini", &INI_),
    ("dockerfile", &DOCKERFILE_),
    ("makefile", &MAKEFILE_),
];

const EXT: &[(&str, &str)] = &[
    ("js", "js"),
    ("mjs", "js"),
    ("cjs", "js"),
    ("jsx", "js"),
    ("qml", "js"),
    ("ts", "ts"),
    ("tsx", "ts"),
    ("mts", "ts"),
    ("cts", "ts"),
    ("json", "json"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("webmanifest", "json"),
    ("code-workspace", "json"),
    ("py", "python"),
    ("pyw", "python"),
    ("pyi", "python"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("fish", "shell"),
    ("ksh", "shell"),
    ("ps1", "powershell"),
    ("psm1", "powershell"),
    ("psd1", "powershell"),
    ("rs", "rust"),
    ("go", "go"),
    ("c", "c"),
    ("h", "c"),
    ("cpp", "cpp"),
    ("cc", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
    ("hxx", "cpp"),
    ("ino", "cpp"),
    ("cs", "csharp"),
    ("csx", "csharp"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("swift", "swift"),
    ("php", "php"),
    ("phtml", "php"),
    ("rb", "ruby"),
    ("rake", "ruby"),
    ("gemspec", "ruby"),
    ("lua", "lua"),
    ("sql", "sql"),
    ("html", "html"),
    ("htm", "html"),
    ("xhtml", "html"),
    ("vue", "html"),
    ("svelte", "html"),
    ("xml", "xml"),
    ("svg", "xml"),
    ("xsl", "xml"),
    ("xslt", "xml"),
    ("plist", "xml"),
    ("csproj", "xml"),
    ("fsproj", "xml"),
    ("vbproj", "xml"),
    ("props", "xml"),
    ("targets", "xml"),
    ("xaml", "xml"),
    ("css", "css"),
    ("scss", "css"),
    ("sass", "css"),
    ("less", "css"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("toml", "ini"),
    ("ini", "ini"),
    ("cfg", "ini"),
    ("conf", "ini"),
    ("env", "ini"),
    ("properties", "ini"),
    ("service", "ini"),
    ("desktop", "ini"),
    ("editorconfig", "ini"),
    ("gitconfig", "ini"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("mdx", "markdown"),
    ("diff", "diff"),
    ("patch", "diff"),
    ("dockerfile", "dockerfile"),
    ("containerfile", "dockerfile"),
    ("mk", "makefile"),
    ("mak", "makefile"),
];
const NAMES: &[(&str, &str)] = &[
    ("dockerfile", "dockerfile"),
    ("containerfile", "dockerfile"),
    ("makefile", "makefile"),
    ("gnumakefile", "makefile"),
    (".bashrc", "shell"),
    (".zshrc", "shell"),
    (".profile", "shell"),
    (".bash_profile", "shell"),
    (".bash_aliases", "shell"),
    (".bash_logout", "shell"),
    (".zprofile", "shell"),
    (".zshenv", "shell"),
    (".envrc", "shell"),
    ("pkgbuild", "shell"),
    (".env", "ini"),
    (".gitconfig", "ini"),
    (".gitmodules", "ini"),
    (".editorconfig", "ini"),
    (".npmrc", "ini"),
    (".gitignore", "ini"),
    (".dockerignore", "ini"),
    (".gitattributes", "ini"),
    ("gemfile", "ruby"),
    ("rakefile", "ruby"),
    ("cmakelists.txt", "makefile"),
    ("jenkinsfile", "java"),
    ("vagrantfile", "ruby"),
];
const ALIAS: &[(&str, &str)] = &[
    ("javascript", "js"),
    ("node", "js"),
    ("typescript", "ts"),
    ("python", "python"),
    ("python3", "python"),
    ("bash", "shell"),
    ("shell", "shell"),
    ("console", "shell"),
    ("shellsession", "shell"),
    ("zsh", "shell"),
    ("sh", "shell"),
    ("pwsh", "powershell"),
    ("powershell", "powershell"),
    ("ps", "powershell"),
    ("rust", "rust"),
    ("golang", "go"),
    ("go", "go"),
    ("c", "c"),
    ("c++", "cpp"),
    ("cpp", "cpp"),
    ("csharp", "csharp"),
    ("c#", "csharp"),
    ("java", "java"),
    ("kotlin", "kotlin"),
    ("swift", "swift"),
    ("php", "php"),
    ("ruby", "ruby"),
    ("lua", "lua"),
    ("sql", "sql"),
    ("html", "html"),
    ("xml", "xml"),
    ("vue", "html"),
    ("svg", "xml"),
    ("css", "css"),
    ("scss", "css"),
    ("yaml", "yaml"),
    ("toml", "ini"),
    ("ini", "ini"),
    ("dotenv", "ini"),
    ("markdown", "markdown"),
    ("diff", "diff"),
    ("patch", "diff"),
    ("dockerfile", "dockerfile"),
    ("docker", "dockerfile"),
    ("makefile", "makefile"),
    ("make", "makefile"),
    ("json", "json"),
    ("jsonc", "json"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_compiles() {
        for (_, spec) in SPECS {
            compile(spec);
        }
    }
}
