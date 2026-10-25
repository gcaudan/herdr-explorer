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
const crypto = require('crypto');
const { writeAtomic, tabFile, run, GIT, gitEnv, realPath } = require('./exec');

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

// The scope in a few words, for a header: HEAD, main base, last turn, abc123, a..b, vs feature.
function label(scope, baseName = '') {
  switch (scope.kind) {
    case 'branch': return `${baseName || 'main'} base`;
    case 'turn': return 'last turn';
    case 'rev': return scope.rev.replace(/refs\/(heads|remotes)\//g, ''); // the full names source control writes
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

// The menu b opens (ui.Menu), and the one of Branch… in its place: pick(scope) for a scope chosen; show(menu) for
// the menu to show next (null: none); prompt(rev) to type a commit or range, from the one typed before; say(text)
// for a message. cwd: a folder of the repository.
function menu(scope, baseName, cwd, { pick, show, prompt, say }) {
  const ui = require('./ui');
  const ask = async kind => {
    show(null);
    if (kind === 'rev') return prompt(scope.kind === 'rev' ? scope.rev : '');
    const list = await branches(cwd);
    if (!list.length) return say('No branch');
    show(new ui.Menu({ title: 'Branch', at: 1, right: ui.size().W,
      items: list.map(b => ({ label: b, value: scope.kind === 'ref' && scope.rev === b ? ['●', ''] : null, run: () => pick({ kind: 'ref', rev: b }) })) }));
  };
  return new ui.Menu({ title: 'Compare with', items: items(scope, baseName, pick, ask), at: 1, right: ui.size().W });
}

// A key in the field of Commit or range…: { done, rev } on enter (rev '' when empty) or escape (rev ''), else
// { changed } when the field took the key.
function revKey(field, k) {
  if (k === 'escape' || k === 'ctrl+c') return { done: true, rev: '' };
  if (k === 'enter') return { done: true, rev: field.value.trim() };
  return { changed: field.key(k) };
}

// The branches of the repository holding cwd, local then remote, the latest commit first, for Branch….
async function branches(cwd) {
  const out = await run(GIT, ['for-each-ref', '--sort=-committerdate', '--format=%(refname)%00%(symref)%00%(refname:short)', 'refs/heads', 'refs/remotes'],
    { cwd, env: await gitEnv(cwd) }) || '';
  // origin/HEAD, whose short name is origin, left out: a remote's symbolic ref (whatever the remote's name), or its
  // HEAD by name when not symbolic. A local symbolic ref (master as an alias of main) stays.
  const remoteHead = (full, sym) => full.startsWith('refs/remotes/') && (sym || /^refs\/remotes\/[^/]+\/HEAD$/.test(full));
  return out.split('\n').map(l => l.split('\0')).filter(([full, sym, short]) => short && !remoteHead(full, sym))
    .map(([, , short]) => short.trim());
}

// --- the agent's last turn

// Untracked files above this size stay out of the snapshot (a dataset or a build output not ignored): hashed, each
// would take its size again in the repository's objects until git gc.
const MAX_SNAPSHOT_FILE = 10 << 20;

// A tree object of the repository's working tree as it is: tracked files with their changes, and untracked ones
// that are not ignored (up to MAX_SNAPSHOT_FILE), added to a copy of the index (the real one left alone), so that git
// rehashes only the files changed since. Its blobs stay in the repository's objects until git gc prunes them. Returns the tree's id, or null.
// keep: the copy is kept in the state folder (index/, one per index file of a repository) for the next call, so that
// the untracked files too are rehashed only once changed (the Diff view reloaded); copied again when the real index
// changes. Not in the git folder: a repository received with it (an archive) could hold a symbolic link there, which
// the copy would follow to overwrite any file of the user.
// only: a file or folder, the one path the caller compares, the only one added (the rest of the tree is then as last
// added, not as it is now).
async function worktree(top, env, keep = false, only = null) {
  // The exclusions below need the pathspec magic that GIT_LITERAL_PATHSPECS turns off: the paths are made literal
  // one by one instead.
  const { GIT_LITERAL_PATHSPECS, ...plain } = env;
  const git = (args, extra = {}) => run(GIT, args, { cwd: top, env: { ...plain, ...extra } });
  const index = (await git(['rev-parse', '--git-path', 'index']) || '').trim();
  if (!index) return null;
  // The large untracked files left out: the last turn shows them as new.
  const big = ((await git(['ls-files', '-o', '--exclude-standard', '-z', '--', only ? `:(literal)${only}` : '.'])) || '').split('\0')
    .filter(f => { try { return f && fs.statSync(path.join(top, f)).size > MAX_SNAPSHOT_FILE; } catch { return false; } });
  const paths = ['--', only ? `:(literal)${only}` : '.', ...big.map(f => `:(exclude,literal)${f}`)];
  const kept = keep && keptIndex(path.resolve(top, index));
  if (kept) {
    const gitDir = path.dirname(path.resolve(top, index));
    if (!oldCopies.has(gitDir)) {
      // The copy that versions before 0.8.0 kept in the git folder, beside the index, with its .from, and the
      // .<pid> copies and git's .lock of a process that died.
      oldCopies.add(gitDir);
      let names = [];
      try { names = fs.readdirSync(gitDir); } catch { }
      for (const n of names.filter(n => /^herdr-explorer-index(\.from(\.\d+)?|\.lock|\.\d+)?$/.test(n))) {
        try { fs.rmSync(path.join(gitDir, n), { force: true }); } catch { }
      }
    }
    const st = fs.statSync(path.resolve(top, index), { throwIfNoEntry: false });
    const from = st ? `${st.mtimeMs}:${st.size}` : 'none';
    try {
      if (fs.readFileSync(`${kept}.from`, 'utf8') !== from || !fs.existsSync(kept)) throw new Error('stale');
    } catch {
      // Copied while holding git's own lock of the copy: a git add of another pane running on it would otherwise put
      // back the old copy when it ends, while .from says it is fresh. Taken already: a copy of its own.
      const lock = `${kept}.lock`, tmp = `${kept}.${process.pid}`;
      try {
        fs.mkdirSync(path.dirname(kept), { recursive: true });
        fs.closeSync(fs.openSync(lock, 'wx'));
      } catch {
        // Never removed here, whatever its age: git add can hold it for minutes on a large repository, and nothing
        // tells whose it is. One left by a process that died goes with the weekly sweep (sweepState).
        return worktree(top, env, false, only);
      }
      try {
        // Another pane may have copied it while this one waited: copied again, the files its git add hashed since
        // would be hashed again.
        // The index read again: it may have changed meanwhile, and .from must say what is copied.
        const cur = fs.statSync(path.resolve(top, index), { throwIfNoEntry: false });
        const curFrom = cur ? `${cur.mtimeMs}:${cur.size}` : 'none';
        let fresh = false;
        try { fresh = fs.readFileSync(`${kept}.from`, 'utf8') === curFrom && fs.existsSync(kept); } catch { }
        if (!fresh) {
          if (cur) copyIndex(path.resolve(top, index), kept, cur, tmp);
          else fs.rmSync(kept, { force: true });
          if (!writeAtomic(`${kept}.from`, curFrom)) throw new Error('not written');
        }
      } catch {
        for (const f of [tmp, lock]) try { fs.rmSync(f, { force: true }); } catch { }
        return worktree(top, env, false, only);
      }
      try { fs.rmSync(lock, { force: true }); } catch { }
    }
    if (await git(['add', '-A', ...paths], { GIT_INDEX_FILE: kept }) !== null) {
      const tree = (await git(['write-tree'], { GIT_INDEX_FILE: kept }) || '').trim();
      // Marked used through .from (sweepState removes the copy a week after it): git rewrites neither file when
      // nothing changed, and the copy's own mtime is git's to judge its racily clean entries.
      const now = new Date();
      try { fs.utimesSync(`${kept}.from`, now, now); } catch { }
      if (tree) return tree;
    }
    // Another pane updating it at the same time (its lock taken): a copy of its own.
    return worktree(top, env, false, only);
  }
  let dir;
  try { dir = fs.mkdtempSync(path.join(os.tmpdir(), 'herdr-explorer-index-')); } catch { return null; }
  const tmp = path.join(dir, 'index');
  try {
    try { copyIndex(path.resolve(top, index), tmp); } catch { }
    if (await git(['add', '-A', ...paths], { GIT_INDEX_FILE: tmp }) === null) return null;
    return (await git(['write-tree'], { GIT_INDEX_FILE: tmp }) || '').trim() || null;
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch { }
  }
}

// Copies the index file src (st, its stat, read before) to dst through tmp, with its dates: git trusts the stat data
// of the files changed in the same tick as the index was written (racily clean) only if they are older than it, not
// the copy. Dated once renamed: sweepState judges a leftover tmp by its own date.
function copyIndex(src, dst, st = fs.statSync(src), tmp = dst) {
  fs.copyFileSync(src, tmp);
  if (tmp !== dst) fs.renameSync(tmp, dst);
  fs.utimesSync(dst, st.atime, st.mtime);
}

// The copy of an index file kept by worktree: index/<hash of its path> in the state folder, null without one. Those
// not used for a week are removed by sweepState (core/exec.js).
function keptIndex(index) {
  const dir = process.env.HERDR_PLUGIN_STATE_DIR;
  if (!dir) return null;
  const key = `${dir}\0${index}`;
  if (!keptPaths.has(key)) keptPaths.set(key, path.join(dir, 'index', crypto.createHash('sha1').update(realPath(index)).digest('hex')));
  return keptPaths.get(key);
}
const keptPaths = new Map();
// The git folders whose old copy was removed by this process.
const oldCopies = new Set();

// What turn-<tab id>.json keeps: { pane, top, tree, t, ended }, the working tree of the repository of the pane whose
// agent started working last, taken when it did (tree null while it is being taken); ended once that agent is idle
// or done again.
function readTurn(tab) {
  try { return JSON.parse(fs.readFileSync(tabFile('turn', tab), 'utf8')); } catch { return null; }
}

// Sets fields of the turn started at t, read again just before: the hooks of several events run at once, and a
// write made from an older read would undo the other's (a tree taken back to null, an end lost). Read back after,
// to write again if another hook wrote over it meanwhile. False when the turn is no longer that one.
function updateTurn(tab, t, fields) {
  for (let tries = 0; tries < 3; tries++) {
    const now = readTurn(tab);
    if (!now || now.t !== t) return false;
    writeAtomic(tabFile('turn', tab), JSON.stringify({ ...now, ...fields }));
    const back = readTurn(tab);
    if (back && back.t === t && Object.keys(fields).every(k => back[k] === fields[k])) return true;
  }
  return false;
}

// An agent's status changed (the pane.agent_status_changed event): a new turn starts when it works again after
// having been idle or done, or for an agent other than the last one; blocked or unknown in between keep the turn.
// cwd: the pane's folder. Returns what was done, for the log.
async function agentStatus(tab, pane, status, cwd) {
  const turn = readTurn(tab);
  const mine = turn && turn.pane === pane;
  if (status !== 'working') {
    if (mine && (status === 'idle' || status === 'done') && !turn.ended) updateTurn(tab, turn.t, { ended: true });
    return 'kept';
  }
  if (mine && !turn.ended) return 'same turn';
  const env = await gitEnv(cwd);
  const top = (await run(GIT, ['rev-parse', '--show-toplevel'], { cwd, env }) || '').trim();
  if (!top) return 'not a repository';
  // The turn is written at once, its tree once taken: the hooks of the events that follow, run while git hashes the
  // files, see the turn started (a working one keeps it, an idle one ends it), and the tree does not undo them.
  const t = Date.now();
  writeAtomic(tabFile('turn', tab), JSON.stringify({ pane, top: path.resolve(top), tree: null, t, ended: false }));
  const tree = await worktree(top, env, true);
  if (!updateTurn(tab, t, { tree })) return 'snapshot dropped: a newer turn';
  return tree ? 'snapshot' : 'no snapshot';
}

module.exports = { KINDS, HEAD, read, write, watch, same, label, items, menu, revKey, branches, worktree, readTurn, agentStatus };
