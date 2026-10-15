// Terminal width of text and the segment helpers built on it: a row must take exactly the columns the terminal
// gives it, or it pushes the scrollbar or the border out of place.
const test = require('node:test');
const assert = require('node:assert');
const ui = require('../src/ui');

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
