// Search popup opened from the explorer: Go to file (fuzzy match on paths, ctrl+p) and Search in files
// (text or regex in the contents, ctrl+f), with a preview of the selected result on wide screens.
// Enter opens the file preview pane at the line; Esc closes the popup.
'use strict';
const fs = require('fs');
const fsp = fs.promises;
const path = require('path');
const ui = require('./ui');
const actions = require('./actions');
const { WIN, run, GIT, gitEnv } = require('./exec');

const { C, fg, bg, line, textWidth, ellipsis, Field } = ui;
const ROOT = path.resolve(process.env.EXPLORER_ROOT || process.argv[2] || process.cwd());
const PANE = process.env.EXPLORER_PANE;
const SAVE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'search.json');
const MAX_FILES = 50000;      // walk without git
const MAX_SIZE = 2 << 20;     // larger files are not searched
const MAX_MATCHES = 5000;
const MAX_ROWS = 1000;        // Go to file results kept
const BATCH = 24;             // files read at once
const DEBOUNCE = 150;

const HI = '\x1b[1m' + fg([24, 163, 255]);   // fuzzy-matched characters
const FOUND = bg([120, 70, 20]) + fg([255, 255, 255]); // text matches
const DIM = fg(C.dim);

// --- state

const saved = (() => { try { return JSON.parse(fs.readFileSync(SAVE, 'utf8')) || {}; } catch { return {}; } })();
const field = value => Object.assign(new Field(value || ''), { restored: !!value });
const state = {
  mode: process.env.EXPLORER_MODE === 'content' ? 'content' : 'files',
  fields: { files: field(saved.files), content: field(saved.content), include: field(saved.include), exclude: field(saved.exclude) },
  focus: 'query', // content mode: query, include or exclude
  opts: { case: !!saved.case, word: !!saved.word, regex: !!saved.regex, hidden: false },
  files: null, lower: null, base: null, capped: false, filesGen: 0,
  // Go to file
  results: [], line: 0, prev: null,
  // Search in files
  groups: [], total: 0, searching: false, error: null, re: null, searchKey: null, touched: false,
  cursor: 0, top: 0, flash: null, lastClick: null,
};
if (process.env.EXPLORER_QUERY) state.fields[state.mode] = field(process.env.EXPLORER_QUERY);
const queryField = () => state.fields[state.mode];
const focused = () => (state.mode === 'files' || state.focus === 'query' ? queryField() : state.fields[state.focus]);

function save() {
  if (!SAVE) return;
  const f = state.fields, o = state.opts;
  const data = { files: f.files.value, content: f.content.value, include: f.include.value, exclude: f.exclude.value,
    case: o.case, word: o.word, regex: o.regex };
  try { fs.writeFileSync(SAVE, JSON.stringify(data)); } catch { }
}

// --- file list

// Tracked and untracked files not ignored, relative to ROOT (git lists only the folder it runs in, even
// below the top of the repository), minus those deleted from the working tree.
async function gitFiles() {
  if (!GIT) return null;
  const env = await gitEnv(ROOT);
  const [all, deleted] = await Promise.all([
    run(GIT, ['ls-files', '-z', '-co', '--exclude-standard'], { cwd: ROOT, env }),
    run(GIT, ['ls-files', '-z', '-d'], { cwd: ROOT, env }),
  ]);
  if (!all) return null;
  const gone = new Set((deleted || '').split('\0'));
  return [...new Set(all.split('\0'))].filter(f => f && !gone.has(f));
}

