// Drawing of the source control pane: the rows of each view, the header, tabs, commit box, status line and key
// help, the diff or branch comparison beside the list, and the branch picker.
'use strict';
const os = require('os');
const path = require('path');
const ui = require('../core/ui');
const diffs = require('../diff');
const icons = require('../core/icons');
const { WIN, readSettings, writeSettings } = require('../core/exec');
const { ROOT, C, fg, bg, BOLD, G, theme, SPLIT, VIEWS, shares, S, frame, late, wide, base, dirOf, flash, quit } = require('./context');
const {
  schedule, histName, renamed, allFiles, ongoing, continues, headName, openPicker, baseName, closeDiff, setView, toggleTree,
} = require('./state');
const { stage, unstage, discard, commit, abort, toggleAmend, generate, fetch, pull, push, newStash, openMenu } = require('./ops');

const CHEVRON = open => (open ? G.open : G.closed);
const key = k => late.key(k);
const listBar = new ui.Scrollbar(top => { S.top = top; draw(); });
const diffBar = new ui.Scrollbar(top => { if (S.diff) { S.diff.top = top; draw(); } });

// --- rows of the views

function changeRows() {
  const r = S.repo, out = [];
  for (const [sec, title, items] of [['conflicts', 'Merge Changes', r.conflicts], ['staged', 'Staged Changes', r.staged],
    ['changes', 'Changes', r.changes]]) {
    if (!items.length && sec !== 'changes') continue;
    const open = !S.collapsed.has(sec);
    out.push({ type: 'section', key: sec, sec, title, items, open });
    if (!open) continue;
    if (!items.length && !r.staged.length && !r.conflicts.length) out.push({ type: 'note', key: 'none', text: 'No changes', depth: 1 });
    else if (S.tree) treeRows(out, sec, items);
    else for (const it of items) out.push({ type: 'file', key: `${sec}:${it.path}`, sec, item: it, items: [it], depth: 1 });
  }
  return out;
}

// Folders of a section, single-child chains compacted as in VS Code (src/lib).
function treeRows(out, sec, items) {
  const node = () => ({ dirs: new Map(), files: [] });
  const root = node();
  for (const it of items) {
    let n = root;
    for (const part of it.path.split('/').slice(0, -1)) {
      if (!n.dirs.has(part)) n.dirs.set(part, node());
      n = n.dirs.get(part);
    }
    n.files.push(it);
  }
  const all = n => [...[...n.dirs.values()].flatMap(all), ...n.files];
  const walk = (n, prefix, depth) => {
    for (const [name, d] of [...n.dirs].sort(([a], [b]) => a.localeCompare(b))) {
      let label = name, dir = d, full = prefix + name;
      while (dir.dirs.size === 1 && !dir.files.length) {
        const [[k, v]] = dir.dirs;
        label += '/' + k; full += '/' + k; dir = v;
      }
      const key = `${sec}:${full}/`, open = !S.collapsed.has(key);
      out.push({ type: 'folder', key, sec, label, items: all(dir), depth, open });
      if (open) walk(dir, full + '/', depth + 1);
    }
    for (const it of n.files) out.push({ type: 'file', key: `${sec}:${it.path}`, sec, item: it, items: [it], depth });
  };
  walk(root, '', 1);
}

function refRows() {
  const R = S.refs, out = [];
  if (!R) return [{ type: 'note', key: 'loading', text: 'Loading…', depth: 1 }];
  const current = S.repo.branch, empty = [];
  // Empty sections are named together on one line at the end.
  const section = (key, title, list, row) => {
    if (!list.length) { empty.push(title.toLowerCase()); return; }
    const open = !S.collapsed.has(`refs:${key}`);
    out.push({ type: 'section', key: `refs:${key}`, title, items: list, open });
    if (open) list.forEach(x => out.push({ key: `${key}:${x.name || x.path}`, depth: 1, ...row(x) }));
  };
  section('heads', 'Local Branches', R.heads, b => ({ type: 'branch', ref: b, current: b.name === current }));
  section('remotes', 'Remote Branches', R.remotes, b => ({ type: 'branch', ref: b }));
  section('tags', 'Tags', R.tags, t => ({ type: 'tag', ref: t }));
  section('urls', 'Remotes', R.urls, u => ({ type: 'remote', ref: u }));
  section('worktrees', 'Worktrees', R.worktrees, w => ({ type: 'worktree', ref: w }));
  if (empty.length) {
    const names = empty.length > 1 ? `${empty.slice(0, -1).join(', ')} or ${empty[empty.length - 1]}` : empty[0];
    out.push({ type: 'note', key: 'empty', text: `No ${names}`, depth: 1 });
  }
  return out;
}

function viewRows() {
  if (!S.repo) return [];
  if (S.view === 'changes') return changeRows();
  if (S.view === 'history') {
    const authorW = Math.min(20, Math.max(0, ...S.history.map(c => ui.textWidth(c.author))));
    const graphW = Math.min(MAX_GRAPH, Math.max(1, ...S.history.map(c => c.graph.length)));
    return S.history.length ? S.history.map(c => ({ type: 'commit', key: c.hash, commit: c, authorW, graphW }))
      : [{ type: 'note', key: 'none', text: !S.repo.hasHead ? 'No commits yet' : S.histLoaded ? 'No commits for this file' : 'Loading…', depth: 1 }];
  }
  if (S.view === 'stashes') {
    return [{ type: 'action', key: 'new', text: '+ Stash all changes…  (n)', fn: newStash },
      ...S.stashes.map(s => ({ type: 'stash', key: s.ref, stash: s }))];
  }
  return refRows();
}

