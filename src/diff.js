// Git changes of a file or folder, in a temporary side pane: tracked files compared with HEAD (staged, then
// unstaged), untracked files in full, or with another scope (b). Inline, or side by side from 120 columns; v switches,
// s, u and D stage, unstage and discard, n and N jump between hunks, arrows, Page keys or the wheel scroll, q or Esc
// closes the pane.
// Required as a module (by the preview and source control), it exports the parser, the renderer, the collection of
// changes and the layout setting.
'use strict';
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const ui = require('./core/ui');
const { GIT, gitEnv, run, readSettings, writeSettings, SETTINGS_FILE } = require('./core/exec');

const MAX_UNTRACKED = 200;
const MAX_ROWS = 200000; // beyond, a huge diff would take seconds to lay out at every resize
const SPLIT_MIN = 120;   // columns from which side by side is the default

// Colours of the rows: tinted backgrounds, stronger on the changed words.
const TINT = {
  get add() { return ui.C.added; }, get del() { return ui.C.removed; },
  get addWord() { return ui.C.addedWord; }, get delWord() { return ui.C.removedWord; }, get gap() { return ui.C.gap; },
};
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
// the first file (commit header and stat); each file { from, to, status, mode, notes, hunks, add, del } (mode: that of
// an added or deleted file), each hunk { head, lines: [[sign, text, old number, new number, place, cr]] }: place, the
// line's number in the new version, or for a removed line that of the line now in its place; cr, the line ended with a
// carriage return, left out of text (core/stage.js puts it back in the patches it makes).
function parse(text) {
  const d = { pre: [], files: [], add: 0, del: 0 };
  let f = null, h = null, oldLeft = 0, newLeft = 0, o = 0, n = 0;
  for (const raw of text.split('\n')) {
    const l = raw.replace(/\r$/, '');
    if (h && (oldLeft > 0 || newLeft > 0 || l.startsWith('\\')) && /^[ +\-\\]/.test(l)) {
      const s = l[0];
      if (s === '\\') { h.lines.push([s, l.slice(1).trim(), null, null]); continue; }
      h.lines.push([s, l.slice(1), s === '+' ? null : o, s === '-' ? null : n, n, l !== raw]);
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
    else if (l.startsWith('new file mode ')) { f.status = 'added'; f.mode = l.slice(14); }
    else if (l.startsWith('deleted file mode ')) { f.status = 'deleted'; f.mode = l.slice(18); }
    else if (l.startsWith('rename from ')) { f.from = unquote(l.slice(12)); f.status = 'renamed'; }
    else if (l.startsWith('rename to ')) f.to = unquote(l.slice(10));
    else if (l.startsWith('copy from ')) { f.from = unquote(l.slice(10)); f.status = 'copied'; }
    else if (l.startsWith('copy to ')) f.to = unquote(l.slice(8));
    else if (l.startsWith('Binary files')) f.notes.push('Binary file');
    else if (l.startsWith('old mode ')) { f.oldMode = l.slice(9); f.notes.push(`mode ${f.oldMode}`); }
    else if (l.startsWith('new mode ')) { f.newMode = l.slice(9); f.notes[f.notes.length - 1] += ` → ${f.newMode}`; }
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

// Rows of segments filling exactly width columns each: spans [text, changed] on the base background, changed parts on
// the strong one. view: { wrap } the text on as many rows as it needs, else on one row from column left.
function cells(spans, width, style, base, strong, { wrap = false, left = 0 } = {}) {
  const segs = spans.filter(([text]) => text)
    .map(([text, changed]) => [text, style + (changed && strong ? ui.bg(strong) : base ? ui.bg(base) : '')]);
  const parts = wrap && width > 0 ? ui.split(segs, width) : [ui.slice(segs, left, width)];
  return parts.map(p => {
    const used = ui.segWidth(p);
    return used < width ? [...p, [' '.repeat(width - used), base ? ui.bg(base) : '']] : p;
  });
}

const num = (v, w) => (v === null || v === undefined ? '' : String(v)).padStart(w);
let DIM, GREEN, RED, CYAN, NOTE;
ui.onTheme(() => { DIM = ui.fg(ui.C.dim); GREEN = ui.fg(ui.C.green); RED = ui.fg(ui.GIT.D); CYAN = ui.fg(ui.C.blue); NOTE = ui.fg(ui.C.yellow); });

// Header lines of a commit (show) and its stat, coloured here.
function preRow(l) {
  if (/^commit /.test(l)) return { segs: [[l, ui.fg(ui.C.yellow)]] };
  if (/^(Author|AuthorDate|Commit|CommitDate|Date|Merge):/.test(l)) return { segs: [[l, DIM]] };
  const stat = /^( .*\| +\d+ )(\+*)(-*)$/.exec(l);
  if (stat) return { segs: [[stat[1], ''], [stat[2], GREEN], [stat[3], RED]] };
  return { segs: [[l, '']] };
}

function fileRow(f, width, viewed = false) {
  const name = f.status === 'renamed' || f.status === 'copied' ? `${f.from} → ${f.to}` : f.to || f.from || '?';
  const status = (f.status === 'modified' ? '' : `  ${f.status}`) + (f.sec ? `  ${f.sec}` : '') + (viewed ? '  ✓ viewed' : '');
  const counts = ` +${f.add} −${f.del} `;
  const label = ' ' + ui.ellipsis(ui.cleanLine(name), Math.max(1, width - counts.length - status.length - 2));
  const pad = ' '.repeat(Math.max(0, width - ui.textWidth(label) - status.length - counts.length));
  return { segs: [[label, '\x1b[1m'], [status, DIM], [pad, ''], [` +${f.add}`, GREEN], [` −${f.del} `, RED]],
    back: ui.C.header, file: true };
}

// Rows of a parsed diff for a width: { segs, back } for ui.line(segs, width, back), hunk: true on the rows
// n and N jump to, ref: what a row shows, { file, from, to } (the path as git gives it, lines of the new version,
// 0 for the whole file). mode: 'inline', or 'split' (old version on the left, new on the right). titled: the
// caller's title already names the only file, whose own row is left out. view: { wrap, left } long lines wrapped
// (onto rows marked more), else cut from column left of the text. What each row shows, for core/stage.js: f, the file
// on its name's row; h, the hunk on its @@ row; lines, the hunk's lines on theirs (two on a side by side row). A file
// of a section (changesOf) has it in its refs (sec). view.viewed(f): the file shows folded, its name's row alone.
function render(d, width, mode = 'inline', titled = false, view = {}) {
  const rows = [];
  const single = titled && !d.pre.length && d.files.length === 1;
  for (const l of d.pre) rows.push(preRow(ui.cleanLine(l)));
  for (const f of d.files) {
    const file = f.to || f.from, sec = f.sec ? { sec: f.sec } : {};
    if (rows.length) rows.push({ segs: [] });
    const folded = !!(view.viewed && view.viewed(f));
    if (!single || f.sec || folded) rows.push({ ...fileRow(f, width, folded), hunk: !f.hunks.length, ref: { file, from: 0, to: 0, ...sec }, f });
    if (folded) continue;
    for (const note of f.notes) rows.push({ segs: [[`  ${note}`, DIM]] });
    if (!f.hunks.length && !f.notes.length && f.status !== 'renamed') rows.push({ segs: [['  (no content change)', DIM]] });
    let last = 1;
    for (const h of f.hunks) for (const [, , a, b] of h.lines) last = Math.max(last, a || 0, b || 0);
    const nw = String(last).length;
    for (const h of f.hunks) {
      const places = h.lines.filter(l => l[0] !== '\\').map(l => l[4]);
      rows.push({ segs: [[' ' + ui.cleanLine(h.head), CYAN]], back: ui.C.section, hunk: true, h,
        ref: { file, from: places.length ? Math.min(...places) : 0, to: places.length ? Math.max(...places) : 0, ...sec } });
      for (const block of blocksOf(h)) {
        if (rows.length > MAX_ROWS) { rows.push({ segs: [[' … diff too long, truncated', DIM]] }); return rows; }
        const at = rows.length;
        if (mode === 'split') splitRows(rows, block, width, nw, view); else inlineRows(rows, block, width, nw, view);
        for (let k = at; k < rows.length; k++) if (rows[k].place) rows[k].ref = { file, from: rows[k].place, to: rows[k].place, ...sec };
      }
    }
  }
  return rows;
}

// The text of a note's lines (ref: { file, from, to, sec }) in the version a diff shows, its new side (the index for
// a staged line, b for a range), so that the note's anchor is not read from the file on disk, which may differ; null
// when the diff does not hold them all (lines between two hunks).
function shownLines(parsed, ref) {
  const f = parsed.files.find(x => (x.to || x.from) === ref.file && (x.sec || '') === (ref.sec || ''));
  if (!f) return null;
  const byLine = new Map();
  for (const h of f.hunks) for (const l of h.lines) if (l[0] !== '\\' && l[3] != null) byLine.set(l[3], l[1]);
  const out = [];
  for (let n = ref.from; n <= ref.to; n++) { if (!byLine.has(n)) return null; out.push(byLine.get(n)); }
  return out;
}

// Where the lines from to to of the index (a staged note's ref) are in the file on disk, through the unstaged changes
// of the same path: { from, to }, the same lines without unstaged changes; null when some were changed or removed
// since they were staged.
function diskLines(parsed, ref) {
  const f = parsed.files.find(x => (x.to || x.from) === ref.file && x.sec === 'unstaged');
  if (!f) return { from: ref.from, to: ref.to };
  const at = i => {
    let delta = 0;
    for (const h of f.hunks) {
      const m = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))?/.exec(h.head);
      if (!m) return null;
      const [a, b, c, d] = [+m[1], m[2] === undefined ? 1 : +m[2], +m[3], m[4] === undefined ? 1 : +m[4]];
      if (b && i >= a && i < a + b) { const l = h.lines.find(x => x[2] === i); return l && l[0] === ' ' ? l[3] : null; }
      // The first line after the hunk, on each side (after line a when it holds none).
      const [oldNext, newNext] = [b ? a + b : a + 1, d ? c + d : c + 1];
      if (oldNext <= i) delta = newNext - oldNext;
      else break;
    }
    return i + delta;
  };
  const from = at(ref.from);
  if (from == null) return null;
  for (let i = ref.from + 1; i <= ref.to; i++) if (at(i) !== from + i - ref.from) return null;
  return { from, to: from + ref.to - ref.from };
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
// A hunk's blocks, worked out once: the changed words take the longest, and ← → render the diff again at each step.
const BLOCKS = new WeakMap();
const blocksOf = h => BLOCKS.get(h) || BLOCKS.set(h, blocks(h.lines)).get(h);

// A line too long for its row wraps onto rows marked more, with a blank gutter.
function inlineRows(rows, b, width, nw, view) {
  const row = (l, spans, back, strong, sign) => {
    const [s, text, a, n, place] = l;
    const gutter = s === '\\' ? ' '.repeat(nw * 2 + 2) : `${num(a, nw)} ${num(n, nw)} `;
    const style = s === '\\' ? DIM : '';
    const rest = Math.max(0, width - nw * 2 - 4);
    const make = left => cells(spans || [[ui.cleanLine(text), false]], rest, style, back, strong, { ...view, left });
    const lead = k => [[k ? ' '.repeat(gutter.length) : gutter, DIM], [k || s === '\\' ? ' ' : s, sign], [' ', '']];
    make(view.left || 0).forEach((c, k) => {
      rows.push({ segs: [...lead(k), ...c], back, place: s === '\\' ? 0 : place, ...(k ? { more: true } : { lines: [l] }),
        ...(view.wrap ? {} : { redo: left => [...lead(0), ...make(left)[0]] }) });
    });
  };
  if (b.line) return row(b.line, null, null, null, DIM);
  b.dels.forEach((l, i) => row(l, b.words[i] && b.words[i][0], TINT.del, TINT.delWord, RED));
  b.adds.forEach((l, i) => row(l, b.words[i] && b.words[i][1], TINT.add, TINT.addWord, GREEN));
}

// Each side wraps on its own: the shorter one is padded with blank rows, on its background.
function splitRows(rows, b, width, nw, view) {
  const half = Math.floor((width - 1) / 2), right = width - 1 - half;
  // The rows of one side, and the blank cell padding it.
  const side = (l, w, spans, base, strong, isNew, left) => {
    if (!l) return { parts: [], blank: [[' '.repeat(w), ui.bg(TINT.gap)]] };
    const [s, text, a, n] = l;
    const no = s === '+' || (s === ' ' && isNew) ? n : a;
    const lead = DIM + (base ? ui.bg(base) : '');
    const parts = cells(spans || [[ui.cleanLine(text), false]], Math.max(0, w - nw - 1), s === '\\' ? DIM : '', base, strong, { ...view, left })
      .map((c, k) => [[k ? ' '.repeat(nw + 1) : `${num(s === '\\' ? null : no, nw)} `, lead], ...c]);
    return { parts, blank: [[' '.repeat(w), base ? ui.bg(base) : '']] };
  };
  const bar = ['│', DIM];
  // pair(left): both sides cut from column left. Not wrapped, a row is made again from another column when drawn
  // (redo), rather than the whole diff at each ← →.
  const push = (pair, place, lines) => {
    const [l, r] = pair(view.left || 0);
    const join = (a, c, k) => [...(a.parts[k] || a.blank), bar, ...(c.parts[k] || c.blank)];
    for (let k = 0; k < Math.max(l.parts.length, r.parts.length, 1); k++) {
      rows.push({ segs: join(l, r, k), place, ...(k ? { more: true } : { lines }),
        ...(view.wrap ? {} : { redo: left => join(...pair(left), 0) }) });
    }
  };
  if (b.line) {
    push(left => [side(b.line, half, null, null, null, false, left), side(b.line, right, null, null, null, true, left)],
      b.line[0] === '\\' ? 0 : b.line[4], [b.line]);
    return;
  }
  for (let i = 0; i < Math.max(b.dels.length, b.adds.length); i++) {
    const w = b.words[i];
    push(left => [side(b.dels[i], half, w && w[0], TINT.del, TINT.delWord, false, left), side(b.adds[i], right, w && w[1], TINT.add, TINT.addWord, true, left)],
      (b.adds[i] || b.dels[i])[4], [b.dels[i], b.adds[i]].filter(Boolean));
  }
}

const defaultMode = width => (width >= SPLIT_MIN ? 'split' : 'inline');

// A commit, a stash or a tag with its changes, as source control and the preview show them: the git arguments, or null
// for a request not well formed. what: { kind, ref, paths }, paths limiting a commit to a file's history (its names).
const SHOW = ['--stat', '-p', '--diff-merges=first-parent'];
function showArgs({ kind, ref, paths = [] } = {}) {
  if (typeof ref !== 'string' || !ref || ref.startsWith('-') || !Array.isArray(paths) || !paths.every(p => typeof p === 'string' && p)) return null;
  if (kind === 'commit') return ['show', ...DIFF_OPTS, ...SHOW, '--format=fuller', '--decorate=short', ref, ...(paths.length ? ['--', ...paths] : [])];
  if (kind === 'stash') return ['stash', 'show', ...DIFF_OPTS, '--stat', '-p', '--include-untracked', ref];
  if (kind === 'tag') return ['show', ...DIFF_OPTS, ...SHOW, `refs/tags/${ref}`];
  return null;
}
// The changes of showArgs, parsed (parse), run from cwd: null when git fails.
async function show(cwd, what) {
  const args = showArgs(what);
  if (!args || !GIT) return null;
  const env = { ...await gitEnv(cwd), GIT_LITERAL_PATHSPECS: '1' };
  const out = await run(GIT, ['-c', 'core.quotePath=false', ...args], { cwd, env });
  return out === null ? null : parse(out);
}

// The options every diff's View menu starts with (⚙ or ,), each changing in place, the menu kept open: layout,
// 'inline' or 'split'; wrap, the Wrap lines setting. As in the Diff pane, the preview and source control.
const viewItems = (layout, wrap, switchLayout, toggleWrap) => [
  { label: 'Layout', key: 'v', value: [layout, CYAN], keep: true, run: switchLayout },
  { label: 'Wrap lines', key: 'w', value: ui.onOff(wrap), keep: true, run: toggleWrap },
];

// The default branch: the remote's HEAD (origin/main), else main, master, origin/main or origin/master, the first
// that exists; '' when none does. out(args): git's trimmed output for args ('' on a failure), or a promise of it.
async function defaultBranch(out) {
  const name = await out(['rev-parse', '--abbrev-ref', '-q', '--verify', 'refs/remotes/origin/HEAD']);
  if (name) return name;
  for (const b of ['main', 'master', 'origin/main', 'origin/master']) if (await out(['rev-parse', '--verify', '-q', `${b}^{commit}`])) return b;
  return '';
}

// The changes to show, as git diff writes them, for a scope (core/scope.js): those of the working tree compared with
// the last commit ('head'), where the branch left the default one ('branch', so every change of the branch,
// committed or not) or a branch ('ref'), untracked files in full; those since the agent's last turn started ('turn',
// turn: its snapshot, untracked files within); those of a commit or a range ('rev'). file: a file or folder to limit
// them to. Returns { text, top, baseName, error }: top, the repository's folder, which the diff's paths are relative
// to; baseName, the default branch ('' when none was found, or when compared with it but no common commit); error,
// why the scope could not be compared (nothing then compared); staged, for HEAD, the staged changes, text then
// holding the others (changesOf parses them together).
async function collect({ cwd, file = '', base = 'head', scope = null, turn = null }) {
  const kind = scope ? scope.kind : base;
  const env = { ...await gitEnv(cwd), GIT_LITERAL_PATHSPECS: '1' }; // a file named '*' or '[id]' means itself
  const target = file ? ['--', file] : [];
  // stdout even on a non-zero exit code: diff --no-index returns 1 when the files differ.
  const gitIn = (dir, ...args) => (GIT && spawnSync(GIT, ['-c', 'core.quotePath=false', ...args],
    { cwd: dir, env, encoding: 'utf8', maxBuffer: 64 << 20, windowsHide: true }).stdout) || '';
  const git = (...args) => gitIn(cwd, ...args);
  const diff = (...args) => git('diff', ...DIFF_OPTS, ...args);
  const top = git('rev-parse', '--show-toplevel').trim() || cwd;
  let baseName = '';
  let from = git('rev-parse', '--verify', '-q', 'HEAD').trim() ? 'HEAD' : null;
  // The default branch, named whatever the base, for the switch: the remote's HEAD, else main or master.
  if (from) {
    baseName = await defaultBranch(args => git(...args).trim());
  }
  if (from && kind === 'branch') {
    const fork = baseName && git('merge-base', 'HEAD', baseName).trim();
    if (fork) from = fork; else baseName = '';
  }
  // A commit the user typed: its id, or null. --end-of-options keeps a name starting with - from reading as an option.
  const commit = rev => (rev && git('rev-parse', '--verify', '-q', '--end-of-options', `${rev}^{commit}`).trim()) || null;
  if (kind === 'turn') {
    if (!turn || path.resolve(turn.top) !== path.resolve(top)) return { text: '', top, baseName, error: 'No agent turn recorded in this repository' };
    if (!turn.tree) return { text: '', top, baseName, error: 'The agent turn is still being recorded' };
    // Only the file compared added: the preview reloads at each save of it.
    const now = await require('./core/scope').worktree(top, env, true, file || null);
    if (!now) return { text: '', top, baseName, error: 'Cannot read the working tree' };
    return { text: diff(turn.tree, now, ...target), top, baseName };
  }
  if (kind === 'rev') {
    // a..b, a...b (from where they parted), or one commit: its own changes, against its first parent. A side left
    // out is HEAD, as for git (main.., ..main).
    const m = /^(.*?)(\.\.\.?)(.*)$/.exec(scope.rev);
    if (m) {
      const [x, y] = [m[1] || 'HEAD', m[3] || 'HEAD'];
      const [a, b] = [commit(x), commit(y)];
      if (!a || !b) return { text: '', top, baseName, error: `Unknown commit: ${!a ? x : y}` };
      const start = m[2] === '...' ? git('merge-base', a, b).trim() : a;
      if (!start) return { text: '', top, baseName, error: 'No common commit' };
      return { text: diff(start, b, ...target), top, baseName };
    }
    const c = commit(scope.rev);
    if (!c) return { text: '', top, baseName, error: `Unknown commit: ${scope.rev}` };
    // No parent: compared with the empty tree, whose id depends on the hash (SHA-1 or SHA-256).
    const parent = commit(`${c}^`) || git('hash-object', '-t', 'tree', '--stdin').trim();
    return { text: diff(parent, c, ...target), top, baseName };
  }
  if (kind === 'ref') {
    from = commit(scope.rev);
    if (!from) return { text: '', top, baseName, error: `Unknown branch: ${scope.rev}` };
  }
  // HEAD (or the empty tree, before any commit): the staged changes apart, without renames so that each path stands
  // for itself when a part of it is staged or unstaged (core/stage.js).
  const sections = kind === 'head' || !from;
  const staged = sections ? diff('--cached', '--no-renames', ...target) : undefined;
  let text = sections ? diff('--no-renames', ...target) : diff(from, ...target);
  // Untracked files named from top, as git diff names the others (ls-files and diff --no-index name them from cwd).
  const untracked = git('ls-files', '--others', '--exclude-standard', '--full-name', '-z', ...target).split('\0').filter(Boolean);
  for (const f of untracked.slice(0, MAX_UNTRACKED)) text += gitIn(top, 'diff', ...DIFF_OPTS, '--no-index', '--', '/dev/null', f);
  if (untracked.length > MAX_UNTRACKED) text += `\n… and ${untracked.length - MAX_UNTRACKED} more untracked files\n`;
  return { text, staged, top, baseName };
}

// The lines of a file changed since HEAD, for the preview's File view, as buffer.lineDiff gives them: { kinds,
// deleted }, kinds[i] 'add' or 'mod' for line i (from 0), deleted the lines just under removed ones (the line count
// for the end); every line added for an untracked file of count lines. null outside a repository, before the first
// commit, or when git fails; nothing changed for an ignored file.
// Kept while the file and HEAD stay the same (cacheKey), since the preview asks again at each focus and reload.
const lineCache = new Map();
async function lineChanges(file, count) {
  const abs = path.resolve(file), key = cacheKey(abs, count), hit = lineCache.get(abs);
  if (key && hit && hit.key === key) return hit.res;
  const res = await readLineChanges(abs, count);
  lineCache.delete(abs);
  if (key && res) lineCache.set(abs, { key, res });
  if (lineCache.size > 50) lineCache.delete(lineCache.keys().next().value);
  return res;
}

// What lineChanges depends on, read without git: the file's mtime and size, and HEAD (its text, and the times of the
// branch it names and of packed-refs, which a commit or a reset changes). null outside a repository found as a .git
// folder or file above the file.
function cacheKey(abs, count) {
  const read = f => { try { return fs.readFileSync(f, 'utf8'); } catch { return null; } };
  const time = f => { try { return fs.statSync(f).mtimeMs; } catch { return 0; } };
  let st;
  try { st = fs.statSync(abs); } catch { return null; }
  for (let d = path.dirname(abs); ; d = path.dirname(d)) {
    const dot = path.join(d, '.git');
    const kind = fs.statSync(dot, { throwIfNoEntry: false });
    if (kind) {
      let gitdir = dot;
      if (kind.isFile()) {
        const m = /^gitdir: (.+)$/m.exec(read(dot) || '');
        if (!m) return null;
        gitdir = path.resolve(d, m[1].trim());
      }
      const head = read(path.join(gitdir, 'HEAD'));
      if (head === null) return null;
      const common = path.resolve(gitdir, (read(path.join(gitdir, 'commondir')) || '.').trim());
      const ref = (/^ref: (.+)$/m.exec(head) || [])[1];
      return [abs, count, st.mtimeMs, st.size, head.trim(), ref ? time(path.join(common, ref.trim())) : 0,
        time(path.join(common, 'packed-refs'))].join('|');
    }
    if (path.dirname(d) === d) return null;
  }
}

async function readLineChanges(abs, count) {
  const cwd = path.dirname(abs);
  const env = { ...await gitEnv(cwd), GIT_LITERAL_PATHSPECS: '1' };
  const git = (...args) => run(GIT, ['-c', 'core.quotePath=false', ...args], { cwd, env });
  const untracked = await git('ls-files', '--others', '--exclude-standard', '-z', '--', abs);
  if (untracked === null) return null;
  if (untracked) return { kinds: new Array(count).fill('add'), deleted: new Set() };
  const text = await git('diff', ...DIFF_OPTS, '-U0', '--no-renames', 'HEAD', '--', abs);
  if (text === null) return null;
  const kinds = [], deleted = new Set();
  for (const m of text.matchAll(/^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/gm)) {
    const del = m[1] === undefined ? 1 : +m[1], add = m[3] === undefined ? 1 : +m[3];
    // Without new lines, the hunk names the line before the removed ones.
    const at = add ? +m[2] - 1 : +m[2];
    for (let k = 0; k < add; k++) kinds[at + k] = k < del ? 'mod' : 'add';
    if (del > add) deleted.add(at + add);
  }
  return { kinds, deleted };
}

// The parsed changes of what collect returned: with the staged changes apart (HEAD), each file of either section
// marked sec 'staged' or 'unstaged', and the files in path order, a path's staged changes before its unstaged ones.
function changesOf(res) {
  if (res.staged === undefined) return parse(res.text);
  const a = parse(res.staged), b = parse(res.text);
  for (const f of a.files) f.sec = 'staged';
  for (const f of b.files) f.sec = 'unstaged';
  const name = f => f.to || f.from || '';
  const files = [...a.files, ...b.files].sort((x, y) => (name(x) < name(y) ? -1 : name(x) > name(y) ? 1 : 0));
  const pre = [...a.pre, ...b.pre].some(l => l.trim()) ? [...a.pre, ...b.pre] : []; // an empty section's blank line
  return { pre, files, add: a.add + b.add, del: a.del + b.del };
}

// The layout chosen with v, kept in the settings for the next diffs (source control and the preview included), by
// width: one for narrow diffs (under SPLIT_MIN columns, as a preview beside the terminal) and one for wide ones (a
// zoomed preview, the Changes pane), so that each keeps its own. diffLayout: { narrow, wide }, each 'inline' or
// 'split', unset to choose by width; a single value (the settings view's, or older versions') holds for both until v
// chooses one.
const sizeOf = width => (width >= SPLIT_MIN ? 'wide' : 'narrow');
function savedMode(width) {
  const m = readSettings().diffLayout, v = m && typeof m === 'object' ? m[sizeOf(width)] : m;
  return v === 'inline' || v === 'split' ? v : null;
}
function saveMode(width, mode) {
  const s = readSettings(), old = s.diffLayout;
  const both = old && typeof old === 'object' ? old : old ? { narrow: old, wide: old } : {};
  writeSettings({ ...s, diffLayout: { ...both, [sizeOf(width)]: mode } });
}
// The layout of a diff of that width: the one chosen for its size, else by width.
const layoutOf = width => savedMode(width) || defaultMode(width);

// The width of the longest line of parsed changes (parse), the most ← → can scroll; 0 without them.
const WIDEST = new WeakMap();
function widestOf(parsed) {
  if (!parsed) return 0;
  if (!WIDEST.has(parsed)) {
    let w = 0;
    for (const f of parsed.files) for (const h of f.hunks) for (const l of h.lines) w = Math.max(w, ui.textWidth(ui.cleanLine(l[1])));
    WIDEST.set(parsed, w);
  }
  return WIDEST.get(parsed);
}

module.exports = { DIFF_OPTS, MAX_UNTRACKED, SPLIT_MIN, parse, render, collect, defaultBranch, viewItems, lineChanges, showArgs, show, changesOf, shownLines, defaultMode, savedMode, saveMode, layoutOf, widestOf, diskLines, unquote, main };

// --- pane

if (require.main === module) main();

function main() {
  const file = process.env.EXPLORER_FILE;
  const cwd = process.env.EXPLORER_ROOT || (file ? path.dirname(file) : process.cwd());
  require('./launch').home(); // the pane's folder for new spaces, tabs and panes made from it
  const PANE = process.env.HERDR_PANE_ID;
  // The explorer's pane, whose terminal the notes go to; this pane's own when opened without it.
  const FROM = process.env.EXPLORER_PANE || PANE;
  const notes = require('./core/notes'), actions = require('./core/actions'), Scope = require('./core/scope'), stage = require('./core/stage');
  const Viewed = require('./core/viewed');
  // scope: what the changes are compared with, the tab's (core/scope.js), HEAD until the tab is known. top: the
  // repository's folder, which the diff's paths are relative to. baseName: the default branch, once found.
  let scope = Scope.HEAD, top = cwd, baseName = '';

  // The changes for the scope chosen now, parsed; null when b chose another one meanwhile, whose run then shows its own.
  let runs = 0;
  async function changes() {
    const run = ++runs;
    const res = await collect({ cwd, file, scope, turn: scope.kind === 'turn' && tab ? Scope.readTurn(tab) : null });
    if (run !== runs) return null;
    top = res.top; baseName = res.baseName;
    if (res.error) say(res.error);
    return changesOf(res);
  }

  const title = () => (file ? path.relative(cwd, file) || path.basename(file) : path.basename(cwd));
  // cur: the row of the cursor, which notes and references take; note: { field, kind, ref } while one is typed.
  // anchor: the other end of the rows selected with shift (or the mouse), null for the cursor's row alone. stay: the
  // row the cursor goes back to once the changes are read again (after s, u or D), null to follow its lines.
  let parsed = null, rows = [], cache = '', top0 = 0, cur = -1, note = null, flash = null, tab = null, tabNotes = [];
  let anchor = null, stay = null;
  // wrap: long lines wrapped (the preview's setting, w in either); left: the first column shown of the text otherwise.
  let wrap = !!readSettings().wrap, left = 0;
  // viewed: the paths (relative to top) of the files marked viewed (m) and unchanged since, shown folded.
  let viewed = new Set();
  // left is not in it: the rows cut from another column are made again when drawn (redo).
  const keyOf = (width, m) => `${width}:${m}:${wrap}:${[...viewed].join('\0')}`;
  const scrollbar = new ui.Scrollbar(t => { top0 = t; draw(); });
  const widest = () => widestOf(parsed);

  function layout(width, m) {
    const key = keyOf(width, m);
    if (parsed && key !== cache) {
      // The cursor stays on the same lines when the layout changes.
      const was = rows[cur] && rows[cur].ref, hunk = !!(rows[cur] && rows[cur].hunk);
      rows = render(parsed, width, m, false, { wrap, viewed: f => viewed.has(f.to || f.from) }); cache = key;
      const at = r => r.ref && !r.more;
      let i = stay !== null ? rows.findIndex((r, k) => k >= stay && at(r))
        : was ? rows.findIndex(r => at(r) && r.ref.file === was.file && r.ref.from === was.from && r.ref.sec === was.sec && !!r.hunk === hunk) : -1;
      if (i < 0 && stay !== null) i = rows.map((r, k) => (at(r) ? k : -1)).filter(k => k >= 0).pop() ?? -1;
      cur = i >= 0 ? i : rows.findIndex(r => r.ref);
      stay = null; anchor = null;
    }
  }

  // Lines covered by a note of the tab, by path relative to the repository.
  const reals = new Map(); // the real path of each file of the diff, looked up once
  // diskLines of a staged row, once per parsed (noted runs at each draw).
  const DISK = new WeakMap();
  const diskOf = ref => {
    if (!DISK.has(parsed)) DISK.set(parsed, new Map());
    const m = DISK.get(parsed), key = `${ref.file}\0${ref.from}\0${ref.to}`;
    return m.has(key) ? m.get(key) : m.set(key, diskLines(parsed, ref)).get(key);
  };
  const realOf = f => (reals.has(f) ? reals.get(f) : reals.set(f, notes.real(path.join(top, f))).get(f));
  // A staged row is at its index line, the notes at disk lines (noteKey): compared where it is on disk.
  const noted = r => {
    if (!r.ref || !r.ref.from || !tabNotes.length) return false;
    const at = r.ref.sec === 'staged' && parsed ? diskOf(r.ref) : r.ref;
    return !!at && tabNotes.some(n => n.from && !n.stale && n.file === realOf(r.ref.file) && at.from >= n.from && at.to <= n.to);
  };

  function draw() {
    const { W, H } = ui.size();
    const m = layoutOf(W);
    const body = H - 2;
    // The first column holds the cursor's bar, the last the scrollbar when it shows (a narrower layout never has
    // fewer rows): one column narrower then, kept while it shows.
    if (cache !== keyOf(W - 2, m) || rows.length <= body) layout(W - 1, m);
    if (rows.length > body) layout(W - 2, m);
    top0 = Math.max(0, Math.min(top0, rows.length - body));
    const bw = scrollbar.set(rows.length, body, top0);
    // Header: the name and the counts, then the scope (a click opens b's menu) and ⚙, which opens the view's options
    // (,), as in the preview's Diff view; the scope left out when they do not fit.
    const counts = parsed ? [[` +${parsed.add}`, GREEN], [` −${parsed.del}`, RED]] : [];
    const head = [[` ${title()}`, '\x1b[1m'], ...counts];
    const gear = { segs: [[` ${ui.G.gear} `, ui.fg(menu ? ui.C.text : ui.C.dim)]], width: 3, hits: [{ id: 'menu', from: 0, to: 2 }] };
    const switches = [ui.segmented([['scope', Scope.label(scope, baseName)]], 'scope'), gear];
    const width = () => switches.reduce((w, s) => w + s.width + 1, 0);
    while (switches.length && W - ui.segWidth(head) - width() < 1) switches.shift();
    chips = [];
    let x = W - width() + 1;
    const segs = [...head, [' '.repeat(Math.max(0, W - ui.segWidth(head) - width())), '']];
    for (const s of switches) {
      segs.push(...s.segs, [' ', '']);
      for (const h of s.hits) chips.push({ id: h.id, from: x + h.from, to: x + h.to });
      x += s.width + 1;
    }
    const lines = [ui.line(segs, W, ui.C.header)];
    if (!parsed) lines.push(...ui.empty(W, body, 'Loading…'));
    else if (!rows.length) lines.push(...ui.empty(W, body, 'No changes', [['b', 'compare with'], ['q', 'close']]));
    // The cursor's row and the others selected, and the noted ones: a bar in the first column.
    const [from, to] = range();
    rows.slice(top0, top0 + body).forEach((r, k) => {
      const i = top0 + k, mark = i >= from && i <= to ? ['▌', ui.fg(ui.C.blue)] : noted(r) ? ['▎', NOTE] : [' ', ''];
      lines.push(ui.line([mark, ...(left && r.redo ? r.redo(left) : r.segs)], W - bw, i === cur && !r.back ? ui.C.selectBlur : r.back) + scrollbar.cell(k));
    });
    while (lines.length < H - 1) lines.push(ui.line([], W));
    lines.length = H - 1;
    lines.push(footer(W));
    if (menu) menu.draw(lines, W, H);
    if (help) ui.help(lines, W, 'Changes', HELP, help);
    paint(lines);
  }

  function footer(W) {
    hints = [];
    if (prompt) {
      const lead = ' Commit or range (abc123, main..HEAD): ';
      return ui.line([[lead, CYAN], ...prompt.field.segments(Math.max(1, W - ui.textWidth(lead)))], W, ui.C.header);
    }
    if (confirm) return ui.line([[` ${confirm.text} `, ui.fg(ui.C.red)], ...ui.keys([['y', 'yes'], ['n', 'no']], W)], W, ui.C.header);
    if (note) {
      const lead = ` ✎ ${noteWhere(note.ref)} `, kind = note.kind ? `[${note.kind}] ` : '[tab: kind] ';
      return ui.line([[lead, NOTE], [kind, DIM], ...note.field.segments(Math.max(1, W - ui.textWidth(lead + kind)))], W, ui.C.header);
    }
    if (flash) return ui.line(ui.message(flash), W, ui.C.header);
    // The keys, and how far down the diff is.
    const body = ui.size().H - 2;
    const pct = rows.length > body ? ` ${Math.round(100 * top0 / Math.max(1, rows.length - body))}% ` : '';
    const send = tabNotes.length ? [['A', `send ${tabNotes.length} note${tabNotes.length > 1 ? 's' : ''}`]] : [];
    // b and v are left out: the header's switches show them (their keys in the help).
    const edit = scope.kind === 'head' ? [['s', 'stage'], ['u', 'unstage']] : [];
    hints = ui.keys([...send, ...edit, ['a', 'note'], ['n N', 'hunks'], ['q', 'close'], ['?', 'keys']], W - pct.length);
    return ui.line([...hints, [pct.padStart(Math.max(0, W - ui.segWidth(hints))), DIM]], W, ui.C.header);
  }

  function say(text) {
    if (!text) return draw();
    flash = text;
    draw();
    clearTimeout(say.timer);
    say.timer = setTimeout(() => { flash = null; draw(); }, ui.messageTime(ui.messageKind(text)));
  }

  // The key help (? or f1), over the whole pane (ui.help): { top, find } while it shows. hints: those of the footer;
  // chips: the choices of the header's switches, { id, from, to } in columns from 1.
  let help = null, hints = [], chips = [];
  const HELP = [
    ['Move', [['↑↓ j k', 'line'], ['pgup pgdn', 'page'], ['space', 'page down'], ['ctrl+u/d', 'half page'], ['g G', 'top, bottom']]],
    ['Diff', [[',', 'the view\'s options (or ⚙)'], ['n N', 'next, previous hunk'], ['v', 'inline, split (side by side)'], ['w', 'wrap long lines'], ['← → h l', 'left, right (no wrap)'],
      ['b', 'compare with: HEAD, merge base, last agent turn, a commit or range, a branch']]],
    ['Stage (HEAD)', [['shift+↑↓ J K', 'select lines'], ['s', 'stage the line, hunk (@@ row) or file (name row)'],
      ['u', 'unstage it'], ['D', 'discard it from the working tree']]],
    ['Review', [['a', 'note on the line (hunk, file)'], ['tab', 'kind of note, while typing'], ['A', 'send the notes to the agent'],
      ['C', 'copy the notes'], ['U', 'remove the last note'], ['y', 'copy path:line'],
      ['m', 'mark the file viewed: folded until it changes'], ['] [', 'next, previous file not viewed']]],
    ['Pane', [['ctrl+p', 'go to file'], ['ctrl+f', 'search in files'], ['z', 'zoom the pane'], ['? f1', 'keys'], ['q esc', 'close']]],
  ];
  // Go to file and Search in files from the diff's folder, as from the explorer.
  const search = mode => actions.search(cwd, PANE, mode);

  // The files marked viewed and unchanged since, among those of the diff.
  function readViewed() {
    const map = tab ? Viewed.read(tab) : {};
    viewed = new Set(parsed ? parsed.files.map(f => f.to || f.from).filter(n => n && Viewed.is(map, realOf(n))) : []);
  }
  // m: the cursor's file viewed, or no longer.
  function markViewed() {
    const r = rows[cur];
    if (!r || !r.ref) return;
    if (!tab) return say('Needs herdr');
    const now = Viewed.toggle(tab, realOf(r.ref.file));
    readViewed();
    say(now ? 'Viewed: folded until it changes' : 'No longer viewed');
  }
  // ] and [: the next or previous file not viewed, its name at the top.
  function nextFile(dir) {
    const ok = (r, k) => r.f && !viewed.has(r.ref.file) && (dir > 0 ? k > cur : k < cur);
    const i = dir > 0 ? rows.findIndex(ok) : rows.map((r, k) => (ok(r, k) ? k : -1)).filter(k => k >= 0).pop();
    if (i === undefined || i < 0) return say(dir > 0 ? 'No next file to view' : 'No previous file to view');
    cur = i; top0 = i; anchor = null; draw();
  }
  // The rows selected: the cursor's, or from the anchor to it.
  const range = () => (anchor === null ? [cur, cur] : [Math.min(anchor, cur), Math.max(anchor, cur)]);
  // The cursor moves over the rows that show lines of a file, the first row of a wrapped line; the view follows it.
  // select: shift held, the rows from where the cursor was are selected, else the selection goes.
  function move(n, select = false) {
    if (select) { if (anchor === null) anchor = cur; } else anchor = null;
    const dir = Math.sign(n);
    for (let i = cur + dir, todo = Math.abs(n); todo > 0 && i >= 0 && i < rows.length; i += dir) if (rows[i].ref && !rows[i].more) { cur = i; todo--; }
    follow();
    draw();
  }
  function follow() {
    const body = ui.size().H - 2;
    if (cur < top0) top0 = cur;
    if (cur >= top0 + body) top0 = cur - body + 1;
  }
  function scroll(n) { top0 += n; draw(); }
  // Next or previous hunk: its header comes to the top, with the cursor.
  function jump(dir) {
    const i = dir > 0 ? rows.findIndex((r, k) => k > cur && r.hunk)
      : rows.map((r, k) => (k < cur && r.hunk ? k : -1)).filter(k => k >= 0).pop();
    if (i !== undefined && i >= 0) { cur = i; top0 = i; draw(); }
  }

  const noteWhere = ref => `${ref.file}${ref.from ? `:${ref.from}${ref.to > ref.from ? `-${ref.to}` : ''}` : ''}`;
  function startNote() {
    const r = rows[cur];
    if (!r || !r.ref) return;
    if (!tab) return say('Notes need herdr');
    note = { field: new ui.Field(), kind: '', ref: r.ref };
    draw();
  }
  function noteKey(k) {
    if (k === 'escape' || k === 'ctrl+c') { note = null; return draw(); }
    if (k === 'tab' || k === 'shift+tab') {
      const i = notes.KINDS.indexOf(note.kind);
      note.kind = notes.KINDS[(i + (k === 'tab' ? 1 : notes.KINDS.length - 1)) % notes.KINDS.length];
      return draw();
    }
    if (k === 'enter') {
      const n = note;
      note = null;
      if (!n.field.value.trim()) return draw();
      // A staged line: placed where it is on disk, which notes follow; stale when changed there since.
      const disk = parsed && n.ref.from && n.ref.sec === 'staged' ? diskLines(parsed, n.ref) : undefined;
      const msg = notes.add(tab, { file: path.join(top, n.ref.file), ...(disk || { from: n.ref.from, to: n.ref.to }), kind: n.kind,
        text: n.field.value, lines: parsed ? shownLines(parsed, n.ref) : undefined, stale: disk === null });
      tabNotes = notes.current(tab);
      return say(msg);
    }
    if (note.field.key(k)) draw();
  }
  async function act(promise) { say(await promise.catch(() => 'Action failed')); }
  // y: the cursor's lines, relative to the pane's folder (src/app.js:12-14), to paste into a prompt.
  function copyReference() {
    const r = rows[cur];
    if (!r || !r.ref) return;
    // From the folder resolved as top is (git gives its real path): /home → /var/home would otherwise give ../../..
    const rel = path.relative(notes.real(cwd), path.join(top, r.ref.file)).split(path.sep).join('/');
    act(actions.copy(noteWhere({ ...r.ref, file: rel }), r.ref.from ? 'Reference copied' : 'Path copied'));
  }
  // The changes loaded again, for a new scope.
  async function reload() {
    parsed = null; cache = ''; draw();
    const d = await changes();
    if (d === null) return;
    parsed = d; cache = '';
    if (tab) tabNotes = notes.current(tab);
    readViewed();
    draw();
    if (scope.kind === 'branch' && !baseName) say('No default branch: compared with HEAD');
  }
  // s, u, D: the rows selected staged, unstaged or discarded (core/stage.js), with HEAD as the scope only (the others
  // show changes the index has nothing to do with); D asks first. The cursor then stays where it was.
  let confirm = null; // { text, run, no } while D asks
  let busy = false; // while stage.apply runs: a second s, u or D would apply the same rows again
  function edit(mode) {
    if (!parsed || !rows.length) return;
    if (busy) return say('Busy…');
    if (scope.kind !== 'head') return say('Not with this scope: stage compared with HEAD (b)');
    const [from, to] = range(), sel = stage.picked(rows.slice(from, to + 1));
    const go = async () => {
      confirm = null;
      if (busy) return say('Busy…');
      busy = true;
      try {
        // Untracked files the trash did not take: deleted for good once asked again.
        const noTrash = names => new Promise(resolve => {
          confirm = { text: `No trash: delete ${names.length > 1 ? `${names.length} files` : names[0]} for good?`, run: () => resolve(true), no: () => resolve(false) };
          draw();
        });
        say(await stage.apply(top, parsed, sel, mode, { noTrash }).catch(() => `Cannot ${mode}`));
        const d = await changes();
        if (d === null) return;
        parsed = d; cache = ''; stay = cur; readViewed(); draw();
      } finally { busy = false; }
    };
    if (mode !== 'discard') return go();
    const n = stage.count(parsed, sel, mode);
    if (!n) return say('Nothing unstaged selected');
    confirm = { text: `Discard ${n} from the working tree?`, run: go };
    draw();
  }
  // A scope chosen here, for every pane of the tab; the current one chosen again reloads it (an agent turn being
  // recorded, for instance).
  function pick(s) {
    menu = null; prompt = null;
    if (!Scope.same(s, scope)) {
      scope = s;
      if (tab) Scope.write(tab, s);
    }
    reload();
  }
  // menu: b's (ui.Menu), or the branches of Branch…; prompt: { field } while a commit or range is typed.
  let menu = null, prompt = null;
  // The view's options (⚙ or ,), as in the preview's Diff view: each changes in place, the menu staying open.
  function openView(sel = 0) {
    const W = ui.size().W;
    menu = new ui.Menu({ title: 'View', at: 1, right: W, items: [...viewItems(layoutOf(W), wrap, switchLayout, toggleWrap),
      { label: 'Compare with', key: 'b', value: [Scope.label(scope, baseName), CYAN], run: openScopes },
    ] });
    menu.sel = sel;
    draw();
  }
  // A choice of a menu, by key or click: run; the view's options shown again with the new values, unless closed.
  function menuChoice(r) {
    menu.choose(r, () => { menu = null; }, openView);
    draw();
  }
  function switchLayout() { const W = ui.size().W; saveMode(W, layoutOf(W) === 'split' ? 'inline' : 'split'); draw(); }
  function toggleWrap() { wrap = !wrap; left = 0; writeSettings({ ...readSettings(), wrap }); draw(); }
  function openScopes() {
    menu = Scope.menu(scope, baseName, cwd, { pick, say,
      show: m => { menu = m; draw(); }, prompt: rev => { prompt = { field: new ui.Field(rev) }; draw(); } });
    draw();
  }
  function promptKey(k) {
    const r = Scope.revKey(prompt.field, k);
    if (!r.done) return r.changed && draw();
    prompt = null;
    return r.rev ? pick({ kind: 'rev', rev: r.rev }) : draw();
  }

  function onKey(k) {
    const page = ui.size().H - 3;
    if (note) return noteKey(k);
    if (prompt) return promptKey(k);
    if (confirm) {
      const c = confirm, yes = k === 'y' || k === 'enter';
      confirm = null;
      if (!yes && c.no) c.no();
      return yes ? c.run() : draw();
    }
    if (menu) return menuChoice(menu.key(k, ui.size().H));
    if (k === 'ctrl+c') process.exit(0);
    if (help) { if (!ui.helpKey(k, help)) help = null; return draw(); }
    if (k === '?' || k === 'f1') { help = { top: 0, find: null }; draw(); }
    else if (k === 'escape' && anchor !== null) { anchor = null; draw(); }
    else if (k === 'q' || k === 'escape') process.exit(0);
    else if (k === 'up' || k === 'k') move(-1);
    else if (k === 'down' || k === 'j' || k === 'enter') move(1);
    else if (k === 'shift+up' || k === 'K') move(-1, true);
    else if (k === 'shift+down' || k === 'J') move(1, true);
    else if (k === 'pageup') move(-page);
    else if (k === 'pagedown' || k === 'space') move(page);
    else if (k === 'ctrl+u') move(-Math.ceil(page / 2));
    else if (k === 'ctrl+d') move(Math.ceil(page / 2));
    else if (k === 'z') actions.zoom(PANE);
    else if (k === 'ctrl+p') act(search('files'));
    else if (k === 'ctrl+f') act(search('content'));
    else if (k === 'home' || k === 'g') { cur = rows.findIndex(r => r.ref); top0 = 0; draw(); }
    else if (k === 'end' || k === 'G') { move(rows.length); top0 = rows.length; draw(); }
    else if (k === 'n') jump(1);
    else if (k === 'N') jump(-1);
    else if (k === 'v') switchLayout();
    else if (k === ',') openView();
    else if (k === 's') edit('stage');
    else if (k === 'u') edit('unstage');
    else if (k === 'D') edit('discard');
    else if (k === 'b') openScopes();
    else if (k === 'w') toggleWrap();
    else if (k === 'left' || k === 'h') { if (!wrap && left) { left = Math.max(0, left - 8); draw(); } }
    else if (k === 'right' || k === 'l') { if (!wrap && left + 8 < widest()) { left += 8; draw(); } }
    else if (k === 'a') startNote();
    else if (k === 'A') act(notes.send(tab, FROM));
    else if (k === 'C') act(notes.copy(tab, top));
    else if (k === 'U') say(notes.undo(tab));
    else if (k === 'm') markViewed();
    else if (k === ']') nextFile(1);
    else if (k === '[') nextFile(-1);
    else if (k === 'y') copyReference();
  }

  const paint = ui.screen({
    key: onKey,
    paste(text) { const f = prompt ? prompt.field : note && note.field; if (f) { f.insert(text); draw(); } },
    mouse(m) {
      if (help) { if (!ui.helpMouse(m, help)) help = null; return draw(); }
      if (menu) return menuChoice(menu.mouse(m, ui.size().H));
      if (m.wheel) return scroll(m.wheel * 3);
      const click = m.press && !m.drag && m.button === 0;
      // The header's switches: a click on a choice takes it.
      const chip = click && m.y === 1 && chips.find(c => m.x >= c.from && m.x <= c.to);
      if (chip) {
        if (chip.id === 'menu') openView();
        if (chip.id === 'scope') openScopes();
        return;
      }
      // A click on a hint of the footer does what its key does.
      const k = click && m.y === ui.size().H && ui.keyAt(hints, m.x - 1);
      if (k) return onKey(k);
      if (scrollbar.mouse(m, m.y - 2, m.x === ui.size().W)) return;
      // A click on a row puts the cursor there; a drag selects the rows from there.
      let i = top0 + m.y - 2;
      if ((click || (m.drag && m.button === 0)) && !note && m.y >= 2 && m.y < ui.size().H && rows[i] && rows[i].ref) {
        while (rows[i].more) i--;
        if (click) anchor = null;
        else if (anchor === null) anchor = cur;
        cur = i;
        draw();
      }
    },
    resize: draw,
    theme() { cache = ''; draw(); },
    // Left zoomed, the tab would show alone whatever pane takes the focus (actions.unzoom).
    focus(on) { if (!on) actions.unzoom(PANE).catch(() => { }); },
  }, { motion: true });
  draw();
  changes().then(d => { if (d !== null) { parsed = d; cache = ''; readViewed(); draw(); } });
  // Wrap lines and the layout changed in another pane (the preview, source control): shown at once.
  if (SETTINGS_FILE) fs.watchFile(SETTINGS_FILE, { interval: 1000 }, () => {
    const now = !!readSettings().wrap;
    if (now !== wrap) { wrap = now; left = 0; }
    draw();
  });
  // The tab, whose notes this pane shows and takes.
  if (PANE) {
    actions.tabOf(PANE).then(t => {
      tab = t && t.me.tab_id;
      if (!tab) return;
      tabNotes = notes.current(tab);
      // Placed again by the pane that saw a file change (or at a reload here): read as saved.
      notes.watch(tab, () => { tabNotes = notes.read(tab); draw(); });
      Viewed.watch(tab, () => { readViewed(); draw(); });
      readViewed();
      // The tab's scope, and the one another pane chooses later.
      const follow = () => { const s = Scope.read(tab); if (!Scope.same(s, scope)) { scope = s; reload(); } };
      Scope.watch(tab, follow);
      follow();
      draw();
    });
  }
}
