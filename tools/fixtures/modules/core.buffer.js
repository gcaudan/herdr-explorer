'use strict';
// The text edited in the preview: the file parsed, read and written back, characters and columns, words, the
// indentation, the lines changed, find, and the editor driven step by step. The Rust counts offsets in bytes where
// the JS counts UTF-16 units: the wrappers take and give bytes, every offset converted at the call. Characters are
// visited at each code point's start (and the end).
const fs = require('fs');
const buffer = require('../../../src/core/buffer');

// UTF-16 offset i of text as bytes, and back.
const toB = (text, i) => Buffer.byteLength(text.slice(0, i));
const toU = (text, b) => Buffer.from(text).subarray(0, b).toString().length;
// Each code point's start, and the end.
const starts = text => { const out = []; for (let i = 0; i < text.length; i += text.codePointAt(i) > 0xffff ? 2 : 1) out.push(i); return [...out, text.length]; };
const docOf = d => (d.error ? { error: d.error } : { lines: d.lines, eol: d.eol, final: d.final, bom: d.bom, mixed: d.mixed });
const diffOf = d => ({ kinds: d.kinds, deleted: [...d.deleted].sort((x, y) => x - y) });
const matchOf = (lines, m) => m && { line: m.line, a: toB(lines[m.line], m.a), b: toB(lines[m.line], m.b) };

const impl = {
  // Text, or an array of bytes.
  parse: input => docOf(buffer.parse(Buffer.from(input))),
  roundOf: text => { const d = buffer.parse(Buffer.from(text)); return d.error ? d : buffer.serialize(d); },
  readOf: file => { const r = buffer.read(file); return r.error ? { error: r.error } : docOf(r.doc); },
  // The save, then the folder's files and the file's text read back.
  saveOf: (dir, file, text) => { buffer.save(file, text); return { files: fs.readdirSync(dir).sort(), text: fs.readFileSync(file, 'utf8') }; },
  stops: text => { const out = []; for (let i = 0; i < text.length; i = buffer.nextAt(text, i)) out.push(toB(text, i)); return out; },
  // prevAt, snap and nextAt from each code point.
  around: text => starts(text).map(i => [toB(text, i), toB(text, buffer.prevAt(text, i)), toB(text, buffer.snap(text, i)), toB(text, buffer.nextAt(text, i))]),
  // The shown text and width, then at each code point [byte, column, byte in the shown text].
  shown: text => { const s = buffer.shown(text); return { text: s.text, width: s.width, at: starts(text).map(i => [toB(text, i), s.col[i], toB(s.text, s.map[i])]) }; },
  atCol: (text, want) => toB(text, buffer.atCol(text, want)),
  colOf: (text, at) => buffer.colOf(text, toU(text, at)),
  words: text => starts(text).map(i => [toB(text, i), toB(text, buffer.wordStart(text, i)), toB(text, buffer.wordEnd(text, i))]),
  indentOf: buffer.indentOf,
  lineDiff: (a, b, limit) => diffOf(buffer.lineDiff(a, b, limit)),
  matches: (lines, query) => buffer.matches(lines, query).map(m => matchOf(lines, m)),
  // An editor on text, then after each step [method, ...args] what it returned and the editor's state; positions in
  // bytes.
  edit: (text, steps) => {
    const ed = new buffer.Editor(buffer.parse(Buffer.from(text)));
    const pos = p => p && { line: p.line, at: toU(ed.lines[Math.min(Math.max(p.line, 0), ed.lines.length - 1)], p.at) };
    const back = p => p && { line: p.line, at: toB(ed.lines[p.line], p.at) };
    return steps.map(([name, ...args]) => {
      let ret;
      switch (name) {
        case 'place': ret = ed.place(pos(args[0]), args[1]); break;
        case 'selectWord': case 'selectLine': ret = ed[name](pos(args[0])); break;
        case 'reset': ret = ed.reset(buffer.parse(Buffer.from(args[0]))); break;
        case 'find': ret = matchOf(ed.lines, ed.find(...args)); break;
        case 'changed': ret = diffOf(ed.changed()); break;
        case 'serialize': ret = buffer.serialize(ed); break;
        default: ret = ed[name](...args);
      }
      return [ret === undefined ? null : ret, ed.lines.slice(), back(ed.cur), back(ed.anchor), ed.dirty(), ed.selected(), ed.indent];
    });
  },
};

