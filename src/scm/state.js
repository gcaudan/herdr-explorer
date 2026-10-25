// State of the source control pane: git run in the repository, the loaders that fill S (status, history, stashes,
// refs, the branch picker, diffs and branch comparisons), the refresh, and the view switches.
'use strict';
const fs = require('fs');
const path = require('path');
const ui = require('../core/ui');
const diffs = require('../diff');
const actions = require('../core/actions');
const Scope = require('../core/scope');
const graph = require('../render/graph');
const { WIN, GIT, gitEnv, gitOperation, readSettings, writeSettings, realPath, addConfig, exec } = require('../core/exec');
const parse = require('./parse');
const { errorOf } = parse;
const { ROOT, REVIEW, NET_TIMEOUT, S, frame, late, draw, select, follow, base, flash } = require('./context');

// --- running git

// Config keys that run a program and that the repository could set in its own config: for the commands that
// write (commit, switch, stash, fetch, pull, push), each one the repository sets is replaced by the user's
// global value or the default. Hooks, fsmonitor and filters are already disabled by gitEnv.
const RISKY = '^(core\\.(sshcommand|askpass|gitproxy)|gpg\\.(.+\\.)?program|gpg\\.ssh\\.defaultkeycommand|credential\\.(.+\\.)?helper'
  + '|remote\\..+\\.(uploadpack|receivepack)|merge\\..+\\.driver)$';
const DEFAULTS = { 'core.sshcommand': 'ssh', 'core.askpass': '', 'gpg.program': 'gpg', 'gpg.openpgp.program': 'gpg',
  'gpg.x509.program': 'gpgsm', 'gpg.ssh.program': 'ssh-keygen', 'gpg.ssh.defaultkeycommand': '' };

// Returns { env, pack }: pack, the command-line options replacing upload-pack and receive-pack programs set by
// the repository (the first value given wins for these, so the config cannot override them).
async function safeEnv(cwd) {
  const env = await gitEnv(cwd);
  const res = await exec(GIT, ['config', '-z', '--includes', '--show-scope', '--get-regexp', RISKY], { cwd, env });
  const keys = parse.parseScopes(res.out);
  // ext:: URLs run a command; a local path or file:// remote runs git-receive-pack on that repository, with its
  // hooks, and git leaves out the environment config (core.hooksPath) for a local transport.
  const extra = [['protocol.ext.allow', 'never'], ['protocol.file.allow', 'never']];
  const pack = { fetch: [], push: [] };
  for (const [key, list] of keys) {
    if (list.every(([scope]) => scope === 'system' || scope === 'global' || scope === 'command')) continue;
    const user = list.filter(([scope]) => scope === 'system' || scope === 'global').map(([, v]) => v);
    if (key.endsWith('.uploadpack')) pack.fetch = ['--upload-pack=git-upload-pack'];
    else if (key.endsWith('.receivepack')) pack.push = ['--receive-pack=git-receive-pack'];
    // The first core.gitProxy wins too, but the variable takes precedence over the config.
    else if (key === 'core.gitproxy') env.GIT_PROXY_COMMAND = '';
    // credential helpers form a list, reset by an empty value.
    else if (key.endsWith('helper')) extra.push([key, ''], ...user.map(v => [key, v]));
    else extra.push([key, user.length ? user[user.length - 1] : key in DEFAULTS ? DEFAULTS[key] : 'git merge-file %A %O %B']);
  }
  addConfig(env, extra);
  return { env, pack };
}

const fromFile = ['--pathspec-from-file=-', '--pathspec-file-nul']; // paths on stdin: no command-line length limit

// Messages in English, like the pane, and recognisable (not fully merged); the character set is kept.
function english(env) {
  if (env.LC_ALL) { env.LC_CTYPE = env.LC_ALL; delete env.LC_ALL; }
  delete env.LANGUAGE;
  env.LC_MESSAGES = 'C';
  return env;
}

