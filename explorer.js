// File explorer of the space, in a herdr pane: tree folded with the mouse and the keyboard, VS Code-style
// git status, automatic refresh. Double-click: open; right-click: action menu (actions.js).
// « at the top right closes it.
'use strict';
const fs = require('fs');
const path = require('path');
const actions = require('./actions');
const { run, result, HERDR, GIT: GIT_BIN, gitEnv, WIDTH_FILE, readWidth } = require('./exec');

const ROOT = path.resolve(process.env.EXPLORER_ROOT || process.argv[2] || process.cwd());
const PANE = process.env.HERDR_PANE_ID;
const WIDTH = 32; // columns when the terminal size is unknown
const SAVE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'expanded.json');
const out = process.stdout;

// Colours: git decorations from the VS Code Dark+ theme, icons from the Seti theme.
const C = {
  yellow: [203, 203, 65], blue: [81, 154, 186], orange: [227, 121, 51], green: [141, 193, 73],
  purple: [160, 116, 196], red: [204, 62, 68], grey: [109, 128, 134], folder: [220, 182, 122],
  dim: [140, 140, 140], select: [4, 57, 94], selectBlur: [55, 55, 61],
};
const GIT = {
  M: [226, 192, 141], A: [129, 184, 139], U: [115, 201, 145], R: [115, 201, 145],
  D: [199, 78, 57], C: [228, 103, 107], '!': [140, 140, 140],
};
const RANK = { U: 1, A: 2, R: 2, M: 3, D: 4, C: 5 };

const FOLDER = [0xf07b, 0xf07c];
const CHEVRON = [0xeab6, 0xeab4];
const BRANCH = 0xe0a0;
const FILE = [0xf15b, C.grey];
const ICONS = {};
for (const [exts, icon] of [
  ['js mjs cjs jsx', [0xe74e, C.yellow]], ['ts tsx mts cts', [0xe628, C.blue]], ['json jsonc', [0xe60b, C.yellow]],
  ['md mdx', [0xe609, C.blue]], ['py', [0xe606, C.blue]], ['html htm', [0xe736, C.orange]],
  ['css scss sass less', [0xe749, C.blue]], ['sh bash zsh fish', [0xe795, C.green]],
  ['ps1 psm1 psd1 bat cmd', [0xebc7, C.blue]], ['png jpg jpeg gif svg webp ico bmp', [0xf1c5, C.purple]],
  ['pdf', [0xf1c1, C.red]], ['zip gz tgz 7z tar rar xz', [0xf1c6, C.orange]], ['xlsx xls csv', [0xf1c3, C.green]],
  ['docx doc odt', [0xf1c2, C.blue]], ['lock', [0xf023, C.grey]],
  ['toml yaml yml ini cfg conf env xml rules service', [0xf013, C.grey]], ['sql db sqlite', [0xf1c0, C.orange]],
  ['rs', [0xe7a8, C.orange]], ['cs csproj sln', [0xf031b, C.purple]], ['java', [0xe738, C.red]],
  ['vue', [0xe6a0, C.green]], ['txt log', [0xf0f6, C.grey]],
  ['c h cpp hpp go rb php kt swift lua qml', [0xf121, C.blue]],
]) for (const ext of exts.split(' ')) ICONS[ext] = icon;
const NAMES = {
  '.gitignore': [0xe702, C.orange], '.gitattributes': [0xe702, C.orange], '.gitmodules': [0xe702, C.orange],
  'package.json': [0xe718, C.green], 'dockerfile': [0xe7b0, C.blue], 'readme.md': [0xf02d, C.blue],
};

const fg = rgb => `\x1b[38;2;${rgb.join(';')}m`;
const bg = rgb => `\x1b[48;2;${rgb.join(';')}m`;
const glyph = code => String.fromCodePoint(code);

const state = {
  expanded: new Set(['']), rows: [], cursor: 0, top: 0, selected: null, focused: true,
  menu: null,      // { items: [[label, action]], at: screen row of the menu top, sel }
  flash: null,     // short message shown in the header
  lastClick: null, // to detect double-clicks
};
const git = { top: null, prefix: '', branch: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set() };
const cache = new Map(); // relative folder → sorted entries

// The repository config is read again before each call, as it may change while the explorer runs.
const gitRun = async args => run(GIT_BIN, args, { cwd: ROOT, env: await gitEnv(ROOT) });
const parentOf = rel => (rel === '' ? null : rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '');

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

function buildRows() {
  const rows = [];
  const walk = (rel, depth) => {
    for (const e of readDir(rel)) {
      rows.push({ ...e, depth });
      if (e.dir && state.expanded.has(e.rel)) walk(e.rel, depth + 1);
    }
  };
  walk('', 0);
  state.rows = rows;
  const i = state.selected === null ? -1 : rows.findIndex(r => r.rel === state.selected);
  select(i >= 0 ? i : state.cursor);
}

