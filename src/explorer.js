// File explorer of the space, in a herdr pane: tree folded with the mouse and the keyboard, VS Code-style
// git status, filter as you type, file operations, automatic refresh. Enter or double-click: preview (or the editor);
// right-click: action menu (actions.js). Header icon: settings.
'use strict';
// The pane's first process only starts the real one (launch.js).
if (require('./launch').launch()) return;
const fs = require('fs');
const path = require('path');
const actions = require('./actions');
const icons = require('./icons');
const ui = require('./ui');
const update = require('./update');
const { run, result, HERDR, GIT: GIT_BIN, gitEnv, WIDTH_FILE, readWidth, readSettings, writeSettings, KEYS_FILE, readKeys,
  keysTemplate, writeKeys, writePaneState, SETTINGS_FILE } = require('./exec');

const { C, GIT, fg, glyph, line, fit, textWidth } = ui;
const HOME = path.resolve(process.env.EXPLORER_ROOT || process.argv[2] || process.cwd());
// the space folder, or another one chosen with Set as root (EXPLORER_TREE: the one it had before a herdr restart)
let ROOT = process.env.EXPLORER_TREE ? path.resolve(process.env.EXPLORER_TREE) : HOME;
const PANE = process.env.HERDR_PANE_ID;
const WIDTH = 32; // columns when the terminal size is unknown
const SAVE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'expanded.json');
const FILTER_BUDGET = 20000; // entries read at most while filtering
const out = process.stdout;
const settings = readSettings();
const SIDE = settings.side; // where this explorer was docked: a later change applies to the next tabs
// The side of its inner border, which shift+← and shift+→ push: on the right of an explorer docked on the left.
const INNER = SIDE === 'right' ? 'left' : 'right';

const RANK = { U: 1, A: 2, R: 2, M: 3, D: 4, C: 5 };

// Actions and their default keys, changed in keys.json ("explorer" object). Key names: up, down, left, right, home,
// end, pageup, pagedown, enter, space, tab, backspace, delete, escape, menu, f1…f5, ctrl+<letter>, alt+<key>, or the
// character itself ('/', 'n', 'N'). In the menu: up, down, open, expand, close; in the filter: the keys that are not
// characters.
const DEFAULT_KEYS = {
  up: ['up', 'k'], down: ['down', 'j'], pageUp: ['pageup'], pageDown: ['pagedown'], top: ['home', 'g'], bottom: ['end', 'G'],
  open: ['enter', 'space'], expand: ['right', 'l'], collapse: ['left', 'h'], close: ['escape', 'q'],
  menu: ['m', 'menu', 'shift+f10'], refresh: ['r'], filter: ['/'], goToFile: ['ctrl+p'], searchInFiles: ['ctrl+f'],
  sourceControl: ['s'], settings: [','], toggleHidden: ['.'], toggleIgnored: ['i'], newFile: ['n'], newFolder: ['N'],
  rename: ['R', 'f2'], delete: ['delete'], edit: ['e'], openInEditor: ['o'], openInApp: ['O'], preview: ['p'], copyPath: ['y'],
  copyRelativePath: ['Y'], parentRoot: ['backspace'], narrower: ['>', INNER === 'right' ? 'shift+left' : 'shift+right'],
  wider: ['<', INNER === 'right' ? 'shift+right' : 'shift+left'], halfPageUp: ['ctrl+u'], halfPageDown: ['ctrl+d'], help: ['?', 'f1'],
};
// The key help (help action), by group, with the keys bound now: [group, [[action, description]]].
const HELP = [
  ['Move', [['up', 'up'], ['down', 'down'], ['pageUp', 'page up'], ['pageDown', 'page down'], ['halfPageUp', 'half page up'],
    ['halfPageDown', 'half page down'], ['top', 'first'], ['bottom', 'last'], ['expand', 'unfold'], ['collapse', 'fold, parent'],
    ['filter', 'filter']]],
  ['Open', [['open', 'open, fold'], ['preview', 'preview'], ['edit', 'edit (terminal)'], ['openInEditor', 'open in editor'], ['openInApp', 'open in app'],
    ['copyPath', 'copy path'], ['copyRelativePath', 'copy relative path']]],
  ['Files', [['newFile', 'new file'], ['newFolder', 'new folder'], ['rename', 'rename'], ['delete', 'to the trash'],
    ['menu', 'actions menu']]],
  ['Panes', [['goToFile', 'go to file'], ['searchInFiles', 'search in files'], ['sourceControl', 'source control'],
    ['settings', 'settings'], ['narrower', 'narrower'], ['wider', 'wider'], ['toggleHidden', 'hidden files'], ['toggleIgnored', 'git-ignored files'], ['parentRoot', 'parent as root'],
    ['refresh', 'refresh'], ['help', 'keys'], ['close', 'close']]],
];
let keys = readKeys('explorer', DEFAULT_KEYS);
if (KEYS_FILE) fs.watchFile(KEYS_FILE, { interval: 1000 }, () => { keys = readKeys('explorer', DEFAULT_KEYS); });

const CHEVRON = [0xeab6, 0xeab4];
const SEARCH = ui.SEARCH.codePointAt(0), GEAR = 0xf013;
let theme = icons.load(settings.icons); // VS Code icon theme; null: the default icons
// Moved by an update of its extension: its new place saved for the next starts. No longer installed: the default
// icons saved, rather than searching for it again at every start.
if (theme ? theme.entry.file !== settings.icons.file : settings.icons && settings.icons.id) {
  settings.icons = theme ? theme.entry : '';
  writeSettings({ ...readSettings(), icons: settings.icons });
}
// The icon theme chosen in another explorer, the update check or the opening in new tabs turned on or off there, and
// the preview and diff settings changed in their panes (shown by an open settings view).
if (SETTINGS_FILE) fs.watchFile(SETTINGS_FILE, { interval: 1000 }, () => {
  // A file read while being written is skipped, rather than taken for the default settings.
  let saved, chosen;
  try { saved = JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8')); chosen = saved.icons || ''; } catch { return; }
  for (const name of ['updateCheck', 'autoOpen', 'wrap', 'lineNumbers', 'diffLayout']) {
    if (saved[name] === undefined || saved[name] === settings[name]) continue;
    settings[name] = saved[name];
    render();
  }
  if (JSON.stringify(chosen) === JSON.stringify(settings.icons)) return;
  settings.icons = chosen;
  theme = icons.load(chosen);
  render();
});
// A newer release, found by the hook's daily check: ↑ in the header.
let latest = update.available();
// An update that changed the manifest or the installer, which the installer must finish: ! in the header.
let finish = update.finishing();
let updating = false; // a pull running: no second one, ↑ hidden
// Restarted on new code: this process only waits for the new one, its async work still running stops there.
let stopped = false;
if (update.FILE) fs.watchFile(update.FILE, { interval: 1000 }, () => { latest = update.available(); finish = update.finishing(); render(); });

const state = {
  expanded: new Set(['']), rows: [], cursor: 0, top: 0, selected: null, focused: true,
  menu: null,      // { items: [[label, action] | null], at: screen row of the menu top, sel }
  help: null,      // { top, find }: the key help over the pane (ui.help)
  settings: null,  // { sel, top, find, icons }: the settings view in place of the tree, its selected row and first row
                   // shown, and the page opened from it: icon themes ({ list, sel, top, loaded }) or key bindings
                   // ({ sel, top, find, capture }); find: the field of /, null when closed
  flash: null,     // short message shown in the header
  lastClick: null, // to detect double-clicks
  filter: null,    // { field, active }: rows limited to the names containing the text
  prompt: null,    // { label, field, done(value), inline }: a name typed in the tree (inline: { rel } or
                   // { dir, folder }) or at the bottom
  confirm: null,   // { title, name, where, note, action, tone, yes(), focus }: y, or Enter on the action's button
                   // (focus 'yes', moved with ← →), to confirm; any other key to cancel
};
const scrollbar = new ui.Scrollbar(top => { state.top = top; render(); });
const git = { top: null, prefix: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set() };
const cache = new Map(); // relative folder → sorted entries

// The repository config is read again before each call, as it may change while the explorer runs.
const gitRun = async args => run(GIT_BIN, args, { cwd: ROOT, env: await gitEnv(ROOT) });
const parentOf = rel => (rel === '' ? null : rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '');
const absOf = rel => path.join(ROOT, rel);

// --- tree

function readDir(rel) {
  if (cache.has(rel)) return cache.get(rel);
  let entries = [];
  try {
    entries = fs.readdirSync(path.join(ROOT, rel), { withFileTypes: true })
      .filter(e => e.name !== '.git')
      .map(e => {
        let dir = e.isDirectory();
        if (e.isSymbolicLink()) try { dir = fs.statSync(path.join(ROOT, rel, e.name)).isDirectory(); } catch { }
        return { name: e.name, dir, rel: rel ? `${rel}/${e.name}` : e.name };
      })
      .sort((a, b) => (b.dir - a.dir) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }));
  } catch { }
  cache.set(rel, entries);
  return entries;
}

// Entries shown with the current settings: dot files and git-ignored ones can be hidden.
const visible = rel => readDir(rel).filter(e => (settings.hidden || !e.name.startsWith('.'))
  && (settings.ignored || decoration(e) !== '!'));

