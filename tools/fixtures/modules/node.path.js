'use strict';
// Node's path module, posix and win32, as the JS calls it: the Rust port (core::paths) must give the same strings.
// Resolving relative paths depends on the working directory: win32 is given absolute ones only.
const path = require('path');

const impl = {};
for (const os of ['posix', 'win32']) {
  const p = path[os];
  Object.assign(impl, {
    [`${os}.resolve`]: (...a) => p.resolve(...a),
    [`${os}.normalize`]: a => p.normalize(a),
    [`${os}.join`]: (...a) => p.join(...a),
    [`${os}.relative`]: (a, b) => p.relative(a, b),
    [`${os}.dirname`]: a => p.dirname(a),
    [`${os}.basename`]: (a, s) => (s === undefined ? p.basename(a) : p.basename(a, s)),
    [`${os}.extname`]: a => p.extname(a),
    [`${os}.isAbsolute`]: a => p.isAbsolute(a),
    [`${os}.root`]: a => p.parse(a).root,
  });
}

const posix = ['', '.', '..', '/', '//', '///a', 'a', 'a/', 'a//b', '/a/b/', '/a/./b/../c', '../../x', 'a/../../b', './a/.',
  '/..', '/../a', 'a/b/../../..', '.bashrc', 'a.b.c', 'a.', '..x', 'dir/.x.y', 'é/ü.txt', '/x/..y/z', 'a/.../b'];
const win = ['', '.', 'C:', 'C:\\', 'c:/a/b', 'C:a\\b', '\\', '/', '\\a\\..\\..\\b', '\\\\server\\share', '\\\\server\\share\\',
  '\\\\server\\share\\a\\..\\b', '\\\\?\\C:\\x\\y', '\\\\.\\pipe\\p', '//server/share/x', 'C:\\a\\b\\..\\..\\..', 'a\\b/c\\..',
  'C:\\a.b\\c.d', 'C:\\x\\', '\\\\server', '\\\\\\x', 'D:\\Été\\ü.TXT'];

const each = (os, fns, inputs) => fns.flatMap(fn => inputs.map(a => ({ fn: `${os}.${fn}`, args: [a] })));
module.exports = {
  module: 'node/path',
  impl,
  cases: [
    ...each('posix', ['normalize', 'dirname', 'basename', 'extname', 'isAbsolute', 'root'], posix),
    ...posix.filter(a => a.startsWith('/')).map(a => ({ fn: 'posix.resolve', args: [a] })),
    ...[['/a', 'b', '../c'], ['/a', '/b', 'c/'], ['/', ''], ['/x', '..', '..', 'y'], ['/a/b', './']]
      .map(args => ({ fn: 'posix.resolve', args })),
    ...[['a', 'b'], ['/a', '', 'b/'], ['', ''], ['a', '../..', 'b'], ['/', '..'], ['a', '/b']].map(args => ({ fn: 'posix.join', args })),
    ...[['/a/b', '/a/b/c'], ['/a/b/c', '/a/b'], ['/a/b', '/a/x'], ['/', '/a'], ['/a', '/'], ['/a/b', '/a/b'], ['/ab', '/a'],
      ['/a', '/ab'], ['/x/y/z', '/q'], ['/a/', '/a/b/']].map(args => ({ fn: 'posix.relative', args })),
    { fn: 'posix.basename', args: ['/a/b.js', '.js'] },
    { fn: 'posix.basename', args: ['/a/.js', '.js'] },
    { fn: 'posix.basename', args: ['bbb', 'bbb'] },
    ...each('win32', ['normalize', 'dirname', 'basename', 'extname', 'isAbsolute', 'root'], win),
    ...win.filter(a => path.win32.isAbsolute(a) && /^([a-z]:|\\\\[^\\])/i.test(a)).map(a => ({ fn: 'win32.resolve', args: [a] })),
    ...[['C:\\a', 'b', '..\\c'], ['C:\\a', 'D:\\b'], ['C:\\a', '\\b'], ['\\\\s\\sh', 'x', '..', '..'], ['C:\\', 'x/y/']]
      .map(args => ({ fn: 'win32.resolve', args })),
    ...[['a', 'b'], ['\\\\server', 'share'], ['\\\\\\a', 'b'], ['C:', 'x'], ['', ''], ['//a', 'b'], ['/', '/x']]
      .map(args => ({ fn: 'win32.join', args })),
    // Node 26: a colon in a relative path, device roots, reserved device names (CON, COM1…).
    ...['ab.cc:', 'a:b', 'x/c:', 'x\\c:\\y', 'ab:/x', '\\\\.\\x', '\\\\?\\x\\', '\\\\?\\COM1:\\a', '\\\\.\\nul:',
      'CON:', 'con:x', 'COM1:\\a\\..', 'CONx', 'NULé', 'COM\xb9:', 'x\\CON:', 'a\\..\\C:x']
      .flatMap(a => [{ fn: 'win32.normalize', args: [a] }, { fn: 'win32.join', args: [a, 'b'] }]),
    ...[['a/c://'], ['\\\\.', '/.b.ca'], ['a', 'CON:x'], ['x', 'prn:', 'y/z'], ['\\\\?', 'COM1:']].map(args => ({ fn: 'win32.join', args })),
    ...[['C:\\a\\b', 'C:\\a\\b\\c'], ['C:\\A\\b', 'c:\\a\\B\\x'], ['C:\\a', 'D:\\a'], ['C:\\a\\b', 'C:\\a\\x'], ['C:\\', 'C:\\x'],
      ['C:\\x', 'C:\\'], ['\\\\s\\sh\\a', '\\\\s\\sh\\b'], ['\\\\s\\sh', '\\\\t\\sh'], ['C:\\Été', 'c:\\été\\x']]
      .map(args => ({ fn: 'win32.relative', args })),
  ],
};
