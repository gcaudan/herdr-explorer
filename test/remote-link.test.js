// Links to a network share (core/exec.js remoteLink): found by reading the links on the way, never by following them,
// so that a repository cannot make the plugin reach a share (and send the user's credentials on Windows).
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { remoteLink } = require('../src/core/exec');

// A name that never resolves (RFC 2606), in case a link were followed by mistake.
const SHARE = '\\\\share.invalid\\s';

test('a link to a network share is found, directly, through a folder or another link', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'links-'));
  try {
    fs.writeFileSync(path.join(dir, 'real.png'), '');
    try {
      fs.symlinkSync(`${SHARE}\\a.png`, path.join(dir, 'remote.png'), 'file');
    } catch (e) {
      // Windows without developer mode refuses links.
      if (e.code === 'EPERM') return t.skip('symbolic links not allowed');
      throw e;
    }
    fs.symlinkSync(SHARE, path.join(dir, 'pics'), 'dir');
    fs.symlinkSync('remote.png', path.join(dir, 'chain.png'), 'file');
    fs.symlinkSync('real.png', path.join(dir, 'local.png'), 'file');
    fs.symlinkSync('loop2', path.join(dir, 'loop1'), 'file');
    fs.symlinkSync('loop1', path.join(dir, 'loop2'), 'file');

    assert.strictEqual(remoteLink(path.join(dir, 'remote.png'), dir), true);
    assert.strictEqual(remoteLink(path.join(dir, 'pics', 'a.png'), dir), true, 'a linked folder on the way');
    assert.strictEqual(remoteLink(path.join(dir, 'chain.png'), dir), true, 'a local link to a remote one');
    assert.strictEqual(remoteLink(path.join(dir, 'loop1'), dir), true, 'a loop');
    assert.strictEqual(remoteLink(path.join(dir, 'local.png'), dir), false);
    assert.strictEqual(remoteLink(path.join(dir, 'real.png'), dir), false);
    assert.strictEqual(remoteLink(path.join(dir, 'missing.png'), dir), false);
    // Without a root, every folder of the path is looked at.
    assert.strictEqual(remoteLink(path.join(dir, 'remote.png')), true);

    // The file actions never hand such a link to a program; a rename moves the link itself.
    const actions = require('../src/core/actions');
    assert.deepStrictEqual(actions.duplicate(path.join(dir, 'remote.png')), [null, 'Link to a network share: not opened']);
    assert.strictEqual(actions.duplicate(path.join(dir, 'local.png'))[1], null);
    assert.deepStrictEqual(actions.rename(path.join(dir, 'remote.png'), 'moved.png'), [path.join(dir, 'moved.png'), null]);
    assert.ok(fs.lstatSync(path.join(dir, 'moved.png')).isSymbolicLink());
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
