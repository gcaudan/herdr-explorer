//! Regular expressions in the ECMAScript syntax with the u flag, the subset the panes use: alternatives, capturing and
//! plain groups, greedy and lazy quantifiers, classes, `\d \w \s \b`, `\p{L}`-style properties, back references,
//! lookahead and lookbehind (matched backwards), flags i m s y. A backtracking matcher on an explicit stack, so that
//! long lines never run out of the thread's stack; positions are byte offsets of the text, a character is a code point
//! and the i flag compares simple case foldings.

use crate::unicode;
use std::collections::HashMap;
use std::sync::LazyLock;

/// The characters `.` does not match, and that `^` and `$` stand after and before with the m flag.
pub fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// A compiled regular expression.
#[derive(Debug, Clone)]
pub struct Regex {
    progs: Vec<Prog>,
    groups: usize,
    regs: usize,
    ci: bool,
    multiline: bool,
    dot_all: bool,
    sticky: bool,
}

/// A match: the whole text matched and each group's, as byte ranges of the text.
#[derive(Debug, Clone)]
pub struct Match<'s> {
    text: &'s str,
    caps: Vec<Option<(usize, usize)>>,
}

impl<'s> Match<'s> {
    /// Where the match starts (`m.index`).
    pub fn start(&self) -> usize {
        self.caps[0].map_or(0, |c| c.0)
    }

    /// Where the match ends.
    pub fn end(&self) -> usize {
        self.caps[0].map_or(0, |c| c.1)
    }

    /// The text matched (`m[0]`).
    pub fn as_str(&self) -> &'s str {
        &self.text[self.start()..self.end()]
    }

    /// The text of group n (`m[n]`), None when it took no part in the match.
    pub fn get(&self, n: usize) -> Option<&'s str> {
        self.range(n).map(|(a, b)| &self.text[a..b])
    }

    /// The byte range of group n.
    pub fn range(&self, n: usize) -> Option<(usize, usize)> {
        self.caps.get(n).copied().flatten()
    }

    /// How many groups the expression has, the whole match included.
    pub fn len(&self) -> usize {
        self.caps.len()
    }

    /// Never: the whole match is always there.
    pub fn is_empty(&self) -> bool {
        self.caps.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Builtin {
    Digit,
    Word,
    Space,
    Letter,
    Number,
    Mark,
    Emoji,
    EmojiPresentation,
    RegionalIndicator,
    Any,
}

#[derive(Debug, Clone, PartialEq)]
enum Item {
    Range(char, char),
    Builtin(Builtin, bool),
}

#[derive(Debug, Clone, PartialEq)]
struct Class {
    neg: bool,
    items: Vec<Item>,
}

#[derive(Debug, Clone)]
enum Ast {
    Empty,
    Char(char),
    Class(Class),
    Bol,
    Eol,
    Boundary(bool),
    Group(Box<Ast>, Option<usize>),
    Ref(usize),
    Cat(Vec<Ast>),
    Alt(Vec<Ast>),
    Rep { ast: Box<Ast>, min: u32, max: Option<u32>, greedy: bool, groups: (usize, usize) },
    Look { ast: Box<Ast>, ahead: bool, neg: bool },
}

/// The instructions a pattern may compile to, past which it is refused.
const MAX_INSTS: u64 = 1 << 20;

/// About the instructions an AST compiles to (each copy of a bounded repeat counted), saturated.
fn size(ast: &Ast) -> u64 {
    match ast {
        Ast::Group(a, _) | Ast::Look { ast: a, .. } => size(a).saturating_add(2),
        Ast::Cat(list) | Ast::Alt(list) => list.iter().fold(list.len() as u64, |n, a| n.saturating_add(size(a))),
        Ast::Rep { ast, min, max, .. } => {
            let copies = u64::from(max.unwrap_or(*min).max(*min)).saturating_add(1);
            copies.saturating_mul(size(ast).saturating_add(4))
        }
        _ => 1,
    }
}

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Class(Class),
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Bol,
    Eol,
    Boundary(bool),
    Ref(usize),
    Look { prog: usize, neg: bool },
    Mark(usize),
    Check(usize),
    Reset(usize, usize),
    Match,
}

#[derive(Debug, Clone)]
struct Prog {
    code: Vec<Inst>,
    back: bool,
    // It sets groups: a lookaround running it saves them first.
    caps: bool,
}

