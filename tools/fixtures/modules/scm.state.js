'use strict';
// The source control pane's git (src/scm/state.js): the environment in English, the paths on stdin, full branch
// names, and the hardened environment of the commands that write, seen through git config --get-all in a repository
// that sets the risky keys itself. HOME and the global config are the case's folder's, so that the generator's own
// config stays out.
const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const state = require('../../../src/scm/state');
const { S } = require('../../../src/scm/context');

// A repository under root/repo with the repository's config (local) and the user's (global, in root/global), then
// the values git gives key, with the hardened environment or without.
async function safeGit(root, local, global, key, safe) {
  const top = path.join(root, 'repo');
  fs.mkdirSync(top);
  execFileSync('git', ['init', '-q', top]);
  for (const [k, v] of local) execFileSync('git', ['config', '--add', k, v], { cwd: top });
  for (const [k, v] of global) execFileSync('git', ['config', '--file', path.join(root, 'global'), '--add', k, v]);
  S.repo = { top };
  const res = await state.git(['config', '--get-all', key], { safe });
  S.repo = null;
  return { code: res.code, out: res.out };
}

const ENV = { HOME: '$ROOT', XDG_CONFIG_HOME: '$ROOT/xdg', GIT_CONFIG_GLOBAL: '$ROOT/global', GIT_CONFIG_NOSYSTEM: '1' };
const cases = [];
const env = (...list) => { for (const e of list) cases.push({ fn: 'english', args: [e] }); };
env({}, { LC_ALL: 'fr_FR.UTF-8', LANG: 'fr_FR.UTF-8', LANGUAGE: 'fr', LC_MESSAGES: 'fr_FR.UTF-8' }, { LC_ALL: '', LC_CTYPE: 'x' },
  { LANG: 'de_DE.UTF-8', PATH: '/bin' });
for (const items of [[], [{ path: 'a' }], [{ path: 'new', orig: 'old' }, { path: 'b c' }]]) cases.push({ fn: 'nul', args: [items] });
for (const b of [{ name: 'main' }, { name: 'origin/x', remote: true }, { name: '-x' }]) cases.push({ fn: 'fullRef', args: [b] });
const safe = (local, global, key) => {
  for (const s of [true, false]) cases.push({ fn: 'safeGit', args: ['$ROOT', local, global, key, s], env: ENV });
};
safe([['core.sshCommand', 'evil']], [], 'core.sshcommand');
safe([['core.sshCommand', 'evil']], [['core.sshCommand', 'ssh -i k']], 'core.sshcommand');
safe([['credential.helper', 'evil']], [['credential.helper', 'store'], ['credential.helper', 'cache']], 'credential.helper');
safe([['credential.https://x.helper', 'evil']], [], 'credential.https://x.helper');
safe([['gpg.program', 'evil']], [], 'gpg.program');
safe([['gpg.ssh.defaultKeyCommand', 'evil']], [], 'gpg.ssh.defaultkeycommand');
safe([['merge.x.driver', 'evil']], [], 'merge.x.driver');
safe([['core.askpass', 'evil']], [['core.askpass', 'mine']], 'core.askpass');
safe([], [['core.sshCommand', 'ssh -v']], 'core.sshcommand');
safe([], [], 'protocol.ext.allow');

const impl = {
  english: e => state.english({ ...e }),
  nul: items => state.nul(items),
  fullRef: b => state.fullRef(b),
  safeGit,
};
module.exports = { module: 'scm/state', cases, impl };