// --- drawing

const SELECT = () => (S.diff && S.diff.focus ? C.selectBlur : C.select);
const badgeColor = code => fg(ui.GIT[code] || C.text);
let KEY, DIM;
ui.onTheme(() => { KEY = BOLD + fg(C.text); DIM = fg(C.dim); });
const UNDERLINE = '\x1b[4m\x1b[58;2;55;148;255m'; // the active tab, underlined in blue where the terminal can
const PRIMARY = bg([14, 99, 156]) + fg([255, 255, 255]) + BOLD; // the main button of the header and the commit box
// A pill: its colour on a shade of it, close to the theme's background.
const shade = rgb => rgb.map((v, i) => Math.round(C.canvas[i] + (v - C.canvas[i]) * 0.25));
const PILLS = { heads: () => C.green, remotes: () => C.blue, tags: () => C.yellow };
const pill = (name, kind, style = '') => [` ${name} `, bg(shade(PILLS[kind]())) + fg(PILLS[kind]()) + style];
// Colours of the graph's lanes, in turn; a branch's pill takes the colour of its lane, on a shade of it.
const LANES = () => [C.blue, C.green, C.purple, C.orange, C.yellow, [86, 182, 194], [224, 108, 117]];
const laneRgb = n => LANES()[n % 7];
const lanePill = (name, n, style = '') => [` ${name} `, bg(shade(laneRgb(n))) + fg(laneRgb(n)) + style];
const MAX_GRAPH = 24; // columns of the graph at most: lanes further right are cut
const SPIN = '⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏';
const spinner = () => SPIN[Math.floor(Date.now() / 100) % SPIN.length];
const pad = (text, w) => { const t = ui.ellipsis(text, w); return t + ' '.repeat(Math.max(0, w - ui.textWidth(t))); };
// Windows paths ignore case, and git gives them with '/'.
const samePath = (a, b) => (WIN ? a.toLowerCase() === b.toLowerCase() : a === b);
const tilde = p => {
  const h = os.homedir(), abs = path.resolve(p);
  return samePath(abs, h) || samePath(abs.slice(0, h.length + 1), h + path.sep) ? `~${abs.slice(h.length)}` : p;
};

// A line made of a left part, cut to fit, and a right part kept whole (buttons, counts).
function split(left, right, width, back) {
  const rw = ui.textWidth(right.map(s => s[0]).join(''));
  const room = Math.max(0, width - rw);
  const segs = [];
  let used = 0;
  for (const [text, style] of left) {
    const t = ui.clean(text);
    if (used + ui.textWidth(t) > room) { segs.push([ui.ellipsis(t, room - used), style]); used = room; break; }
    segs.push([t, style]); used += ui.textWidth(t);
  }
  return ui.line([...segs, [' '.repeat(Math.max(0, room - used)), ''], ...right], width, back);
}

// Buttons [label, action, style] from column x, registered as clickable.
function place(list, y, x) {
  const segs = [];
  for (const [label, fn, style] of list) {
    frame.hits.push({ y, x0: x, x1: x + ui.textWidth(label) + 2, fn });
    segs.push([` ${label} `, style || KEY]);
    x += ui.textWidth(label) + 2;
  }
  return segs;
}

// Buttons shown on the right of a line, rightAfter columns before its end.
function buttons(list, y, width, rightAfter) {
  return place(list, y, width - rightAfter - list.reduce((n, [l]) => n + ui.textWidth(l) + 2, 0));
}