// git in the repository. The paths given are literal pathspecs: a file named '*.js' or ':x' means itself (not
// for the other commands: stash -u relies on pathspec magic internally). opts.safe: the hardened environment,
// for the commands that write. opts.config: [key, value] pairs, after the repository's config, as GIT_CONFIG_KEY_n
// (-c would split a key holding '=', which a branch name can).
// In the repository loaded, or opts.top.
async function git(args, opts = {}) {
  const cwd = opts.top || (S.repo && S.repo.top) || ROOT;
  const safe = opts.safe ? await safeEnv(cwd) : null;
  const literal = args.includes('--') || args.includes(fromFile[0]) ? { GIT_LITERAL_PATHSPECS: '1' } : {};
  const env = english({ ...(safe ? safe.env : await gitEnv(cwd)), ...literal, ...opts.env });
  addConfig(env, opts.config || []);
  const pack = safe ? (args[0] === 'push' ? safe.pack.push : /^(fetch|pull)$/.test(args[0]) ? safe.pack.fetch : []) : [];
  return exec(GIT, ['-c', 'core.quotePath=false', args[0], ...pack, ...args.slice(1)], { ...opts, cwd, env });
}

// A git command that changes something: shown as running, then its error, if any, in the status line.
async function op(label, args, opts = {}) {
  if (S.busy) { flash(`Busy: ${S.busy}`, true); return null; }
  S.busy = label; draw();
  const spin = setInterval(draw, 100); // the spinner of the status line and of the header's button
  const res = await git(args, { safe: true, ...opts });
  clearInterval(spin);
  S.busy = null;
  if (res.code !== 0) { if (!opts.quiet) flash(`${label} failed: ${errorOf(res)}`, true); schedule(); return opts.quiet ? res : null; }
  schedule();
  return res;
}
// Network settings the repository could set in its own config to send the user's credentials elsewhere (another
// address or proxy, a certificate of its own, no TLS check) or to rewrite the remote URL; and
// core.alternateRefsCommand, a shell command git runs while negotiating a fetch; remote.<name>.vcs, a remote helper
// program that can be a relative path into the repository.
const NETWORK = '^(http\\..+|url\\..+\\.(insteadof|pushinsteadof)|remote\\..+\\.(proxy|vcs)|core\\.alternaterefscommand)$';
async function repoNetworkKeys() {
  const res = await git(['config', '-z', '--includes', '--show-scope', '--get-regexp', NETWORK]);
  return parse.repoKeys(res.out);
}

// fetch, pull and push: no password prompt in the terminal, and a time limit. Refused when the repository sets
// network settings itself: the user runs them in the terminal after checking them.
async function net(label, args) {
  const keys = await repoNetworkKeys();
  if (keys.length) { flash(`${label.split(' ')[0]} refused: the repository sets ${keys[0]}; use the terminal`, true); return null; }
  // Only the usual transports: no remote helper (git-remote-<name>), local path or ext:: command.
  if (S.busy) { flash(`Busy: ${S.busy}`, true); return null; }
  S.netOp = args[0];
  const res = await op(label, args, { detached: true, timeout: NET_TIMEOUT,
    env: { GIT_TERMINAL_PROMPT: '0', GIT_ALLOW_PROTOCOL: 'https:http:ssh:git' } });
  S.netOp = null;
  // On Windows, ssh may still have asked for a passphrase or a host key on the pane's console, over the screen.
  late.paint.again();
  draw();
  return res;
}
const nul = items => items.flatMap(i => (i.orig ? [i.orig, i.path] : [i.path])).join('\0') + '\0';

// --- repository state

async function loadStatus() {
  const r = S.repo || {};
  const res = await git(['status', '--porcelain=v2', '-z', '--branch', '--ignore-submodules=all', '-uall']);
  if (res.code !== 0) return false;
  const info = { top: r.top, gitdir: r.gitdir, common: r.common, ...parse.parseStatus(res.out) };
  info.state = gitOperation(info.gitdir);
  S.repo = info;
  return true;
}

