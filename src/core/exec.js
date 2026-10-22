// Running external programs safely, and helpers shared by the plugin's scripts.
'use strict';
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');

const WIN = process.platform === 'win32';
const PLUGIN = process.env.HERDR_PLUGIN_ID || 'gcaudan.explorer';

// pathDirs and which live apart so that peek.js can find herdr before the state folder this module reads is set.
const { pathDirs, which } = require('./which');

// A path that reaches a network share or a device through a symbolic link (or junction) on its way: a link to
// \\host\share, \\?\… or \\.\…. Opening it would reach the network, which on Windows sends the user's credentials,
// and a repository can hold such a link. The links are read (readlink), never followed; the folders of top (the
// tree's root, trusted) and above are not looked at. A loop of links counts as remote.
function remoteLink(p, top = '') {
  const trusted = top && path.resolve(top).toLowerCase() + path.sep;
  let cur = path.resolve(p);
  for (let hops = 0; hops <= 32; hops++) {
    const { root } = path.parse(cur), parts = cur.slice(root.length).split(path.sep).filter(Boolean);
    let next = null;
    for (let i = 0, at = root; i < parts.length && !next; i++) {
      at = path.join(at, parts[i]);
      if (trusted && trusted.startsWith(at.toLowerCase() + path.sep)) continue;
      let target;
      try { target = fs.lstatSync(at).isSymbolicLink() ? fs.readlinkSync(at) : null; } catch { return false; }
      if (target === null) continue;
      if (/^[\\/]{2}/.test(target)) return true;
      next = path.resolve(path.dirname(at), target, ...parts.slice(i + 1));
    }
    if (!next) return false;
    cur = next;
  }
  return true;
}

// A path with its symbolic links resolved, as far as it exists; one that reaches a network share (remoteLink) is
// left as given, since resolving it would reach the network.
function realPath(p) {
  if (remoteLink(p)) return path.resolve(p);
  const real = q => {
    try { return fs.realpathSync(q); } catch { const up = path.dirname(q); return up === q ? q : path.join(real(up), path.basename(q)); }
  };
  return real(p);
}

// [key, value] pairs added to env as git config (GIT_CONFIG_KEY_n, GIT_CONFIG_VALUE_n), after those already there.
function addConfig(env, pairs) {
  let n = parseInt(env.GIT_CONFIG_COUNT, 10) || 0;
  for (const [key, value] of pairs) { env[`GIT_CONFIG_KEY_${n}`] = key; env[`GIT_CONFIG_VALUE_${n++}`] = value; }
  env.GIT_CONFIG_COUNT = String(n);
  return env;
}

// A file written to a temporary file then renamed, so that another pane never reads it half-written; its folder made
// when missing (only then, as some files are written often: the resizing marker). Returns whether it was written.
function writeAtomic(file, text) {
  const tmp = `${file}.${process.pid}`;
  const put = () => { fs.writeFileSync(tmp, text); fs.renameSync(tmp, file); };
  try {
    try { put(); } catch (e) {
      if (e.code !== 'ENOENT') throw e;
      fs.mkdirSync(path.dirname(file), { recursive: true });
      put();
    }
    return true;
  } catch { try { fs.unlinkSync(tmp); } catch { } return false; }
}

// The labels of the plugin's own panes (herdr-plugin.toml), told apart from the terminals and agents.
const OWN_PANES = new Set(['Explorer', 'Preview', 'Source Control', 'Changes', 'Editor', 'Search']);

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
  addConfig(env, config);
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
const DEFAULT_WIDTH = 32;
const WIDTH_FILE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'width');
function readWidth() {
  try { return Math.max(10, parseInt(fs.readFileSync(WIDTH_FILE, 'utf8'), 10)) || DEFAULT_WIDTH; } catch { return DEFAULT_WIDTH; }
}