// Without git, or to include ignored and hidden files: the folder itself, without .git (nor node_modules and
// dot-folders unless hidden), symbolic links followed once (real paths remembered against loops), capped.
async function walk(hidden) {
  const files = [], seen = new Set(), queue = [''];
  while (queue.length && files.length < MAX_FILES) {
    await Promise.all(queue.splice(0, 16).map(async rel => {
      const abs = path.join(ROOT, rel);
      let entries;
      try {
        const real = await fsp.realpath(abs);
        if (seen.has(real)) return;
        seen.add(real);
        entries = await fsp.readdir(abs, { withFileTypes: true });
      } catch { return; }
      for (const e of entries) {
        const r = rel ? `${rel}/${e.name}` : e.name;
        let dir = e.isDirectory();
        if (e.isSymbolicLink()) try { dir = (await fsp.stat(path.join(abs, e.name))).isDirectory(); } catch { continue; }
        if (!dir) { if (files.length < MAX_FILES) files.push(r); continue; }
        if (e.name === '.git' || (!hidden && (e.name === 'node_modules' || e.name.startsWith('.')))) continue;
        queue.push(r);
      }
    }));
  }
  state.capped = files.length >= MAX_FILES;
  return files;
}

async function loadFiles() {
  const gen = ++state.filesGen;
  searchGen++; // a running search would go on with the old list
  state.files = null;
  state.capped = false;
  const hidden = state.opts.hidden;
  let files = hidden ? null : await gitFiles();
  if (!files || !files.length) files = await walk(hidden);
  if (gen !== state.filesGen) return;
  files.sort();
  state.files = files;
  state.lower = files.map(f => f.toLowerCase());
  state.base = files.map(f => f.lastIndexOf('/') + 1);
  state.prev = null;
  state.searchKey = null;
  refresh();
}

// --- fuzzy matching

// Subsequence match scored like fzf and VS Code: each matched character scores, more at the start of the
// path, of a segment or a word, and in the file name; consecutive runs score more, gaps cost. Dynamic
// programming over (query character, path position), keeping the best alignment.
const SCORE = 16, CONSEC = 12, GAP_OPEN = 3, GAP = 1, SEGMENT = 10, WORD = 7, CAMEL = 6, NAME = 4, CASE = 1;
const NEG = -1e8;
let cells = new Int32Array(1024), from = new Uint8Array(1024);

function bonus(s, j, base) {
  const p = j ? s[j - 1] : '/', c = s[j];
  let b = j >= base ? NAME : 0;
  if (p === '/' || p === '\\') b += SEGMENT;
  else if (!/[\p{L}\p{N}]/u.test(p)) b += WORD;
  else if (p === p.toLowerCase() && c !== c.toLowerCase()) b += CAMEL;
  return b;
}

// Score of the query q (ql in lower case) in the path s (sl), or null; with positions: also the matched indexes.
function fuzzy(q, ql, s, sl, base, positions) {
  const m = ql.length, n = sl.length;
  let i = 0;
  for (let j = 0; j < n && i < m; j++) if (sl.charCodeAt(j) === ql.charCodeAt(i)) i++;
  if (i < m) return null;
  if (m * n > cells.length) { cells = new Int32Array(m * n * 2); from = new Uint8Array(m * n * 2); }
  for (i = 0; i < m; i++) {
    const row = i * n, up = row - n, qc = ql.charCodeAt(i);
    let gap = NEG; // best previous-row cell at least two columns back, minus the gap cost
    for (let j = 0; j < n; j++) {
      if (i && j >= 2) gap = Math.max(gap - GAP, cells[up + j - 2] - GAP_OPEN);
      let v = NEG, f = 0;
      if (sl.charCodeAt(j) === qc) {
        const b = SCORE + bonus(s, j, base) + (s[j] === q[i] ? CASE : 0);
        if (!i) v = b;
        else {
          const diag = j ? cells[up + j - 1] : NEG;
          if (diag > NEG / 2 && diag + CONSEC >= gap) { v = diag + CONSEC + b; f = 1; } else if (gap > NEG / 2) { v = gap + b; f = 2; }
        }
      }
      cells[row + j] = v; from[row + j] = f;
    }
  }
  let best = NEG, end = -1;
  for (let j = 0, row = (m - 1) * n; j < n; j++) if (cells[row + j] > best) { best = cells[row + j]; end = j; }
  if (best <= NEG / 2) return null;
  if (!positions) return { score: best };
  const pos = [];
  for (i = m - 1, pos.push(end); i > 0; i--, pos.push(end)) {
    const up = (i - 1) * n;
    if (from[i * n + end] === 1) { end--; continue; }
    let k = -1, kv = NEG;
    for (let c = 0; c <= end - 2; c++) {
      const v = cells[up + c] - GAP_OPEN - (end - c - 2) * GAP;
      if (v > kv) { kv = v; k = c; }
    }
    end = k;
  }
  return { score: best, pos: pos.reverse() };
}

