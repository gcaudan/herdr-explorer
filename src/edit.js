// Terminal editor on a file, in a full-tab pane opened from the explorer or the preview: the editor of the
// settings when it needs a terminal, $VISUAL, $EDITOR, else the first of nano, nvim, vim, vi (edit on Windows).
// The pane closes when the editor exits.
'use strict';
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { WIN, which, words } = require('./core/exec');

const file = process.env.EXPLORER_FILE;

// Absolute path of a program, or null when missing: found in the absolute folders of PATH only, as every program
// the plugin runs (an empty or '.' entry of PATH would find one planted in the folder of the pane). A program given
// with its folder must exist, else the next editor is tried.
function find(name) {
  if (!/[\\/]/.test(name)) return which(name);
  if (!path.isAbsolute(name)) return null;
  for (const full of WIN && !path.extname(name) ? [name, name + '.exe'] : [name]) {
    try { if (fs.statSync(full).isFile()) { if (!WIN) fs.accessSync(full, fs.constants.X_OK); return full; } } catch { }
  }
  return null;
}

function editor() {
  for (const v of [process.env.EXPLORER_EDITOR, process.env.VISUAL, process.env.EDITOR]) {
    const [cmd, ...args] = words(v || '');
    const found = cmd && find(cmd);
    if (found) return [found, args];
  }
  for (const name of WIN ? ['edit', 'nvim', 'vim', 'nano'] : ['nano', 'nvim', 'vim', 'vi']) {
    const found = find(name);
    if (found) return [found, []];
  }
  return null;
}

const found = file && editor();
if (!found) {
  process.stdout.write(file ? 'No terminal editor found: set $EDITOR.\r\n' : 'No file.\r\n');
  setTimeout(() => process.exit(1), 3000);
} else {
  const res = spawnSync(found[0], [...found[1], file], { stdio: 'inherit' });
  if (res.error) {
    process.stdout.write(`Cannot start ${found[0]}: ${res.error.code}\r\n`);
    setTimeout(() => process.exit(1), 3000);
  } else process.exit(res.status || 0);
}
