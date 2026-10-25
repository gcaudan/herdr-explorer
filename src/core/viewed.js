// Files marked viewed in the Changes pane of a tab (m), until they change: viewed-<tab id>.json in the state folder,
// { real path: signature of the content when marked }. A viewed file shows folded, and ] and [ skip it.
'use strict';
const fs = require('fs');
const crypto = require('crypto');
const { writeAtomic, tabFile } = require('./exec');

const fileOf = tab => tabFile('viewed', tab);

function read(tab) {
  try {
    const map = JSON.parse(fs.readFileSync(fileOf(tab), 'utf8'));
    return map && typeof map === 'object' && !Array.isArray(map) ? map : {};
  } catch { return {}; }
}

// What the file holds now, in short: a hash of its content, 'gone' once deleted. Hashed again only when its size or
// times change, as git does: the Changes pane asks at each reload. Nor trusted for a file changed less than RACY
// before it was hashed: changed again in the same tick (a coarse clock, as on Windows), its times would not show it.
const RACY = 2000;
const signs = new Map(); // file → { stat: 'size mtime ctime', sign, at: when hashed }
function sign(file) {
  try {
    const st = fs.statSync(file), stat = `${st.size} ${st.mtimeMs} ${st.ctimeMs}`, known = signs.get(file);
    if (known && known.stat === stat && known.at - st.mtimeMs > RACY) return known.sign;
    const at = Date.now(), sign = crypto.createHash('sha1').update(fs.readFileSync(file)).digest('hex');
    signs.set(file, { stat, sign, at });
    return sign;
  } catch { signs.delete(file); return 'gone'; }
}

// Viewed, and unchanged since.
const is = (map, file) => !!map[file] && map[file] === sign(file);

// Marks the file viewed, or no longer; returns whether it is now. Entries of files changed since are dropped.
function toggle(tab, file) {
  const map = read(tab), now = !is(map, file);
  for (const f of Object.keys(map)) if (f === file || !is(map, f)) delete map[f];
  if (now) map[file] = sign(file);
  const path = fileOf(tab);
  if (path && !Object.keys(map).length) { try { fs.unlinkSync(path); } catch { } } else if (path) writeAtomic(path, JSON.stringify(map));
  return now;
}

// Calls fn whenever another pane of the tab marks a file; returns a function that stops watching.
function watch(tab, fn) {
  const file = fileOf(tab);
  if (!file) return () => { };
  fs.watchFile(file, { interval: 500 }, fn);
  return () => fs.unwatchFile(file, fn);
}

module.exports = { read, sign, is, toggle, watch };
