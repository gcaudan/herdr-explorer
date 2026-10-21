// Finding programs in the absolute folders of PATH, without loading exec.js (which reads the state folder at load).
'use strict';
const fs = require('fs');
const path = require('path');

const WIN = process.platform === 'win32';

// Absolute folders of PATH (relative entries would depend on the working directory).
const pathDirs = () => (process.env.PATH || '').split(path.delimiter)
  .map(d => d.replace(/^"(.*)"$/, '$1')).filter(d => path.isAbsolute(d));

// Absolute path of a program found in the absolute folders of PATH, or null. On Windows, a bare name is looked up
// in the child's working directory before PATH; elsewhere, an empty or '.' entry of PATH means that folder too: a git
// planted in an untrusted repository would run. Resolved first, on every system.
function which(name) {
  if (path.isAbsolute(name)) return name;
  const file = WIN && !path.extname(name) ? name + '.exe' : name;
  for (const dir of pathDirs()) {
    const full = path.join(dir, file);
    try {
      if (fs.statSync(full).isFile()) { if (!WIN) fs.accessSync(full, fs.constants.X_OK); return full; }
    } catch { }
  }
  return null;
}

module.exports = { pathDirs, which };