// --- parsing

struct Parser {
    src: Vec<char>,
    at: usize,
    groups: usize,
    total: usize,
}

fn err<T>(msg: &str) -> Result<T, String> {
    Err(format!("Invalid regular expression: {msg}"))
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.src.get(self.at).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.src.get(self.at + k).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn alternatives(&mut self) -> Result<Ast, String> {
        let mut alts = vec![self.sequence()?];
        while self.eat('|') {
            alts.push(self.sequence()?);
        }
        Ok(if alts.len() == 1 { alts.pop().unwrap_or(Ast::Empty) } else { Ast::Alt(alts) })
    }

    fn sequence(&mut self) -> Result<Ast, String> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let before = self.groups;
            let atom = self.atom()?;
            let quantifiable = !matches!(atom, Ast::Bol | Ast::Eol | Ast::Boundary(_) | Ast::Look { .. });
            match self.quantifier()? {
                Some((min, max, greedy)) => {
                    if !quantifiable {
                        return err("nothing to repeat");
                    }
                    items.push(Ast::Rep {
                        ast: Box::new(atom),
                        min,
                        max,
                        greedy,
                        groups: (before + 1, self.groups + 1),
                    });
                }
                None => items.push(atom),
            }
        }
        Ok(match items.len() {
            0 => Ast::Empty,
            1 => items.pop().unwrap_or(Ast::Empty),
            _ => Ast::Cat(items),
        })
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == start {
            return None;
        }
        let s: String = self.src[start..self.at].iter().collect();
        Some(s.parse::<u64>().map_or(u32::MAX, |n| n.min(u32::MAX as u64) as u32))
    }

    fn quantifier(&mut self) -> Result<Option<(u32, Option<u32>, bool)>, String> {
        let (min, max) = match self.peek() {
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('?') => (0, Some(1)),
            Some('{') => {
                let save = self.at;
                self.at += 1;
                let Some(min) = self.number() else {
                    self.at = save;
                    return err("incomplete quantifier");
                };
                let max = if self.eat(',') { self.number() } else { Some(min) };
                if !self.eat('}') {
                    self.at = save;
                    return err("incomplete quantifier");
                }
                self.at -= 1;
                if max.is_some_and(|m| m < min) {
                    return err("numbers out of order in {} quantifier");
                }
                (min, max)
            }
            _ => return Ok(None),
        };
        self.at += 1;
        let greedy = !self.eat('?');
        Ok(Some((min, max, greedy)))
    }

    fn atom(&mut self) -> Result<Ast, String> {
        let Some(c) = self.peek() else { return err("unexpected end") };
        self.at += 1;
        Ok(match c {
            '^' => Ast::Bol,
            '$' => Ast::Eol,
            '.' => Ast::Class(Class { neg: false, items: vec![Item::Builtin(Builtin::Any, false)] }),
            '(' => {
                let ast = if self.eat('?') {
                    let look = |ahead, neg| (ahead, neg);
                    let kind = match (self.peek(), self.peek_at(1)) {
                        (Some(':'), _) => None,
                        (Some('='), _) => Some(look(true, false)),
                        (Some('!'), _) => Some(look(true, true)),
                        (Some('<'), Some('=')) => Some(look(false, false)),
                        (Some('<'), Some('!')) => Some(look(false, true)),
                        _ => return err("invalid group"),
                    };
                    self.at += if matches!(kind, Some((false, _))) { 2 } else { 1 };
                    let inner = self.alternatives()?;
                    match kind {
                        None => Ast::Group(Box::new(inner), None),
                        Some((ahead, neg)) => Ast::Look { ast: Box::new(inner), ahead, neg },
                    }
                } else {
                    self.groups += 1;
                    let n = self.groups;
                    Ast::Group(Box::new(self.alternatives()?), Some(n))
                };
                if !self.eat(')') {
                    return err("unterminated group");
                }
                ast
            }
            ')' => return err("unmatched ')'"),
            '[' => Ast::Class(self.class()?),
            '*' | '+' | '?' => return err("nothing to repeat"),
            '{' | '}' | ']' => return err("lone quantifier brackets"),
            '\\' => self.escape()?,
            c => Ast::Char(c),
        })
    }

    fn hex(&mut self, n: usize) -> Option<u32> {
        let digits: String = self.src.get(self.at..self.at + n)?.iter().collect();
        let v = u32::from_str_radix(&digits, 16).ok().filter(|_| digits.chars().all(|c| c.is_ascii_hexdigit()))?;
        self.at += n;
        Some(v)
    }

    // A character escape after '\' (shared by atoms and classes), None for the class escapes.
    fn char_escape(&mut self, c: char, in_class: bool) -> Result<char, String> {
        Ok(match c {
            't' => '\t',
            'n' => '\n',
            'v' => '\u{b}',
            'f' => '\u{c}',
            'r' => '\r',
            'b' if in_class => '\u{8}',
            '0' if !self.peek().is_some_and(|d| d.is_ascii_digit()) => '\0',
            'c' => match self.peek() {
                Some(l) if l.is_ascii_alphabetic() => {
                    self.at += 1;
                    char::from((l as u8) % 32)
                }
                _ => return err("invalid unicode escape"),
            },
            'x' => match self.hex(2) {
                Some(v) => char::from_u32(v).unwrap_or('x'),
                None => return err("invalid escape"),
            },
            'u' => {
                if self.peek() == Some('{') {
                    let end = self.src[self.at..].iter().position(|&c| c == '}');
                    let digits: String = match end {
                        Some(e) => self.src[self.at + 1..self.at + e].iter().collect(),
                        None => return err("invalid unicode escape"),
                    };
                    let v = u32::from_str_radix(&digits, 16).ok().and_then(char::from_u32);
                    self.at += end.unwrap_or(0) + 1;
                    match v {
                        Some(c) => c,
                        None => return err("invalid unicode escape"),
                    }
                } else {
                    match self.hex(4) {
                        Some(v) => char::from_u32(v).unwrap_or('\u{fffd}'),
                        None => return err("invalid unicode escape"),
                    }
                }
            }
            c if !"^$\\.*+?()[]{}|/-".contains(c) => return err("invalid escape"),
            c => c,
        })
    }

    fn property(&mut self) -> Result<Builtin, String> {
        if !self.eat('{') {
            return err("invalid property name");
        }
        let end = self.src[self.at..].iter().position(|&c| c == '}');
        let Some(end) = end else { return err("invalid property name") };
        let name: String = self.src[self.at..self.at + end].iter().collect();
        self.at += end + 1;
        Ok(match name.as_str() {
            "L" | "Letter" => Builtin::Letter,
            "N" | "Number" => Builtin::Number,
            "M" | "Mark" => Builtin::Mark,
            "Emoji" => Builtin::Emoji,
            "Emoji_Presentation" => Builtin::EmojiPresentation,
            "Regional_Indicator" => Builtin::RegionalIndicator,
            _ => return err("invalid property name"),
        })
    }

    // A class escape (\d \w \s \p{…} and their negations) after '\', or None.
    fn class_escape(&mut self, c: char) -> Result<Option<Item>, String> {
        let b = match c.to_ascii_lowercase() {
            'd' => Builtin::Digit,
            'w' => Builtin::Word,
            's' => Builtin::Space,
            'p' => self.property()?,
            _ => return Ok(None),
        };
        Ok(Some(Item::Builtin(b, c.is_ascii_uppercase())))
    }

    fn escape(&mut self) -> Result<Ast, String> {
        let Some(c) = self.peek() else { return err("\\ at end of pattern") };
        self.at += 1;
        match c {
            'b' => return Ok(Ast::Boundary(true)),
            'B' => return Ok(Ast::Boundary(false)),
            '1'..='9' => {
                let start = self.at - 1;
                self.at = start;
                let n = self.number().unwrap_or(0) as usize;
                if n <= self.total {
                    return Ok(Ast::Ref(n));
                }
                return err("invalid escape");
            }
            _ => {}
        }
        if let Some(item) = self.class_escape(c)? {
            return Ok(Ast::Class(Class { neg: false, items: vec![item] }));
        }
        Ok(Ast::Char(self.char_escape(c, false)?))
    }

    fn class(&mut self) -> Result<Class, String> {
        let neg = self.eat('^');
        let mut items = Vec::new();
        loop {
            let Some(c) = self.peek() else { return err("unterminated character class") };
            if c == ']' {
                self.at += 1;
                break;
            }
            let a = self.class_atom()?;
            if self.peek() == Some('-') && self.peek_at(1).is_some_and(|n| n != ']') {
                self.at += 1;
                let b = self.class_atom()?;
                match (a, b) {
                    (Item::Range(x, _), Item::Range(y, _)) => {
                        if x > y {
                            return err("range out of order in character class");
                        }
                        items.push(Item::Range(x, y));
                    }
                    _ => return err("invalid character class"),
                }
            } else {
                items.push(a);
            }
        }
        Ok(Class { neg, items })
    }

    fn class_atom(&mut self) -> Result<Item, String> {
        let Some(c) = self.peek() else { return err("unterminated character class") };
        self.at += 1;
        if c != '\\' {
            return Ok(Item::Range(c, c));
        }
        let Some(e) = self.peek() else { return err("\\ at end of pattern") };
        self.at += 1;
        if let Some(item) = self.class_escape(e)? {
            return Ok(item);
        }
        let ch = match e {
            '-' => '-',
            _ => self.char_escape(e, true)?,
        };
        Ok(Item::Range(ch, ch))
    }
}