function buildRows() {
  const rows = [];
  const query = state.filter && state.filter.field.value.toLowerCase();
  if (query) {
    // Every folder is read, except ignored ones (node_modules, build output), within a budget.
    let budget = FILTER_BUDGET;
    const walk = (rel, depth) => {
      const found = [];
      for (const e of visible(rel)) {
        if (--budget < 0) break;
        const at = e.name.toLowerCase().indexOf(query);
        const kids = e.dir && !git.ignored.has(e.rel) ? walk(e.rel, depth + 1) : [];
        if (at >= 0 || kids.length) found.push({ ...e, depth, hit: at >= 0 ? [at, at + query.length] : null, open: kids.length > 0 }, ...kids);
      }
      return found;
    };
    rows.push(...walk('', 0));
  } else {
    const walk = (rel, depth) => {
      for (const e of visible(rel)) {
        rows.push({ ...e, depth, open: e.dir && state.expanded.has(e.rel) });
        if (e.dir && state.expanded.has(e.rel)) walk(e.rel, depth + 1);
      }
    };
    walk('', 0);
  }
  state.rows = rows;
  const i = state.selected === null ? -1 : rows.findIndex(r => r.rel === state.selected);
  select(i >= 0 ? i : query ? rows.findIndex(r => r.hit) : state.cursor);
}

function select(i) {
  state.cursor = Math.max(0, Math.min(i, state.rows.length - 1));
  state.selected = state.rows.length ? state.rows[state.cursor].rel : null;
}

function toggle(row, open) {
  if (!row || !row.dir || state.filter) return;
  if (open === undefined) open = !state.expanded.has(row.rel);
  if (open) state.expanded.add(row.rel); else state.expanded.delete(row.rel);
  buildRows();
  save();
}

// Expands the folders above an entry and selects it.
function revealRel(rel) {
  for (let a = parentOf(rel); a; a = parentOf(a)) state.expanded.add(a);
  state.selected = rel;
  buildRows();
  reveal();
  save();
}

// Expanded folders, per space folder, restored when the explorer is reopened.
function load() {
  try { for (const rel of JSON.parse(fs.readFileSync(SAVE, 'utf8'))[ROOT] || []) state.expanded.add(rel); } catch { }
}
// Shared by every explorer: written to a temporary file then renamed, so that another one never reads it
// half-written; left alone when unreadable rather than overwritten with this space only.
function save() {
  if (!SAVE) return;
  let all = {};
  try { all = JSON.parse(fs.readFileSync(SAVE, 'utf8')); } catch (e) { if (e.code !== 'ENOENT') return; }
  all[ROOT] = [...state.expanded].filter(Boolean);
  const tmp = `${SAVE}.${process.pid}`;
  try { fs.writeFileSync(tmp, JSON.stringify(all, null, 1)); fs.renameSync(tmp, SAVE); } catch { try { fs.unlinkSync(tmp); } catch { } }
}

// Another folder as the root of the tree: from the menu (Set as root), or its parent (Backspace).
function setRoot(dir) {
  dir = path.resolve(dir);
  if (stopped || dir === ROOT) return;
  const from = ROOT;
  save();
  ROOT = dir;
  writePaneState(PANE, { home: HOME, root: ROOT });
  state.expanded = new Set(['']); state.filter = null; state.top = 0; state.cursor = 0;
  state.selected = null;
  load();
  cache.clear();
  Object.assign(git, { top: null, prefix: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set() });
  // Going up: the folder left is selected.
  const back = path.relative(ROOT, from).split(path.sep).join('/');
  if (back && !back.startsWith('..') && !path.isAbsolute(back)) revealRel(back);
  watchRoot();
  buildRows();
  render();
  initGit().then(refresh);
}

// --- git

// Also called when a .git folder appears (git init, clone, archive extracted) in a folder that had none.
let probing = false, gitWatcher = null;
async function initGit() {
  if (git.top || probing) return;
  probing = true;
  const root = ROOT;
  const res = await gitRun(['rev-parse', '--show-toplevel', '--absolute-git-dir', '--show-prefix']);
  probing = false;
  if (stopped || !res || root !== ROOT) return;
  const [top, gitdir, prefix = ''] = res.split(/\r?\n/);
  git.top = top;
  git.prefix = prefix.replace(/\/$/, '');
  // The repository changes without touching the working tree (commit, stage, branch switch).
  if (gitWatcher) gitWatcher.close();
  try {
    gitWatcher = fs.watch(gitdir, (ev, f) => { if (f === 'index' || f === 'HEAD') schedule(); });
    gitWatcher.on('error', () => { });
  } catch { }
}

function code(x, y) {
  if (x === '?') return 'U';
  if (x === '!') return '!';
  if (x === 'U' || y === 'U' || (x === 'A' && y === 'A') || (x === 'D' && y === 'D')) return 'C';
  if (x === 'D' || y === 'D') return 'D';
  if (x === 'R' || y === 'R') return 'R';
  if (x === 'A' || x === 'C') return 'A';
  return 'M';
}

async function refreshGit() {
  if (!git.top) return;
  const status = await gitRun(['status', '--porcelain=v1', '-z', '--ignored=matching', '--ignore-submodules=all', '--', '.']);
  if (status === null) return;
  git.files = new Map(); git.dirs = new Map(); git.untracked = new Set(); git.ignored = new Set();
  const parts = status.split('\0');
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (p.length < 4) continue;
    const c = code(p[0], p[1]);
    if ('RC'.includes(p[0]) || 'RC'.includes(p[1])) i++; // the original path follows (renamed in the index or the tree)
    let rel = p.slice(3);
    if (git.prefix) {
      if (!rel.startsWith(git.prefix + '/')) continue;
      rel = rel.slice(git.prefix.length + 1);
    }
    const isDir = rel.endsWith('/');
    if (isDir) rel = rel.slice(0, -1);
    if (c === '!') { git.ignored.add(rel); continue; }
    if (isDir) git.untracked.add(rel); else git.files.set(rel, c);
    for (let a = parentOf(rel); a !== null; a = parentOf(a)) {
      if (!git.dirs.has(a) || RANK[c] > RANK[git.dirs.get(a)]) git.dirs.set(a, c);
    }
  }
}

// Decoration of an entry: that of an ignored or untracked folder extends to all its contents.
function decoration(row) {
  for (let a = row.rel; a !== null && a !== ''; a = parentOf(a)) {
    if (git.ignored.has(a)) return '!';
    if (git.untracked.has(a)) return 'U';
  }
  // A symbolic link to a folder is a file for git.
  return (row.dir && git.dirs.get(row.rel)) || git.files.get(row.rel);
}

// A change inside a collapsed ignored folder (node_modules, bin, obj…) is not visible: skip it.
function hidden(rel) {
  for (let a = parentOf(rel); a !== null && a !== ''; a = parentOf(a)) {
    if (git.ignored.has(a) && !state.expanded.has(a)) return true;
  }
  return false;
}

// --- refresh

// At most one refresh every 250 ms: a file written continuously (a log) must not postpone it forever.
let timer = null, busy = false, again = false;
function schedule() {
  if (!timer && !stopped) timer = setTimeout(() => { timer = null; refresh(); }, 250);
}
async function refresh() {
  if (stopped) return;
  if (busy) { again = true; return; }
  busy = true;
  cache.clear();
  await refreshGit();
  buildRows();
  render();
  busy = false;
  if (again) { again = false; schedule(); }
}

let rootWatcher = null;
// Recursive, except on $HOME, one of its parents or the filesystem root: on Linux, Node adds one watch per folder
// of the tree, which would take seconds and could use up the system's limit. Top level only there, and also when a
// folder of the tree cannot be read (the recursive watch then fails); the 30 s scan covers the rest.
function watchRoot() {
  if (rootWatcher) rootWatcher.close();
  rootWatcher = null;
  if (stopped) return;
  const real = p => { try { return fs.realpathSync(p); } catch { return p; } }; // /home → /var/home on some systems
  const up = path.relative(real(ROOT), real(require('os').homedir()));
  const big = ROOT === path.parse(ROOT).root || !up.startsWith('..') && !path.isAbsolute(up);
  for (const recursive of big ? [false] : [true, false]) {
    try { rootWatcher = watchTree(recursive); return; } catch { }
  }
}

function watchTree(recursive) {
  const watcher = fs.watch(ROOT, { recursive }, (ev, f) => {
    const rel = f ? String(f).split(path.sep).join('/') : '';
    if (rel === '.git' || rel.startsWith('.git/')) {
      if (!git.top) setTimeout(() => initGit().then(() => git.top && schedule()), 500);
      return;
    }
    if (rel.includes('/.git/') || hidden(rel)) return;
    schedule();
  });
  watcher.on('error', () => { });
  return watcher;
}

// --- pane width

// Enforced on opening and whenever another resize moves the explorer (actions.widthKeeper). A resize of its own
// split, dragging its border or with < >, becomes the new width, remembered for the next openings.
let width = readWidth();
const keeper = actions.widthKeeper(PANE, { columns: () => out.columns, get: () => width, set: saveWidth, side: SIDE });
const ensureWidth = () => keeper.check(true);
let settle = null;

function saveWidth(w) {
  if (w === width) return;
  width = w;
  try { fs.writeFileSync(WIDTH_FILE, String(width)); } catch { }
}

// The width checked at once, so that a pane moved by another one's resize goes back while the gesture goes on
// (the checks asked meanwhile make one more). Alone in the tab (the terminal has exited), the explorer closes, and the
// tab with it: decided at the end of the gesture (a drag sends one event per column).
function resized() {
  keeper.check();
  clearTimeout(settle);
  settle = setTimeout(async () => {
    if (!PANE) return;
    const layout = await actions.layoutOf(PANE).catch(() => null);
    if (layout && !layout.zoomed && layout.panes.length === 1 && await alone()) return close();
  }, 400);
}

// < and >: 4 columns wider or narrower, from the width asked for last, so that a key held down adds up.
function widen(cols) {
  saveWidth(Math.max(10, width + cols));
  ensureWidth();
}