function rowLine(row, width, y, selected) {
  const back = selected ? SELECT() : S.pulse && S.pulse.has(row.key) ? C.pulse : null; // rows just staged or unstaged
  const indent = ' '.repeat(row.depth ? row.depth * 2 : 0);
  const act = selected && S.view === 'changes' ? rowButtons(row) : [];
  if (row.type === 'section') {
    return split([[` ${CHEVRON(row.open)} `, DIM], [row.title.toUpperCase(), BOLD], [`  ${row.items.length}`, DIM]],
      [...buttons(act, y, width, 1), [' ', '']], width, back);
  }
  if (row.type === 'folder') {
    // A chain of folders shown as one ("src/lib"): the icon of the last.
    const [mark, rgb] = icons.icon(theme, row.label.split('/').pop(), true, row.open);
    return split([[`${indent}${CHEVRON(row.open)} `, DIM], [mark, fg(rgb)], [row.label, '']],
      [...buttons(act, y, width, 3), [' ● ', badgeColor(rank(row.items))]], width, back);
  }
  if (row.type === 'file') {
    const it = row.item, color = badgeColor(it.code) + (it.code === 'D' ? '\x1b[9m' : '');
    const [mark, rgb] = icons.icon(theme, base(it.path), false);
    const name = S.tree ? base(it.path) : it.orig ? `${it.orig} → ${it.path}` : base(it.path);
    const dir = S.tree ? (it.orig ? ` ← ${base(it.orig)}` : '') : it.orig ? '' : dirOf(it.path);
    const letter = it.code === 'C' ? '!' : it.code;
    return split([[S.tree ? indent + '  ' : indent + ' ', ''], [mark, fg(rgb)], [name, color], [dir ? `  ${dir}` : '', DIM],
      [it.label ? `  ${it.label}` : '', DIM]], [...buttons(act, y, width, 3), [` ${letter} `, badgeColor(it.code)]], width, back);
  }
  if (row.type === 'commit') {
    // Subject first, so that subjects line up; author, age and hash in columns on the right.
    const c = row.commit;
    const pills = c.refs.flatMap((r, i) => [[i ? ' ' : '  ', ''],
      r.kind === 'tags' ? pill(r.name, 'tags') : lanePill(r.name, c.lane, c.head && r.kind === 'heads' ? BOLD : '')]);
    const author = row.authorW && width >= 90 ? [[` ${pad(c.author, row.authorW)} `, DIM]] : [];
    // The graph, cut at MAX_GRAPH columns and padded so that subjects line up; the commit checked out in bold.
    const cells = c.graph.slice(0, row.graphW).map(([ch, n]) => (ch === '●' ? [c.node, fg(laneRgb(n)) + (c.head ? BOLD : '')]
      : [ch, n === null ? '' : fg(laneRgb(n))]));
    while (cells.length < row.graphW) cells.push([' ', '']);
    return split([[' ', ''], ...cells, ['  ', ''], [c.subject, c.head ? BOLD : ''], ...pills],
      [...author, [`${c.date.padStart(6)}  `, DIM], [`${c.short} `, fg(C.yellow)]], width, back);
  }
  if (row.type === 'stash') {
    const s = row.stash;
    return split([[` ${s.ref}  `, fg(C.yellow)], [s.subject, '']], [[` ${s.date} `, DIM]], width, back);
  }
  if (row.type === 'branch') {
    const b = row.ref;
    if (row.current) {
      const track = b.upstream ? `${b.upstream}${b.track ? ` ${b.track}` : ''}` : 'not published';
      return split([[indent + ' ', ''], pill(b.name, 'heads', BOLD), [`  current · ${track}`, DIM], [`  ${b.subject}`, DIM]],
        [[` ${b.date} `, DIM]], width, back);
    }
    const track = b.upstream ? ` ${b.upstream}${b.track ? ' ' + b.track : ''}` : '';
    return split([[indent + '  ', ''], [b.name, ''], [track ? `  ${track}` : '', DIM], [`  ${b.subject}`, DIM]], [[` ${b.date} `, DIM]], width, back);
  }
  if (row.type === 'tag') return split([[indent + '  ', ''], [row.ref.name, fg(C.yellow)], [`  ${row.ref.subject}`, DIM]], [[` ${row.ref.date} `, DIM]], width, back);
  if (row.type === 'remote') return split([[indent + '  ', ''], [row.ref.name, BOLD], [`  ${row.ref.url}`, DIM]], [], width, back);
  if (row.type === 'worktree') {
    const w = row.ref, here = samePath(path.resolve(w.path), S.repo.top);
    return split([[indent + '  ', ''], [path.basename(w.path), ''], [here ? '  this folder' : '', DIM], [`  ${w.branch}`, fg(C.blue)],
      [`  ${tilde(w.path)}`, DIM]], [], width, back);
  }
  if (row.type === 'action') return split([[` ${row.text}`, fg(C.blue)]], [], width, back);
  return split([[`${indent}${row.text}`, DIM]], [], width, back);
}

const RANK = { U: 1, A: 2, R: 2, M: 3, D: 4, C: 5 };
const rank = items => items.reduce((best, i) => (RANK[i.code] > (RANK[best] || 0) ? i.code : best), 'M');

function rowButtons(row) {
  if (!row.items || !row.items.length) return [];
  if (row.sec === 'staged') return [['−', () => unstage(row.items)]];
  if (row.sec === 'conflicts') return [['+', () => stage(row.items)]];
  return [['↶', () => discard(row.items, row.sec)], ['+', () => stage(row.items)]];
}

// The header's main button: what the branch needs (publish, pull, push), none when it is up to date.
function syncButton(r, short) {
  const spin = (op, label) => (S.netOp === op ? `${spinner()}${label.slice(1)}` : label);
  if (r.detached || !r.hasHead) return null;
  if (!r.upstream) return [spin('push', short ? '↑ Publish' : '↑ Publish branch'), push, PRIMARY];
  if (r.behind) return [spin('pull', r.ahead ? `⇅ ↓${r.behind} ↑${r.ahead}` : `↓ Pull ${r.behind}`), pull, PRIMARY];
  if (r.ahead) return [spin('push', `↑ Push ${r.ahead}`), push, PRIMARY];
  return null;
}

