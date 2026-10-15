// Every script parses, on the Node versions the plugin supports: a pane that fails to parse never opens.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.join(__dirname, '..');
for (const file of fs.readdirSync(root).filter(f => f.endsWith('.js'))) {
  test(file, () => {
    const res = spawnSync(process.execPath, ['--check', path.join(root, file)], { encoding: 'utf8' });
    assert.strictEqual(res.status, 0, res.stderr);
  });
}
