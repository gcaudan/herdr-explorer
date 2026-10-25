// git's output turned into data, for the source control pane: status, history, stashes, refs, remotes, worktrees,
// branch comparisons, config scopes and error messages. Pure functions: nothing runs at require time.
'use strict';
const path = require('path');

const CODE = { M: 'M', T: 'M', A: 'A', D: 'D', R: 'R', C: 'A' };
const CONFLICT = { DD: 'both deleted', AU: 'added by us', UD: 'deleted by them', UA: 'added by them', DU: 'deleted by us',
  AA: 'both added', UU: 'both modified' };

// git rev-parse --show-toplevel --absolute-git-dir --git-common-dir, run in root: the repository's folders, null
// without a top folder (the common folder can be relative to root).
function parseRepo(out, root) {
  const [top, gitdir, common] = out.split(/\r?\n/);
  if (!top) return null;
  return { top: path.resolve(top), gitdir: path.resolve(gitdir), common: path.resolve(root, common) };
}

// git status --porcelain=v2 -z --branch: the branch, its upstream, and the conflicts, staged and unstaged changes,
// each sorted by path. A rename (type 2) is followed by its former path in the next field.
function parseStatus(out) {
  const info = { branch: '', oid: '', upstream: '', ahead: 0, behind: 0, conflicts: [], staged: [], changes: [] };
  const f = out.split('\0');
  for (let i = 0; i < f.length; i++) {
    const e = f[i], parts = e.split(' ');
    if (e.startsWith('# branch.oid ')) info.oid = e.slice(13);
    else if (e.startsWith('# branch.head ')) info.branch = e.slice(14);
    else if (e.startsWith('# branch.upstream ')) info.upstream = e.slice(18);
    else if (e.startsWith('# branch.ab ')) { const m = /\+(\d+) -(\d+)/.exec(e); if (m) [info.ahead, info.behind] = [+m[1], +m[2]]; }
    else if (e[0] === '1' || e[0] === '2') {
      const xy = parts[1], p = parts.slice(e[0] === '1' ? 8 : 9).join(' ');
      const orig = e[0] === '2' ? f[++i] : null;
      if (xy[0] !== '.') info.staged.push({ path: p, orig: xy[0] === 'R' ? orig : null, code: CODE[xy[0]] || 'M', sec: 'staged' });
      if (xy[1] !== '.') info.changes.push({ path: p, code: CODE[xy[1]] || 'M', sec: 'changes' });
    } else if (e[0] === 'u') {
      info.conflicts.push({ path: parts.slice(10).join(' '), code: 'C', label: CONFLICT[parts[1]], sec: 'conflicts' });
    } else if (e[0] === '?') info.changes.push({ path: e.slice(2), code: 'U', untracked: true, sec: 'changes' });
  }
  const byPath = (a, b) => a.path.localeCompare(b.path, undefined, { numeric: true, sensitivity: 'base' });
  for (const k of ['conflicts', 'staged', 'changes']) info[k].sort(byPath);
  info.detached = info.branch === '(detached)';
  info.hasHead = info.oid !== '(initial)';
  return info;
}

// The names a file had, from git log --follow --name-only -z --format=.
const parseNames = out => out.split(/[\0\n]/).filter(Boolean);

// git log --decorate=full --format=%H%x1f%h%x1f%s%x1f%an%x1f%at%x1f%D%x1f%P: one commit per line.
function parseHistory(out) {
  return out.split('\n').filter(Boolean).map(l => {
    const [hash, short, subject, author, time, refs, parents] = l.split('\x1f');
    return { hash, short, subject, author, date: ago(+time), refs: refList(refs), head: /(^|, )HEAD($|,| ->)/.test(refs),
      parents: (parents || '').split(' ').filter(Boolean) };
  });
}

// Short age of a commit: "5 min", "3 h", "2 d", "4 mo", "1 y".
function ago(time) {
  const s = Math.max(0, Date.now() / 1000 - time);
  return s < 3600 ? `${Math.max(1, Math.floor(s / 60))} min` : s < 86400 ? `${Math.floor(s / 3600)} h`
    : s < 30 * 86400 ? `${Math.floor(s / 86400)} d` : s < 365 * 86400 ? `${Math.floor(s / (30 * 86400))} mo` : `${Math.floor(s / (365 * 86400))} y`;
}

// Decorations of a commit (%D with full names): branches, remote branches (without their HEAD) and tags.
function refList(D) {
  const out = [];
  for (let r of (D || '').split(', ').filter(Boolean)) {
    r = r.replace(/^HEAD -> /, '');
    const m = /^(?:tag: )?refs\/(heads|remotes|tags)\/(.+)$/.exec(r);
    if (r === 'HEAD') out.push({ name: 'HEAD', kind: 'heads' });
    else if (m && !(m[1] === 'remotes' && /\/HEAD$/.test(m[2]))) out.push({ name: m[2], kind: m[1] });
  }
  return out;
}

