// File explorer of the space, in a herdr pane: tree folded with the mouse and the keyboard, VS Code-style
// git status, filter as you type, file operations, automatic refresh. Enter or double-click: preview (or the editor);
// right-click: action menu (actions.js). Header icons: search, source control, settings, « to close.
'use strict';
const fs = require('fs');
const path = require('path');
const actions = require('./actions');
const icons = require('./icons');
const ui = require('./ui');
const { run, result, HERDR, GIT: GIT_BIN, gitEnv, WIDTH_FILE, readWidth, readSettings, writeSettings, KEYS_FILE, readKeys,
  keysTemplate, writePaneState, SETTINGS_FILE } = require('./exec');

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
  rename: ['R', 'f2'], delete: ['delete'], edit: ['e'], openInEditor: ['o'], preview: ['p'], copyPath: ['y'],
  copyRelativePath: ['Y'], parentRoot: ['backspace'],
};
let keys = readKeys('explorer', DEFAULT_KEYS);
if (KEYS_FILE) fs.watchFile(KEYS_FILE, { interval: 1000 }, () => { keys = readKeys('explorer', DEFAULT_KEYS); });

const CHEVRON = [0xeab6, 0xeab4];
const BRANCH = 0xe0a0;
const SEARCH = 0xf002, SCM = 0xe725, GEAR = 0xf013;
let theme = icons.load(settings.icons); // VS Code icon theme; null: the default icons
// Moved by an update of its extension: its new place saved for the next starts.
if (theme && theme.entry.file !== settings.icons.file) {
  settings.icons = theme.entry;
  writeSettings({ ...readSettings(), icons: theme.entry });
}
// The icon theme chosen in another explorer.
if (SETTINGS_FILE) fs.watchFile(SETTINGS_FILE, { interval: 1000 }, () => {
  const chosen = readSettings().icons;
  if (JSON.stringify(chosen) === JSON.stringify(settings.icons)) return;
  settings.icons = chosen;
  theme = icons.load(chosen);
  render();
});

