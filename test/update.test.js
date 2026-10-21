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
