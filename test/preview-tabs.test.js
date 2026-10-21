// The preview's tabs: a click replaces the temporary tab, a kept tab stays, and the strip always shows the current tab.
const test = require('node:test');
const assert = require('node:assert');
const tabs = require('../src/core/tabs');

const files = s => s.list.map(t => `${t.file}${t.kept ? '' : '*'}`).join(' ');
const empty = { list: [], current: 0 };

test('a click replaces the temporary tab, a kept one stays', () => {
  let s = tabs.open(empty, 'a');
  assert.strictEqual(files(s), 'a*');
  assert.strictEqual(s.fresh, true);
  s = tabs.open(s, 'b');
  assert.deepStrictEqual([files(s), s.current], ['b*', 0]);
  s = tabs.open(s, 'b', true);
  assert.deepStrictEqual([files(s), s.fresh], ['b', false]);
  s = tabs.open(s, 'c');
  assert.deepStrictEqual([files(s), s.current], ['b c*', 1]);
  s = tabs.open(s, 'd', true);
  assert.deepStrictEqual([files(s), s.current], ['b d', 1]);
});

test('a file already open goes to its tab, keeping the others', () => {
  let s = { list: [{ file: 'a', kept: true }, { file: 'b', kept: false }, { file: 'c', kept: true }], current: 1 };
  s = tabs.open(s, 'a');
  assert.deepStrictEqual([files(s), s.current, s.fresh], ['a b* c', 0, false]);
  // A kept file takes the temporary tab's place too, as a double-click does in VS Code.
  s = tabs.open(s, 'x', true);
  assert.deepStrictEqual([files(s), s.current], ['a x c', 1]);
  // The tab's own fields are kept when it is shown again.
  s.list[0] = { ...s.list[0], top: 12 };
  assert.strictEqual(tabs.open(s, 'a').list[0].top, 12);
});

test('the list stays under its maximum', () => {
  let s = empty;
  for (let i = 0; i < tabs.MAX + 5; i++) s = tabs.open(s, `f${i}`, true);
  assert.strictEqual(s.list.length, tabs.MAX);
  assert.strictEqual(s.list[s.current].file, `f${tabs.MAX + 4}`);
});

test('closing shows the tab on the right, else on the left', () => {
  const s = { list: ['a', 'b', 'c'].map(file => ({ file, kept: true })), current: 1 };
  assert.deepStrictEqual([files(tabs.close(s, 1)), tabs.close(s, 1).current], ['a c', 1]);
  assert.deepStrictEqual([files(tabs.close(s, 0)), tabs.close(s, 0).current], ['b c', 0]);
  assert.deepStrictEqual([files(tabs.close(s, 2)), tabs.close(s, 2).current], ['a b', 1]);
  const last = { list: s.list, current: 2 };
  assert.strictEqual(tabs.close(last, 2).current, 1);
  // The last tab is never closed.
  assert.strictEqual(tabs.close({ list: [s.list[0]], current: 0 }, 0).list.length, 1);
});

test('next and previous go round, keep keeps', () => {
  const s = { list: ['a', 'b', 'c'].map(file => ({ file, kept: false })), current: 2 };
  assert.strictEqual(tabs.step(s, 1).current, 0);
  assert.strictEqual(tabs.step(s, -1).current, 1);
  assert.strictEqual(tabs.step({ list: s.list, current: 0 }, -1).current, 2);
  assert.strictEqual(files(tabs.keep(s)), 'a* b* c');
  assert.strictEqual(files(tabs.keep(s, 0)), 'a b* c*');
});

test('labels add the folder to names found twice', () => {
  assert.deepStrictEqual(tabs.labels(['/r/src/index.js', '/r/test/index.js', '/r/README.md']),
    ['src/index.js', 'test/index.js', 'README.md']);
});

test('the strip keeps the current tab in view', () => {
  const w = [10, 10, 10, 10, 10];
  assert.deepStrictEqual(tabs.strip(w, 0, 100), { first: 0, last: 4, cut: 0 });
  // 25 columns: two tabs and an arrow.
  assert.deepStrictEqual(tabs.strip(w, 0, 25), { first: 0, last: 1, cut: 0 });
  const end = tabs.strip(w, 4, 25);
  assert.deepStrictEqual(end, { first: 3, last: 4, cut: 0 });
  // Going back by one keeps the strip where it was while the current tab shows.
  assert.deepStrictEqual(tabs.strip(w, 3, 25, end.first), { first: 3, last: 4, cut: 0 });
  // Both sides hidden: two arrows.
  assert.deepStrictEqual(tabs.strip(w, 2, 23, 2), { first: 2, last: 2, cut: 0 });
  for (let room = 1; room < 60; room++) {
    for (let cur = 0; cur < w.length; cur++) {
      const s = tabs.strip(w, cur, room, 0);
      assert.ok(s.first <= cur && cur <= s.last, `room ${room}, tab ${cur}`);
      const used = w.slice(s.first, s.last + 1).reduce((a, b) => a + b, 0) + (s.first ? 2 : 0) + (s.last < 4 ? 2 : 0);
      assert.strictEqual(used - s.cut <= room, true, `room ${room}, tab ${cur}`);
      if (s.cut) assert.strictEqual(s.first, s.last);
    }
  }
});