async function loadHistory() {
  // One file: every name it had (--follow sees its renames, but on one line of commits only, without the graph), whose
  // commits the log below then lists.
  if (S.histFile) {
    const name = histName();
    const res = await git(['log', '--follow', '--name-only', '-z', '--format=', '-n', '2000', 'HEAD', '--', name]);
    const names = res.code === 0 ? parse.parseNames(res.out) : [];
    S.histNames = [...new Set([name, ...names])];
  }
  // Children before their parents, by date, as the graph needs. For one file, --parents links each commit to the
  // previous one that changed it, so that the graph joins them up.
  const revs = S.allBranches ? [...(S.repo.hasHead ? ['HEAD'] : []), '--branches', '--remotes'] : [];
  const [res, fork] = await Promise.all([git(['log', '--no-show-signature', '--decorate=full', '--date-order', '-n', '500',
    '--format=%H%x1f%h%x1f%s%x1f%an%x1f%at%x1f%D%x1f%P', ...(S.histFile ? ['--parents'] : []), ...revs, '--', ...(S.histFile ? S.histNames : [])]),
  forkPoint()]);
  S.history = res.code !== 0 ? [] : parse.parseHistory(res.out);
  graph.layout(S.history, fork).forEach((g, i) => Object.assign(S.history[i], { graph: g.cells, lane: g.color, node: node(g) }));
  S.histLoaded = true;
}

// Where the current branch leaves the default branch (diffs.defaultBranch, the Diff view's main base): the history's
// graph changes colour there. null on the default branch, or a branch it already has.
async function forkPoint() {
  if (!S.repo.hasHead) return null;
  const out = async args => { const r = await git(args); return r.code === 0 ? r.out.trim() : ''; };
  const name = await diffs.defaultBranch(out);
  if (!name) return null;
  const [fork, head] = await Promise.all([out(['merge-base', 'HEAD', name]), out(['rev-parse', 'HEAD'])]);
  return fork && fork !== head ? fork : null;
}

// The file whose history is shown, relative to the repository (git runs there), through symbolic links (/home and
// /var/home).
const histName = () => path.relative(realPath(S.repo.top), realPath(S.histFile)).split(path.sep).join('/');
// The file's former names, shown after its name: " ← old.js", " ← 2 names".
const renamed = () => (S.histNames.length < 2 ? '' : S.histNames.length === 2 ? ` ← ${path.posix.basename(S.histNames[1])}` : ` ← ${S.histNames.length - 1} names`);
// Back to the history of every file.
function allFiles() {
  S.histFile = null; S.histNames = []; S.histLoaded = false; S.cursor = 0; S.top = 0; S.selKey = null; S.diff = null;
  flash('History of every file');
  schedule();
}

// The commit in the graph: its lane crossed by a heavy bar. Box-drawing characters reach the top and bottom of the
// cell in every terminal, so the commits of a lane join up from one row to the next (● and the Nerd Font circles
// do not, where the terminal keeps glyphs at their size). A branch tip has no line above, the first commit none
// below.
const node = g => (g.above && g.below ? '┿' : g.below ? '┯' : g.above ? '┷' : '━');

async function loadStashes() {
  const res = await git(['stash', 'list', '--format=%gd%x1f%s%x1f%cr']);
  S.stashes = res.code !== 0 ? [] : parse.parseStashes(res.out);
}

async function loadRefs() {
  const [refs, remotes, trees] = await Promise.all([
    git(['for-each-ref', '--format=%(refname)%1f%(objectname:short)%1f%(upstream:short)%1f%(upstream:track)%1f%(committerdate:relative)%1f%(contents:subject)',
      'refs/heads', 'refs/remotes', 'refs/tags']),
    git(['remote', '-v']), git(['worktree', 'list', '--porcelain']),
  ]);
  S.refs = parse.parseRefs(refs.out, remotes.out, trees.out);
}

