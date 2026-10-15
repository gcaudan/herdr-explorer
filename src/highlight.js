// Syntax highlighting for the file viewer: a table-driven tokenizer per language (comments, strings, keywords,
// numbers), not a parser. Strings and comments spanning lines carry a state from one line to the next.
// Output: one array of [text, style] segments per source line, ready for ui.line().
'use strict';
const { fg, cleanLine } = require('./ui');

// VS Code Dark+ token colours.
const COLORS = {
  keyword: [86, 156, 214], control: [197, 134, 192], string: [206, 145, 120], number: [181, 206, 168],
  comment: [106, 153, 85], type: [78, 201, 176], function: [220, 220, 170], variable: [156, 220, 254],
  constant: [79, 193, 255], regexp: [209, 105, 105], escape: [215, 186, 125], tag: [86, 156, 214],
  attribute: [156, 220, 254], punctuation: [128, 128, 128], selector: [215, 186, 125], heading: [86, 156, 214],
  added: [129, 184, 139], removed: [244, 135, 113], meta: [155, 155, 155],
};
const S = {};
for (const [k, rgb] of Object.entries(COLORS)) S[k] = fg(rgb);
S.heading = '\x1b[1m' + S.heading;
S.bold = '\x1b[1m';
S.italic = '\x1b[3m';

const LONG = 2000; // longer lines are left plain: minified files would cost too much for nothing readable

const words = s => new Set(s.split(' ').filter(Boolean));
const esc = s => s.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');

