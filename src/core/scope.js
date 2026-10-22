// The diff scope of a tab: what the Changes pane and the preview's Diff view compare the files with, chosen with b
// (or Compare in source control) and shared by every pane of the tab through scope-<tab id>.json in the state folder.
// A scope is { kind, rev }:
//   head    the last commit (the default)
//   branch  where the branch left the default one (merge-base): every change of the branch, committed or not
//   turn    the working tree when the agent of the tab last started working (turn-<tab id>.json, see snapshot)
//   rev     a commit (its own changes) or a range, a..b or a...b; rev the text typed
//   ref     a branch (or any commit) compared with the working tree; rev its name
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { writeAtomic, tabFile, run, GIT, gitEnv } = require('./exec');

const KINDS = ['head', 'branch', 'turn', 'rev', 'ref'];
const HEAD = { kind: 'head' };

function read(tab) {
  try {
    const s = JSON.parse(fs.readFileSync(tabFile('scope', tab), 'utf8'));
    if (KINDS.includes(s.kind) && (!['rev', 'ref'].includes(s.kind) || (typeof s.rev === 'string' && s.rev))) return s;
  } catch { }
  return HEAD;
}

// HEAD removes the file, the default.
function write(tab, scope) {
  const file = tabFile('scope', tab);
  if (!file) return false;
  if (scope.kind === 'head') { try { fs.unlinkSync(file); } catch { } return true; }
  return writeAtomic(file, JSON.stringify({ kind: scope.kind, ...(scope.rev ? { rev: scope.rev } : {}) }));
}

// Calls fn whenever the tab's scope changes (another pane chose one); returns a function that stops watching.
function watch(tab, fn) {
  const file = tabFile('scope', tab);
  if (!file) return () => { };
  fs.watchFile(file, { interval: 500 }, fn);
  return () => fs.unwatchFile(file, fn);
}

const same = (a, b) => a.kind === b.kind && (a.rev || '') === (b.rev || '');

// The scope in a few words, for a header: HEAD, main (merge-base), last turn, abc123, a..b, vs feature.
function label(scope, baseName = '') {
  switch (scope.kind) {
    case 'branch': return `${baseName || 'main'} base`;
    case 'turn': return 'last turn';
    case 'rev': return scope.rev;
    case 'ref': return `vs ${scope.rev}`;
    default: return 'HEAD';
  }
}

// The items of the menu b opens, as ui.Menu takes them: pick(scope) for a scope chosen at once; ask(kind) for those
// that need a commit, a range or a branch typed.
function items(scope, baseName, pick, ask) {
  const mark = s => (same(scope, s) || (scope.kind === s.kind && ['rev', 'ref'].includes(s.kind)) ? ['●', ''] : null);
  return [
    { label: 'HEAD', value: mark(HEAD), run: () => pick(HEAD) },
    { label: `Merge base with ${baseName || 'main'}`, value: mark({ kind: 'branch' }), run: () => pick({ kind: 'branch' }) },
    { label: 'Last agent turn', value: mark({ kind: 'turn' }), run: () => pick({ kind: 'turn' }) },
    null,
    { label: `Commit or range…${scope.kind === 'rev' ? `  ${scope.rev}` : ''}`, value: mark({ kind: 'rev' }), run: () => ask('rev') },
    { label: `Branch…${scope.kind === 'ref' ? `  ${scope.rev}` : ''}`, value: mark({ kind: 'ref' }), run: () => ask('ref') },
  ];
}

// The branches of the repository holding cwd, local then remote, the latest commit first, for Branch….
async function branches(cwd) {
  const out = await run(GIT, ['for-each-ref', '--sort=-committerdate', '--format=%(refname:short)', 'refs/heads', 'refs/remotes'],
    { cwd, env: await gitEnv(cwd) }) || '';
  return out.split('\n').map(s => s.trim()).filter(s => s && !s.endsWith('/HEAD'));
}

// --- the agent's last turn

// A tree object of the repository's working tree as it is: tracked files with their changes, and untracked ones
// that are not ignored, added to a copy of the index (the real one left alone), so that git rehashes only the files
// changed since. Its blobs stay in the repository's objects until git gc prunes them. Returns the tree's id, or null.
async function worktree(top, env) {
  const git = (args, extra = {}) => run(GIT, args, { cwd: top, env: { ...env, ...extra } });
  const index = (await git(['rev-parse', '--git-path', 'index']) || '').trim();
  if (!index) return null;
  const tmp = path.join(os.tmpdir(), `herdr-explorer-index-${process.pid}-${Date.now()}`);
  try {
    try { fs.copyFileSync(path.resolve(top, index), tmp); } catch { }
    if (await git(['add', '-A', '--', '.'], { GIT_INDEX_FILE: tmp }) === null) return null;
    return (await git(['write-tree'], { GIT_INDEX_FILE: tmp }) || '').trim() || null;
  } finally {
    try { fs.unlinkSync(tmp); } catch { }
  }
}

// What turn-<tab id>.json keeps: { pane, top, tree, t, ended }, the working tree of the repository of the pane whose
// agent started working last, taken when it did; ended once that agent is idle or done again.
function readTurn(tab) {
  try { return JSON.parse(fs.readFileSync(tabFile('turn', tab), 'utf8')); } catch { return null; }
}

// An agent's status changed (the pane.agent_status_changed event): a new turn starts when it works again after
// having been idle or done, or for an agent other than the last one; blocked or unknown in between keep the turn.
// cwd: the pane's folder. Returns what was done, for the log.
async function agentStatus(tab, pane, status, cwd) {
  const turn = readTurn(tab);
  const mine = turn && turn.pane === pane;
  if (status !== 'working') {
    if (mine && (status === 'idle' || status === 'done') && !turn.ended) writeAtomic(tabFile('turn', tab), JSON.stringify({ ...turn, ended: true }));
    return 'kept';
  }
  if (mine && !turn.ended) return 'same turn';
  const env = await gitEnv(cwd);
  const top = (await run(GIT, ['rev-parse', '--show-toplevel'], { cwd, env }) || '').trim();
  if (!top) return 'not a repository';
  const tree = await worktree(top, env);
  if (!tree) return 'no snapshot';
  writeAtomic(tabFile('turn', tab), JSON.stringify({ pane, top: path.resolve(top), tree, t: Date.now(), ended: false }));
  return 'snapshot';
}

module.exports = { KINDS, HEAD, read, write, watch, same, label, items, branches, worktree, readTurn, agentStatus };
