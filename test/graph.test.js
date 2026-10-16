// The lanes of the history graph: a branch drawn beside its parent's lane, joined back where it started, and the
// second parent of a merge opening a lane.
const test = require('node:test');
const assert = require('node:assert');
const { layout } = require('../src/graph');

const draw = commits => layout(commits).map(r => r.cells.map(([ch]) => ch).join('').trimEnd());

test('a branch beside main, joined where it started', () => {
  assert.deepStrictEqual(draw([
    { hash: 'f2', parents: ['f1'] },
    { hash: 'm2', parents: ['m1'] },
    { hash: 'f1', parents: ['m1'] },
    { hash: 'm1', parents: ['m0'] },
    { hash: 'm0', parents: [] },
  ]), ['●', '│ ●', '● │', '●─╯', '●']);
});

test('a merge opens a lane for its second parent', () => {
  assert.deepStrictEqual(draw([
    { hash: 'M', parents: ['a', 'b'] },
    { hash: 'b', parents: ['a'] },
    { hash: 'a', parents: [] },
  ]), ['●─╮', '│ ●', '●─╯']);
});

test('a lane keeps its colour', () => {
  const rows = layout([{ hash: 'b', parents: ['a'] }, { hash: 'c', parents: ['a'] }, { hash: 'a', parents: [] }]);
  assert.strictEqual(rows[0].color, rows[2].color);
  assert.notStrictEqual(rows[1].color, rows[0].color);
});

test('a tip has no line above, the first commit none below', () => {
  const rows = layout([{ hash: 'b', parents: ['a'] }, { hash: 'a', parents: [] }]);
  assert.deepStrictEqual(rows.map(r => [r.above, r.below]), [[false, true], [true, false]]);
});