// The capturing groups of a pattern, counted before parsing so that \N knows whether it is a back reference.
fn count_groups(src: &[char]) -> usize {
    let (mut n, mut k, mut class) = (0, 0, false);
    while k < src.len() {
        match src[k] {
            '\\' => k += 1,
            '[' => class = true,
            ']' => class = false,
            '(' if !class && src.get(k + 1) != Some(&'?') => n += 1,
            _ => {}
        }
        k += 1;
    }
    n
}

// --- compiling

struct Compiler {
    progs: Vec<Prog>,
    regs: usize,
}

impl Compiler {
    fn prog(&mut self, ast: &Ast, back: bool) -> usize {
        let at = self.progs.len();
        self.progs.push(Prog { code: Vec::new(), back, caps: false });
        let mut code = Vec::new();
        self.emit(ast, back, &mut code);
        code.push(Inst::Match);
        // Groups set here or in a lookaround inside (compiled before this one).
        self.progs[at].caps = code.iter().any(|c| match c {
            Inst::Save(_) | Inst::Reset(..) => true,
            Inst::Look { prog, .. } => self.progs[*prog].caps,
            _ => false,
        });
        self.progs[at].code = code;
        at
    }

    fn emit(&mut self, ast: &Ast, back: bool, code: &mut Vec<Inst>) {
        match ast {
            Ast::Empty => {}
            Ast::Char(c) => code.push(Inst::Char(*c)),
            Ast::Class(c) => code.push(Inst::Class(c.clone())),
            Ast::Bol => code.push(Inst::Bol),
            Ast::Eol => code.push(Inst::Eol),
            Ast::Boundary(b) => code.push(Inst::Boundary(*b)),
            Ast::Ref(n) => code.push(Inst::Ref(*n)),
            Ast::Group(inner, None) => self.emit(inner, back, code),
            Ast::Group(inner, Some(n)) => {
                // Matched backwards, a group meets its end first.
                let (first, last) = if back { (2 * n + 1, 2 * n) } else { (2 * n, 2 * n + 1) };
                code.push(Inst::Save(first));
                self.emit(inner, back, code);
                code.push(Inst::Save(last));
            }
            Ast::Cat(items) => {
                if back {
                    items.iter().rev().for_each(|a| self.emit(a, back, code));
                } else {
                    items.iter().for_each(|a| self.emit(a, back, code));
                }
            }
            Ast::Alt(alts) => {
                let mut jumps = Vec::new();
                for (k, alt) in alts.iter().enumerate() {
                    if k + 1 < alts.len() {
                        let split = code.len();
                        code.push(Inst::Split(split + 1, 0));
                        self.emit(alt, back, code);
                        jumps.push(code.len());
                        code.push(Inst::Jmp(0));
                        let next = code.len();
                        code[split] = Inst::Split(split + 1, next);
                    } else {
                        self.emit(alt, back, code);
                    }
                }
                let end = code.len();
                for j in jumps {
                    code[j] = Inst::Jmp(end);
                }
            }
            Ast::Look { ast, ahead, neg } => {
                let prog = self.prog(ast, !ahead);
                code.push(Inst::Look { prog, neg: *neg });
            }
            Ast::Rep { ast, min, max, greedy, groups } => {
                let reset = |code: &mut Vec<Inst>| {
                    if groups.1 > groups.0 {
                        code.push(Inst::Reset(2 * groups.0, 2 * groups.1));
                    }
                };
                for _ in 0..*min {
                    reset(code);
                    self.emit(ast, back, code);
                }
                let split = |at: usize, body: usize, out: usize| {
                    if *greedy { (at, Inst::Split(body, out)) } else { (at, Inst::Split(out, body)) }
                };
                // An optional iteration: skipped, or matched, but never empty once min is reached.
                let optional = |this: &mut Self, code: &mut Vec<Inst>| {
                    let reg = this.regs;
                    this.regs += 1;
                    let at = code.len();
                    code.push(Inst::Jmp(0));
                    reset(code);
                    code.push(Inst::Mark(reg));
                    this.emit(ast, back, code);
                    code.push(Inst::Check(reg));
                    at
                };
                match max {
                    None => {
                        let at = optional(self, code);
                        code.push(Inst::Jmp(at));
                        let (k, inst) = split(at, at + 1, code.len());
                        code[k] = inst;
                    }
                    Some(max) => {
                        let mut splits = Vec::new();
                        for _ in *min..*max {
                            splits.push(optional(self, code));
                        }
                        let end = code.len();
                        for at in splits {
                            let (k, inst) = split(at, at + 1, end);
                            code[k] = inst;
                        }
                    }
                }
            }
        }
    }
}

