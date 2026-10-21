'use strict';
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { target } = require('../src/core/places');

const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'places-'));

test('lines, ranges and columns, separated by commas', () => {
  assert.deepStrictEqual(target('app.js:12-14,30,42:5-18', cwd), [path.join(cwd, 'app.js'), [
    { from: 12, to: 14, a: 0, b: 0 }, { from: 30, to: 30, a: 0, b: 0 }, { from: 42, to: 42, a: 5, b: 18 }]]);
});

test('a single column is a position: the whole line', () => {
  assert.deepStrictEqual(target('app.js:12:5', cwd)[1], [{ from: 12, to: 12, a: 0, b: 0 }]);
});

test('no places, or a tail that does not read as places: the name whole', () => {
  assert.deepStrictEqual(target('app.js', cwd), [path.join(cwd, 'app.js'), []]);
  assert.deepStrictEqual(target('notes:draft', cwd), [path.join(cwd, 'notes:draft'), []]);
});

test('a name with a colon that exists is taken whole', () => {
  if (process.platform === 'win32') return;
  fs.writeFileSync(path.join(cwd, 'a:12'), '');
  assert.deepStrictEqual(target('a:12', cwd), [path.join(cwd, 'a:12'), []]);
});

test('places that cannot be are refused', () => {
  for (const bad of ['app.js:0', 'app.js:14-12', 'app.js:12:9-5', 'app.js:12:0-5']) {
    assert.throws(() => target(bad, cwd), /wrong place/, bad);
  }
});
