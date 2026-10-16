// Shows a file in the preview of the tab it runs in: for an agent, a script or a key binding.
//   node peek.js <file>[:<places>] the file, scrolled to the first place and the places highlighted: 12, 12-14,
//                                 12:5-18 (columns of line 12), several separated by commas (12-14,30,42:5-18)
//   node peek.js --last           the changed file of the repository written last (the peek action)
//   node peek.js --diff …         the same, showing the file's git changes (the preview's Diff view) at the places
// Run from a herdr pane (an agent's terminal): the preview of that pane's tab shows the file, opened next to it
// when there is none. Prints what it showed, or why not, with exit code 1 (2: wrong use).
'use strict';
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const fail = (text, code = 1) => { console.error(`peek: ${text}`); process.exit(code); };
const HERDR_BIN = process.env.HERDR_BIN_PATH || 'herdr';
const herdr = (...args) => { try { return JSON.parse(execFileSync(HERDR_BIN, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })).result; } catch { return null; } };

// The plugin's state folder, where the preview reads its requests: herdr gives it to the plugin's own processes only.
// From another pane, the one the hook saved in the plugin's config folder (state-dir), else herdr's usual place.
if (!process.env.HERDR_PLUGIN_STATE_DIR) {
  const id = process.env.HERDR_PLUGIN_ID || 'gcaudan.explorer';
  let config = '';
  try { config = execFileSync(HERDR_BIN, ['plugin', 'config-dir', id], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { }
  let dir = '';
  try { dir = fs.readFileSync(path.join(config, 'state-dir'), 'utf8').trim(); } catch { }
  if (!dir) {
    const base = process.platform === 'win32' ? process.env.LOCALAPPDATA || ''
      : process.env.XDG_STATE_HOME || path.join(require('os').homedir(), '.local', 'state');
    dir = path.join(base, 'herdr', 'plugins', id);
  }
  if (!fs.existsSync(dir)) fail('cannot find the plugin\'s state folder: open a new tab once so that the explorer saves it');
  process.env.HERDR_PLUGIN_STATE_DIR = dir;
}
// Loaded once the state folder is set: the settings' path depends on it.
const actions = require('./core/actions');
const { GIT, gitEnv, run } = require('./core/exec');

// The pane it runs from, and its folder. As the peek action, the tab's focused pane, or its first terminal when the
// focus is on one of the plugin's panes.
const OWN = new Set(['Explorer', 'Preview', 'Source Control', 'Changes', 'Editor', 'Search']);
let pane = process.env.HERDR_PANE_ID, cwd = process.cwd();
if (process.env.HERDR_PLUGIN_CONTEXT_JSON) {
  let ctx = {};
  try { ctx = JSON.parse(process.env.HERDR_PLUGIN_CONTEXT_JSON); } catch { }
  const list = (ctx.workspace_id && herdr('pane', 'list', '--workspace', ctx.workspace_id)) || { panes: [] };
  const panes = list.panes.filter(p => p.tab_id === ctx.tab_id);
  const focused = panes.find(p => p.pane_id === ctx.focused_pane_id);
  const terminal = focused && !OWN.has(focused.label) ? focused : panes.find(p => !OWN.has(p.label));
  pane = terminal ? terminal.pane_id : ctx.focused_pane_id;
  cwd = (terminal && terminal.cwd) || ctx.workspace_cwd || cwd;
}
if (!pane) fail('run it from a herdr pane', 2);

// The changed file of the repository written last: modified, added or untracked, still on disk.
async function last() {
  const env = await gitEnv(cwd);
  const top = (await run(GIT, ['rev-parse', '--show-toplevel'], { cwd, env }) || '').trim();
  if (!top) fail('not in a git repository');
  const out = await run(GIT, ['status', '--porcelain=v1', '-z', '--untracked-files=all'], { cwd: top, env });
  if (out === null) fail('git status failed');
  let best = null;
  const parts = out.split('\0');
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (p.length < 4) continue;
    if ('RC'.includes(p[0])) i++; // the original path follows
    const abs = path.join(top, p.slice(3));
    let stat;
    try { stat = fs.statSync(abs); } catch { continue; }
    if (stat.isFile() && (!best || stat.mtimeMs > best.time)) best = { abs, time: stat.mtimeMs };
  }
  if (!best) fail('no changed file');
  return [best.abs, 0];
}

// file[:places]: places are separated by commas, each a line (12), lines (12-14), or columns of a line (12:5-18,
// counted in characters from 1); 12:5 is line 12, as compilers and grep write it. A name that exists as given is
// taken whole, so that a colon in it is not read as places. Returns [file, marks].
const PLACE = /^(\d+)(?:-(\d+))?(?::(\d+)(?:-(\d+))?)?$/;
function target(arg) {
  const whole = path.resolve(cwd, arg);
  const at = arg.indexOf(':', /^[a-z]:[\\/]/i.test(arg) ? 2 : 0); // past a Windows drive
  if (fs.existsSync(whole) || at < 1) return [whole, []];
  const marks = [];
  for (const place of arg.slice(at + 1).split(',')) {
    const m = PLACE.exec(place);
    if (!m) return [whole, []];
    const from = +m[1], to = m[2] ? +m[2] : from;
    // Columns only on one line, a range of them only (a single column is a position, not a part).
    const cols = !m[2] && m[4];
    if (!from || to < from || (cols && (!+m[3] || +m[4] < +m[3]))) fail(`wrong place: ${place}`, 2);
    marks.push({ from, to, a: cols ? +m[3] : 0, b: cols ? +m[4] : 0 });
  }
  return [path.resolve(cwd, arg.slice(0, at)), marks];
}

(async () => {
  const args = process.argv.slice(2);
  // --diff: the file's git changes, at the places, rather than the file.
  const diff = args[0] === '--diff' && args.length > 1;
  if (diff) args.shift();
  if (args.length !== 1 || args[0] === '--help' || args[0] === '-h') fail('usage: peek.js [--diff] <file>[:<line>[-<line>][:<col>-<col>],…] | [--diff] --last', 2);
  const [abs, marks] = args[0] === '--last' ? [(await last())[0], []] : target(args[0]);
  const line = marks.length ? marks[0].from : 0;
  // A deleted file still has changes to show.
  let stat = null;
  try { stat = fs.statSync(abs); } catch { if (!diff) fail(`no such file: ${abs}`); }
  if (stat && !stat.isFile()) fail(`not a file: ${abs}`);
  const err = await actions.preview(abs, cwd, pane, line, false, true, { marks, view: diff ? 'diff' : 'file' });
  if (err) fail(err);
  const places = marks.map(m => `${m.from}${m.to > m.from ? `-${m.to}` : ''}${m.a ? `:${m.a}-${m.b}` : ''}`).join(',');
  console.log(`Preview${diff ? ' (diff)' : ''}: ${actions.relativeTo(cwd, abs)}${places ? `:${places}` : ''}`);
})();