// Alone in its tab by the pane list too: the layout alone can show a single pane while the tab has others (a
// source control pane zoomed over the explorer, a client attaching), and closing then lost the explorer.
async function alone() {
  const me = (result(await run(HERDR, ['pane', 'get', PANE])) || {}).pane;
  if (!me) return false;
  const all = (result(await run(HERDR, ['pane', 'list', '--workspace', me.workspace_id])) || {}).panes;
  return !!all && all.filter(p => p.tab_id === me.tab_id).length === 1;
}

// The tab's preview closes with the explorer: reopened, the explorer then takes the side of the tab, and the next
// preview opens beside the terminal (see settlePreview in hook.js).
async function close() {
  save();
  if (!PANE) return;
  const tab = await actions.tabOf(PANE).catch(() => null);
  for (const p of (tab && tab.panes) || []) if (p.label === 'Preview') await run(HERDR, ['pane', 'close', p.pane_id]);
  run(HERDR, ['pane', 'close', PANE]);
}

// --- following the terminal

// The folder of the terminal next to the explorer (that of its running program when herdr knows it), revealed in
// the tree when it changes; outside the root, the root changes. The setting is read each time, so that a change
// made in another explorer applies here too.
let lastCwd = null;
async function follow() {
  settings.follow = readSettings().follow;
  if (!settings.follow || !PANE) return;
  const [target] = await actions.terminalOf(PANE);
  const p = target && (result(await run(HERDR, ['pane', 'get', target])) || {}).pane;
  const cwd = p && (p.foreground_cwd || p.cwd);
  // Retried once the menu, the settings, the filter or the prompt is closed.
  if (stopped || !cwd || cwd === lastCwd || state.filter || state.menu || state.settings || state.help || state.prompt || state.confirm) return;
  lastCwd = cwd;
  const inside = (dir, root) => { const r = path.relative(root, dir); return !r.startsWith('..') && !path.isAbsolute(r); };
  // Outside the root: back to the space folder when it holds the terminal folder. Never above it, where the
  // recursive watch could cover $HOME or the whole disk.
  if (!inside(cwd, ROOT)) {
    if (!inside(cwd, HOME)) return;
    setRoot(HOME);
  }
  const rel = path.relative(ROOT, cwd).split(path.sep).join('/');
  if (!rel) return;
  state.expanded.add(rel);
  revealRel(rel);
  render();
}

// --- rendering

const bodyHeight = () => (out.rows || 24) - 1 - (state.filter || state.prompt ? 1 : 0);

// The tree, or a page in its place (settings, key help), and a question over it.
function render() {
  const W = out.columns || WIDTH, H = out.rows || 24;
  const lines = state.settings ? settingsPage().view(W, H) : treeView(W, H);
  if (state.help) ui.help(lines, W, 'Explorer', helpGroups(), state.help);
  if (state.confirm) ui.confirm(lines, W, H, state.confirm);
  paint(lines);
}

// The tree, with the name being typed in it (Rename, New file), then at the bottom the prompt, its keys or the filter.
function treeView(W, H) {
  const lines = [header(W)];
  const listH = bodyHeight();
  const shown = shownRows(), edit = shown.findIndex(e => e.edit);
  if (edit >= 0 && edit < state.top) state.top = edit;
  if (edit >= state.top + listH) state.top = edit - listH + 1;
  if (state.top > shown.length - listH) state.top = Math.max(0, shown.length - listH);
  const bw = scrollbar.set(shown.length, listH, state.top);
  const selected = state.rows[state.cursor];
  for (let i = state.top; i < Math.min(shown.length, state.top + listH); i++) {
    const e = shown[i];
    lines.push((e.edit ? editRow(e, W - bw) : row(e.r, W - bw, e.r === selected)) + scrollbar.cell(i - state.top));
  }
  if (!shown.length) {
    const first = a => ui.keyLabel(keysOf(a)[0] || '');
    lines.push(...(state.filter ? ui.empty(W, listH, 'No match', [['esc', 'clear']])
      : ui.empty(W, listH, 'Empty folder', [[first('newFile'), 'new file'], [first('newFolder'), 'folder']].filter(([k]) => k))));
  }
  while (lines.length < listH + 1) lines.push(line([], W));
  if (state.prompt) lines.push(state.prompt.inline ? line(promptKeys(W), W, C.header) : promptLine(W));
  else if (state.filter) {
    lines.push(filterLine(W));
  }
  if (state.menu) drawMenu(lines, W);
  while (lines.length < H) lines.push(line([], W));
  lines.length = H;
  return lines;
}

// The rows shown: the tree's, the one being renamed made a field, or a new one at the top of the folder it goes to.
function shownRows() {
  const shown = state.rows.map(r => ({ r }));
  const at = state.prompt && state.prompt.inline;
  if (!at) return shown;
  if (at.rel !== undefined) {
    const i = state.rows.findIndex(r => r.rel === at.rel);
    if (i >= 0) shown[i] = { r: state.rows[i], edit: true };
    return shown;
  }
  const i = at.dir ? state.rows.findIndex(r => r.rel === at.dir) : -1;
  shown.splice(i + 1, 0, { edit: true, depth: i >= 0 ? state.rows[i].depth + 1 : 0, dir: at.folder });
  return shown;
}

// A row being named, its icon following the name typed.
const FIELD = [60, 60, 60];
function editRow(e, W) {
  const f = state.prompt.field, r = e.r, dir = r ? r.dir : e.dir, open = r ? r.open : false;
  const icon = icons.icon(theme, f.value || (r ? r.name : ''), dir, open);
  const lead = ' '.repeat(1 + (r ? r.depth : e.depth) * 2) + (dir ? glyph(CHEVRON[open ? 1 : 0]) : ' ') + ' ';
  return line([[lead, fg(C.dim)], [glyph(icon[0]) + ' ', fg(icon[1])], ...f.segments(Math.max(1, W - textWidth(lead) - 3))], W, FIELD);
}

// The keys of a name typed in the tree, and where a new entry goes.
function promptKeys(W) {
  const at = state.prompt.inline;
  const what = at.rel !== undefined ? 'rename' : `create in ${path.basename(at.dir || ROOT)}`;
  return ui.keys([['⏎', what], ['esc', 'cancel']], W);
}

// A prompt at the bottom (the editor command).
function promptLine(W) {
  const label = ` ${state.prompt.label}: `;
  return line([[label, fg(C.dim)], ...state.prompt.field.segments(W - textWidth(label))], W, C.header);
}

// Header: space name (or an action message), then the icons: ! when the installer must finish an
// update, else ↑ when a newer release exists (left out while a message shows), and the settings gear.
const showUpdate = () => !state.flash && (finish || (latest && settings.updateCheck && !updating));
function header(W) {
  const up = !showUpdate() ? [] : finish ? [['! ', fg(C.yellow)]] : [['↑ ', fg(C.green)]];
  const icons = [...up, [`${glyph(GEAR)} `, fg(C.dim)]];
  const room = W - 3 - (up.length ? 2 : 0);
  if (state.flash) {
    const msg = ui.slice(ui.message(state.flash), 0, room);
    return line([...msg, [' '.repeat(Math.max(0, room - ui.segWidth(msg) + 1)), ''], ...icons], W);
  }
  const [name, nw] = fit(` ${path.basename(ROOT).toUpperCase() || ROOT}`, room);
  return line([[name, ROOT === HOME ? '\x1b[1m' : '\x1b[1;3m'], [' '.repeat(Math.max(0, room - nw + 1)), ''], ...icons], W);
}

// The filter's line: while typed, the field and the count on the right; once kept (Tab), the filter in blue, the
// count, and how to clear it.
const HIT = '\x1b[1m' + fg([24, 163, 255]);
function filterLine(W) {
  const f = state.filter, n = state.rows.filter(r => r.hit).length;
  const count = f.field.value ? `${n} match${n === 1 ? '' : 'es'}` : '';
  const lead = [` ${glyph(SEARCH)} `, fg(C.dim)];
  if (f.active) {
    const field = f.field.segments(Math.max(1, W - 5 - count.length));
    return line([lead, ...field, [count.padStart(Math.max(0, W - 4 - ui.segWidth(field))), fg(C.dim)]], W, C.header);
  }
  const left = [lead, [f.field.value, fg(C.blue)], [count && `  ${count}`, fg(C.dim)]];
  const hint = ui.keys([['esc', 'clear']]);
  const room = W - ui.segWidth(left) - ui.segWidth(hint) - 1;
  return line(room >= 0 ? [...left, [' '.repeat(room), ''], ...hint] : left, W, C.header);
}

function row(r, W, selected) {
  const deco = decoration(r);
  const color = deco ? fg(GIT[deco]) : '';
  const icon = icons.icon(theme, r.name, r.dir, r.open);
  const indent = ' '.repeat(1 + r.depth * 2);
  const chevron = r.dir ? glyph(CHEVRON[r.open ? 1 : 0]) : ' ';
  const badge = !deco || deco === '!' ? '' : r.dir ? '●' : deco === 'C' ? '!' : deco;
  const room = W - indent.length - 4 - (badge ? 2 : 0) - 1;
  let name = r.name;
  if (textWidth(name) > room) name = fit(name, Math.max(1, room - 1))[0] + '…';
  const pad = Math.max(0, W - indent.length - 4 - textWidth(name) - (badge ? 2 : 0) - 1);
  // The part matching the filter in bold blue, as in Go to file; a folder shown only for what it holds, dimmed.
  const parts = r.hit && r.hit[1] <= name.length
    ? [[name.slice(0, r.hit[0]), color], [name.slice(r.hit[0], r.hit[1]), HIT], [name.slice(r.hit[1]), color]]
    : [[name, state.filter && state.filter.field.value && !r.hit ? fg(C.dim) : color]];
  return line([
    [indent + chevron + ' ', fg(C.dim)], [glyph(icon[0]) + ' ', fg(deco === '!' ? GIT['!'] : icon[1])],
    ...parts, [' '.repeat(pad) + (badge ? ' ' + badge : ''), color],
  ], W, selected ? (state.focused ? C.select : C.selectBlur) : null);
}