// Header: repository, branch (opens the picker), its state against the upstream, and the buttons.
function header(W) {
  const r = S.repo, short = W < 80;
  const left = [[` ${path.basename(r ? r.top : ROOT)} `, BOLD]];
  const btns = [];
  if (r) {
    const name = r.detached ? `detached ${r.oid.slice(0, 7)}` : r.branch;
    const x0 = ui.textWidth(left[0][0]) + 1;
    const label = ` ${G.branch} ${name} `;
    frame.hits.push({ y: 0, x0, x1: x0 + ui.textWidth(label), fn: openPicker });
    left.push([' ', ''], [label, bg(shade(C.blue)) + fg(C.blue)]);
    if (r.upstream) left.push(r.ahead || r.behind ? [`  ↑${r.ahead} ↓${r.behind}`, fg(C.yellow)] : ['  up to date', DIM]);
    else if (!r.hasHead) left.push(['  no commits yet', DIM]);
    else if (!r.detached) left.push(['  not published', DIM]);
    if (r.state) left.push([`  ${r.state}`, fg(C.orange) + BOLD]);
    const main = syncButton(r, short);
    if (main) btns.push(main);
    const fetchLabel = S.netOp === 'fetch' ? spinner() : '⇣';
    btns.push([short ? fetchLabel : `${fetchLabel} Fetch`, fetch], ['⋯', () => openMenu(true)], ['↻', () => { flash('Refreshed'); schedule(); }]);
  }
  btns.push(['✕', quit]);
  return split(left, buttons(btns, 0, W, 0), W, C.header);
}

function tabs(W) {
  const segs = [], r = S.repo;
  let x = 0;
  VIEWS.forEach(([id, label]) => {
    const n = id === 'changes' ? r.conflicts.length + r.staged.length + r.changes.length : id === 'stashes' && S.stashes.length ? S.stashes.length : 0;
    const count = n ? ` ${n}` : '', w = ui.textWidth(label + count) + 2;
    frame.hits.push({ y: 1, x0: x, x1: x + w, fn: () => setView(id) });
    segs.push([' ', ''], [label, id === S.view ? BOLD + fg(C.text) + UNDERLINE : DIM], [count, DIM], ['  ', '']);
    x += w + 1;
  });
  // List · Tree, the active one filled (ui.segmented's look); a click on the other switches.
  const right = S.view === 'changes' ? buttons([['List', () => S.tree && toggleTree(), ui.switchStyle(!S.tree)],
    ['Tree', () => !S.tree && toggleTree(), ui.switchStyle(S.tree)]], 1, W, 0)
    : S.view === 'history' && S.histFile ? buttons([[`${ui.ellipsis(histName() + renamed(), Math.max(8, W - x - 8))} ✕`, allFiles, fg(C.blue)]], 1, W, 0) : [];
  return split(segs, right, W);
}

// What the commit button does: commit what is staged (or everything), amend, continue or conclude an operation.
function commitLabel(short) {
  const r = S.repo, o = ongoing();
  if (S.amend) return '✓ Amend';
  if (continues()) return short ? '✓ Continue' : `✓ Continue ${o.name}`;
  if (o) return short ? '✓ Commit' : `✓ Commit ${o.name}`;
  if (r.staged.length) return short ? '✓ Commit' : `✓ Commit ${r.staged.length} staged`;
  if (r.changes.length) return short ? '✓ Commit all' : `✓ Commit all ${r.changes.length}`;
  return '✓ Commit';
}

// Commit box, on two lines: the message field, then the commit button, ✧ (message written by Claude), the amend
// toggle and, during a merge, rebase, cherry-pick or revert, Abort.
function commitRows(W) {
  const o = ongoing(), fw = Math.max(4, W - 2);
  frame.hits.push({ y: 2, x0: 0, x1: W, fn: () => { S.editing = true; } });
  const box = bg(C.panel);
  const cut = text => ui.ellipsis(text, fw - 1);
  let field;
  if (S.generating) field = [[cut('✧ generating…'), fg(C.purple) + box]];
  else if (S.editing) field = S.field.segments(fw - 1, box + fg(C.text));
  else if (!S.field.value) {
    const text = S.amend ? 'Message (empty: keep the last one)'
      : o && S.repo.state === 'REBASING' ? 'Rebase in progress: continue, or X to abort'
      : o ? 'Message (empty: the one git prepared)' : 'Message  (c)';
    field = [[cut(text), DIM + box]];
  } else field = [[cut(S.field.value), fg(C.text) + box]];
  const used = ui.textWidth(field.map(s => s[0]).join(''));
  const fieldLine = ui.line([[' ', ''], [' ', box], ...field, [' '.repeat(Math.max(0, fw - 1 - used)), box]], W);
  const list = short => [[commitLabel(short), commit, PRIMARY], [short ? '✧' : '✧ Claude', generate, fg(C.purple)],
    [S.amend ? '☑ Amend' : '☐ Amend', toggleAmend, S.amend ? fg(C.text) : DIM], ...(o ? [['✕ Abort', abort, fg(C.red)]] : [])];
  const fits = l => l.reduce((n, [t]) => n + ui.textWidth(t) + 2, 1) <= W;
  const btns = fits(list(false)) ? list(false) : list(true);
  return [fieldLine, ui.line([[' ', ''], ...place(btns, 3, 1)], W)];
}