// --- generic tokenizer
// spec: line (regex sources of line comments), block ([open, close] comments), str (delimiter, or
// [open, close, flags]: m multi-line, r raw i.e. no escapes), kw / ctl / consts / types (word lists),
// rules ([regex source without capture groups, style or function(text) → style], tried before words),
// ci (case-insensitive words), ident (style of other identifiers), capType (Capitalized words are types),
// keys (a string followed by ':' is a key), escChar (string escape character, \ by default).
const NUMBER = '\\b(?:0[xX][\\da-fA-F_]+|0[bBoO][0-7_]+|\\d[\\d_]*(?:\\.\\d[\\d_]*)?(?:[eE][+-]?\\d+)?)[a-zA-Z]*';
const WORD = '[A-Za-z_$][\\w$]*';
const PAREN = /\s*\(/y;
const COLON = /\s*:/y;

function compile(spec) {
  const opens = [];
  for (const re of spec.line || []) opens.push({ re, style: S.comment, line: true });
  for (const [o, c] of spec.block || []) opens.push({ re: esc(o), close: c, style: S.comment, multi: true });
  for (const s of spec.str || []) {
    const [o, c = o, flags = ''] = Array.isArray(s) ? s : [s];
    opens.push({ re: esc(o), close: c, style: S.string, esc: flags.includes('r') ? null : spec.escChar || '\\',
      multi: flags.includes('m'), keys: spec.keys });
  }
  // Longer delimiters first: """ before ", --[[ before --.
  opens.sort((a, b) => b.re.length - a.re.length);
  const rules = [
    ...opens.map(open => ({ re: open.re, open })),
    ...(spec.rules || []).map(([re, style]) => ({ re, style })),
    { re: NUMBER, style: S.number },
    { re: spec.word || WORD, word: true },
  ];
  return {
    spec, rules, re: new RegExp(rules.map(r => `(${r.re})`).join('|'), spec.ci ? 'gi' : 'g'),
    kw: words(spec.kw || ''), ctl: words(spec.ctl || ''), consts: words(spec.consts || ''),
    types: words(spec.types || ''),
  };
}

// Scans a string or comment from `from` to its closing delimiter; returns [end, state still open or null].
function close(line, from, o, add) {
  let j = from;
  if (!o.esc) j = line.indexOf(o.close, from);
  else {
    for (; j < line.length; j++) {
      if (line[j] === o.esc) j++;
      else if (line.startsWith(o.close, j)) break;
    }
    if (j >= line.length) j = -1;
  }
  if (j < 0) { add(line.slice(from), o.style); return [line.length, o]; }
  add(line.slice(from, j + o.close.length), o.style);
  return [j + o.close.length, null];
}

function segmenter() {
  const out = [];
  const add = (text, style) => {
    if (!text) return;
    const last = out[out.length - 1];
    if (last && last[1] === style) last[0] += text; else out.push([text, style]);
  };
  return [out, add];
}

function wordStyle(c, line, w, start, end) {
  const k = c.spec.ci ? w.toLowerCase() : w;
  if (c.ctl.has(k)) return S.control;
  if (c.kw.has(k) || c.consts.has(k)) return S.keyword;
  if (c.types.has(k)) return S.type;
  PAREN.lastIndex = end;
  if (PAREN.test(line)) return S.function;
  if (line[start - 1] === '.' && c.spec.ident === undefined) return S.variable;
  if (c.spec.capType && w.charCodeAt(0) >= 65 && w.charCodeAt(0) <= 90 && /[a-z]/.test(w)) return S.type;
  return c.spec.ident !== undefined ? c.spec.ident : S.variable;
}

function generic(spec) {
  let c;
  return (line, st) => {
    c = c || compile(spec);
    const [out, add] = segmenter();
    let i = 0;
    if (st) [i, st] = close(line, 0, st, add);
    const re = c.re;
    while (i < line.length) {
      re.lastIndex = i;
      const m = re.exec(line);
      if (!m) { add(line.slice(i), ''); break; }
      add(line.slice(i, m.index), '');
      let g = 1;
      while (m[g] === undefined) g++;
      const rule = c.rules[g - 1], end = m.index + m[0].length;
      if (rule.open) {
        const o = rule.open;
        if (o.line) { add(line.slice(m.index), o.style); break; }
        // A key ("name": in JSON, YAML): the string is coloured as a property once closed.
        const keyed = [];
        [i, st] = close(line, end, o, (t, s) => keyed.push(t));
        COLON.lastIndex = i;
        add(m[0] + keyed.join(''), o.keys && !st && COLON.test(line) ? S.variable : o.style);
        if (st && !o.multi) st = null; // an unterminated single-line string stops at the end of the line
        continue;
      }
      if (end === m.index) { add(line[i], ''); i++; continue; } // empty match: never loop forever
      const style = typeof rule.style === 'function' ? rule.style(m[0]) : rule.style;
      add(m[0], rule.word ? wordStyle(c, line, m[0], m.index, end) : style);
      i = end;
    }
    return [out, st];
  };
}

// --- language tables

const C_LIKE = { line: ['//'], block: [['/*', '*/']], capType: true };
const C_CTL = 'if else for while do switch case default break continue return goto try catch throw finally';
const JS = {
  ...C_LIKE, str: ['"', "'", ['`', '`', 'm']],
  kw: 'var let const function class extends new delete typeof instanceof in of void this super async static get set'
    + ' debugger with as interface type enum namespace declare readonly abstract implements private protected public'
    + ' keyof infer is asserts satisfies override module require',
  ctl: C_CTL + ' import export from default await yield',
  consts: 'true false null undefined NaN Infinity',
  types: 'string number boolean any unknown never object symbol bigint',
  rules: [
    // A regular expression literal after an operator or an opening bracket (not a division).
    ['\\/(?![*/])(?<=(?:^|[(,=:\\[!&|?{};+\\-*%<>~^]|\\breturn|\\btypeof)\\s*\\/)'
      + '(?:\\\\.|\\[(?:\\\\.|[^\\]\\\\])*\\]|[^/\\\\[\\n])+\\/[dgimsuyv]*', S.regexp],
    ['@[\\w.]+', S.function], ['#[A-Za-z_]\\w*', S.variable],
  ],
};
const PY = {
  line: ['#'], str: [['"""', '"""', 'm'], ["'''", "'''", 'm'], '"', "'"],
  kw: 'def class lambda and or not in is global nonlocal pass del async None True False',
  ctl: 'if elif else for while break continue return try except finally raise with as import from yield assert match'
    + ' case await',
  types: 'int float str bool list dict set tuple bytes object type',
  rules: [['@[\\w.]+', S.function], ['\\b[rRbBfFuU]{1,2}(?=["\'])', S.keyword]],
  capType: true,
};
const SHELL = {
  line: ['(?<![^\\s;|&(])#'], str: [['"', '"', 'm'], ["'", "'", 'mr']],
  kw: 'function local export declare readonly typeset unset alias source set shift trap eval exec end begin not and or',
  ctl: 'if then else elif fi for while until do done case esac in select return exit break continue time switch',
  rules: [
    ['\\$\\{[^}\\s]*\\}|\\$(?:[A-Za-z_]\\w*|[\\d@#?$!*-])', S.variable], ['(?<=\\s|^)--?[\\w-]+', S.constant],
  ],
  word: '[A-Za-z_][\\w.-]*', ident: '',
};
const PS = {
  ci: true, line: ['#'], block: [['<#', '#>']], escChar: '`',
  str: [['@"', '"@', 'm'], ["@'", "'@", 'mr'], '"', ["'", "'", 'r']],
  kw: 'function param begin process end filter class enum using module dynamicparam',
  ctl: 'if elseif else switch foreach for while do until break continue return try catch finally throw trap exit in',
  consts: '$true $false $null',
  rules: [
    ['\\$(?:true|false|null)\\b', S.keyword], ['\\$[\\w:]+|\\$\\{[^}]*\\}', S.variable],
    ['\\b[A-Za-z]+-[A-Za-z]\\w*', S.function],
    ['-(?:eq|ne|gt|ge|lt|le|like|notlike|match|notmatch|contains|notcontains|in|notin|and|or|not|xor|is|isnot|as'
      + '|replace|split|join|f)\\b', S.keyword],
    ['\\[[\\w.]+\\]', S.type],
  ],
  ident: '',
};
const RUST = {
  ...C_LIKE, str: [['"', '"', 'm']],
  kw: 'fn let mut const static struct enum trait impl type mod pub use crate self Self super where as ref move unsafe'
    + ' extern dyn async',
  ctl: 'if else match loop while for in return break continue yield await',
  consts: 'true false None Some Ok Err',
  types: 'i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str String Vec Option Result Box',
  rules: [
    ["b?'(?:\\\\.|[^\\\\'])'", S.string], ["'[A-Za-z_]\\w*", S.keyword], ['#!?\\[[^\\]]*\\]', S.meta],
    ['\\w+!', S.function],
  ],
};
const GO = {
  ...C_LIKE, str: ['"', "'", ['`', '`', 'mr']],
  kw: 'package import func var const type struct interface map chan go defer select',
  ctl: 'if else for range switch case default break continue return goto fallthrough',
  consts: 'true false nil iota',
  types: 'int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 uintptr float32 float64 complex64 complex128'
    + ' string bool byte rune error any',
  capType: false,
};
const C_KW = 'int char float double void long short signed unsigned struct union enum typedef static extern const'
  + ' volatile register inline auto sizeof bool';
const C = {
  ...C_LIKE, str: ['"', "'"], kw: C_KW, ctl: C_CTL, consts: 'true false NULL nullptr',
  types: 'size_t ssize_t ptrdiff_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t FILE',
  rules: [['^\\s*#\\s*\\w+', S.control], ['(?<=#\\s*include\\s*)<[^>]*>', S.string]],
};
const CPP = {
  ...C, kw: C_KW + ' class namespace template typename public private protected virtual override final friend'
    + ' operator new delete this using constexpr consteval noexcept explicit mutable decltype static_cast'
    + ' dynamic_cast reinterpret_cast const_cast concept requires co_await co_return co_yield',
  types: C.types + ' std string vector map unique_ptr shared_ptr',
};
const CS = {
  ...C_LIKE, str: [['@"', '"', 'mr'], ['"""', '"""', 'm'], '"', "'"],
  kw: 'using namespace class struct interface enum record public private protected internal static readonly const'
    + ' void var new this base override virtual abstract sealed async partial get set init value int string bool'
    + ' double float decimal long short byte char object dynamic out ref in params is as typeof nameof delegate'
    + ' event operator implicit explicit unsafe fixed lock checked unchecked where required',
  ctl: C_CTL + ' foreach yield await when',
  consts: 'true false null',
  rules: [['^\\s*#\\s*\\w+', S.control]],
};
const JAVA = {
  ...C_LIKE, str: [['"""', '"""', 'm'], '"', "'"],
  kw: 'package import class interface enum record extends implements public private protected static final abstract'
    + ' native synchronized transient volatile strictfp new this super void int long short byte char boolean float'
    + ' double var instanceof throws assert permits sealed',
  ctl: C_CTL + ' yield', consts: 'true false null',
  rules: [['@\\w+', S.type]],
};
const KOTLIN = {
  ...C_LIKE, str: [['"""', '"""', 'mr'], '"', "'"],
  kw: 'package import class interface object fun val var typealias constructor init companion data sealed enum'
    + ' annotation open override abstract final private protected public internal inline suspend operator infix'
    + ' lateinit const vararg reified this super is as in out by get set where',
  ctl: 'if else when for while do break continue return throw try catch finally', consts: 'true false null',
  rules: [['@\\w+', S.type]],
};
const SWIFT = {
  ...C_LIKE, str: [['"""', '"""', 'm'], '"'],
  kw: 'import class struct enum protocol extension func var let init deinit subscript typealias associatedtype static'
    + ' public private fileprivate internal open final override mutating nonmutating lazy weak unowned inout some'
    + ' any self Self super is as where in throws rethrows async convenience required indirect operator',
  ctl: 'if else guard switch case default for while repeat break continue return throw do try catch defer'
    + ' fallthrough await',
  consts: 'true false nil',
  rules: [['@\\w+', S.type]],
};
const PHP = {
  ...C_LIKE, line: ['//', '#(?!\\[)'], str: [['"', '"', 'm'], ["'", "'", 'm']], ci: true,
  kw: 'function class interface trait extends implements namespace use public private protected static final abstract'
    + ' const new clone instanceof echo print var global fn readonly enum',
  ctl: 'if else elseif endif for foreach endforeach while endwhile do switch case default break continue return'
    + ' throw try catch finally yield include require include_once require_once as match',
  consts: 'true false null',
  rules: [['<\\?(?:php|=)?|\\?>', S.meta], ['\\$\\w+', S.variable]],
};
const RUBY = {
  line: ['#'], block: [['=begin', '=end']], str: [['"', '"', 'm'], ["'", "'", 'm'], ['`', '`', 'm']],
  kw: 'def class module self nil true false and or not alias undef super lambda proc attr_reader attr_accessor'
    + ' attr_writer require require_relative include extend private protected public',
  ctl: 'if elsif else unless case when while until for in do end begin rescue ensure raise return break next redo'
    + ' retry yield then',
  rules: [['(?<![:\\w]):[A-Za-z_]\\w*[?!]?', S.constant], ['@@?\\w+|\\$\\w+', S.variable]],
  word: '[A-Za-z_]\\w*[?!]?', capType: true,
};
const LUA = {
  line: ['--'], block: [['--[[', ']]']], str: ['"', "'", ['[[', ']]', 'mr']],
  kw: 'local function and or not nil true false self',
  ctl: 'if then else elseif end for in while do repeat until return break goto',
};
const SQL = {
  ci: true, line: ['--'], block: [['/*', '*/']], str: [["'", "'", 'mr'], '"', ['`', '`', 'r']], ident: '',
  kw: 'select from where insert into values update set delete create table drop alter add column index view join'
    + ' inner left right outer full cross on as and or not null is in exists between like ilike limit offset order'
    + ' by group having distinct union all intersect except case when then else end primary key foreign'
    + ' references unique default check constraint begin commit rollback transaction with recursive returning if'
    + ' replace trigger function procedure declare cast asc desc database schema grant revoke int integer varchar'
    + ' text boolean date time timestamp serial bigint smallint real numeric decimal char float double true false'
    + ' over partition window language returns',
};
const JSON_ = {
  line: ['//'], block: [['/*', '*/']], str: ['"'], keys: true, consts: 'true false null', ident: '',
};
const YAML = {
  line: ['(?<!\\S)#'], str: ['"', ["'", "'", 'r']], keys: true, ci: true, ident: S.string,
  consts: 'true false null yes no on off',
  rules: [
    ['^\\s*(?:-\\s+)*[^\\s#:\'"\\-][^:#]*(?=:(?:\\s|$))', S.keyword], ['^(?:---|\\.\\.\\.)(?=\\s|$)', S.control],
    ['[&*][\\w-]+', S.type], ['!!?[\\w-]*', S.type], ['^\\s*-(?=\\s|$)', ''], ['[|>][+-]?(?=\\s*$)', S.control],
  ],
  word: '[A-Za-z_~][\\w.-]*',
};
const INI = {
  line: ['(?<!\\S)[#;]'], str: [['"""', '"""', 'm'], ["'''", "'''", 'mr'], '"', ["'", "'", 'r']],
  consts: 'true false', ident: '',
  rules: [
    ['^\\s*\\[[^\\]]*\\]\\]?', S.type], ['^\\s*(?:export\\s+)?[\\w.\\-]+(?=\\s*[=:])', S.variable],
    ['\\$\\{[^}\\s]*\\}', S.variable],
  ],
};
const DOCKER = {
  ci: true, line: ['^\\s*#'], str: ['"', "'"], ident: '',
  rules: [
    ['^\\s*(?:from|run|cmd|label|maintainer|expose|env|add|copy|entrypoint|volume|user|workdir|arg|onbuild|stopsignal'
      + '|healthcheck|shell)\\b', S.keyword],
    ['\\bas\\b(?=\\s+\\w+\\s*$)', S.keyword], ['\\$\\{[^}\\s]*\\}|\\$\\w+', S.variable],
    ['(?<=\\s)--[\\w-]+', S.constant],
  ],
};
const MAKE = {
  line: ['#'], str: ['"', "'"], ident: '',
  ctl: 'ifeq ifneq ifdef ifndef else endif include define endef export override unexport vpath',
  rules: [
    ['\\$\\([^)\\s]*\\)|\\$\\{[^}\\s]*\\}|\\$[@<^+?*%$]', S.variable],
    ['^[\\w.\\-]+(?=\\s*[:+?!]?=)', S.variable], ['^[^\\s:#=$][^:#=]*(?=:(?!=))', S.function],
  ],
  word: '[A-Za-z_][\\w-]*',
};

// --- line-based languages

const DIFF_HEAD = /^(diff |index |--- |\+\+\+ |new file|deleted file|old mode|new mode|similarity|rename |Binary )/;
function diff(line) {
  const style = DIFF_HEAD.test(line) ? S.bold : line.startsWith('@@') ? S.keyword : line.startsWith('+') ? S.added
    : line.startsWith('-') ? S.removed : line.startsWith('\\') ? S.comment : '';
  return [[[line, style]], null];
}

// Markdown source: block markers by line, a few inline spans, code fences highlighted in their language.
const MD_INLINE = new RegExp(['(`+)[^`]*\\1', '\\*\\*[^*]+\\*\\*|__[^_]+__|\\*[^*\\s][^*]*\\*',
  '!?\\[[^\\]]*\\]\\([^)\\s]*\\)', '<https?:\\/\\/[^>\\s]*>', '<!--.*?(?:-->|$)', '<\\/?[A-Za-z][^>]*>'].join('|'), 'g');
function markdown(line, st) {
  st = st || {};
  const [out, add] = segmenter();
  if (st.comment) {
    const j = line.indexOf('-->');
    if (j < 0) return [[[line, S.comment]], st];
    add(line.slice(0, j + 3), S.comment);
    line = line.slice(j + 3);
    st = {};
  }
  const fence = /^\s*(`{3,}|~{3,})\s*([\w+#.-]*)/.exec(line);
  if (st.fence) {
    const closing = fence && fence[1][0] === st.fence[0] && fence[1].length >= st.fence.length && !fence[2];
    if (closing) return [[[line, S.string]], {}];
    if (!st.fn) return [[[line, '']], st];
    const [segs, sub] = st.fn(line, st.sub);
    return [segs, { ...st, sub }];
  }
  if (fence) {
    const id = language(fence[2]) || language('x.' + fence[2]);
    return [[[line, S.string]], { fence: fence[1], fn: id ? lineFn(id) : null }];
  }
  let m;
  if ((m = /^\s{0,3}#{1,6}(\s|$)/.exec(line))) return [[[line, S.heading]], st];
  if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) return [[[line, S.comment]], st];
  if ((m = /^\s*(?:>\s?)+/.exec(line)) || (m = /^\s*(?:[-*+]|\d+[.)])(?:\s+\[[ xX]\])?(?=\s|$)/.exec(line))) {
    add(m[0], m[0].includes('>') ? S.comment : S.keyword);
    line = line.slice(m[0].length);
  }
  let i = 0;
  MD_INLINE.lastIndex = 0;
  while ((m = MD_INLINE.exec(line))) {
    add(line.slice(i, m.index), '');
    const t = m[0];
    if (t.startsWith('<!--')) {
      add(t, S.comment);
      if (!t.endsWith('-->')) st = { comment: true };
    } else if (t[0] === '`') add(t, S.string);
    else if (t.startsWith('**') || t.startsWith('__')) add(t, S.bold);
    else if (t[0] === '*') add(t, S.italic);
    else if (t[0] === '<' && !t.startsWith('<http')) add(t, S.tag);
    else if (t[0] === '<') add(t, S.string);
    else {
      const k = t.indexOf('](');
      add(t.slice(0, k + 1), S.string);
      add(t.slice(k + 1), S.keyword);
    }
    i = m.index + t.length;
  }
  add(line.slice(i), '');
  return [out, st];
}

// HTML / XML / Vue: tags and attributes, comments, CDATA; <script> and <style> contents in JavaScript / CSS.
const TAG_TOKEN = /\s+|[/?]?>|[^\s=>/"']+|=|["']|./y;
const OPEN_TAG = /<(\/?)([A-Za-z][\w:.-]*)/y;
const DECL = /<[!?][\w-]*/y;
const sticky = (re, s, i) => { re.lastIndex = i; return re.exec(s); };
function html(line, st) {
  st = st || { mode: 'text' };
  const [out, add] = segmenter();
  let i = 0;
  const n = line.length;
  let lower = null;
  while (i < n) {
    if (st.mode === 'comment' || st.mode === 'cdata') {
      const endTok = st.mode === 'comment' ? '-->' : ']]>';
      const j = line.indexOf(endTok, i), e = j < 0 ? n : j + endTok.length;
      add(line.slice(i, e), st.mode === 'comment' ? S.comment : S.string);
      i = e;
      if (j >= 0) st = { mode: 'text' };
      continue;
    }
    if (st.mode === 'script' || st.mode === 'style') {
      lower = lower || line.toLowerCase();
      const j = lower.indexOf('</' + st.mode, i), e = j < 0 ? n : j;
      if (e > i) {
        const [segs, sub] = lineFn(st.mode === 'script' ? 'js' : 'css')(line.slice(i, e), st.sub);
        for (const [t, s] of segs) add(t, s);
        st = { ...st, sub };
      }
      i = e;
      if (j >= 0) st = { mode: 'text' };
      continue;
    }
    if (st.mode === 'tag') {
      if (st.quote) {
        const j = line.indexOf(st.quote, i), e = j < 0 ? n : j + 1;
        add(line.slice(i, e), S.string);
        i = e;
        if (j >= 0) st = { ...st, quote: null };
        continue;
      }
      TAG_TOKEN.lastIndex = i;
      const t = TAG_TOKEN.exec(line)[0];
      if (t === '"' || t === "'") { add(t, S.string); st = { ...st, quote: t }; }
      else if (t.endsWith('>')) {
        add(t, S.punctuation);
        st = { mode: !st.closing && t === '>' && (st.tag === 'script' || st.tag === 'style') ? st.tag : 'text' };
      } else add(t, /^[^\s=]/.test(t) ? S.attribute : '');
      i += t.length;
      continue;
    }
    // Text: entities, then the next tag.
    const j = line.indexOf('<', i), e = j < 0 ? n : j;
    const text = line.slice(i, e);
    let k = 0;
    for (const m of text.matchAll(/&(?:#\d+|#x[\da-fA-F]+|\w+);/g)) {
      add(text.slice(k, m.index), ''); add(m[0], S.constant); k = m.index + m[0].length;
    }
    add(text.slice(k), '');
    i = e;
    if (j < 0) break;
    let m;
    if (line.startsWith('<!--', i)) { add('<!--', S.comment); i += 4; st = { mode: 'comment' }; }
    else if (line.startsWith('<![CDATA[', i)) { add('<![CDATA[', S.string); i += 9; st = { mode: 'cdata' }; }
    else if ((m = sticky(OPEN_TAG, line, i))) {
      add('<' + m[1], S.punctuation); add(m[2], S.tag);
      i += m[0].length;
      st = { mode: 'tag', tag: m[2].toLowerCase(), closing: !!m[1] };
    } else if ((m = sticky(DECL, line, i))) {
      add(m[0], S.keyword); i += m[0].length; st = { mode: 'tag', tag: '', closing: true };
    } else { add('<', ''); i++; }
  }
  return [out, st];
}

// CSS / SCSS / Less: selectors, properties and values told apart by braces, ':' and ';'.
const CSS_TOKEN = new RegExp(['\\/\\*', '(?<![:\\w(])\\/\\/', '["\']', '@[\\w-]+', '#[\\da-fA-F]{3,8}\\b',
  '-?(?:\\d+\\.?\\d*|\\.\\d+)(?:[a-zA-Z%]+)?', '!important', '[\\w-]+', '[{};:]', '\\s+', '.'].join('|'), 'gy');
function css(line, st) {
  st = st ? { ...st } : { depth: 0, value: false, start: true, comment: false };
  const [out, add] = segmenter();
  let i = 0;
  while (i < line.length) {
    if (st.comment || st.quote) {
      const endTok = st.comment ? '*/' : st.quote;
      const j = line.indexOf(endTok, i), e = j < 0 ? line.length : j + endTok.length;
      add(line.slice(i, e), st.comment ? S.comment : S.string);
      i = e;
      if (j >= 0) st.comment = st.quote = null;
      continue;
    }
    CSS_TOKEN.lastIndex = i;
    const t = CSS_TOKEN.exec(line)[0];
    const end = i + t.length;
    let style = '';
    if (t === '/*') { st.comment = true; style = S.comment; }
    else if (t === '//') { add(line.slice(i), S.comment); break; }
    else if (t === '"' || t === "'") { st.quote = t; style = S.string; }
    else if (t[0] === '@') { style = S.control; st.start = false; }
    else if (t === '{') { st.depth++; st.value = false; st.start = true; }
    else if (t === '}') { st.depth = Math.max(0, st.depth - 1); st.value = false; st.start = true; }
    else if (t === ';') { st.value = false; st.start = true; }
    else if (t === ':' && st.depth > 0 && st.prop) { st.value = true; st.prop = false; }
    else if (/^\s/.test(t)) { add(t, ''); i = end; continue; }
    else if (st.value) {
      style = t === '!important' ? S.keyword : /^[-.\d#]/.test(t) && /\d|#/.test(t) ? S.number
        : line[end] === '(' ? S.function : /^[\w-]/.test(t) ? S.string : '';
    } else if (/^[\w-]/.test(t) && st.depth > 0 && st.start && /^\s*:/.test(line.slice(end, end + 40))
      && (line.indexOf('{', end) < 0 || line.indexOf(';', end) >= 0)) {
      style = S.variable; st.prop = true;
    } else if (/^[\w-]/.test(t) || t === '#' || t === '.') {
      style = line[end] === '(' ? S.function : S.selector;
    } else if (/^#|^-?\d/.test(t)) style = S.selector;
    if (!/^[{};\s]/.test(t) && t !== ':') st.start = false;
    add(t, style);
    i = end;
  }
  return [out, st];
}

// --- languages

const SPECS = {
  js: JS, ts: JS, json: JSON_, python: PY, shell: SHELL, powershell: PS, rust: RUST, go: GO, c: C, cpp: CPP,
  csharp: CS, java: JAVA, kotlin: KOTLIN, swift: SWIFT, php: PHP, ruby: RUBY, lua: LUA, sql: SQL, yaml: YAML,
  ini: INI, dockerfile: DOCKER, makefile: MAKE,
};
const FNS = { diff, markdown, html, xml: html, css };
const cacheFns = {};
function lineFn(id) {
  if (!cacheFns[id]) cacheFns[id] = FNS[id] || (SPECS[id] && generic(SPECS[id]));
  return cacheFns[id];
}

const EXT = {};
for (const [id, exts] of Object.entries({
  js: 'js mjs cjs jsx qml', ts: 'ts tsx mts cts', json: 'json jsonc json5 webmanifest code-workspace',
  python: 'py pyw pyi', shell: 'sh bash zsh fish ksh', powershell: 'ps1 psm1 psd1', rust: 'rs', go: 'go',
  c: 'c h', cpp: 'cpp cc cxx hpp hh hxx ino', csharp: 'cs csx', java: 'java', kotlin: 'kt kts', swift: 'swift',
  php: 'php phtml', ruby: 'rb rake gemspec', lua: 'lua', sql: 'sql', html: 'html htm xhtml vue svelte',
  xml: 'xml svg xsl xslt plist csproj fsproj vbproj props targets xaml', css: 'css scss sass less',
  yaml: 'yaml yml', ini: 'toml ini cfg conf env properties service desktop editorconfig gitconfig',
  markdown: 'md markdown mdx', diff: 'diff patch', dockerfile: 'dockerfile containerfile', makefile: 'mk mak',
})) for (const e of exts.split(' ')) EXT[e] = id;
const NAMES = {
  dockerfile: 'dockerfile', containerfile: 'dockerfile', makefile: 'makefile', gnumakefile: 'makefile',
  '.bashrc': 'shell', '.zshrc': 'shell', '.profile': 'shell', '.bash_profile': 'shell', '.bash_aliases': 'shell',
  '.bash_logout': 'shell', '.zprofile': 'shell', '.zshenv': 'shell', '.envrc': 'shell', 'pkgbuild': 'shell',
  '.env': 'ini', '.gitconfig': 'ini', '.gitmodules': 'ini', '.editorconfig': 'ini', '.npmrc': 'ini',
  '.gitignore': 'ini', '.dockerignore': 'ini', '.gitattributes': 'ini', 'gemfile': 'ruby', 'rakefile': 'ruby',
  'cmakelists.txt': 'makefile', 'jenkinsfile': 'java', 'vagrantfile': 'ruby',
};
// Names used after ``` in Markdown.
const ALIAS = {
  javascript: 'js', node: 'js', typescript: 'ts', python: 'python', python3: 'python', bash: 'shell',
  shell: 'shell', console: 'shell', shellsession: 'shell', zsh: 'shell', sh: 'shell', pwsh: 'powershell',
  powershell: 'powershell', ps: 'powershell', rust: 'rust', golang: 'go', go: 'go', c: 'c', 'c++': 'cpp',
  cpp: 'cpp', csharp: 'csharp', 'c#': 'csharp', java: 'java', kotlin: 'kotlin', swift: 'swift', php: 'php',
  ruby: 'ruby', lua: 'lua', sql: 'sql', html: 'html', xml: 'xml', vue: 'html', svg: 'xml', css: 'css', scss: 'css',
  yaml: 'yaml', toml: 'ini', ini: 'ini', dotenv: 'ini', markdown: 'markdown', diff: 'diff', patch: 'diff',
  dockerfile: 'dockerfile', docker: 'dockerfile', makefile: 'makefile', make: 'makefile', json: 'json', jsonc: 'json',
};

// Language id of a file name (or of a Markdown fence name such as "javascript"), null when unknown.
function language(fileName) {
  if (!fileName) return null;
  const base = String(fileName).split(/[\\/]/).pop().toLowerCase();
  if (NAMES[base]) return NAMES[base];
  if (base.startsWith('.env.') || base.startsWith('dockerfile.')) return base[1] === 'e' ? 'ini' : 'dockerfile';
  const dot = base.lastIndexOf('.');
  if (dot < 0) return ALIAS[base] || null;
  return EXT[base.slice(dot + 1)] || null;
}

// Segments of each line of text (a final newline adds no empty line). lang: id from language(), null for none.
function highlight(text, lang) {
  const raw = String(text).split('\n');
  if (raw.length > 1 && raw[raw.length - 1] === '') raw.pop();
  const fn = lang && lineFn(lang);
  let st = null;
  return raw.map(l => {
    const line = cleanLine(l);
    if (!fn || line.length > LONG) return [[line, '']];
    let segs;
    [segs, st] = fn(line, st);
    return segs.length ? segs : [['', '']];
  });
}

module.exports = { language, highlight, COLORS, STYLES: S };