const big = Array.from({ length: 30 }, (_, i) => `l${i}`);
const t = 'é🇫🇷👩‍💻z';
const fancy = ['', 'a', 'a\tb中éx', t, '🇫🇷🇩🇪🇮', '\x01\x7f\x9fa', '\t\t x\t', 'a⚠️b', '👍🏽!', 'x​y', '‍', '́a'];
const wordy = ['  hello world', 'foo_bar$baz+=  qux', 'été 中文 x', 'áb c', '', '   ', '1.5e3', '((x))'];

module.exports = {
  module: 'core/buffer',
  impl,
  cases: [
    ...['', 'a', 'a\n', 'a\nb', 'a\r\nb\r\n', 'a\r\nb', '\n', '\n\n', '﻿x\r\n', 'a\rb\n', 'a\r', '\r\n', 'a\r\nb\r\nc\n', 'a\nb\r\n', '﻿']
      .flatMap(s => [{ fn: 'parse', args: [s] }, { fn: 'roundOf', args: [s] }]),
    { fn: 'parse', args: ['a\0b'] },
    { fn: 'parse', args: [[0x61, 0xe9, 0x62]] },
    { fn: 'parse', args: [[0xef, 0xbb, 0xbf, 0x61]] },
    { fn: 'parse', args: [[0xed, 0xa0, 0x80]] },
    { fn: 'parse', args: ['héllo ✓ 中文'] },
    { fn: 'readOf', args: ['$ROOT/a.txt'], files: { 'a.txt': 'x\r\ny' } },
    { fn: 'readOf', args: ['$ROOT/missing'] },
    { fn: 'readOf', args: ['$ROOT/d'], files: { d: null } },
    { fn: 'readOf', args: ['$ROOT/bin'], files: { bin: 'a\0' } },
    { fn: 'readOf', args: ['$ROOT/link'], files: { 'a.txt': 'via link\n', link: { link: '$ROOT/a.txt' } } },
    { fn: 'saveOf', args: ['$ROOT', '$ROOT/a.sh', 'new\n'], files: { 'a.sh': 'old' } },
    { fn: 'saveOf', args: ['$ROOT', '$ROOT/new.txt', 'made'] },
    { fn: 'saveOf', args: ['$ROOT', '$ROOT/link', 'through'], files: { 'a.sh': 'old', link: { link: 'a.sh' } } },
    ...fancy.flatMap(s => [{ fn: 'stops', args: [s] }, { fn: 'around', args: [s] }, { fn: 'shown', args: [s] }]),
    ...[0, 1, 2, 3, 4, 5, 9].map(w => ({ fn: 'atCol', args: ['中文x', w] })),
    ...[0, 1, 2, 3, 4, 5].map(w => ({ fn: 'atCol', args: ['\tx\ty', w] })),
    ...[0, 1, 3, 7, 11, 22, 23].map(at => ({ fn: 'colOf', args: [t, at] })),
    { fn: 'colOf', args: ['ab', 9] },
    ...wordy.map(s => ({ fn: 'words', args: [s] })),
    { fn: 'indentOf', args: [['a', '\tb', '\tc']] },
    { fn: 'indentOf', args: [['a', '  b', '    c', '\td']] },
    { fn: 'indentOf', args: [['a', ' b', '   ']] },
    { fn: 'indentOf', args: [['          ten']] },
    { fn: 'indentOf', args: [[]] },
    { fn: 'indentOf', args: [['   three', '      six', '\tt']] },
    { fn: 'lineDiff', args: [['a', 'b', 'c'], ['a', 'b', 'c'], 1000] },
    { fn: 'lineDiff', args: [['a', 'b', 'c'], ['a', 'x', 'b', 'c'], 1000] },
    { fn: 'lineDiff', args: [['a', 'b', 'c', 'd'], ['a', 'd'], 1000] },
    { fn: 'lineDiff', args: [['a', 'b', 'c', 'd', 'e', 'f'], ['b', 'c', 'X', 'e', 'f', 'g'], 1000] },
    { fn: 'lineDiff', args: [['a', '1', '2', 'z'], ['a', '2', '1', 'z'], 0] },
    { fn: 'lineDiff', args: [['a', '1', '2', 'z'], ['a', '2', '1', 'z'], 1] },
    { fn: 'lineDiff', args: [[], ['a', 'b'], 1000] },
    { fn: 'lineDiff', args: [['a', 'b'], [], 1000] },
    { fn: 'lineDiff', args: [['x', 'a', 'x', 'b', 'x'], ['a', 'x', 'x', 'b', 'y', 'x'], 1000] },
    { fn: 'lineDiff', args: [big, [...big.slice(0, 5), 'n', ...big.slice(7, 20), ...big.slice(22), 'end'], 1000] },
    { fn: 'matches', args: [['foo Foo', 'bar foo', 'FOOfoo'], 'foo'] },
    { fn: 'matches', args: [['foo Foo', 'bar foo'], 'Foo'] },
    { fn: 'matches', args: [['aaaa'], 'aa'] },
    { fn: 'matches', args: [['été ÉTÉ Été'], 'été'] },
    { fn: 'matches', args: [['straße STRASSE'], 'ß'] },
    { fn: 'matches', args: [['ǅ ǆ Ǆ'], 'ǆ'] },
    { fn: 'matches', args: [['K k K'], 'k'] },
    { fn: 'matches', args: [['ſ s S'], 's'] },
    { fn: 'matches', args: [['ΣΑΣ σας'], 'σ'] },
    { fn: 'matches', args: [['a.b a*b (x)'], '.'] },
    { fn: 'matches', args: [['😀a😀'], '😀'] },
    { fn: 'matches', args: [['x'], ''] },
    { fn: 'edit', args: ['one\r\ntwo', [['place', { line: 1, at: 3 }, false], ['newline'], ['type', 'three', ''], ['serialize']]] },
    { fn: 'edit', args: ['  hello world\n中文\nabcdefgh', [
      ['place', { line: 0, at: 6 }, false], ['move', 'down', false, 10], ['move', 'down', false, 10], ['move', 'up', true, 10],
      ['move', 'pageup', true, 10], ['move', 'pagedown', false, 10], ['move', 'pagedown', false, 1],
      ['place', { line: 0, at: 13 }, false], ['move', 'wordleft', false, 10], ['move', 'wordleft', false, 10],
      ['move', 'wordright', false, 10], ['move', 'home', false, 10], ['move', 'home', false, 10], ['move', 'end', true, 10],
      ['move', 'left', false, 10], ['move', 'end', true, 10], ['move', 'right', false, 10], ['move', 'right', false, 10],
      ['move', 'left', true, 10], ['move', 'wordleft', false, 10], ['move', 'wordright', false, 10], ['move', 'top', false, 10],
      ['move', 'left', false, 10], ['move', 'bottom', true, 10], ['move', 'right', false, 10], ['move', 'nope', false, 10],
      ['place', { line: 1, at: 3 }, false], ['place', { line: 9, at: 99 }, false], ['goto', 2], ['goto', 0], ['goto', 99],
    ]] },
    { fn: 'edit', args: ['    if (x) {\n}\n', [
      ['place', { line: 0, at: 12 }, false], ['newline'], ['indentLines', 1], ['type', 'y', ''], ['selectAll'], ['indentLines', -1],
      ['place', { line: 1, at: 0 }, false], ['erase', -1, false], ['place', { line: 0, at: 13 }, false], ['erase', -1, true],
      ['indentLines', 1], ['place', { line: 0, at: 2 }, false], ['indentLines', 1], ['selectAll'], ['indentLines', 1],
      ['indentLines', -1], ['indentLines', -1], ['undo'], ['undo'], ['redo'], ['changed'],
    ]] },
    { fn: 'edit', args: ['a\n\tb\n\tc\n', [['place', { line: 1, at: 1 }, false], ['indentLines', 1], ['newline'],
      ['place', { line: 0, at: 0 }, false], ['place', { line: 2, at: 0 }, true], ['indentLines', 1], ['indentLines', -1], ['indentLines', -1]]] },
    { fn: 'edit', args: ['abcdef', [['place', { line: 0, at: 3 }, false], ['place', { line: 0, at: 4 }, true], ['place', { line: 0, at: 3 }, true],
      ['erase', -1, false], ['erase', -1, false], ['erase', 1, false], ['erase', 1, true], ['erase', 1, false], ['erase', -1, true], ['erase', -1, false]]] },
    { fn: 'edit', args: ['a\nb', [['place', { line: 1, at: 0 }, false], ['place', { line: 1, at: 0 }, true], ['erase', -1, false], ['erase', -1, false]]] },
    { fn: 'edit', args: ['', [
      ...[...'hello world'].map(ch => ['type', ch, 'type']), ['undo'], ['undo'], ['undo'], ['redo'], ['redo'], ['redo'],
      ['move', 'home', false, 10], ['type', '>', 'type'], ['move', 'end', false, 10], ['type', '!', 'type'], ['undo'],
      ['markSaved'], ['type', '?', 'type'], ['undo'], ['erase', -1, false], ['erase', -1, false], ['undo'], ['redo'], ['undo'],
      ['type', 'a', 'type'], ['type', ' ', 'type'], ['type', ' ', 'type'], ['type', 'b', 'type'], ['undo'], ['undo'], ['undo'],
    ]] },
    { fn: 'edit', args: ['ab', [['place', { line: 0, at: 1 }, false], ['type', 'x\ty\n\tz', ''], ['undo'], ['type', '中\n', 'type'], ['type', 'é', 'type'], ['undo']]] },
    { fn: 'edit', args: ['foo Foo\nbar foo', [
      ['place', { line: 0, at: 1 }, false], ['find', 'foo', 1, false], ['find', 'foo', 1, false], ['find', 'foo', 1, false],
      ['find', 'foo', -1, false], ['find', 'foo', 1, true], ['find', 'nope', 1, false], ['replaceOne', 'foo', 'qux'],
      ['replaceOne', 'foo', 'qux'], ['replaceAll', 'foo', 'x'], ['replaceAll', 'nope', 'x'], ['undo'], ['changed'],
    ]] },
    { fn: 'edit', args: ['été 中文 🇫🇷x\nl2', [
      ['place', { line: 0, at: 1 }, false], ['move', 'right', false, 10], ['move', 'right', false, 10], ['move', 'down', false, 10],
      ['move', 'up', false, 10], ['selectWord', { line: 0, at: 0 }], ['selectWord', { line: 0, at: 7 }], ['selectWord', { line: 0, at: 10 }],
      ['selectWord', { line: 0, at: 18 }], ['selectWord', { line: 0, at: 99 }], ['selectLine', { line: 0, at: 3 }], ['selectLine', { line: 1, at: 0 }],
      ['place', { line: 0, at: 22 }, false], ['erase', -1, false], ['erase', -1, true], ['find', 'É', 1, false], ['find', 'é', 1, false],
    ]] },
    { fn: 'edit', args: ['a\nb\nc', [['place', { line: 2, at: 1 }, false], ['type', 'X', ''], ['reset', 'a\nB\n'], ['changed'], ['type', 'z', '']]] },
    { fn: 'edit', args: ['', [['erase', -1, false], ['erase', 1, false], ['undo'], ['redo'], ['indentLines', 1], ['changed']]] },
  ],
};