// `path:line` (or path:line:column) jumps to a line.
function parseQuery(text) {
  const m = /^(.*?):(\d+)(?::\d+)?$/.exec(text.trim());
  const q = (m ? m[1] : text).replace(/\s+/g, '').replace(/\\/g, '/');
  return { q, line: m ? +m[2] : 0 };
}

// Scores every path, or only the previous matches when the query grows (a match of the longer query
// matches the shorter one too); keeps the best MAX_ROWS.
function filterFiles() {
  const { files, lower, base } = state;
  const { q, line } = parseQuery(state.fields.files.value);
  state.line = line;
  if (!files) { state.results = []; return; }
  const ql = q.toLowerCase();
  if (!q) {
    state.results = files.slice(0, MAX_ROWS).map((_, i) => ({ i, score: 0 }));
    state.prev = null;
    return;
  }
  const prev = state.prev && ql.startsWith(state.prev.ql) ? state.prev.idx : null;
  const idx = [], scored = [];
  const each = i => {
    const r = fuzzy(q, ql, files[i], lower[i], base[i], false);
    if (r) { idx.push(i); scored.push({ i, score: r.score }); }
  };
  if (prev) for (const i of prev) each(i); else for (let i = 0; i < files.length; i++) each(i);
  scored.sort((a, b) => b.score - a.score || files[a.i].length - files[b.i].length || (files[a.i] < files[b.i] ? -1 : 1));
  state.prev = { ql, idx };
  state.results = scored.slice(0, MAX_ROWS);
}

// --- search in files

// Glob to RegExp on '/' paths: * and ? within a segment, ** across segments, {a,b}, [abc]. A pattern
// without a leading ./ or / matches at any depth, and a folder matches its contents (VS Code behaviour).
function globSource(glob) {
  let g = glob.trim().replace(/\\/g, '/');
  g = /^\.?\//.test(g) ? g.replace(/^\.?\//, '') : g.startsWith('**/') ? g : '**/' + g;
  g = g.replace(/\/+$/, '');
  let s = '', depth = 0;
  for (let i = 0; i < g.length; i++) {
    const c = g[i];
    if (c === '*' && g[i + 1] === '*') {
      i++;
      if (g[i + 1] === '/') { i++; s += '(?:.*/)?'; } else s += '.*';
    } else if (c === '*') s += '[^/]*';
    else if (c === '?') s += '[^/]';
    else if (c === '{') { depth++; s += '(?:'; } else if (c === '}' && depth) { depth--; s += ')'; } else if (c === ',' && depth) s += '|';
    else if (c === '[') {
      const end = g.indexOf(']', i + 2);
      if (end < 0) { s += '\\['; continue; }
      s += '[' + g.slice(i + 1, end).replace(/^!/, '^').replace(/\\/g, '\\\\') + ']';
      i = end;
    } else s += c.replace(/[.+^$()|\\]/g, '\\$&');
  }
  return s + ')'.repeat(depth) + '(?:/.*)?';
}

// Comma-separated globs (commas inside braces kept) as one RegExp, or null when empty.
function globs(text) {
  const parts = [];
  let cur = '', depth = 0;
  for (const c of text) {
    if (c === ',' && !depth) { parts.push(cur); cur = ''; continue; }
    if (c === '{') depth++; else if (c === '}') depth = Math.max(0, depth - 1);
    cur += c;
  }
  parts.push(cur);
  const sources = parts.filter(p => p.trim()).map(globSource);
  if (!sources.length) return null;
  try { return new RegExp(`^(?:${sources.join('|')})$`, WIN ? 'i' : ''); } catch { return null; }
}

