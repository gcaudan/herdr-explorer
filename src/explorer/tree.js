// The explorer's tree, apart from the terminal: the folders read (cache), the git status of the root, the rows of a
// view (folders expanded, filter, changed files only) and the order of the entries. Nothing runs at require time:
// create() makes the tree of a root.
'use strict';
const fs = require('fs');
const path = require('path');
const { remoteLink } = require('../core/exec');

const FILTER_BUDGET = 20000; // entries read at most while filtering

const RANK = { U: 1, A: 2, R: 2, M: 3, D: 4, C: 5 };

const parentOf = rel => (rel === '' ? null : rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '');

// Whether a row shows the entry rel: its own, or a folder of its compact chain (from, the chain's first folder).
const holds = (row, rel) => row.rel === rel
  || (!!row.from && (rel === row.from || rel.startsWith(`${row.from}/`)) && row.rel.startsWith(`${rel}/`));

// Folders first, then names, numbers in their order.
const ENTRY_ORDER = (a, b) => (b.dir - a.dir) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });

function code(x, y) {
  if (x === '?') return 'U';
  if (x === '!') return '!';
  if (x === 'U' || y === 'U' || (x === 'A' && y === 'A') || (x === 'D' && y === 'D')) return 'C';
  if (x === 'D' || y === 'D') return 'D';
  if (x === 'R' || y === 'R') return 'R';
  if (x === 'A' || x === 'C') return 'A';
  return 'M';
}

// The output of git status --porcelain=v1 -z, its paths taken relative to the folder prefix of the repository: the
// changed files, each folder with the strongest change it holds (RANK), the untracked folders and the ignored entries.
function parseStatus(status, prefix) {
  const git = { files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set() };
  const parts = status.split('\0');
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (p.length < 4) continue;
    const c = code(p[0], p[1]);
    if ('RC'.includes(p[0]) || 'RC'.includes(p[1])) i++; // the original path follows (renamed in the index or the tree)
    let rel = p.slice(3);
    if (prefix) {
      if (!rel.startsWith(prefix + '/')) continue;
      rel = rel.slice(prefix.length + 1);
    }
    const isDir = rel.endsWith('/');
    if (isDir) rel = rel.slice(0, -1);
    if (c === '!') { git.ignored.add(rel); continue; }
    if (isDir) git.untracked.add(rel); else git.files.set(rel, c);
    for (let a = parentOf(rel); a !== null; a = parentOf(a)) {
      if (!git.dirs.has(a) || RANK[c] > RANK[git.dirs.get(a)]) git.dirs.set(a, c);
    }
  }
  return git;
}

// Names in the tree's order, numbers in their order.
const NAME_ORDER = (a, b) => a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
// Whether a comes before b in the tree; aDir and bDir: whether each is a folder.
function treeBefore(a, aDir, b, bDir) {
  const x = a.split('/'), y = b.split('/');
  for (let i = 0; i < Math.min(x.length, y.length); i++) {
    if (x[i] === y[i]) continue;
    const xd = i < x.length - 1 || aDir, yd = i < y.length - 1 || bDir;
    if (xd !== yd) return xd;
    return NAME_ORDER(x[i], y[i]) < 0;
  }
  return x.length < y.length;
}