// --- matching

enum Bt {
    Go(usize, usize),
    Undo(usize, Option<usize>),
}

fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// Simple case folding: the lower case of the upper case, when each is one character.
fn canon(c: char) -> char {
    single(c.to_uppercase()).and_then(|u| single(u.to_lowercase())).or(single(c.to_lowercase())).unwrap_or(c)
}

// The characters of each canonical form other than itself (none has a case past U+1FFFF).
static SAME: LazyLock<HashMap<char, Vec<char>>> = LazyLock::new(|| {
    let mut map: HashMap<char, Vec<char>> = HashMap::new();
    for c in ('\0'..='\u{1ffff}').filter(|&c| canon(c) != c) {
        map.entry(canon(c)).or_default().push(c);
    }
    map
});

pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Builtin {
    fn has(self, c: char, dot_all: bool) -> bool {
        match self {
            Builtin::Digit => c.is_ascii_digit(),
            Builtin::Word => is_word(c),
            Builtin::Space => unicode::is_space(c),
            Builtin::Letter => unicode::is_letter(c),
            Builtin::Number => unicode::is_number(c),
            Builtin::Mark => unicode::is_mark(c),
            Builtin::Emoji => unicode::is_emoji(c),
            Builtin::EmojiPresentation => unicode::is_emoji_presentation(c),
            Builtin::RegionalIndicator => unicode::is_regional_indicator(c),
            Builtin::Any => dot_all || !is_line_terminator(c),
        }
    }
}