function matcher() {
  const { case: cs, word, regex } = state.opts;
  let src = state.fields.content.value;
  if (!regex) src = src.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  if (word) src = `\\b(?:${src})\\b`;
  return new RegExp(src, 'gm' + (cs ? '' : 'i'));
}

// Text of a regular file, or [null, reason] when not searchable: too large or binary (NUL in the first 8 kB).
// Stat first: opening a FIFO would block.
async function readText(abs) {
  try {
    const st = await fsp.stat(abs);
    if (!st.isFile()) return [null, 'not a file'];
    if (st.size > MAX_SIZE) return [null, 'file too large'];
    const buf = await fsp.readFile(abs);
    if (buf.subarray(0, 8192).includes(0)) return [null, 'binary file'];
    return [buf.toString('utf8')];
  } catch { return [null, 'cannot read']; }
}

// Non-empty match ranges of re in a text, at most max.
function ranges(re, text, max = 50) {
  const found = [];
  re.lastIndex = 0;
  for (let m; found.length < max && (m = re.exec(text));) {
    if (!m[0].length) { re.lastIndex++; continue; }
    found.push([m.index, m.index + m[0].length]);
  }
  return found;
}

async function grep(rel, re, room) {
  const [text] = await readText(path.join(ROOT, rel));
  if (!text) return null;
  re.lastIndex = 0;
  if (!re.test(text)) return null; // most files: no split needed
  const matches = [];
  let count = 0;
  const lines = text.split('\n');
  for (let n = 0; n < lines.length && count < room; n++) {
    let l = lines[n];
    if (l.endsWith('\r')) l = l.slice(0, -1);
    const r = ranges(re, l, room - count);
    if (!r.length) continue;
    count += r.length;
    // Long lines (minified files) kept around the first match only.
    const start = l.length > 1000 ? Math.max(0, r[0][0] - 100) : 0;
    matches.push({ n: n + 1, text: l.slice(start, start + 1000), cut: start > 0,
      ranges: r.map(([a, b]) => [a - start, b - start]).filter(([a]) => a < 1000) });
  }
  return matches.length ? { rel, matches, count, collapsed: false } : null;
}

// Restarted on every change; a newer search makes the running one stop at its next batch.
let searchGen = 0, debounce = null;
function scheduleSearch() {
  clearTimeout(debounce);
  state.searching = !!state.fields.content.value;
  debounce = setTimeout(search, DEBOUNCE);
}

async function search() {
  clearTimeout(debounce);
  const gen = ++searchGen;
  const f = state.fields;
  state.searchKey = JSON.stringify([f.content.value, f.include.value, f.exclude.value, state.opts, state.filesGen]);
  Object.assign(state, { groups: [], total: 0, error: null, re: null, cursor: 0, top: 0, touched: false });
  if (!f.content.value) { state.searching = false; return render(); }
  try { state.re = matcher(); } catch (e) {
    state.searching = false;
    state.error = 'Invalid regex: ' + String(e.message).replace(/^Invalid regular expression: \/.*\/[a-z]*: /s, '');
    return render();
  }
  state.searching = true;
  render();
  if (!state.files) return; // loadFiles starts it again
  const inc = globs(f.include.value), exc = globs(f.exclude.value);
  const todo = state.files.filter(rel => (!inc || inc.test(rel)) && !(exc && exc.test(rel)));
  const re = state.re;
  for (let k = 0; k < todo.length && state.total < MAX_MATCHES; k += BATCH) {
    // Each file gets its own regex copy: lastIndex is shared state.
    const found = await Promise.all(todo.slice(k, k + BATCH).map(rel => grep(rel, new RegExp(re), MAX_MATCHES)));
    if (gen !== searchGen) return;
    for (const g of found) {
      if (!g || state.total >= MAX_MATCHES) continue;
      if (state.total + g.count > MAX_MATCHES) {
        let left = MAX_MATCHES - state.total, keep = 0, count = 0;
        for (; keep < g.matches.length && left > 0; keep++) { const c = Math.min(left, g.matches[keep].ranges.length); count += c; left -= c; }
        g.matches = g.matches.slice(0, keep); g.count = count;
      }
      state.groups.push(g);
      state.total += g.count;
    }
    paintSoon();
  }
  if (gen !== searchGen) return;
  state.searching = false;
  render();
}

