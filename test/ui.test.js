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

test('segmented switch: the choices and their columns', () => {
  const s = ui.segmented([['file', 'File'], ['diff', 'Diff']], 'diff');
  assert.strictEqual(ui.segText(s.segs), ' File  Diff ');
  assert.deepStrictEqual(s.hits, [{ id: 'file', from: 0, to: 5 }, { id: 'diff', from: 6, to: 11 }]);
});

test('a menu drawn on the right keeps what is left of it', () => {
  const plain = s => s.replace(/\x1b\[[0-9;]*m/g, '');
  const row = ui.line([['abcdefghij', ui.fg(ui.C.red)]], 20);
  const lines = ['', row, row, row, row, row];
  const menu = new ui.Menu({ title: 'V', items: [{ label: 'A', value: ui.onOff(true) }, null, { label: 'B', key: 's', run: () => 'b' }], at: 1, right: 22 });
  menu.draw(lines, 22, 24);
  assert.deepStrictEqual(lines.slice(1).map(plain), ['abcdefghij╭ V ──────╮', 'abcdefghij│ A  ● on │', 'abcdefghij├─────────┤',
    'abcdefghij│ B  s    │', 'abcdefghij╰─────────╯']);
  assert.strictEqual(menu.key('s', 24).run(), 'b');
  assert.strictEqual(menu.key('escape', 24), 'close');
});

test('a menu item with a key and no value keeps its whole label', () => {
  const plain = s => s.replace(/\x1b\[[0-9;]*m/g, '');
  const lines = ['', '', ''];
  new ui.Menu({ title: 'M', items: [{ label: 'Rebase onto…', key: 'R' }] }).draw(lines, 40, 24);
  assert.strictEqual(plain(lines[1]).trimEnd(), '│ Rebase onto…  R  │');
});

test('a menu from a left column keeps what is left of it', () => {
  const plain = s => s.replace(/\x1b\[[0-9;]*m/g, '');
  const lines = ['', 'xyz', 'xyz', 'xyz'];
  const menu = new ui.Menu({ title: 'M', items: [{ label: 'Delete…', key: 'del', danger: true, run: () => 'd' }], left: 2 });
  menu.draw(lines, 30, 24);
  assert.deepStrictEqual(lines.slice(0, 3).map(l => plain(l).trimEnd()), [' ╭ M ────────────╮', 'x│ Delete…  del  │', 'x╰───────────────╯']);
  assert.strictEqual(menu.mouse({ press: true, button: 0, x: 4, y: 2 }, 24).run(), 'd');
  assert.strictEqual(menu.mouse({ press: true, button: 0, x: 20, y: 2 }, 24), 'close');
});

test('arrows change an option of a menu, never run an action', () => {
  const menu = new ui.Menu({ title: 'M', items: [{ label: 'Push', key: 'P', run: () => 'push' }, { label: 'Wrap', value: ui.onOff(true), run: () => 'wrap' }] });
  assert.strictEqual(menu.key('right', 24), null);
  assert.strictEqual(menu.key('enter', 24).run(), 'push');
  menu.move(1, 24);
  assert.strictEqual(menu.key('left', 24).run(), 'wrap');
});

test('backgroundOf reads the terminal background from its OSC 11 answer', () => {
  assert.strictEqual(ui.backgroundOf('\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\'), 'dark');
  assert.strictEqual(ui.backgroundOf('\x1b]11;rgb:ffff/ffff/ffff\x07'), 'light');
  assert.strictEqual(ui.backgroundOf('\x1b]11;rgb:fd/f6/e3\x07'), 'light');
  assert.strictEqual(ui.backgroundOf('\x1b]10;rgb:ffff/ffff/ffff\x07'), null);
});

test('useTheme swaps the colours in place and tells the modules that work styles out once', () => {
  const C = ui.C, seen = [];
  ui.onTheme(t => seen.push(t));
  ui.useTheme('light');
  assert.strictEqual(ui.C, C);
  assert.deepStrictEqual(C.strong, [0, 0, 0]);
  assert.strictEqual(ui.themeName(), 'light');
  ui.useTheme('nope');
  assert.deepStrictEqual(C.strong, [255, 255, 255]);
  assert.deepStrictEqual(seen, ['dark', 'light', 'dark']);
});

test('a card with several answers wraps its buttons to fit the pane', () => {
  const plain = l => l.replace(/\x1b\[[0-9;]*m/g, '');
  for (const [W, H] of [[80, 30], [34, 24], [20, 12], [30, 5]]) {
    const lines = Array.from({ length: H }, () => ' '.repeat(W));
    const q = { title: 'Update herdr-explorer', name: '0.7.1 → 0.9.0', list: ['a change', 'another change'], note: 'and 2 more',
      buttons: [{ key: 'y', label: 'Update', tone: 'go' }, { key: 'r', label: 'Notes' }, { key: 's', label: 'Skip version' }, { key: 'n', label: 'Later' }], focus: 3 };
    ui.ask(lines, W, H, q);
    for (const l of lines) assert.strictEqual(ui.textWidth(plain(l)), W, `${W}×${H}: ${plain(l)}`);
    assert.deepStrictEqual(q.zones.buttons.map(b => b[2]).sort(), ['n', 'r', 's', 'y'], `${W}×${H}: every button clickable`);
    for (const [x0, , key, y] of q.zones.buttons) assert.strictEqual(ui.askMouse({ press: true, button: 0, x: x0, y: y + 1 }, q), key);
  }
});