function select(i) {
  state.cursor = Math.max(0, Math.min(i, state.rows.length - 1));
  state.selected = state.rows.length ? state.rows[state.cursor].rel : null;
}

function toggle(row, open = !state.expanded.has(row.rel)) {
  if (!row || !row.dir) return;
  if (open) state.expanded.add(row.rel); else state.expanded.delete(row.rel);
  buildRows();
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

// --- git

// Also called when a .git folder appears (git init, clone, archive extracted) in a folder that had none.
let probing = false;
async function initGit() {
  if (git.top || probing) return;
  probing = true;
  const res = await gitRun(['rev-parse', '--show-toplevel', '--absolute-git-dir', '--show-prefix']);
  probing = false;
  if (!res) return;
  const [top, gitdir, prefix = ''] = res.split(/\r?\n/);
  git.top = top;
  git.prefix = prefix.replace(/\/$/, '');
  // The repository changes without touching the working tree (commit, stage, branch switch).
  try { fs.watch(gitdir, (ev, f) => { if (f === 'index' || f === 'HEAD') schedule(); }).on('error', () => { }); } catch { }
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
    // The split whose whole left part is the explorer.
    const split = layout.splits.find(s => s.direction === 'right' && s.rect.x === me.rect.x
      && s.rect.y === me.rect.y && s.rect.height === me.rect.height
      && Math.abs(Math.round(s.rect.width * s.ratio) - me.rect.width) <= 1);
    if (!split) return;
    const want = width + me.rect.width - out.columns; // the pane width includes its border
    const delta = (want + 0.25) / split.rect.width - split.ratio;
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

// --- rendering

// Terminal columns of a character: none for combining marks (accents of decomposed names), two for wide
// CJK characters and emoji.
const WIDE = /[\u1100-\u115f\u2e80-\u303e\u3041-\u33ff\u3400-\u4dbf\u4e00-\u9fff\ua000-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe30-\ufe4f\uff00-\uff60\uffe0-\uffe6\u{1f300}-\u{1f64f}\u{1f900}-\u{1f9ff}\u{20000}-\u{3fffd}]/u;
const cols = ch => (/[\p{M}\u200b\u200d\ufe0f]/u.test(ch) ? 0 : WIDE.test(ch) ? 2 : 1);

// The longest start of a text that fits in max columns, and its width.
function fit(text, max) {
  let s = '', w = 0;
  for (const ch of text) {
    const c = cols(ch);
    if (w + c > max) break;
    s += ch; w += c;
  }
  return [s, w];
}
const textWidth = text => fit(text, Infinity)[1];

function line(segments, width, back) {
  // segments: [text, style sequence]; the text is truncated to the width, control characters replaced.
  let s = back ? bg(back) : '', used = 0;
  for (const [text, style] of segments) {
    const [part, w] = fit(text.replace(/[\x00-\x1f\x7f-\x9f]/g, '?'), width - used);
    if (!part) continue;
    s += (style || '') + part + '\x1b[39;22m';
    used += w;
  }
  return s + ' '.repeat(Math.max(0, width - used)) + '\x1b[0m';
}

function render() {
  const W = out.columns || WIDTH, H = out.rows || 24;
  const lines = [];

  // Header: space name, git branch (or an action message), and « on the right to close.
  if (state.flash) {
    const [text, w] = fit(` ${state.flash}`, W - 3);
    lines.push(line([[text, fg(C.yellow)], [' '.repeat(Math.max(0, W - 2 - w)), ''], ['«', fg(C.dim)]], W));
  } else {
    const [name, nw] = fit(` ${path.basename(ROOT).toUpperCase()}`, W - 3);
    const [branch, bw] = fit(git.branch ? `  ${glyph(BRANCH)} ${git.branch}` : '', W - 3 - nw);
    const pad = ' '.repeat(Math.max(0, W - 2 - nw - bw));
    lines.push(line([[name, '\x1b[1m'], [branch, fg(C.dim)], [pad, ''], ['«', fg(C.dim)]], W));
  }

  const listH = H - 1;
  if (state.top > state.rows.length - listH) state.top = Math.max(0, state.rows.length - listH);
  for (let i = state.top; i < Math.min(state.rows.length, state.top + listH); i++) lines.push(row(state.rows[i], W, i === state.cursor));
  if (!state.rows.length) lines.push(line([[' (empty)', fg(C.dim)]], W));
  while (lines.length < H) lines.push(line([], W));
  if (state.menu) drawMenu(lines, W);
  out.write('\x1b[?2026h\x1b[H' + lines.slice(0, H).join('\r\n') + '\x1b[?2026l');
}

function row(r, W, selected) {
  const deco = decoration(r);
  const color = deco ? fg(GIT[deco]) : '';
  let icon;
  if (r.dir) icon = [FOLDER[state.expanded.has(r.rel) ? 1 : 0], C.folder];
  else icon = NAMES[r.name.toLowerCase()] || ICONS[path.extname(r.name).slice(1).toLowerCase()] || FILE;
  const indent = ' '.repeat(1 + r.depth * 2);
  const chevron = r.dir ? glyph(CHEVRON[state.expanded.has(r.rel) ? 1 : 0]) : ' ';
  const badge = !deco || deco === '!' ? '' : r.dir ? '●' : deco === 'C' ? '!' : deco;
  const room = W - indent.length - 4 - (badge ? 2 : 0) - 1;
  let name = r.name;
  if (textWidth(name) > room) name = fit(name, Math.max(1, room - 1))[0] + '…';
  const pad = Math.max(0, W - indent.length - 4 - textWidth(name) - (badge ? 2 : 0) - 1);
  return line([
    [indent + chevron + ' ', fg(C.dim)], [glyph(icon[0]) + ' ', fg(deco === '!' ? GIT['!'] : icon[1])],
    [name, color], [' '.repeat(pad) + (badge ? ' ' + badge : ''), color],
  ], W, selected ? (state.focused ? C.select : C.selectBlur) : null);
}

// --- action menu

function menuItems(r) {
  const abs = path.join(ROOT, r.rel);
  const deco = decoration(r);
  const items = [['Insert into terminal', () => actions.insert(abs, PANE)], ['Open in VS Code', () => actions.vscode(abs)]];
  if (!r.dir && actions.canOpen(abs)) items.push(['Open with default app', () => actions.system(abs)]);
  items.push([actions.REVEAL, () => actions.reveal(abs)], ['Copy path', () => actions.copy(abs)],
    ['Copy relative path', () => actions.copy(r.rel)]);
  if (deco && deco !== '!') items.push(['View changes', () => actions.diff(abs, ROOT, PANE)]);
  return items;
}

function openMenu(i) {
  const r = state.rows[i];
  if (!r) return;
  select(i);
  reveal();
  const items = menuItems(r);
  const H = out.rows || 24, y = i - state.top + 1; // screen row of the entry (0 = header)
  const h = items.length + 2;
  state.menu = { items, sel: 0, at: y + 1 + h <= H ? y + 1 : Math.max(0, y - h) };
  render();
}

function drawMenu(lines, W) {
  const { items, sel, at } = state.menu;
  const inner = Math.min(W - 3, Math.max(...items.map(([label]) => Array.from(label).length)) + 2);
  const edge = fg(C.dim);
  lines[at] = line([[' ╭' + '─'.repeat(inner) + '╮', edge]], W);
  items.forEach(([label], k) => {
    const cell = line([[Array.from(` ${label}`).slice(0, inner).join(''), '']], inner, k === sel ? C.select : C.selectBlur);
    lines[at + 1 + k] = ` ${edge}│\x1b[39m${cell}${edge}│\x1b[0m` + ' '.repeat(Math.max(0, W - inner - 3));
  });
  lines[at + items.length + 1] = line([[' ╰' + '─'.repeat(inner) + '╯', edge]], W);
}

async function runAction(action) {
  state.menu = null;
  render();
  flash(await action().catch(() => 'Action failed'));
}

function flash(text) {
  if (!text) return;
  state.flash = text;
  render();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { state.flash = null; render(); }, 2000);
}

function menuKey(k) {
  const m = state.menu;
  if (k === 'up') m.sel = (m.sel + m.items.length - 1) % m.items.length;
  else if (k === 'down') m.sel = (m.sel + 1) % m.items.length;
  else if (k === 'enter' || k === 'space' || k === 'right') return runAction(m.items[m.sel][1]);
  else if (k === 'escape' || k === 'left' || k === 'menu') state.menu = null;
  render();
}

function openFile(r) {
  if (r && !r.dir) runAction(() => actions.open(path.join(ROOT, r.rel)));
}

function reveal() {
  const listH = (out.rows || 24) - 1;
  if (state.cursor < state.top) state.top = state.cursor;
  if (state.cursor >= state.top + listH) state.top = state.cursor - listH + 1;
}

// --- keyboard and mouse

function key(k) {
  if (state.menu) return menuKey(k);
  const r = state.rows[state.cursor];
  const page = (out.rows || 24) - 2;
  switch (k) {
    case 'up': select(state.cursor - 1); break;
    case 'down': select(state.cursor + 1); break;
    case 'pageup': select(state.cursor - page); break;
    case 'pagedown': select(state.cursor + page); break;
    case 'home': select(0); break;
    case 'end': select(state.rows.length - 1); break;
    case 'enter': case 'space':
      if (r && !r.dir) return openFile(r);
      toggle(r);
      break;
    case 'menu': return openMenu(state.cursor);
    case 'right':
      if (r && r.dir && !state.expanded.has(r.rel)) toggle(r, true);
      else if (r && r.dir) select(state.cursor + 1);
      break;
    case 'left':
      if (r && r.dir && state.expanded.has(r.rel)) toggle(r, false);
      else if (r) { const p = state.rows.findIndex(x => x.rel === parentOf(r.rel)); if (p >= 0) select(p); }
      break;
    case 'refresh': schedule(); return;
    default: return;
  }
  reveal();
  render();
}

function mouse(button, x, y) {
  if (button === 64 || button === 65) { // wheel
    const max = Math.max(0, state.rows.length - ((out.rows || 24) - 1));
    state.top = Math.max(0, Math.min(max, state.top + (button === 64 ? -3 : 3)));
    return render();
  }
  const btn = button & ~28; // modifiers ignored
  if (btn !== 0 && btn !== 2) return;
  if (state.menu) {
    // Click in the menu: action; elsewhere: close the menu (a right-click reopens it on the new row).
    const k = y - 2 - state.menu.at;
    if (btn === 0 && k >= 0 && k < state.menu.items.length) return runAction(state.menu.items[k][1]);
    state.menu = null;
    if (btn === 0) return render();
  }
  if (y === 1) { if (btn === 0 && x >= (out.columns || WIDTH) - 2) close(); return render(); }
  const i = state.top + y - 2;
  if (i >= state.rows.length) return render();
  if (btn === 2) return openMenu(i);
  const r = state.rows[i];
  const now = Date.now(), double = state.lastClick && state.lastClick.i === i && now - state.lastClick.t < 400;
  state.lastClick = double ? null : { i, t: now };
  select(i);
  if (r.dir) toggle(r);
  else if (double) return openFile(r);
  render();
}

const KEYS = {
  '\x1b[A': 'up', '\x1b[B': 'down', '\x1b[C': 'right', '\x1b[D': 'left', '\x1bOA': 'up', '\x1bOB': 'down',
  '\x1bOC': 'right', '\x1bOD': 'left', '\x1b[H': 'home', '\x1b[F': 'end', '\x1b[1~': 'home', '\x1b[4~': 'end',
  '\x1b[5~': 'pageup', '\x1b[6~': 'pagedown', '\r': 'enter', '\n': 'enter', ' ': 'space',
  k: 'up', j: 'down', l: 'right', h: 'left', g: 'home', G: 'end', r: 'refresh',
  m: 'menu', '\x1b[29~': 'menu', '\x1b[21;2~': 'menu', '\x1b': 'escape', q: 'escape',
};
const TOKEN = /\x1b\[<(\d+);(\d+);(\d+)([Mm])|\x1b\[([IO])|\x1b\[[\d;]*[A-Za-z~]|\x1bO[A-Za-z]|\x1b[^\x1b]|[\s\S]/g;

function input(data) {
  for (const m of data.matchAll(TOKEN)) {
    if (m[1] !== undefined) { if (m[4] === 'M') mouse(+m[1], +m[2], +m[3]); continue; }
    if (m[5]) { state.focused = m[5] === 'I'; render(); continue; }
    if (KEYS[m[0]]) key(KEYS[m[0]]);
  }
}

// --- startup

out.write('\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?1000h\x1b[?1006h\x1b[?1004h');
process.on('exit', () => out.write('\x1b[?1004l\x1b[?1006l\x1b[?1000l\x1b[?7h\x1b[?25h\x1b[?1049l'));
for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT']) process.on(sig, () => process.exit(0));

if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stdin.on('data', input);
out.on('resize', () => { resized(); reveal(); render(); });

try {
  fs.watch(ROOT, { recursive: true }, (ev, f) => {
    const rel = f ? String(f).split(path.sep).join('/') : '';
    if (rel === '.git' || rel.startsWith('.git/')) {
      if (!git.top) setTimeout(() => initGit().then(() => git.top && schedule()), 500);
      return;
    }
    if (rel.includes('/.git/') || hidden(rel)) return;
    schedule();
  }).on('error', () => { });
} catch { }
setInterval(schedule, 30000); // safety net in case a file event is lost

// Right-clicks in this pane go to the explorer (its menu) instead of herdr's menu; the frame keeps herdr's.
if (PANE) run(HERDR, ['pane', 'input', PANE, '--right-click', 'pane']);

load();
buildRows();
render();
initGit().then(refresh);
for (const ms of [300, 1000, 2500]) setTimeout(ensureWidth, ms);