const state = {
  expanded: new Set(['']), rows: [], cursor: 0, top: 0, selected: null, focused: true,
  menu: null,      // { items: [[label, action] | null], at: screen row of the menu top, sel, keep }
  flash: null,     // short message shown in the header
  lastClick: null, // to detect double-clicks
  filter: null,    // { field, active }: rows limited to the names containing the text
  prompt: null,    // { label, field, done(value) }: a name typed at the bottom
  confirm: null,   // { text, yes() }: y to confirm, any other key to cancel
};
const git = { top: null, prefix: '', branch: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set() };
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
  if (dir === ROOT) return;
  const from = ROOT;
  save();
  ROOT = dir;
  writePaneState(PANE, { home: HOME, root: ROOT });
  state.expanded = new Set(['']); state.filter = null; state.top = 0; state.cursor = 0;
  state.selected = null;
  load();
  cache.clear();
  Object.assign(git, { top: null, prefix: '', branch: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set() });
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
  if (!res || root !== ROOT) return;
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
  const [status, branch] = await Promise.all([
    gitRun(['status', '--porcelain=v1', '-z', '--ignored=matching', '--ignore-submodules=all', '--', '.']),
    gitRun(['branch', '--show-current']),
  ]);
  if (status === null) return;
  git.branch = (branch || '').trim();
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
  if (!timer) timer = setTimeout(() => { timer = null; refresh(); }, 250);
}
async function refresh() {
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

// Enforced on opening and when the window is resized. A manual resize, with the window unchanged, becomes
// the new width, remembered for the next openings.
let width = readWidth();

let sizing = false, area = 0, ownResize = 0, settle = null;

async function getLayout() {
  const res = result(await run(HERDR, ['pane', 'layout', '--pane', PANE]));
  return res && res.layout;
}

async function ensureWidth() {
  if (!PANE || sizing) return;
  sizing = true;
  try {
    const layout = await getLayout();
    if (!layout) return;
    area = layout.area.width;
    const me = layout.panes.find(p => p.pane_id === PANE);
    if (!me || layout.zoomed || out.columns === width) return;
    const want = width + me.rect.width - out.columns; // the pane width includes its border
    // The split whose whole left part (right part when docked to the right) is the explorer.
    const right = SIDE === 'right';
    const split = layout.splits.find(s => s.direction === 'right' && s.rect.y === me.rect.y && s.rect.height === me.rect.height
      && (right ? s.rect.x + s.rect.width === me.rect.x + me.rect.width
        && Math.abs(Math.round(s.rect.width * (1 - s.ratio)) - me.rect.width) <= 1
        : s.rect.x === me.rect.x && Math.abs(Math.round(s.rect.width * s.ratio) - me.rect.width) <= 1));
    if (!split) return;
    // The ratio is that of the left part.
    const ratio = right ? 1 - (want + 0.25) / split.rect.width : (want + 0.25) / split.rect.width;
    const delta = ratio - split.ratio;
    ownResize = Date.now();
    await run(HERDR, ['pane', 'resize', '--pane', PANE, '--direction', delta > 0 ? 'right' : 'left',
      '--amount', Math.abs(delta).toFixed(6)]);
  } catch { } finally { sizing = false; }
}

// Waits for the end of the gesture (a drag sends one event per column) before deciding.
// Alone in the tab (the terminal has exited), the explorer closes, and the tab with it.
function resized() {
  clearTimeout(settle);
  settle = setTimeout(async () => {
    if (!PANE || sizing) return;
    const layout = await getLayout().catch(() => null);
    if (!layout) return;
    if (layout.zoomed) return;
    if (layout.panes.length === 1) return close();
    if (Date.now() - ownResize < 1000) return;
    if (layout.area.width !== area) return ensureWidth();
    if (out.columns !== width) {
      width = out.columns;
      try { fs.writeFileSync(WIDTH_FILE, String(width)); } catch { }
    }
  }, 400);
}

function close() {
  save();
  if (PANE) run(HERDR, ['pane', 'close', PANE]);
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
  // Retried once the menu, the filter or the prompt is closed.
  if (!cwd || cwd === lastCwd || state.filter || state.menu || state.prompt || state.confirm) return;
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

const bodyHeight = () => (out.rows || 24) - 1 - (state.filter || state.prompt || state.confirm ? 1 : 0);

function render() {
  const W = out.columns || WIDTH, H = out.rows || 24;
  const lines = [header(W)];
  const listH = bodyHeight();
  if (state.top > state.rows.length - listH) state.top = Math.max(0, state.rows.length - listH);
  for (let i = state.top; i < Math.min(state.rows.length, state.top + listH); i++) lines.push(row(state.rows[i], W, i === state.cursor));
  if (!state.rows.length) lines.push(line([[state.filter ? ' No match' : ' (empty)', fg(C.dim)]], W));
  while (lines.length < listH + 1) lines.push(line([], W));
  // The question cut to leave room for y/N in a narrow pane.
  if (state.confirm) lines.push(line([[` ${ui.ellipsis(state.confirm.text, W - 6)} `, fg(C.yellow)], ['y/N', '\x1b[1m']], W, C.header));
  else if (state.prompt) {
    const label = ` ${state.prompt.label}: `;
    lines.push(line([[label, fg(C.dim)], ...state.prompt.field.segments(W - textWidth(label))], W, C.header));
  } else if (state.filter) {
    const count = state.filter.field.value ? ` ${state.rows.filter(r => r.hit).length}` : '';
    const field = state.filter.active ? state.filter.field.segments(W - 4 - count.length) : [[state.filter.field.value, '']];
    lines.push(line([[` ${glyph(SEARCH)} `, fg(C.dim)], ...field, [count.padStart(Math.max(0, W - 3 - ui.segWidth(field))), fg(C.dim)]], W, C.header));
  }
  if (state.menu) drawMenu(lines, W);
  paint(lines.slice(0, H));
}

// Header: space name, git branch (or an action message), then the icons: search, source control, settings, «.
function header(W) {
  const icons = [[`${glyph(SEARCH)} ${glyph(SCM)} ${glyph(GEAR)} `, fg(C.dim)], ['«', fg(C.dim)]];
  const room = W - 9;
  if (state.flash) {
    const [text, w] = fit(` ${state.flash}`, room);
    return line([[text, fg(C.yellow)], [' '.repeat(Math.max(0, room - w + 1)), ''], ...icons], W);
  }
  const [name, nw] = fit(` ${path.basename(ROOT).toUpperCase() || ROOT}`, room);
  // A branch too long is cut with an ellipsis; without room for a few letters, it is left out rather than cut
  // down to its icon.
  const full = git.branch ? `  ${glyph(BRANCH)} ${git.branch}` : '';
  const branch = textWidth(full) <= room - nw ? full : room - nw >= 7 ? ui.ellipsis(full, room - nw) : '';
  const bw = textWidth(branch);
  return line([[name, ROOT === HOME ? '\x1b[1m' : '\x1b[1;3m'], [branch, fg(C.dim)], [' '.repeat(Math.max(0, room - nw - bw + 1)), ''], ...icons], W);
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
  // The part matching the filter, underlined and bold.
  const parts = r.hit && r.hit[1] <= name.length
    ? [[name.slice(0, r.hit[0]), color], [name.slice(r.hit[0], r.hit[1]), color + '\x1b[1;4m'], [name.slice(r.hit[1]), color]]
    : [[name, color]];
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

// empty: an empty answer is accepted (otherwise it cancels), and the cursor stays at the end.
function ask(label, value, done, empty = false) {
  state.menu = null;
  state.prompt = { label, field: new ui.Field(value), done, empty };
  // A new name: the cursor before the extension.
  const dot = value.lastIndexOf('.');
  if (dot > 0 && !empty) state.prompt.field.cursor = Array.from(value.slice(0, dot)).length;
  render();
}

function confirm(text, yes) {
  state.menu = null;
  state.confirm = { text, yes };
  render();
}

// After a change on disk: the new entry selected, its folder expanded.
function created(abs) {
  cache.clear();
  const rel = path.relative(ROOT, abs).split(path.sep).join('/');
  revealRel(rel);
  render();
}

function newEntry(dirRel, folder) {
  ask(folder ? 'New folder' : 'New file', '', name => {
    const [abs, err] = actions.create(absOf(dirRel), name, folder);
    if (err) return flash(err);
    if (dirRel) state.expanded.add(dirRel);
    created(abs);
  });
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
  });
}

function deleteEntry(r) {
  const abs = absOf(r.rel);
  confirm(`Move ${r.name} to the trash?`, async () => {
    flash('Moving to the trash…');
    if (await actions.trash(abs)) return flash(`${r.name} moved to the trash`);
    confirm(`No trash: delete ${r.name} for good?`, () => flash(actions.remove(abs) || `${r.name} deleted`));
  });
}

// --- action menu

function menuItems(r) {
  const items = [];
  const dirRel = r ? (r.dir ? r.rel : parentOf(r.rel)) : '';
  if (r) {
    const abs = absOf(r.rel);
    const deco = decoration(r);
    if (!r.dir) {
      if (actions.canPreview(abs)) items.push(['Preview', () => actions.preview(abs, ROOT, PANE)]);
      items.push(['Open in editor', () => actions.editor(abs, PANE)]);
      if (actions.canOpen(abs)) items.push(['Open in app', () => actions.system(abs)]);
      items.push(['Edit in terminal', () => actions.edit(abs, PANE)]);
    } else items.push(['Open in editor', () => actions.editor(abs, PANE)]);
    items.push(['Insert into terminal', () => actions.insert(abs, PANE)], null);
    items.push(['New file…', () => newEntry(dirRel, false)], ['New folder…', () => newEntry(dirRel, true)]);
    if (r.dir) items.push(['Search in folder…', () => actions.search(abs, PANE, 'content')], ['Set as root', () => setRoot(abs)]);
    items.push(['Rename…', () => renameEntry(r)], ['Duplicate', () => {
      const [to, err] = actions.duplicate(abs);
      if (err) return err;
      created(to);
    }], ['Delete…', () => deleteEntry(r)], null);
    items.push([actions.REVEAL, () => actions.reveal(abs)], ['Copy path', () => actions.copy(abs)],
      ['Copy relative path', () => actions.copy(r.rel)]);
    if (git.top && deco !== '!') {
      const changed = deco && deco !== '!';
      items.push(null);
      if (changed) items.push(['View changes', () => actions.diff(abs, ROOT, PANE)]);
      if (changed) items.push(['Stage', () => actions.stage(abs, ROOT)]);
      items.push(['Unstage', () => actions.unstage(abs, ROOT)]);
      if (changed) items.push(['Discard changes…', () => confirm(`Discard the changes of ${r.name}?`, () => runAction(() => actions.discard(abs, ROOT)))]);
    }
  } else {
    items.push(['New file…', () => newEntry('', false)], ['New folder…', () => newEntry('', true)], null);
    items.push(['Go to file…', () => actions.search(ROOT, PANE, 'files')], ['Search in files…', () => actions.search(ROOT, PANE, 'content')]);
    if (git.top) items.push(['Source control', () => actions.scm(ROOT, PANE)]);
    items.push(null, [actions.REVEAL, () => actions.reveal(ROOT)], ['Copy path', () => actions.copy(ROOT)], ['Up one level', () => setRoot(path.dirname(ROOT))]);
    if (ROOT !== HOME) items.push(['Back to the space folder', () => setRoot(HOME)]);
    items.push(['Refresh', () => schedule()], ['Settings…', () => openSettings()]);
  }
  return items;
}

// Settings: toggles that keep the menu open, saved for every explorer.
function settingsItems() {
  const check = on => (on ? '✓ ' : '  ');
  const flip = (name, value = !settings[name]) => () => {
    settings[name] = value;
    writeSettings({ ...readSettings(), [name]: value });
    cache.clear();
    buildRows();
    if (name === 'follow') { lastCwd = null; follow(); }
    if (name === 'side') return 'Applies to the next tabs';
    return null;
  };
  return [
    [check(settings.hidden) + 'Show hidden files', flip('hidden')],
    [check(settings.ignored) + 'Show git-ignored files', flip('ignored')],
    [check(settings.open === 'preview') + 'Open in the preview', flip('open', settings.open === 'preview' ? 'editor' : 'preview')],
    ['  Editor command…', () => ask('Editor', readSettings().editor, cmd => {
      settings.editor = cmd.trim();
      writeSettings({ ...readSettings(), editor: settings.editor });
      return settings.editor ? `Editor: ${settings.editor}` : 'Editor: default';
    }, true)],
    [check(settings.singleClick) + 'Preview on single click', flip('singleClick')],
    [check(settings.follow) + 'Follow the terminal folder', flip('follow')],
    [check(settings.side === 'right') + 'Dock on the right', flip('side', settings.side === 'right' ? 'left' : 'right')],
    ['  Icon theme…', () => openIconThemes()],
    ['  Edit key bindings…', () => {
      const file = keysTemplate('explorer', DEFAULT_KEYS);
      return file ? actions.edit(file, PANE) : 'No config folder';
    }],
  ];
}

// Icon themes: the current one and the default icons, then with Search… the VS Code ones installed (a scan of the
// editors' extensions, kept out of the start). The tree shows the theme under the selection; Escape goes back to
// the one saved.
const NERD_FONT = { id: '', label: 'Nerd Font (default)' };
function openIconThemes(found) {
  const current = (settings.icons && settings.icons.id) ? settings.icons : NERD_FONT;
  const list = found ? [NERD_FONT, ...found] : current.id ? [current, NERD_FONT] : [NERD_FONT];
  const loaded = new Map();
  const get = t => {
    if (!loaded.has(t.id)) loaded.set(t.id, icons.load(t));
    return loaded.get(t.id);
  };
  const items = () => [
    ...list.map(t => [(t.id === ((settings.icons && settings.icons.id) || '') ? '✓ ' : '  ') + t.label, () => {
      const chosen = get(t);
      if (t.id && !chosen) return `${t.label}: not found`;
      theme = chosen;
      settings.icons = chosen ? chosen.entry : '';
      writeSettings({ ...readSettings(), icons: settings.icons });
      return null;
    }]),
    ...(found ? [] : [['  Search…', () => openIconThemes(icons.themes())]]),
  ];
  const saved = () => get((settings.icons && settings.icons.id) ? settings.icons : NERD_FONT);
  const sel = Math.max(0, list.findIndex(t => t.id === current.id));
  state.menu = {
    items: items(), sel, off: 0, at: 1, keep: items,
    move: i => { theme = list[i] ? get(list[i]) : saved(); },
    cancel: () => { theme = saved(); },
  };
  // The saved theme, which a preview may have replaced, and the selection on screen.
  theme = saved();
  state.menu.off = Math.max(0, sel - menuRows() + 1);
  return null;
}

function openSettings() {
  state.menu = { items: settingsItems(), sel: 0, off: 0, at: 1, keep: settingsItems };
  render();
  return null;
}

function openMenu(i) {
  const r = i === null ? null : state.rows[i];
  if (r) { select(i); reveal(); }
  const items = menuItems(r);
  const H = out.rows || 24, y = r ? i - state.top + 1 : Math.min(H - 1, state.rows.length - state.top + 1);
  const h = Math.min(items.length, H - 2) + 2;
  state.menu = { items, sel: 0, off: 0, at: y + 1 + h <= H ? y + 1 : Math.max(0, Math.min(y - h, H - h)) };
  render();
}

// Rows of the menu that fit in the pane; a longer menu scrolls with its selection (▴ ▾ on its border).
const menuRows = () => Math.max(1, Math.min(state.menu.items.length, (out.rows || 24) - 2 - state.menu.at));

function drawMenu(lines, W) {
  const { items, sel, at, off } = state.menu;
  const n = menuRows();
  const inner = Math.min(W - 3, Math.max(...items.map(item => (item ? textWidth(item[0]) : 0))) + 2);
  const edge = fg(C.dim);
  const border = (l, r, more) => line([[` ${l}${(more ? '─'.repeat(Math.floor(inner / 2) - 1) + more : '').padEnd(inner, '─')}${r}`, edge]], W);
  lines[at] = border('╭', '╮', off > 0 && '▴');
  items.slice(off, off + n).forEach((item, k) => {
    lines[at + 1 + k] = item
      ? ` ${edge}│\x1b[39m${line([[` ${item[0]}`, '']], inner, off + k === sel ? C.select : C.selectBlur)}${edge}│\x1b[0m` + ' '.repeat(Math.max(0, W - inner - 3))
      : line([[' ├' + '─'.repeat(inner) + '┤', edge]], W);
  });
  lines[at + n + 1] = border('╰', '╯', off + n < items.length && '▾');
}

async function runAction(action) {
  const menu = state.menu;
  state.menu = menu && menu.keep ? { ...menu, items: menu.keep() } : null;
  render();
  flash(await Promise.resolve().then(action).catch(() => 'Action failed'));
  if (state.menu && state.menu.keep) { state.menu.items = state.menu.keep(); render(); }
}

function flash(text) {
  if (!text) return render();
  state.flash = text;
  render();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { state.flash = null; render(); }, 2000);
}