impl Class {
    fn has(&self, c: char, dot_all: bool) -> bool {
        let found = self.items.iter().any(|it| match *it {
            Item::Range(a, b) => a <= c && c <= b,
            Item::Builtin(b, neg) => b.has(c, dot_all) != neg,
        });
        found != self.neg
    }

    // With the i flag: some member has the same canonical form as c.
    fn has_ci(&self, c: char, dot_all: bool) -> bool {
        let u = canon(c);
        let mut forms = vec![c, u];
        forms.extend(SAME.get(&u).into_iter().flatten().filter(|&&f| f != c));
        let any = |cls: &Class| {
            forms.iter().any(|&f| {
                cls.items.iter().any(|it| match *it {
                    Item::Range(a, b) => a <= f && f <= b,
                    Item::Builtin(b, neg) => b.has(f, dot_all) != neg,
                })
            })
        };
        any(self) != self.neg
    }
}

struct Run<'s, 'r> {
    re: &'r Regex,
    text: &'s str,
    slots: Vec<Option<usize>>,
    // Shared by the lookarounds' runs, each above the entries of the run that started it.
    stack: Vec<Bt>,
}

impl Run<'_, '_> {
    fn next(&self, i: usize, back: bool) -> Option<(char, usize)> {
        if back {
            let c = self.text[..i].chars().next_back()?;
            Some((c, i - c.len_utf8()))
        } else {
            let c = self.text[i..].chars().next()?;
            Some((c, i + c.len_utf8()))
        }
    }

