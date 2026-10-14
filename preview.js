// File preview of a tab, opened next to the terminal by the explorer and the search: syntax-highlighted text,
// rendered Markdown, images drawn with half blocks, a hex dump for other binary files. Reloads by itself when the
// file changes on disk, and shows the next file the explorer asks for (request file, see actions.preview).
'use strict';
const fs = require('fs');
const path = require('path');
const ui = require('./ui');
const actions = require('./actions');
const update = require('./update');
const { run, HERDR, readSettings, writeSettings, writePaneState } = require('./exec');

const PANE = process.env.HERDR_PANE_ID;
const REQUEST = process.env.EXPLORER_TAB && actions.requestFile(process.env.EXPLORER_TAB);
const MAX_BYTES = 8 << 20;     // larger files: the start only
const MAX_HIGHLIGHT = 1 << 20; // larger files: plain text
const HEX_BYTES = 64 << 10;
const { C, fg, bg } = ui;

// Optional renderers: the preview still shows plain text without them.
const optional = name => { try { return require(`./${name}`); } catch { return null; } };
const highlight = optional('highlight'), markdown = optional('markdown'), image = optional('image');

const settings = readSettings();
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
};

// --- loading

const ext = file => path.extname(file).slice(1).toLowerCase();
const isMarkdown = file => /^(md|markdown|mdx|mkd)$/.test(ext(file));

