// File explorer of the space, in a herdr pane: tree folded with the mouse and the keyboard, VS Code-style
// git status, filter as you type, file operations, automatic refresh. Enter or double-click: preview (or the editor);
// right-click: action menu (actions.js). Header icon: settings.
'use strict';
// The pane's first process only starts the real one (launch.js).
if (require('./launch').launch()) return;
const fs = require('fs');
const path = require('path');
const actions = require('./core/actions');
const icons = require('./core/icons');
const ui = require('./core/ui');
const update = require('./core/update');
const Tree = require('./explorer/tree');
const { run, result, HERDR, GIT: GIT_BIN, gitEnv, DEFAULT_WIDTH, WIDTH_FILE, readWidth, readSettings, writeSettings, KEYS_FILE, readKeys,
  writePaneState, SETTINGS_FILE, remoteLink, writeAtomic } = require('./core/exec');

const HOME = path.resolve(process.env.EXPLORER_ROOT || process.argv[2] || process.cwd());
// the space folder, or another one chosen with Set as root (EXPLORER_TREE: the one it had before a herdr restart)
let ROOT = process.env.EXPLORER_TREE ? path.resolve(process.env.EXPLORER_TREE) : HOME;
const PANE = process.env.HERDR_PANE_ID;
const WIDTH = DEFAULT_WIDTH; // columns when the terminal size is unknown
const SAVE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'expanded.json');
const out = process.stdout;
const settings = readSettings();
ui.useGlyphs(settings.glyphs);
const SIDE = settings.side; // where this explorer was docked: a later change applies to the next tabs
// The side of its inner border, which shift+← and shift+→ push: on the right of an explorer docked on the left.
const INNER = SIDE === 'right' ? 'left' : 'right';

// Actions and their default keys, changed in keys.json ("explorer" object). Key names: up, down, left, right, home,
// end, pageup, pagedown, enter, space, tab, backspace, delete, escape, menu, f1…f5, ctrl+<letter>, alt+<key>, or the
// character itself ('/', 'n', 'N'). In the menu: up, down, open, expand, close; in the filter: the keys that are not
// characters.
const DEFAULT_KEYS = {
  up: ['up', 'k'], down: ['down', 'j'], pageUp: ['pageup'], pageDown: ['pagedown'], top: ['home', 'g'], bottom: ['end', 'G'],
  open: ['enter', 'space'], expand: ['right', 'l'], collapse: ['left', 'h'], close: ['escape', 'q'],
  menu: ['m', 'menu', 'shift+f10'], refresh: ['r'], filter: ['/'], goToFile: ['ctrl+p'], searchInFiles: ['ctrl+f'],
  sourceControl: ['s'], settings: [','], toggleHidden: ['.'], toggleIgnored: ['i'], newFile: ['n'], newFolder: ['N'],
  rename: ['R', 'f2'], delete: ['delete'], edit: ['E'], openInEditor: ['e'], openInApp: ['o'], preview: ['p'], copyPath: ['y'],
  copyRelativePath: ['Y'], parentRoot: ['backspace'], changedOnly: ['c'], viewChanges: ['d'], nextChange: [']'], previousChange: ['['], fileHistory: ['H'], narrower: ['>', INNER === 'right' ? 'shift+left' : 'shift+right'],
  wider: ['<', INNER === 'right' ? 'shift+right' : 'shift+left'], halfPageUp: ['ctrl+u'], halfPageDown: ['ctrl+d'], help: ['?', 'f1'],
};
// The key help (help action), by group, with the keys bound now: [group, [[action, description]]].
const HELP = [
  ['Move', [['up', 'up'], ['down', 'down'], ['pageUp', 'page up'], ['pageDown', 'page down'], ['halfPageUp', 'half page up'],
    ['halfPageDown', 'half page down'], ['top', 'first'], ['bottom', 'last'], ['expand', 'unfold'], ['collapse', 'fold, parent'],
    ['filter', 'filter']]],
  ['Review', [['changedOnly', 'changed files only'], ['viewChanges', 'changes in the preview'], ['nextChange', 'next changed file'], ['previousChange', 'previous changed file'],
    ['fileHistory', 'history of the file']]],
  ['Open', [['open', 'open, fold'], ['preview', 'preview'], ['openInEditor', 'open in editor'], ['edit', 'edit (terminal)'], ['openInApp', 'open in app'],
    ['copyPath', 'copy path'], ['copyRelativePath', 'copy relative path']]],
  ['Files', [['newFile', 'new file'], ['newFolder', 'new folder'], ['rename', 'rename'], ['delete', 'to the trash'],
    ['menu', 'actions menu']]],
  ['Panes', [['goToFile', 'go to file'], ['searchInFiles', 'search in files'], ['sourceControl', 'source control'],
    ['settings', 'settings'], ['narrower', 'narrower'], ['wider', 'wider'], ['toggleHidden', 'hidden files'], ['toggleIgnored', 'git-ignored files'], ['parentRoot', 'parent as root'],
    ['refresh', 'refresh'], ['help', 'keys'], ['close', 'close']]],
];
let keys = readKeys('explorer', DEFAULT_KEYS);
// The keys of an action, its default ones first in their order.
const keysOf = action => {
  const rank = k => { const i = DEFAULT_KEYS[action].indexOf(k); return i < 0 ? Infinity : i; };
  return [...keys].filter(([, a]) => a === action).map(([k]) => k).sort((a, b) => rank(a) - rank(b));
};
if (KEYS_FILE) fs.watchFile(KEYS_FILE, { interval: 1000 }, () => { keys = readKeys('explorer', DEFAULT_KEYS); });