// --- rows

// Go to file: one row per file. Search in files: a header row per file, then its matching lines.
function rows() {
  if (state.mode === 'files') return state.results;
  const out = [];
  for (const g of state.groups) {
    out.push({ g });
    if (!g.collapsed) for (const m of g.matches) out.push({ g, m });
  }
  return out;
}

// File and line of a row.
function target(r) {
  if (!r) return null;
  if (state.mode === 'files') return { rel: state.files[r.i], line: state.line };
  return { rel: r.g.rel, line: (r.m || r.g.matches[0]).n };
}

// Segments of a text with ranges highlighted, tabs expanded and control characters dropped.
function marked(text, found, style, hi, tab = 4) {
  const out = [];
  let cur = '', on = false, col = 0, r = 0;
  for (let i = 0; i < text.length; i++) {
    while (r < found.length && i >= found[r][1]) r++;
    const inside = r < found.length && i >= found[r][0];
    if (inside !== on) { if (cur) out.push([cur, on ? hi : style]); cur = ''; on = inside; }
    const ch = text[i];
    if (ch === '\t') { const n = tab - (col % tab); cur += ' '.repeat(n); col += n; } else if (!/[\x00-\x1f\x7f-\x9f]/.test(ch)) { cur += ch; col++; }
  }
  if (cur) out.push([cur, on ? hi : style]);
  return out;
}

function fileRow(r, W) {
  const rel = state.files[r.i];
  if (state.fields.files.value && !r.pos) {
    const { q } = parseQuery(state.fields.files.value);
    r.pos = q ? (fuzzy(q, q.toLowerCase(), rel, state.lower[r.i], state.base[r.i], true) || {}).pos || [] : [];
  }
  const pos = new Set(r.pos || []);
  const b = state.base[r.i];
  const part = (from, to, style) => {
    const segs = [];
    for (let i = from; i < to; i++) {
      const st = pos.has(i) ? HI : style, last = segs[segs.length - 1];
      if (last && last[1] === st) last[0] += rel[i]; else segs.push([rel[i], st]);
    }
    return segs;
  };
  return [[' ', ''], ...part(b, rel.length, ''), ['  ', ''], ...part(0, Math.max(0, b - 1), DIM)];
}

function contentRow(r, W) {
  const { g, m } = r;
  if (!m) {
    const b = g.rel.lastIndexOf('/') + 1;
    return [[g.collapsed ? ' ▸ ' : ' ▾ ', DIM], [g.rel.slice(b), '\x1b[1m'], ['  ' + g.rel.slice(0, Math.max(0, b - 1)), DIM],
      [`  ${g.count}`, fg(C.blue)]];
  }
  const num = `   ${String(m.n).padStart(5)}  `;
  // Leading blanks removed, then the start cut so that the first match shows within the room left.
  let text = m.text, found = m.ranges, cut = m.cut;
  const lead = text.length - text.trimStart().length;
  text = text.slice(lead); found = found.map(([a, b]) => [a - lead, b - lead]);
  const room = W - num.length - 1;
  if (found.length && textWidth(text.slice(0, found[0][1])) > room - 8) {
    let at = found[0][0] - Math.floor(room * 0.3);
    if (/[\udc00-\udfff]/.test(text[at])) at++;
    text = text.slice(at); found = found.map(([a, b]) => [a - at, b - at]); cut = true;
  }
  return [[num, DIM], ...(cut ? [['…', DIM]] : []), ...marked(text, found, '', FOUND, 1)];
}

// --- preview

const previews = new Map(); // rel → { lines } or { error }
function preview(rel) {
  if (previews.has(rel)) return previews.get(rel);
  previews.set(rel, null);
  readText(path.join(ROOT, rel)).then(([text, error]) => {
    if (previews.size > 30) previews.delete(previews.keys().next().value);
    previews.set(rel, text === null || text === undefined ? { error } : { lines: text.replace(/\r?\n$/, '').split('\n').map(l => l.replace(/\r$/, '')) });
    paintSoon();
  });
  return null;
}

