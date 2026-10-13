// Terminal editor on a file, in a full-tab pane opened from the explorer or the preview: the editor of the
// settings when it needs a terminal, $VISUAL, $EDITOR, else the first of nano, nvim, vim, vi (edit on Windows).
// The pane closes when the editor exits.
'use strict';
const { spawnSync } = require('child_process');
const { WIN, which, words } = require('./exec');

const file = process.env.EXPLORER_FILE;

// Absolute path of a program, or null when missing.
const find = name => (WIN ? which(name)
  : spawnSync('sh', ['-c', 'command -v -- "$1"', 'sh', name], { encoding: 'utf8' }).stdout.trim() || null);

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
