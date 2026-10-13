// Git changes of a file or folder, in a temporary full-tab pane: tracked files compared with HEAD (index and
// working tree), untracked files in full; or only the staged changes (EXPLORER_STAGED=1), or a commit
// (EXPLORER_COMMIT=<hash>). Inline, or side by side from 120 columns; s switches, n and N jump between hunks,
// arrows, Page keys or the wheel scroll, q or Esc closes the pane.
// Required as a module (by scm.js), it only exports the parser and the renderer.
'use strict';
const path = require('path');
const { spawnSync } = require('child_process');
const ui = require('./ui');
const { GIT, gitEnv } = require('./exec');

const MAX_UNTRACKED = 200;
const MAX_ROWS = 200000; // beyond, a huge diff would take seconds to lay out at every resize
const SPLIT_MIN = 120;   // columns from which side by side is the default

// Colours of the rows: tinted backgrounds, stronger on the changed words.
const TINT = { add: ui.C.added, del: ui.C.removed, addWord: [46, 105, 52], delWord: [125, 42, 42], gap: [30, 30, 30] };
// Options of every git diff, show or stash show: no colours, external tools or textconv filters (from the
// repository config, they could run commands), and the a/ b/ prefixes whatever diff.noprefix or
// diff.mnemonicPrefix say, so that the parser finds the paths.
const DIFF_OPTS = ['--no-color', '--no-ext-diff', '--no-textconv', '--ignore-submodules=all', '--src-prefix=a/', '--dst-prefix=b/'];

// --- parsing

