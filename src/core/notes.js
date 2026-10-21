// Review notes of a tab: lines of a file (or a whole file) with a remark, taken in the preview and the Changes pane,
// then typed together into the agent's prompt next to the explorer, without submitting it.
// Stored in notes-<tab id>.json in the state folder, so that every pane of the tab sees the same list.
'use strict';
const fs = require('fs');
const path = require('path');
const actions = require('./actions');
const { realPath, writeAtomic } = require('./exec');

// The kinds a note can have, chosen with Tab while it is typed; '' for a plain remark.
const KINDS = ['', 'fix', 'question', 'nit', 'verify'];

const fileOf = tab => process.env.HERDR_PLUGIN_STATE_DIR && tab
  && path.join(process.env.HERDR_PLUGIN_STATE_DIR, `notes-${String(tab).replace(/[^\w-]/g, '_')}.json`);

// The notes of a tab: [{ file, from, to, kind, text, t }], from and to 1-based lines, 0 for the whole file.
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

// A file as notes name it: its real path, so that the preview (from the explorer's folder, maybe through a symbolic
// link such as /home → /var/home) and the diff (from the repository's real folder) agree.
const real = realPath;

function add(tab, { file, from = 0, to = from, kind = '', text }) {
  text = clean(text);
  if (!text || !file) return 'Empty note';
  if (from > to) [from, to] = [to, from];
  const list = read(tab);
  list.push({ file: real(file), from, to, kind: KINDS.includes(kind) ? kind : '', text, t: Date.now() });
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
// resolved as the note's file is, so that one reached through a symbolic link still gives a relative path.
function where(n, dir) {
  const ref = actions.relativeTo(dir && real(dir), n.file);
  return n.from ? `${ref}:${n.from}${n.to > n.from ? `-${n.to}` : ''}` : ref;
}

// Each note as "src/a.js:12-14 (fix): rename it", in file and line order.
const items = (list, dir) => [...list].sort((a, b) => a.file.localeCompare(b.file) || a.from - b.from || a.t - b.t)
  .map(n => `${clean(where(n, dir))}${n.kind ? ` (${n.kind})` : ''}: ${clean(n.text)}`);

// One line for the agent: "Review notes: src/a.js:12-14 (fix): rename it; …".
const format = (list, dir) => `Review notes: ${items(list, dir).join('; ')} `;

// For the clipboard, one note per line under a title, the files relative to dir; the notes are kept.
function copy(tab, dir) {
  const list = read(tab);
  if (!list.length) return Promise.resolve('No note to copy');
  return actions.copy(`Review notes:\n${items(list, dir).map(i => `- ${i}`).join('\n')}\n`,
    `${list.length} note${list.length > 1 ? 's' : ''} copied`);
}

// Types every note of the tab into the terminal next to the explorer, then clears them.
async function send(tab, pane) {
  const list = read(tab);
  if (!list.length) return 'No note to send';
  const err = await actions.type(dir => format(list, dir), pane);
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

module.exports = { KINDS, real, read, add, undo, send, copy, where, format, watch };