function status(W) {
  if (S.busy) return ui.line([[` ${spinner()} ${S.busy}…`, fg(C.blue)]], W, C.header);
  if (S.generating) return ui.line([[' ✧ generating…', fg(C.purple)]], W, C.header);
  if (S.flash) return ui.line(ui.message(S.flash.text, S.flash.error ? 'error' : undefined), W, C.header);
  // ? opens the full list wherever a key is not typed into a field.
  const right = S.picker || S.inline || S.menu || S.editing || !S.repo ? [] : [['?', KEY], [W >= 60 ? ' all keys ' : ' ', DIM]];
  const rw = ui.segWidth(right);
  const segs = ui.keys(hints(), W - rw - 2);
  // A click on a hint does what its key does.
  const y = ui.size().H - 1;
  for (const z of segs.zones) frame.hits.push({ y, x0: z.x0, x1: z.x1, fn: () => key(z.key) });
  if (rw) frame.hits.push({ y, x0: W - rw, x1: W, fn: () => key('?') });
  return ui.line([...segs, [' '.repeat(Math.max(0, W - ui.segWidth(segs) - rw)), ''], ...right], W, C.header);
}

// The keys of the moment, most useful first: as many as fit show in the status line.
function hints() {
  if (S.inline) return S.inline.keys;
  if (S.menu) return [['↑↓', 'move'], ['⏎', 'run'], ['esc', 'close']];
  if (S.picker && S.picker.mode) return [['↑↓', 'move'], ['⏎', S.picker.mode], ['/', 'filter'], ['esc', S.picker.typing ? 'clear' : 'close']];
  if (S.picker) return S.picker.typing ? [['↑↓', 'move'], ['⏎', 'switch'], ['esc', 'clear']]
    : [['⏎', 'switch'], ['n', 'new branch'], ['D', 'delete'], ['/', 'filter'], ['esc', 'close']];
  const commitKey = continues() ? 'continue' : 'commit';
  if (S.editing) return [['⏎', commitKey], ['esc', 'leave'], ['ctrl+g', '✧ Claude'], ['alt+a', 'amend']];
  if (S.diff && S.diff.compare && (S.diff.focus || !wide())) return [['↑↓', 'select'], ['⏎', 'show commit'], ['tab', 'list'], ['q', 'back']];
  if (S.diff && S.diff.back && (S.diff.focus || !wide())) return [['↑↓', 'scroll'], ['n N', 'hunks'], ['q', 'back to the comparison']];
  if (S.diff && (S.diff.focus || !wide())) return [['↑↓', 'scroll'], ['n N', 'hunks'], ['tab', 'list'], ['q', 'back']];
  if (!S.repo) return S.noGit ? [['q', 'quit']] : [['i', 'git init'], ['q', 'quit']];
  const common = [['b', 'branch'], ['f', 'fetch'], ['P', 'push'], ['L', 'pull'], ['m', 'more'], ['q', 'quit']];
  if (S.view === 'changes') {
    return [['space', 'stage'], ['u', 'unstage'], ['d', 'discard'], ['c', commitKey], ...(ongoing() ? [['X', 'abort']] : []),
      ['⏎', wide() ? 'focus diff' : 'diff'], ['ctrl+g', '✧ Claude'], ...common];
  }
  if (S.view === 'history') {
    return [['⏎', wide() ? 'focus commit' : 'show commit'], ...(S.histFile ? [['x', 'every file']] : []),
      ['a', S.allBranches ? 'current branch' : 'all branches'], ['t', 'tag'], ['C', 'cherry-pick'], ...common];
  }
  if (S.view === 'stashes') return [['n', 'stash'], ['a', 'apply'], ['o', 'pop'], ['d', 'drop'], ['⏎', 'show'], ...common];
  return [['⏎', 'checkout'], ['M', 'merge'], ['R', 'rebase onto'], ['=', 'compare'], ['B', 'compare base'], ['D', 'delete branch'], ...common];
}

// Every key, for the help overlay (?): the view's own first.
function helpGroups() {
  const g = [];
  if (S.view === 'changes') {
    g.push(['Files', [['space s', 'stage'], ['u', 'unstage'], ['a A', 'stage, unstage all'], ['d', 'discard'], ['t', 'list or tree'],
      ['y', 'copy path'], ['Y', 'copy relative path'], ['e', 'edit in the terminal'], ['o', 'open in the editor']]],
      ['Commit', [['c', 'type the message'], ['⏎', 'commit, when typing'], ['ctrl+g', '✧ write with Claude'], ['alt+a', 'amend'],
        ['X', 'abort the operation']]]);
  }
  if (S.view === 'history') {
    g.push(['History', [['⏎', 'show the commit'], ['a', 'all branches or current'], ['x', 'every file, after a file history'],
      ['t', 'tag the commit'], ['C', 'cherry-pick the commit'], ['m', 'more: reset --hard to the commit']]]);
  }
  if (S.view === 'stashes') g.push(['Stashes', [['n', 'stash all changes'], ['a', 'apply'], ['o', 'pop'], ['d', 'drop'], ['⏎', 'show']]]);
  if (S.view === 'refs') {
    g.push(['Refs', [['⏎', 'check out'], ['M', 'merge into the current branch'], ['R', 'rebase the current branch onto it'],
      ['=', 'compare with the base, in the diffs of the tab too'], ['B', 'set as compare base'], ['D', 'delete the branch']]],
      ['Comparison', [['↑↓', 'select a commit'], ['⏎', 'show the commit'], ['q', 'back']]]);
  }
  g.push(['Repository', [['f', 'fetch'], ['L', 'pull'], ['P', 'push or publish'], ['b', 'branches'], ['T', 'tag HEAD'],
    ['m', 'more: merge, rebase, force push, undo commit, reset --hard'], ['r', 'refresh']]],
    ['Move', [['↑↓ j k', 'move'], ['← → h l', 'fold, unfold'], ['pgup pgdn', 'page'], ['ctrl+u/d', 'half page'], ['g G', 'first, last'],
      ['1-4', 'switch view']]],
    ['Diff', [['tab', 'focus the diff'], ['n N', 'next, previous hunk'], ['v', 'inline, split (side by side)'], ['< >', 'narrower, wider list'], ['shift+← →', 'move the separator'],
      ['q', 'back to the list']]],
    ['Pane', [['ctrl+p', 'go to file'], ['ctrl+f', 'search in files'], ['? f1', 'keys'], ['q esc', 'close']]]);
  return g;
}

