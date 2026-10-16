// Terminal width of text and the segment helpers built on it: a row must take exactly the columns the terminal
// gives it, or it pushes the scrollbar or the border out of place.
const test = require('node:test');
const assert = require('node:assert');
const ui = require('../src/core/ui');

const plain = s => s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, '');
const text = segs => segs.map(([t]) => t).join('');

test('character widths', () => {
  const cases = { a: 1, '中': 2, '😀': 2, '✅': 2, '☰': 2, '≡': 1, '⚠': 1, '⚠️': 2, '🇫🇷': 2, 'é': 1, '1️⃣': 2 };
  for (const [s, w] of Object.entries(cases)) assert.strictEqual(ui.textWidth(s), w, JSON.stringify(s));
});

test('the emoji selector counts across segments', () => {
  assert.strictEqual(ui.segWidth([['⚠', ''], ['️c', '']]), 3);
  assert.strictEqual(ui.textWidth(plain(ui.line([['⚠', ''], ['️c', '']], 4))), 4);
});

test('split keeps a character with its emoji selector', () => {
  assert.deepStrictEqual(ui.split([['ab⚠️cd', '']], 3).map(text), ['ab', '⚠️c', 'd']);
  assert.deepStrictEqual(ui.split([['ab⚠', 'x'], ['️', 'y']], 3).map(text), ['ab', '⚠️']);
  assert.deepStrictEqual(ui.split([['a🌡️', '']], 2).map(text), ['a', '🌡️']);
});

test('slice puts a space for a selector whose character is cut', () => {
  assert.strictEqual(text(ui.slice([['a⚠️bc', '']], 2, 3)), ' bc');
  assert.strictEqual(text(ui.slice([['a⚠️bc', '']], 1, 3)), '⚠️b');
});

// Random segment lists: every helper keeps the text and the width it promises.
test('random text keeps its width', () => {
  const parts = ['a', 'b', ' ', '中', '😀', '✅', '☰', '⚠', '️', '🇫', '🇷', 'é', '❤'];
  let seed = 42;
  const rand = n => (seed = (seed * 1103515245 + 12345) % 2147483648) % n;
  for (let i = 0; i < 3000; i++) {
    const segs = Array.from({ length: 1 + rand(4) }, () => [Array.from({ length: rand(12) }, () => parts[rand(parts.length)]).join(''), '']);
    const width = 2 + rand(20);
    const all = text(segs);
    const lines = ui.split(segs, width);
    assert.strictEqual(lines.map(text).join(''), all, 'split keeps the text');
    for (const l of lines) assert.ok(ui.segWidth(l) <= width, `split line over ${width}: ${JSON.stringify(l)}`);
    assert.strictEqual(ui.textWidth(plain(ui.line(segs, width))), width, `line fills ${width}: ${JSON.stringify(segs)}`);
    const from = rand(10);
    assert.ok(ui.segWidth(ui.slice(segs, from, width)) <= width, `slice over ${width}: ${JSON.stringify(segs)}`);
  }
});

test('key hints keep the pairs that fit, whole', () => {
  const pairs = [['s', 'stage'], ['d', 'discard'], ['q', 'quit']];
  assert.strictEqual(text(ui.keys(pairs)), ' s stage   d discard   q quit');
  assert.strictEqual(text(ui.keys(pairs, 20)), ' s stage   d discard');
  assert.strictEqual(ui.keys(pairs)[0][1].includes('\x1b[1m'), true);
});

test('a click on a hint gives its key', () => {
  const segs = ui.keys([['⏎', 'open'], ['↑↓', 'move'], ['space s', 'stage']]);
  // " ⏎ open   ↑↓ move   space s stage"
  assert.strictEqual(ui.keyAt(segs, 1), 'enter');
  assert.strictEqual(ui.keyAt(segs, 11), null);
  assert.strictEqual(ui.keyAt(segs, 21), 'space');
  assert.strictEqual(ui.keyAt(segs, 0), null);
  assert.strictEqual(ui.keyAt(segs, 22, 1), 'space');
});

test('a question card fits the pane, buttons included', () => {
  const plain = l => l.replace(/[[0-9;]*m/g, '');
  for (const [W, H] of [[80, 30], [26, 12], [12, 8], [10, 5], [30, 4], [30, 3], [2, 10], [1, 10]]) {
    const lines = Array.from({ length: H }, () => ' '.repeat(W));
    const q = { title: 'Move to the trash?', name: 'a name long enough to wrap on a few lines.txt', where: 'in src/',
      note: 'This cannot be undone.', action: 'Move', focus: 'no' };
    ui.confirm(lines, W, H, q);
    assert.strictEqual(lines.length, H, `${W}×${H}: rows`);
    for (const l of lines) assert.strictEqual(ui.textWidth(plain(l)), W, `${W}×${H}: ${plain(l)}`);
    if (W < 2) assert.strictEqual(q.zones, null, `${W}×${H}: no card`);
    else assert.ok(q.zones.y < H, `${W}×${H}: buttons out of the pane`);
  }
});
