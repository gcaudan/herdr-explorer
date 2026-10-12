// Git changes of a file or folder, opened from the explorer menu in a temporary pane:
// tracked files compared with HEAD (index and working tree), untracked files in full.
// Scroll with arrows, Page keys or the wheel; q or Esc closes the pane.
'use strict';
const path = require('path');
const { spawnSync } = require('child_process');
const { GIT, gitEnv } = require('./exec');

const file = process.env.EXPLORER_FILE;
const cwd = process.env.EXPLORER_ROOT || path.dirname(file);
const out = process.stdout;
const MAX_UNTRACKED = 200;

async function changes() {
  const env = await gitEnv(cwd);
  // stdout even on a non-zero exit code: diff --no-index returns 1 when the files differ.
  const git = (...args) => (GIT && spawnSync(GIT, ['-c', 'color.ui=never', ...args],
    { cwd, env, encoding: 'utf8', maxBuffer: 64 << 20, windowsHide: true }).stdout) || '';
  // External diff tools and textconv filters come from the repository config: not run.
  const diff = (...args) => git('diff', '--no-ext-diff', '--no-textconv', '--ignore-submodules=all', ...args);

  // Without any commit, compare with the empty tree: staged changes, then unstaged ones.
  let text = git('rev-parse', '--verify', '-q', 'HEAD').trim() ? diff('HEAD', '--', file)
    : diff('--cached', '--', file) + diff('--', file);
  const untracked = git('ls-files', '--others', '--exclude-standard', '-z', '--', file).split('\0').filter(Boolean);
  for (const f of untracked.slice(0, MAX_UNTRACKED)) text += diff('--no-index', '--', '/dev/null', f);
  if (untracked.length > MAX_UNTRACKED) text += `\n… and ${untracked.length - MAX_UNTRACKED} more untracked files\n`;
  return text;
}

// Control characters from the files are dropped, then lines are coloured here rather than by git, so that a
// file cannot hide its own changes with escape sequences.
const clean = t => t.replace(/[\x00-\x08\x0a-\x1f\x7f-\x9f]/g, '');
const HEADER = /^(diff |index |--- |\+\+\+ |new file|deleted file|old mode|new mode|similarity|rename |Binary )/;
const colour = l => HEADER.test(l) ? `\x1b[1m${l}` : l.startsWith('@@') ? `\x1b[36m${l}`
  : l.startsWith('+') ? `\x1b[32m${l}` : l.startsWith('-') ? `\x1b[31m${l}` : l;
let lines = ['\x1b[2mLoading…'];
const title = clean(` ${path.relative(cwd, file) || path.basename(file)}  ·  q or Esc: close`);
let top = 0;

function render() {
  const H = (out.rows || 24) - 1, W = out.columns || 80;
  top = Math.max(0, Math.min(top, lines.length - H));
  const head = `\x1b[7m${Array.from(title).slice(0, W).join('').padEnd(W)}\x1b[0m`;
  const body = lines.slice(top, top + H).map(l => l + '\x1b[0m\x1b[K');
  while (body.length < H) body.push('\x1b[K');
  out.write('\x1b[H' + [head, ...body].join('\r\n'));
}

function scroll(n) { top += n; render(); }

out.write('\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?1000h\x1b[?1006h');
process.on('exit', () => out.write('\x1b[?1006l\x1b[?1000l\x1b[?7h\x1b[?25h\x1b[?1049l'));
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stdin.on('data', data => {
  const page = (out.rows || 24) - 2;
  for (const m of data.matchAll(/\x1b\[<(\d+);\d+;\d+[Mm]|\x1b\[[\d;]*[A-Za-z~]|\x1bO[A-Za-z]|\x1b[^\x1b]|[\s\S]/g)) {
    const k = m[0];
    if (m[1] === '64') scroll(-3);
    else if (m[1] === '65') scroll(3);
    else if (k === 'q' || k === '\x1b' || k === '\x03') process.exit(0);
    else if (k === '\x1b[A' || k === '\x1bOA' || k === 'k') scroll(-1);
    else if (k === '\x1b[B' || k === '\x1bOB' || k === 'j' || k === '\r') scroll(1);
    else if (k === '\x1b[5~' || k === 'b') scroll(-page);
    else if (k === '\x1b[6~' || k === ' ') scroll(page);
    else if (k === '\x1b[H' || k === '\x1b[1~' || k === 'g') { top = 0; render(); }
    else if (k === '\x1b[F' || k === '\x1b[4~' || k === 'G') { top = lines.length; render(); }
  }
});
out.on('resize', render);
render();
changes().then(text => {
  lines = text.trim() ? text.split('\n').map(l => colour(clean(l))) : ['\x1b[2m(no changes)'];
  render();
});
