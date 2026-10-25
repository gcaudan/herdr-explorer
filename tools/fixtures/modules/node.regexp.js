'use strict';
// Node's regular expressions, as the JS's modules use them (core::regex): exec from a position (index in UTF-16
// units, then each group, null when undefined), split, every match. The patterns: those of the render modules and
// the features they rely on (lookbehind, back references, lazy and counted quantifiers, i and u flags, Annex B).

// A pattern Node rejects gives 'SyntaxError' (the messages are Node's own).
const checked = fn => (...args) => { try { return fn(...args); } catch (e) { if (e instanceof SyntaxError) return 'SyntaxError'; throw e; } };
const impl = {
  exec: checked((src, flags, text, from) => {
    const re = new RegExp(src, flags);
    re.lastIndex = from;
    const m = re.exec(text);
    return m && [m.index, ...[...m].map(g => (g === undefined ? null : g))];
  }),
  split: checked((src, flags, text) => text.split(new RegExp(src, flags)).map(g => (g === undefined ? null : g))),
  all: checked((src, flags, text) => [...text.matchAll(new RegExp(src, flags.includes('g') ? flags : flags + 'g'))].map(m => [m.index, m[0]])),
};

const TEXTS = [
  '', 'a', 'abc', 'const x = /ab+c/gi.test(s) // re', 'a / b / c', 'return /x\\/y[/]z/.source', 'f(/[a-z]/)',
  "  # comment", 'echo $HOME ${PATH} $1 --flag -x a#b', '#include <stdio.h>', '  #  define X 1',
  'key: value # note', '- item: "x"', '--- ', '!!str &anchor *ref', '[section]', 'export A.B=1', 'name = "v"',
  'FROM node AS build', 'RUN --mount=x echo', 'all: $(CC) ${X} $@', 'CC := gcc', 'target: dep',
  ':symbol foo?', '@@cls $glob @inst', 'r"raw" b\'x\' f"{y}"', "'a' b'\\n' 'lifetime", '#[derive(Debug)]',
  'println!("x")', '-eq -NotMatch [string] Get-Item $true', '<?php $x ?>', '#[attr] # c',
  '0x1F 0b101 1_000.5e-3 42u8 .5', 'éclair été 界 under_score', 'Ünïcödé ŞŠ ſ K', 'tab\there\vx\fy\r\n',
  '**bold** __b__ *it* `code` ``a`b`` ![img](u.png) [l](http://x) <https://a.b> <!-- c --> <b>t</b>',
  '<a href="x" title=\'t\' data-x=y>', '&amp; &#123; &#x1F600; &bogus;', 'https://ex.am/ple(1)).',
  '    code', '```js', '~~~', '## Heading ##', '***', '- - -', '> quote', '1. item', '10) x', '- [x] task',
  '| a | b |', '|:--|--:|', ':---:', '[ref]: <http://x> "t"', '<!-- x', 'Title\n===',
  'graph TD', '  A[Start] --> B{Is it?}', '  B -- yes --> C(Done)', 'C -.-> D((x))', 'A ==> B', 'A --o B |lbl|',
  'aaa', 'aaaa', 'xyzxyz', 'foobar', 'foo bar', '1053', ' x ', '\u2028\u00a0\ufeff',
];
const PATTERNS = [
  // The JS rules of highlight.js.
  ['\\/(?![*/])(?<=(?:^|[(,=:\\[!&|?{};+\\-*%<>~^]|\\breturn|\\btypeof)\\s*\\/)(?:\\\\.|\\[(?:\\\\.|[^\\]\\\\])*\\]|[^/\\\\[\\n])+\\/[dgimsuyv]*', 'g'],
  ['@[\\w.]+', 'g'], ['#[A-Za-z_]\\w*', 'g'], ['\\b[rRbBfFuU]{1,2}(?=["\'])', 'g'],
  ['(?<![^\\s;|&(])#', 'g'], ['\\$\\{[^}\\s]*\\}|\\$(?:[A-Za-z_]\\w*|[\\d@#?$!*-])', 'g'], ['(?<=\\s|^)--?[\\w-]+', 'g'],
  ['\\$(?:true|false|null)\\b', 'gi'], ['\\b[A-Za-z]+-[A-Za-z]\\w*', 'gi'], ['-(?:eq|ne|notmatch|f)\\b', 'gi'],
  ['\\[[\\w.]+\\]', 'gi'], ["b?'(?:\\\\.|[^\\\\'])'", 'g'], ["'[A-Za-z_]\\w*", 'g'], ['#!?\\[[^\\]]*\\]', 'g'],
  ['\\w+!', 'g'], ['^\\s*#\\s*\\w+', 'g'], ['(?<=#\\s*include\\s*)<[^>]*>', 'g'], ['<\\?(?:php|=)?|\\?>', 'gi'],
  ['#(?!\\[)', 'gi'], ['(?<![:\\w]):[A-Za-z_]\\w*[?!]?', 'g'], ['@@?\\w+|\\$\\w+', 'g'],
  ['^\\s*(?:-\\s+)*[^\\s#:\'"\\-][^:#]*(?=:(?:\\s|$))', 'gi'], ['^(?:---|\\.\\.\\.)(?=\\s|$)', 'gi'],
  ['[|>][+-]?(?=\\s*$)', 'gi'], ['(?<!\\S)[#;]', 'g'], ['^\\s*\\[[^\\]]*\\]\\]?', 'g'],
  ['^\\s*(?:export\\s+)?[\\w.\\-]+(?=\\s*[=:])', 'g'], ['^\\s*(?:from|run|cmd|as)\\b', 'gi'],
  ['\\bas\\b(?=\\s+\\w+\\s*$)', 'gi'], ['(?<=\\s)--[\\w-]+', 'gi'], ['\\$\\([^)\\s]*\\)|\\$\\{[^}\\s]*\\}|\\$[@<^+?*%$]', 'g'],
  ['^[\\w.\\-]+(?=\\s*[:+?!]?=)', 'g'], ['^[^\\s:#=$][^:#=]*(?=:(?!=))', 'g'],
  ['\\b(?:0[xX][\\da-fA-F_]+|0[bBoO][0-7_]+|\\d[\\d_]*(?:\\.\\d[\\d_]*)?(?:[eE][+-]?\\d+)?)[a-zA-Z]*', 'g'],
  ['(\\/\\/)|(\\/\\*)|(")|(\')|([A-Za-z_$][\\w$]*)', 'g'], ['\\s*\\(', 'y'], ['\\s*:', 'y'],
  ['^(diff |index |--- |\\+\\+\\+ |new file)', ''], ['(`+)[^`]*\\1|\\*\\*[^*]+\\*\\*|__[^_]+__|\\*[^*\\s][^*]*\\*|!?\\[[^\\]]*\\]\\([^)\\s]*\\)|<https?:\\/\\/[^>\\s]*>|<!--.*?(?:-->|$)|<\\/?[A-Za-z][^>]*>', 'g'],
  ['^\\s*(`{3,}|~{3,})\\s*([\\w+#.-]*)', ''], ['^\\s{0,3}#{1,6}(\\s|$)', ''], ['^\\s{0,3}([-*_])(\\s*\\1){2,}\\s*$', ''],
  ['^\\s*(?:>\\s?)+', ''], ['^\\s*(?:[-*+]|\\d+[.)])(?:\\s+\\[[ xX]\\])?(?=\\s|$)', ''],
  ['\\s+|[/?]?>|[^\\s=>/"\']+|=|["\']|.', 'y'], ['<(\\/?)([A-Za-z][\\w:.-]*)', 'y'], ['&(?:#\\d+|#x[\\da-fA-F]+|\\w+);', 'g'],
  ['\\/\\*|(?<![:\\w(])\\/\\/|["\']|@[\\w-]+|#[\\da-fA-F]{3,8}\\b|-?(?:\\d+\\.?\\d*|\\.\\d+)(?:[a-zA-Z%]+)?|!important|[\\w-]+|[{};:]|\\s+|.', 'gy'],
  ['[\\\\/]', ''],
  // markdown.js.
  ['[\\\\`*_~[!<&\\n]|\\bhttps?:\\/\\/', 'g'], ['[!-/:-@[-`{-~]', ''], ['&(#\\d{1,7}|#[xX][\\da-fA-F]{1,6}|[A-Za-z]{2,8});', 'y'],
  ['<((?:https?|ftp|mailto):[^\\s<>]*|[^\\s<>@]+@[^\\s<>@]+\\.\\w+)>', 'y'],
  ['<(\\/?)([A-Za-z][\\w-]*)((?:\\s+[^\\s>/=<]+(?:\\s*=\\s*(?:"[^"]*"|\'[^\']*\'|[^\\s>"\'<]+))?)*)\\s*(\\/?)>', 'y'],
  ['https?:\\/\\/[^\\s<>]*', 'y'], ['[\\p{L}\\p{N}]', 'u'], ['(?:^|\\s)href\\s*=\\s*(?:"([^"]*)"|\'([^\']*)\'|([^\\s>]+))', 'i'],
  ['^\\s*(?:<([^>]*)>|(\\S*))', ''], ['[.,:;!?"\'*_~]+$', ''], ['^\\s*$', ''], ['^( {0,3})(`{3,}|~{3,})(.*)$', ''],
  ['^ {0,3}(#{1,6})(?:[ \\t]+|$)(.*)$', ''], ['^ {0,3}([-*_])(?:[ \\t]*\\1){2,}[ \\t]*$', ''], ['^ {0,3}(=+|-+)[ \\t]*$', ''],
  ['^ {0,3}> ?', ''], ['^( {0,3})([-+*]|\\d{1,9}[.)])(?= |$)( *)', ''],
  ['^ {0,3}\\|?(?:[ \\t]*:?-+:?[ \\t]*\\|)*[ \\t]*:?-+:?[ \\t]*\\|?[ \\t]*$', ''],
  ['^ {0,3}\\[([^\\]]+)\\]:[ \\t]*<?(\\S+?)>?(?:[ \\t]+.*)?$', ''], ['^ {0,3}<!--', ''], ['^\\{\\.?|\\}$', 'g'],
  ['[\\s,{]', ''], ['\\\\$', ''], [' {2,}$', ''], ['\\d', ''], ['^\\[([ xX])\\](?: +|$)', ''], ['(?:^|[ \\t]+)#+[ \\t]*$', ''],
  ['[^\\p{L}\\p{N}\\s_-]', 'gu'], ['\\s', 'g'],
  ['\\s*([A-Za-z_]\\w*)(?:\\s*([[({>]+)\\s*("[^"]*"|[^\\])}"]*)\\s*[\\])}]+)?', 'y'],
  ['\\s*(?:--\\s+([^-|>]+?)\\s+-->|<?(?:--+|==+|-\\.+-?)[>ox]?)\\s*(?:\\|([^|]*)\\|)?', 'y'],
  ['^\\s*(?:flowchart|graph)\\b', ''], ['%%.*', ''], ['^\\[!(\\w+)\\][ \\t]*\\n?', ''], ['\\r\\n?', 'g'], ['^---\\s*$', ''],
  // Features.
  ['a(b|c)+d', ''], ['a*?b', ''], ['(a)|b', ''], ['x{2,3}', ''], ['x{,3}', ''], ['(a|)*b', ''], ['(?:(a)|b)+', ''],
  ['(?<=(\\d+)(\\d+))$', ''], ['(?<=\\$)\\d+', ''], ['foo(?!bar)', ''], ['(?=(\\w+))\\w', ''], ['(z)?(?:\\1y)', ''],
  ['(a+)+$', ''], ['\\x41\\u00e9\\101\\0\\cJ\\8', ''], ['[\\d-z]+', ''], ['[^]', ''], ['[]', ''], ['.', 's'],
  ['^\\w+$', 'm'], ['ſ|K', 'i'], ['[a-z]+', 'i'], ['É', 'i'], ['\\bé', ''], ['(?:)', ''], ['\\B', 'g'],
  ['(', ''], ['a**', ''], ['[b-a]', ''], ['\\p{L}', ''], ['x{2', 'u'], ['\\q', 'u'],
];