// A path as git quotes it when it contains special characters: "a/tab\there" with octal escapes for bytes.
function unquote(p) {
  if (!p.startsWith('"') || !p.endsWith('"') || p.length < 2) return p;
  const bytes = [], esc = { a: 7, b: 8, t: 9, n: 10, v: 11, f: 12, r: 13 };
  for (let i = 1; i < p.length - 1; i++) {
    if (p[i] !== '\\') { bytes.push(...Buffer.from(p[i])); continue; }
    const n = p[++i];
    if (/[0-7]/.test(n)) { bytes.push(parseInt(p.substr(i, 3), 8)); i += 2; } else bytes.push(esc[n] || n.charCodeAt(0));
  }
  return Buffer.from(bytes).toString('utf8');
}
const strip = p => (p = unquote(p.replace(/\t$/, ''))) === '/dev/null' ? null : p.replace(/^[ab]\//, '');

// Parses the output of git diff, show or stash show. Returns { pre, files, add, del }: pre, the lines before
// the first file (commit header and stat); each file { from, to, status, notes, hunks, add, del }, each hunk
// { head, lines: [[sign, text, old number, new number]] }.
function parse(text) {
  const d = { pre: [], files: [], add: 0, del: 0 };
  let f = null, h = null, oldLeft = 0, newLeft = 0, o = 0, n = 0;
  for (const raw of text.split('\n')) {
    const l = raw.replace(/\r$/, '');
    if (h && (oldLeft > 0 || newLeft > 0 || l.startsWith('\\')) && /^[ +\-\\]/.test(l)) {
      const s = l[0];
      if (s === '\\') { h.lines.push([s, l.slice(1).trim(), null, null]); continue; }
      h.lines.push([s, l.slice(1), s === '+' ? null : o, s === '-' ? null : n]);
      if (s !== '+') { o++; oldLeft--; }
      if (s !== '-') { n++; newLeft--; }
      if (s === '+') { f.add++; d.add++; } else if (s === '-') { f.del++; d.del++; }
      continue;
    }
    h = null;
    if (l.startsWith('diff ')) {
      // diff --git a/x b/x: the path, when the same on both sides (otherwise from --- +++ or rename lines).
      const rest = l.replace(/^diff --\S+ /, ''), half = (rest.length - 1) / 2;
      const same = rest.startsWith('a/') && rest.slice(2, half) === rest.slice(half + 3) ? rest.slice(2, half) : null;
      f = { from: same, to: same, status: 'modified', notes: [], hunks: [], add: 0, del: 0 };
      d.files.push(f);
      continue;
    }
    if (!f) { d.pre.push(l); continue; }
    const m = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/.exec(l);
    if (m) {
      o = +m[1]; n = +m[3];
      oldLeft = m[2] === undefined ? 1 : +m[2];
      newLeft = m[4] === undefined ? 1 : +m[4];
      h = { head: l, context: m[5], lines: [] };
      f.hunks.push(h);
    } else if (l.startsWith('--- ')) f.from = strip(l.slice(4));
    else if (l.startsWith('+++ ')) f.to = strip(l.slice(4));
    else if (l.startsWith('new file mode')) f.status = 'added';
    else if (l.startsWith('deleted file mode')) f.status = 'deleted';
    else if (l.startsWith('rename from ')) { f.from = unquote(l.slice(12)); f.status = 'renamed'; }
    else if (l.startsWith('rename to ')) f.to = unquote(l.slice(10));
    else if (l.startsWith('copy from ')) { f.from = unquote(l.slice(10)); f.status = 'copied'; }
    else if (l.startsWith('copy to ')) f.to = unquote(l.slice(8));
    else if (l.startsWith('Binary files')) f.notes.push('Binary file');
    else if (l.startsWith('old mode ')) f.notes.push(`mode ${l.slice(9)}`);
    else if (l.startsWith('new mode ')) f.notes[f.notes.length - 1] += ` → ${l.slice(9)}`;
    else if (l.trim() && !/^(index |similarity |dissimilarity |new file|deleted file)/.test(l)) d.pre.push(l);
  }
  for (const file of d.files) {
    if (file.status === 'added') file.from = null;
    if (file.status === 'deleted') file.to = null;
  }
  return d;
}

// --- rendering

// Changed words of a removed line and its added counterpart: spans [text, changed] for each, or null when the
// lines have little in common (the whole row is then the change).
function words(a, b) {
  const split = s => s.match(/[\p{L}\p{N}_]+|\s+|[^\p{L}\p{N}_\s]/gu) || [];
  const ta = split(a), tb = split(b);
  if (!ta.length || !tb.length || ta.length * tb.length > 40000) return null;
  const dp = Array.from({ length: ta.length + 1 }, () => new Uint16Array(tb.length + 1));
  for (let i = ta.length - 1; i >= 0; i--) {
    for (let j = tb.length - 1; j >= 0; j--) dp[i][j] = ta[i] === tb[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  }
  const sa = [], sb = [];
  const push = (spans, text, changed) => {
    const last = spans[spans.length - 1];
    if (last && last[1] === changed) last[0] += text; else spans.push([text, changed]);
  };
  let i = 0, j = 0, same = 0;
  while (i < ta.length || j < tb.length) {
    if (i < ta.length && j < tb.length && ta[i] === tb[j]) { push(sa, ta[i], false); push(sb, tb[j], false); same += ta[i].trim().length; i++; j++; }
    else if (j >= tb.length || (i < ta.length && dp[i + 1][j] >= dp[i][j + 1])) push(sa, ta[i++], true);
    else push(sb, tb[j++], true);
  }
  return same < 0.4 * Math.max(a.trim().length, b.trim().length) ? null : [sa, sb];
}

// Segments filling exactly width columns: spans [text, changed] on the base background, changed parts on the
// strong one.
function cell(spans, width, style, base, strong) {
  const segs = [];
  let used = 0;
  for (const [text, changed] of spans) {
    const [part, w] = ui.fit(text, width - used);
    if (part) segs.push([part, style + (changed && strong ? ui.bg(strong) : base ? ui.bg(base) : '')]);
    used += w;
    if (used >= width) break;
  }
  if (used < width) segs.push([' '.repeat(width - used), base ? ui.bg(base) : '']);
  return segs;
}

const num = (v, w) => (v === null || v === undefined ? '' : String(v)).padStart(w);
const DIM = ui.fg(ui.C.dim), GREEN = ui.fg(ui.C.green), RED = ui.fg(ui.GIT.D), CYAN = ui.fg(ui.C.blue);

// Header lines of a commit (show) and its stat, coloured here.
function preRow(l) {
  if (/^commit /.test(l)) return { segs: [[l, ui.fg(ui.C.yellow)]] };
  if (/^(Author|AuthorDate|Commit|CommitDate|Date|Merge):/.test(l)) return { segs: [[l, DIM]] };
  const stat = /^( .*\| +\d+ )(\+*)(-*)$/.exec(l);
  if (stat) return { segs: [[stat[1], ''], [stat[2], GREEN], [stat[3], RED]] };
  return { segs: [[l, '']] };
}

function fileRow(f, width) {
  const name = f.status === 'renamed' || f.status === 'copied' ? `${f.from} → ${f.to}` : f.to || f.from || '?';
  const status = f.status === 'modified' ? '' : `  ${f.status}`;
  const counts = ` +${f.add} −${f.del} `;
  const label = ' ' + ui.ellipsis(ui.cleanLine(name), Math.max(1, width - counts.length - status.length - 2));
  const pad = ' '.repeat(Math.max(0, width - ui.textWidth(label) - status.length - counts.length));
  return { segs: [[label, '\x1b[1m'], [status, DIM], [pad, ''], [` +${f.add}`, GREEN], [` −${f.del} `, RED]],
    back: ui.C.header, file: true };
}

// Rows of a parsed diff for a width: { segs, back } for ui.line(segs, width, back), hunk: true on the rows
// n and N jump to. mode: 'inline', or 'split' (old version on the left, new on the right).
function render(d, width, mode = 'inline') {
  const rows = [];
  for (const l of d.pre) rows.push(preRow(ui.cleanLine(l)));
  for (const f of d.files) {
    if (rows.length) rows.push({ segs: [] });
    rows.push({ ...fileRow(f, width), hunk: !f.hunks.length });
    for (const note of f.notes) rows.push({ segs: [[`  ${note}`, DIM]] });
    if (!f.hunks.length && !f.notes.length && f.status !== 'renamed') rows.push({ segs: [['  (no content change)', DIM]] });
    let last = 1;
    for (const h of f.hunks) for (const [, , a, b] of h.lines) last = Math.max(last, a || 0, b || 0);
    const nw = String(last).length;
    for (const h of f.hunks) {
      rows.push({ segs: [[' ' + ui.cleanLine(h.head), CYAN]], back: [30, 36, 44], hunk: true });
      for (const block of blocks(h.lines)) {
        if (rows.length > MAX_ROWS) { rows.push({ segs: [[' … diff too long, truncated', DIM]] }); return rows; }
        if (mode === 'split') splitRows(rows, block, width, nw); else inlineRows(rows, block, width, nw);
      }
    }
  }
  return rows;
}

// Lines of a hunk grouped: unchanged lines, notes ("No newline at end of file"), and changes (removed lines
// followed by added ones, paired for the word highlight and the side-by-side rows).
function blocks(lines) {
  const out = [];
  for (const l of lines) {
    const [s] = l;
    const last = out[out.length - 1];
    if (s === '-' || s === '+') {
      const chg = last && last.dels && !(s === '-' && last.adds.length) ? last : (out.push({ dels: [], adds: [] }), out[out.length - 1]);
      (s === '-' ? chg.dels : chg.adds).push(l);
    } else out.push({ line: l });
  }
  for (const b of out) {
    if (!b.dels) continue;
    b.words = b.dels.map((l, i) => b.adds[i] && words(ui.cleanLine(l[1]), ui.cleanLine(b.adds[i][1])));
  }
  return out;
}

function inlineRows(rows, b, width, nw) {
  const row = ([s, text, a, n], spans, back, strong, sign) => {
    const gutter = s === '\\' ? ' '.repeat(nw * 2 + 2) : `${num(a, nw)} ${num(n, nw)} `;
    const style = s === '\\' ? DIM : '';
    const rest = Math.max(0, width - nw * 2 - 4);
    rows.push({ segs: [[gutter, DIM], [s === '\\' ? ' ' : s, sign], [' ', ''],
      ...cell(spans || [[ui.cleanLine(text), false]], rest, style, back, strong)], back });
  };
  if (b.line) return row(b.line, null, null, null, DIM);
  b.dels.forEach((l, i) => row(l, b.words[i] && b.words[i][0], TINT.del, TINT.delWord, RED));
  b.adds.forEach((l, i) => row(l, b.words[i] && b.words[i][1], TINT.add, TINT.addWord, GREEN));
}

function splitRows(rows, b, width, nw) {
  const half = Math.floor((width - 1) / 2), right = width - 1 - half;
  const side = (l, w, spans, base, strong, isNew) => {
    if (!l) return [[' '.repeat(w), ui.bg(TINT.gap)]];
    const [s, text, a, n] = l;
    const no = s === '+' || (s === ' ' && isNew) ? n : a;
    return [[`${num(s === '\\' ? null : no, nw)} `, DIM + (base ? ui.bg(base) : '')],
      ...cell(spans || [[ui.cleanLine(text), false]], Math.max(0, w - nw - 1), s === '\\' ? DIM : '', base, strong)];
  };
  const bar = ['│', DIM];
  if (b.line) {
    rows.push({ segs: [...side(b.line, half, null, null, null, false), bar, ...side(b.line, right, null, null, null, true)] });
    return;
  }
  for (let i = 0; i < Math.max(b.dels.length, b.adds.length); i++) {
    const w = b.words[i];
    rows.push({ segs: [...side(b.dels[i], half, w && w[0], TINT.del, TINT.delWord, false), bar,
      ...side(b.adds[i], right, w && w[1], TINT.add, TINT.addWord, true)] });
  }
}

const defaultMode = width => (width >= SPLIT_MIN ? 'split' : 'inline');

module.exports = { DIFF_OPTS, MAX_UNTRACKED, SPLIT_MIN, parse, render, defaultMode, unquote };

// --- pane

if (require.main === module) main();

function main() {
  const file = process.env.EXPLORER_FILE;
  const cwd = process.env.EXPLORER_ROOT || (file ? path.dirname(file) : process.cwd());
  const commit = process.env.EXPLORER_COMMIT;
  const staged = process.env.EXPLORER_STAGED === '1';
  const target = file ? ['--', file] : [];

  async function changes() {
    const env = { ...await gitEnv(cwd), GIT_LITERAL_PATHSPECS: '1' }; // a file named '*' or '[id]' means itself
    // stdout even on a non-zero exit code: diff --no-index returns 1 when the files differ.
    const git = (...args) => (GIT && spawnSync(GIT, ['-c', 'core.quotePath=false', ...args],
      { cwd, env, encoding: 'utf8', maxBuffer: 64 << 20, windowsHide: true }).stdout) || '';
    const diff = (...args) => git('diff', ...DIFF_OPTS, ...args);
    // A revision that would read as an option is refused; --end-of-options for the others.
    if (commit) {
      return /^-|[\x00-\x20]/.test(commit) ? ''
        : git('show', ...DIFF_OPTS, '--stat', '-p', '--diff-merges=first-parent', '--end-of-options', commit, ...target);
    }
    if (staged) return diff('--cached', ...target);
    // Without any commit, compare with the empty tree: staged changes, then unstaged ones.
    let text = git('rev-parse', '--verify', '-q', 'HEAD').trim() ? diff('HEAD', ...target)
      : diff('--cached', ...target) + diff(...target);
    const untracked = git('ls-files', '--others', '--exclude-standard', '-z', ...target).split('\0').filter(Boolean);
    for (const f of untracked.slice(0, MAX_UNTRACKED)) text += diff('--no-index', '--', '/dev/null', f);
    if (untracked.length > MAX_UNTRACKED) text += `\n… and ${untracked.length - MAX_UNTRACKED} more untracked files\n`;
    return text;
  }

  const name = commit ? `commit ${commit}${file ? ` · ${path.relative(cwd, file) || path.basename(file)}` : ''}`
    : `${file ? path.relative(cwd, file) || path.basename(file) : path.basename(cwd)}${staged ? ' (staged)' : ''}`;
  let parsed = null, rows = [], mode = null, cache = '', top = 0;

  function layout(W) {
    const key = `${W}:${mode || defaultMode(W)}`;
    if (parsed && key !== cache) { rows = render(parsed, W, mode || defaultMode(W)); cache = key; }
  }

  function draw() {
    const { W, H } = ui.size();
    layout(W);
    const body = H - 1;
    top = Math.max(0, Math.min(top, rows.length - body));
    const m = mode || defaultMode(W);
    const counts = parsed ? [[` +${parsed.add}`, GREEN], [` −${parsed.del}`, RED]] : [];
    const hints = `   ${m === 'split' ? 'side by side' : 'inline'} · s: layout · n/N: hunks · q: close`;
    const lines = [ui.line([[` ${name}`, '\x1b[1m'], ...counts, [hints, DIM]], W, ui.C.header)];
    if (!parsed) lines.push(ui.line([[' Loading…', DIM]], W));
    else if (!rows.length) lines.push(ui.line([[' (no changes)', DIM]], W));
    for (const r of rows.slice(top, top + body)) lines.push(ui.line(r.segs, W, r.back));
    while (lines.length < H) lines.push(ui.line([], W));
    paint(lines.slice(0, H));
  }

  function scroll(n) { top += n; draw(); }
  // Next or previous hunk: its header comes to the top.
  function jump(dir) {
    const i = dir > 0 ? rows.findIndex((r, k) => k > top && r.hunk)
      : rows.map((r, k) => (k < top && r.hunk ? k : -1)).filter(k => k >= 0).pop();
    if (i !== undefined && i >= 0) { top = i; draw(); }
  }

  const paint = ui.screen({
    key(k) {
      const page = ui.size().H - 2;
      if (k === 'q' || k === 'escape' || k === 'ctrl+c') process.exit(0);
      else if (k === 'up' || k === 'k') scroll(-1);
      else if (k === 'down' || k === 'j' || k === 'enter') scroll(1);
      else if (k === 'pageup' || k === 'b') scroll(-page);
      else if (k === 'pagedown' || k === 'space') scroll(page);
      else if (k === 'home' || k === 'g') { top = 0; draw(); }
      else if (k === 'end' || k === 'G') { top = rows.length; draw(); }
      else if (k === 'n') jump(1);
      else if (k === 'N') jump(-1);
      else if (k === 's') { mode = (mode || defaultMode(ui.size().W)) === 'split' ? 'inline' : 'split'; draw(); }
    },
    mouse(m) { if (m.wheel) scroll(m.wheel * 3); },
    resize: draw,
  }, { focus: false });
  draw();
  changes().then(text => { parsed = parse(text); cache = ''; draw(); });
}
