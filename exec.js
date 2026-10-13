// Running external programs safely, and helpers shared by the plugin's scripts.
'use strict';
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');

const WIN = process.platform === 'win32';
const PLUGIN = process.env.HERDR_PLUGIN_ID || 'gcaudan.explorer';

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

// Command line split at spaces outside quotes: "code --wait" → ['code', '--wait'].
const words = s => (String(s).match(/"[^"]*"|'[^']*'|\S+/g) || []).map(w => w.replace(/^(["'])(.*)\1$/, '$2'));

// Runs a program and resolves to its stdout, or null on failure (including a missing program).
const run = (cmd, args, opts = {}) => new Promise(resolve => {
  if (!cmd) return resolve(null);
  execFile(cmd, args, { windowsHide: true, maxBuffer: 64 << 20, ...opts }, (err, stdout) => resolve(err ? null : stdout));
});

// The result of a herdr command's JSON answer, or null.
function result(stdout) {
  try { return JSON.parse(stdout).result; } catch { return null; }
}

const HERDR = which(process.env.HERDR_BIN_PATH || 'herdr');
const GIT = which('git');

// Hooks folder that never exists. On Windows, /dev/null would mean C:\dev\null, a folder any user can create.
const NO_HOOKS = WIN ? path.join(__dirname, '.no-hooks') : '/dev/null';

// Environment for every git call in a repository that may not be trusted, whose config could run commands:
// - core.fsmonitor and hooks are disabled: hook files through core.hooksPath, and the hooks the repository defines
//   in its config (hook.<name>.command) one by one;
// - so are the filters (clean, smudge, process) the repository declares itself, in .git/config, config.worktree or
//   an included file; global ones, such as git-lfs, are kept;
// - no lazy fetching in partial clones, and a core.worktree set by the repository is replaced by the folder holding
//   it;
// - submodules have their own config, out of reach of this one: git never recurses into them (switch, restore,
//   fetch, pull, push), and callers pass --ignore-submodules=all.
// Passed as GIT_CONFIG_KEY_n / GIT_CONFIG_VALUE_n, which, unlike -c, does not split a driver name containing '='.
// GIT_OPTIONAL_LOCKS=0 keeps git status from taking index.lock, which would make a concurrent git commit fail.
async function gitEnv(cwd) {
  // log.showSignature would run the repository's gpg.program on every signed commit shown.
  const config = [['core.fsmonitor', 'false'], ['core.hooksPath', NO_HOOKS], ['log.showSignature', 'false'],
    ['submodule.recurse', 'false'], ['fetch.recurseSubmodules', 'false'], ['push.recurseSubmodules', 'no']];
  const listing = await run(GIT, ['config', '-z', '--includes', '--show-scope', '--name-only', '--get-regexp',
    '^((filter|hook)\\..+|core\\.worktree)$'], { cwd, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' } }) || '';
  const fields = listing.split('\0');
  const drivers = new Set(), hooks = new Set();
  let worktree = false;
  for (let i = 0; i + 1 < fields.length; i += 2) {
    if (fields[i] === 'system' || fields[i] === 'global') continue;
    if (fields[i + 1] === 'core.worktree') { worktree = true; continue; }
    const m = /^(filter|hook)\.(.+)\.[^.]+$/s.exec(fields[i + 1]);
    if (m) (m[1] === 'filter' ? drivers : hooks).add(m[2]);
  }
  for (const d of drivers) {
    for (const k of ['clean', 'smudge', 'process']) config.push([`filter.${d}.${k}`, '']);
    config.push([`filter.${d}.required`, 'false']);
  }
  for (const h of hooks) config.push([`hook.${h}.enabled`, 'false']);
  // GIT_NO_LAZY_FETCH: in a partial clone, a missing object would make git fetch it from the promisor remote,
  // running that remote's upload-pack or ssh command, even for git status.
  const env = { ...process.env, GIT_OPTIONAL_LOCKS: '0', GIT_NO_LAZY_FETCH: '1' };
  let n = parseInt(env.GIT_CONFIG_COUNT, 10) || 0;
  for (const [key, value] of config) {
    env[`GIT_CONFIG_KEY_${n}`] = key;
    env[`GIT_CONFIG_VALUE_${n++}`] = value;
  }
  env.GIT_CONFIG_COUNT = String(n);
  // core.worktree set by the repository could point anywhere (the home folder): switch, pull or discard would write
  // there. The working tree is kept to the folder holding the repository.
  if (worktree) {
    const dir = (await run(GIT, ['rev-parse', '--absolute-git-dir'], { cwd, env }) || '').trim();
    env.GIT_WORK_TREE = dir ? path.dirname(dir) : cwd;
  }
  return env;
}

// Operation under way in a repository, from the files git keeps in its folder: 'MERGING', 'REBASING'… or ''.
// rebase-apply is also git am's (applying); .git/sequencer alone: a cherry-pick or revert of several commits,
// between two of them once a commit made by hand ended one.
function gitOperation(gitdir) {
  const has = name => fs.existsSync(path.join(gitdir, name));
  const next = () => { try { return fs.readFileSync(path.join(gitdir, 'sequencer', 'todo'), 'utf8'); } catch { return ''; } };
  return has('MERGE_HEAD') ? 'MERGING' : has('rebase-apply/applying') ? 'APPLYING'
    : has('rebase-merge') || has('rebase-apply') ? 'REBASING'
    : has('CHERRY_PICK_HEAD') ? 'CHERRY-PICKING' : has('REVERT_HEAD') ? 'REVERTING'
    : has('sequencer') ? (/^\s*revert\s/.test(next()) ? 'REVERTING' : 'CHERRY-PICKING')
    : has('BISECT_LOG') ? 'BISECTING' : '';
}

// Dotted versions compared number by number: newer('1.10.0', '1.9.2') is true.
const version = v => String(v).split('.').map(n => parseInt(n, 10) || 0);
function newer(a, b) {
  const [x, y] = [version(a), version(b)];
  const i = x.findIndex((n, j) => n !== (y[j] || 0));
  return i >= 0 && x[i] > (y[i] || 0);
}

// Pane width remembered by the explorer (width file in the state folder), 32 columns by default.
const WIDTH_FILE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'width');
function readWidth() {
  try { return Math.max(10, parseInt(fs.readFileSync(WIDTH_FILE, 'utf8'), 10)) || 32; } catch { return 32; }
}

// User settings, shared by the explorer, the preview and the hook (settings.json in the state folder).
const DEFAULTS = {
  hidden: true,       // show dot files
  ignored: true,      // show git-ignored files, greyed out
  open: 'preview',    // Enter and double-click on a text file: 'preview' or 'editor'
  editor: '',         // external editor command ("zed", "subl -w"); empty: VS Code when found, else the terminal editor
  singleClick: false, // a single click on a file opens the preview, like VS Code
  side: 'left',       // explorer docked to the left or the right of the tab
  follow: false,      // reveal the folder of the terminal next to the explorer when it changes
  icons: '',          // explorer icon theme: { id, label, file } of a VS Code one; empty: Nerd Font icons
  wrap: false,        // preview: wrap long lines
  lineNumbers: true,  // preview: line number gutter
  updateCheck: true,  // check GitHub once a day for a newer release
};
const SETTINGS_FILE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'settings.json');
function readSettings() {
  try { return { ...DEFAULTS, ...JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8')) }; } catch { return { ...DEFAULTS }; }
}
// Written to a temporary file then renamed, so that another pane never reads it half-written.
function writeSettings(settings) {
  if (!SETTINGS_FILE) return;
  const tmp = `${SETTINGS_FILE}.${process.pid}`;
  try { fs.writeFileSync(tmp, JSON.stringify(settings, null, 1)); fs.renameSync(tmp, SETTINGS_FILE); } catch { try { fs.unlinkSync(tmp); } catch { } }
}

// What each plugin pane shows, one file per pane in the state folder (panes/), since herdr does not keep it across a
// restart: the explorer's space folder and tree root, which Set as root may have moved; the preview's file.
const PANES_DIR = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'panes');
const paneFile = pane => PANES_DIR && pane && path.join(PANES_DIR, `${pane.replace(/[^\w.-]/g, '_')}.json`);
function readPaneState(pane) {
  try { return JSON.parse(fs.readFileSync(paneFile(pane), 'utf8')); } catch { return null; }
}
function writePaneState(pane, data) {
  const file = paneFile(pane);
  if (!file) return;
  const tmp = `${file}.${process.pid}`;
  try {
    fs.mkdirSync(PANES_DIR, { recursive: true });
    fs.writeFileSync(tmp, JSON.stringify(data)); fs.renameSync(tmp, file);
  } catch { try { fs.unlinkSync(tmp); } catch { } }
}

// Key bindings, editable by the user: keys.json in the plugin config folder, one object per pane, mapping an action
// to its keys ({ "explorer": { "newFile": ["a"] } }). An action left out keeps its default keys.
const KEYS_FILE = process.env.HERDR_PLUGIN_CONFIG_DIR && path.join(process.env.HERDR_PLUGIN_CONFIG_DIR, 'keys.json');

// Key name → action. A key the user gives to an action is taken from the action that had it by default.
function readKeys(pane, defaults) {
  let user = {};
  try { user = JSON.parse(fs.readFileSync(KEYS_FILE, 'utf8'))[pane] || {}; } catch { }
  const map = new Map();
  for (const [action, keys] of Object.entries(defaults)) if (!(action in user)) for (const k of keys) map.set(k, action);
  for (const [action, keys] of Object.entries(user)) {
    if (action in defaults) for (const k of [].concat(keys)) if (typeof k === 'string') map.set(k, action);
  }
  return map;
}

// The file with every action of the pane and its current keys, created or completed so that the user edits a full
// list; returns its path.
function keysTemplate(pane, defaults) {
  if (!KEYS_FILE) return null;
  let all = {};
  try { all = JSON.parse(fs.readFileSync(KEYS_FILE, 'utf8')); } catch (e) { if (e.code !== 'ENOENT') return KEYS_FILE; }
  const keys = { ...defaults, ...(all[pane] || {}) };
  if (JSON.stringify(all[pane]) === JSON.stringify(keys)) return KEYS_FILE;
  all[pane] = keys;
  const lines = Object.entries(all).map(([p, actions]) => `  ${JSON.stringify(p)}: {\n`
    + Object.entries(actions).map(([a, k]) => `    ${JSON.stringify(a)}: ${JSON.stringify(k)}`).join(',\n') + '\n  }');
  try { fs.mkdirSync(path.dirname(KEYS_FILE), { recursive: true }); fs.writeFileSync(KEYS_FILE, `{\n${lines.join(',\n')}\n}\n`); } catch { }
  return KEYS_FILE;
}

module.exports = {
  WIN, PLUGIN, pathDirs, which, words, run, result, HERDR, GIT, gitEnv, gitOperation, version, newer, WIDTH_FILE, readWidth,
  SETTINGS_FILE, readSettings, writeSettings, PANES_DIR, paneFile, readPaneState, writePaneState, KEYS_FILE, readKeys, keysTemplate,
};
