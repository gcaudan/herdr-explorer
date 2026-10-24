// Review notes of a tab: lines of a file (or a whole file) with a remark, taken in the preview and the Changes pane,
// then typed together into the agent's prompt next to the explorer, without submitting it.
// Stored in notes-<tab id>.json in the state folder, so that every pane of the tab sees the same list. Each note keeps
// the text of its lines (anchor), so that it follows them when lines are added or removed above, and is marked stale
// once they are gone (current).
'use strict';
const fs = require('fs');
const path = require('path');
const actions = require('./actions');
const { realPath, writeAtomic, tabFile, readSettings } = require('./exec');

// The kinds a note can have, chosen with Tab while it is typed; '' for a plain remark.
const KINDS = ['', 'fix', 'question', 'nit', 'verify'];

const fileOf = tab => tabFile('notes', tab);

// The notes of a tab: [{ file, from, to, kind, text, t, anchor, stale }], from and to 1-based lines, 0 for the whole
// file; anchor, the text of those lines when the note was taken (none past MAX_ANCHOR lines); stale, they are no
// longer in the file.
function read(tab) {
  try {
    const list = JSON.parse(fs.readFileSync(fileOf(tab), 'utf8'));
    return Array.isArray(list) ? list.filter(n => n && typeof n.file === 'string' && typeof n.text === 'string') : [];
  } catch { return []; }
}

// Written to a temporary file then renamed, so that another pane never reads it half-written. An empty list removes
// the file.
function write(tab, list) {
  const file = fileOf(tab);
  if (!file) return false;
  if (!list.length) { try { fs.unlinkSync(file); } catch { } return true; }
  return writeAtomic(file, JSON.stringify(list));
}

// A note's text keeps no control character, which could submit the prompt once typed.
const clean = text => String(text).replace(/[\x00-\x1f\x7f-\x9f]/g, ' ').trim();

const MAX_ANCHOR = 200;    // lines kept as a note's anchor
const MAX_SIZE = 4 << 20;  // a larger file is not read to place its notes
// The lines of a file; null when it is gone, undefined when they are not known (too large, unreadable for now).
function linesOf(file) {
  try {
    if (fs.statSync(file).size > MAX_SIZE) return undefined;
    return fs.readFileSync(file, 'utf8').split(/\r?\n/);
  } catch (e) { return e.code === 'ENOENT' || e.code === 'ENOTDIR' ? null : undefined; }
}

// A note placed in the file's lines now: where its anchor is, the nearest place to where it was when the text is in
// several; stale when nowhere. The same note when nothing changes, else a new one, and when the lines are not known.
function locate(n, lines) {
  if (!n.from || !Array.isArray(n.anchor) || !n.anchor.length || lines === undefined) return n;
  const len = n.anchor.length, was = n.from - 1;
  const at = i => n.anchor.every((l, k) => lines[i + k] === l);
  let best = -1;
  if (lines && at(was)) best = was;
  else if (lines) {
    for (let i = 0; i + len <= lines.length; i++) if (lines[i] === n.anchor[0] && at(i) && (best < 0 || Math.abs(i - was) < Math.abs(best - was))) best = i;
  }
  if (best < 0) return n.stale ? n : { ...n, stale: true };
  if (best === was && !n.stale) return n;
  const { stale, ...rest } = n;
  return { ...rest, from: best + 1, to: best + len };
}

// The notes of a tab placed again in their files as they are now (locate), saved when one moved, for every pane.
// only: the real path of the one file to read (a pane showing it, which saw it change), the other notes as saved.
function current(tab, only = null) {
  const list = read(tab), files = new Map();
  const placed = list.map(n => {
    if (!n.from || !n.anchor || (only && n.file !== only)) return n;
    if (!files.has(n.file)) files.set(n.file, linesOf(n.file));
    return locate(n, files.get(n.file));
  });
  const moved = placed.filter((n, i) => n !== list[i]);
  return moved.length ? update(tab, moved) : placed;
}

// The same note in two readings of the list: taken at the same time, on the same file, with the same text.
const same = (a, b) => a.t === b.t && a.file === b.file && a.text === b.text;
// Writes the notes moved into the list read again just before, so that a note taken, sent or removed meanwhile by
// another pane is kept; read back after, to write again if another pane wrote over it meanwhile. Returns the list.
function update(tab, moved) {
  let list = read(tab);
  for (let tries = 0; tries < 3; tries++) {
    list = list.map(n => moved.find(m => same(m, n)) || n);
    write(tab, list);
    const back = read(tab);
    if (moved.every(m => !back.some(n => same(m, n)) || back.some(n => n === m || JSON.stringify(n) === JSON.stringify(m)))) return back;
    list = back;
  }
  return list;
}