// The tree of the folder root() returns (Set as root changes it). settings: hidden, ignored and compact, read at each
// call.
function create(root, settings) {
  // gone: the deleted files no longer on disk, which the tree shows only among the changed files.
  const git = { top: null, prefix: '', files: new Map(), dirs: new Map(), untracked: new Set(), ignored: new Set(), gone: [] };
  const cache = new Map(); // relative folder → sorted entries
  // After a change on disk (stale), a folder read is read again only when its modification time moved: the
  // compact folders look into every folder shown, collapsed ones included, at each build of the rows. A time too
  // close to the reading (within the 2 s of the coarsest file systems) is not trusted.
  const stamps = new Map(); // relative folder → its modification time when read, or null
  let check = new Set(); // folders read, to compare with their modification time before reuse
  const mtime = rel => fs.statSync(path.join(root(), rel), { throwIfNoEntry: false })?.mtimeMs ?? null;

  function readDir(rel) {
    if (cache.has(rel)) {
      if (!check.has(rel)) return cache.get(rel);
      check.delete(rel);
      const was = stamps.get(rel);
      if (was != null && mtime(rel) === was) return cache.get(rel);
    }
    const t = mtime(rel);
    stamps.set(rel, t !== null && Date.now() - t > 2000 ? t : null);
    let entries = [];
    try {
      entries = fs.readdirSync(path.join(root(), rel), { withFileTypes: true })
        .filter(e => e.name !== '.git')
        .map(e => {
          let dir = e.isDirectory();
          // A link to a network share stays a plain entry, never followed (see remoteLink).
          const full = path.join(root(), rel, e.name);
          if (e.isSymbolicLink() && !remoteLink(full, root())) try { dir = fs.statSync(full).isDirectory(); } catch { }
          return { name: e.name, dir, rel: rel ? `${rel}/${e.name}` : e.name };
        })
        .sort(ENTRY_ORDER);
    } catch { }
    cache.set(rel, entries);
    return entries;
  }

  // Entries shown with the current settings: dot files and git-ignored ones can be hidden.
  const visible = rel => readDir(rel).filter(e => (settings.hidden || !e.name.startsWith('.'))
    && (settings.ignored || decoration(e) !== '!'));

  // A changed entry: a file with a git change, or a folder holding one.
  const isChanged = e => { const d = decoration(e); return !!d && d !== '!'; };

  // The entries of a folder among the changed files: those on disk, and the deleted files and folders no longer there
  // (gone), in the tree's order.
  function withGone(rel) {
    const list = visible(rel), here = new Set(list.map(e => e.name)), pre = rel ? `${rel}/` : '', gone = new Map();
    for (const g of git.gone) {
      if (!g.startsWith(pre)) continue;
      const rest = g.slice(pre.length), i = rest.indexOf('/'), name = i < 0 ? rest : rest.slice(0, i);
      if (!here.has(name) && (settings.hidden || !name.startsWith('.'))) gone.set(name, i >= 0);
    }
    if (!gone.size) return list;
    return [...list, ...[...gone].map(([name, dir]) => ({ name, dir, rel: pre + name, gone: true }))].sort(ENTRY_ORDER);
  }

  // Compact folders: a folder whose only entry (in list) is a folder shows on one row with it, src/main/java, as the
  // last one (its rel, expanded or not), chain the names before it ('src/main/'), from the first one's rel. At most
  // 32 folders, as a link to a folder above would make the chain endless.
  function compact(e, list) {
    if (!settings.compact || !e.dir) return e;
    let last = e;
    const names = [];
    for (let n = 0; n < 32; n++) {
      const kids = list(last.rel);
      if (kids.length !== 1 || !kids[0].dir) break;
      names.push(last.name);
      last = kids[0];
    }
    return last === e ? e : { ...last, from: e.rel, chain: `${names.join('/')}/` };
  }

  // The rows of a view: { query } the filter's text in lower case (no compact folders), { changed } the changed files
  // only, { expanded } the folders unfolded.
  function rows({ query, changed, expanded }) {
    const rows = [];
    if (query) {
      // Every folder is read, except ignored ones (node_modules, build output), within a budget.
      let budget = FILTER_BUDGET;
      const walk = (rel, depth) => {
        const found = [];
        for (const e of changed ? withGone(rel) : visible(rel)) {
          if (--budget < 0) break;
          if (changed && !isChanged(e)) continue;
          const at = e.name.toLowerCase().indexOf(query);
          const kids = e.dir && !git.ignored.has(e.rel) ? walk(e.rel, depth + 1) : [];
          if (at >= 0 || kids.length) found.push({ ...e, depth, hit: at >= 0 ? [at, at + query.length] : null, open: kids.length > 0 }, ...kids);
        }
        return found;
      };
      rows.push(...walk('', 0));
    } else if (changed) {
      // Only the changed entries, every folder unfolded, within the filter's budget.
      let budget = FILTER_BUDGET;
      const list = rel => withGone(rel).filter(isChanged);
      const walk = (rel, depth) => {
        for (const entry of withGone(rel)) {
          if (--budget < 0) return;
          if (!isChanged(entry)) continue;
          const e = compact(entry, list);
          rows.push({ ...e, depth, open: e.dir });
          if (e.dir) walk(e.rel, depth + 1);
        }
      };
      walk('', 0);
    } else {
      const walk = (rel, depth) => {
        for (const entry of visible(rel)) {
          const e = compact(entry, visible);
          rows.push({ ...e, depth, open: e.dir && expanded.has(e.rel) });
          if (e.dir && expanded.has(e.rel)) walk(e.rel, depth + 1);
        }
      };
      walk('', 0);
    }
    return rows;
  }

  // A new git status (parseStatus), and the deleted files no longer on disk.
  function applyStatus(status) {
    Object.assign(git, parseStatus(status, git.prefix));
    git.gone = [...git.files].filter(([rel, c]) => c === 'D' && !fs.existsSync(path.join(root(), rel))).map(([rel]) => rel);
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
  function hidden(rel, expanded) {
    for (let a = parentOf(rel); a !== null && a !== ''; a = parentOf(a)) {
      if (git.ignored.has(a) && !expanded.has(a)) return true;
    }
    return false;
  }

  // The changed files in the tree's order (folders first, then names, as readDir sorts them).
  function changedFiles() {
    const out = [];
    let budget = FILTER_BUDGET;
    const walk = rel => {
      for (const e of withGone(rel)) {
        if (--budget < 0) return;
        if (!isChanged(e)) continue;
        if (e.dir) walk(e.rel); else out.push(e.rel);
      }
    };
    walk('');
    return out;
  }

  // A change on disk: the folders read are checked again before reuse.
  function stale() { check = new Set(cache.keys()); }

  return { git, cache, stale, readDir, visible, isChanged, withGone, rows, applyStatus, decoration, hidden, changedFiles };
}

module.exports = { FILTER_BUDGET, RANK, parentOf, holds, ENTRY_ORDER, NAME_ORDER, code, parseStatus, treeBefore, create };