// --- opening

// Enter and double-click: documents and media other than images with their application, the rest in the preview
// (or the editor, depending on the settings).
async function openFile(r) {
  if (!r || r.dir) return;
  const abs = absOf(r.rel);
  if (settings.open === 'preview' && actions.canPreview(abs)) return runAction(() => actions.preview(abs, ROOT, PANE));
  return runAction(() => actions.open(abs, PANE));
}

// --- prompts

// empty: an empty answer is accepted (otherwise it cancels), and the cursor stays at the end. inline: typed in the
// tree, on the row renamed ({ rel }) or on a new row in its folder ({ dir, folder }), rather than at the bottom.
function ask(label, value, done, empty = false, inline = null) {
  state.menu = null;
  state.prompt = { label, field: new ui.Field(value), done, empty, inline };
  // A new name: the cursor before the extension.
  const dot = value.lastIndexOf('.');
  if (dot > 0 && !empty) state.prompt.field.cursor = Array.from(value.slice(0, dot)).length;
  render();
}

// q: { title, name, where, note, action, tone }, see ui.confirm.
function confirm(q, yes) {
  state.menu = null;
  state.confirm = { ...q, yes, focus: 'no' };
  render();
}

// Where an entry is, for a question: its folder from the tree's root.
const whereOf = rel => `in ${path.basename(ROOT)}/${parentOf(rel) ? parentOf(rel) + '/' : ''}`;

// After a change on disk: the new entry selected, its folder expanded.
function created(abs) {
  cache.clear();
  const rel = path.relative(ROOT, abs).split(path.sep).join('/');
  revealRel(rel);
  render();
}

// The new name typed at the top of its folder, opened for it.
function newEntry(dirRel, folder) {
  if (dirRel && !state.expanded.has(dirRel)) { state.expanded.add(dirRel); buildRows(); }
  ask(folder ? 'New folder' : 'New file', '', name => {
    const [abs, err] = actions.create(absOf(dirRel), name, folder);
    if (err) return flash(err);
    created(abs);
  }, false, { dir: dirRel, folder });
}

function renameEntry(r) {
  ask('Rename', r.name, name => {
    const [abs, err] = actions.rename(absOf(r.rel), name);
    if (err) return flash(err);
    // The expanded subfolders follow the folder.
    if (r.dir) {
      const to = path.relative(ROOT, abs).split(path.sep).join('/');
      state.expanded = new Set([...state.expanded].map(e => (e === r.rel || e.startsWith(r.rel + '/') ? to + e.slice(r.rel.length) : e)));
    }
    created(abs);
  }, false, { rel: r.rel });
}

function deleteEntry(r) {
  const abs = absOf(r.rel), name = r.name + (r.dir ? '/' : ''), where = whereOf(r.rel);
  confirm({ title: 'Move to the trash?', name, where, action: 'Move' }, async () => {
    flash('Moving to the trash…');
    if (await actions.trash(abs)) return flash(`${r.name} moved to the trash`);
    confirm({ title: 'No trash: delete for good?', name, where, note: 'This cannot be undone.', action: 'Delete' },
      () => flash(actions.remove(abs) || `${r.name} deleted`));
  });
}

// --- action menu

// Each item: [label, action(), the action of its key (shown on the right), danger (red)], or null for a separator.
function menuItems(r) {
  const items = [];
  const dirRel = r ? (r.dir ? r.rel : parentOf(r.rel)) : '';
  if (r) {
    const abs = absOf(r.rel);
    const deco = decoration(r);
    if (!r.dir) {
      if (actions.canPreview(abs)) items.push(['Preview', () => actions.preview(abs, ROOT, PANE), 'preview']);
      items.push(['Open in editor', () => actions.editor(abs, PANE), 'openInEditor']);
      if (actions.canOpen(abs)) items.push(['Open in app', () => actions.system(abs), 'openInApp']);
      items.push(['Edit in terminal', () => actions.edit(abs, PANE), 'edit']);
    } else items.push(['Open in editor', () => actions.editor(abs, PANE), 'openInEditor']);
    items.push(['Insert into terminal', () => actions.insert(abs, PANE)], null);
    items.push(['New file…', () => newEntry(dirRel, false), 'newFile'], ['New folder…', () => newEntry(dirRel, true), 'newFolder']);
    if (r.dir) items.push(['Search in folder…', () => actions.search(abs, PANE, 'content'), 'searchInFiles'], ['Set as root', () => setRoot(abs)]);
    items.push(['Rename…', () => renameEntry(r), 'rename'], ['Duplicate', () => {
      const [to, err] = actions.duplicate(abs);
      if (err) return err;
      created(to);
    }], ['Delete…', () => deleteEntry(r), 'delete', true], null);
    items.push([actions.REVEAL, () => actions.reveal(abs)], ['Copy path', () => actions.copy(abs), 'copyPath'],
      ['Copy relative path', () => actions.copy(r.rel), 'copyRelativePath']);
    if (git.top && deco !== '!') {
      const changed = deco && deco !== '!';
      items.push(null);
      if (changed) items.push(['View changes', () => actions.diff(abs, ROOT, PANE)]);
      if (changed) items.push(['Stage', () => actions.stage(abs, ROOT)]);
      items.push(['Unstage', () => actions.unstage(abs, ROOT)]);
      if (changed) {
        items.push(['Discard changes…', () => confirm({ title: 'Discard the changes?', name: r.name, where: whereOf(r.rel),
          note: 'This cannot be undone.', action: 'Discard' }, () => runAction(() => actions.discard(abs, ROOT))), null, true]);
      }
    }
  } else {
    items.push(['New file…', () => newEntry('', false), 'newFile'], ['New folder…', () => newEntry('', true), 'newFolder'], null);
    items.push(['Go to file…', () => actions.search(ROOT, PANE, 'files'), 'goToFile'],
      ['Search in files…', () => actions.search(ROOT, PANE, 'content'), 'searchInFiles']);
    if (git.top) items.push(['Source control', () => actions.scm(ROOT, PANE), 'sourceControl']);
    items.push(null, [actions.REVEAL, () => actions.reveal(ROOT)], ['Copy path', () => actions.copy(ROOT)],
      ['Up one level', () => setRoot(path.dirname(ROOT)), 'parentRoot']);
    if (ROOT !== HOME) items.push(['Back to the space folder', () => setRoot(HOME)]);
    items.push(['Refresh', () => schedule(), 'refresh'], ['Settings…', () => openSettings(), 'settings']);
  }
  return items;
}

// A fast-forward pull of the plugin's clone; the panes then restart on the new code by themselves (update.watch).
// Refused: the reason only, since the command would bypass the checks. The pull failed: the command copied, to run in
// the terminal. "Updating…" stays until the pull ends, which may take up to a minute. Installed by herdr: herdr's
// update command copied instead, since only herdr may move its checkout (herdr not answering: the pull, whose checks
// refuse such a checkout).
async function askUpdate() {
  if (updating) return 'Update in progress';
  if (await update.managed()) return actions.copy(update.HERDR_UPDATE, 'Update command copied');
  const to = latest;
  confirm({ title: `Update to ${to}?`, name: 'herdr-explorer', note: 'The panes restart on the new code.', action: 'Update',
    tone: 'go' }, async () => {
    updating = true;
    clearTimeout(flash.timer);
    state.flash = 'Updating…';
    render();
    const { ok, retry, message } = await update.install();
    updating = false;
    state.flash = null;
    if (ok || !retry) return message;
    return `${message}: ${await actions.copy(`git -C "${update.DIR}" pull --ff-only --no-rebase`, 'update command copied')}`;
  });
  return null;
}

// The installer command copied, to run in a terminal; the ! then goes away in every explorer.
async function finishUpdate() {
  const done = await actions.copy(update.INSTALLER, 'Installer command copied');
  if (done !== 'Clipboard unavailable') update.finished();
  return done;
}

function openMenu(i) {
  const r = i === null ? null : state.rows[i];
  if (r) { select(i); reveal(); }
  const items = menuItems(r);
  const H = out.rows || 24, y = r ? i - state.top + 1 : Math.min(H - 1, state.rows.length - state.top + 1);
  const h = Math.min(items.length, H - 2) + 2;
  const title = r ? r.name + (r.dir ? '/' : '') : `${path.basename(ROOT)}/`;
  state.menu = { items, title, sel: 0, off: 0, at: y + 1 + h <= H ? y + 1 : Math.max(0, Math.min(y - h, H - h)) };
  render();
}

// Rows of the menu that fit in the pane; a longer menu scrolls with its selection (▴ ▾ on its border).
const menuRows = () => Math.max(1, Math.min(state.menu.items.length, (out.rows || 24) - 2 - state.menu.at));