function moveMenu(dir) {
  const m = state.menu;
  do m.sel = (m.sel + dir + m.items.length) % m.items.length; while (!m.items[m.sel]);
  const n = menuRows();
  if (m.sel < m.off) m.off = m.sel;
  if (m.sel >= m.off + n) m.off = m.sel - n + 1;
  if (m.move) m.move(m.sel);
}

// Closed without choosing: the menu can undo what its selection showed.
function closeMenu() {
  const m = state.menu;
  state.menu = null;
  if (m && m.cancel) m.cancel();
}

// Escape always closes the menu, whatever the bindings.
function menuKey(k) {
  const m = state.menu, action = keys.get(k);
  if (action === 'up') moveMenu(-1);
  else if (action === 'down') moveMenu(1);
  else if (action === 'open' || action === 'expand') return runAction(m.items[m.sel][1]);
  else if (k === 'escape' || ['close', 'collapse', 'menu', 'settings'].includes(action)) closeMenu();
  render();
}

function reveal() {
  const listH = bodyHeight();
  if (state.cursor < state.top) state.top = state.cursor;
  if (state.cursor >= state.top + listH) state.top = state.cursor - listH + 1;
}

// --- keyboard and mouse

function promptKey(k) {
  if (state.confirm) {
    const { yes } = state.confirm;
    state.confirm = null;
    if (k === 'y' || k === 'Y') return runAction(yes);
    return render();
  }
  const p = state.prompt;
  if (k === 'escape') state.prompt = null;
  else if (k === 'enter') { state.prompt = null; const v = p.field.value; if (v.trim() || p.empty) runAction(() => p.done(v)); }
  else p.field.key(k);
  render();
}