function previewLines(t, W, H) {
  const out = [];
  if (!t) return out;
  out.push(line([[' ' + t.rel + (t.line ? `:${t.line}` : ''), '\x1b[1m']], W, C.header));
  const p = preview(t.rel);
  if (!p) return out;
  if (p.error) return out.concat(line([[` (${p.error})`, DIM]], W));
  const h = H - 1, at = Math.max(0, (t.line || 1) - 1);
  const start = t.line ? Math.max(0, Math.min(at - Math.floor(h / 3), p.lines.length - h)) : 0;
  const digits = String(Math.min(p.lines.length, start + h)).length;
  for (let n = start; n < Math.min(p.lines.length, start + h); n++) {
    const l = p.lines[n], current = t.line && n === at;
    const found = state.mode === 'content' && state.re ? ranges(state.re, l) : [];
    out.push(line([[` ${String(n + 1).padStart(digits)} `, current ? fg(C.text) : DIM], ...marked(l, found, '', FOUND)], W,
      current ? C.selectBlur : null));
  }
  return out;
}

// --- rendering

let zones = []; // clickable areas of the last frame: { y, x1, x2, fn }
let paint, paintTimer = null;

// A screen line whose parts ([text, style, onClick]) are clickable.
function bar(y, parts, W, back) {
  let x = 1;
  for (const [text, , fn] of parts) {
    const w = textWidth(text);
    if (fn && w) zones.push({ y, x1: x, x2: x + w - 1, fn });
    x += w;
  }
  return line(parts, W, back);
}
const width = parts => parts.reduce((w, [t]) => w + textWidth(t), 0);
const chip = (label, on, fn) => [` ${label} `, on ? bg(C.select) + fg([255, 255, 255]) : DIM, fn];

// The visible part of a field, the cursor shown only in the focused one; restored text shown selected:
// typing replaces it.
function fieldParts(f, w, focus) {
  const style = f.restored ? bg(C.select) : '';
  if (!focus) return [[ellipsis(f.value, w), f.restored ? '' : fg(C.text)]];
  return f.segments(w, style);
}

function listHeight() {
  const { H } = ui.size();
  return Math.max(1, H - (state.mode === 'content' ? 5 : 4));
}