// The menu in a frame titled with the entry it acts on, each item with the first key of its action on the right.
function drawMenu(lines, W) {
  const { items, sel, at, off, title } = state.menu;
  const n = menuRows();
  const keyOf = item => { const k = item[2] && keysOf(item[2])[0]; return k ? ui.keyLabel(k) : ''; };
  const want = Math.max(textWidth(title) + 4, ...items.map(item => (item ? textWidth(item[0]) + (keyOf(item) ? textWidth(keyOf(item)) + 2 : 0) + 2 : 0)));
  const inner = Math.min(W - 3, want);
  const edge = fg(C.dim);
  const border = (l, r, t, more) => line([[` ${l}${t}${'─'.repeat(Math.max(0, inner - textWidth(t) - (more ? 2 : 0)))}${more ? more + '─' : ''}${r}`, edge]], W);
  lines[at] = border('╭', '╮', ` ${ui.ellipsis(title, Math.max(1, inner - 4))} `, off > 0 && '▴');
  items.slice(off, off + n).forEach((item, k) => {
    if (!item) { lines[at + 1 + k] = line([[' ├' + '─'.repeat(inner) + '┤', edge]], W); return; }
    const key = keyOf(item), label = ui.ellipsis(item[0], Math.max(1, inner - 3 - (key ? textWidth(key) + 1 : 0)));
    const pad = ' '.repeat(Math.max(1, inner - 2 - textWidth(label) - textWidth(key)));
    const segs = [[` ${label}`, item[3] ? fg(C.red) : ''], [`${pad}${key} `, fg(C.dim)]];
    lines[at + 1 + k] = ` ${edge}│\x1b[39m${line(segs, inner, off + k === sel ? C.select : C.selectBlur)}${edge}│\x1b[0m` + ' '.repeat(Math.max(0, W - inner - 3));
  });
  lines[at + n + 1] = border('╰', '╯', '', off + n < items.length && '▾');
}

async function runAction(action) {
  state.menu = null;
  render();
  flash(await Promise.resolve().then(action).catch(() => 'Action failed'));
}

function flash(text) {
  if (!text) return render();
  state.flash = text;
  render();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { state.flash = null; render(); }, ui.messageTime(ui.messageKind(text)));
}

function moveMenu(dir) {
  const m = state.menu;
  do m.sel = (m.sel + dir + m.items.length) % m.items.length; while (!m.items[m.sel]);
  const n = menuRows();
  if (m.sel < m.off) m.off = m.sel;
  if (m.sel >= m.off + n) m.off = m.sel - n + 1;
}

function closeMenu() {
  state.menu = null;
}

// Escape always closes the menu, whatever the bindings; the key of an item runs it.
function menuKey(k) {
  const m = state.menu, action = keys.get(k);
  if (k === 'escape') closeMenu();
  else if (action === 'up') moveMenu(-1);
  else if (action === 'down') moveMenu(1);
  else if (action === 'open' || action === 'expand') return runAction(m.items[m.sel][1]);
  else {
    // The key shown beside an item runs it (Settings… on ,); the others close the menu.
    const item = action && m.items.find(it => it && it[2] === action);
    if (item) return runAction(item[1]);
    if (['close', 'collapse', 'menu', 'settings'].includes(action)) closeMenu();
  }
  render();
}

function reveal() {
  const listH = bodyHeight();
  if (state.cursor < state.top) state.top = state.cursor;
  if (state.cursor >= state.top + listH) state.top = state.cursor - listH + 1;
}

// --- settings

// A setting changed from the settings view or a key, saved for every explorer.
function setSetting(name, value) {
  settings[name] = value;
  writeSettings({ ...readSettings(), [name]: value });
  cache.clear();
  buildRows();
  if (name === 'follow') { lastCwd = null; follow(); }
  return null;
}

// The value of a setting and its change(dir): the next value (1: enter, → or a click) or the previous one (-1: ←),
// or the setting's dialog (left alone by ←); change returns a message or null.
const onOff = name => ({
  value: settings[name] ? ['● on', fg(C.green)] : ['○ off', fg(C.dim)],
  change: () => setSetting(name, !settings[name]),
});
const choice = (name, values, shown = v => v) => {
  const now = Math.max(0, values.indexOf(settings[name]));
  return { value: [shown(values[now]), fg(C.blue)], change: dir => setSetting(name, values[(now + dir + values.length) % values.length]) };
};
const dialog = (value, open) => ({ value: [value, fg(C.blue)], change: dir => (dir > 0 ? open() : null) });

// The rows of the settings view: a group title, or a setting with its value and its help.
function settingsRows() {
  const editor = () => ask('Editor', readSettings().editor, cmd => {
    settings.editor = cmd.trim();
    writeSettings({ ...readSettings(), editor: settings.editor });
    return settings.editor ? `Editor: ${settings.editor}` : 'Editor: default';
  }, true);
  return [
    { group: 'TREE' },
    { label: 'Hidden files', help: 'Show dot files in the tree.', ...onOff('hidden') },
    { label: 'Git-ignored files', help: 'Show the files git ignores, greyed out.', ...onOff('ignored') },
    { label: 'Follow terminal', help: 'Reveal the folder of the terminal beside the explorer, within the tab folder.', ...onOff('follow') },
    { label: 'Icon theme', help: 'Nerd Font icons, or a file icon theme installed for VS Code.',
      ...dialog((settings.icons && settings.icons.label) || 'Nerd Font', () => openIconThemes()) },
    { group: 'OPENING' },
    { label: 'Enter opens', help: 'Enter and double-click on a text file: show it in the preview or in the editor.', ...choice('open', ['preview', 'editor']) },
    { label: 'Editor', help: 'Command run with the path. Empty: VS Code if installed, else a terminal editor.', ...dialog(settings.editor || 'default', editor) },
    { label: 'Single-click preview', help: 'A single click on a file shows it in the preview.', ...onOff('singleClick') },
    { group: 'NEXT TABS' },
    { label: 'Dock side', help: 'Where new explorers dock. An open explorer keeps its side.', ...choice('side', ['left', 'right']) },
    { label: 'Open automatically', help: 'Open the explorer in new tabs that have a single pane. Off: only with prefix+f.', ...onOff('autoOpen') },
    { group: 'PREVIEW' },
    { label: 'Wrap lines', help: 'Wrap long lines in the preview (w). Other previews at their next opening.', ...onOff('wrap') },
    { label: 'Line numbers', help: 'Line numbers in the preview (L). Other previews at their next opening.', ...onOff('lineNumbers') },
    { label: 'Diff layout', help: 'Diffs inline or side by side (s). Auto: side by side when the pane is wide enough.', ...choice('diffLayout', ['', 'split', 'inline'], v => v || 'auto') },
    { group: 'GENERAL' },
    { label: 'Key bindings', help: 'Change the explorer keys, saved in keys.json.',
      ...dialog(changedActions().length ? `${changedActions().length} changed` : 'default', () => openKeys()) },
    { label: 'Check for updates', help: 'Look for a newer release on GitHub, once a day.', ...onOff('updateCheck') },
  ];
}

// The update to install, or the one the installer must finish: [text, colour, action], or null.
const pendingUpdate = () => (finish ? ['! Finish the update', C.yellow, finishUpdate]
  : latest && settings.updateCheck && !updating ? [`↑ ${latest} available`, C.green, askUpdate] : null);

function openSettings() {
  state.menu = null;
  state.settings = { sel: 1, top: 0 };
  render();
  return null;
}

// --- settings pages: the settings, then the icon themes and the key bindings, opened from them. Each one takes the
// place of the tree: its title (or an action message), a list scrolled with the selection, a rule, what the page
// shows of the selection, its keys.

const pageTitle = (title, W) => line(state.flash ? ui.message(state.flash) : [[' ‹ ', fg(C.dim)], [title, '\x1b[1m']], W);

// The rows a find keeps: those whose label, value or help has the text typed (any case), under their group title.
function found(list, p) {
  if (!p.find || !p.find.value) return list;
  const q = p.find.value.toLowerCase(), out = [];
  let group = null;
  for (const r of list) {
    if (r.group) { group = r; continue; }
    if (!`${r.label} ${r.value[0]} ${r.help || ''} ${r.action || ''}`.toLowerCase().includes(q)) continue;
    if (group) { out.push(group); group = null; }
    out.push(r);
  }
  return out;
}
const firstRow = list => Math.max(0, list.findIndex(r => !r.group));
const sameRow = (a, b) => a && b && !a.group && !b.group && a.label === b.label && a.action === b.action;

// The find of a page (/ or the key bound to filter): typed characters go to it and the list keeps the rows that
// match; the arrows, Enter and the other keys that are not characters go to the page. Escape closes it, the selection
// staying on its row. all: the page's rows. Returns true when the key was the find's.
function findKey(p, k, all) {
  if (!p.find) {
    if (k !== '/' && keys.get(k) !== 'filter') return false;
    p.find = new ui.Field('');
    return true;
  }
  const r = found(all, p)[p.sel];
  if (k === 'escape') {
    p.find = null;
    const i = all.findIndex(x => sameRow(x, r));
    p.sel = i >= 0 ? i : firstRow(all);
    return true;
  }
  if (!ui.printable(k) && k !== 'space' && k !== 'backspace' && k !== 'delete') return false;
  if (p.find.key(k)) {
    const list = found(all, p), i = list.findIndex(x => sameRow(x, r));
    p.sel = i >= 0 ? i : firstRow(list);
    p.top = 0;
  }
  return true;
}

// The find's line: the field, and the count of rows kept.
function findLine(p, list, W) {
  const count = p.find.value ? ` ${list.filter(r => !r.group).length}` : '';
  const field = p.find.segments(Math.max(1, W - 4 - count.length));
  return line([[` ${glyph(SEARCH)} `, fg(C.dim)], ...field, [count.padStart(Math.max(0, W - 3 - ui.segWidth(field))), fg(C.dim)]], W, C.header);
}