const cases = [];
for (const [src, flags] of PATTERNS) {
  for (const text of TEXTS) {
    cases.push({ fn: 'exec', args: [src, flags, text, 0] });
  }
}
// From a position: sticky and global searches.
for (const [src, flags, text, from] of [
  ['\\s*\\(', 'y', 'f (x)', 1], ['\\s*\\(', 'y', 'f (x)', 0], ['b', 'g', 'abcb', 2], ['b', 'g', 'abc', 9],
  ['(?<=a)b', 'y', 'ab', 1], ['^b', 'g', 'ab', 1], ['\\bb', 'g', 'a b', 1], ['é', 'g', 'aéé', 2],
]) cases.push({ fn: 'exec', args: [src, flags, text, from] });
// Groups set inside nested lookarounds, case-insensitive back references and classes, quantifiers on nothing.
for (const [src, flags, text] of [
  ['(?:(?=(?=(a)))ac|ab)', '', 'ab'], ['(?:(?<=(?=(a)))bc|bd)', '', 'abd'], ['(?:(?!(?=(a)))x|ab)', '', 'ab'],
  ['(?:(?=(a))(?=(?=(b)))ac|ab)', '', 'ab'], ['(.)\\1', 'i', 'Ⱥⱥ'], ['(.)\\1', 'i', 'ⱥȺ'], ['(\\w+)\\1', 'i', 'ſS'],
  ['(.)\\1', 'iu', 'Ⱥⱥ'], ['[ς]', 'i', 'σ'], ['[ς]', 'i', 'Σ'], ['ς', 'i', 'σ'], ['[σ]', 'i', 'ς'], ['[^ς]', 'i', 'Σ'],
  ['[α-ω]', 'i', 'Σ'], ['[k]', 'i', 'K'], ['[k]', 'iu', 'K'], ['\\ud801\\udc00', 'i', '\u{10428}'],
  ['\\u{10400}', 'iu', '\u{10428}'], ['[\\u{10400}]', 'iu', '\u{10428}'], ['\u{10400}', 'i', '\u{10428}'],
  ['{2}', '', '{2}'], ['a|{2}', '', 'a'], ['(?=a)*', '', 'a'], ['(?!a)+b', '', 'b'], ['(?<=a)*', '', 'a'],
  ['\\b*', '', 'a'], ['^*', '', 'a'], ['$+', '', 'a'], ['x{2}{3}', '', 'xx'], ['{2,}', '', 'x'], ['(?:{2})', '', 'x'],
  ['{,2}', '', '{,2}'], ['{a}', '', '{a}'], ['{', '', '{'], ['x{2}?', '', 'xx'], ['(?=a){2}', '', 'a'],
]) cases.push({ fn: 'exec', args: [src, flags, text, 0] });
for (const [src, flags, text] of [
  ['(\\n| +)', '', 'a  b\nc  '], ['', '', 'abc'], ['', '', ''], ['x', '', ''], ['\\s+', '', ' a  b '],
  ['[\\s,{]', '', 'js {.x}'], ['(a)|(b)', '', 'xaybz'], ['[\\\\/]', '', 'a/b\\c'], ['(?:)', '', 'é界'],
]) cases.push({ fn: 'split', args: [src, flags, text] });
for (const [src, flags, text] of [
  ['x*', '', 'abc'], ['\\B', '', 'ab cd'], ['[^\\p{L}\\p{N}\\s_-]', 'u', 'Héllo, wörld! 界 😀_-'], ['&(?:#\\d+|\\w+);', '', '&a; &#1; &'],
]) cases.push({ fn: 'all', args: [src, flags, text] });

module.exports = { module: 'node/regexp', impl, cases };