// A commit's labels (refList) in the order the history shows them: a branch and the remote ones of the same name at
// the same commit as one (remote), the branch checked out first (current; head: the commit is HEAD, branch: the
// branch checked out, '' when detached), then the other branches, the remote ones alone and the tags (tag).
function labels(refs, head, branch) {
  const heads = refs.filter(r => r.kind === 'heads'), remotes = refs.filter(r => r.kind === 'remotes');
  const joined = new Set();
  const local = heads.map(h => {
    const mine = remotes.filter(r => r.name.slice(r.name.indexOf('/') + 1) === h.name);
    mine.forEach(r => joined.add(r));
    return { name: h.name, remote: mine.length > 0, current: !!head && (h.name === 'HEAD' || h.name === branch) };
  });
  return [...local.filter(l => l.current), ...local.filter(l => !l.current),
    ...remotes.filter(r => !joined.has(r)).map(r => ({ name: r.name })), ...refs.filter(r => r.kind === 'tags').map(t => ({ name: t.name, tag: true }))];
}

// git stash list --format=%gd%x1f%s%x1f%cr.
function parseStashes(out) {
  return out.split('\n').filter(Boolean).map(l => {
    const [ref, subject, date] = l.split('\x1f');
    return { ref, subject, date };
  });
}

// git for-each-ref (refname, short hash, upstream, track, date, subject, separated by %1f), git remote -v and git
// worktree list --porcelain: branches, remote branches (without their HEAD), tags, remotes and worktrees.
function parseRefs(refs, remotes, trees) {
  const R = { heads: [], remotes: [], tags: [], urls: [], worktrees: [] };
  for (const l of refs.split('\n').filter(Boolean)) {
    const [ref, hash, upstream, track, date, subject] = l.split('\x1f');
    const m = /^refs\/(heads|remotes|tags)\/(.+)$/.exec(ref);
    if (!m || (m[1] === 'remotes' && /\/HEAD$/.test(m[2]))) continue;
    R[m[1]].push({ name: m[2], hash, upstream, track, date, subject, remote: m[1] === 'remotes', tag: m[1] === 'tags' });
  }
  for (const l of remotes.split('\n')) {
    const m = /^(\S+)\s+(.*) \(fetch\)$/.exec(l);
    if (m) R.urls.push({ name: m[1], url: m[2] });
  }
  for (const block of trees.split(/\n\n/)) {
    const wt = /^worktree (.*)$/m.exec(block);
    if (wt) R.worktrees.push({ path: wt[1], branch: (/^branch refs\/heads\/(.*)$/m.exec(block) || [])[1] || '(detached)' });
  }
  return R;
}

// One side of a branch comparison, git log --cherry-mark --format=%m%x1f%h%x1f%H%x1f%s%x1f%at: '=' marks a change
// that is on both sides.
function parseCompare(out) {
  return out.split('\n').filter(Boolean).map(l => {
    const [mark, short, hash, subject, time] = l.split('\x1f');
    return { same: mark === '=', short, hash, subject, date: ago(+time) };
  });
}

// git rev-list --left-right --count base...branch: [behind, ahead].
const parseCount = out => out.trim().split(/\s+/).map(Number);

// git diff --numstat: { add, del, path }, add and del '-' for a binary file.
const parseNumstat = out => out.split('\n').filter(Boolean).map(l => { const [add, del, ...p] = l.split('\t'); return { add, del, path: p.join('\t') }; });

// git config -z --show-scope --get-regexp: the values of each key, with their scope, in order: Map key → [[scope, value]].
function parseScopes(out) {
  const keys = new Map();
  const f = out.split('\0');
  for (let i = 0; i + 1 < f.length; i += 2) {
    const nl = f[i + 1].indexOf('\n');
    const key = nl < 0 ? f[i + 1] : f[i + 1].slice(0, nl);
    if (!keys.has(key)) keys.set(key, []);
    keys.get(key).push([f[i], nl < 0 ? '' : f[i + 1].slice(nl + 1)]);
  }
  return keys;
}

// The same output: the keys the repository sets itself (not the system, user or command line).
function repoKeys(out) {
  const f = out.split('\0'), keys = [];
  for (let i = 0; i + 1 < f.length; i += 2) {
    if (f[i] !== 'system' && f[i] !== 'global' && f[i] !== 'command') keys.push(f[i + 1].split('\n')[0]);
  }
  return keys;
}

// The message of a failed git command: its first error line, without the "fatal: " prefix.
function errorOf(res) {
  const lines = (res.err || res.out || '').split(/\r?\n/).map(l => l.trim()).filter(Boolean);
  // A refused push: the reason (non-fast-forward, fetch first…) rather than "failed to push some refs".
  const rejected = lines.find(x => /^! \[(remote )?rejected\]/.test(x));
  if (rejected) return rejected.replace(/^! \[(remote )?rejected\]\s+/, 'rejected ');
  const l = lines.find(x => /^(fatal|error):/.test(x)) || lines[0] || (res.code === null ? 'timed out or not run' : 'failed');
  return l.replace(/^(fatal|error): /, '');
}

module.exports = {
  CODE, CONFLICT, parseRepo, parseStatus, parseNames, parseHistory, ago, refList, labels, parseStashes, parseRefs, parseCompare,
  parseCount, parseNumstat, parseScopes, repoKeys, errorOf,
};