// The rows of a list from p.top, n at most, the selection kept in view with the title of its group: a group title,
// or a label with a value on the right ([text, style]).
function listLines(list, p, n, W) {
  const first = list[p.sel - 1] && list[p.sel - 1].group ? p.sel - 1 : p.sel;
  p.top = Math.max(0, Math.min(p.top, first, list.length - n));
  if (p.sel >= p.top + n) p.top = p.sel - n + 1;
  const lines = list.slice(p.top, p.top + n).map((r, k) => {
    if (r.group) return line([[` ${r.group}`, '\x1b[1m' + fg(C.dim)]], W);
    // The value keeps at least half the room, the label is cut first.
    const value = ui.ellipsis(r.value[0], Math.max(1, W - 4 - Math.min(textWidth(r.label), Math.floor((W - 4) / 2))));
    const label = ui.ellipsis(r.label, Math.max(1, W - 4 - textWidth(value)));
    const pad = ' '.repeat(Math.max(1, W - 3 - textWidth(label) - textWidth(value)));
    const back = p.top + k === p.sel ? (state.focused ? C.select : C.selectBlur) : null;
    return line([[`  ${label}${pad}`, ''], [value, r.value[1]]], W, back);
  });
  if (!list.length) lines.push(...ui.empty(W, n, 'No match', [['esc', 'clear']]));
  while (lines.length < n) lines.push(line([], W));
  return lines;
}

// The rule under a list, with ▴ ▾ when it scrolls.
function rule(list, p, n, W) {
  const more = (p.top > 0 ? '▴' : '') + (p.top + n < list.length ? '▾' : '');
  return line([['─'.repeat(Math.max(0, W - more.length - 1)) + more, fg(C.dim)]], W);
}

// The selection moved by dir rows, past the group titles, within the list.
function moveIn(list, p, dir) {
  const step = dir < 0 ? -1 : 1, ok = i => list[i] && !list[i].group;
  let i = Math.max(0, Math.min(list.length - 1, p.sel + dir));
  if (!ok(i)) i = ok(i + step) ? i + step : i - step;
  p.sel = i;
}

// The page shown: the settings, or one opened from them.
const settingsPage = () => (state.settings.icons ? { view: iconsView, key: iconsKey, mouse: iconsMouse }
  : state.settings.keys ? { view: keysView, key: keysKey, mouse: keysMouse }
    : { view: settingsView, key: settingsKey, mouse: settingsMouse });

// The moves of the bound keys, page by page of n rows. ARROWS: the keys that move in a page whatever the bindings.
const ARROWS = { up: 'up', down: 'down', pageup: 'pageUp', pagedown: 'pageDown', home: 'top', end: 'bottom' };
function moves(n) {
  const page = n - 1;
  return { up: -1, down: 1, pageUp: -page, pageDown: page, halfPageUp: -Math.ceil(page / 2), halfPageDown: Math.ceil(page / 2),
    top: -Infinity, bottom: Infinity };
}

// --- the settings

// The settings view: the find when open and the update when there is one under the title, the settings, the help of
// the selected one, the keys, and a prompt. at: screen row of the first setting, n: the rows they get, update: the
// row of the update (0: none), detail: the help's lines.
function settingsLayout(H, W) {
  const find = state.settings.find ? 1 : 0, update = pendingUpdate() ? 1 + find : 0;
  const at = 1 + find + (update ? 1 : 0), detail = detailLines(W);
  return { at, n: Math.max(1, H - at - 2 - detail - (state.prompt ? 1 : 0)), update, detail };
}
// The lines of the longest help at this width, whichever setting is selected, so that the list does not move: from
// 1 to MAX_DETAIL.
const MAX_DETAIL = 4;
const detailLines = W => Math.min(MAX_DETAIL, Math.max(1, ...settingsRows().filter(r => r.help).map(r => ui.wrap(r.help, W - 2).length)));
const settingsKeys = W => ui.keys([['←→', 'change'], ['/', 'find'], ['esc', 'back']], W);

function settingsView(W, H) {
  const s = state.settings, list = found(settingsRows(), s), up = pendingUpdate(), { n, detail } = settingsLayout(H, W);
  const lines = [pageTitle('Settings', W)];
  if (s.find) lines.push(findLine(s, list, W));
  if (up) {
    // The key on the right, left out when it does not fit.
    let hint = ui.keys([['u', finish ? 'finish' : 'update']]);
    if (textWidth(up[0]) + 2 + ui.segWidth(hint) > W) hint = [];
    lines.push(line([[` ${up[0]}`.padEnd(Math.max(0, W - 1 - ui.segWidth(hint))), fg(up[1])], ...hint], W));
  }
  lines.push(...listLines(list, s, n, W), rule(list, s, n, W));
  const r = list[s.sel];
  // The help wrapped, its last line cut when it needs more.
  const help = ui.wrap(r && !r.group ? r.help : '', W - 2);
  if (help.length > detail) help.splice(detail - 1, help.length, ui.ellipsis(help.slice(detail - 1).join(' '), W - 2));
  for (let i = 0; i < detail; i++) lines.push(line([[` ${help[i] || ''}`, fg(C.dim)]], W));
  lines.push(line(settingsKeys(W), W, C.header));
  if (state.prompt) lines.push(promptLine(W));
  while (lines.length < H) lines.push(line([], W));
  lines.length = H;
  return lines;
}

// ← → and enter change the selected setting, u installs the update. Escape, and the keys bound to close and
// settings, go back to the tree.
function settingsKey(k) {
  const s = state.settings;
  if (findKey(s, k, settingsRows())) return render();
  const list = found(settingsRows(), s), r = list[s.sel], action = keys.get(k), up = pendingUpdate();
  const move = moves(settingsLayout(out.rows || 24, out.columns || 80).n), to = ARROWS[k] || action;
  if (k === 'escape' || action === 'close' || action === 'settings') state.settings = null;
  else if (k === 'u' && up) return runAction(up[2]);
  else if (to in move) moveIn(list, s, move[to]);
  else if (r && !r.group && (k === 'enter' || k === 'right' || action === 'open' || action === 'expand')) return runAction(() => r.change(1));
  else if (r && !r.group && (k === 'left' || action === 'collapse')) return runAction(() => r.change(-1));
  render();
}

// The wheel moves the selection. A click on the title goes back, on the update installs it, on a setting changes it,
// on a key hint acts as its key; elsewhere, it cancels a prompt.
function settingsMouse(m) {
  const s = state.settings, list = found(settingsRows(), s), W = out.columns || WIDTH, y = m.y - 1;
  const { at, n, update, detail } = settingsLayout(out.rows || 24, out.columns || 80);
  if (m.wheel) { moveIn(list, s, m.wheel); return render(); }
  if (!m.press || m.drag || m.button !== 0) return;
  if (state.prompt) { state.prompt = null; return render(); }
  if (y === 0) { state.settings = null; return render(); }
  if (update && y === update) return runAction(pendingUpdate()[2]);
  const r = list[s.top + y - at];
  if (y >= at && y < at + n && r && !r.group) {
    s.sel = s.top + y - at;
    return runAction(() => r.change(1));
  }
  // The keys come after the list, its rule and the help lines.
  const k = y === at + n + 1 + detail && ui.keyAt(settingsKeys(W), m.x - 1);
  if (k) settingsKey(k);
}

// --- icon themes

// The default icons and the VS Code themes installed (a scan of the editors' extensions, made when the page opens),
// above a sample tree drawn with the theme under the selection. Enter saves it and goes back to the settings; Escape
// goes back without a change.
const NERD_FONT = { id: '', label: 'Nerd Font (default)' };
const SAMPLE = [
  [0, 'src', true, true], [1, 'components', true], [1, 'App.tsx'], [1, 'main.py'], [1, 'styles.css'],
  [0, '.github', true], [0, 'docs', true], [0, '.gitignore'], [0, 'Dockerfile'], [0, 'logo.png'],
  [0, 'package.json'], [0, 'README.md'],
];

function openIconThemes() {
  if (!state.settings) return null; // closed by a key typed meanwhile (the action runs after the keys already read)
  const current = (settings.icons && settings.icons.id) ? settings.icons : NERD_FONT;
  const found = icons.themes();
  // The saved theme kept in the list when the scan misses it (an editor folder out of reach).
  const list = [NERD_FONT, ...(current.id && !found.some(t => t.id === current.id) ? [current] : []), ...found];
  state.settings.icons = { list, sel: Math.max(0, list.findIndex(t => t.id === current.id)), top: 0, loaded: new Map() };
  return null;
}

// The theme of an entry of the list, loaded once (null: the default icons, or a theme not found).
function loadTheme(t) {
  const { loaded } = state.settings.icons;
  if (!loaded.has(t.id)) loaded.set(t.id, t.id ? icons.load(t) : null);
  return loaded.get(t.id);
}

function chooseTheme(t) {
  const chosen = loadTheme(t);
  if (t.id && !chosen) return `${t.label}: not found`;
  theme = chosen;
  settings.icons = chosen ? chosen.entry : '';
  writeSettings({ ...readSettings(), icons: settings.icons });
  if (state.settings) state.settings.icons = null;
  return null;
}

// The themes as list rows, the saved one checked.
const themeRows = () => state.settings.icons.list.map(t => ({
  label: t.label, value: [t.id === ((settings.icons && settings.icons.id) || '') ? '✓' : '', fg(C.green)],
}));

// The sample gets at least 4 rows, the themes what remains. at: screen row of the first theme, n: the rows they get.
function iconsLayout(H) {
  const room = H - 3, count = state.settings.icons.list.length;
  const sample = Math.max(0, Math.min(SAMPLE.length, Math.max(Math.min(4, room - 1), room - count)));
  return { at: 1, n: Math.max(1, Math.min(count, room - sample)), sample };
}
const iconsKeys = W => ui.keys([['↑↓', 'move'], ['⏎', 'choose'], ['esc', 'back']], W);

