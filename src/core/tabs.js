// The preview's tabs, as VS Code's editors: { list, current }, a list of { file, kept, … } and the index of the tab
// shown. A file opened by a click or by following the selection replaces the temporary tab (kept false, shown in
// italics); a double-click, Enter, an edit or t keeps it. Each function returns a new { list, current }, leaving its
// input alone; the other fields of a tab (its scroll, view, marks) are the preview's.
'use strict';
const path = require('path');

// At most this many tabs: past it, the one furthest from the tab opened goes.
const MAX = 50;

// A file shown: in its tab when one has it (kept when asked), else in the temporary tab, else in a new tab after the
// current one. fresh: the tab is new or replaced, its view to start over.
function open({ list, current }, file, keep = false) {
  const at = list.findIndex(t => t.file === file);
  if (at >= 0) return { list: list.map((t, i) => (i === at && keep && !t.kept ? { ...t, kept: true } : t)), current: at, fresh: false };
  const tab = { file, kept: !!keep };
  const temp = list.findIndex(t => !t.kept);
  if (temp >= 0) return { list: list.map((t, i) => (i === temp ? tab : t)), current: temp, fresh: true };
  const i = Math.min(list.length, current + 1);
  let next = [...list.slice(0, i), tab, ...list.slice(i)], shown = i;
  if (next.length > MAX) { const drop = shown > next.length / 2 ? 0 : next.length - 1; next = next.filter((_, k) => k !== drop); if (drop < shown) shown--; }
  return { list: next, current: shown, fresh: true };
}

// The tab i closed: the one on its right shown in its place, else the one on its left. The last tab stays (the preview
// closes instead, see the preview's closeTab).
function close({ list, current }, i) {
  if (list.length <= 1 || i < 0 || i >= list.length) return { list, current };
  const next = list.filter((_, k) => k !== i);
  return { list: next, current: i < current ? current - 1 : Math.min(current, next.length - 1) };
}

// The next tab (dir 1) or the previous one (-1), going round.
const step = ({ list, current }, dir) => ({ list, current: list.length ? (current + dir + list.length) % list.length : 0 });

// The tab i kept: the next file opened by a click no longer replaces it.
const keep = ({ list, current }, i = current) => ({ list: list.map((t, k) => (k === i && !t.kept ? { ...t, kept: true } : t)), current });

// The names on the tabs: the file name, with its folder when another tab has the same name (src/index.js, test/index.js).
function labels(files) {
  const count = new Map();
  for (const f of files) count.set(path.basename(f), (count.get(path.basename(f)) || 0) + 1);
  return files.map(f => (count.get(path.basename(f)) > 1 ? `${path.basename(path.dirname(f))}/${path.basename(f)}` : path.basename(f)));
}

// The tabs that fit in room columns, the current one always among them: widths, the columns of each tab; first, the
// first tab shown before, so that the strip moves only when the current tab would leave it. A side with hidden tabs
// takes MORE columns for its arrow (‹ ›). Returns { first, last } (both shown), and cut: the columns that the current
// tab, alone and too wide, must lose.
const MORE = 2;
function strip(widths, current, room, first = 0) {
  const n = widths.length;
  if (!n) return { first: 0, last: -1, cut: 0 };
  const need = (a, b) => widths.slice(a, b + 1).reduce((s, w) => s + w, 0) + (a > 0 ? MORE : 0) + (b < n - 1 ? MORE : 0);
  first = Math.max(0, Math.min(first, current));
  let last = current;
  while (first < current && need(first, last) > room) first++;
  while (last + 1 < n && need(first, last + 1) <= room) last++;
  while (first > 0 && need(first - 1, last) <= room) first--;
  return { first, last, cut: Math.max(0, need(first, last) - room) };
}

module.exports = { MAX, MORE, open, close, step, keep, labels, strip };
