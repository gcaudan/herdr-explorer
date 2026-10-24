'use strict';
// core/exec's state files: the JS reads the state and config folders when the module loads, so each call loads it
// again, under the case's environment. readKeys's Map is given as its [key, action] pairs.
const path = require('path');

const src = path.join(__dirname, '..', '..', '..', 'src');
const fresh = () => {
  for (const k of Object.keys(require.cache)) if (k.startsWith(src)) delete require.cache[k];
  return require(path.join(src, 'core', 'exec'));
};
const impl = {
  readWidth: () => fresh().readWidth(),
  readSettings: () => fresh().readSettings(),
  writeSettings: s => fresh().writeSettings(s),
  paneFile: p => fresh().paneFile(p),
  readPaneState: p => fresh().readPaneState(p),
  writePaneState: (p, d) => fresh().writePaneState(p, d),
  tabFile: (k, t) => fresh().tabFile(k, t),
  readKeys: (p, d) => [...fresh().readKeys(p, d)],
  keysTemplate: (p, d) => fresh().keysTemplate(p, d),
  writeKeys: (p, c) => fresh().writeKeys(p, c),
  writeAtomic: (f, t) => fresh().writeAtomic(f, t),
};

const state = { HERDR_PLUGIN_STATE_DIR: '$ROOT' };
const config = { HERDR_PLUGIN_CONFIG_DIR: '$ROOT' };
const DEFAULTS = { newFile: ['a'], del: ['d', 'delete'], open: ['enter'] };
const keys = (fn, text, args, more = {}) => ({ fn, args, env: config, ...(text !== undefined ? { files: { 'keys.json': text } } : {}), ...more });

module.exports = {
  module: 'core/exec.state',
  impl,
  cases: [
    ...[undefined, '50', '5', 'abc', '  42px', '-7', '1e3'].map(w => ({ fn: 'readWidth', args: [], env: state, ...(w !== undefined ? { files: { width: w } } : {}) })),
    { fn: 'readWidth', args: [] },
    ...[undefined, '{"side":"right"}', 'not json', '[1,2]', '"ab"', '5', 'null', '{"theme":"dark","x":1,"7":2,"hidden":false}']
      .map(t => ({ fn: 'readSettings', args: [], env: state, ...(t !== undefined ? { files: { 'settings.json': t } } : {}) })),
    { fn: 'readSettings', args: [], files: { 'settings.json': '{"side":"right"}' } },
    { fn: 'writeSettings', args: [{ a: 1, b: [1, { c: 'é' }], d: {} }], env: state, after: { 'settings.json': null } },
    { fn: 'writeSettings', args: [{ a: 1 }], env: { HERDR_PLUGIN_STATE_DIR: '$ROOT/new/dir' }, after: { 'new/dir/settings.json': null } },
    ...['w1:p2', '', 'a/b c.é', 'x😀'].map(p => ({ fn: 'paneFile', args: [p], env: state })),
    { fn: 'paneFile', args: ['w1:p2'] },
    { fn: 'readPaneState', args: ['w1:p2'], env: state, files: { 'panes/w1_p2.json': '{"root":"/x"}' } },
    { fn: 'readPaneState', args: ['w1:p2'], env: state, files: { 'panes/w1_p2.json': '{' } },
    { fn: 'readPaneState', args: ['w1:p9'], env: state },
    { fn: 'writePaneState', args: ['w1:p2', { root: '/x', n: 1.5 }], env: state, after: { 'panes/w1_p2.json': null } },
    ...[['notes', 'w1:t1'], ['scope', ''], ['x', 'é😀-a_b.c']].map(args => ({ fn: 'tabFile', args, env: state })),
    { fn: 'tabFile', args: ['notes', 'w1:t1'] },
    keys('readKeys', undefined, ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":{"newFile":["n"],"del":"x","bogus":["q"]}}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":{"newFile":["d"]}}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":{"newFile":[1,"z",null]}}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":null}', ['explorer', DEFAULTS]),
    keys('readKeys', 'nope', ['explorer', DEFAULTS]),
    keys('readKeys', 'null', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":[["a"]]}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":"str"}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":5}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":0}', ['explorer', DEFAULTS]),
    keys('readKeys', '{"explorer":"str"}', ['explorer', {}]),
    { fn: 'readKeys', args: ['explorer', DEFAULTS], files: { 'keys.json': '{"explorer":{"newFile":["n"]}}' } },
    ...[undefined, '{"explorer":{"newFile":["a"],"del":["d","delete"],"open":["enter"]}}', '{"explorer":{"del":["x"]}}', 'nope',
      '{"other":{"x":["y"]}}', '[1]', '{"explorer":{"zz":1},"other":"ab"}', 'null', '5', '"s"', '{"explorer":null}']
      .map(t => keys('keysTemplate', t, ['explorer', DEFAULTS], { after: { 'keys.json': null } })),
    { fn: 'keysTemplate', args: ['explorer', DEFAULTS] },
    ...[undefined, '{"explorer":{"del":["x"],"open":["o"]},"other":{"y":["z"]}}', 'nope', 'null', '{"explorer":null,"b":null}', '[]']
      .map(t => keys('writeKeys', t, ['explorer', { newFile: ['n', 'N'], open: null }], { after: { 'keys.json': null } })),
    { fn: 'writeKeys', args: ['explorer', { newFile: ['n'] }] },
    { fn: 'writeAtomic', args: ['$ROOT/a/b/c.txt', 'hi'], after: { 'a/b/c.txt': null } },
    { fn: 'writeAtomic', args: ['$ROOT/f', 'new'], files: { f: 'old' }, after: { f: null } },
    { fn: 'writeAtomic', args: ['$ROOT/f/g', 'x'], files: { f: 'a file' }, after: { 'f/g': null } },
  ],
};