// The diff area: a title row and the rows of diff.js, rendered for its width and cached.
function diffLines(width, height) {
  const d = S.diff;
  if (d.compare) return compareLines(d, width, height);
  const mode = diffs.layoutOf(width);
  // While the separator is dragged, the rows laid out for the old width are cut or padded: laying out a long
  // diff again at each column would make the separator lag behind the mouse. The release lays it out.
  const layout = w => {
    if (S.resizing && d.rows && d.rows.length) return;
    if (d.parsed && d.cache !== `${w}:${mode}`) { d.rows = diffs.render(d.parsed, w, mode, !!d.item); d.cache = `${w}:${mode}`; }
  };
  const body = height - 1;
  // One column narrower when the scrollbar shows (a narrower layout never has fewer rows): kept while it does.
  if (d.cache !== `${width - 1}:${mode}` || d.rows.length <= body) layout(width);
  if ((d.rows || []).length > body) layout(width - 1);
  d.top = Math.max(0, Math.min(d.top, (d.rows || []).length - body));
  const bw = diffBar.set((d.rows || []).length, body, d.top);
  const counts = d.parsed ? [[`  +${d.parsed.add}`, fg(C.green)], [` −${d.parsed.del}`, fg(ui.GIT.D)]] : [];
  // The layout switch as in the Changes pane, the active choice filled (clicks: see switchHits, d.switchAt).
  const sw = ui.segmented([['inline', 'Inline'], ['split', 'Split']], mode);
  const right = [...sw.segs, [wide() ? ' ' : '  ✕ ', fg(C.text)]];
  d.switchAt = width - ui.segWidth(right); d.switchHits = sw.hits; d.width = width;
  const lines = [split([[` ${d.title}`, BOLD], [d.tag ? `  ${d.tag}` : '', DIM], ...counts], right, width, d.focus ? C.select : C.header)];
  if (!d.parsed) lines.push(...ui.empty(width, body, 'Loading…'));
  else if (!d.rows.length) lines.push(...ui.empty(width, body, 'No changes'));
  (d.rows || []).slice(d.top, d.top + body).forEach((r, k) => lines.push(ui.line(r.segs, width - bw, r.back) + diffBar.cell(k)));
  while (lines.length < height) lines.push(ui.line([], width));
  return lines.slice(0, height);
}

// The diff's layout switch, its title row's Inline and Split, clickable; x: the diff area's first column.
function switchHits(x) {
  const d = S.diff;
  if (!d || d.compare || d.switchAt === undefined) return;
  const set = mode => () => { if (diffs.layoutOf(d.width) !== mode) { diffs.saveMode(d.width, mode); draw(); } };
  for (const h of d.switchHits) frame.hits.push({ y: 1, x0: x + d.switchAt + h.from, x1: x + d.switchAt + h.to + 1, fn: set(h.id) });
}

// A comparison in the diff area: its title row, then its rows (selected commit highlighted).
function compareLines(d, width, height) {
  const body = height - 1;
  if (!S.resizing && d.cache !== `${width}`) { d.rows = compareRows(d, width - 1); d.cache = `${width}`; }
  d.top = Math.max(0, Math.min(d.top, d.rows.length - body));
  const bw = diffBar.set(d.rows.length, body, d.top);
  const c = d.cmp;
  const state = c && !c.failed ? [[`${c.ahead} ahead · ${c.behind} behind `, c.ahead || c.behind ? fg(C.yellow) : DIM]] : [];
  const lines = [split([[` ${d.title}`, BOLD], ['  ↔  ', DIM], [d.base, BOLD]], state, width, d.focus ? C.select : C.header)];
  if (!c) lines.push(...ui.empty(width, body, 'Loading…'));
  d.rows.slice(d.top, d.top + body).forEach((r, k) => {
    const sel = d.top + k === d.sel ? (d.focus ? C.select : C.selectBlur) : r.back;
    lines.push(ui.line(r.segs, width - bw, sel) + diffBar.cell(k));
  });
  while (lines.length < height) lines.push(ui.line([], width));
  return lines.slice(0, height);
}

