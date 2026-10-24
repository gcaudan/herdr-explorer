// localeCompare as the JS calls it: the default, and { numeric: true, sensitivity: 'base' }, on pairs of names that
// put each rule to the test (punctuation, symbols, digits and numbers, accents, case, ligatures, other scripts).
'use strict';

const words = [
  '', 'a', 'A', 'b', 'B', 'z', 'Z', 'ab', 'aB', 'Ab', 'a b', 'a_b', 'a-b', 'a.b', 'a/b', 'a~b', 'a$b', '_a', '.a', '-a',
  '1', '2', '10', '01', '001', '0', 'a1', 'a2', 'a10', 'a01', 'a1b', 'a10b', 'v1.2.10', 'v1.10.2', 'x9y', 'x09y',
  'é', 'e', 'E', 'É', 'è', 'ê', 'ë', 'ef', 'éa', 'eb', 'à', 'ä', 'å', 'ç', 'ñ', 'ø', 'ł', 'l', 'lz', 'ß', 'ss', 'st',
  'æ', 'ae', 'af', 'œ', 'oe', 'þ', 'zz', 'ŋ', 'n', 'o', 'α', 'Ω', 'я', '中', '日本', '€', '©', 'á', 'á', 'ab́',
  'a\u0000b', 'ab\t', 'ab ', 'ab ', 'README.md', 'readme.md', 'Readme.md', 'src', 'Src', 'SRC', 'node_modules',
  'node-modules', '.gitignore', '.github', '__init__.py', 'Ünïcödé', 'unicode', 'résumé', 'resume', 'Resume', 'ﬁ',
  'fi', '٣', '3', '😀', '#', '@', 'ǆ', 'dž', 'Ǆ',
];

// Each case: the signs of a.localeCompare(b) for every pair, a row per a, '-', '0' or '+' per b.
const sign = n => '-0+'[Math.sign(n) + 1];
const impl = {
  variant: list => list.map(a => list.map(b => sign(a.localeCompare(b))).join('')),
  numericBase: list => list.map(a => list.map(b => sign(a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' }))).join('')),
};

module.exports = { module: 'node/locale', impl, cases: [{ fn: 'variant', args: [words] }, { fn: 'numericBase', args: [words] }] };