function render() {
  clearTimeout(paintTimer); paintTimer = null;
  const { W, H } = ui.size();
  zones = [];
  const lines = [];
  const content = state.mode === 'content';

  // Tabs, and the space folder on the right.
  const tab = (label, mode) => [` ${label} `, state.mode === mode ? '\x1b[1m' + bg(C.select) + fg([255, 255, 255]) : DIM, () => setMode(mode)];
  const tabs = [[' ', ''], tab('Go to file  ctrl+p', 'files'), [' ', ''], tab('Search in files  ctrl+f', 'content')];
  const name = ellipsis(path.basename(ROOT), Math.max(0, W - width(tabs) - 2));
  lines.push(bar(0, [...tabs, [' '.repeat(Math.max(0, W - width(tabs) - textWidth(name) - 1)), ''], [name, DIM]], W, C.header));

  // Query and option chips.
  const toggle = k => () => toggleOpt(k);
  const chips = content
    ? [chip('Aa', state.opts.case, toggle('case')), chip('ab', state.opts.word, toggle('word')), chip('.*', state.opts.regex, toggle('regex')), [' ', '']]
    : [];
  chips.push(chip('hidden', state.opts.hidden, toggle('hidden')), [' ', '']);
  const prompt = [' ❯ ', fg(C.blue), () => focus('query')];
  const qw = Math.max(1, W - 3 - width(chips) - 1);
  const qparts = fieldParts(queryField(), qw, !content || state.focus === 'query').map(p => [...p, () => focus('query')]);
  lines.push(bar(1, [prompt, ...qparts, [' '.repeat(Math.max(0, W - 3 - width(qparts) - width(chips))), '', () => focus('query')], ...chips], W));

  // Include and exclude globs.
  if (content) {
    const half = Math.floor(W / 2);
    const globField = (label, k, w) => {
      const l = [label, DIM, () => focus(k)];
      const parts = fieldParts(state.fields[k], Math.max(1, w - textWidth(label) - 1), state.focus === k).map(p => [...p, () => focus(k)]);
      return [l, ...parts, [' '.repeat(Math.max(0, w - textWidth(label) - width(parts))), '', () => focus(k)]];
    };
    lines.push(bar(2, [...globField('   include ', 'include', half), ...globField(' exclude ', 'exclude', W - half)], W));
  }

  // Status.
  let status, style = DIM;
  if (state.flash) { status = state.flash; style = fg(C.yellow); } else if (!state.files) status = 'indexing…';
  else if (content && state.error) { status = state.error; style = fg(C.red); } else if (content) {
    const n = state.total, m = state.groups.length;
    status = state.fields.content.value ? `${n}${n >= MAX_MATCHES ? '+' : ''} result${n === 1 ? '' : 's'} in ${m} file${m === 1 ? '' : 's'}` : 'type to search';
    if (state.searching) status += '  searching…';
  } else {
    status = `${state.fields.files.value ? `${state.results.length}${state.results.length >= MAX_ROWS ? '+' : ''} of ` : ''}${state.files.length} files`;
  }
  if (state.capped && !state.flash) status += `  (first ${MAX_FILES} files only)`;
  lines.push(line([[' ' + status, style]], W));

  // Results, and the preview on the right.
  const list = rows();
  const h = listHeight();
  if (content && !state.touched && list.length > 1 && state.cursor === 0) state.cursor = 1;
  state.cursor = Math.max(0, Math.min(state.cursor, list.length - 1));
  state.top = Math.max(0, Math.min(state.top, list.length - h));
  const LW = W >= 100 ? Math.floor(W / 2) : W;
  const right = LW < W ? previewLines(target(list[state.cursor]), W - LW - 1, h) : [];
  for (let k = 0; k < h; k++) {
    const i = state.top + k, r = list[i];
    let l = r ? line(content ? contentRow(r, LW) : fileRow(r, LW), LW, i === state.cursor ? C.select : null) : line([], LW);
    if (!r && !k && state.files && !list.length) l = line([[content && !state.fields.content.value ? '' : '  no results', DIM]], LW);
    if (LW < W) l += DIM + '│\x1b[0m' + (right[k] || line([], W - LW - 1));
    lines.push(l);
  }

  // Keys.
  const keys = '↵ open  alt+↵ editor  ctrl+t insert  ctrl+y copy path' + (content ? '  tab fields  alt+←→ fold' : '') + '  esc close';
  lines.push(line([[' ' + keys, DIM]], W, C.header));
  paint(lines.slice(0, H));
}

function paintSoon() {
  if (!paintTimer) paintTimer = setTimeout(render, 50);
}

function flash(text) {
  if (!text) return;
  state.flash = text;
  render();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { state.flash = null; render(); }, 2500);
}

// --- actions

// Results recomputed for the current mode (a search only when its inputs changed).
function refresh() {
  if (state.mode === 'files') { filterFiles(); return render(); }
  const f = state.fields;
  const key = JSON.stringify([f.content.value, f.include.value, f.exclude.value, state.opts, state.filesGen]);
  if (key !== state.searchKey || !state.files) search(); else render();
}

function setMode(mode) {
  if (state.mode === mode) return;
  state.mode = mode;
  state.focus = 'query';
  state.cursor = 0; state.top = 0;
  refresh();
}

function focus(k) {
  if (state.mode === 'content') state.focus = k;
  render();
}

function toggleOpt(k) {
  state.opts[k] = !state.opts[k];
  if (k === 'hidden') { loadFiles(); return render(); }
  if (state.mode === 'content') search(); else render();
}

function select(i) {
  const list = rows();
  state.cursor = Math.max(0, Math.min(i, list.length - 1));
  state.touched = true;
  const h = listHeight();
  if (state.cursor < state.top) state.top = state.cursor;
  if (state.cursor >= state.top + h) state.top = state.cursor - h + 1;
}