function iconsView(W, H) {
  const p = state.settings.icons, list = themeRows(), { n, sample } = iconsLayout(H);
  const lines = [pageTitle('Icon theme', W), ...listLines(list, p, n, W), rule(list, p, n, W)];
  // The sample tree, in the theme under the selection.
  const th = loadTheme(p.list[p.sel]);
  for (const [depth, name, dir, open] of SAMPLE.slice(0, sample)) {
    const icon = icons.icon(th, name, dir, open);
    const chevron = dir ? glyph(CHEVRON[open ? 1 : 0]) : ' ';
    lines.push(line([[' '.repeat(1 + depth * 2) + chevron + ' ', fg(C.dim)], [glyph(icon[0]) + ' ', fg(icon[1])], [name, '']], W));
  }
  while (lines.length < H - 1) lines.push(line([], W));
  lines.push(line(iconsKeys(W), W, C.header));
  lines.length = H;
  return lines;
}

function iconsKey(k) {
  const p = state.settings.icons, action = keys.get(k), move = moves(iconsLayout(out.rows || 24).n);
  if (k === 'escape' || ['close', 'settings', 'collapse'].includes(action)) state.settings.icons = null;
  else if (action in move) moveIn(p.list, p, move[action]);
  else if (action === 'open' || action === 'expand') return runAction(() => chooseTheme(p.list[p.sel]));
  render();
}

// The wheel moves the selection; a click on the title goes back, on a theme chooses it, on a key hint acts as its key.
function iconsMouse(m) {
  const p = state.settings.icons, W = out.columns || WIDTH, H = out.rows || 24, { at, n } = iconsLayout(H), y = m.y - 1;
  if (m.wheel) { moveIn(p.list, p, m.wheel); return render(); }
  if (!m.press || m.drag || m.button !== 0) return;
  if (y === 0) { state.settings.icons = null; return render(); }
  if (y >= at && y < at + n && p.list[p.top + y - at]) {
    p.sel = p.top + y - at;
    return runAction(() => chooseTheme(p.list[p.sel]));
  }
  const k = y === H - 1 && ui.keyAt(iconsKeys(W), m.x - 1);
  if (k) iconsKey(k);
}

// --- key bindings

// The explorer's actions by group, as in the key help, each with its keys; below the rule, the selected action's
// keys and its default ones. Enter waits for a key and adds it to the action (taken from the action that had it),
// Delete removes the action's last key, r gives it back its default keys. Saved in keys.json, which the last row
// opens in the editor. The page's own keys are the arrows and the bound ones, so that a rebinding cannot lock it.
function openKeys() {
  if (!state.settings) return null;
  state.settings.keys = { sel: 1, top: 0, find: null, capture: false };
  return null;
}

// The keys of an action, its default ones first in their order.
const keysOf = action => {
  const rank = k => { const i = DEFAULT_KEYS[action].indexOf(k); return i < 0 ? Infinity : i; };
  return [...keys].filter(([, a]) => a === action).map(([k]) => k).sort((a, b) => rank(a) - rank(b));
};
const sameKeys = (a, b) => a.length === b.length && a.every(k => b.includes(k));
const changedActions = () => Object.keys(DEFAULT_KEYS).filter(a => !sameKeys(keysOf(a), DEFAULT_KEYS[a]));
const keyText = list => list.map(ui.keyLabel).join(' ') || '—';

function keyRows() {
  return [
    ...HELP.flatMap(([group, items]) => [{ group: group.toUpperCase() }, ...items.map(([action, text]) => ({
      action, label: text[0].toUpperCase() + text.slice(1),
      value: [keyText(keysOf(action)), fg(sameKeys(keysOf(action), DEFAULT_KEYS[action]) ? C.blue : C.yellow)],
    }))]),
    { group: 'FILE' },
    { label: 'Open keys.json', value: ['edit…', fg(C.blue)] },
  ];
}

// The changes saved, then read back at once rather than at the next check of the file.
function saveKeys(changes) {
  if (!writeKeys('explorer', changes)) return 'Cannot write keys.json';
  keys = readKeys('explorer', DEFAULT_KEYS);
  return null;
}

// The default keys of an action given back, taken from the actions that have them now; the keys it took from other
// actions given back to them.
function resetKeys(action) {
  const changes = { [action]: null };
  const now = a => (a in changes ? changes[a] || DEFAULT_KEYS[a] : keysOf(a));
  for (const k of DEFAULT_KEYS[action]) {
    const owner = keys.get(k);
    if (owner && owner !== action) changes[owner] = now(owner).filter(x => x !== k);
  }
  for (const k of keysOf(action)) {
    const home = Object.keys(DEFAULT_KEYS).find(a => a !== action && DEFAULT_KEYS[a].includes(k));
    if (home && !now(home).includes(k)) changes[home] = [...now(home), k];
  }
  for (const a of Object.keys(changes)) if (changes[a] && sameKeys(changes[a], DEFAULT_KEYS[a])) changes[a] = null;
  return saveKeys(changes);
}

// The key pressed while waiting, added to the action and taken from the one that had it.
function bindKey(action, k) {
  const owner = keys.get(k);
  if (owner === action) return `${ui.keyLabel(k)}: already bound`;
  const changes = { [action]: [...keysOf(action), k] };
  if (owner) changes[owner] = keysOf(owner).filter(x => x !== k);
  const label = owner && HELP.flatMap(([, items]) => items).find(([a]) => a === owner);
  return saveKeys(changes) || (owner ? `${ui.keyLabel(k)} taken from ${label ? label[1] : owner}` : null);
}

function keysLayout(H) {
  const find = state.settings.keys.find ? 1 : 0;
  return { at: 1 + find, n: Math.max(1, H - 7 - find) };
}
const keysKeys = (W, capture) => ui.keys(capture ? [['esc', 'cancel']] : [['⏎', 'add'], ['del', 'remove'], ['/', 'find']], W);

function keysView(W, H) {
  const p = state.settings.keys, list = found(keyRows(), p), r = list[p.sel], { n } = keysLayout(H);
  const lines = [pageTitle('Key bindings', W), ...(p.find ? [findLine(p, list, W)] : []), ...listLines(list, p, n, W), rule(list, p, n, W)];
  if (r && r.action) {
    const now = keysOf(r.action), changed = !sameKeys(now, DEFAULT_KEYS[r.action]);
    lines.push(line([[` ${r.label}`, '\x1b[1m'], [changed ? '  changed' : '', fg(C.yellow)]], W));
    lines.push(line([[' Keys     ', fg(C.dim)], [keyText(now), fg(changed ? C.yellow : C.blue)]], W));
    lines.push(line([[' Default  ', fg(C.dim)], [keyText(DEFAULT_KEYS[r.action]), fg(C.dim)]], W));
    // r, which the keys line has no room for, told where it applies (typed in the find while it is open).
    lines.push(line(p.capture ? [[' Press the new key…', fg(C.yellow)]] : changed && !p.find ? ui.keys([['r', 'back to the default keys']], W) : [], W));
  } else if (r && !r.group) {
    lines.push(line([[' keys.json', '\x1b[1m']], W), line([[` ${ui.ellipsis('Opened in the editor', W - 2)}`, fg(C.dim)]], W));
  }
  while (lines.length < H - 1) lines.push(line([], W));
  lines.push(line(keysKeys(W, p.capture), W, C.header));
  lines.length = H;
  return lines;
}

function keysKey(k) {
  const p = state.settings.keys;
  if (p.capture) {
    const r = found(keyRows(), p)[p.sel];
    p.capture = false;
    return k === 'escape' ? render() : runAction(() => bindKey(r.action, k));
  }
  if (findKey(p, k, keyRows())) return render();
  const list = found(keyRows(), p), r = list[p.sel], action = keys.get(k);
  const move = moves(keysLayout(out.rows || 24).n), to = ARROWS[k] || action;
  if (k === 'escape' || k === 'left' || ['close', 'settings', 'collapse'].includes(action)) state.settings.keys = null;
  else if (to in move) moveIn(list, p, move[to]);
  else if (!r || r.group) return render();
  else if (k === 'enter' || action === 'open' || action === 'expand') {
    if (!r.action) return runAction(() => { const file = keysTemplate('explorer', DEFAULT_KEYS); return file ? actions.edit(file, PANE) : 'No config folder'; });
    p.capture = true;
  } else if (r.action && (k === 'delete' || k === 'backspace')) {
    const now = keysOf(r.action);
    return runAction(() => (now.length ? saveKeys({ [r.action]: now.slice(0, -1) }) : 'No key to remove'));
  } else if (r.action && k === 'r') return runAction(() => resetKeys(r.action));
  render();
}

// The wheel moves the selection. A click on the title goes back, on an action selects it (on the selected one, waits
// for its new key), on a key hint acts as its key; any click while waiting cancels.
function keysMouse(m) {
  const p = state.settings.keys, list = found(keyRows(), p), W = out.columns || WIDTH, H = out.rows || 24, { at, n } = keysLayout(H), y = m.y - 1;
  if (m.wheel) { if (!p.capture) moveIn(list, p, m.wheel); return render(); }
  if (!m.press || m.drag || m.button !== 0) return;
  if (p.capture) { p.capture = false; return render(); }
  if (y === 0) { state.settings.keys = null; return render(); }
  const i = p.top + y - at;
  if (y >= at && y < at + n && list[i] && !list[i].group) {
    if (i === p.sel) return keysKey('enter');
    p.sel = i;
    return render();
  }
  const k = y === H - 1 && ui.keyAt(keysKeys(W, false), m.x - 1);
  if (k) keysKey(k);
}

// --- key help

// Every key of the explorer, as bound now, over the whole pane (ui.help, as in the other panes).
function openHelp() {
  state.help = { top: 0, find: null };
  render();
}

const helpGroups = () => HELP.map(([group, items]) => [group, items.map(([action, text]) => [keysOf(action).join(' ') || '—', text])]);