    fn set(&mut self, slot: usize, v: Option<usize>) {
        self.stack.push(Bt::Undo(slot, self.slots[slot]));
        self.slots[slot] = v;
    }

    fn word_at(&self, i: usize, back: bool) -> bool {
        self.next(i, back).is_some_and(|(c, _)| is_word(c))
    }

    // Runs program p from i; the position where it matched, the slots set by the match kept.
    fn run(&mut self, p: usize, start: usize) -> Option<usize> {
        let re = self.re;
        let prog = &re.progs[p];
        let back = prog.back;
        let base = self.stack.len();
        let (mut pc, mut i) = (0, start);
        loop {
            let ok = match &prog.code[pc] {
                Inst::Match => {
                    self.stack.truncate(base);
                    return Some(i);
                }
                Inst::Char(c) => match self.next(i, back) {
                    Some((d, j)) if d == *c || (re.ci && canon(d) == canon(*c)) => {
                        i = j;
                        true
                    }
                    _ => false,
                },
                Inst::Class(cls) => match self.next(i, back) {
                    Some((d, j)) if if re.ci { cls.has_ci(d, re.dot_all) } else { cls.has(d, re.dot_all) } => {
                        i = j;
                        true
                    }
                    _ => false,
                },
                Inst::Split(a, b) => {
                    self.stack.push(Bt::Go(*b, i));
                    pc = *a;
                    continue;
                }
                Inst::Jmp(a) => {
                    pc = *a;
                    continue;
                }
                Inst::Save(slot) => {
                    self.set(*slot, Some(i));
                    true
                }
                Inst::Mark(r) => {
                    self.set(2 * re.groups + 2 + r, Some(i));
                    true
                }
                Inst::Check(r) => self.slots[2 * re.groups + 2 + r] != Some(i),
                Inst::Reset(a, b) => {
                    for slot in *a..*b {
                        self.set(slot, None);
                    }
                    true
                }
                Inst::Bol => i == 0 || (re.multiline && self.next(i, true).is_some_and(|(c, _)| is_line_terminator(c))),
                Inst::Eol => {
                    i == self.text.len()
                        || (re.multiline && self.next(i, false).is_some_and(|(c, _)| is_line_terminator(c)))
                }
                Inst::Boundary(b) => (self.word_at(i, true) != self.word_at(i, false)) == *b,
                Inst::Ref(n) => match (self.slots[2 * n], self.slots[2 * n + 1]) {
                    (Some(a), Some(b)) if a <= b => {
                        // Character by character: the two cases of a letter can differ in length.
                        let mut j = i;
                        let mut ok = true;
                        let mut each = |want: char| match self.next(j, back) {
                            Some((d, k)) if d == want || (re.ci && canon(d) == canon(want)) => {
                                j = k;
                            }
                            _ => ok = false,
                        };
                        if back {
                            self.text[a..b].chars().rev().for_each(&mut each);
                        } else {
                            self.text[a..b].chars().for_each(&mut each);
                        }
                        if ok {
                            i = j;
                        }
                        ok
                    }
                    _ => true,
                },
                Inst::Look { prog: sub, neg } => {
                    let saved = re.progs[*sub].caps.then(|| self.slots[..2 * re.groups + 2].to_vec());
                    let found = self.run(*sub, i).is_some();
                    if let Some(saved) = saved {
                        if *neg || !found {
                            self.slots[..saved.len()].copy_from_slice(&saved);
                        } else {
                            // The groups the lookaround set stay, undone when the outer match backtracks over it.
                            for (slot, old) in saved.into_iter().enumerate() {
                                if self.slots[slot] != old {
                                    self.stack.push(Bt::Undo(slot, old));
                                }
                            }
                        }
                    }
                    found != *neg
                }
            };
            if ok {
                pc += 1;
                continue;
            }
            loop {
                if self.stack.len() == base {
                    return None;
                }
                match self.stack.pop() {
                    None => return None,
                    Some(Bt::Undo(slot, v)) => self.slots[slot] = v,
                    Some(Bt::Go(a, j)) => {
                        pc = a;
                        i = j;
                        break;
                    }
                }
            }
        }
    }
}