// User settings, shared by the explorer, the preview and the hook (settings.json in the state folder).
const DEFAULTS = {
  hidden: true,       // show dot files
  ignored: true,      // show git-ignored files, greyed out
  open: 'preview',    // Enter and double-click on a text file: 'preview' or 'editor'
  editor: '',         // external editor command ("zed", "subl -w"); empty: VS Code when found, else the terminal editor
  singleClick: false, // the preview follows the selection (click or arrows): Preview follows selection
  side: 'left',       // explorer docked to the left or the right of the tab
  autoOpen: true,     // open the explorer in every new tab that has a single pane
  compact: true,      // compact folders: a folder whose only entry is a folder shows on one row with it
  follow: false,      // reveal the folder of the terminal next to the explorer when it changes
  glyphs: 'nerd',     // icon set: 'nerd' (Nerd Font glyphs), 'emoji', 'unicode' or 'ascii', for terminals without a Nerd Font
  theme: 'auto',      // colours: 'dark', 'light', or 'auto', by the terminal's background (ui.settingTheme)
  icons: '',          // explorer icon theme: { id, label, file } of a VS Code one; empty: Nerd Font icons
  wrap: false,        // preview: wrap long lines
  lineNumbers: true,  // preview: line number gutter
  images: 'auto',     // preview images: 'auto' (pixels when the terminal has the Kitty graphics protocol, else half
                      // blocks), 'blocks' (half blocks), 'pixels' (pixels without waiting for the terminal's answer)
  reviewPrefix: '',   // review notes: text typed before them when sent to the agent (A), such as an instruction
  diffLayout: '',     // diffs 'inline' or 'split' (side by side), or { narrow, wide } (diff.js); empty: by the pane's width
  updateCheck: true,  // check GitHub once a day for a newer release
  scmTree: false,     // source control: the changes as a folder tree rather than a list
  scmGraphAll: true,  // source control history: every branch, or only the current one
  scmListShare: {},   // source control: the list's share of the width beside the diff, by view
};
const SETTINGS_FILE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'settings.json');
function readSettings() {
  try { return { ...DEFAULTS, ...JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8')) }; } catch { return { ...DEFAULTS }; }
}
function writeSettings(settings) {
  if (SETTINGS_FILE) writeAtomic(SETTINGS_FILE, JSON.stringify(settings, null, 1));
}

// What each plugin pane shows, one file per pane in the state folder (panes/), since herdr does not keep it across a
// restart: the explorer's space folder and tree root, which Set as root may have moved; the preview's file.
const PANES_DIR = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'panes');
const paneName = pane => `${pane.replace(/[^\w.-]/g, '_')}.json`;
const paneFile = pane => PANES_DIR && pane && path.join(PANES_DIR, paneName(pane));
function readPaneState(pane) {
  try { return JSON.parse(fs.readFileSync(paneFile(pane), 'utf8')); } catch { return null; }
}
function writePaneState(pane, data) {
  const file = paneFile(pane);
  if (file) writeAtomic(file, JSON.stringify(data));
}

// What a tab keeps in the state folder, one file per tab and kind: <kind>-<tab id>.json. The kinds: the review notes
// (notes.js), the preview's requests (actions.js), the diff scope and the agent's last turn (scope.js). Read from the environment at each call, as peek.js sets the state
// folder after loading this module.
const TAB_KINDS = ['notes', 'preview', 'scope', 'turn', 'viewed'];
const tabName = (kind, tab) => `${kind}-${String(tab).replace(/[^\w-]/g, '_')}.json`;
const tabFile = (kind, tab) => process.env.HERDR_PLUGIN_STATE_DIR && tab
  && path.join(process.env.HERDR_PLUGIN_STATE_DIR, tabName(kind, tab));

// The state files left by tabs and panes that no longer exist: names, the files of the state folder; panes, those of
// its panes folder; tabs and livePanes, the ids that still exist. Returns the paths to remove.
function staleState(dir, names, panes, tabs, livePanes) {
  const keepTabs = new Set(TAB_KINDS.flatMap(k => tabs.map(t => tabName(k, t))));
  const kinds = new RegExp(`^(${TAB_KINDS.join('|')})-.+[.]json$`);
  const keepPanes = new Set(livePanes.map(paneName));
  return [...names.filter(n => kinds.test(n) && !keepTabs.has(n)).map(n => path.join(dir, n)),
    ...panes.filter(n => !keepPanes.has(n)).map(n => path.join(dir, 'panes', n))];
}

// Removes the state of the tabs and panes that no longer exist (staleState); the ids of those that do, from herdr.
function sweepState(tabs, livePanes) {
  const dir = process.env.HERDR_PLUGIN_STATE_DIR;
  if (!dir) return;
  const list = d => { try { return fs.readdirSync(d); } catch { return []; } };
  for (const f of staleState(dir, list(dir), list(path.join(dir, 'panes')), tabs, livePanes)) {
    try { fs.rmSync(f, { recursive: true, force: true }); } catch { }
  }
}

// Key bindings, editable by the user: keys.json in the plugin config folder, one object per pane (only the
// explorer reads its own so far), mapping an action to its keys ({ "explorer": { "newFile": ["a"] } }). An action left out keeps its default keys.
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
  saveKeys(all);
  return KEYS_FILE;
}

// Bindings changed from a pane: each action of changes given its keys, or its default ones (null). An unreadable
// file is left alone, for its owner to fix. Returns false when nothing could be written.
function writeKeys(pane, changes) {
  if (!KEYS_FILE) return false;
  let all = {};
  try { all = JSON.parse(fs.readFileSync(KEYS_FILE, 'utf8')); } catch (e) { if (e.code !== 'ENOENT') return false; }
  const own = { ...(all[pane] || {}) };
  for (const [action, keys] of Object.entries(changes)) if (keys === null) delete own[action]; else own[action] = keys;
  all[pane] = own;
  return saveKeys(all);
}

// keys.json written with one action per line, through a temporary file renamed, so that a pane never reads it
// half-written.
function saveKeys(all) {
  const lines = Object.entries(all).map(([p, actions]) => `  ${JSON.stringify(p)}: {\n`
    + Object.entries(actions).map(([a, k]) => `    ${JSON.stringify(a)}: ${JSON.stringify(k)}`).join(',\n') + '\n  }');
  return writeAtomic(KEYS_FILE, `{\n${lines.join(',\n')}\n}\n`);
}

module.exports = {
  WIN, PLUGIN, pathDirs, which, remoteLink, realPath, writeAtomic, addConfig, OWN_PANES, words, run, result, HERDR, GIT, gitEnv, gitOperation, version, newer, DEFAULT_WIDTH, WIDTH_FILE, readWidth,
  SETTINGS_FILE, readSettings, writeSettings, PANES_DIR, paneFile, readPaneState, writePaneState, TAB_KINDS, tabFile, staleState, sweepState, KEYS_FILE, readKeys, keysTemplate, writeKeys,
};
