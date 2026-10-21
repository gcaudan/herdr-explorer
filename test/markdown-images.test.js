// The images of rendered Markdown in pixels: the part of each one that shows for a scroll position, and the lines
// that carry them through lists and quotes.
const test = require('node:test');
const assert = require('node:assert');
const { crop, shown } = require('../src/render/image');
const markdown = require('../src/render/markdown');

// An image 4 rows high, 64 pixels (16 a row), its first row at row 10 of the text, under a body 5 rows high.
test('an image wholly in view shows whole', () => {
  assert.deepStrictEqual(crop(10, 4, 64, 8, 10), { at: 2, from: 0, count: 4, y: 0, h: 64 });
});

test('an image scrolled partly above the top shows its lower rows, at the top', () => {
  assert.deepStrictEqual(crop(10, 4, 64, 11, 5), { at: 0, from: 1, count: 3, y: 16, h: 48 });
  assert.deepStrictEqual(crop(10, 4, 64, 13, 5), { at: 0, from: 3, count: 1, y: 48, h: 16 });
});

test('an image going on below the bottom shows its upper rows', () => {
  assert.deepStrictEqual(crop(10, 4, 64, 7, 5), { at: 3, from: 0, count: 2, y: 0, h: 32 });
});

test('an image taller than the body shows the rows in view', () => {
  assert.deepStrictEqual(crop(10, 4, 64, 11, 2), { at: 0, from: 1, count: 2, y: 16, h: 32 });
});

test('an image scrolled out shows nothing', () => {
  assert.strictEqual(crop(10, 4, 64, 14, 5), null);
  assert.strictEqual(crop(10, 4, 64, 5, 5), null);
  assert.strictEqual(crop(10, 0, 64, 10, 5), null);
});

test('the rows split pixels that are not a multiple of the cell evenly', () => {
  // 3 rows over 40 pixels: 13, 14 and 13 pixels.
  assert.deepStrictEqual([0, 1, 2].map(k => crop(0, 3, 40, k, 1)).map(p => [p.y, p.h]), [[0, 13], [13, 14], [27, 13]]);
});

// Rows as the preview builds them: text, then each image's rows carrying figure: { pic, row }.
const rowsOf = (...parts) => parts.flatMap(p => (typeof p === 'number' ? Array.from({ length: p }, () => ({})) : Array.from({ length: p.rows }, (_, row) => ({ figure: { pic: p, row } }))));

test('several images in view, each once with where it starts', () => {
  const a = { id: 1, rows: 3, pixels: 48 }, b = { id: 2, rows: 2, pixels: 32 };
  const list = rowsOf(2, a, 1, b, 4);
  assert.deepStrictEqual(shown(list, 3, 5).map(f => [f.pic.id, f.start, f.at, f.from, f.count]), [[1, 2, 0, 1, 2], [2, 6, 3, 0, 2]]);
  assert.deepStrictEqual(shown(list, 9, 5), []);
});

test('two copies of an image are told apart by their start', () => {
  const a = { id: 1, rows: 2, pixels: 32 };
  assert.deepStrictEqual(shown(rowsOf(a, 1, a), 0, 5).map(f => f.start), [0, 3]);
});

test('an image line keeps its figure in a list and a quote', () => {
  const pic = { rows: 2 };
  const image = () => [0, 1].map(row => Object.assign([[' ', ''], ['    ', '']], { figure: { pic, row } }));
  for (const text of ['- item ![x](a.png)', '> quote ![x](a.png)']) {
    const lines = markdown.render(text, 40, { image }).lines;
    assert.deepStrictEqual(lines.filter(l => l.figure).map(l => l.figure.row), [0, 1]);
    // The blank cells of the image end the line, after the prefix.
    assert.ok(lines.filter(l => l.figure).every(l => l[l.length - 1][0] === '    '));
  }
});