impl Regex {
    /// The pattern compiled with its flags; or why not, for a pattern the syntax rejects (or one outside the subset,
    /// such as named groups).
    pub fn new(src: &str, flags: &str) -> Result<Regex, String> {
        let has = |f: char| flags.contains(f);
        if let Some(f) = flags.chars().find(|f| !"dgimsuvy".contains(*f)) {
            return Err(format!("Invalid flags supplied to RegExp constructor '{f}'"));
        }
        let chars: Vec<char> = src.chars().collect();
        let total = count_groups(&chars);
        let mut p = Parser { src: chars, at: 0, groups: 0, total };
        let ast = p.alternatives()?;
        if p.at < p.src.len() {
            return err("unmatched ')'");
        }
        // A{n,m} copies its body m times: a{999999999} would take all the memory before any match.
        if size(&ast) > MAX_INSTS {
            return err("regular expression too large");
        }
        let mut c = Compiler { progs: Vec::new(), regs: 0 };
        c.prog(&ast, false);
        Ok(Regex {
            progs: c.progs,
            groups: p.groups,
            regs: c.regs,
            ci: has('i'),
            multiline: has('m'),
            dot_all: has('s'),
            sticky: has('y'),
        })
    }

    fn runner<'s>(&self, text: &'s str) -> Run<'s, '_> {
        Run { re: self, text, slots: vec![None; 2 * self.groups + 2 + self.regs], stack: Vec::new() }
    }

    // The match starting exactly at i.
    fn at<'s>(&self, run: &mut Run<'s, '_>, i: usize) -> Option<Match<'s>> {
        run.slots.fill(None);
        let end = run.run(0, i)?;
        let text = run.text;
        let mut caps: Vec<Option<(usize, usize)>> = (0..=self.groups)
            .map(|n| match (run.slots[2 * n], run.slots[2 * n + 1]) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            })
            .collect();
        caps[0] = Some((i, end));
        Some(Match { text, caps })
    }

    /// `re.lastIndex = i; re.exec(text)`: the first match from byte i (only at i with the y flag), as with the g
    /// or y flag; i past the end, or not on a character, finds nothing.
    pub fn exec_at<'s>(&self, text: &'s str, i: usize) -> Option<Match<'s>> {
        if i > text.len() || !text.is_char_boundary(i) {
            return None;
        }
        let mut run = self.runner(text);
        if self.sticky {
            return self.at(&mut run, i);
        }
        let mut k = i;
        loop {
            if let Some(m) = self.at(&mut run, k) {
                return Some(m);
            }
            k += text[k..].chars().next()?.len_utf8();
        }
    }

    /// `re.exec(text)` without the g flag: from the start (from i with the y flag, here always 0).
    pub fn exec<'s>(&self, text: &'s str) -> Option<Match<'s>> {
        self.exec_at(text, 0)
    }

    /// `re.test(text)` without the g flag.
    pub fn test(&self, text: &str) -> bool {
        self.exec(text).is_some()
    }

    /// The matches from the start, each after the previous one (`text.matchAll(re)`, `replace` with the g flag): an
    /// empty match moves on by one character.
    pub fn matches<'s>(&self, text: &'s str) -> Vec<Match<'s>> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(m) = self.exec_at(text, i) {
            i = if m.end() == m.start() {
                match text[m.end()..].chars().next() {
                    Some(c) => m.end() + c.len_utf8(),
                    None => text.len() + 1,
                }
            } else {
                m.end()
            };
            out.push(m);
        }
        out
    }

    /// `text.replace(re, fn)`: every match with the g flag (all), else the first one.
    pub fn replace(&self, text: &str, all: bool, mut with: impl FnMut(&Match) -> String) -> String {
        let found = if all { self.matches(text) } else { self.exec(text).into_iter().collect() };
        let mut out = String::new();
        let mut last = 0;
        for m in &found {
            out.push_str(&text[last..m.start()]);
            out.push_str(&with(m));
            last = m.end();
        }
        out.push_str(&text[last..]);
        out
    }

    /// `text.replace(re, s)` with a plain replacement (no `$`).
    pub fn replace_with(&self, text: &str, all: bool, s: &str) -> String {
        self.replace(text, all, |_| s.to_string())
    }

    /// `text.split(re)`: the pieces between matches, each followed by the groups of the match (None: a group that took no part).
    pub fn split<'s>(&self, text: &'s str) -> Vec<Option<&'s str>> {
        let mut run = self.runner(text);
        if text.is_empty() {
            return if self.at(&mut run, 0).is_some() { vec![] } else { vec![Some(text)] };
        }
        let mut out = Vec::new();
        let (mut p, mut q) = (0, 0);
        let step = |q: usize| q + text[q..].chars().next().map_or(1, char::len_utf8);
        while q < text.len() {
            match self.at(&mut run, q) {
                Some(m) if m.end() != p => {
                    out.push(Some(&text[p..q]));
                    out.extend((1..m.len()).map(|n| m.get(n)));
                    p = m.end();
                    q = p;
                }
                _ => q = step(q),
            }
        }
        out.push(Some(&text[p..]));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(src: &str, flags: &str, text: &str) -> Option<Vec<Option<String>>> {
        let re = Regex::new(src, flags).unwrap();
        re.exec(text).map(|m| (0..m.len()).map(|n| m.get(n).map(String::from)).collect())
    }

    #[test]
    fn groups_alternatives_and_quantifiers() {
        assert_eq!(find("a(b|c)+d", "", "xxacbcd"), Some(vec![Some("acbcd".into()), Some("c".into())]));
        assert_eq!(find("a*?b", "", "aaab").unwrap()[0].as_deref(), Some("aaab"));
        assert_eq!(find("(a)|b", "", "b").unwrap()[1], None);
        assert_eq!(find("x{2,3}", "", "xxxx").unwrap()[0].as_deref(), Some("xxx"));
        assert!(Regex::new("x{,3}", "").is_err() && Regex::new("a{", "").is_err());
        // An iteration matching nothing ends the loop, and the groups of each iteration start afresh.
        assert_eq!(find("(a|)*b", "", "aab").unwrap()[0].as_deref(), Some("aab"));
        assert_eq!(find("(?:(a)|b)+", "", "ab").unwrap()[1], None);
        // Counts too large to compile are refused, not expanded.
        assert!(Regex::new("a{999999999}", "").is_err() && Regex::new("(a{1000}){1000}", "").is_err());
        assert!(Regex::new("a{1000}", "").is_ok());
    }

    #[test]
    fn lookarounds_and_references() {
        assert_eq!(find("(?<=\\$)\\d+", "", "a $42").unwrap()[0].as_deref(), Some("42"));
        assert_eq!(find("(?<!\\S)#", "", "a#b #c").map(|m| m[0].clone()), Some(Some("#".into())));
        assert_eq!(Regex::new("(?<!\\S)#", "").unwrap().exec("a#b #c").unwrap().start(), 4);
        assert_eq!(find("(`+)[^`]*\\1", "", "a ``x` y`` b").unwrap()[0].as_deref(), Some("``"));
        assert_eq!(find("(?<=(\\d+)(\\d+))$", "", "1053").unwrap()[1..], [Some("1".into()), Some("053".into())]);
        assert!(find("foo(?!bar)", "", "foobar").is_none());
    }

    #[test]
    fn flags_and_classes() {
        assert!(Regex::new("SELECT\\b", "i").unwrap().test("select x"));
        assert!(Regex::new("[a-z]+", "i").unwrap().test("ABC"));
        assert!(!Regex::new("^[\\p{L}\\p{N}]$", "u").unwrap().test("_"));
        assert!(Regex::new("^[\\p{L}\\p{N}]$", "u").unwrap().test("é"));
        assert!(Regex::new("^[\\w-]$", "").unwrap().test("-"));
        let re = Regex::new("b", "y").unwrap();
        assert!(re.exec_at("ab", 0).is_none() && re.exec_at("ab", 1).is_some());
        assert!(Regex::new("(", "").is_err() && Regex::new("a**", "").is_err());
    }

    #[test]
    fn split_and_replace() {
        let re = Regex::new("(\\n| +)", "").unwrap();
        assert_eq!(re.split("a  b\nc"), [Some("a"), Some("  "), Some("b"), Some("\n"), Some("c")]);
        assert_eq!(Regex::new("", "").unwrap().split("ab"), [Some("a"), Some("b")]);
        assert_eq!(
            Regex::new("[.*]", "").unwrap().replace("a.b*c", true, |m| format!("\\{}", m.as_str())),
            "a\\.b\\*c"
        );
        assert_eq!(Regex::new("x*", "").unwrap().replace_with("abc", true, "-"), "-a-b-c-");
    }
}
