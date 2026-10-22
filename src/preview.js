// File preview of a tab, opened next to the terminal by the explorer and the search: syntax-highlighted text,
// rendered Markdown, images (in pixels with the Kitty graphics protocol, else drawn with half blocks), a hex dump for
// other binary files; or the file's git changes (Diff view, d). Reloads by itself when the file changes on disk, and
// shows the next file the explorer asks for (request file, see actions.preview), in tabs (core/tabs.js).
'use strict';
// The pane's first process only starts the real one (launch.js).
if (require('./launch').launch()) return;
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const ui = require('./core/ui');
const actions = require('./core/actions');
const notes = require('./core/notes');
const Tabs = require('./core/tabs');
const buffer = require('./core/buffer');
const update = require('./core/update');
const Scope = require('./core/scope');
const stage = require('./core/stage');
const { run, HERDR, readSettings, writeSettings, writePaneState, SETTINGS_FILE, remoteLink } = require('./core/exec');

const PANE = process.env.HERDR_PANE_ID;
const TAB = process.env.EXPLORER_TAB;
const REQUEST = TAB && actions.requestFile(TAB);
const MAX_BYTES = 8 << 20;     // larger files: the start only
const MAX_HIGHLIGHT = 1 << 20; // larger files: plain text
const HEX_BYTES = 64 << 10;
const { C, fg, bg } = ui;

// Optional renderers: the preview still shows plain text without them.
const optional = name => { try { return require(`./render/${name}`); } catch { return null; } };
const highlight = optional('highlight'), markdown = optional('markdown'), image = optional('image');
const diffs = (() => { try { return require('./diff'); } catch { return null; } })();

const settings = readSettings();
ui.useGlyphs(settings.glyphs);
const state = {
  file: process.env.EXPLORER_FILE, root: process.env.EXPLORER_ROOT || '', line: +process.env.EXPLORER_LINE || 0,
  kind: 'text',     // text, markdown, image, binary, error
  info: '',         // header details: size, language, dimensions
  text: '',         // file contents (text and markdown)
  source: [],       // highlighted source lines: segment lists
  plain: [],        // the same lines as plain text, for the search and the copy
  buffer: null,     // image bytes
  raw: false,       // markdown shown as source
  rows: null,       // display rows for the current width: { segs, src, first }
  top: 0, left: 0, focused: true,
  find: null,       // { field, active } while searching
  matches: [], match: -1,
  select: null,     // { from, to, moved } positions { row, at } (display row, character) selected with the mouse
  flash: null, chips: [],
  notes: [],        // the tab's review notes on this file (notes.js)
  noteCount: 0,     // the tab's notes on every file
  note: null,       // { field, kind, from, to } while a note is typed
  sharp: null,      // p: images in pixels (true) or half blocks (false) in this pane; null: as the Images setting
  marks: [],        // parts highlighted by the caller (peek.js): { from, to, a, b }, 1-based lines, columns a to b
                    // included (0: whole lines); n and N go through them, Escape clears them
  mark: 0,          // the mark n and N went to last
  view: process.env.EXPLORER_VIEW === 'diff' ? 'diff' : 'file', // the file, or its git changes (d)
  diff: null,       // the Diff view's changes: { parsed, top, baseName }, or { loading: true }
  scope: Scope.HEAD, // what the Diff view compares with: the tab's scope (core/scope.js, b)
  revPrompt: null,  // the field of a commit or range typed for the scope (b, Commit or range…)
  hunk: null,       // the hunk n or N went to in the Diff view, which s, u and D take without a selection
  menu: null,       // the view's options (ui.Menu) while open
  wantTop: null,    // the scroll of a tab shown again in the Diff view, for once its changes are loaded
  edit: null,       // the edit mode (i, see startEdit): { ed (buffer.Editor), base, disk, prompt, hl }
  ask: null,        // a question in a card (ui.ask): { …, on: { key: what to do } }
};

// Marks from a request or the environment, kept only when well formed: at most 200, in order.
function cleanMarks(list) {
  if (!Array.isArray(list)) return [];
  const int = v => (Number.isInteger(v) && v >= 0 ? v : 0);
  return list.slice(0, 200).map(m => m && { from: int(m.from), to: int(m.to), a: int(m.a), b: int(m.b) })
    .filter(m => m && m.from >= 1 && m.to >= m.from && (!m.a || m.b >= m.a));
}
try { state.marks = cleanMarks(JSON.parse(process.env.EXPLORER_MARKS || '[]')); } catch { }

// --- tabs
//
// The files shown, as VS Code's editors (core/tabs.js): tabs.list holds each tab's file and kept, and, for the tabs not
// shown, what VIEW names of its state (stash); the tab shown lives in state. A tab from a click is temporary (in
// italics) until kept: t, a double-click on it, e, o, a note, or an Enter or double-click in the explorer.
// EXPLORER_TABS: the tabs saved before a restart, { list, current }.
// edit: a tab being edited keeps its text, cursor and history while another tab shows (never saved for a restart).
const VIEW = ['root', 'line', 'top', 'left', 'view', 'marks', 'mark', 'raw', 'edit'];
const DEFAULTS = { line: 0, top: 0, left: 0, view: 'file', marks: [], mark: 0, raw: false, edit: null };
let tabs = { list: [], current: 0 };
try {
  const t = JSON.parse(process.env.EXPLORER_TABS || 'null');
  const list = t && Array.isArray(t.list) ? t.list.filter(x => x && typeof x.file === 'string' && x.file).slice(0, Tabs.MAX) : [];
  tabs = { current: Math.max(0, Math.min((t && +t.current) || 0, list.length - 1)), list: list.map(x => ({
    file: x.file, kept: !!x.kept, root: typeof x.root === 'string' ? x.root : '', line: +x.line || 0, top: +x.top || 0, left: +x.left || 0,
    view: x.view === 'diff' ? 'diff' : 'file', marks: cleanMarks(x.marks), mark: 0, raw: !!x.raw,
  })) };
} catch { }
// The file of the environment in its tab; restored: that tab was saved, with its scroll (see startup).
if (state.file) tabs = Tabs.open(tabs, state.file, process.env.EXPLORER_KEEP === '1');
const restored = state.file && !tabs.fresh && tabs.list[tabs.current];
// The tab shown's state put away in its tab, and taken back.
function stash() {
  const t = tabs.list[tabs.current];
  if (!t || t.file !== state.file) return;
  tabs.list[tabs.current] = { ...t, ...Object.fromEntries(VIEW.map(k => [k, state[k]])) };
}
function unstash() {
  const t = tabs.list[tabs.current];
  Object.assign(state, DEFAULTS, { root: state.root }, Object.fromEntries(VIEW.filter(k => k in t).map(k => [k, t[k]])), { file: t.file });
}
const scrollbar = new ui.Scrollbar(top => { state.top = top; render(); });

// --- loading

const ext = file => path.extname(file).slice(1).toLowerCase();
const isMarkdown = file => /^(md|markdown|mdx|mkd)$/.test(ext(file));
// file inside dir (or dir itself), by their paths.
const within = (file, dir) => { const r = path.relative(dir, file); return r !== '..' && !r.startsWith(`..${path.sep}`) && !path.isAbsolute(r); };

function load(keepScroll) {
  const file = state.file;
  let buf;
  // Through a link to a network share, the read would send the user's credentials (see remoteLink).
  if (remoteLink(file, state.root && within(file, state.root) ? state.root : '')) return setError('Link to a network share: not opened');
  try {
    const stat = fs.statSync(file);
    if (stat.isDirectory()) return setError('Folder');
    // A named pipe or a device would block the read, and the whole pane with it.
    if (!stat.isFile()) return setError('Not a regular file');
    // An image is read whole, up to the decoder's limit and one byte more, so that a larger one is reported as too
    // large rather than decoded from its start.
    const limit = image && image.FORMATS.has(ext(file)) ? image.MAX_BYTES + 1 : MAX_BYTES;
    const fd = fs.openSync(file, 'r');
    try {
      buf = Buffer.alloc(Math.min(stat.size, limit));
      fs.readSync(fd, buf, 0, buf.length, 0);
    } finally { fs.closeSync(fd); }
    state.info = size(stat.size) + (stat.size > limit ? ' (start only)' : '');
  } catch (e) { return setError(e.code === 'ENOENT' ? 'File deleted' : `Cannot read: ${e.code || e.message}`); }

  const top = state.top, left = state.left;
  state.rows = null; state.buffer = null; state.matches = []; state.match = -1; state.converting = false; state.missing = null; forgetPictures(file);
  if (image && image.FORMATS.has(ext(file))) {
    state.kind = 'image'; state.buffer = buf; state.plain = []; state.source = [];
    const meta = image.info(buf);
    if (meta) state.info = `${meta.format} ${meta.width}×${meta.height} · ${state.info}`;
    // A WebP: shown once dwebp has made a PNG of it.
    if (meta && meta.format === 'WebP') {
      state.converting = true;
      image.convert(path.resolve(file), buf).then(png => {
        if (state.file !== file || state.buffer !== buf) return;
        state.converting = false;
        if (png) state.buffer = png; else state.missing = 'install dwebp (libwebp) to see WebP images';
        state.rows = null;
        render();
      });
    }
  } else if (buf.subarray(0, 8000).includes(0)) {
    state.kind = 'binary';
    state.plain = hexdump(buf.subarray(0, HEX_BYTES));
    state.source = state.plain.map(l => [[l.slice(0, 10), fg(C.dim)], [l.slice(10), '']]);
    if (buf.length > HEX_BYTES) state.info += ` · first ${size(HEX_BYTES)}`;
  } else {
    state.text = buf.toString('utf8').replace(/^﻿/, '');
    state.kind = isMarkdown(file) ? 'markdown' : 'text';
    const lines = state.text.split(/\r?\n/);
    if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
    state.plain = lines.map(l => ui.cleanLine(l));
    const lang = highlight && buf.length <= MAX_HIGHLIGHT ? highlight.language(path.basename(file)) : null;
    try { state.source = lang ? highlight.highlight(state.text, lang) : null; } catch { state.source = null; }
    if (!state.source || state.source.length < state.plain.length) state.source = state.plain.map(l => [[l, '']]);
    state.info = `${lines.length} line${lines.length === 1 ? '' : 's'} · ${state.info}`;
  }
  if (keepScroll) { state.top = top; state.left = left; } else { state.top = 0; state.left = 0; }
  if (state.find && state.find.field.value) search(false);
  if (state.view === 'diff') loadDiff(keepScroll);
  if (state.edit) diskChanged();
  render();
}

// --- Diff view

// The file's changes (diff.collect), from the nearest folder still on disk (that of a deleted file may be gone).
// keepScroll: a reload, where the view stays; otherwise it goes to the line asked for, or the first change.
let diffRun = 0;
async function loadDiff(keepScroll) {
  if (!diffs) return;
  const run = ++diffRun, file = state.file;
  if (!keepScroll || !state.diff) state.diff = { loading: true };
  let cwd = path.dirname(file);
  while (!fs.existsSync(cwd) && path.dirname(cwd) !== cwd) cwd = path.dirname(cwd);
  let res;
  const scope = state.scope, turn = scope.kind === 'turn' && TAB ? Scope.readTurn(TAB) : null;
  try { res = await diffs.collect({ cwd, file, scope, turn }); } catch { res = { text: '', top: cwd, baseName: '' }; }
  if (run !== diffRun || state.file !== file) return;
  if (res.error) flash(res.error);
  state.diff = { parsed: diffs.changesOf(res), top: res.top, baseName: res.baseName };
  state.rows = null;
  if (!keepScroll) goToLine();
  else if (state.wantTop !== null) state.top = state.wantTop;
  state.wantTop = null;
  render();
}
// The row of a line of the new version in the Diff view, or of the first change after it: -1 for none.
const diffRow = line => rows().findIndex(r => r.ref && !r.hunk && r.ref.from >= line);
// The Diff view at the line asked for (or the first mark), else at the first change, a third of the way down.
function goToLine() {
  const want = state.marks.length ? state.marks[0].from : state.line;
  const i = want ? diffRow(want) : rows().findIndex(r => r.hunk);
  state.top = Math.max(0, i - (want ? Math.floor((ui.size().H - 2) / 3) : 0));
}
function toggleView() {
  if (state.edit) return flash('Not while editing: esc first');
  state.view = state.view === 'diff' ? 'file' : 'diff';
  state.rows = null; state.top = 0; state.left = 0; state.select = null;
  state.find = null; state.matches = [];
  if (state.view === 'diff') loadDiff(false);
  else if (state.line) state.top = Math.max(0, rowOf(state.line - 1) - Math.floor((ui.size().H - 2) / 3));
  render();
}
// The diff's rows: one column for the marks' bar on the left. Long lines wrap with the file's setting (w), else the
// text scrolls from state.left (← →), rendered again at each step.
function diffRows(W) {
  if (!diffs) return [{ empty: 'No diff renderer', pairs: [['d', 'file'], ['q', 'close']], src: -1 }];
  if (!state.diff || state.diff.loading) return [{ empty: 'Loading…', pairs: [['d', 'file']], src: -1 }];
  const width = Math.max(10, W - 2);
  const mode = diffs.layoutOf(width);
  const list = diffs.render(state.diff.parsed, width, mode, true, { wrap: settings.wrap, left: state.left })
    .map(r => ({ segs: [[diffMarked(r) ? '▌' : ' ', fg(C.blue)], ...r.segs], back: r.back, ref: r.ref, hunk: r.hunk, more: r.more, src: -1,
      f: r.f, h: r.h, lines: r.lines }));
  return list.length ? list : [{ empty: 'No changes', pairs: [['d', 'file'], ['b', 'compare with'], ['q', 'close']], src: -1 }];
}
// A row of the Diff view on a marked line (whole lines: the diff has no columns of the file), or the @@ row of the
// hunk n went to.
const diffMarked = r => (r.h && r.h === state.hunk)
  || (r.ref && !r.hunk && r.ref.from && r.ref.from === r.ref.to && state.marks.some(m => r.ref.from >= m.from && r.ref.from <= m.to));
