'use strict';
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const diffs = require('../src/diff');

test('a commit, a stash and a tag give their git arguments; anything else none', () => {
  assert.deepStrictEqual(diffs.showArgs({ kind: 'commit', ref: 'abc', paths: ['a.txt'] }).slice(-3), ['abc', '--', 'a.txt']);
  assert.deepStrictEqual(diffs.showArgs({ kind: 'stash', ref: 'stash@{0}' }).slice(0, 2), ['stash', 'show']);
  assert.strictEqual(diffs.showArgs({ kind: 'tag', ref: 'v1' }).pop(), 'refs/tags/v1');
  // A ref read as an option, an unknown kind, paths not strings: refused, as the request file is the preview's input.
  assert.strictEqual(diffs.showArgs({ kind: 'commit', ref: '--output=/tmp/x' }), null);
  assert.strictEqual(diffs.showArgs({ kind: 'log', ref: 'abc' }), null);
  assert.strictEqual(diffs.showArgs({ kind: 'commit', ref: 'abc', paths: [1] }), null);
});

test('a commit shows its changes', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'show-'));
  const git = (...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...args], { cwd: dir, encoding: 'utf8' });
  git('init', '-q');
  fs.writeFileSync(path.join(dir, 'a.txt'), 'one\n');
  git('add', '.'); git('commit', '-qm', 'first');
  fs.writeFileSync(path.join(dir, 'a.txt'), 'one\ntwo\n');
  git('commit', '-qam', 'second');
  const d = await diffs.show(dir, { kind: 'commit', ref: git('rev-parse', 'HEAD').trim() });
  assert.strictEqual(d.files.length, 1);
  assert.strictEqual(d.add, 1);
  assert.ok(d.pre.some(l => l.includes('second')));
  assert.strictEqual(await diffs.show(dir, { kind: 'commit', ref: 'nope' }), null);
  fs.rmSync(dir, { recursive: true, force: true });
});