// The rows of a comparison for a width; the commit rows can be selected (Enter shows them).
function compareRows(d, width) {
  const c = d.cmp, out = [];
  if (!c) return out;
  if (c.failed) return [{ segs: [[` ${c.failed}`, fg(C.red)]] }];
  const section = (title, n) => out.push({ segs: [[` ${title}`, BOLD], [`  ${n}`, DIM]], back: C.section });
  const commits = (list, total) => {
    if (!list.length) out.push({ segs: [['   none', DIM]] });
    for (const x of list) {
      const date = ` ${x.date.padStart(6)} `, room = Math.max(1, width - date.length - 13);
      out.push({ commit: x, segs: [[x.same ? '   = ' : '     ', DIM], [`${x.short}  `, fg(C.yellow)],
        [pad(x.subject, room), x.same ? DIM : ''], [date, DIM]] });
    }
    if (total > list.length) out.push({ segs: [[`   … and ${total - list.length} more`, DIM]] });
    out.push({ segs: [] });
  };
  section(`Only on ${d.title}`, c.ahead);
  commits(c.mine, c.ahead);
  section(`Only on ${d.base}`, c.behind);
  commits(c.theirs, c.behind);
  const n = v => (v === '-' ? 0 : +v);
  section('Files changed', `${c.files.length}  +${c.files.reduce((t, f) => t + n(f.add), 0)} −${c.files.reduce((t, f) => t + n(f.del), 0)}`);
  if (!c.files.length) out.push({ segs: [['   none', DIM]] });
  for (const f of c.files) {
    const counts = f.add === '-' ? ' binary ' : ` +${f.add} −${f.del} `;
    out.push({ segs: [['   ', ''], [pad(f.path, Math.max(1, width - counts.length - 3)), ''],
      ...(f.add === '-' ? [[counts, DIM]] : [[` +${f.add}`, fg(C.green)], [` −${f.del} `, fg(ui.GIT.D)]])] });
  }
  return out;
}

// The diff area with nothing to show: what to select for it.
function emptyDiff(width, height) {
  const rows = frame.rows;
  const text = S.view === 'changes' ? (rows.some(r => r.type === 'file') ? 'Select a file to see its changes' : 'No changes')
    : S.view === 'history' ? 'Select a commit to see it'
    : S.view === 'stashes' ? (S.stashes.length ? 'Select a stash to see it' : 'No stashes')
    : rows[S.cursor] && rows[S.cursor].type === 'branch' ? `${baseName()} is the base: select another branch to compare with it`
    : `Select a branch to compare it with ${baseName()}`;
  return [ui.line([], width, C.header), ...ui.empty(width, height - 1, text)];
}

// Columns of the list beside the diff: at least 30, and 40 left to the diff.
const MIN_LIST = 30, MIN_DIFF = 40;
function listWidth(W) {
  const share = S.listShare[S.view];
  const w = share === undefined ? Math.max(34, Math.min(70, Math.round(W * 0.38))) : Math.round(W * share);
  return Math.max(MIN_LIST, Math.min(W - 1 - MIN_DIFF, w));
}

// Sets the list's width in columns and keeps it as a share of the width; saved unless a drag is still going on.
function resizeList(cols, save = true) {
  const { W } = ui.size();
  S.listShare[S.view] = Math.round(Math.max(MIN_LIST, Math.min(W - 1 - MIN_DIFF, cols)) / W * 1000) / 1000;
  if (save) writeSettings({ ...readSettings(), scmListShare: { ...shares(readSettings().scmListShare), [S.view]: S.listShare[S.view] } });
  draw();
}

function draw() {
  const paint = late.paint;
  if (!paint) return;
  const { W, H } = ui.size();
  frame.hits = [];
  const lines = [header(W)];
  if (!S.repo) {
    lines.push(...(S.repo === null ? ui.empty(W, H - 2, 'Loading…') : S.noGit ? ui.empty(W, H - 2, 'git not found in PATH')
      : ui.empty(W, H - 2, 'Not a git repository', [['i', 'git init here']])));
  } else if (S.diff && W < SPLIT) {
    lines.push(...diffLines(W, H - 2));
    switchHits(0);
    frame.hits.push({ y: 1, x0: W - 3, x1: W, fn: closeDiff });
    frame.listW = 0;
  } else {
    // From SPLIT columns, the diff of the selection always shows on the right.
    const LW = W >= SPLIT ? listWidth(W) : W;
    const left = [tabs(LW)];
    if (S.view === 'changes') {
      left.push(...commitRows(LW));
      if (H >= 20) left.push(ui.line([], LW));
    }
    frame.listY = 1 + left.length; frame.listH = Math.max(1, H - 1 - frame.listY); frame.listW = LW;
    const rows = frame.rows = viewRows(), listY = frame.listY, listH = frame.listH;
    syncCursor();
    S.top = Math.max(0, Math.min(S.top, rows.length - listH));
    const bw = listBar.set(rows.length, listH, S.top);
    // A list with nothing in it but a note: the sections' titles, then the note centred in the rest (ui.empty).
    const notes = rows.filter(r => r.type === 'note');
    const lone = notes.length === 1 && rows.every(r => r.type === 'note' || r.type === 'section') ? notes[0] : null;
    for (let i = S.top; i < Math.min(rows.length, S.top + listH); i++) {
      if (rows[i] === lone) continue;
      left.push(rowLine(rows[i], LW - bw, listY + i - S.top, i === S.cursor) + listBar.cell(i - S.top));
    }
    if (lone) left.push(...ui.empty(LW, Math.max(1, H - 2 - left.length), lone.text, lone.text === 'No changes' ? [['ctrl+p', 'go to file'], ['2', 'history']] : []));
    // The stash or the tag being named: a row above the list.
    if (S.inline && LEADS[S.inline.kind]) { left.splice(listY - 1, 0, inlineRow(LW, LEADS[S.inline.kind], null)); left.length = Math.min(left.length, H - 2); }
    while (left.length < H - 2) left.push(ui.line([], LW));
    if (LW < W) {
      // The diff stays while there is room for it: no close button.
      const right = S.diff ? diffLines(W - LW - 1, H - 2) : emptyDiff(W - LW - 1, H - 2);
      if (S.diff) switchHits(LW + 1);
      const bar = (S.resizing ? fg(C.blue) : DIM) + '│\x1b[0m';
      left.forEach((l, i) => lines.push(l + bar + right[i]));
    } else lines.push(...left);
  }
  if (S.picker) drawPicker(lines, W, H);
  while (lines.length < H - 1) lines.push(ui.line([], W));
  lines.length = H - 1;
  lines.push(status(W));
  if (S.menu) S.menu.draw(lines, W, H);
  if (S.confirm) ui.confirm(lines, W, H, S.confirm);
  if (S.help) ui.help(lines, W, (VIEWS.find(v => v[0] === S.view) || [])[1], helpGroups(), S.help);
  paint(lines);
}