// The repository is looked up again while there is none (git init, clone in the folder).
let watching = false;
async function load() {
  if (!GIT) { S.repo = false; S.noGit = true; return; }
  if (!S.repo) {
    const res = await exec(GIT, ['rev-parse', '--show-toplevel', '--absolute-git-dir', '--git-common-dir'], { cwd: ROOT, env: await gitEnv(ROOT) });
    if (res.code === null && /ENOENT|not found/.test(res.err)) { S.repo = false; S.noGit = true; return; }
    const repo = res.code === 0 ? parse.parseRepo(res.out, ROOT) : null;
    if (!repo) { S.repo = false; return; }
    S.repo = repo;
    watch();
  }
  if (!await loadStatus()) { S.repo = false; return; }
  if (S.view === 'history') await loadHistory();
  else if (S.view === 'stashes') await loadStashes();
  else if (S.view === 'refs') await loadRefs();
  if (S.picker) await loadPicker();
  // The open diff follows its file, staged or not since.
  if (S.diff && S.diff.item) {
    const { path: p, sec } = S.diff.item, r = S.repo;
    const it = [...r[sec], ...r.conflicts, ...r.staged, ...r.changes].find(i => i.path === p);
    if (it) { S.diff.key = `${it.sec}:${it.path}`; await openItemDiff(it, true); }
  }
}

// Commits, staging, branch switches and fetches change files of the git folder, not the working tree.
function watch() {
  if (watching) return;
  watching = true;
  const { gitdir, common } = S.repo;
  const names = /^(index|HEAD|ORIG_HEAD|FETCH_HEAD|MERGE_HEAD|packed-refs|stash|CHERRY_PICK_HEAD|REVERT_HEAD)$/;
  try { fs.watch(gitdir, (ev, f) => { if (f && names.test(String(f))) schedule(); }).on('error', () => { }); } catch { }
  if (common !== gitdir) try { fs.watch(common, (ev, f) => { if (f && names.test(String(f))) schedule(); }).on('error', () => { }); } catch { }
  try {
    fs.watch(path.join(common, 'refs'), { recursive: true }, (ev, f) => { if (!f || !String(f).endsWith('.lock')) schedule(); })
      .on('error', () => { });
  } catch { }
}

// At most one refresh every 250 ms, never two at once.
let timer = null, loading = false, again = false;
function schedule() {
  if (!timer) timer = setTimeout(() => { timer = null; refresh(); }, 250);
}
async function refresh() {
  if (loading) { again = true; return; }
  loading = true;
  try { await load(); } catch { }
  loading = false;
  if (reveal.file && S.repo) { const f = reveal.file; reveal.file = null; reveal(f); }
  else { draw(); follow(); }
  if (again) { again = false; schedule(); }
}

// The file or folder selected in the explorer, selected here once the changes are loaded: the file, else the first
// changed file of the folder.
reveal.file = process.env.EXPLORER_FILE || null;
function reveal(abs) {
  const rel = path.relative(realPath(S.repo.top), realPath(abs)).split(path.sep).join('/');
  const r = S.repo, inside = p => !rel || p === rel || p.startsWith(rel + '/');
  const it = rel.startsWith('..') ? null : [...r.conflicts, ...r.staged, ...r.changes].find(i => inside(i.path));
  if (!it) { draw(); return flash(`No changes in ${base(rel) || 'the repository'}`); }
  // Its section and folders unfolded.
  S.collapsed.delete(it.sec);
  for (const k of [...S.collapsed]) if (k.startsWith(`${it.sec}:`) && it.path.startsWith(k.slice(it.sec.length + 1))) S.collapsed.delete(k);
  // Drawn first, so that the list's height is known (none before the first load) and select scrolls to the row.
  draw();
  select(frame.rows.findIndex(x => x.key === `${it.sec}:${it.path}`));
  draw();
}

// --- operations in progress