// --- keyboard and mouse

// A question (ui.confirmKey), or a prompt.
function promptKey(k) {
  if (state.confirm) {
    const q = state.confirm, answer = ui.confirmKey(k, q);
    if (answer === 'move') return render();
    state.confirm = null;
    if (answer === 'yes') return runAction(q.yes);
    return render();
  }
  const p = state.prompt;
  if (k === 'escape') state.prompt = null;
  else if (k === 'enter') { state.prompt = null; const v = p.field.value; if (v.trim() || p.empty) runAction(() => p.done(v)); }
  else p.field.key(k);
  render();
}

// While filtering, typed characters go to the filter; arrows and Enter act on the list.
const FILTER_EDIT = new Set(['backspace', 'delete', 'ctrl+u', 'ctrl+d']);
function filterKey(k) {
  const f = state.filter;
  if (k === 'escape') return closeFilter();
  // Bound keys that are not characters (arrows, Enter, ctrl+…) act on the list; characters and the field's editing
  // keys go to the filter (ctrl+u and ctrl+d move half a page in the tree only).
  if (keys.has(k) && !ui.printable(k) && !FILTER_EDIT.has(k)) return listKey(k);
  if (k === 'tab') { f.active = false; return render(); }
  if (f.field.key(k)) {
    state.selected = null;
    if (!f.field.value) { state.filter = { field: f.field, active: true }; }
    buildRows();
    state.top = 0;
    reveal();
    render();
  }
}

function closeFilter() {
  const sel = state.selected;
  state.filter = null;
  if (sel) revealRel(sel); else buildRows();
  render();
}

function key(k) {
  if (state.confirm || state.prompt) return promptKey(k);
  if (state.menu) return menuKey(k);
  if (state.settings) return settingsPage().key(k);
  if (state.help) { if (!ui.helpKey(k, state.help)) state.help = null; return render(); }
  if (state.filter && state.filter.active) return filterKey(k);
  if (state.filter && (k === 'escape' || keys.get(k) === 'close')) return closeFilter();
  listKey(k);
}

function listKey(k) {
  listAction(keys.get(k));
}

// An action of the tree, from its key.
function listAction(action) {
  const r = state.rows[state.cursor];
  const page = bodyHeight() - 1;
  const dirOf = () => (r ? (r.dir ? r.rel : parentOf(r.rel)) : '');
  switch (action) {
    case 'up': select(state.cursor - 1); break;
    case 'down': select(state.cursor + 1); break;
    case 'pageUp': select(state.cursor - page); break;
    case 'pageDown': select(state.cursor + page); break;
    case 'halfPageUp': select(state.cursor - Math.ceil(page / 2)); break;
    case 'halfPageDown': select(state.cursor + Math.ceil(page / 2)); break;
    case 'help': return openHelp();
    case 'close': return close();
    case 'top': select(0); break;
    case 'bottom': select(state.rows.length - 1); break;
    case 'open':
      if (r && !r.dir) return openFile(r);
      toggle(r);
      break;
    case 'menu': return openMenu(state.rows.length ? state.cursor : null);
    case 'expand':
      if (r && r.dir && !r.open) toggle(r, true);
      else if (r && r.dir) select(state.cursor + 1);
      break;
    case 'collapse':
      if (r && r.dir && r.open && !state.filter) toggle(r, false);
      else if (r) { const p = state.rows.findIndex(x => x.rel === parentOf(r.rel)); if (p >= 0) select(p); }
      break;
    case 'refresh': schedule(); return;
    case 'filter': state.filter = { field: new ui.Field(state.filter ? state.filter.field.value : ''), active: true }; buildRows(); break;
    case 'goToFile': return runAction(() => actions.search(ROOT, PANE, 'files'));
    case 'searchInFiles': return runAction(() => actions.search(r && r.dir ? absOf(r.rel) : ROOT, PANE, 'content'));
    case 'sourceControl': return runAction(() => (git.top ? actions.scm(ROOT, PANE, r ? absOf(r.rel) : '') : 'Not a git repository'));
    case 'settings': return openSettings();
    case 'toggleHidden': setSetting('hidden', !settings.hidden); flash(settings.hidden ? 'Hidden files shown' : 'Hidden files hidden'); return;
    case 'toggleIgnored': setSetting('ignored', !settings.ignored); flash(settings.ignored ? 'Ignored files shown' : 'Ignored files hidden'); return;
    case 'newFile': return newEntry(dirOf(), false);
    case 'newFolder': return newEntry(dirOf(), true);
    case 'rename': if (r) renameEntry(r); return;
    case 'delete': if (r) deleteEntry(r); return;
    case 'edit': if (r && !r.dir) runAction(() => actions.edit(absOf(r.rel), PANE)); return;
    case 'openInEditor': if (r) runAction(() => actions.editor(absOf(r.rel), PANE)); return;
    case 'openInApp': if (r && !r.dir) runAction(() => actions.system(absOf(r.rel))); return;
    case 'preview': if (r && !r.dir) runAction(() => actions.preview(absOf(r.rel), ROOT, PANE, 0, true)); return;
    case 'copyPath': if (r) runAction(() => actions.copy(absOf(r.rel))); return;
    case 'copyRelativePath': if (r) runAction(() => actions.copy(r.rel)); return;
    case 'parentRoot': return setRoot(path.dirname(ROOT));
    case 'narrower': return widen(-4);
    case 'wider': return widen(4);
    default: return;
  }
  reveal();
  render();
}

function mouse(m) {
  const H = out.rows || 24, W = out.columns || WIDTH;
  // A question: a click on its first button confirms, any other click cancels.
  if (state.confirm) {
    const q = state.confirm, answer = ui.confirmMouse(m, q);
    if (!answer) return;
    state.confirm = null;
    if (answer === 'yes') return runAction(q.yes);
    return render();
  }
  if (m.wheel && state.menu) { moveMenu(m.wheel); return render(); }
  if (state.settings) return settingsPage().mouse(m);
  if (state.help) { if (!ui.helpMouse(m, state.help)) state.help = null; return render(); }
  if (m.wheel) {
    const max = Math.max(0, state.rows.length - bodyHeight());
    state.top = Math.max(0, Math.min(max, state.top + m.wheel * 3));
    return render();
  }
  if (scrollbar.mouse(m, m.y - 2, m.x === W && !state.menu)) return;
  if (!m.press || m.drag || (m.button !== 0 && m.button !== 2)) return;
  const btn = m.button, { x, y } = m;
  if (state.prompt) { state.prompt = null; return render(); }
  if (state.menu) {
    // Click in the menu: action; elsewhere: close the menu (a right-click reopens it on the new row).
    const k = y - 2 - state.menu.at;
    if (btn === 0 && k >= 0 && k < menuRows()) {
      const item = state.menu.items[state.menu.off + k];
      return item ? runAction(item[1]) : undefined;
    }
    closeMenu();
    if (btn === 0) return render();
  }
  if (y === 1) {
    if (btn !== 0) return render();
    // Each icon and the cell after it: a Nerd Font glyph often spills over the next cell.
    if (x >= W - 2) return openSettings();
    if (x >= W - 4 && showUpdate()) return runAction(finish ? finishUpdate : askUpdate);
    return render();
  }
  if (y > bodyHeight() + 1) { if (state.filter) { state.filter.active = true; render(); } return; }
  const i = state.top + y - 2;
  if (i >= state.rows.length) return btn === 2 ? openMenu(null) : render();
  if (btn === 2) return openMenu(i);
  if (state.filter) state.filter.active = false;
  const r = state.rows[i];
  const now = Date.now(), double = state.lastClick && state.lastClick.i === i && now - state.lastClick.t < 400;
  state.lastClick = double ? null : { i, t: now };
  select(i);
  if (r.dir) toggle(r);
  else if (double) return openFile(r);
  else if (settings.singleClick && actions.canPreview(absOf(r.rel))) return runAction(() => actions.preview(absOf(r.rel), ROOT, PANE));
  render();
}

// --- startup

const paint = ui.screen({
  key, mouse,
  focus: f => { state.focused = f; render(); },
  paste: text => {
    if (state.prompt) state.prompt.field.insert(text);
    else if (state.filter && state.filter.active) { state.filter.field.insert(text); buildRows(); }
    render();
  },
  resize: () => { resized(); reveal(); render(); },
}, { motion: true });

watchRoot();
const timers = [
  setInterval(schedule, 30000), // safety net in case a file event is lost
  setInterval(follow, 2000),
];

// Updated plugin: this explorer starts again on the new code, at the same root, once nothing is being typed or
// chosen. This process stops watching and acting, the new one taking over the pane.
update.watch(() => !updating && !state.prompt && !state.confirm && !state.menu && !state.settings && !state.help && !(state.filter && state.filter.active), () => {
  stopped = true;
  save();
  writePaneState(PANE, { home: HOME, root: ROOT });
  for (const t of timers) clearInterval(t);
  clearTimeout(settle); clearTimeout(timer);
  for (const w of [rootWatcher, gitWatcher]) if (w) w.close();
  for (const f of [KEYS_FILE, SETTINGS_FILE, update.FILE]) if (f) fs.unwatchFile(f);
  ui.restart({ EXPLORER_TREE: ROOT });
});

// Right-clicks in this pane go to the explorer (its menu) instead of herdr's menu; the frame keeps herdr's.
if (PANE) run(HERDR, ['pane', 'input', PANE, '--right-click', 'pane']);

writePaneState(PANE, { home: HOME, root: ROOT });
load();
buildRows();
render();
initGit().then(refresh);
for (const ms of [300, 1000, 2500]) setTimeout(ensureWidth, ms);
