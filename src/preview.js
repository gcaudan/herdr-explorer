// File preview of a tab, opened next to the terminal by the explorer and the search: syntax-highlighted text,
// rendered Markdown, images (in pixels with the Kitty graphics protocol, else drawn with half blocks), a hex dump for
// other binary files. Reloads by itself when the
// file changes on disk, and shows the next file the explorer asks for (request file, see actions.preview).
'use strict';
// The pane's first process only starts the real one (launch.js).
if (require('./launch').launch()) return;
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const ui = require('./core/ui');
const actions = require('./core/actions');
const notes = require('./core/notes');
const update = require('./core/update');
const { run, HERDR, readSettings, writeSettings, writePaneState, SETTINGS_FILE } = require('./core/exec');

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
  notes: [],        // the tab's review notes on this file (notes.js)
  noteCount: 0,     // the tab's notes on every file
  note: null,       // { field, kind, from, to } while a note is typed
  sharp: null,      // p: images in pixels (true) or half blocks (false) in this pane; null: as the Images setting
};
const scrollbar = new ui.Scrollbar(top => { state.top = top; render(); });

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
  state.rows = null; state.buffer = null; state.matches = []; state.match = -1; state.converting = false; state.missing = null; pictures.clear();
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
  if (state.kind === 'image' && (state.converting || (kitty.probing && state.sharp === null && settings.images === 'auto'))) list = [];
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
    if (res) list = res.lines.map(segs => ({ segs: [[' ', ''], ...segs], src: -1, pad: 1 }));
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
  list.width = width; list.height = H;
  return (state.rows = list);
}

// The review notes shown in the text: each one under the last line it covers, those on the whole file (and every
// one where the rows are not the file's lines: rendered Markdown) at the top.
const NOTE = fg(C.yellow);
function withNotes(list, width) {
  if (!state.notes.length || state.kind === 'image' || state.kind === 'error' || (list.length === 1 && list[0].empty)) return list;
  const lines = !markdownView() && state.kind !== 'binary';
  const rowsOf = n => ui.split([[`✎ ${lines || !n.from ? '' : `${notes.where(n, path.dirname(n.file)).replace(/^.*:/, 'L')} `}`
    + `${n.kind ? `${n.kind}: ` : ''}${ui.cleanLine(n.text)}`, NOTE]], Math.max(1, width - 2))
    .map(segs => ({ segs: [['  ', ''], ...segs], src: -1, note: true }));
  const top = state.notes.filter(n => !lines || !n.from || n.to > state.plain.length).flatMap(rowsOf);
  if (!lines) return [...top, ...list];
  const out = [...top];
  list.forEach((r, i) => {
    out.push(r);
    const next = list[i + 1];
    if (r.src >= 0 && !(next && next.src === r.src)) out.push(...state.notes.filter(n => n.from && n.to === r.src + 1).flatMap(rowsOf));
  });
  return out;
}
const noted = src => state.notes.some(n => n.from && src + 1 >= n.from && src + 1 <= n.to);

const rowOf = src => Math.max(0, rows().findIndex(r => r.src >= src));

// Why an image is not shown, with how to see it when a tool is missing.
const noPreview = why => (state.missing ? `${why || 'No preview'}: ${state.missing}` : why || 'No preview');

// --- images in rendered Markdown