// n and N in the Diff view without marks: the next or previous hunk, its header at the top, marked (state.hunk) for
// s, u and D, even when the diff is too short to scroll.
function nextHunk(dir) {
  const list = rows(), was = state.hunk ? list.findIndex(r => r.h === state.hunk) : -1, at = was >= 0 ? was : state.top;
  const i = dir > 0 ? list.findIndex((r, k) => k > at && r.hunk) : list.map((r, k) => (k < at && r.hunk ? k : -1)).filter(k => k >= 0).pop();
  if (i !== undefined && i >= 0) { state.top = i; state.hunk = list[i].h || null; state.rows = null; render(); }
}
// b: the menu of the scopes (Scope.items); a scope chosen holds for every pane of the tab (scope-<tab>.json).
function pickScope(s) {
  state.menu = null; state.revPrompt = null;
  if (!Scope.same(s, state.scope)) {
    state.scope = s;
    if (TAB) Scope.write(TAB, s);
    return reloadScope();
  }
  render();
}
function reloadScope() {
  if (state.view !== 'diff') return render();
  loadDiff(false).then(() => { if (state.scope.kind === 'branch' && state.diff && !state.diff.baseName) flash('No default branch: compared with HEAD'); });
}
function openScopes() {
  state.menu = new ui.Menu({ title: 'Compare with', items: Scope.items(state.scope, branchName(), pickScope, askScope), at: 1, right: ui.size().W });
  render();
}
async function askScope(kind) {
  state.menu = null;
  if (kind === 'rev') { state.revPrompt = new ui.Field(state.scope.kind === 'rev' ? state.scope.rev : ''); return render(); }
  const list = await Scope.branches(path.dirname(state.file));
  if (!list.length) return flash('No branch');
  state.menu = new ui.Menu({ title: 'Branch', at: 1, right: ui.size().W,
    items: list.map(b => ({ label: b, value: state.scope.kind === 'ref' && state.scope.rev === b ? ['●', ''] : null, run: () => pickScope({ kind: 'ref', rev: b }) })) });
  render();
}
// The tab's scope chosen in another pane (the Changes pane, source control's compare).
if (TAB) {
  const followScope = () => { const s = Scope.read(TAB); if (!Scope.same(s, state.scope)) { state.scope = s; reloadScope(); } };
  state.scope = Scope.read(TAB);
  Scope.watch(TAB, followScope);
}
// s, u, D in the Diff view: the rows selected with the mouse, else the hunk n went to, else the hunk at the top or the
// file whose name is there, staged, unstaged or discarded (core/stage.js), compared with HEAD only; D asks first.
function editDiff(mode) {
  const d = state.diff;
  if (!d || !d.parsed) return;
  if (state.scope.kind !== 'head') return flash('Not with this scope: stage compared with HEAD (b)');
  const list = rows(), sel = state.select && state.select.moved ? state.select : null;
  const chosen = state.hunk ? list.findIndex(r => r.h === state.hunk) : -1;
  let a = chosen >= 0 ? chosen : state.top, b = a;
  if (sel) [a, b] = before(sel.from, sel.to) ? [sel.from.row, sel.to.row] : [sel.to.row, sel.from.row];
  else if (chosen < 0) { while (a > 0 && list[a] && !list[a].h && !list[a].f) a--; b = a; }
  const picked = stage.picked(list.slice(a, b + 1));
  const go = async () => {
    state.select = null;
    flash(await stage.apply(d.top, d.parsed, picked, mode).catch(() => `Cannot ${mode}`));
    loadDiff(true);
  };
  if (mode !== 'discard') return go();
  const n = stage.count(d.parsed, picked, mode);
  if (!n) return flash('Nothing unstaged selected');
  ask({ title: 'Discard from the working tree?', name: n, where: path.basename(state.file),
    buttons: [{ key: 'd', label: 'Discard', tone: 'danger' }, { key: 'escape', label: 'Cancel' }], on: { d: go } });
}
function switchLayout() {
  if (!diffs) return;
  const width = Math.max(10, ui.size().W - 2);
  diffs.saveMode(width, diffs.layoutOf(width) === 'split' ? 'inline' : 'split');
  state.rows = null;
  render();
}

function setError(text) {
  state.kind = 'error'; state.info = text; state.rows = null;
  state.plain = []; state.source = []; state.buffer = null;
  if (state.edit) diskChanged();
  // A deleted file still has its changes.
  if (state.view === 'diff' && state.file) loadDiff(false);
  render();
}

const size = n => (n < 1024 ? `${n} B` : n < 1 << 20 ? `${(n / 1024).toFixed(1)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`);

