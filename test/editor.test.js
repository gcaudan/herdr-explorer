// The preview's edit mode (core/buffer.js): the file written back as it was read, the cursor's columns over wide
// characters, marks and tabs, undo steps, find and replace, and the lines marked as changed.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const buffer = require('../src/core/buffer');

const doc = text => buffer.parse(Buffer.from(text));
const editor = text => new buffer.Editor(doc(text));
const round = text => buffer.serialize(doc(text));

test('line endings, the final newline and the byte order mark are kept', () => {
  for (const t of ['', 'a', 'a\n', 'a\nb', 'a\r\nb\r\n', 'a\r\nb', '\n', '\n\n', '\ufeffx\r\n', 'a\rb\n']) assert.strictEqual(round(t), t, JSON.stringify(t));
  assert.strictEqual(doc('a\r\nb\r\n').eol, '\r\n');
  assert.strictEqual(doc('a\nb\n').eol, '\n');
  // Mixed: the line ending most lines have, on every line.
  const mixed = doc('a\r\nb\r\nc\n');
  assert.deepStrictEqual([mixed.eol, mixed.mixed, buffer.serialize(mixed)], ['\r\n', true, 'a\r\nb\r\nc\r\n']);
  // An edit keeps them.
  const ed = editor('one\r\ntwo');
  ed.place({ line: 1, at: 3 }); ed.newline(); ed.type('three');
  assert.strictEqual(buffer.serialize(ed), 'one\r\ntwo\r\nthree');
});

test('binary, other encodings and large files are refused', () => {
  assert.ok(doc('a\0b').error);
  assert.ok(buffer.parse(Buffer.from([0x61, 0xe9, 0x62])).error, 'Latin-1');
  assert.ok(buffer.parse(Buffer.alloc(buffer.MAX_BYTES + 1, 0x61)).error);
  assert.ok(!doc('héllo ✓ 中文').error);
});