// The selection follows its row (same file, commit or branch) when the rows change.
function syncCursor() {
  const rows = frame.rows;
  if (S.selKey !== null && (!rows[S.cursor] || rows[S.cursor].key !== S.selKey)) {
    const i = rows.findIndex(r => r.key === S.selKey);
    if (i >= 0) S.cursor = i;
  }
  S.cursor = Math.max(0, Math.min(S.cursor, rows.length - 1));
  S.selKey = rows[S.cursor] ? rows[S.cursor].key : null;
}

// --- branch picker

function drawPicker(lines, W, H) {
  const p = S.picker;
  const w = Math.min(W - 2, 72), h = Math.max(5, Math.min(H - 4, p.items.length + 4));
  const x = Math.floor((W - w) / 2), y = 2, body = h - 4;
  if (p.sel < p.top) p.top = p.sel;
  if (p.sel >= p.top + body) p.top = p.sel - body + 1;
  p.box = { x, y, w, h, body };
  const box = [];
  const title = p.mode === 'merge' ? ` Merge into ${headName()}` : p.mode === 'rebase' ? ` Rebase ${headName()} onto` : ' Switch branch';
  box.push(ui.line([[title, BOLD], ['  (local, then remote)', DIM]], w, C.header));
  box.push(ui.line([[` ${G.search} `, DIM], ...(p.typing ? p.field.segments(w - 4) : [[p.field.value || 'type / to filter', DIM]])], w, C.panel));
  for (let i = p.top; i < p.top + body; i++) {
    const b = p.items[i];
    if (!b) { box.push(ui.line([], w, C.menu)); continue; }
    const back = i === p.sel ? C.select : C.menu;
    if (b.create && S.inline && S.inline.kind === 'branch') box.push(inlineRow(w, ' + ', C.menu));
    else if (b.create) box.push(ui.line([[' + ', fg(C.green)], [b.name, fg(C.green)]], w, back));
    else {
      box.push(split([[b.current ? ' ● ' : '   ', fg(C.green)], [b.name, b.remote ? fg(C.purple) : b.current ? BOLD : ''],
        [`  ${b.subject}`, DIM]], [[` ${b.date} `, DIM]], w, back));
    }
  }
  const count = p.items.filter(b => !b.create).length;
  box.push(ui.line([[` ${count} branch${count === 1 ? '' : 'es'}`, DIM]], w, C.header));
  box.push(ui.line([['─'.repeat(w), DIM]], w));
  // Rows under the box are cleared on both sides: styled lines cannot be cut at a column.
  box.slice(0, h).forEach((l, i) => { if (y + i < H - 1) lines[y + i] = ' '.repeat(x) + l + ' '.repeat(W - x - w); });
}

// What the row of a name typed above the list starts with, by kind (the branch's is in the picker).
const LEADS = { stash: '  new        ', tag: '  tag        ', message: '  message    ' };
// The row of the name being typed, width w, on back.
function inlineRow(w, lead, back) {
  const it = S.inline, right = ` ${it.right} `, FIELD = C.field;
  const fw = Math.max(4, w - ui.textWidth(lead) - ui.textWidth(right) - 1);
  const field = it.field.segments(fw - 1).map(([t, st]) => [t, bg(FIELD) + (st || '')]);
  return ui.line([[lead, DIM], [' ', bg(FIELD)], ...field, [' '.repeat(Math.max(0, fw - 1 - ui.segWidth(field))), bg(FIELD)],
    [' '.repeat(Math.max(1, w - ui.textWidth(lead) - fw - ui.textWidth(right))), ''], [right, DIM]], w, back);
}

module.exports = { draw, listBar, diffBar, resizeList };