let theme = icons.load(settings.icons); // VS Code icon theme; null: the default icons
// Moved by an update of its extension: its new place saved for the next starts. No longer installed: the default
// icons saved, rather than searching for it again at every start.
if (theme ? theme.entry.file !== settings.icons.file : settings.icons && settings.icons.id) {
  settings.icons = theme ? theme.entry : '';
  writeSettings({ ...readSettings(), icons: settings.icons });
}
// The icon theme or icon set chosen in another explorer, the update check, the opening in new tabs, what Enter opens,
// the editor and the preview following the selection changed there, and the preview and diff settings changed in
// their panes (shown by an open settings view). Hidden and ignored files and Follow terminal stay this explorer's.
if (SETTINGS_FILE) fs.watchFile(SETTINGS_FILE, { interval: 1000 }, () => {
  // A file read while being written is skipped, rather than taken for the default settings.
  let saved, chosen;
  try { saved = JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8')); chosen = saved.icons || ''; } catch { return; }
  for (const name of ['updateCheck', 'autoOpen', 'open', 'editor', 'singleClick', 'wrap', 'lineNumbers', 'images', 'diffLayout', 'glyphs']) {
    if (saved[name] === undefined || saved[name] === settings[name]) continue;
    settings[name] = saved[name];
    if (name === 'glyphs') ui.useGlyphs(saved.glyphs);
    render();
  }
  if (JSON.stringify(chosen) === JSON.stringify(settings.icons)) return;
  settings.icons = chosen;
  theme = icons.load(chosen);
  render();
});
// A newer release, found by the hook's daily check: ↑ and its version in the header; skipped: the one the user
// skipped, only in the settings.
let latest = update.available(), skipped = update.skipped();
// An update that changed the manifest or the installer, which the installer must finish: ! finish in the header.
let finish = update.finishing();
let updating = false; // a pull running: no second one, ↑ hidden
// Restarted on the code of an update done here: the version in a message, and the release's notes in the header's
// pill for half a minute.
let notice = update.justUpdated();
if (notice) setTimeout(() => { notice = null; render(); }, 30 * 1000);
// Restarted on new code: this process only waits for the new one, its async work still running stops there.
let stopped = false;
if (update.FILE) {
  fs.watchFile(update.FILE, { interval: 1000 }, () => {
    latest = update.available(); skipped = update.skipped(); finish = update.finishing(); render();
  });
}

const state = {
  expanded: new Set(['']), rows: [], cursor: 0, top: 0, selected: null, focused: true,
  menu: null,      // the entry's actions (ui.Menu) while open
  help: null,      // { top, find }: the key help over the pane (ui.help)
  settings: null,  // { sel, top, find, icons }: the settings view in place of the tree, its selected row and first row
                   // shown, and the page opened from it: icon themes ({ list, sel, top, loaded }) or key bindings
                   // ({ sel, top, find, capture }); find: the field of /, null when closed
  flash: null,     // short message shown in the header
  lastClick: null, // to detect double-clicks
  filter: null,    // { field, active }: rows limited to the names containing the text
  changed: false,  // rows limited to the changed files and their folders, all unfolded
  prompt: null,    // { label, field, done(value), inline }: a name typed in the tree (inline: { rel } or
                   // { dir, folder }) or at the bottom
  confirm: null,   // { title, name, where, note, action, tone, yes(), focus }: y, or Enter on the action's button
  choice: null,    // a question with several answers (ui.ask): { title, name, list, note, buttons, focus, on: { key: fn } }
                   // (focus 'yes', moved with ← →), to confirm; any other key to cancel
};
const scrollbar = new ui.Scrollbar(top => { state.top = top; render(); });
const tree = Tree.create(() => ROOT, settings);
const { git, cache, decoration, changedFiles } = tree;

// The repository config is read again before each call, as it may change while the explorer runs.
const gitRun = async args => run(GIT_BIN, args, { cwd: ROOT, env: await gitEnv(ROOT) });
const { parentOf, treeBefore, holds } = Tree;
const absOf = rel => path.join(ROOT, rel);

// --- tree

// The rows of the tree as the view shows them, the selection kept on its entry (else on the first match, or at the
// same place).
function buildRows() {
  const query = state.filter && state.filter.field.value.toLowerCase();
  const rows = state.rows = tree.rows({ query, changed: state.changed, expanded: state.expanded });
  // An entry within a compact folder row: that row.
  const i = state.selected === null ? -1 : rows.findIndex(r => holds(r, state.selected));
  select(i >= 0 ? i : query ? rows.findIndex(r => r.hit) : state.cursor);
}

function select(i) {
  state.cursor = Math.max(0, Math.min(i, state.rows.length - 1));
  state.selected = state.rows.length ? state.rows[state.cursor].rel : null;
}

function toggle(row, open) {
  if (!row || !row.dir || state.filter || state.changed) return;
  if (open === undefined) open = !state.expanded.has(row.rel);
  // Opened, a compact folder row unfolds each folder of its chain, as they show when the setting is off.
  if (open) for (let a = row.rel; a && a !== parentOf(row.from || row.rel); a = parentOf(a)) state.expanded.add(a);
  else state.expanded.delete(row.rel);
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
  writeAtomic(SAVE, JSON.stringify(all, null, 1));
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
  Object.assign(git, { top: null, prefix: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set(), gone: [] });
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

async function refreshGit() {
  if (!git.top) return;
  const status = await gitRun(['status', '--porcelain=v1', '-z', '--ignored=matching', '--ignore-submodules=all', '--', '.']);
  if (status === null) return;
  tree.applyStatus(status);
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
  for (const recursive of big(ROOT) ? [false] : [true, false]) {
    try { rootWatcher = watchTree(recursive); return; } catch { }
  }
}

// $HOME, one of its parents or the filesystem root.
function big(dir) {
  const real = p => { try { return fs.realpathSync(p); } catch { return p; } }; // /home → /var/home on some systems
  const up = path.relative(real(dir), real(require('os').homedir()));
  return dir === path.parse(dir).root || !up.startsWith('..') && !path.isAbsolute(up);
}

function watchTree(recursive) {
  const watcher = fs.watch(ROOT, { recursive }, (ev, f) => {
    const rel = f ? String(f).split(path.sep).join('/') : '';
    if (rel === '.git' || rel.startsWith('.git/')) {
      if (!git.top) setTimeout(() => initGit().then(() => git.top && schedule()), 500);
      return;
    }
    if (rel.includes('/.git/') || tree.hidden(rel, state.expanded)) return;
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
  writeAtomic(WIDTH_FILE, String(width));
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
  if (stopped || !cwd || cwd === lastCwd || state.filter || state.menu || state.settings || state.help || state.prompt || state.confirm || state.choice) return;
  lastCwd = cwd;
  const inside = (dir, root) => { const r = path.relative(root, dir); return !r.startsWith('..') && !path.isAbsolute(r); };
  // Outside the root: back to the space folder when it holds the terminal folder, else the terminal folder itself.
  // Never $HOME, one of its parents or the filesystem root, where the tree would be too big to follow.
  if (!inside(cwd, ROOT)) {
    if (inside(cwd, HOME)) setRoot(HOME);
    else if (big(cwd)) return;
    else setRoot(cwd);
  }
  const rel = path.relative(ROOT, cwd).split(path.sep).join('/');
  if (!rel) return;
  state.expanded.add(rel);
  revealRel(rel);
  render();
}

// --- rendering

// The tree, or a page in its place (settings, key help), and a question over it.
function render() {
  const W = out.columns || WIDTH, H = out.rows || 24;
  const lines = state.settings ? settingsPage().view(W, H) : treeView(W, H);
  if (state.help) ui.help(lines, W, 'Explorer', helpGroups(), state.help);
  if (state.confirm) ui.confirm(lines, W, H, state.confirm);
  if (state.choice) ui.ask(lines, W, H, state.choice);
  paint(lines);
}

// --- opening

// The changes of an entry: a file's in the preview's Diff view, a folder's in the Changes pane (the whole tab).
const viewChanges = r => (r.dir ? actions.diff(absOf(r.rel), ROOT, PANE)
  : actions.preview(absOf(r.rel), ROOT, PANE, 0, false, true, { view: 'diff' }));

// Enter and double-click: documents and media other than images with their application, the rest in the preview
// (or the editor, depending on the settings).
async function openFile(r) {
  if (!r || r.dir) return;
  const abs = absOf(r.rel);
  // A deleted file: what it held, in its diff.
  if (r.gone) return runAction(() => viewChanges(r));
  if (settings.open === 'preview' && actions.canPreview(abs)) return runAction(() => actions.preview(abs, ROOT, PANE, 0, false, true, { keep: true }));
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
  // Never to the trash, which would follow it (see actions.trash).
  if (remoteLink(abs, ROOT)) {
    return confirm({ title: 'Link to a network share: delete the link?', name, where,
      note: 'Only the link goes; the share is untouched.', action: 'Delete' },
    () => flash(actions.remove(abs) || `${r.name} deleted`));
  }
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
  if (r && r.gone) {
    // A deleted entry: its changes and history, or back with Discard.
    const abs = absOf(r.rel);
    items.push(['View changes', () => viewChanges(r), 'viewChanges']);
    if (!r.dir) items.push(['File history', () => actions.scm(ROOT, PANE, abs, true), 'fileHistory']);
    items.push(null, ['Stage', () => actions.stage(abs, ROOT)], ['Unstage', () => actions.unstage(abs, ROOT)],
      ['Discard changes…', () => confirm({ title: 'Restore it?', name: r.name + (r.dir ? '/' : ''), where: whereOf(r.rel),
        note: 'Its changes are discarded.', action: 'Restore' }, () => runAction(() => actions.discard(abs, ROOT))), null, true],
      null, ['Copy relative path', () => actions.copy(r.rel), 'copyRelativePath']);
  } else if (r) {
    const abs = absOf(r.rel);
    const deco = decoration(r);
    if (!r.dir) {
      if (actions.canPreview(abs)) items.push(['Preview', () => actions.preview(abs, ROOT, PANE, 0, false, true, { keep: true }), 'preview']);
      items.push(['Open in editor', () => actions.editor(abs, PANE), 'openInEditor']);
      if (actions.canOpen(abs)) items.push(['Open in app', () => actions.system(abs), 'openInApp']);
      items.push(['Edit in terminal', () => actions.edit(abs, PANE), 'edit']);
    } else items.push(['Open in editor', () => actions.editor(abs, PANE), 'openInEditor'], ['Open in file manager', () => actions.app(abs, PANE), 'openInApp']);
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
      if (changed) items.push(['View changes', () => viewChanges(r), 'viewChanges']);
      if (!r.dir && deco !== 'U') items.push(['File history', () => actions.scm(ROOT, PANE, abs, true), 'fileHistory']);
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
    if (git.top) {
      items.push(['Source control', () => actions.scm(ROOT, PANE), 'sourceControl'],
        [state.changed ? 'All files' : 'Changed files only', () => showChanged(!state.changed), 'changedOnly']);
    }
    items.push(null, [actions.REVEAL, () => actions.reveal(ROOT)], ['Copy path', () => actions.copy(ROOT)],
      ['Up one level', () => setRoot(path.dirname(ROOT)), 'parentRoot']);
    if (ROOT !== HOME) items.push(['Back to the space folder', () => setRoot(HOME)]);
    items.push(['Refresh', () => schedule(), 'refresh'], ['Settings…', () => openSettings(), 'settings']);
  }
  return items;
}

// The newer release: the versions, its first changes (the release's text, kept by the daily check), and the answers:
// update, its notes in the browser, skip it (not for one skipped already), later (focused, as escape).
// The update: a fast-forward pull of the plugin's clone; the panes then restart on the new code by themselves
// (update.watch). Refused: the reason only, since the command would bypass the checks. The pull failed: the command
// copied, to run in the terminal. "Updating…" stays until the pull ends, which may take up to a minute. Installed by
// herdr: herdr's update command copied instead, since only herdr may move its checkout (herdr not answering: the
// pull, whose checks refuse such a checkout).
async function askUpdate() {
  if (updating) return 'Update in progress';
  if (await update.managed()) return actions.copy(update.HERDR_UPDATE, 'Update command copied');
  const to = latest || skipped, { notes, url } = update.release();
  if (!to) return null;
  const install = async () => {
    updating = true;
    clearTimeout(flash.timer);
    state.flash = 'Updating…';
    render();
    const { ok, retry, message } = await update.install();
    updating = false;
    state.flash = null;
    if (ok || !retry) return message;
    return `${message}: ${await actions.copy(`git -C "${update.DIR}" pull --ff-only --no-rebase`, 'update command copied')}`;
  };
  const buttons = [{ key: 'y', label: 'Update', tone: 'go' }, { key: 'r', label: 'Notes' },
    ...(latest ? [{ key: 's', label: 'Skip version' }] : []), { key: 'n', label: 'Later' }];
  state.menu = null;
  state.choice = {
    title: 'Update herdr-explorer', name: `${update.CURRENT || '?'} → ${to}`, list: notes.slice(0, 3),
    note: notes.length > 3 ? `and ${notes.length - 3} more (r)` : 'The panes restart on the new code.',
    buttons, focus: buttons.length - 1,
    on: { y: install, r: () => actions.web(url, 'the release notes'), s: () => { update.skip(to); return `${to} skipped: Settings to update anyway`; } },
  };
  render();
  return null;
}

// The update the installer must finish: its command shown, then copied to run in a terminal; the ! finish then goes
// away in every explorer.
function finishUpdate() {
  state.menu = null;
  state.choice = {
    title: `Finish the ${finish} update`, name: update.INSTALLER, note: 'Run it once in a terminal.',
    buttons: [{ key: 'c', label: 'Copy command', tone: 'go' }, { key: 'n', label: 'Later' }], focus: 0,
    on: { c: async () => {
      const done = await actions.copy(update.INSTALLER, 'Installer command copied');
      if (done !== 'Clipboard unavailable') update.finished();
      return done;
    } },
  };
  render();
  return null;
}

// What the header's pill does (render.js pill): finish, update, or the notes of the update just done.
const pillAction = () => (finish ? finishUpdate : latest && settings.updateCheck && !updating ? askUpdate
  : () => actions.web(update.release().url, 'the release notes'));

function openMenu(i) {
  const r = i === null ? null : state.rows[i];
  if (r) { select(i); reveal(); }
  const items = menuItems(r);
  const H = out.rows || 24, y = r ? i - state.top + 1 : Math.min(H - 1, state.rows.length - state.top + 1);
  const h = Math.min(items.length, H - 2) + 2;
  const title = r ? r.name + (r.dir ? '/' : '') : `${path.basename(ROOT)}/`;
  const at = y + 1 + h <= H ? y + 1 : Math.max(0, Math.min(y - h, H - h));
  // Each item with the first key of its action on the right.
  const keyOf = action => { const k = action && keysOf(action)[0]; return k ? ui.keyLabel(k) : ''; };
  state.menu = new ui.Menu({ title, at, left: 2, items: items.map(it => it && { label: it[0], run: it[1], action: it[2], key: keyOf(it[2]), danger: it[3] }) });
  render();
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
  if (name === 'glyphs') ui.useGlyphs(value);
  if (name === 'theme') ui.useTheme(ui.settingTheme(value));
  cache.clear();
  buildRows();
  if (name === 'follow') { lastCwd = null; follow(); }
  return null;
}

// --- key help

// Every key of the explorer, as bound now, over the whole pane (ui.help, as in the other panes).
function openHelp() {
  state.help = { top: 0, find: null };
  render();
}

const helpGroups = () => HELP.map(([group, items]) => [group, items.map(([action, text]) => [keysOf(action).join(' ') || '—', text])]);

// The changed files only, or the whole tree again with the selected entry shown.
function showChanged(on) {
  if (on && !git.top) return flash('Not a git repository');
  const sel = state.selected;
  state.changed = on;
  state.top = 0;
  if (!on && sel) revealRel(sel); else { buildRows(); reveal(); }
  render();
}

// ] and [: the next or previous changed file after the selected entry, unfolded, selected and previewed; past the
// last one, back to the first.
function nextChange(dir) {
  if (!git.top) return flash('Not a git repository');
  const list = changedFiles();
  if (!list.length) return flash('No changes');
  const r = state.rows[state.cursor];
  let i;
  if (!r) i = dir > 0 ? 0 : list.length - 1;
  else if (dir > 0) { i = list.findIndex(f => treeBefore(r.rel, r.dir, f, false)); if (i < 0) i = 0; }
  else { i = list.findLastIndex(f => treeBefore(f, false, r.rel, r.dir)); if (i < 0) i = list.length - 1; }
  const rel = list[i];
  if (!git.gone.includes(rel)) revealRel(rel);
  else {
    // A deleted file shows only among the changed files, every folder unfolded there.
    state.changed = true; state.selected = rel;
    buildRows(); reveal();
  }
  render();
  const abs = absOf(rel);
  // Among the changed files, and for a deleted one, the preview shows the changes.
  if (state.changed) runAction(() => actions.preview(abs, ROOT, PANE, 0, false, true, { view: 'diff' }));
  else if (actions.canPreview(abs)) runAction(() => actions.preview(abs, ROOT, PANE));
}

// --- startup

// Whether the terminal has the Kitty graphics protocol, for the Images setting: null until its answer, at most 500 ms.
let graphics = null;

// The explorer's modules (src/explorer/), given what they share in ctx: the state and the objects that stay, the
// values that change (read through ctx, as ctx.ROOT), the functions of this file they call, and the functions of each
// module, added for the next ones.
const ctx = {
  state, settings, out, scrollbar, tree, git, HOME, PANE, WIDTH, DEFAULT_KEYS, HELP,
  get ROOT() { return ROOT; },
  get theme() { return theme; },
  set theme(t) { theme = t; },
  get keys() { return keys; },
  set keys(k) { keys = k; },
  get latest() { return latest; },
  get skipped() { return skipped; },
  get finish() { return finish; },
  get notice() { return notice; },
  get updating() { return updating; },
  get graphics() { return graphics; },
  keysOf, render, flash, runAction, ask, setSetting, askUpdate, finishUpdate, pillAction, select, toggle, reveal, revealRel, buildRows,
  close, schedule, openFile, openMenu, openHelp, newEntry, renameEntry, deleteEntry, setRoot, showChanged, viewChanges,
  nextChange, widen, absOf, parentOf,
};
const view = require('./explorer/render')(ctx);
Object.assign(ctx, view);
const pages = require('./explorer/settings')(ctx);
Object.assign(ctx, pages);
const { key, mouse } = require('./explorer/input')(ctx);
const { bodyHeight, treeView } = view;
const { settingsPage, openSettings } = pages;

const paint = ui.screen({
  key, mouse,
  reply: text => { if (ui.KITTY_OK.test(text)) { graphics = true; render(); } },
  focus: f => { state.focused = f; render(); },
  paste: text => {
    if (state.prompt) state.prompt.field.insert(text);
    else if (state.filter && state.filter.active) { state.filter.field.insert(text); buildRows(); }
    render();
  },
  resize: () => { resized(); reveal(); render(); },
  theme: () => { cache.clear(); buildRows(); render(); },
}, { motion: true });
// The Kitty graphics query, as the preview sends it.
process.stdout.write(ui.KITTY_QUERY);
if (notice) flash(`Updated to ${notice.to}`);
setTimeout(() => { if (graphics === null) { graphics = false; render(); } }, 500);

watchRoot();
const timers = [
  setInterval(schedule, 30000), // safety net in case a file event is lost
  setInterval(follow, 2000),
];

// Updated plugin: this explorer starts again on the new code, at the same root, once nothing is being typed or
// chosen. This process stops watching and acting, the new one taking over the pane.
update.watch(() => !updating && !state.prompt && !state.confirm && !state.choice && !state.menu && !state.settings && !state.help && !(state.filter && state.filter.active), () => {
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