// Operations in progress, by repository state: continue (a merge ends with a commit instead) and abort.
const ONGOING = {
  MERGING: { name: 'merge', abort: ['merge', '--abort'] },
  REBASING: { name: 'rebase', cont: ['rebase', '--continue'], abort: ['rebase', '--abort'] },
  'CHERRY-PICKING': { name: 'cherry-pick', cont: ['cherry-pick', '--continue'], abort: ['cherry-pick', '--abort'] },
  REVERTING: { name: 'revert', cont: ['revert', '--continue'], abort: ['revert', '--abort'] },
};
const ongoing = () => (S.repo && ONGOING[S.repo.state]) || null;
// Continue rather than commit: always for a rebase (git keeps each commit's message), without a message of the
// user's for a cherry-pick or a revert (with one, a commit uses it).
// branch.<name>.mergeOptions, options the repository adds to every git merge on the branch, --abort included (a
// strategy of its own, see unsafeToContinue): emptied.
const mergeOptions = () => (S.repo && S.repo.branch && !S.repo.detached ? [[`branch.${S.repo.branch}.mergeoptions`, '']] : []);
const continues = () => { const o = ongoing(); return !!(o && o.cont && !S.amend && (S.repo.state === 'REBASING' || !S.field.value.trim())); };

const headName = () => (S.repo.detached ? 'HEAD' : S.repo.branch);

// --- branches

async function loadPicker() {
  await loadRefs();
  const p = S.picker;
  if (!p) return;
  const cur = S.repo.branch;
  p.all = [...S.refs.heads.map(b => ({ ...b, current: b.name === cur })), ...S.refs.remotes].filter(b => !p.mode || !b.current);
  filterPicker();
}

function filterPicker() {
  const p = S.picker, q = p.field.value.toLowerCase();
  p.items = [...(p.mode ? [] : [{ create: true, name: q ? `New branch "${p.field.value}"…` : 'New branch…' }]),
    ...p.all.filter(b => !q || b.name.toLowerCase().includes(q))];
  p.sel = Math.max(0, Math.min(p.sel, p.items.length - 1));
}

// mode: 'merge' or 'rebase' (the menu's Merge a branch… and Rebase onto a branch…), else the picker switches.
// anchor: { right, at }, the right edge (column from 0, excluded) and top line of the menu that opened it, where it
// hangs; else under the header's branch.
function openPicker(mode, anchor) {
  if (!S.repo) return;
  const first = mode ? 0 : 1;
  S.picker = { mode, anchor: mode ? anchor : null, field: new ui.Field(), typing: false, sel: first, top: 0, all: [], items: [] };
  filterPicker();
  draw();
  loadPicker().then(() => { if (S.picker && !S.inline) { S.picker.sel = Math.max(0, Math.min(first, S.picker.items.length - 1)); draw(); } });
}

// --- branch comparison (Refs)

// Full names: a branch called like an option, or like a commit, stays a branch for git.
const fullRef = b => `refs/${b.remote ? 'remotes' : 'heads'}/${b.name}`;
const baseRef = () => (S.compareBase ? fullRef(S.compareBase) : 'HEAD');
const baseName = () => (S.compareBase ? S.compareBase.name : S.repo.detached ? 'HEAD' : S.repo.branch);
const MAX_COMPARE = 300; // commits listed on each side

// The commits only on the branch, those only on the base (= when the same change is on both sides, as after a
// cherry-pick), and the files changed on the branch since it left the base.
async function compare(row) {
  const b = row.ref, range = `${baseRef()}...${fullRef(b)}`;
  const d = { key: row.key, compare: true, title: b.name, base: baseName(), top: 0, sel: -1, rows: [], cache: '',
    focus: S.diff ? S.diff.focus : false };
  S.diff = d;
  draw();
  const side = only => git(['log', '--no-show-signature', '--no-ext-diff', '--no-textconv', only, '--cherry-mark', '-n', String(MAX_COMPARE),
    '--format=%m%x1f%h%x1f%H%x1f%s%x1f%at', range, '--']);
  const [count, mine, theirs, stat] = await Promise.all([git(['rev-list', '--left-right', '--count', range, '--']),
    side('--right-only'), side('--left-only'), git(['diff', ...diffs.DIFF_OPTS, '--numstat', '-M', range, '--'])]);
  if (S.diff !== d) return;
  const list = res => parse.parseCompare(res.out);
  const [behind, ahead] = parse.parseCount(count.out);
  const files = parse.parseNumstat(stat.out);
  d.cmp = { ahead: ahead || 0, behind: behind || 0, mine: list(mine), theirs: list(theirs), files,
    failed: count.code !== 0 && errorOf(count) };
  d.cache = '';
  draw();
}

