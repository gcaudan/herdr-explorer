// The GitHub repository the update check asks for releases (core/update.js repoOf): the clone's origin, on GitHub only.
const test = require('node:test');
const assert = require('node:assert');
const { repoOf } = require('../src/core/update');

test('the repository is read from a GitHub origin URL', () => {
  assert.strictEqual(repoOf('git@github.com:gcaudan/herdr-explorer.git'), 'gcaudan/herdr-explorer');
  assert.strictEqual(repoOf('https://github.com/alice/herdr-explorer'), 'alice/herdr-explorer');
  assert.strictEqual(repoOf('https://github.com/alice/fork.git/'), 'alice/fork');
  assert.strictEqual(repoOf('ssh://git@github.com/alice/fork.git\n'), 'alice/fork');
});

test('another host or a malformed URL gives none', () => {
  assert.strictEqual(repoOf('https://gitlab.com/alice/fork'), null);
  assert.strictEqual(repoOf('https://github.com/alice/fork/tree/main'), null);
  assert.strictEqual(repoOf('https://github.com/../fork'), null);
  assert.strictEqual(repoOf('https://github.com.evil.example/alice/fork'), null);
  assert.strictEqual(repoOf(''), null);
});

test('the changes of a release-please text, as plain lines', () => {
  const body = '## [0.8.0](https://x) (2026-10-22)\n\n### Features\n\n* **diff:** stage hunks ([#12](https://g/12)) ([abc1234](https://g/c))\n'
    + '* add a `light` theme ([d1e2f3a](https://g/d))\n\n### Bug Fixes\n\n- **scm:** keep [the list](https://l)\nplain text\n';
  assert.deepStrictEqual(require('../src/core/update').releaseNotes(body), ['diff: stage hunks', 'add a light theme', 'scm: keep the list']);
  assert.deepStrictEqual(require('../src/core/update').releaseNotes(null), []);
});