// The images a Markdown file shows, drawn with half blocks under their paragraph: local files only (never a URL),
// relative to the Markdown file, at most 60 columns and 20 rows. Decoded once per file and width (pictures).
const pictures = new Map();
function picture(url, width) {
  if (!image || /^[a-z][\w+.-]*:/i.test(url) || url.startsWith('//')) return null;
  let rel = url.replace(/[?#].*$/, '');
  try { rel = decodeURIComponent(rel); } catch { }
  if (!rel || !image.FORMATS.has(ext(rel)) || ext(rel) === 'svg') return null;
  const abs = path.resolve(path.dirname(state.file), rel), key = `${abs}\0${width}`;
  if (pictures.has(key)) return pictures.get(key);
  let lines = null;
  try {
    const stat = fs.statSync(abs);
    if (stat.isFile() && stat.size <= image.MAX_BYTES) {
      const res = image.render(fs.readFileSync(abs), Math.min(60, width - 2), 20);
      if (res.lines) lines = res.lines.map(segs => [[' ', ''], ...segs]);
    }
  } catch { }
  pictures.set(key, lines);
  return lines;
}

// --- Kitty graphics
//
// The Images setting chooses how an image shows: 'blocks' with half blocks, 'auto' in pixels when the terminal
// answers the protocol's query (herdr does, in a terminal that supports it), 'pixels' in pixels without waiting for
// the answer. In pixels, an image is sent once and placed over blank rows. supported: the terminal answered; probing:
// the answer is awaited, at most 500 ms, before an image is drawn in 'auto'. cell: the size of a cell in pixels,
// [width, height].
const kitty = { supported: false, probing: true, cell: [8, 16], id: 0x4846, sent: null, shown: null };
const sharp = () => (state.sharp ?? (settings.images === 'pixels' || (settings.images === 'auto' && kitty.supported)));

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
// p: this pane's images in pixels or half blocks, whatever the setting.
function toggleSharp() {
  if (state.kind !== 'image') return;
  state.sharp = !sharp();
  state.rows = null;
  if (state.sharp && !kitty.supported && !kitty.probing) flash('The terminal did not answer the Kitty graphics query: the image may not show');
  else render();
}
// restarting: the terminal is being handed to the next process (update), which sends its own image.
let restarting = false;
const graphics = text => { if (!restarting) process.stdout.write(text); };

function reply(text) {
  if (/^\x1b_Gi=31;OK/.test(text)) { kitty.supported = true; kitty.probing = false; state.rows = null; render(); }
  const m = /^\x1b\[6;(\d+);(\d+)t$/.exec(text);
  if (m && +m[1] && +m[2]) { kitty.cell = [+m[2], +m[1]]; state.rows = null; render(); }
}
function settle() {
  if (!kitty.probing) return;
  kitty.probing = false;
  state.rows = null;
  render();
}

// After each frame: the image of the rows placed, sent first when new; removed when none shows (another file, the
// key help over the pane).
function placeImage() {
  const first = state.kind === 'image' && sharp() && !state.help && state.rows && state.rows[0];
  const pic = first && first.picture;
  if (!pic) {
    if (kitty.shown) graphics(`\x1b_Ga=d,d=${state.kind === 'image' ? 'i' : 'I'},i=${kitty.id},q=2\x1b\\`);
    kitty.shown = null;
    if (state.kind !== 'image') kitty.sent = null;
    return;
  }
  if (kitty.sent !== pic) {
    const { px } = pic;
    const data = zlib.deflateSync(Buffer.from(px.data.buffer, px.data.byteOffset, px.data.length)).toString('base64');
    let out = `\x1b_Ga=d,d=I,i=${kitty.id},q=2\x1b\\`;
    for (let i = 0; i < data.length; i += 4096) {
      const more = i + 4096 < data.length ? 1 : 0, chunk = data.slice(i, i + 4096);
      out += i ? `\x1b_Gm=${more};${chunk}\x1b\\` : `\x1b_Ga=t,f=32,o=z,s=${px.width},v=${px.height},i=${kitty.id},q=2,m=${more};${chunk}\x1b\\`;
    }
    graphics(out);
    kitty.sent = pic; kitty.shown = null;
  }
  const at = `${2 - state.top};${pic.pad + 1}`;
  if (kitty.shown === at) return;
  // The cursor saved and restored around the placement, which C=1 keeps from moving anyway.
  graphics(`\x1b7\x1b[${at}H\x1b_Ga=p,i=${kitty.id},p=1,c=${pic.cols},r=${pic.rows},C=1,q=2\x1b\\\x1b8`);
  kitty.shown = at;
}

// --- rendering

function render() {
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
  const none = list.length === 1 && list[0].empty ? list[0] : (state.kind === 'text' || state.kind === 'markdown') && state.text === ''
    ? { empty: 'Empty file', pairs: [['e', 'edit'], ['q', 'close']] } : null;
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
    const sel = selectedRange(i);
    // A selection going on past the end of the row shows one more cell, as an editor shows the line break.
    if (sel) segs = [...ui.mark(segs, [sel], bg(C.select)), ...(sel[2] ? [[' ', bg(C.select)]] : [])];
    if (!settings.wrap && r.src >= 0) segs = ui.slice(segs, state.left, W - g - bw);
    const back = r.src >= 0 && r.src + 1 === state.line ? C.selectBlur : null;
    lines.push(ui.line([...num, ...segs], W - bw, back) + scrollbar.cell(i - state.top));
  }
  while (lines.length < H - 1) lines.push(ui.line([], W));
  lines.push(footer(W));
  if (state.help) ui.help(lines, W, path.basename(state.file || ''), HELP, state.help);
  paint(lines);
  placeImage();
}

// The key help (? or f1), over the whole pane (ui.help); state.help: { top, find } while it shows.
const HELP = [
  ['Scroll', [['↑↓ j k', 'line'], ['pgup pgdn', 'page'], ['space', 'page down'], ['ctrl+u/d', 'half page'], ['g G', 'top, bottom'],
    ['← → h l', 'left, right']]],
  ['Find', [['/', 'find in the file'], ['n N', 'next, previous match'], ['esc', 'stop finding']]],
  ['View', [['w', 'wrap'], ['L', 'line numbers'], ['m', 'Markdown source'], ['p', 'image sharp or pixelated'], ['z', 'zoom the pane'], ['< >', 'narrower, wider'], ['shift+← →', 'move the border'], ['r', 'reload']]],
  ['File', [['e', 'edit in the terminal'], ['o', 'open in the editor (media: in its app)'], ['O', 'open with the system'], ['y', 'copy path'],
    ['Y', 'copy relative path, with the selected lines'], ['H', 'history of the file']]],
  ['Review', [['a', 'note on the selected lines (or the file)'], ['tab', 'kind of note, while typing'], ['A', 'send the notes to the agent'],
    ['u', 'remove the last note']]],
  ['Pane', [['ctrl+p', 'go to file'], ['ctrl+f', 'search in files'], ['? f1', 'keys'], ['q esc', 'close']]],
];

// Header: file name and details on the left; on the right, the view's switches, clickable: Rendered · Raw (the
// active one bold and underlined), and wrap and lines with ● or ○ as in the explorer's settings.
function header(W) {
  const name = state.root ? path.relative(state.root, state.file) || path.basename(state.file) : path.basename(state.file);
  const ON = '\x1b[1;4m', onOff = (on, text) => (on ? [['●', fg(C.blue)], [` ${text}`, '']] : [[`○ ${text}`, fg(C.dim)]]);
  // Groups of [segments, id] items, the last ones left out while the name would get less than 20 columns.
  const groups = [];
  if (state.kind === 'markdown' && markdown) {
    groups.push([[[['Rendered', state.raw ? fg(C.dim) : ON]], 'rendered'], [[[' · ', fg(C.dim)]], null], [[['Raw', state.raw ? ON : fg(C.dim)]], 'raw']]);
  }
  if (state.kind === 'text' || (state.kind === 'markdown' && state.raw)) {
    groups.push([[onOff(settings.wrap, 'wrap'), 'wrap']], [[onOff(settings.lineNumbers, 'lines'), 'lines']]);
  }
  const width = list => list.reduce((w, g) => w + g.reduce((n, [segs]) => n + ui.segWidth(segs), 0), 0) + (list.length ? 3 * list.length - 1 : 0);
  while (groups.length && width(groups) > W - 2 - Math.min(20, ui.textWidth(` ${name}`))) groups.pop();
  const items = groups.flatMap((g, i) => (i ? [[[['   ', '']], null], ...g] : g));
  const right = width(groups) + (groups.length ? 1 : 0);
  const [left, lw] = ui.fit(` ${name}`, Math.max(1, W - right - 1));
  // The details whole, else their first part (the lines), else none, a column before the switches.
  const info = [`  ${state.info}`, `  ${String(state.info).split(' · ')[0]}`, ''].find(t => !t || ui.textWidth(t) < W - right - lw);
  const iw = ui.textWidth(info);
  const segs = [[left, '\x1b[1m'], [info, fg(C.dim)], [' '.repeat(Math.max(0, W - right - lw - iw)), '']];
  let x = W - right + 1;
  state.chips = [];
  for (const [parts, id] of items) {
    const w = ui.segWidth(parts);
    segs.push(...parts);
    if (id) state.chips.push({ id, from: x, to: x + w - 1 });
    x += w;
  }
  return ui.line(segs, W, C.header);
}

function footer(W) {
  // The keys shown, for clicks: none while the find or a message takes the line.
  state.hints = null;
  if (state.find && state.find.active) {
    const count = state.find.field.value ? ` ${state.matches.length ? state.match + 1 : 0}/${state.matches.length}` : '';
    return ui.line([[` ${ui.SEARCH} `, fg(C.dim)], ...state.find.field.segments(W - 4 - count.length), [count, fg(C.dim)]], W, C.header);
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
  const keys = ui.keys(state.kind === 'image' ? [...send, ['p', sharp() ? 'pixelated' : 'sharp'], ['o', 'open'], ['y', 'copy path'], ['q', 'close'], ['?', 'keys']]
    : [...send, ['/', 'find'], ['a', 'note'], ['e', 'edit'], ['o', 'editor'], ['w', 'wrap'], ['z', 'zoom'], ['q', 'close'], ['?', 'keys']], W - pos.length - 1);
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

// --- input

function scroll(n) {
  state.top += n;
  render();
}

function key(k) {
  if (state.note) return noteKey(k);
  if (state.find && state.find.active) {
    if (k === 'escape') { state.find = null; state.matches = []; return render(); }
    if (k === 'enter' || k === 'down') { state.find.active = k !== 'enter' || !state.matches.length; return next(1); }
    if (k === 'up') return next(-1);
    if (state.find.field.key(k)) { search(); return; }
    return;
  }
  const page = ui.size().H - 3;
  // The key help (ui.helpKey): scrolls, finds with /, closes with esc, q, ? or f1.
  if (state.help && k !== 'ctrl+c') { if (!ui.helpKey(k, state.help)) state.help = null; return render(); }
  switch (k) {
    case '?': case 'f1': state.help = { top: 0, find: null }; return render();
    case 'q': case 'escape': case 'ctrl+c':
      if (k === 'escape' && state.select) { state.select = null; return render(); }
      if (state.find) { state.find = null; state.matches = []; return render(); }
      return process.exit(0);
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
    case '/': state.find = { field: new ui.Field(), active: true }; return render();
    case 'n': return next(1);
    case 'N': return next(-1);
    case 'w': return toggle('wrap');
    case 'L': return toggle('lineNumbers');
    case 'p': return toggleSharp();
    case 'm': if (state.kind === 'markdown') { state.raw = !state.raw; state.rows = null; state.top = 0; render(); } return;
    case 'r': return load(true);
    case 'e': return act(actions.edit(state.file, PANE));
    case 'o': return act(actions.open(state.file, PANE));
    case 'O': return act(actions.system(state.file));
    case 'y': return act(actions.copy(state.file));
    case 'Y': return copyReference();
    case 'H': return act(actions.scm(state.root || path.dirname(state.file), PANE, state.file, true));
    case 'a': return startNote();
    case 'A': return act(notes.send(TAB, PANE));
    case 'u': return act(Promise.resolve(notes.undo(TAB)));
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
    loadNotes();
    return flash(msg);
  }
  if (n.field.key(k)) render();
}

// The tab's notes on the shown file, read again when they change (taken or sent in another pane).
function readNotes() {
  const all = notes.read(TAB), file = state.file && notes.real(state.file);
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

function mouse(m) {
  if (state.help) { if (!ui.helpMouse(m, state.help)) state.help = null; return render(); }
  // A click on a hint of the footer does what its key does.
  if (m.y === ui.size().H && m.press && !m.drag && !m.wheel && m.button === 0 && state.hints) {
    const k = ui.keyAt(state.hints, m.x - 1);
    if (k) return key(k);
  }
  // The wheel scrolls under a selection, which may still be dragged: the button state is unknown here.
  if (m.wheel) return scroll(m.wheel * 3);
  const { W, H } = ui.size();
  if (scrollbar.mouse(m, m.y - 2, m.x === W && !(state.select && !state.select.done))) return;
  // The header has the chips; a selection going on is dragged or released over it like over the body.
  const selecting = state.select && !state.select.done && (m.drag || !m.press);
  if (m.y === 1 && !selecting) {
    if (!m.press || m.button !== 0 || m.drag) return;
    endSelection();
    const chip = state.chips.find(c => m.x >= c.from && m.x <= c.to);
    if (!chip) return;
    if (chip.id === 'wrap') return toggle('wrap');
    if (chip.id === 'lines') return toggle('lineNumbers');
    if ((chip.id === 'raw') !== state.raw) { state.raw = !state.raw; state.rows = null; state.top = 0; render(); }
    return;
  }
  if (m.button !== 0 && !(m.drag && state.select)) return;
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
  paste: text => {
    if (state.note) { state.note.field.insert(text); render(); } else if (state.find && state.find.active) { state.find.field.insert(text); search(); }
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
graphics('\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[16t');
setTimeout(settle, 500);
process.on('exit', () => { if (kitty.sent) process.stdout.write(`\x1b_Ga=d,d=I,i=${kitty.id},q=2\x1b\\`); });

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
  if (!same) { state.find = null; state.note = null; watch(); readNotes(); }
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
  readNotes();
  watch();
  load(false);
  saveFile();
  if (state.line) { state.top = Math.max(0, rowOf(state.line - 1) - Math.floor((ui.size().H - 2) / 3)); render(); }
}

// Updated plugin: the preview starts again on the new code, on the same file and line, once no search is typed nor
// selection dragged. This process stops watching, the new one taking over the pane.
update.watch(() => !(state.find && state.find.active) && !state.note && !(state.select && !state.select.done), () => {
  saveFile();
  clearTimeout(reload);
  if (kitty.sent) graphics(`\x1b_Ga=d,d=I,i=${kitty.id},q=2\x1b\\`);
  restarting = true; kitty.sent = null;
  if (watcher) watcher.close();
  if (REQUEST) fs.unwatchFile(REQUEST);
  if (SETTINGS_FILE) fs.unwatchFile(SETTINGS_FILE);
  ui.restart({ EXPLORER_FILE: state.file, EXPLORER_ROOT: state.root, EXPLORER_LINE: String(state.line), EXPLORER_SEEN: String(seen) });
});