// = also has the Changes pane and the preview's Diff view of the tab show the branch's changes since it left the base
// (the scope base...branch, core/scope.js, in full names as compare has them: a tag of the same name does not stand
// in); a branch merely selected leaves the scope alone. In a review, the tab reviewed too (REVIEW), not only the
// review's own, whose preview shows the files.
let myTab;
async function shareCompare(row) {
  if (myTab === undefined) myTab = (await actions.tabOf(process.env.HERDR_PANE_ID).catch(() => null) || { me: {} }).me.tab_id || null;
  for (const tab of new Set([myTab, REVIEW].filter(Boolean))) Scope.write(tab, { kind: 'rev', rev: `${baseRef()}...${fullRef(row.ref)}` });
}

// --- diffs

async function showDiff(key, title, args, item, tag) {
  const d = S.diff && S.diff.key === key ? S.diff : { key, title, top: 0, focus: S.diff ? S.diff.focus : false };
  d.item = item; d.title = title; d.tag = tag;
  S.diff = d;
  draw();
  const res = await git(args);
  if (S.diff !== d) return;
  d.parsed = diffs.parse(res.out);
  d.cache = '';
  draw();
}

async function openItemDiff(it, quiet) {
  const D = ['diff', ...diffs.DIFF_OPTS, '-M'];
  const args = it.sec === 'staged' ? [...D, '--cached', '--', ...(it.orig ? [it.orig] : []), it.path]
    : it.untracked ? [...D, '--no-index', '--', '/dev/null', it.path]
      : it.sec === 'conflicts' && S.repo.hasHead ? [...D, 'HEAD', '--', it.path] : [...D, '--', it.path];
  const key = `${it.sec}:${it.path}`;
  if (quiet && S.diff && S.diff.key !== key) return;
  return showDiff(key, it.path, args, it, it.sec === 'staged' ? 'staged' : it.untracked ? 'untracked' : it.sec === 'conflicts' ? 'conflict' : '');
}

function closeDiff() { S.diff = null; draw(); }

// From SPLIT columns, the diff shows the selected file, commit, stash or tag (shortly after, when moving fast).
// A branch other than the base shows its comparison with the base.
const comparable = row => !!row && row.type === 'branch' && baseRef() !== fullRef(row.ref) && !(row.current && !S.compareBase);
const diffable = row => !!row && (['file', 'commit', 'stash', 'tag'].includes(row.type) || comparable(row));
// The diff on the right of the list stands for this row: its own, or a commit opened from its comparison.
const showing = row => !!S.diff && (S.diff.key === row.key || (!!S.diff.back && S.diff.back.key === row.key));

// --- views

function setView(id) {
  if (S.view === id) return;
  S.view = id; S.cursor = 0; S.top = 0; S.selKey = null; S.diff = null;
  draw();
  schedule();
}

// History of every branch or of the current one, kept for the next panes.
function toggleAllBranches() {
  S.allBranches = !S.allBranches;
  writeSettings({ ...readSettings(), scmGraphAll: S.allBranches });
  flash(S.allBranches ? 'All branches' : 'Current branch only');
  schedule();
}

// Kept for the next source control panes.
function toggleTree() {
  S.tree = !S.tree;
  writeSettings({ ...readSettings(), scmTree: S.tree });
  draw();
}

function toggle(row, open = !row.open) {
  if (open) S.collapsed.delete(row.key); else S.collapsed.add(row.key);
  draw();
}

module.exports = {
  exec, english, git, errorOf, op, net, fromFile, nul, loadStatus, histName, renamed, allFiles, schedule, refresh, ongoing,
  mergeOptions, continues, headName, filterPicker, openPicker, fullRef, baseName, compare, shareCompare, showDiff, openItemDiff,
  closeDiff, comparable, diffable, showing, setView, toggleAllBranches, toggleTree, toggle, forkPoint,
};