function load(keepScroll) {
  const file = state.file;
  let buf;
  try {
    const stat = fs.statSync(file);
    if (stat.isDirectory()) return setError('Folder');
    // A named pipe or a device would block the read, and the whole pane with it.
    if (!stat.isFile()) return setError('Not a regular file');
    const fd = fs.openSync(file, 'r');
    try {
      buf = Buffer.alloc(Math.min(stat.size, MAX_BYTES));
      fs.readSync(fd, buf, 0, buf.length, 0);
    } finally { fs.closeSync(fd); }
    state.info = size(stat.size) + (stat.size > MAX_BYTES ? ' (start only)' : '');
  } catch (e) { return setError(e.code === 'ENOENT' ? 'File deleted' : `Cannot read: ${e.code || e.message}`); }

  const top = state.top, left = state.left;
  state.rows = null; state.buffer = null; state.matches = []; state.match = -1;
  if (image && image.FORMATS.has(ext(file))) {
    state.kind = 'image'; state.buffer = buf; state.plain = []; state.source = [];
    const meta = image.info(buf);
    if (meta) state.info = `${meta.format} ${meta.width}×${meta.height} · ${state.info}`;
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
  render();
}

function setError(text) {
  state.kind = 'error'; state.info = text; state.rows = null;
  state.plain = []; state.source = []; state.buffer = null;
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

const markdownView = () => state.kind === 'markdown' && !state.raw && markdown;
const gutter = () => (settings.lineNumbers && !markdownView() && state.kind !== 'binary' && state.kind !== 'image'
  ? String(state.source.length).length + 2 : 0);

// Display rows for the current width: wrapped or rendered text, built again when the width changes.
function rows() {
  const { W, H } = ui.size();
  const width = Math.max(1, W - gutter() - 1);
  if (state.rows && state.rows.width === width && state.rows.height === H) return state.rows;
  // A selection holds positions in the old rows.
  state.select = null;
  let list = [];
  if (state.kind === 'image') {
    let res;
    try { res = image.render(state.buffer, W, H - 2); } catch { res = null; }
    if (res && res.lines) {
      const pad = Math.max(0, Math.floor((W - res.width) / 2));
      list = res.lines.map(segs => ({ segs: [[' '.repeat(pad), ''], ...segs], src: -1 }));
    } else list = [{ segs: [[` ${(res && res.info) || 'No preview'}`, fg(C.dim)]], src: -1 }];
  } else if (markdownView()) {
    let res;
    try { res = markdown.render(state.text, Math.min(width, 120), { highlight }); } catch { res = null; }
    if (res) list = res.lines.map(segs => ({ segs: [[' ', ''], ...segs], src: -1, pad: 1 }));
    else list = state.source.map((segs, i) => ({ segs, src: i, first: true }));
  } else if (state.kind === 'error') {
    list = [{ segs: [[` ${state.info}`, fg(C.dim)]], src: -1 }];
  } else if (settings.wrap) {
    state.source.forEach((segs, i) => ui.split(segs, width).forEach((part, k) => list.push({ segs: part, src: i, first: !k })));
  } else list = state.source.map((segs, i) => ({ segs, src: i, first: true }));
  list.width = width; list.height = H;
  return (state.rows = list);
}

const rowOf = src => Math.max(0, rows().findIndex(r => r.src >= src));

// --- rendering

function render() {
  const { W, H } = ui.size();
  const list = rows();
  const bodyH = H - 2;
  state.top = Math.max(0, Math.min(state.top, list.length - bodyH));
  const lines = [header(W)];
  const g = gutter();
  const ranges = matchRanges();
  for (let i = state.top; i < Math.min(list.length, state.top + bodyH); i++) {
    const r = list[i];
    const num = g ? [[(r.first ? String(r.src + 1) : '').padStart(g - 1) + ' ', fg(r.src + 1 === state.line ? C.text : C.dim)]] : [];
    let segs = r.src >= 0 && ranges.has(r.src) && r.first && !settings.wrap ? ui.mark(r.segs, ranges.get(r.src), bg(C.find)) : r.segs;
    const sel = selectedRange(i);
    // A selection going on past the end of the row shows one more cell, as an editor shows the line break.
    if (sel) segs = [...ui.mark(segs, [sel], bg(C.select)), ...(sel[2] ? [[' ', bg(C.select)]] : [])];
    if (!settings.wrap && r.src >= 0) segs = ui.slice(segs, state.left, W - g);
    const back = r.src >= 0 && r.src + 1 === state.line ? C.selectBlur : null;
    lines.push(ui.line([...num, ...segs], W, back));
  }
  while (lines.length < H - 1) lines.push(ui.line([], W));
  lines.push(footer(W));
  paint(lines);
}

// Header: file name and details on the left; clickable chips on the right.
function header(W) {
  const name = state.root ? path.relative(state.root, state.file) || path.basename(state.file) : path.basename(state.file);
  const chips = [];
  if (state.kind === 'markdown' && markdown) chips.push([state.raw ? ' Raw ' : ' Rendered ', 'raw']);
  if (state.kind === 'text' || (state.kind === 'markdown' && state.raw)) chips.push([settings.wrap ? ' Wrap ' : ' No wrap ', 'wrap']);
  chips.push([' ✕ ', 'close']);
  const right = chips.reduce((w, [t]) => w + ui.textWidth(t) + 1, 0);
  const [left, lw] = ui.fit(` ${name}`, Math.max(1, W - right - 1));
  const [info, iw] = ui.fit(`  ${state.info}`, Math.max(0, W - right - lw));
  const segs = [[left, '\x1b[1m'], [info, fg(C.dim)], [' '.repeat(Math.max(0, W - right - lw - iw)), '']];
  let x = W - right + 1;
  state.chips = [];
  for (const [text, id] of chips) {
    segs.push([' ', ''], [text, id === 'close' ? fg(C.dim) : bg(C.selectBlur)]);
    state.chips.push({ id, from: x + 1, to: x + ui.textWidth(text) });
    x += ui.textWidth(text) + 1;
  }
  return ui.line(segs, W, C.header);
}

function footer(W) {
  if (state.find && state.find.active) {
    const count = state.find.field.value ? ` ${state.matches.length ? state.match + 1 : 0}/${state.matches.length}` : '';
    return ui.line([[' Find: ', fg(C.dim)], ...state.find.field.segments(W - 8 - count.length), [count, fg(C.dim)]], W, C.header);
  }
  if (state.flash) return ui.line([[` ${state.flash}`, fg(C.yellow)]], W, C.header);
  const list = rows(), bodyH = ui.size().H - 2;
  const pos = list.length > bodyH ? ` ${Math.round(100 * Math.min(1, (state.top + bodyH) / list.length))}%` : '';
  const keys = state.kind === 'image' ? 'o open · y copy path · q close'
    : '/ find · e edit · o editor · w wrap · z zoom · q close';
  return ui.line([[` ${keys}`, fg(C.dim)], [pos.padStart(Math.max(0, W - keys.length - 1)), fg(C.dim)]], W, C.header);
}

function flash(text) {
  if (!text) return;
  state.flash = text;
  render();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { state.flash = null; render(); }, 2500);
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

function matchRanges() {
  const map = new Map();
  if (markdownView() || !state.matches.length) return map;
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
    const width = ui.size().W - gutter();
    const col = ui.textWidth(state.plain[l].slice(0, a));
    if (col < state.left || col >= state.left + width) state.left = Math.max(0, col - 8);
  }
  if (md) state.line = 0; else state.line = l + 1;
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
  let col = x - 1 - gutter() + (!settings.wrap && r.src >= 0 ? state.left : 0), w = 0, at = 0;
  for (const ch of text) {
    w += ui.cols(ch);
    if (w > col) return { row, at };
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

// --- input

function scroll(n) {
  state.top += n;
  render();
}

function key(k) {
  if (state.find && state.find.active) {
    if (k === 'escape') { state.find = null; state.matches = []; return render(); }
    if (k === 'enter' || k === 'down') { state.find.active = k !== 'enter' || !state.matches.length; return next(1); }
    if (k === 'up') return next(-1);
    if (state.find.field.key(k)) { search(); return; }
    return;
  }
  const page = ui.size().H - 3;
  switch (k) {
    case 'q': case 'escape': case 'ctrl+c':
      if (k === 'escape' && state.select) { state.select = null; return render(); }
      if (state.find) { state.find = null; state.matches = []; return render(); }
      return process.exit(0);
    case 'up': case 'k': return scroll(-1);
    case 'down': case 'j': case 'enter': return scroll(1);
    case 'pageup': case 'b': return scroll(-page);
    case 'pagedown': case 'space': return scroll(page);
    case 'ctrl+u': return scroll(-Math.floor(page / 2));
    case 'ctrl+d': return scroll(Math.floor(page / 2));
    case 'home': case 'g': state.top = 0; state.left = 0; return render();
    case 'end': case 'G': state.top = Infinity; return render();
    case 'left': case 'h': state.left = Math.max(0, state.left - 8); return render();
    case 'right': case 'l': if (!settings.wrap) state.left += 8; return render();
    case '/': case 'ctrl+f': state.find = { field: new ui.Field(), active: true }; return render();
    case 'n': return next(1);
    case 'N': return next(-1);
    case 'w': return toggle('wrap');
    case 'L': return toggle('lineNumbers');
    case 'm': if (state.kind === 'markdown') { state.raw = !state.raw; state.rows = null; state.top = 0; render(); } return;
    case 'r': return load(true);
    case 'e': return act(actions.edit(state.file, PANE));
    case 'o': return act(actions.open(state.file, PANE));
    case 'O': return act(actions.system(state.file));
    case 'y': return act(actions.copy(state.file));
    case 'Y': return act(actions.copy(state.root ? path.relative(state.root, state.file) : state.file));
    case 'z': if (PANE) herdr('pane', 'zoom', PANE, '--toggle'); return;
    case 'ctrl+p': return act(actions.search(state.root || path.dirname(state.file), PANE, 'files'));
  }
}

let lastClick = { time: 0, x: 0, y: 0, count: 0 };

function mouse(m) {
  // The wheel scrolls under a selection, which may still be dragged: the button state is unknown here.
  if (m.wheel) return scroll(m.wheel * 3);
  const H = ui.size().H;
  // The header has the chips; a selection going on is dragged or released over it like over the body.
  const selecting = state.select && !state.select.done && (m.drag || !m.press);
  if (m.y === 1 && !selecting) {
    if (!m.press || m.button !== 0 || m.drag) return;
    endSelection();
    const chip = state.chips.find(c => m.x >= c.from && m.x <= c.to);
    if (!chip) return;
    if (chip.id === 'close') return process.exit(0);
    if (chip.id === 'wrap') return toggle('wrap');
    if (chip.id === 'raw') { state.raw = !state.raw; state.rows = null; state.top = 0; return render(); }
    return;
  }
  if (m.button !== 0 && !(m.drag && state.select)) return;
  if (state.kind === 'image' || !rows().length) return;
  // Selection of characters with the left button, copied when released; a double click selects a word, a triple
  // click the line. Above the body: from the start of the top row; below: to the end of the bottom row.
  const under = () => {
    const row = Math.max(0, Math.min(rows().length - 1, state.top + Math.min(Math.max(m.y, 2), H - 1) - 2));
    return positionAt(row, m.y < 2 ? -Infinity : m.y >= H ? Infinity : m.x);
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
      if (m.x >= ui.size().W) state.left += 2;
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
  key, mouse,
  focus: f => { state.focused = f; if (!f) endSelection(); },
  paste: text => { if (state.find && state.find.active) { state.find.field.insert(text); search(); } },
  resize: () => { state.rows = null; render(); },
}, { motion: true });

// The file shown, reopened by the startup hook after a herdr restart.
const saveFile = () => writePaneState(PANE, { file: state.file, root: state.root, line: state.line });

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
  const same = req.file === state.file;
  state.file = req.file; state.root = req.root || state.root; state.line = +req.line || 0;
  state.raw = same && state.raw;
  if (!same) { state.find = null; watch(); }
  load(same && !state.line);
  saveFile();
  if (state.line) { state.top = Math.max(0, rowOf(state.line - 1) - Math.floor((ui.size().H - 2) / 3)); render(); }
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
      reload = setTimeout(() => load(true), 80);
    });
    watcher.on('error', () => { });
  } catch { }
}

if (!state.file) { state.file = ''; setError('No file'); } else {
  watch();
  load(false);
  saveFile();
  if (state.line) { state.top = Math.max(0, rowOf(state.line - 1) - Math.floor((ui.size().H - 2) / 3)); render(); }
}

// Updated plugin: the preview starts again on the new code, on the same file and line, once no search is typed nor
// selection dragged. This process stops watching, the new one taking over the pane.
update.watch(() => !(state.find && state.find.active) && !(state.select && !state.select.done), () => {
  saveFile();
  clearTimeout(reload);
  if (watcher) watcher.close();
  if (REQUEST) fs.unwatchFile(REQUEST);
  ui.restart({ EXPLORER_FILE: state.file, EXPLORER_ROOT: state.root, EXPLORER_LINE: String(state.line), EXPLORER_SEEN: String(seen) });
});