// The preview pane and the insertion close the popup on success (null); otherwise their message shows.
async function act(kind) {
  const t = target(rows()[state.cursor]);
  if (!t) return;
  const abs = path.join(ROOT, t.rel);
  let msg;
  try {
    if (kind === 'open') msg = await actions.preview(abs, ROOT, PANE, t.line || undefined);
    else if (kind === 'insert') msg = await actions.insert(abs, PANE);
    else if (kind === 'editor') msg = await actions.editor(abs, PANE);
    else if (kind === 'copy') return flash(await actions.copy(t.rel));
  } catch { msg = 'Action failed'; }
  if (!msg) process.exit(0);
  flash(msg);
}

function fold(collapsed) {
  if (state.mode !== 'content') return;
  const r = rows()[state.cursor];
  if (!r) return;
  r.g.collapsed = collapsed;
  select(rows().findIndex(x => x.g === r.g && !x.m));
}

// --- input

function key(k) {
  const page = listHeight() - 1;
  switch (k) {
    case 'escape': case 'ctrl+c': return process.exit(0);
    case 'ctrl+p': return setMode('files');
    case 'ctrl+f': return setMode('content');
    case 'alt+c': return toggleOpt('case');
    case 'alt+w': return toggleOpt('word');
    case 'alt+r': return toggleOpt('regex');
    case 'alt+h': return toggleOpt('hidden');
    case 'tab': case 'shift+tab': {
      if (state.mode !== 'content') return;
      const order = ['query', 'include', 'exclude'];
      return focus(order[(order.indexOf(state.focus) + (k === 'tab' ? 1 : 2)) % 3]);
    }
    case 'up': case 'ctrl+k': select(state.cursor - 1); break;
    case 'down': case 'ctrl+j': case 'ctrl+n': select(state.cursor + 1); break;
    case 'pageup': select(state.cursor - page); break;
    case 'pagedown': select(state.cursor + page); break;
    case 'ctrl+home': select(0); break;
    case 'ctrl+end': select(rows().length - 1); break;
    case 'alt+left': fold(true); break;
    case 'alt+right': fold(false); break;
    case 'enter': return act('open');
    case 'alt+enter': case 'ctrl+o': return act('editor');
    case 'ctrl+t': return act('insert');
    case 'ctrl+y': return act('copy');
    default: {
      const f = focused(), before = f.value;
      if (f.restored && (ui.printable(k) || k === 'backspace' || k === 'delete')) {
        f.set('');
        if (ui.printable(k)) f.key(k);
      } else if (!f.key(k)) return;
      f.restored = false;
      if (f.value !== before) return edited();
    }
  }
  render();
}

function edited() {
  state.cursor = 0; state.top = 0; state.touched = false;
  if (state.mode === 'files') return refresh();
  scheduleSearch();
  render();
}

function paste(text) {
  const f = focused();
  if (f.restored) f.set('');
  f.restored = false;
  f.insert(text);
  edited();
}

function mouse({ button, x, y, press, wheel, drag }) {
  if (wheel) {
    state.top = Math.max(0, Math.min(state.top + wheel * 3, rows().length - listHeight()));
    return render();
  }
  if (!press || drag || button !== 0) return;
  const zone = zones.find(z => z.y === y - 1 && x >= z.x1 && x <= z.x2);
  if (zone) return zone.fn();
  const first = state.mode === 'content' ? 4 : 3; // screen rows above the list
  const i = state.top + y - 1 - first;
  const r = rows()[i];
  if (y - 1 < first || !r || y > ui.size().H - 1) return;
  const now = Date.now(), double = state.lastClick && state.lastClick.i === i && now - state.lastClick.t < 400;
  state.lastClick = double ? null : { i, t: now };
  select(i);
  if (double) return act('open');
  if (state.mode === 'content' && !r.m) fold(!r.g.collapsed);
  render();
}

// --- startup

paint = ui.screen({ key, mouse, paste, resize: render });
process.on('exit', save);
render();
loadFiles();
