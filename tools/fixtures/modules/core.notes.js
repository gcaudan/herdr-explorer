'use strict';
// Review notes of a tab. add takes its note at a fixed time; locate gets 'unknown' for lines that are not known
// (undefined), and gives back the note itself when nothing changed.
const notes = require('../../../src/core/notes');

const T = 1700000000000;
const impl = {
  ...notes,
  add: (tab, note) => { const now = Date.now; Date.now = () => T; try { return notes.add(tab, note); } finally { Date.now = now; } },
  locate: (n, lines) => notes.locate(n, lines === 'unknown' ? undefined : lines),
};
const env = { HERDR_PLUGIN_STATE_DIR: '$ROOT/state' };
const N = 'state/notes-w1_t1.json';
const after = { [N]: null };
const note = (file, from, to, more = {}) => ({ file, from, to, kind: '', text: `on ${from}`, t: from, ...more });
const saved = list => ({ [N]: JSON.stringify(list) });
const text = 'one\ntwo\nthree\nfour\nfive\n';

module.exports = {
  module: 'core/notes',
  impl,
  cases: [
    { fn: 'read', args: ['w1:t1'], env },
    { fn: 'read', args: ['w1:t1'], env, files: { [N]: '[{"file":"/a","text":"x"},{"file":1,"text":"y"},null,[1],{"file":"/b"},"s"]' } },
    { fn: 'read', args: ['w1:t1'], env, files: { [N]: '{"file":"/a","text":"x"}' } },
    { fn: 'read', args: ['w1:t1'], env, files: { [N]: 'nope' } },
    { fn: 'read', args: ['w1:t1'], files: { [N]: '[{"file":"/a","text":"x"}]' } },

    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'] }), ['one', 'two', 'three']] },
    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'] }), ['new', 'one', 'two', 'three']] },
    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'], stale: true }), ['one', 'two', 'three']] },
    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'] }), ['one', 'three']] },
    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'], stale: true }), ['one', 'three']] },
    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'] }), null] },
    { fn: 'locate', args: [note('/f', 2, 3, { anchor: ['two', 'three'] }), 'unknown'] },
    { fn: 'locate', args: [note('/f', 0, 0, { anchor: ['two'] }), ['one']] },
    { fn: 'locate', args: [note('/f', 2, 2, { anchor: [] }), ['one']] },
    { fn: 'locate', args: [note('/f', 2, 2), ['one']] },
    { fn: 'locate', args: [note('/f', 5, 5, { anchor: ['x'] }), ['x', 'a', 'a', 'a', 'a', 'a', 'a', 'a', 'x']] },
    { fn: 'locate', args: [note('/f', 6, 6, { anchor: ['x'] }), ['x', 'a', 'a', 'a', 'a', 'a', 'a', 'a', 'x']] },
    { fn: 'locate', args: [note('/f', 5, 5, { anchor: ['x'] }), ['a', 'x', 'a', 'a', 'a', 'a', 'a', 'a', 'x']] },
    { fn: 'locate', args: [note('/f', 9, 10, { anchor: ['x', 'y'] }), ['x', 'y', 'z']] },
    { fn: 'locate', args: [note('/f', 1, 1, { anchor: [null] }), ['x']] },
    { fn: 'locate', args: [note('/f', 1.5, 1, { anchor: ['x'] }), ['x', 'x']] },
    { fn: 'locate', args: [note('/f', '3', 3, { anchor: ['x'] }), ['a', 'b', 'x']] },

    { fn: 'current', args: ['w1:t1'], env, files: { f: text, ...saved([note('$ROOT/f', 2, 3, { anchor: ['two', 'three'] })]) }, after },
    { fn: 'current', args: ['w1:t1'], env, files: { f: `zero\n${text}`, ...saved([note('$ROOT/f', 2, 3, { anchor: ['two', 'three'] }), note('$ROOT/g', 1, 1, { anchor: ['x'] }), note('$ROOT/f', 0, 0)]) }, after },
    { fn: 'current', args: ['w1:t1', ''], env, files: { f: `zero\n${text}`, ...saved([note('$ROOT/f', 2, 3, { anchor: ['two', 'three'] })]) }, after },
    { fn: 'current', args: ['w1:t1', '$ROOT/f'], env, files: { f: `zero\n${text}`, ...saved([note('$ROOT/f', 2, 3, { anchor: ['two', 'three'] }), note('$ROOT/g', 1, 1, { anchor: ['x'] })]) }, after },
    { fn: 'current', args: ['w1:t1'], env, files: { f: 'one\r\ntwo\r\n', ...saved([note('$ROOT/f', 1, 1, { anchor: ['two'] })]) }, after },
    { fn: 'current', args: ['w1:t1'], env, files: { f: null, ...saved([note('$ROOT/f/x', 1, 1, { anchor: ['two'] })]) }, after },
    { fn: 'current', args: ['w1:t1'], env, files: { f: 'a\n\n', ...saved([note('$ROOT/f', 3, 3, { anchor: [''] })]) }, after },
    { fn: 'current', args: ['w1:t1'], env, files: { f: 'a\n\n', ...saved([note('$ROOT/f', 2, 2, { anchor: [''], extra: { k: 1 }, stale: true })]) }, after },

    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 2, to: 3, kind: 'fix', text: ' rename\tit\n ' }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 3, to: 2, kind: 'odd', text: 'x' }], env, files: { f: text, ...saved([note('/x', 1, 1)]) }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', text: 'whole' }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 4, text: 'past', }], env, files: { f: 'a\n' }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 1, to: 2, text: 'shown', lines: ['x', 'y'], stale: true }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 1, to: 2, text: 'shown', lines: ['x'] }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 1, to: 2, text: 'unknown', lines: null, stale: true }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 1, to: 1, text: 'stale', stale: true }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', from: 1, to: 201, text: 'long' }], env, files: { f: text }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/l', from: 1, text: 'linked' }], env, files: { f: text, l: { link: '$ROOT/f' } }, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', text: ' \x1b ' }], env, after },
    { fn: 'add', args: ['w1:t1', { file: '', text: 'x' }], env, after },
    { fn: 'add', args: ['w1:t1', { file: '$ROOT/f', text: 'x' }], files: { f: text } },

    { fn: 'undo', args: ['w1:t1'], env, after },
    { fn: 'undo', args: ['w1:t1'], env, files: saved([note('/a', 1, 1), note('/b', 2, 2)]), after },
    { fn: 'undo', args: ['w1:t1'], env, files: saved([note('/a', 1, 1)]), after },

    { fn: 'where', args: [note('$ROOT/d/f', 12, 14), '$ROOT/d'], files: { d: null } },
    { fn: 'where', args: [note('$ROOT/d/f', 12, 12), '$ROOT'] },
    { fn: 'where', args: [note('$ROOT/d/f', 0, 0), '$ROOT/d'] },
    { fn: 'where', args: [note('$ROOT/d/f', 12, 14), '$ROOT/e'] },
    { fn: 'where', args: [note('$ROOT/d/f', 12, 14), ''] },
    { fn: 'where', args: [note('$ROOT/d/f', 12, 14), '$ROOT/d/f'] },
    { fn: 'where', args: [note('$ROOT/d/f', 12, 14), '$ROOT/l'], files: { d: null, l: { link: '$ROOT/d' } } },
    { fn: 'where', args: [note('$ROOT/d/f', 12, 14), '$ROOT/l', true], files: { d: null, l: { link: '$ROOT/d' } } },
    { fn: 'where', args: [{ file: '/x', from: 3 }, '/'] },
    // n.to > n.from: two strings compare as strings.
    { fn: 'where', args: [{ file: '/r/a', from: '9', to: '10' }, '/r'] },
    { fn: 'where', args: [{ file: '/r/a', from: '9', to: 10 }, '/r'] },
    { fn: 'where', args: [{ file: '/r/a', from: [2], to: '10' }, '/r'] },

    { fn: 'format', args: [[], '/'] },
    { fn: 'format', args: [[note('/r/b', 3, 3), note('/r/a', 9, 9, { kind: 'nit' }), note('/r/a', 2, 4, { stale: true, kind: 'fix' }),
      note('/r/B', 1, 1, { text: 'line\nbreak' }), note('/r/a', 2, 2, { t: 0 }), note('/x/é', 0, 0), note('/x/e', 0, 0), note('/r/a', 2, 2, { t: -1, stale: 1 })], '/r', ' Please \x07check: '] },
    { fn: 'format', args: [[note('/r/a10', 1, 1), note('/r/a9', 1, 1), note('/r/a', null, 1), note('/r/a', 'x', 1)], '/r', '\n'] },
    // A comparator that is not an order (NaN from x): sorted as V8 sorts lists under 64 items. A longer list, with
    // an order (ties kept in place).
    ...[12, 40].map(n => ({ fn: 'format', args: [Array.from({ length: n }, (_, i) => note('/r/a', i % 3 ? (i * 7) % 11 : 'x', 1, { t: (i * 5) % 13 })), '/r', ''] })),
    { fn: 'format', args: [Array.from({ length: 150 }, (_, i) => note(`/r/${'abc'[i % 3]}`, (i * 7) % 11, 1, { t: i % 4, text: `n${i}` })), '/r', ''] },
  ],
};