// A file as notes name it: its real path (realPath, exported as real), so that the preview (from the explorer's
// folder, maybe through a symbolic link such as /home → /var/home) and the diff (from the repository's real folder)
// agree. The lines are those of the file on disk, or given (lines: the text from to to of the version shown, as a diff
// of the index or of a commit shows it; null when it is not known, for no anchor). stale: those lines are no longer
// on disk.
function add(tab, { file, from = 0, to = from, kind = '', text, lines: shown, stale = false }) {
  text = clean(text);
  if (!text || !file) return 'Empty note';
  if (from > to) [from, to] = [to, from];
  const list = read(tab), real = realPath(file);
  let anchor = null;
  if (from && to - from < MAX_ANCHOR) {
    if (shown !== undefined) anchor = shown && shown.length === to - from + 1 ? shown : null;
    else { const lines = linesOf(real); anchor = lines && to <= lines.length ? lines.slice(from - 1, to) : null; }
  }
  list.push({ file: real, from, to, kind: KINDS.includes(kind) ? kind : '', text, t: Date.now(), ...(anchor ? { anchor } : {}),
    ...(stale && anchor ? { stale: true } : {}) });
  return write(tab, list) ? `Note added (${list.length})` : 'Cannot save the note';
}

// Removes the last note taken.
function undo(tab) {
  const list = read(tab);
  if (!list.length) return 'No note';
  const last = list.pop();
  return write(tab, list) ? `Note removed: ${last.text}` : 'Cannot remove the note';
}

// Where a note points, relative to a folder: src/app.js:12, src/app.js:12-14, or the file alone. The folder is
// resolved as the note's file is, so that one reached through a symbolic link still gives a relative path (resolved:
// it already is).
function where(n, dir, resolved = false) {
  const ref = actions.relativeTo(dir && !resolved ? realPath(dir) : dir, n.file);
  return n.from ? `${ref}:${n.from}${n.to > n.from ? `-${n.to}` : ''}` : ref;
}

// Each note as "src/a.js:12-14 (fix): rename it", in file and line order; "(stale)" when its lines are gone.
// The folder is resolved once for all of them.
function items(list, dir) {
  const at = dir && realPath(dir);
  return [...list].sort((a, b) => a.file.localeCompare(b.file) || a.from - b.from || a.t - b.t).map(n => {
    const tags = [n.kind, n.stale && 'stale'].filter(Boolean).join(', ');
    return `${clean(where(n, at, true))}${tags ? ` (${tags})` : ''}: ${clean(n.text)}`;
  });
}

// One line for the agent: "Review notes: src/a.js:12-14 (fix): rename it; …", after the reviewPrefix setting.
const format = (list, dir, prefix = '') => `${clean(prefix) ? `${clean(prefix)} ` : ''}Review notes: ${items(list, dir).join('; ')} `;

// For the clipboard, one note per line under a title, the files relative to dir; the notes are kept.
function copy(tab, dir) {
  const list = current(tab);
  if (!list.length) return Promise.resolve('No note to copy');
  return actions.copy(`Review notes:\n${items(list, dir).map(i => `- ${i}`).join('\n')}\n`,
    `${list.length} note${list.length > 1 ? 's' : ''} copied`);
}

// Types every note of the tab into the terminal next to the explorer, then clears them.
async function send(tab, pane) {
  const list = current(tab);
  if (!list.length) return 'No note to send';
  const prefix = readSettings().reviewPrefix || '';
  const err = await actions.type(dir => format(list, dir, prefix), pane);
  if (err) return err;
  // Notes taken while sending are kept.
  const sent = new Set(list.map(n => n.t));
  write(tab, read(tab).filter(n => !sent.has(n.t)));
  return `${list.length} note${list.length > 1 ? 's' : ''} sent`;
}

// Calls fn whenever the tab's notes change (another pane took or sent one); returns a function that stops watching.
function watch(tab, fn) {
  const file = fileOf(tab);
  if (!file) return () => { };
  fs.watchFile(file, { interval: 500 }, fn);
  return () => fs.unwatchFile(file, fn);
}

module.exports = { KINDS, real: realPath, read, current, locate, add, undo, send, copy, where, format, watch };