// While filtering, typed characters go to the filter; arrows and Enter act on the list.
function filterKey(k) {
  const f = state.filter;
  if (k === 'escape') return closeFilter();
  // Bound keys that are not characters (arrows, Enter, ctrl+…) act on the list; characters go to the filter.
  if (keys.has(k) && !ui.printable(k) && k !== 'backspace' && k !== 'delete') return listKey(k);
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
  if (state.filter && state.filter.active) return filterKey(k);
  if (state.filter && (k === 'escape' || keys.get(k) === 'close')) return closeFilter();
  listKey(k);
}

function listKey(k) {
  const r = state.rows[state.cursor];
  const page = bodyHeight() - 1;
  const dirOf = () => (r ? (r.dir ? r.rel : parentOf(r.rel)) : '');
  switch (keys.get(k)) {
    case 'up': select(state.cursor - 1); break;
    case 'down': select(state.cursor + 1); break;
    case 'pageUp': select(state.cursor - page); break;
    case 'pageDown': select(state.cursor + page); break;
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
    case 'sourceControl': return runAction(() => (git.top ? actions.scm(ROOT, PANE) : 'Not a git repository'));
    case 'settings': return openSettings();
    case 'toggleHidden': settingsItems()[0][1](); flash(settings.hidden ? 'Hidden files shown' : 'Hidden files hidden'); return;
    case 'toggleIgnored': settingsItems()[1][1](); flash(settings.ignored ? 'Ignored files shown' : 'Ignored files hidden'); return;
    case 'newFile': return newEntry(dirOf(), false);
    case 'newFolder': return newEntry(dirOf(), true);
    case 'rename': if (r) renameEntry(r); return;
    case 'delete': if (r) deleteEntry(r); return;
    case 'edit': if (r && !r.dir) runAction(() => actions.edit(absOf(r.rel), PANE)); return;
    case 'openInEditor': if (r) runAction(() => actions.editor(absOf(r.rel), PANE)); return;
    case 'preview': if (r && !r.dir) runAction(() => actions.preview(absOf(r.rel), ROOT, PANE, 0, true)); return;
    case 'copyPath': if (r) runAction(() => actions.copy(absOf(r.rel))); return;
    case 'copyRelativePath': if (r) runAction(() => actions.copy(r.rel)); return;
    case 'parentRoot': return setRoot(path.dirname(ROOT));
    default: return;
  }
  reveal();
  render();
}