test('the save replaces the file whole, keeping its mode, through a link', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'edit-'));
  try {
    const file = path.join(dir, 'a.sh');
    fs.writeFileSync(file, 'old');
    fs.chmodSync(file, 0o750);
    fs.symlinkSync('a.sh', path.join(dir, 'link'));
    const stat = buffer.save(path.join(dir, 'link'), 'new');
    assert.strictEqual(fs.readFileSync(file, 'utf8'), 'new');
    assert.ok(fs.lstatSync(path.join(dir, 'link')).isSymbolicLink(), 'still a link');
    assert.strictEqual(fs.statSync(file).mode & 0o777, 0o750);
    assert.strictEqual(stat.size, 3);
    assert.deepStrictEqual(fs.readdirSync(dir).sort(), ['a.sh', 'link'], 'no temporary file left');
    const r = buffer.read(file);
    assert.deepStrictEqual(r.doc.lines, ['new']);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('columns over tabs, wide characters, combining marks and emoji', () => {
  const s = buffer.shown('a\tb中e\u0301x');
  assert.strictEqual(s.text, 'a   b中e\u0301x');
  assert.deepStrictEqual([0, 1, 2, 3, 4, 5, 6, 7].map(i => s.col[i]), [0, 1, 4, 5, 7, 8, 8, 9]);
  // The cursor steps over a character with its marks, a flag, a joined emoji.
  const t = 'e\u0301🇫🇷👩\u200d💻z';
  const stops = [];
  for (let i = 0; i < t.length; i = buffer.nextAt(t, i)) stops.push(i);
  assert.deepStrictEqual(stops, [0, 2, 6, 11]);
  assert.strictEqual(buffer.prevAt(t, 11), 6);
  assert.strictEqual(buffer.prevAt(t, 6), 2);
  // A column inside a wide character is the character.
  assert.strictEqual(buffer.atCol('中文x', 1), 0);
  assert.strictEqual(buffer.atCol('中文x', 3), 1);
  assert.strictEqual(buffer.atCol('中文x', 9), 3);
  assert.strictEqual(buffer.atCol('\tx', 2), 0);
  assert.strictEqual(buffer.atCol('\tx', 4), 1);
});

test('moves: arrows keep their column, words, home', () => {
  const ed = editor('  hello world\n中文\nabcdefgh');
  ed.place({ line: 0, at: 6 });
  ed.move('down');
  assert.deepStrictEqual(ed.cur, { line: 1, at: 2 }, 'column 6 is past 中文 (4 columns)');
  ed.move('down');
  assert.deepStrictEqual(ed.cur, { line: 2, at: 6 }, 'the column is kept');
  ed.place({ line: 0, at: 13 });
  ed.move('wordleft');
  assert.strictEqual(ed.cur.at, 8);
  ed.move('wordleft');
  assert.strictEqual(ed.cur.at, 2);
  ed.move('wordright');
  assert.strictEqual(ed.cur.at, 7);
  ed.move('home');
  assert.strictEqual(ed.cur.at, 2);
  ed.move('home');
  assert.strictEqual(ed.cur.at, 0);
  ed.move('end', true);
  assert.strictEqual(ed.selected(), '  hello world');
  ed.move('left');
  assert.deepStrictEqual([ed.cur.at, ed.selection()], [0, null], '← folds the selection to its start');
});

test('editing: enter keeps the indentation, backspace joins lines, tab indents', () => {
  const ed = editor('    if (x) {\n}\n');
  ed.place({ line: 0, at: 12 });
  ed.newline();
  ed.indentLines(1);
  ed.type('y');
  assert.strictEqual(buffer.serialize(ed), '    if (x) {\n        y\n}\n');
  assert.strictEqual(ed.indent, '    ');
  ed.selectAll();
  ed.indentLines(-1);
  assert.strictEqual(buffer.serialize(ed), 'if (x) {\n    y\n}\n');
  ed.place({ line: 1, at: 0 });
  ed.erase(-1);
  assert.strictEqual(ed.lines[0], 'if (x) {    y');
  ed.place({ line: 0, at: 13 });
  ed.erase(-1, true);
  assert.strictEqual(ed.lines[0], 'if (x) {    ');
  const tabs = editor('a\n\tb\n\tc\n');
  assert.strictEqual(tabs.indent, '\t');
});

test('backspace after a selection shrunk to nothing erases before the cursor, again and again', () => {
  // shift+→ then shift+← leave the selection's other end at the cursor: the erase must not take it as a selection.
  const ed = editor('abcdef');
  ed.place({ line: 0, at: 3 }); ed.place({ line: 0, at: 4 }, true); ed.place({ line: 0, at: 3 }, true);
  ed.erase(-1); ed.erase(-1);
  assert.strictEqual(buffer.serialize(ed), 'adef');
  // Nor once the line it was on is gone.
  const joined = editor('a\nb');
  joined.place({ line: 1, at: 0 }); joined.place({ line: 1, at: 0 }, true);
  joined.erase(-1); joined.erase(-1);
  assert.strictEqual(buffer.serialize(joined), 'b');
});

test('undo groups typed characters by word, redo comes back', () => {
  const ed = editor('');
  for (const ch of 'hello world') ed.type(ch, 'type');
  assert.strictEqual(ed.text, 'hello world');
  assert.ok(ed.dirty());
  ed.undo();
  assert.strictEqual(ed.text, 'hello');
  ed.undo();
  assert.strictEqual(ed.text, '');
  assert.ok(!ed.dirty(), 'back as opened');
  ed.redo(); ed.redo();
  assert.strictEqual(ed.text, 'hello world');
  // A move starts a new step; so does a save.
  ed.move('home'); ed.type('>', 'type'); ed.move('end'); ed.type('!', 'type');
  ed.undo();
  assert.strictEqual(ed.text, '>hello world');
  ed.markSaved();
  ed.type('?', 'type');
  ed.undo();
  assert.ok(!ed.dirty());
  // Backspaces join; a typed character after them is a step of its own.
  ed.erase(-1); ed.erase(-1);
  ed.undo();
  assert.strictEqual(ed.text, '>hello world');
  assert.deepStrictEqual(ed.cur, { line: 0, at: 12 });
});

test('a paste keeps its text, tabs and lines, as one step', () => {
  const ed = editor('ab');
  ed.place({ line: 0, at: 1 });
  ed.type('x\ty\n\tz');
  assert.deepStrictEqual(ed.lines, ['ax\ty', '\tzb']);
  assert.deepStrictEqual(ed.cur, { line: 1, at: 2 });
  ed.undo();
  assert.deepStrictEqual(ed.lines, ['ab']);
});

test('find, replace one and all', () => {
  const ed = editor('foo Foo\nbar foo');
  assert.strictEqual(buffer.matches(ed.lines, 'foo').length, 3, 'any case');
  assert.strictEqual(buffer.matches(ed.lines, 'Foo').length, 1, 'a capital: exact case');
  ed.place({ line: 0, at: 1 });
  assert.deepStrictEqual(ed.find('foo'), { line: 0, a: 4, b: 7 });
  assert.deepStrictEqual(ed.find('foo'), { line: 1, a: 4, b: 7 });
  assert.deepStrictEqual(ed.find('foo'), { line: 0, a: 0, b: 3 }, 'round');
  assert.deepStrictEqual(ed.find('foo', -1), { line: 1, a: 4, b: 7 });
  assert.ok(ed.replaceOne('foo', 'qux'));
  assert.strictEqual(ed.text, 'foo Foo\nbar qux');
  assert.strictEqual(ed.replaceAll('foo', 'x'), 2);
  assert.strictEqual(ed.text, 'x x\nbar qux');
  ed.undo();
  assert.strictEqual(ed.text, 'foo Foo\nbar qux');
});

test('changed lines: added, modified, and where lines were removed', () => {
  const kinds = (a, b) => {
    const d = buffer.lineDiff(a.split(' '), b.split(' '));
    return [d.kinds.map(k => k[0] || '.').join(''), [...d.deleted].sort((x, y) => x - y)];
  };
  assert.deepStrictEqual(kinds('a b c', 'a b c'), ['...', []]);
  assert.deepStrictEqual(kinds('a b c', 'a x b c'), ['.a..', []]);
  assert.deepStrictEqual(kinds('a b c', 'a B c'), ['.m.', []]);
  assert.deepStrictEqual(kinds('a b c d', 'a d'), ['..', [1]]);
  assert.deepStrictEqual(kinds('a b c', 'a b'), ['..', [2]]);
  assert.deepStrictEqual(kinds('a b c d e', 'x b c y e'), ['m..m.', []]);
  assert.deepStrictEqual(kinds('a b c d e f', 'b c X e f g'), ['..m..a', [0]]);
  // Past the limit, the whole middle counts as changed.
  const d = buffer.lineDiff(['a', '1', '2', 'z'], ['a', '2', '1', 'z'], 0);
  assert.deepStrictEqual(d.kinds, ['', 'mod', 'mod', '']);
  // A large file with a change at each end stays quick.
  const big = Array.from({ length: 50000 }, (_, i) => `line ${i}`), edited = big.slice();
  edited[0] = 'first'; edited.splice(40000, 1);
  const t = Date.now(), r = buffer.lineDiff(big, edited);
  assert.ok(Date.now() - t < 1000);
  assert.deepStrictEqual([r.kinds[0], r.kinds.filter(Boolean).length, [...r.deleted]], ['mod', 1, [40000]]);
});
