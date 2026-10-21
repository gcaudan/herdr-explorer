// Every script parses, on the Node versions the plugin supports: a pane that fails to parse never opens.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.join(__dirname, '..');
const scripts = dir => fs.readdirSync(path.join(root, dir)).filter(f => f.endsWith('.js')).map(f => path.join(dir, f));
for (const file of [...scripts('.'), ...scripts('src'), ...scripts('src/core'), ...scripts('src/render'),
  ...scripts('src/explorer'), ...scripts('src/scm')]) {
  test(file, () => {
    const res = spawnSync(process.execPath, ['--check', path.join(root, file)], { encoding: 'utf8' });
    assert.strictEqual(res.status, 0, res.stderr);
  });
}