function mouse(m) {
  const H = out.rows || 24, W = out.columns || WIDTH;
  if (m.wheel && state.menu) { moveMenu(m.wheel); return render(); }
  if (m.wheel) {
    const max = Math.max(0, state.rows.length - bodyHeight());
    state.top = Math.max(0, Math.min(max, state.top + m.wheel * 3));
    return render();
  }
  if (!m.press || m.drag || (m.button !== 0 && m.button !== 2)) return;
  const btn = m.button, { x, y } = m;
  if (state.confirm || state.prompt) { state.confirm = null; state.prompt = null; return render(); }
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
    if (x >= W - 1) return close();
    if (x >= W - 3) return openSettings();
    if (x >= W - 5) return runAction(() => (git.top ? actions.scm(ROOT, PANE) : 'Not a git repository'));
    if (x >= W - 7) return runAction(() => actions.search(ROOT, PANE, 'files'));
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
});

watchRoot();
setInterval(schedule, 30000); // safety net in case a file event is lost
setInterval(follow, 2000);

// Right-clicks in this pane go to the explorer (its menu) instead of herdr's menu; the frame keeps herdr's.
if (PANE) run(HERDR, ['pane', 'input', PANE, '--right-click', 'pane']);

writePaneState(PANE, { home: HOME, root: ROOT });
load();
buildRows();
render();
initGit().then(refresh);
for (const ms of [300, 1000, 2500]) setTimeout(ensureWidth, ms);