function hexdump(buf) {
  const lines = [];
  for (let i = 0; i < buf.length; i += 16) {
    const row = buf.subarray(i, i + 16);
    const hex = Array.from(row, b => b.toString(16).padStart(2, '0')).join(' ').padEnd(47);
    const ascii = Array.from(row, b => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')).join('');
    lines.push(`${i.toString(16).padStart(8, '0')}  ${hex}  ${ascii}`);
  }
  return lines;
}

// --- display rows

// While editing, a Markdown file shows its source.
const markdownView = () => state.view === 'file' && state.kind === 'markdown' && !state.raw && !state.edit && markdown;
const gutter = () => (state.view === 'file' && settings.lineNumbers && !markdownView() && state.kind !== 'binary' && state.kind !== 'image'
  ? String(state.source.length).length + 2 : 0);

// Display rows for the current width: wrapped or rendered text, built again when the width changes.
function rows() {
  const { W, H } = ui.size();
  const width = Math.max(1, W - gutter() - 1);
  // The Diff view cuts its rows at state.left itself (diffRows), the file view when drawing them.
  const left = state.view === 'diff' && !settings.wrap ? state.left : 0;
  if (state.rows && state.rows.width === width && state.rows.height === H && state.rows.left === left) return state.rows;
  // A selection holds positions in the old rows.
  state.select = null;
  let list = [];
  if (state.view === 'diff') list = diffRows(W);
  else if (state.kind === 'image' && (state.converting || waiting())) list = [];
  else if (state.kind === 'image' && sharp()) {
    // In pixels: blank rows that the image covers, the first holding what to place (see placeImage).
    const [cw, ch] = kitty.cell;
    let px;
    try { px = image.pixels(state.buffer, W * cw, (H - 2) * ch); } catch { px = { info: 'No preview' }; }
    if (px.data) {
      const cols = Math.min(W, Math.ceil(px.width / cw)), count = Math.min(H - 2, Math.ceil(px.height / ch));
      const picture = { px, cols, rows: count, pad: Math.max(0, Math.floor((W - cols) / 2)) };
      list = Array.from({ length: count }, (_, k) => ({ segs: [], src: -1, picture: k ? null : picture }));
    } else list = [{ empty: noPreview(px.info), pairs: [['o', 'open'], ['q', 'close']], src: -1 }];
  } else if (state.kind === 'image') {
    // With half blocks, and a line under the image on how to see it sharp.
    const hint = imageHint(W);
    let res;
    try { res = image.render(state.buffer, W, H - 2 - (hint ? 2 : 0)); } catch { res = null; }
    if (res && res.lines) {
      const pad = Math.max(0, Math.floor((W - res.width) / 2));
      list = res.lines.map(segs => ({ segs: [[' '.repeat(pad), ''], ...segs], src: -1 }));
      if (hint) list.push({ segs: [], src: -1 }, hint);
    } else list = [{ empty: noPreview(res && res.info), pairs: [['o', 'open'], ['q', 'close']], src: -1 }];
  } else if (markdownView()) {
    let res;
    try { res = markdown.render(state.text, Math.max(1, Math.min(width - 1, 120)), { highlight, image: picture }); } catch { res = null; }
    // The images taken back are in pictures now; the others are dropped.
    if (!waiting()) stalePictures = new Map();
    if (res) list = res.lines.map(segs => ({ segs: [[' ', ''], ...segs], src: -1, pad: 1, figure: segs.figure }));
    else list = state.source.map((segs, i) => ({ segs, src: i, first: true }));
  } else if (state.kind === 'error') {
    list = [{ empty: state.info, pairs: [['r', 'reload'], ['q', 'close']], src: -1 }];
  } else if (settings.wrap) {
    // off: where the part starts in its line, for the find's matches.
    state.source.forEach((segs, i) => {
      let off = 0;
      ui.split(segs, width).forEach((part, k) => { list.push({ segs: part, src: i, first: !k, off }); off += ui.segText(part).length; });
    });
  } else list = state.source.map((segs, i) => ({ segs, src: i, first: true }));
  list = withNotes(list, width);
  list.width = width; list.height = H; list.left = left;
  // The ids of the images of rendered Markdown in pixels that the rows hold, for placeFigures.
  list.figures = new Set(list.filter(r => r.figure).map(r => r.figure.pic.id));
  return (state.rows = list);
}

// The review notes shown in the text: each one under the last line it covers, those on the whole file, the stale ones
// (their lines gone) and every one where the rows are not the file's lines (rendered Markdown) at the top.
const NOTE = fg(C.yellow);
function withNotes(list, width) {
  if (!state.notes.length || state.view === 'diff' || state.kind === 'image' || state.kind === 'error' || (list.length === 1 && list[0].empty)) return list;
  const lines = !markdownView() && state.kind !== 'binary';
  const rowsOf = n => ui.split([[`✎ ${(lines && !n.stale) || !n.from ? '' : `${notes.where(n, path.dirname(n.file)).replace(/^.*:/, 'L')} `}`
    + `${n.stale ? 'stale, ' : ''}${n.kind ? `${n.kind}: ` : ''}${ui.cleanLine(n.text)}`, n.stale ? fg(C.dim) : NOTE]], Math.max(1, width - 2))
    .map(segs => ({ segs: [['  ', ''], ...segs], src: -1, note: true }));
  const top = state.notes.filter(n => !lines || !n.from || n.stale || n.to > state.plain.length).flatMap(rowsOf);
  if (!lines) return [...top, ...list];
  const out = [...top];
  list.forEach((r, i) => {
    out.push(r);
    const next = list[i + 1];
    if (r.src >= 0 && !(next && next.src === r.src)) out.push(...state.notes.filter(n => n.from && !n.stale && n.to === r.src + 1).flatMap(rowsOf));
  });
  return out;
}
const noted = src => state.notes.some(n => n.from && !n.stale && src + 1 >= n.from && src + 1 <= n.to);

const rowOf = src => Math.max(0, rows().findIndex(r => r.src >= src));

// Why an image is not shown, with how to see it when a tool is missing.
const noPreview = why => (state.missing ? `${why || 'No preview'}: ${state.missing}` : why || 'No preview');

// --- images in rendered Markdown

// The images a Markdown file shows, under their paragraph: local files only (never a URL), relative to the Markdown
// file, at most 60 columns and 20 rows. As the Images setting and p say, in pixels (blank lines that the image is
// placed over, see placeFigures; nothing until the terminal's answer in 'auto') or with half blocks. Decoded once
// per file, width and way of drawing (pictures).
const pictures = new Map();
// The pictures of the Markdown file shown before a reload: those whose file kept its size and date are taken back
// rather than decoded again (an agent rewriting the file reloads it often), the rest dropped once the rows are
// rebuilt; another file's are dropped at once.
let stalePictures = new Map(), picturesOf = null;
function forgetPictures(file) {
  // Two reloads in a row, with no drawing between them, keep the older ones.
  if (file !== picturesOf) stalePictures = new Map(); else if (pictures.size) stalePictures = new Map(pictures);
  picturesOf = file;
  pictures.clear();
}
function picture(url, width) {
  if (!image || /^[a-z][\w+.-]*:/i.test(url) || url.startsWith('//')) return null;
  let rel = url.replace(/[?#].*$/, '');
  try { rel = decodeURIComponent(rel); } catch { }
  if (!rel || !image.FORMATS.has(ext(rel)) || ext(rel) === 'svg') return null;
  // Checked again once decoded, and kept in the project (or the Markdown file's folder): %5C%5Chost%5Cshare would
  // otherwise reach a network share, which on Windows sends the user's credentials.
  const abs = path.resolve(path.dirname(state.file), rel);
  const base = state.root && within(state.file, state.root) ? state.root : path.dirname(state.file);
  if (!within(abs, base)) return null;
  if (waiting()) return null;
  const pixels = sharp(), [cw, ch] = kitty.cell;
  const key = `${abs}\0${width}${pixels ? `\0${cw}×${ch}` : ''}`;
  if (pictures.has(key)) return pictures.get(key).lines;
  let lines = null, stat = null;
  try { stat = remoteLink(abs, base) ? null : fs.statSync(abs); } catch { }
  const sig = stat ? `${stat.mtimeMs}:${stat.size}` : '';
  const old = stalePictures.get(key);
  if (old && old.sig === sig) { pictures.set(key, old); return old.lines; }
  try {
    if (stat && stat.isFile() && stat.size <= image.MAX_BYTES) {
      if (pixels) {
        const room = Math.max(1, Math.min(60, width - 2));
        const px = image.pixels(fs.readFileSync(abs), room * cw, 20 * ch);
        if (px.data) {
          const pic = { px, id: kitty.next++, cols: Math.min(room, Math.ceil(px.width / cw)), rows: Math.ceil(px.height / ch), pixels: px.height };
          lines = Array.from({ length: pic.rows }, (_, row) => Object.assign([[' ', ''], [' '.repeat(pic.cols), '']], { figure: { pic, row } }));
        }
      } else {
        const res = image.render(fs.readFileSync(abs), Math.min(60, width - 2), 20);
        if (res.lines) lines = res.lines.map(segs => [[' ', ''], ...segs]);
      }
    }
  } catch { }
  pictures.set(key, { lines, sig });
  return lines;
}
const hasPictures = () => [...pictures.values()].some(p => p.lines);

// --- Kitty graphics
//
// The Images setting chooses how an image shows: 'blocks' with half blocks, 'auto' in pixels when the terminal
// answers the protocol's query (herdr does, in a terminal that supports it), 'pixels' in pixels without waiting for
// the answer. In pixels, an image is sent once and placed over blank rows. supported: the terminal answered; probing:
// the answer is awaited, at most 500 ms, before an image is drawn in 'auto'. cell: the size of a cell in pixels,
// [width, height].
// The images of rendered Markdown each have an id of their own from next on (picture), kept in figures once sent;
// placed: the placements shown, by 'image id,placement id', with where and what part.
const kitty = { supported: false, probing: true, cell: [8, 16], id: 0x4846, sent: null, shown: null, next: 0x4847, figures: new Set(), placed: new Map() };
const sharp = () => (state.sharp ?? (settings.images === 'pixels' || (settings.images === 'auto' && kitty.supported)));
// 'auto' waiting for the terminal's answer: images are not drawn yet.
const waiting = () => kitty.probing && state.sharp === null && settings.images === 'auto';

// The line under an image drawn with half blocks: p and the setting when the terminal has the protocol, else where
// the documentation tells which terminals have it. Nothing until the answer, nor when p chose half blocks. Its parts
// with an action are clickable (imageClick).
const DOCS = 'https://github.com/gcaudan/herdr-explorer/blob/main/docs/usage.md#sharp-images';
function imageHint(W) {
  if (kitty.probing || state.sharp === false) return null;
  const dim = fg(C.dim), link = fg(C.blue) + '\x1b[4m';
  const choices = kitty.supported
    ? [[['▦ Pixelated  ', dim], ['p', fg(C.text) + '\x1b[1m', 'p'], [' sharp', dim, 'p'], ['  ·  ', dim], ['sharp by default', link, 'default']],
      [['p', fg(C.text) + '\x1b[1m', 'p'], [' sharp', dim, 'p']]]
    : [[['▦ Pixelated: this terminal shows no image in pixels  ', dim], ['Sharp images ↗', link, 'docs']],
      [['▦ Pixelated  ', dim], ['Sharp images ↗', link, 'docs']], [['Sharp images ↗', link, 'docs']]];
  const parts = choices.find(c => ui.segWidth(c) <= W - 2) || choices[choices.length - 1];
  const pad = Math.max(0, Math.floor((W - ui.segWidth(parts)) / 2));
  // The screen columns of each part, from 1.
  let x = pad + 1;
  const chips = parts.map(([text, , id]) => { const from = x; x += ui.textWidth(text); return { id, from, to: x - 1 }; }).filter(c => c.id);
  return { segs: [[' '.repeat(pad), ''], ...parts.map(([text, style]) => [text, style])], src: -1, hint: chips };
}
function imageClick(id) {
  if (id === 'p') return key('p');
  if (id === 'docs') return act(actions.web(DOCS));
  if (id === 'default') {
    settings.images = 'auto';
    writeSettings({ ...readSettings(), images: 'auto' });
    state.rows = null;
    return flash('Images: sharp when the terminal can');
  }
}
// p: this pane's images in pixels or half blocks, whatever the setting: an image file, or those of rendered Markdown.
function toggleSharp() {
  if (state.kind !== 'image' && !(markdownView() && hasPictures())) return;
  state.sharp = !sharp();
  state.rows = null;
  if (state.sharp && !kitty.supported && !kitty.probing) flash('The terminal did not answer the Kitty graphics query: the image may not show');
  else render();
}
// restarting: the terminal is being handed to the next process (update), which sends its own image.
let restarting = false;
const graphics = text => { if (!restarting) process.stdout.write(text); };

function reply(text) {
  if (ui.KITTY_OK.test(text)) { kitty.supported = true; kitty.probing = false; state.rows = null; render(); }
  const m = /^\x1b\[6;(\d+);(\d+)t$/.exec(text);
  if (m && +m[1] && +m[2]) { kitty.cell = [+m[2], +m[1]]; state.rows = null; render(); }
}
function settle() {
  if (!kitty.probing) return;
  kitty.probing = false;
  state.rows = null;
  render();
}

// The pixels of an image sent under an id, as zlib-compressed RGBA in 4096-byte chunks.
function transmit(px, id) {
  const data = zlib.deflateSync(Buffer.from(px.data.buffer, px.data.byteOffset, px.data.length)).toString('base64');
  let out = '';
  for (let i = 0; i < data.length; i += 4096) {
    const more = i + 4096 < data.length ? 1 : 0, chunk = data.slice(i, i + 4096);
    out += i ? `\x1b_Gm=${more};${chunk}\x1b\\` : `\x1b_Ga=t,f=32,o=z,s=${px.width},v=${px.height},i=${id},q=2,m=${more};${chunk}\x1b\\`;
  }
  return out;
}

// After each frame: the image of the rows placed, sent first when new; removed when none shows (another file, the
// key help over the pane).
function placeImage() {
  const first = state.view === 'file' && state.kind === 'image' && !state.edit && sharp() && !state.help && !state.menu && state.rows && state.rows[0];
  const pic = first && first.picture;
  if (!pic) {
    if (kitty.shown) graphics(`\x1b_Ga=d,d=${state.kind === 'image' ? 'i' : 'I'},i=${kitty.id},q=2\x1b\\`);
    kitty.shown = null;
    if (state.kind !== 'image') kitty.sent = null;
    return;
  }
  if (kitty.sent !== pic) {
    graphics(`\x1b_Ga=d,d=I,i=${kitty.id},q=2\x1b\\` + transmit(pic.px, kitty.id));
    kitty.sent = pic; kitty.shown = null;
  }
  const at = `${2 - state.top};${pic.pad + 1}`;
  if (kitty.shown === at) return;
  // The cursor saved and restored around the placement, which C=1 keeps from moving anyway.
  graphics(`\x1b7\x1b[${at}H\x1b_Ga=p,i=${kitty.id},p=1,c=${pic.cols},r=${pic.rows},C=1,q=2\x1b\\\x1b8`);
  kitty.shown = at;
}

// After each frame, the images of rendered Markdown in pixels (picture): each one in view placed over its blank rows,
// cut to the rows that show (image.shown: the source rectangle x, y, w, h of its pixels over c × r cells), sent first
// when new. A placement is removed once its image scrolls out, and all are while something covers the pane (the key
// help, the menu) or the view changes; an image no longer in the rows (another file, the source, the Diff view, another
// width) is deleted. Two copies of an image share its id, each placed under the row where it starts (placement id).
function placeFigures() {
  const list = state.rows || [], on = markdownView() && sharp() && !state.help && !state.menu;
  const want = new Map();
  if (on && image) for (const f of image.shown(list, state.top, ui.size().H - 2)) want.set(`${f.pic.id},${f.start + 1}`, f);
  let out = '';
  for (const k of kitty.placed.keys()) {
    if (want.has(k)) continue;
    const [i, p] = k.split(',');
    out += `\x1b_Ga=d,d=i,i=${i},p=${p},q=2\x1b\\`;
    kitty.placed.delete(k);
  }
  for (const id of kitty.figures) if (!(list.figures && list.figures.has(id))) { out += `\x1b_Ga=d,d=I,i=${id},q=2\x1b\\`; kitty.figures.delete(id); }
  for (const [k, f] of want) {
    const { pic } = f;
    if (!kitty.figures.has(pic.id)) { out += transmit(pic.px, pic.id); kitty.figures.add(pic.id); }
    // The image's left column: the row ends with its blank cells.
    const row = state.rows[f.start + f.from], x = ui.segWidth(row.segs) - pic.cols + 1;
    const at = `${2 + f.at};${x}`, part = `${at};${f.y};${f.h};${f.count}`;
    if (kitty.placed.get(k) === part) continue;
    out += `\x1b7\x1b[${at}H\x1b_Ga=p,i=${pic.id},p=${f.start + 1},x=0,y=${f.y},w=${pic.px.width},h=${f.h},c=${pic.cols},r=${f.count},C=1,q=2\x1b\\\x1b8`;
    kitty.placed.set(k, part);
  }
  if (out) graphics(out);
}
// Every image of rendered Markdown deleted: at exit, and before a restart.
function forgetFigures() {
  const out = [...kitty.figures].map(id => `\x1b_Ga=d,d=I,i=${id},q=2\x1b\\`).join('');
  kitty.figures.clear(); kitty.placed.clear();
  return out;
}

// --- rendering

function render() {
  if (state.edit) return renderEdit();
  const { W, H } = ui.size();
  const list = rows();
  const bodyH = H - 2;
  state.top = Math.max(0, Math.min(state.top, list.length - bodyH));
  const lines = [header(W)];
  const g = gutter();
  const ranges = matchRanges(), md = markdownView();
  const current = state.find && state.match >= 0 ? state.matches[state.match] : null;
  // Wrapped and rendered rows (see rows()) leave the last column free for the scrollbar.
  const bw = scrollbar.set(list.length, bodyH, state.top);
  // Nothing to show (an error, an image that cannot be drawn, an empty file): why, centred, and what to do.
  const none = list.length === 1 && list[0].empty ? list[0] : state.view === 'file' && (state.kind === 'text' || state.kind === 'markdown') && state.text === ''
    ? { empty: 'Empty file', pairs: [['i', 'edit here'], ['e', 'edit'], ['q', 'close']] } : null;
  if (none) lines.push(...ui.empty(W, bodyH, none.empty, none.pairs));
  for (let i = state.top; !none && i < Math.min(list.length, state.top + bodyH); i++) {
    const r = list[i];
    const num = g ? [[(r.first ? String(r.src + 1) : '').padStart(g - 1) + ' ',
      r.src >= 0 && noted(r.src) ? NOTE + '\x1b[1m' : fg(r.src + 1 === state.line ? C.text : C.dim)]] : [];
    // The find's matches: every one marked, the current one brighter (in rendered Markdown, matches are by row).
    const at = md ? i : r.src, off = r.off || 0;
    let segs = r.segs;
    if ((md || r.src >= 0) && ranges.has(at)) {
      segs = ui.mark(segs, ranges.get(at).map(([a, b]) => [a - off, b - off]), bg(C.find));
      if (current && current[0] === at) segs = ui.mark(segs, [[current[1] - off, current[2] - off]], bg(CURRENT) + '\x1b[1m');
    }
    // The caller's marks: parts of the line on the selection's colour, whole lines on the line highlight's (back).
    const cols = !md && r.src >= 0 ? marksOf(r.src).filter(m => m.a).map(m => [m.a - 1 - off, m.b - off]) : [];
    if (cols.length) segs = ui.mark(segs, cols, bg(C.select));
    const sel = selectedRange(i);
    // A selection going on past the end of the row shows one more cell, as an editor shows the line break.
    if (sel) segs = [...ui.mark(segs, [sel], bg(C.select)), ...(sel[2] ? [[' ', bg(C.select)]] : [])];
    if (!settings.wrap && r.src >= 0) segs = ui.slice(segs, state.left, W - g - bw);
    const back = r.src >= 0 && (r.src + 1 === state.line && !state.marks.length || marksOf(r.src).some(m => !m.a)) ? C.selectBlur : r.back || null;
    lines.push(ui.line([...num, ...segs], W - bw, back) + scrollbar.cell(i - state.top));
  }
  while (lines.length < H - 1) lines.push(ui.line([], W));
  lines.push(footer(W));
  if (state.menu) state.menu.draw(lines, W, H);
  if (state.help) ui.help(lines, W, path.basename(state.file || ''), HELP, state.help);
  if (state.ask) ui.ask(lines, W, H, state.ask);
  paint(lines);
  showCursor(null);
  placeImage();
  placeFigures();
}

// The marks on a line (0-based).
const marksOf = src => state.marks.filter(m => src + 1 >= m.from && src + 1 <= m.to);

// n and N without a find: the next or previous mark, a third of the way down the pane.
function nextMark(dir) {
  state.mark = (state.mark + dir + state.marks.length) % state.marks.length;
  const from = state.marks[state.mark].from;
  state.top = Math.max(0, (state.view === 'diff' ? diffRow(from) : rowOf(from - 1)) - Math.floor((ui.size().H - 2) / 3));
  flash(`Mark ${state.mark + 1}/${state.marks.length}`);
}
// Marks are on the file's lines: rendered Markdown shows its source instead.
function showMarks() {
  state.mark = 0;
  if (state.marks.length && state.view === 'file' && state.kind === 'markdown' && !state.raw) { state.raw = true; state.rows = null; render(); }
}

// The key help (? or f1), over the whole pane (ui.help); state.help: { top, find } while it shows.
const HELP = [
  ['Scroll', [['↑↓ j k', 'line'], ['pgup pgdn', 'page'], ['space', 'page down'], ['ctrl+u/d', 'half page'], ['g G', 'top, bottom'],
    ['← → h l', 'left, right']]],
  ['Find', [['/', 'find in the file'], ['n N', 'next, previous match (else mark)'], ['esc', 'stop finding, clear the marks']]],
  ['Diff', [[',', 'the view\'s options (or ⚙)'], ['d', 'the file or its changes'], ['n N', 'next, previous hunk'],
    ['b', 'compare with: HEAD, merge base, last agent turn, a commit or range, a branch'], ['v', 'inline, side by side'],
    ['s', 'stage the lines selected (drag), else the hunk at the top (HEAD)'], ['u', 'unstage them'], ['D', 'discard them from the working tree']]],
  ['Tabs', [['tab shift+tab', 'next, previous tab'], ['ctrl+pgdn/pgup', 'next, previous tab'], ['t', 'keep the tab (in italics: the next file clicked replaces it)'],
    ['x', 'close the tab (middle click)']]],
  ['View', [['w', 'wrap'], ['L', 'line numbers'], ['m', 'Markdown source'], ['p', 'image sharp or pixelated'], ['z', 'zoom the pane'], ['< >', 'narrower, wider'], ['shift+← →', 'move the border'], ['r', 'reload']]],
  ['Edit', [['i', 'edit the file here (or the View menu)'], ['ctrl+s', 'save'], ['esc', 'stop editing'], ['ctrl+z ctrl+y', 'undo, redo (or ctrl+shift+z)'],
    ['ctrl+f', 'find: ⏎ ↓ next, ↑ previous'], ['ctrl+r', 'replace: ⏎ one, alt+a all, tab to the other field'], ['ctrl+g', 'go to line'],
    ['ctrl+c ctrl+x ctrl+v', 'copy, cut, paste'], ['ctrl+a', 'select all'], ['shift+arrows', 'select (or drag)'], ['ctrl+← →', 'word'],
    ['tab shift+tab', 'indent, outdent'], ['alt+r alt+k', 'file changed on disk: reload, keep yours']]],
  ['File', [['e', 'edit in the terminal'], ['o', 'open in the editor (media: in its app)'], ['O', 'open with the system'], ['y', 'copy path'],
    ['Y', 'copy relative path, with the selected lines'], ['H', 'history of the file']]],
  ['Review', [['a', 'note on the selected lines (or the file)'], ['tab', 'kind of note, while typing'], ['A', 'send the notes to the agent'],
    ['C', 'copy the notes'], ['U', 'remove the last note']]],
  ['Pane', [['ctrl+p', 'go to file'], ['ctrl+f', 'search in files'], ['? f1', 'keys'], ['q esc', 'close']]],
];

// What the Diff view compares with: the tab's scope, vs HEAD, vs main base, vs last turn, vs abc123, vs feature.
const branchName = () => (state.diff && state.diff.baseName) || 'main';
const base = () => (state.scope.kind === 'ref' ? Scope.label(state.scope) : `vs ${Scope.label(state.scope, branchName())}`);
const diffMode = W => (diffs ? diffs.layoutOf(Math.max(10, W - 2)) : 'inline');

// What the Diff view compares with, in the header (vs HEAD, vs main): the only option that does not show in the
// content itself (⚙ changes it).
const reminder = () => (state.view === 'diff' ? base() : '');

// Header: the tabs on the left, or with a single tab, the file's path; then the details; on the right, what the Diff
// view compares with (dimmed), the File · Diff switch and ⚙, which opens the view's options (,). A narrow pane drops
// the reminder, then the switch, then the details, then the tabs out of reach of the one shown (tabStrip).
// state.chips: the clickable parts, { id, from, to, tab } in columns from 1.
function header(W) {
  const one = tabs.list.length < 2, t = tabs.list[tabs.current];
  const name = state.root ? path.relative(state.root, state.file) || path.basename(state.file) : path.basename(state.file);
  const sw = diffs && state.file ? ui.segmented([['file', 'File'], ['diff', 'Diff']], state.view) : null;
  let note = reminder();
  const gear = 3; // the glyph and the cell after it, which a Nerd Font glyph often spills over
  const room = () => W - gear - (sw ? sw.width + 1 : 0) - (note ? ui.textWidth(note) + 2 : 0);
  const nameRoom = Math.min(20, ui.textWidth(` ${one ? name : path.basename(state.file)} ×  `));
  if (note && room() < nameRoom) note = '';
  const showSwitch = sw && room() >= nameRoom;
  const right = (note ? ui.textWidth(note) + 2 : 0) + (showSwitch ? sw.width + 1 : 0) + gear;
  state.chips = [];
  let segs, lw;
  if (one) {
    // A temporary tab in italics: the next file clicked replaces it.
    const [left, w] = ui.fit(` ${name}${unsavedAt(tabs.current) ? ' ●' : ''}`, Math.max(1, W - right - 1));
    segs = [[left, '\x1b[1m' + (t && !t.kept ? '\x1b[3m' : '')]]; lw = w;
    state.chips.push({ id: 'tab', tab: tabs.current, from: 1, to: w });
  } else {
    const strip = tabStrip(Math.max(1, W - right - 1));
    segs = strip.segs; lw = strip.width;
    state.chips.push(...strip.chips);
  }
  // The details whole, else their first part (the lines), else none; in the Diff view, the added and removed lines.
  const d = state.view === 'diff' && state.diff && state.diff.parsed;
  const details = state.edit ? editInfo() : state.view === 'diff' ? (d ? `+${d.add} −${d.del}` : '') : state.info;
  const info = [`  ${details}`, `  ${String(details).split(' · ')[0]}`, ''].find(t => !t || ui.textWidth(t) < W - right - lw);
  const iw = ui.textWidth(info);
  segs.push([info, fg(C.dim)], [' '.repeat(Math.max(0, W - right - lw - iw)), '']);
  let x = W - right + 1;
  if (note) { segs.push([`${note}  `, fg(C.dim)]); x += ui.textWidth(note) + 2; }
  if (showSwitch) {
    segs.push(...sw.segs, [' ', '']);
    for (const h of sw.hits) state.chips.push({ id: h.id, from: x + h.from, to: x + h.to });
    x += sw.width + 1;
  }
  segs.push([` ${ui.G.gear} `, state.menu ? fg(C.text) : fg(C.dim)]);
  state.chips.push({ id: 'menu', from: x, to: x + gear - 1 });
  return ui.line(segs, W, C.header);
}

// The tabs in room columns (Tabs.strip), from the first column: each one " name × ", the one shown brighter on a lighter
// background, a temporary one in italics; ‹ and › where tabs are hidden, which show the nearest of them. The strip
// moves only when the tab shown would leave it (stripFirst). Returns { segs, width, chips }.
let stripFirst = 0;
const TAB_BACK = [55, 55, 61]; // the tab shown's background
function tabStrip(room) {
  const names = Tabs.labels(tabs.list.map(t => t.file)).map(n => ui.cleanLine(n));
  const s = Tabs.strip(names.map(n => ui.textWidth(n) + 4), tabs.current, room, stripFirst);
  stripFirst = s.first;
  const segs = [], chips = [];
  let x = 1;
  const put = (text, style, chip) => {
    const w = ui.textWidth(text);
    if (chip) chips.push({ ...chip, from: x, to: x + w - 1 });
    segs.push([text, style]); x += w;
  };
  if (s.first > 0) put('‹ ', fg(C.dim), { id: 'tab', tab: s.first - 1 });
  for (let i = s.first; i <= s.last; i++) {
    const on = i === tabs.current, back = on ? bg(TAB_BACK) : '';
    const name = on && s.cut ? ui.ellipsis(names[i], Math.max(1, ui.textWidth(names[i]) - s.cut)) : names[i];
    put(` ${name} `, back + fg(on ? C.text : C.dim) + (on ? '\x1b[1m' : '') + (tabs.list[i].kept ? '' : '\x1b[3m'), { id: 'tab', tab: i });
    // ● for unsaved edits, as VS Code: the click still closes, asking first.
    put(unsavedAt(i) ? '● ' : '× ', back + fg(C.dim), { id: 'close', tab: i });
  }
  if (s.last < tabs.list.length - 1) put(' ›', fg(C.dim), { id: 'tab', tab: s.last + 1 });
  return { segs, width: x - 1, chips };
}

// --- tabs: shown, kept, closed

// The tab i shown as it was left: its file loaded again, at its scroll, in its view.
function showTab(i) {
  if (i === tabs.current || !tabs.list[i]) return render();
  stash();
  tabs = { list: tabs.list, current: i };
  enterTab();
}
function enterTab() {
  unstash();
  state.diff = null; state.find = null; state.matches = []; state.note = null; state.select = null; state.menu = null;
  if (state.view === 'diff') state.wantTop = state.top;
  watch();
  readNotes();
  load(true);
  saveFile();
}
// t, a double-click on the tab, an edit or a note: the tab kept. Returns whether it was temporary.
function keepTab(i = tabs.current) {
  if (!tabs.list[i] || tabs.list[i].kept) return false;
  tabs = Tabs.keep(tabs, i);
  saveFile();
  return true;
}
// x, a click on its × or a middle click: the tab closed, its unsaved edits saved or not first (unsaved). The last one
// closes the preview, as q does.
function closeTab(i = tabs.current) {
  if (tabs.list.length < 2) return quit();
  if (!tabs.list[i]) return;
  unsaved(editAt(i), tabs.list[i].file, () => {
    const was = tabs.current;
    stash();
    tabs = Tabs.close(tabs, i);
    if (i === was) return enterTab();
    saveFile();
    render();
  });
}
// q, or the last tab closed: the preview closes, once the unsaved edits of its tabs are saved or dropped.
function quit() {
  const list = edits().filter(x => x.edit.ed.dirty());
  if (!list.length) return process.exit(0);
  if (list.length === 1) return unsaved(list[0].edit, list[0].file, () => process.exit(0));
  ask({ title: `Unsaved changes in ${list.length} files`, name: list.map(x => path.basename(x.file)).join(', '),
    buttons: [{ key: 's', label: 'Save all', tone: 'go' }, { key: 'd', label: 'Close without saving', tone: 'danger' }, { key: 'escape', label: 'Cancel' }],
    on: { s: () => saveAll(list, () => process.exit(0)), d: () => process.exit(0) } });
}

// --- the view's options (⚙ or ,, as the explorer's settings)

// The options of the view shown, as the explorer's settings show theirs: a choice in blue, or ● on / ○ off. Each
// changes in place with ← →, enter or a click, the menu staying open; the key beside it does the same outside the
// menu. The actions of the footer (z, e…) are not repeated here.
function openMenu(sel = 0) {
  const { W } = ui.size(), items = [], choice = text => [text, fg(C.blue)];
  if (state.view === 'diff') {
    items.push({ label: 'Layout', key: 'v', value: choice(diffMode(W)), run: switchLayout },
      { label: 'Wrap lines', key: 'w', value: ui.onOff(settings.wrap), run: () => toggle('wrap') },
      { label: 'Compare with', key: 'b', value: choice(Scope.label(state.scope, branchName())), run: openScopes, stay: false });
  } else {
    if (state.kind === 'markdown' && markdown && !state.edit) {
      items.push({ label: 'Markdown', key: 'm', value: choice(state.raw ? 'raw' : 'rendered'), run: () => { state.raw = !state.raw; state.rows = null; state.top = 0; } });
    }
    if (!state.edit && (state.kind === 'text' || (state.kind === 'markdown' && state.raw))) {
      items.push({ label: 'Wrap lines', key: 'w', value: ui.onOff(settings.wrap), run: () => toggle('wrap') },
        { label: 'Line numbers', key: 'L', value: ui.onOff(settings.lineNumbers), run: () => toggle('lineNumbers') });
    }
    if (state.edit) items.push({ label: 'Line numbers', key: 'L', value: ui.onOff(settings.lineNumbers), run: () => toggle('lineNumbers') });
    if (state.edit || state.kind === 'text' || state.kind === 'markdown') items.push({ label: 'Edit here', key: 'i', value: ui.onOff(!!state.edit), run: () => (state.edit ? stopEdit() : startEdit()), stay: false });
    if (state.kind === 'image') items.push({ label: 'Image', key: 'p', value: choice(sharp() ? 'sharp' : 'pixelated'), run: toggleSharp });
    else if (markdownView() && hasPictures()) items.push({ label: 'Images', key: 'p', value: choice(sharp() ? 'sharp' : 'pixelated'), run: toggleSharp });
  }
  if (!items.length) return flash('No view option for this file');
  for (const it of items) it.keep = it.stay !== false;
  state.menu = new ui.Menu({ title: 'View', items, at: 1, right: W });
  state.menu.sel = Math.min(sel, items.length - 1);
  render();
}
// A choice of the menu, by key or click: run, then the menu shown again with the new values, unless closed.
function menuChoice(r) {
  const sel = state.menu.sel;
  if (r === 'close' || (r && r.run)) state.menu = null;
  if (r && r.run) { r.run(); if (r.keep) return openMenu(sel); }
  render();
}
function menuKey(k) {
  menuChoice(state.menu.key(k, ui.size().H));
}

function footer(W) {
  // The keys shown, for clicks: none while the find or a message takes the line.
  state.hints = null;
  if (state.revPrompt) {
    const lead = ' Commit or range (abc123, main..HEAD): ';
    return ui.line([[lead, fg(C.blue)], ...state.revPrompt.segments(Math.max(1, W - ui.textWidth(lead)))], W, C.header);
  }
  if (state.find && state.find.active) {
    const count = state.find.field.value ? ` ${state.matches.length ? state.match + 1 : 0}/${state.matches.length}` : '';
    return ui.line([[` ${ui.G.search} `, fg(C.dim)], ...state.find.field.segments(W - 4 - count.length), [count, fg(C.dim)]], W, C.header);
  }
  if (state.note) {
    const n = state.note, lead = ` ✎ ${n.from ? `L${n.from}${n.to > n.from ? `-${n.to}` : ''}` : 'file'} `;
    const kind = n.kind ? `[${n.kind}] ` : '[tab: kind] ';
    return ui.line([[lead, NOTE], [kind, fg(C.dim)], ...n.field.segments(Math.max(1, W - ui.textWidth(lead + kind)))], W, C.header);
  }
  if (state.flash) return ui.line(ui.message(state.flash), W, C.header);
  const list = rows(), bodyH = ui.size().H - 2;
  const pos = list.length > bodyH ? ` ${Math.round(100 * Math.min(1, (state.top + bodyH) / list.length))}%` : '';
  const all = state.noteCount;
  const send = all ? [['A', `send ${all} note${all > 1 ? 's' : ''}`]] : [];
  const marks = [...(state.marks.length > 1 ? [['n N', `${state.marks.length} marks`]] : []), ...(state.marks.length ? [['esc', 'clear']] : [])];
  const keys = ui.keys(state.view === 'diff'
    ? [...send, ...marks, ...(state.marks.length > 1 ? [] : [['n N', 'hunks']]), ['a', 'note'], ['z', 'zoom'], ['q', 'close'], ['?', 'keys']]
    : state.kind === 'image' ? [...send, ['p', sharp() ? 'pixelated' : 'sharp'], ['o', 'open'], ['y', 'copy path'], ['q', 'close'], ['?', 'keys']]
      : [...send, ...marks, ['/', 'find'], ['a', 'note'], ['e', 'edit'], ['o', 'editor'], ['z', 'zoom'], ['q', 'close'], ['?', 'keys']], W - pos.length - 1);
  state.hints = keys;
  return ui.line([...keys, [pos.padStart(Math.max(0, W - ui.segWidth(keys))), fg(C.dim)]], W, C.header);
}

function flash(text) {
  if (!text) return;
  state.flash = text;
  render();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { state.flash = null; render(); }, ui.messageTime(ui.messageKind(text)));
}

// --- find in file

// Case-insensitive unless the query has a capital letter.
function search(jump = true) {
  const q = state.find.field.value;
  state.matches = [];
  if (q) {
    const flags = q !== q.toLowerCase() ? 'g' : 'gi';
    const re = new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), flags);
    // Rendered markdown: matches in the displayed text.
    const lines = markdownView() ? rows().map(r => ui.segText(r.segs)) : state.plain;
    lines.forEach((l, i) => { for (const m of l.matchAll(re)) state.matches.push([i, m.index, m.index + m[0].length]); });
  }
  state.match = -1;
  if (jump) next(1, true);
}

// Matches by line (by row in rendered Markdown): [[start, end)].
const CURRENT = [200, 140, 30]; // the current match, over C.find for the others
function matchRanges() {
  const map = new Map();
  if (!state.matches.length) return map;
  for (const [i, a, b] of state.matches) map.set(i, [...(map.get(i) || []), [a, b]]);
  return map;
}

function next(dir, fromTop) {
  if (!state.matches.length) return render();
  const md = markdownView();
  const at = fromTop ? (md ? state.top : (rows()[state.top] || {}).src || 0) - (dir > 0 ? 1 : 0) : null;
  if (at !== null) {
    const i = state.matches.findIndex(([l]) => l > at);
    state.match = i < 0 ? 0 : i;
  } else state.match = (state.match + dir + state.matches.length) % state.matches.length;
  const [l, a] = state.matches[state.match];
  const row = md ? l : rowOf(l);
  const bodyH = ui.size().H - 2;
  if (row < state.top || row >= state.top + bodyH) state.top = Math.max(0, row - Math.floor(bodyH / 3));
  if (!md) {
    // Less the scrollbar's column, shown when the rows do not fit.
    const width = ui.size().W - gutter() - (rows().length > bodyH ? 1 : 0);
    const col = ui.textWidth(state.plain[l].slice(0, a));
    if (col < state.left || col >= state.left + width) state.left = Math.max(0, col - 8);
  }
  // The match itself is marked: no line highlight.
  state.line = 0;
  render();
}

// --- actions

async function act(promise) { flash(await promise.catch(() => 'Action failed')); }
const herdr = (...args) => run(HERDR, args);

function toggle(name) {
  settings[name] = !settings[name];
  writeSettings({ ...readSettings(), [name]: settings[name] });
  state.rows = null;
  render();
}

// --- mouse selection, by character
//
// A position is { row, at }: a display row and the index of a character in its text. Both ends of the selection
// are included, as in a terminal; at equal to the text length is past the end of the row.

const before = (a, b) => a.row < b.row || (a.row === b.row && a.at <= b.at);
// The end of the character at, with the combining marks that follow it (a decomposed accent, an emoji selector).
function charEnd(text, at) {
  const next = i => String.fromCodePoint(text.codePointAt(i));
  if (at >= text.length) return at;
  let i = at + next(at).length;
  while (i < text.length && ui.cols(next(i)) === 0) i += next(i).length;
  return i;
}

// The position under a screen cell of the body.
function positionAt(row, x) {
  const r = rows()[row];
  const text = ui.segText(r.segs);
  let col = x - 1 - gutter() + (!settings.wrap && r.src >= 0 ? state.left : 0), w = 0, at = 0, prev, prevAt = 0;
  for (const ch of text) {
    const c = ui.cols(ch, prev);
    w += c;
    // The cell an emoji selector adds belongs to the character before it.
    if (w > col) return { row, at: c && ch === '\ufe0f' && prev ? prevAt : at };
    prev = ch; prevAt = at;
    at += ch.length;
  }
  return { row, at: text.length };
}

// The selected characters [start, end) of a display row, and whether the selection takes the line break at its
// end (none between the rows of a wrapped line).
function selectedRange(i) {
  if (!state.select || !state.select.moved) return null;
  const { from, to } = state.select;
  const [a, b] = before(from, to) ? [from, to] : [to, from];
  if (i < a.row || i > b.row) return null;
  const list = rows(), text = ui.segText(list[i].segs);
  const start = i === a.row ? a.at : 0;
  const end = i === b.row ? charEnd(text, b.at) : text.length;
  const next = list[i + 1];
  const lineEnd = !next || next.src < 0 || next.first;
  return [start, end, lineEnd && (i < b.row || b.at >= text.length)];
}

// The selected text: wrapped rows of a line joined again, rendered rows without their left margin and trailing
// spaces.
function selectedText() {
  const { from, to } = state.select;
  const [a, b] = before(from, to) ? [from, to] : [to, from];
  const list = rows();
  let text = '';
  for (let i = a.row; i <= b.row; i++) {
    const r = list[i];
    const [start, end] = selectedRange(i);
    let part = ui.segText(r.segs).slice(Math.max(start, r.pad || 0), end);
    if (r.src < 0) part = part.trimEnd();
    text += (i > a.row && (r.src < 0 || r.first) ? '\n' : '') + part;
  }
  return text;
}

function copySelection() {
  const text = selectedText();
  if (!text) return;
  const n = text.split('\n').length, chars = Array.from(text).length;
  act(actions.copy(text, n > 1 ? `Copied ${n} lines` : `Copied ${chars} character${chars > 1 ? 's' : ''}`));
}

// Ends the selection, copied if the mouse moved: on release, or later when the release came outside the pane
// and never reached it. A copied selection stays shown (done) until the next click or Escape.
function endSelection() {
  if (!state.select || state.select.done) return;
  if (state.select.moved) { copySelection(); state.select.done = true; } else state.select = null;
  render();
}

// The word, run of spaces or punctuation character under a position: its first and last characters.
const WORD = /[\p{L}\p{N}\p{M}_$]/u;
function wordAt({ row, at }) {
  const chars = [];
  let i = 0;
  for (const ch of ui.segText(rows()[row].segs)) { chars.push([i, WORD.test(ch) ? 1 : /\s/.test(ch) ? 2 : 3]); i += ch.length; }
  let a = chars.findIndex(([j]) => j === at), b = a;
  if (a < 0) return null;
  const kind = chars[a][1];
  if (kind !== 3) {
    while (a > 0 && chars[a - 1][1] === kind) a--;
    while (b < chars.length - 1 && chars[b + 1][1] === kind) b++;
  }
  return { from: { row, at: chars[a][0] }, to: { row, at: chars[b][0] }, moved: true, fixed: true };
}

// All the rows of the line under a position, up to its end.
function lineAt({ row }) {
  const list = rows();
  let a = row, b = row;
  if (list[row].src >= 0) {
    while (a > 0 && list[a - 1].src === list[row].src) a--;
    while (b < list.length - 1 && list[b + 1].src === list[row].src) b++;
  }
  return { from: { row: a, at: 0 }, to: { row: b, at: ui.segText(list[b].segs).length }, moved: true, fixed: true };
}

// --- edit mode
//
// i, or Edit here in the View menu, edits a text file in the pane: read again whole (buffer.read: UTF-8 only, up to
// buffer.MAX_BYTES), shown as its source without wrapping, highlighted, with the terminal's cursor. state.edit lives
// in its tab (VIEW), so that the other tabs keep working, and holds:
//   ed      the text, its cursor and history (buffer.Editor)
//   base    the file's stat when read or saved: a file on disk that differs is newer than the text
//   disk    the newer file's stat, or 'gone', while the bar about it shows
//   prompt  the footer's field: { kind: 'find', 'replace' or 'line', field, with (the replacement's field), on (1: in
//           it) }; query, the last one searched
//   hl, found  the highlighted lines and the find's matches, kept until the text changes
// state.top and state.left are then a line and a column of the text. ctrl+s writes it back (saveEdit), esc leaves
// (stopEdit); both, and closing the tab or the pane, ask first about unsaved changes (unsaved) or a newer file.

const sameStat = (a, b) => !!a && !!b && a.mtimeMs === b.mtimeMs && a.size === b.size;
// The edit of tab i (the tab shown has its own in state), every tab's with its file, and whether one is unsaved.
const editAt = i => (i === tabs.current ? state.edit : tabs.list[i] && tabs.list[i].edit) || null;
const edits = () => tabs.list.map((t, i) => ({ edit: editAt(i), file: t.file })).filter(x => x.edit);
const unsavedAt = i => { const e = editAt(i); return !!e && e.ed.dirty(); };
const eolName = ed => (ed.eol === '\r\n' ? 'CRLF' : 'LF');
// The header's details while editing.
function editInfo() {
  const ed = state.edit.ed, n = ed.lines.length;
  return `${ed.dirty() ? 'modified' : 'editing'} · ${n} line${n === 1 ? '' : 's'} · ${eolName(ed)}`;
}

function startEdit() {
  if (state.edit) return;
  if (state.view !== 'file' || (state.kind !== 'text' && state.kind !== 'markdown')) return flash('Cannot edit this file here (e: in the terminal)');
  const r = buffer.read(state.file);
  if (r.error) return flash(`Cannot edit: ${r.error}`);
  // The cursor on the line highlighted, else on the first line shown.
  const row = rows().slice(state.top).find(x => x.src >= 0), first = row ? row.src : 0;
  const ed = new buffer.Editor(r.doc);
  ed.goto(state.line || first + 1);
  state.edit = { ed, base: r.stat, disk: null, prompt: null, query: '', hl: null, found: null };
  state.top = first; state.left = 0;
  state.select = null; state.find = null; state.matches = []; state.menu = null; state.note = null;
  keepTab();
  revealCursor();
  if (r.doc.mixed) flash(`Mixed line endings: saved as ${eolName(ed)}`);
  else render();
}

// esc: back to the preview at the same place, once unsaved changes are saved or dropped.
function stopEdit() {
  unsaved(state.edit, state.file, () => {
    const line = state.top;
    state.edit = null;
    load(true);
    state.top = rowOf(line);
    render();
  });
}

// A question in a card (ui.ask): q.on, what each button's key does; Cancel (escape) leaves things as they were.
function ask(q) { state.ask = { focus: 0, ...q }; render(); }
function askChoice(k) {
  const q = state.ask;
  if (!k) return;
  if (k === 'move') return render();
  state.ask = null;
  if (q.on[k]) q.on[k](); else render();
}
const askKey = k => askChoice(ui.askKey(k, state.ask));
// The folder of a file for a card: relative to the root, nothing for the root itself.
const folderOf = file => {
  const rel = state.root ? path.relative(state.root, path.dirname(file)) : null;
  return rel === '' ? '' : `in ${rel === null || rel.startsWith('..') || path.isAbsolute(rel) ? path.dirname(file) : rel}`;
};

// The unsaved changes of an edit (the tab shown's, or another's) saved or dropped, then then runs.
function unsaved(edit, file, then) {
  if (!edit || !edit.ed.dirty()) return then();
  ask({ title: 'Save the changes?', name: path.basename(file), where: folderOf(file),
    buttons: [{ key: 's', label: 'Save', tone: 'go' }, { key: 'd', label: "Don't save", tone: 'danger' }, { key: 'escape', label: 'Cancel' }],
    on: { s: () => saveEdit(edit, file, then), d: then } });
}

// ctrl+s: the text written over the file (buffer.save) as it was read: its line endings, final newline and byte order
// mark. A file changed on disk since it was read (by an agent, another editor) is replaced only once confirmed.
// then: what to do once saved (a tab or the pane closing).
function saveEdit(edit = state.edit, file = state.file, then = null, force = false) {
  const ed = edit.ed;
  if (!ed.dirty() && edit.disk !== 'gone') return then ? then() : flash('No changes to save');
  let now = null;
  try { now = fs.statSync(file); } catch { }
  if (!force && now && !sameStat(now, edit.base)) {
    return ask({ title: 'The file changed on disk', name: path.basename(file), note: 'Saving replaces the newer file with your text',
      buttons: [{ key: 'o', label: 'Overwrite', tone: 'danger' }, { key: 'escape', label: 'Cancel' }], focus: 1,
      on: { o: () => saveEdit(edit, file, then, true) } });
  }
  try { edit.base = buffer.save(file, buffer.serialize(ed)); } catch (e) { return flash(`Cannot save: ${e.code || e.message}`); }
  ed.markSaved();
  edit.disk = null;
  flash(`Saved ${path.basename(file)} · ${eolName(ed)}${ed.final ? '' : ', no final newline'}`);
  if (then) then();
}
function saveAll(list, then) {
  if (!list.length) return then();
  saveEdit(list[0].edit, list[0].file, () => saveAll(list.slice(1), then));
}

// The file changed on disk while edited (load: the watcher, or the tab shown again). Nothing when it is the text read
// or saved last; read again when nothing was typed; else the bar, which offers to reload it (alt+r) or to keep the
// text (alt+k, the save asking before replacing the newer file).
function diskChanged() {
  const e = state.edit;
  let stat = null;
  try { stat = fs.statSync(state.file); } catch { }
  if (!stat) { e.disk = 'gone'; return; }
  if (sameStat(stat, e.base)) { if (e.disk === 'gone') e.disk = null; return; }
  if (e.ed.dirty()) { e.disk = stat; return; }
  reloadEdit();
}
function reloadEdit() {
  const e = state.edit, r = buffer.read(state.file);
  if (r.error) {
    state.edit = null;
    state.rows = null;
    return flash(`Cannot edit: ${r.error}`);
  }
  e.ed.reset(r.doc);
  Object.assign(e, { base: r.stat, disk: null, hl: null, found: null });
}
function diskKey(k) {
  const e = state.edit;
  if (!e.disk) return;
  if (k === 'alt+r' && e.disk !== 'gone') { reloadEdit(); return state.edit ? flash('Reloaded from disk') : undefined; }
  if (k === 'alt+k') { e.disk = null; render(); }
}

// --- edit mode: drawing

// The gutter: the change's mark, then the line numbers when on.
const editGutter = () => 2 + (settings.lineNumbers ? String(state.edit.ed.lines.length).length : 0);
const editHeight = () => Math.max(1, ui.size().H - 2 - (state.edit.disk ? 1 : 0) - (state.edit.prompt && state.edit.prompt.with ? 1 : 0));

// The text's lines from..to highlighted (their shown text, buffer.shown), with LOOK lines before them, so that a
// comment or a string opened above is still coloured. Kept until an edit, or a scroll out of them.
const LOOK = 200;
function highlighted(from, to) {
  const e = state.edit, ed = e.ed, c = e.hl;
  if (c && c.version === ed.version && from >= c.lo && to <= c.hi) return c;
  const a = Math.max(0, from - LOOK), b = Math.min(ed.lines.length, to + LOOK);
  const text = ed.lines.slice(a, b).map(l => buffer.shown(l).text);
  const lang = highlight && highlight.language(path.basename(state.file));
  let segs = null;
  try { segs = lang ? highlight.highlight(text.join('\n'), lang) : null; } catch { }
  // The highlighter's text must be the line's, the marks (selection, matches) being placed by offsets in it.
  const lines = text.map((t, i) => (segs && segs[i] && ui.segText(segs[i]) === t ? segs[i] : [[t, '']]));
  return (e.hl = { version: ed.version, lo: a && a + LOOK / 4, hi: b, a, lines });
}

// The matches of the find's query, computed once per query and text.
function found() {
  const e = state.edit, q = e.prompt.field.value;
  if (!e.found || e.found.q !== q || e.found.version !== e.ed.version) e.found = { q, version: e.ed.version, list: buffer.matches(e.ed.lines, q) };
  return e.found.list;
}

// The rows above the footer: the bar of a file changed on disk, and the replacement's field. Each with its hints, and
// what a click on one does (act).
const DISK_BACK = [64, 48, 16];
function editBars(W) {
  const e = state.edit, out = [];
  if (e.disk) {
    const gone = e.disk === 'gone', lead = gone ? ' ⚠ Deleted on disk ' : ' ⚠ Changed on disk ';
    const hints = ui.keys(gone ? [['ctrl+s', 'save it again'], ['alt+k', 'dismiss']] : [['alt+r', 'reload, dropping your edits'], ['alt+k', 'keep yours']],
      W - ui.textWidth(lead));
    out.push({ segs: [[lead, fg(C.orange) + '\x1b[1m'], ...hints], hints, from: ui.textWidth(lead), back: DISK_BACK, act: k => editKey(k) });
  }
  const p = e.prompt;
  if (p && p.with) {
    const hints = ui.keys([['⏎', 'replace'], ['alt+a', 'all']]), hw = ui.segWidth(hints), room = Math.max(1, W - 4 - hw);
    const field = p.on ? p.with.segments(room) : [[ui.fit(p.with.value, room)[0], fg(C.dim)]];
    const fw = ui.segWidth(field);
    out.push({ segs: [[' → ', fg(C.dim)], ...field, [' '.repeat(Math.max(0, W - 3 - fw - hw)), ''], ...hints], hints, from: Math.max(3 + fw, W - hw),
      back: C.header, act: k => { p.on = 1; promptKey(k); } });
  }
  return out;
}

function editFooter(W) {
  state.hints = null;
  const e = state.edit, ed = e.ed, p = e.prompt, note = state.flash ? `  ${state.flash}` : '';
  if (p && p.kind === 'line') {
    const lead = ` Go to line (1-${ed.lines.length}) `;
    return ui.line([[lead, fg(C.dim)], ...p.field.segments(Math.max(1, W - ui.textWidth(lead + note) - 1)), [note, fg(C.dim)]], W, C.header);
  }
  if (p) {
    const list = found(), s = ed.selection();
    const i = s ? list.findIndex(m => m.line === s[0].line && m.line === s[1].line && m.a === s[0].at && m.b === s[1].at) : -1;
    const count = p.field.value ? ` ${i + 1}/${list.length}` : '', room = Math.max(1, W - 4 - ui.textWidth(count + note));
    const field = p.on ? [[ui.fit(p.field.value, room)[0], fg(C.dim)]] : p.field.segments(room);
    return ui.line([[` ${ui.G.search} `, fg(C.dim)], ...field, [count + note, fg(C.dim)]], W, C.header);
  }
  if (state.flash) return ui.line(ui.message(state.flash), W, C.header);
  const pos = ` Ln ${ed.cur.line + 1}, Col ${buffer.colOf(ed.lines[ed.cur.line], ed.cur.at) + 1} `;
  const keys = ui.keys([['ctrl+s', 'save'], ['esc', 'done'], ['ctrl+z', 'undo'], ['ctrl+f', 'find'], ['ctrl+r', 'replace'], ['ctrl+g', 'line'],
    ['f1', 'keys']], W - pos.length - 1);
  state.hints = keys;
  return ui.line([...keys, [pos.padStart(Math.max(0, W - ui.segWidth(keys))), fg(C.dim)]], W, C.header);
}

// The text with its gutter: on the left of each line, ▎ in green for a line added since the file was opened, in blue
// for one changed, ▔ in red over a line where lines were removed (▁ under the last line, for the end).
function renderEdit() {
  const { W, H } = ui.size(), e = state.edit, ed = e.ed;
  if (e.reveal) { e.reveal = false; revealCursor(); }
  const bars = editBars(W), bodyH = editHeight();
  state.top = Math.max(0, Math.min(state.top, ed.lines.length - bodyH));
  const bw = scrollbar.set(ed.lines.length, bodyH, state.top);
  const g = editGutter(), width = Math.max(1, W - g - bw), top = state.top;
  const hl = highlighted(top, top + bodyH), { kinds, deleted } = ed.changed(), sel = ed.selection(), p = e.prompt;
  const hits = new Map();
  if (p && p.kind !== 'line') for (const m of found()) if (m.line >= top && m.line < top + bodyH) hits.set(m.line, [...(hits.get(m.line) || []), m]);
  const lines = [header(W)];
  for (let k = 0; k < bodyH; k++) {
    const i = top + k;
    if (i >= ed.lines.length) { lines.push(ui.line([], W - bw) + scrollbar.cell(k)); continue; }
    const { map } = buffer.shown(ed.lines[i]);
    let segs = hl.lines[i - hl.a];
    if (hits.has(i)) segs = ui.mark(segs, hits.get(i).map(m => [map[m.a], map[m.b]]), bg(C.find));
    // The selection, one cell more past the end when it takes the line break.
    if (sel && i >= sel[0].line && i <= sel[1].line) {
      const a = i === sel[0].line ? sel[0].at : 0, b = i === sel[1].line ? sel[1].at : ed.lines[i].length;
      segs = [...ui.mark(segs, [[map[a], map[b]]], bg(C.select)), ...(i < sel[1].line ? [[' ', bg(C.select)]] : [])];
    }
    const kind = kinds[i], end = i === ed.lines.length - 1 && deleted.has(ed.lines.length);
    const mark = kind ? ['▎', fg(kind === 'add' ? C.green : C.blue)] : deleted.has(i) ? ['▔', fg(C.red)] : end ? ['▁', fg(C.red)] : [' ', ''];
    const num = settings.lineNumbers ? String(i + 1).padStart(g - 2) + ' ' : ' ';
    lines.push(ui.line([mark, [num, fg(i === ed.cur.line ? C.text : C.dim)], ...ui.slice(segs, state.left, width)], W - bw) + scrollbar.cell(k));
  }
  for (const b of bars) { b.y = lines.length + 1; lines.push(ui.line(b.segs, W, b.back)); }
  e.bars = bars;
  lines.push(editFooter(W));
  if (state.menu) state.menu.draw(lines, W, H);
  if (state.help) ui.help(lines, W, path.basename(state.file || ''), HELP, state.help);
  if (state.ask) ui.ask(lines, W, H, state.ask);
  paint(lines);
  // The terminal's cursor on the text's, unless something covers it or it is scrolled out.
  const x = g + buffer.colOf(ed.lines[ed.cur.line], ed.cur.at) - state.left + 1, y = ed.cur.line - top + 2;
  showCursor(!state.menu && !state.help && !state.ask && !p && y >= 2 && y < 2 + bodyH && x > g && x <= W - bw ? [y, x] : null);
  placeImage();
  placeFigures();
}

// The terminal's cursor, a bar at [row, column] while editing; hidden otherwise.
let cursorShown = false;
function showCursor(at) {
  if (restarting) return;
  if (at) { process.stdout.write(`\x1b[${at[0]};${at[1]}H\x1b[6 q\x1b[?25h`); cursorShown = true; } else if (cursorShown) { process.stdout.write('\x1b[?25l\x1b[0 q'); cursorShown = false; }
}

// The view moved to the cursor: just enough when it is near, a third of the way down when it jumped (a find, a line).
function revealCursor() {
  const { W } = ui.size(), ed = state.edit.ed, bodyH = editHeight(), line = ed.cur.line;
  if (line < state.top || line >= state.top + bodyH) {
    const far = line < state.top - 1 || line > state.top + bodyH;
    state.top = far ? Math.max(0, line - Math.floor(bodyH / 3)) : line < state.top ? line : line - bodyH + 1;
  }
  const width = Math.max(1, W - editGutter() - (ed.lines.length > bodyH ? 1 : 0)), col = buffer.colOf(ed.lines[line], ed.cur.at);
  const margin = Math.min(8, Math.floor(width / 4));
  if (col < state.left) state.left = Math.max(0, col - margin);
  else if (col >= state.left + width) state.left = col - width + 1 + margin;
}
function follow() {
  revealCursor();
  render();
}

// --- edit mode: keys, mouse, clipboard

// Moves by key, with shift extending the selection.
const MOVES = { left: 'left', right: 'right', up: 'up', down: 'down', home: 'home', end: 'end', pageup: 'pageup', pagedown: 'pagedown',
  'ctrl+left': 'wordleft', 'ctrl+right': 'wordright', 'ctrl+home': 'top', 'ctrl+end': 'bottom' };
function editKey(k) {
  const e = state.edit, ed = e.ed;
  if (e.prompt) return promptKey(k);
  const name = k.replace('shift+', ''), shift = name !== k && k !== 'shift+tab';
  if (MOVES[name]) { ed.move(MOVES[name], shift, editHeight() - 1); return follow(); }
  switch (k) {
    case 'ctrl+s': return saveEdit();
    case 'escape':
      if (ed.selection()) { ed.place(ed.cur); return render(); }
      return stopEdit();
    case 'ctrl+z': ed.undo(); return follow();
    case 'ctrl+y': case 'ctrl+shift+z': ed.redo(); return follow();
    case 'ctrl+c': case 'ctrl+x': return copyCut(k === 'ctrl+x');
    case 'ctrl+v': return pasteClipboard();
    case 'ctrl+a': ed.selectAll(); return render();
    case 'ctrl+f': return openPrompt('find');
    case 'ctrl+r': return openPrompt('replace');
    case 'ctrl+g': return openPrompt('line');
    case 'alt+r': case 'alt+k': return diskKey(k);
    case 'enter': ed.newline(); return follow();
    case 'tab': case 'shift+tab': ed.indentLines(k === 'tab' ? 1 : -1); return follow();
    case 'backspace': case 'alt+backspace': ed.erase(-1, k !== 'backspace'); return follow();
    case 'delete': case 'ctrl+delete': ed.erase(1, k !== 'delete'); return follow();
    case 'f1': state.help = { top: 0, find: null }; return render();
    case 'ctrl+p': return act(actions.search(state.root || path.dirname(state.file), PANE, 'files'));
    case 'ctrl+pagedown': case 'ctrl+pageup': return showTab(Tabs.step(tabs, k === 'ctrl+pagedown' ? 1 : -1).current);
  }
  if (ui.printable(k)) { ed.type(k === 'space' ? ' ' : k, 'type'); follow(); }
}

// ctrl+f, ctrl+r, ctrl+g: the footer's field, with the selected text (or the last query) to find. A find selects the
// first match from the cursor as the query is typed.
function openPrompt(kind) {
  const e = state.edit, ed = e.ed, was = e.prompt, sel = ed.selected();
  const query = sel && !sel.includes('\n') ? sel : e.query;
  e.prompt = { kind, field: new ui.Field(kind === 'line' ? '' : query), with: kind === 'replace' ? new ui.Field(was && was.with ? was.with.value : '') : null, on: 0 };
  render();
}
function promptKey(k) {
  const e = state.edit, ed = e.ed, p = e.prompt, q = p.field.value;
  if (k === 'escape') { e.prompt = null; return render(); }
  if (k === 'ctrl+f' || k === 'ctrl+r' || k === 'ctrl+g') return openPrompt({ 'ctrl+f': 'find', 'ctrl+r': 'replace', 'ctrl+g': 'line' }[k]);
  if (k === 'ctrl+s') return saveEdit();
  if (k === 'alt+r' || k === 'alt+k') return diskKey(k);
  if (p.kind === 'line') {
    if (k === 'enter') {
      const n = parseInt(p.field.value, 10);
      e.prompt = null;
      if (n > 0) ed.goto(Math.min(n, ed.lines.length));
      return follow();
    }
    if (p.field.key(k)) render();
    return;
  }
  if ((k === 'tab' || k === 'shift+tab') && p.with) { p.on = 1 - p.on; return render(); }
  if (k === 'alt+a' && p.with) {
    const n = q ? ed.replaceAll(q, p.with.value) : 0;
    flash(n ? `Replaced ${n}` : 'No match');
    return follow();
  }
  if (k === 'enter' && p.on) {
    if (q) ed.replaceOne(q, p.with.value);
    return follow();
  }
  if (k === 'enter' || k === 'down' || k === 'up') {
    if (q && !ed.find(q, k === 'up' ? -1 : 1)) flash('No match');
    return follow();
  }
  const field = p.on ? p.with : p.field;
  if (!field.key(k)) return;
  if (!p.on) {
    e.query = p.field.value;
    if (e.query) ed.find(e.query, 1, true);
  }
  follow();
}

// ctrl+c, ctrl+x: the selection, else the cursor's line, as VS Code does.
let clip = ''; // the text copied last, for ctrl+v when the system's clipboard cannot be read
function copyCut(cut) {
  const ed = state.edit.ed;
  let text = ed.selected();
  if (!text) {
    text = ed.lines[ed.cur.line] + '\n';
    if (cut) ed.selectLine(ed.cur);
  }
  clip = text;
  if (cut) ed.type('');
  follow();
  const n = text.split('\n').length - (text.endsWith('\n') ? 1 : 0);
  act(actions.copy(text, `${cut ? 'Cut' : 'Copied'} ${n > 1 ? `${n} lines` : `${Array.from(text).length} characters`}`));
}
// ctrl+v: the system's clipboard (actions.clipboard), else the text copied last here; the terminal's own paste comes as
// a bracketed paste (editPaste).
async function pasteClipboard() {
  const e = state.edit;
  const text = (await actions.clipboard()) ?? clip;
  if (state.edit !== e) return;
  if (!text) return flash('Nothing to paste');
  editPaste(text.replace(/\n/g, ' '), text);
}
// A paste: into the footer's field, on one line; else into the text as it came, tabs and lines kept, as one step.
function editPaste(text, raw) {
  const e = state.edit, p = e.prompt;
  if (p) {
    (p.on ? p.with : p.field).insert(text);
    if (!p.on && p.kind !== 'line' && (e.query = p.field.value)) e.ed.find(e.query, 1, true);
    return follow();
  }
  e.ed.type(raw ?? text);
  follow();
}

// The mouse while editing: a click puts the cursor (with shift, extends the selection), a double click selects a word,
// a triple click the line; a drag selects, scrolling at the edges; the wheel scrolls.
let editDrag = false;
function editMouse(m) {
  const e = state.edit, ed = e.ed, { W } = ui.size(), bodyH = editHeight();
  if (m.wheel) { state.top = Math.max(0, Math.min(ed.lines.length - bodyH, state.top + m.wheel * 3)); return render(); }
  if (scrollbar.mouse(m, m.y - 2, m.x === W && !editDrag)) return;
  if (m.y === 1 && !editDrag) return headerMouse(m);
  if (m.y > 1 + bodyH && !editDrag) {
    const bar = m.press && !m.drag && m.button === 0 && (e.bars || []).find(b => b.y === m.y);
    const k = bar && ui.keyAt(bar.hints, m.x - 1, bar.from);
    if (k) bar.act(k);
    return;
  }
  if (m.button !== 0 && !(m.drag && editDrag)) return;
  const g = editGutter();
  const under = () => {
    const line = Math.max(0, Math.min(ed.lines.length - 1, state.top + Math.min(Math.max(m.y, 2), 1 + bodyH) - 2));
    const at = m.y < 2 ? 0 : m.y > 1 + bodyH ? ed.lines[line].length : buffer.atCol(ed.lines[line], Math.max(0, m.x - 1 - g + state.left));
    return { line, at };
  };
  if (m.press && !m.drag) {
    const now = Date.now(), again = now - lastClick.time < 400 && m.x === lastClick.x && m.y === lastClick.y;
    lastClick = { time: now, x: m.x, y: m.y, count: again ? lastClick.count % 3 + 1 : 1 };
    if (lastClick.count === 2) ed.selectWord(under());
    else if (lastClick.count === 3) ed.selectLine(under());
    else ed.place(under(), !!(m.mods & 4));
    editDrag = lastClick.count === 1;
    return render();
  }
  if (m.drag && editDrag) {
    // At the edges, scrolls, then takes the position under the mouse.
    if (m.y <= 2 && state.top > 0) state.top--;
    if (m.y >= 1 + bodyH) state.top = Math.min(state.top + 1, Math.max(0, ed.lines.length - bodyH));
    if (m.x >= W - 1) state.left += 2;
    else if (m.x <= g + 1) state.left = Math.max(0, state.left - 2);
    ed.place(under(), true);
    return render();
  }
  if (!m.press) editDrag = false;
}

// --- input

function scroll(n) {
  state.top += n;
  render();
}

function key(k) {
  if (state.ask) return askKey(k);
  if (state.note) return noteKey(k);
  if (state.revPrompt) {
    if (k === 'escape' || k === 'ctrl+c') { state.revPrompt = null; return render(); }
    if (k === 'enter') { const rev = state.revPrompt.value.trim(); state.revPrompt = null; return rev ? pickScope({ kind: 'rev', rev }) : render(); }
    if (state.revPrompt.key(k)) render();
    return;
  }
  if (state.menu) return menuKey(k);
  if (state.edit && !state.help) return editKey(k);
  if (state.find && state.find.active) {
    if (k === 'escape') { state.find = null; state.matches = []; return render(); }
    if (k === 'enter' || k === 'down') { state.find.active = k !== 'enter' || !state.matches.length; return next(1); }
    if (k === 'up') return next(-1);
    if (state.find.field.key(k)) { search(); return; }
    return;
  }
  const page = ui.size().H - 3;
  // The key help (ui.helpKey): scrolls, finds with /, closes with esc, q, ? or f1.
  // While editing, ctrl+c (copy there) only closes the help, never the pane.
  if (state.help && state.edit && k === 'ctrl+c') { state.help = null; return render(); }
  if (state.help && k !== 'ctrl+c') { if (!ui.helpKey(k, state.help)) state.help = null; return render(); }
  switch (k) {
    case '?': case 'f1': state.help = { top: 0, find: null }; return render();
    case 'q': case 'escape': case 'ctrl+c':
      if (k === 'escape' && state.select) { state.select = null; return render(); }
      if (k === 'escape' && state.marks.length) { state.marks = []; state.rows = null; return render(); }
      if (state.find) { state.find = null; state.matches = []; return render(); }
      return quit();
    case 'up': case 'k': return scroll(-1);
    case 'down': case 'j': case 'enter': return scroll(1);
    case 'pageup': return scroll(-page);
    case 'pagedown': case 'space': return scroll(page);
    case 'ctrl+u': return scroll(-Math.floor(page / 2));
    case 'ctrl+d': return scroll(Math.floor(page / 2));
    case 'home': case 'g': state.top = 0; state.left = 0; return render();
    case 'end': case 'G': state.top = Infinity; return render();
    case 'left': case 'h': state.left = Math.max(0, state.left - 8); return render();
    case 'right': case 'l': if (!settings.wrap) state.left += 8; return render();
    case '/': if (state.view === 'diff') return flash('Find works in the file: d'); state.find = { field: new ui.Field(), active: true }; return render();
    case 'n': case 'N': {
      const dir = k === 'n' ? 1 : -1;
      if (state.marks.length && !state.matches.length) return nextMark(dir);
      return state.view === 'diff' ? nextHunk(dir) : next(dir);
    }
    case 'd': return toggleView();
    case 'tab': case 'ctrl+pagedown': return showTab(Tabs.step(tabs, 1).current);
    case 'shift+tab': case 'ctrl+pageup': return showTab(Tabs.step(tabs, -1).current);
    case 't': return flash(keepTab() ? 'Tab kept: the next file clicked opens in another tab' : 'Tab already kept');
    case 'x': return closeTab();
    case ',': return openMenu();
    case 'b': if (state.view === 'diff') openScopes(); return;
    case 'v': if (state.view === 'diff') switchLayout(); return;
    case 's': if (state.view === 'diff') editDiff('stage'); return;
    case 'u': if (state.view === 'diff') editDiff('unstage'); return;
    case 'D': if (state.view === 'diff') editDiff('discard'); return;
    case 'w': return toggle('wrap');
    case 'L': return toggle('lineNumbers');
    case 'p': return toggleSharp();
    case 'm': if (state.kind === 'markdown') { state.raw = !state.raw; state.rows = null; state.top = 0; render(); } return;
    case 'r': return load(true);
    case 'e': keepTab(); return act(actions.edit(state.file, PANE));
    case 'o': keepTab(); return act(actions.open(state.file, PANE));
    case 'O': return act(actions.system(state.file));
    case 'i': return startEdit();
    case 'y': return act(actions.copy(state.file));
    case 'Y': return copyReference();
    case 'H': return act(actions.scm(state.root || path.dirname(state.file), PANE, state.file, true));
    case 'a': return startNote();
    case 'A': return act(notes.send(TAB, PANE));
    case 'C': return act(notes.copy(TAB, state.root || path.dirname(state.file)));
    case 'U': return act(Promise.resolve(notes.undo(TAB)));
    case 'z': if (PANE) herdr('pane', 'zoom', PANE, '--toggle'); return;
    case '<': case '>': return widen(k === '<' ? -4 : 4);
    // The inner border pushed: on the left of a preview that is the right part of its split, as it is most often.
    case 'shift+left': case 'shift+right': return widen((k === 'shift+left') === (keeper.left() === true) ? -4 : 4);
    case 'ctrl+p': return act(actions.search(state.root || path.dirname(state.file), PANE, 'files'));
    case 'ctrl+f': return act(actions.search(state.root || path.dirname(state.file), PANE, 'content'));
  }
}

// --- review notes and references

// The file's lines under the selection, else the highlighted line: [from, to], 1-based; null when none (rendered
// Markdown, no selection).
function selectedLines() {
  // In the Diff view: the new version's lines of the rows selected, or of the top row.
  if (state.view === 'diff') {
    const list = rows(), sel = state.select && state.select.moved ? state.select : null;
    const [a, b] = sel ? (before(sel.from, sel.to) ? [sel.from.row, sel.to.row] : [sel.to.row, sel.from.row]) : [state.top, state.top];
    const refs = list.slice(a, b + 1).map(r => r.ref).filter(r => r && r.from);
    return refs.length ? [Math.min(...refs.map(r => r.from)), Math.max(...refs.map(r => r.to))] : null;
  }
  if (state.select && state.select.moved) {
    const { from, to } = state.select;
    const [a, b] = before(from, to) ? [from, to] : [to, from];
    const src = rows().slice(a.row, b.row + 1).map(r => r.src).filter(i => i >= 0);
    // A selection ending at the very start of a row leaves that line out, as an editor does.
    if (src.length > 1 && b.at === 0 && rows()[b.row].first && rows()[b.row].src === src[src.length - 1]) src.pop();
    return src.length ? [Math.min(...src) + 1, Math.max(...src) + 1] : null;
  }
  return state.line ? [state.line, state.line] : null;
}

// Y: the path relative to the root, with the selected lines (src/app.js:12-14), to paste into a prompt.
function copyReference() {
  const rel = (state.root ? path.relative(state.root, state.file) : state.file).split(path.sep).join('/');
  const at = selectedLines();
  act(actions.copy(at ? `${rel}:${at[0]}${at[1] > at[0] ? `-${at[1]}` : ''}` : rel, at ? 'Reference copied' : 'Path copied'));
}

function startNote() {
  if (!TAB) return flash('Notes need the explorer');
  if (!state.file || state.kind === 'error') return;
  const at = selectedLines();
  state.note = { field: new ui.Field(), kind: '', from: at ? at[0] : 0, to: at ? at[1] : 0 };
  render();
}

// While a note is typed: Enter saves it, Esc cancels, Tab changes its kind; other keys edit it.
function noteKey(k) {
  const n = state.note;
  if (k === 'escape' || k === 'ctrl+c') { state.note = null; return render(); }
  if (k === 'tab' || k === 'shift+tab') {
    const i = notes.KINDS.indexOf(n.kind);
    n.kind = notes.KINDS[(i + (k === 'tab' ? 1 : notes.KINDS.length - 1)) % notes.KINDS.length];
    return render();
  }
  if (k === 'enter') {
    state.note = null;
    if (!n.field.value.trim()) return render();
    state.select = null;
    const msg = notes.add(TAB, { file: state.file, from: n.from, to: n.to, kind: n.kind, text: n.field.value });
    keepTab();
    loadNotes();
    return flash(msg);
  }
  if (n.field.key(k)) render();
}

// The tab's notes on the shown file, read again when they change (taken or sent in another pane) and placed again
// when the file does (notes.current).
function readNotes() {
  const all = notes.current(TAB), file = state.file && notes.real(state.file);
  state.noteCount = all.length;
  state.notes = all.filter(n => n.file === file);
}
function loadNotes() {
  const was = JSON.stringify(state.notes);
  readNotes();
  if (JSON.stringify(state.notes) !== was) state.rows = null;
  render();
}
if (TAB) notes.watch(TAB, loadNotes);

let lastClick = { time: 0, x: 0, y: 0, count: 0 };
let lastTab = { time: 0, tab: -1 };

function mouse(m) {
  if (state.ask) return askChoice(ui.askMouse(m, state.ask));
  if (state.menu) return menuChoice(state.menu.mouse(m, ui.size().H));
  if (state.help) { if (!ui.helpMouse(m, state.help)) state.help = null; return render(); }
  // A click on a hint of the footer does what its key does.
  if (m.y === ui.size().H && m.press && !m.drag && !m.wheel && m.button === 0 && state.hints) {
    const k = ui.keyAt(state.hints, m.x - 1);
    if (k) return key(k);
  }
  if (state.edit) return editMouse(m);
  // The wheel scrolls under a selection, which may still be dragged: the button state is unknown here.
  if (m.wheel) return scroll(m.wheel * 3);
  const { W, H } = ui.size();
  if (scrollbar.mouse(m, m.y - 2, m.x === W && !(state.select && !state.select.done))) return;
  // The header has the chips; a selection going on is dragged or released over it like over the body.
  const selecting = state.select && !state.select.done && (m.drag || !m.press);
  if (m.y === 1 && !selecting) return headerMouse(m);
  if (m.button !== 0 && !(m.drag && state.select)) return;
  bodyMouse(m);
}

// A click on the header: a tab shown, kept on a double-click, closed by a middle click or its ×; ⚙; File · Diff.
function headerMouse(m) {
  if (!m.press || m.drag || (m.button !== 0 && m.button !== 1)) return;
  endSelection();
  const chip = state.chips.find(c => m.x >= c.from && m.x <= c.to);
  if (!chip) return;
  if (m.button === 1) { if (chip.tab !== undefined) closeTab(chip.tab); return; }
  if (chip.id === 'close') return closeTab(chip.tab);
  if (chip.id === 'tab') {
    const now = Date.now(), again = now - lastTab.time < 400 && lastTab.tab === chip.tab;
    lastTab = { time: again ? 0 : now, tab: chip.tab };
    if (again && keepTab(chip.tab)) return render();
    return showTab(chip.tab);
  }
  if (chip.id === 'menu') return openMenu();
  if (chip.id !== state.view) toggleView();
}

// The body: an image's hint, else the selection of characters.
function bodyMouse(m) {
  const { W, H } = ui.size();
  if (state.kind === 'image') {
    const r = m.press && !m.drag && rows()[state.top + m.y - 2], chip = r && r.hint && r.hint.find(c => m.x >= c.from && m.x <= c.to);
    if (chip) imageClick(chip.id);
    return;
  }
  if (!rows().length) return;
  // Selection of characters with the left button, copied when released; a double click selects a word, a triple
  // click the line. Above the body: from the start of the top row; below: to the end of the bottom row.
  // right: the last column of text, before the scrollbar when it shows.
  const right = W - (scrollbar.bar ? 1 : 0);
  const under = () => {
    const row = Math.max(0, Math.min(rows().length - 1, state.top + Math.min(Math.max(m.y, 2), H - 1) - 2));
    return positionAt(row, m.y < 2 ? -Infinity : m.y >= H ? Infinity : Math.min(m.x, right));
  };
  let pos = under();
  if (m.press && !m.drag) {
    endSelection();
    const now = Date.now();
    const again = now - lastClick.time < 400 && m.x === lastClick.x && m.y === lastClick.y;
    lastClick = { time: now, x: m.x, y: m.y, count: again ? lastClick.count % 3 + 1 : 1 };
    state.select = lastClick.count === 2 ? wordAt(pos) : lastClick.count === 3 ? lineAt(pos) : null;
    if (!state.select) state.select = { from: pos, to: pos, moved: false };
    return render();
  }
  if (!state.select) return;
  if (m.drag) {
    if (state.select.fixed || state.select.done) return;
    // At the edges, scrolls, then takes the position under the mouse again.
    const top = state.top, left = state.left;
    if (m.y <= 2 && state.top > 0) state.top--;
    if (m.y >= H - 1) state.top = Math.min(state.top + 1, Math.max(0, rows().length - (H - 2)));
    // Rendered rows (Markdown, messages) never scroll sideways.
    if (!settings.wrap && rows()[pos.row].src >= 0) {
      if (m.x >= right) state.left += 2;
      else if (m.x <= gutter() + 1) state.left = Math.max(0, state.left - 2);
    }
    if (state.top !== top || state.left !== left) pos = under();
    state.select.to = pos; state.select.moved = true;
    return render();
  }
  if (!m.press) endSelection();
}

// --- startup

const paint = ui.screen({
  key, mouse, reply,
  focus: f => { state.focused = f; if (!f) endSelection(); },
  paste: (text, raw) => {
    if (state.ask) return;
    if (state.edit) return editPaste(text, raw);
    if (state.revPrompt) { state.revPrompt.insert(text); render(); } else if (state.note) { state.note.field.insert(text); render(); } else if (state.find && state.find.active) { state.find.field.insert(text); search(); }
  },
  resize: () => { state.rows = null; render(); keeper.check(); },
}, { motion: true });

// The preview keeps its width when another resize moves it (actions.widthKeeper): the width herdr gave it at first,
// then the one set by dragging its border or with < >. Checked at each resize, so that it goes back while another
// pane's border is still being dragged.
let width = null;
const keeper = actions.widthKeeper(PANE, { columns: () => ui.size().W, get: () => width, set: w => { width = w; } });
// < and >: 4 columns narrower or wider, from the width asked for last, so that a key held down adds up.
function widen(cols) {
  width = (width ?? ui.size().W) + cols;
  keeper.check(true);
}
keeper.check();

// The Images setting changed in the explorer's settings, or by a click in another preview: the image drawn again
// as it says, p's choice forgotten.
if (SETTINGS_FILE) fs.watchFile(SETTINGS_FILE, { interval: 1000 }, () => {
  const images = readSettings().images;
  if (images === settings.images) return;
  settings.images = images;
  state.sharp = null; state.rows = null;
  render();
});

// The Kitty graphics query, and the size of a cell in pixels.
graphics(`${ui.KITTY_QUERY}\x1b[16t`);
setTimeout(settle, 500);
process.on('exit', () => {
  saveFile();
  if (kitty.sent) process.stdout.write(`\x1b_Ga=d,d=I,i=${kitty.id},q=2\x1b\\`);
  if (kitty.figures.size) process.stdout.write(forgetFigures());
  if (cursorShown) process.stdout.write('\x1b[0 q');
});

// The tabs, with the scroll and view of each (the marks of the tab shown only: EXPLORER_MARKS), for the next process.
function savedTabs() {
  stash();
  const fields = ['file', 'kept', ...VIEW.filter(k => k !== 'marks' && k !== 'mark' && k !== 'edit')];
  return { current: tabs.current, list: tabs.list.map(t => Object.fromEntries(fields.map(k => [k, t[k]]))) };
}
// The file shown and the tabs, reopened by the startup hook after a herdr restart.
const saveFile = () => writePaneState(PANE, { file: state.file, root: state.root, line: state.line, tabs: savedTabs() });

// The explorer and the search ask for another file through the request file.
// After a restart on new code, seen is the previous process's: a request made while the pane changed hands is still
// taken, once the file it showed is loaded.
const handed = process.env.EXPLORER_SEEN !== undefined;
let seen = +process.env.EXPLORER_SEEN || 0;
if (REQUEST) {
  const stat = () => { try { return fs.statSync(REQUEST); } catch { return null; } };
  if (!handed) seen = (stat() || {}).mtimeMs || 0;
  else setImmediate(() => { const s = stat(); if (s) take(s); });
  fs.watchFile(REQUEST, { interval: 200 }, take);
}
function take(stat) {
  if (!stat.mtimeMs || stat.mtimeMs === seen) return;
  seen = stat.mtimeMs;
  let req;
  try { req = JSON.parse(fs.readFileSync(REQUEST, 'utf8')); } catch { return; }
  if (!req.file) return;
  // A question of the tab shown is dropped, as canceled: its answer would go to another tab.
  state.ask = null;
  // In its tab, the temporary one, or a new one (Tabs.open). same: a tab that already showed the file, as it was left;
  // other: the pane shows another file than before.
  const prev = state.file, was = tabs.current;
  stash();
  tabs = Tabs.open(tabs, req.file, req.keep === true, t => !!t.edit && t.edit.ed.dirty());
  if (tabs.fresh || tabs.current !== was) unstash();
  const same = !tabs.fresh, other = state.file !== prev || tabs.fresh;
  const view = req.view === 'diff' ? 'diff' : 'file', keep = same && view === state.view;
  state.root = req.root || state.root; state.line = +req.line || 0; state.marks = cleanMarks(req.marks);
  state.raw = same && state.raw;
  if (other || view !== state.view) { state.diff = null; state.find = null; state.matches = []; }
  state.view = view;
  // A tab being edited stays in its edit mode, its cursor on the line asked for.
  if (state.edit) {
    state.view = 'file';
    if (state.line) { state.edit.ed.goto(state.line); state.edit.reveal = true; }
  }
  if (other) { state.find = null; state.note = null; state.select = null; state.menu = null; watch(); readNotes(); }
  if (keep && view === 'diff' && !state.diff) state.wantTop = state.top;
  load(keep && !state.line && !state.marks.length);
  showMarks();
  saveFile();
  if (state.line && state.view === 'file' && !state.edit) { state.top = Math.max(0, rowOf(state.line - 1) - Math.floor((ui.size().H - 2) / 3)); render(); }
}

// Changes on disk, by an agent or an editor: reloaded, keeping the position. The folder is watched rather than
// the file, which editors and agents often replace by renaming a new one over it.
let watcher = null, reload = null;
function watch() {
  if (watcher) watcher.close();
  watcher = null;
  try {
    const base = path.basename(state.file);
    watcher = fs.watch(path.dirname(state.file), (ev, f) => {
      if (f && String(f) !== base) return;
      clearTimeout(reload);
      // The notes follow their lines (notes.current), the file's own reloaded with it.
      reload = setTimeout(() => { readNotes(); load(true); }, 80);
    });
    watcher.on('error', () => { });
  } catch { }
}

if (!state.file) { state.file = ''; setError('No file'); } else {
  readNotes();
  watch();
  // A tab saved before a restart: at its scroll.
  if (restored) {
    Object.assign(state, { top: restored.top, left: restored.left, raw: restored.raw });
    if (state.view === 'diff') state.wantTop = state.top;
  }
  load(!!restored);
  showMarks();
  saveFile();
  if (!restored && state.line && state.view === 'file') { state.top = Math.max(0, rowOf(state.line - 1) - Math.floor((ui.size().H - 2) / 3)); render(); }
}

// Updated plugin: the preview starts again on the new code, on the same file and line, once no search is typed nor
// selection dragged. This process stops watching, the new one taking over the pane.
// Never while a tab is edited: its text would be lost.
update.watch(() => !(state.find && state.find.active) && !state.note && !(state.select && !state.select.done) && !edits().length, () => {
  saveFile();
  clearTimeout(reload);
  if (kitty.sent) graphics(`\x1b_Ga=d,d=I,i=${kitty.id},q=2\x1b\\`);
  if (kitty.figures.size) graphics(forgetFigures());
  restarting = true; kitty.sent = null;
  if (watcher) watcher.close();
  if (REQUEST) fs.unwatchFile(REQUEST);
  if (SETTINGS_FILE) fs.unwatchFile(SETTINGS_FILE);
  ui.restart({ EXPLORER_FILE: state.file, EXPLORER_ROOT: state.root, EXPLORER_LINE: String(state.line), EXPLORER_SEEN: String(seen),
    EXPLORER_MARKS: JSON.stringify(state.marks), EXPLORER_VIEW: state.view, EXPLORER_TABS: JSON.stringify(savedTabs()) });
});
