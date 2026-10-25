'use strict';
// Scopes of the Changes pane, and what needs a repository (built under $ROOT/repo by a shell script, setup): the
// branches, a snapshot of the working tree (with the files then in the state folder), the agent's turn (its time
// given as its type). The kept copies of an index are named by a hash of its path, which changes with $ROOT: HASH.
const { execFileSync } = require('child_process');
const scope = require('../../../src/core/scope');

const sh = script => execFileSync('sh', ['-c', script], { encoding: 'utf8' });
const files = dir => sh(`cd '${dir}' 2>/dev/null && find . -type f | sort || true`).replace(/[0-9a-f]{40}/g, 'HASH');
const turnOf = tab => { const t = scope.readTurn(tab); return t && { ...t, t: typeof t.t }; };
const impl = {
  ...scope,
  // The items of the menu, each run given as what it calls: ['pick', scope] or ['ask', kind].
  itemsOf: (sc, baseName) => scope.items(sc, baseName, s => ['pick', s], k => ['ask', k]).map(it => it && { label: it.label, value: it.value, run: it.run() }),
  // Keys typed in the field of Commit or range…, from value: what each gave, and the field's value after.
  revKeyOf: (value, keys) => { const f = new (require('../../../src/core/ui').Field)(value); return keys.map(k => [scope.revKey(f, k), f.value]); },
  branchesIn: (setup, cwd) => { sh(setup); return scope.branches(cwd); },
  worktreeOf: async (setup, top, keep, only, calls = 1) => {
    sh(setup);
    const trees = [];
    for (let i = 0; i < calls; i++) trees.push(await scope.worktree(top, { ...process.env, GIT_LITERAL_PATHSPECS: '1' }, keep, only));
    return { trees, state: files(process.env.HERDR_PLUGIN_STATE_DIR) };
  },
  agentStatusIn: async (setup, steps) => {
    sh(setup);
    const out = [];
    for (const [tab, pane, status, cwd] of steps) out.push([await scope.agentStatus(tab, pane, status, cwd), turnOf(tab)]);
    return out;
  },
};

const env = { HERDR_PLUGIN_STATE_DIR: '$ROOT' };
const GIT = {
  HOME: '$ROOT/home', XDG_CONFIG_HOME: '$ROOT/home', GIT_CONFIG_NOSYSTEM: '1', LC_ALL: 'C', LANG: 'C',
  HERDR_PLUGIN_STATE_DIR: '$ROOT/state', GIT_AUTHOR_NAME: 'A', GIT_AUTHOR_EMAIL: 'a@example.com',
  GIT_AUTHOR_DATE: '2024-01-01T00:00:00Z', GIT_COMMITTER_NAME: 'A', GIT_COMMITTER_EMAIL: 'a@example.com',
};
const repo = more => `set -e; mkdir -p $ROOT/home $ROOT/repo $ROOT/plain; cd $ROOT/repo; git init -q -b main;
  printf 'a\\n' > a.txt; printf 'ignored.txt\\n' > .gitignore; git add -A;
  GIT_COMMITTER_DATE=2024-01-01T00:00:00Z git commit -qm init; ${more}`;
const branchy = repo(`git branch old; GIT_COMMITTER_DATE=2024-02-01T00:00:00Z git commit -q --allow-empty -m two;
  git checkout -q -b feature; GIT_COMMITTER_DATE=2024-03-01T00:00:00Z git commit -q --allow-empty -m three; git checkout -q main;
  git update-ref refs/remotes/origin/main old; git update-ref refs/remotes/up/x/HEAD old;
  git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main;`);
const dirty = repo(`printf 'b\\n' > a.txt; printf 'new\\n' > new.txt; printf 'no\\n' > ignored.txt; mkdir d; printf 'x\\n' > d/x;
  truncate -s 11M big.bin;`);
const turn = (t, more = {}) => JSON.stringify({ pane: 'p1', top: '$ROOT/repo', tree: null, t, ended: false, ...more });
const read = (text, tab = 'w1:t1') => ({ fn: 'read', args: [tab], env, ...(text !== undefined ? { files: { 'scope-w1_t1.json': text } } : {}) });
const write = (scope, files) => ({ fn: 'write', args: ['w1:t1', scope], env, ...(files ? { files } : {}), after: { 'scope-w1_t1.json': null } });

module.exports = {
  module: 'core/scope',
  impl,
  cases: [
    { fn: 'label', args: [{ kind: 'head' }] },
    { fn: 'label', args: [{ kind: 'branch' }] },
    { fn: 'label', args: [{ kind: 'branch' }, 'develop'] },
    { fn: 'label', args: [{ kind: 'turn' }] },
    { fn: 'label', args: [{ kind: 'rev', rev: 'refs/heads/a..refs/remotes/origin/b' }] },
    { fn: 'label', args: [{ kind: 'ref', rev: 'feature' }] },
    { fn: 'label', args: [{ kind: 'other' }] },
    { fn: 'same', args: [{ kind: 'rev', rev: 'a' }, { kind: 'rev', rev: 'a' }] },
    { fn: 'same', args: [{ kind: 'rev', rev: 'a' }, { kind: 'rev', rev: 'b' }] },
    { fn: 'same', args: [{ kind: 'head' }, { kind: 'head', rev: '' }] },
    read(),
    read('{"kind":"branch"}'),
    read('{"kind":"rev","rev":"a..b"}'),
    read('{"kind":"rev","rev":""}'),
    read('{"kind":"ref"}'),
    read('{"kind":"nope"}'),
    read('not json'),
    read('{"kind":"turn","extra":1}'),
    { fn: 'read', args: [''], env },
    write({ kind: 'head' }, { 'scope-w1_t1.json': '{"kind":"turn"}' }),
    write({ kind: 'branch' }),
    write({ kind: 'rev', rev: 'a...b' }),
    write({ kind: 'turn', rev: '' }),
    { fn: 'write', args: ['w1:t1', { kind: 'branch' }] },
    { fn: 'branchesIn', args: [branchy, '$ROOT/repo'], env: GIT },
    { fn: 'branchesIn', args: [branchy, '$ROOT/repo/.git'], env: GIT },
    { fn: 'branchesIn', args: [repo(''), '$ROOT/plain'], env: GIT },
    { fn: 'worktreeOf', args: [repo(''), '$ROOT/repo', false, null], env: GIT },
    { fn: 'worktreeOf', args: [dirty, '$ROOT/repo', false, null], env: GIT },
    { fn: 'worktreeOf', args: [dirty, '$ROOT/repo', false, 'd'], env: GIT },
    { fn: 'worktreeOf', args: [dirty, '$ROOT/repo', true, null, 2], env: GIT },
    { fn: 'worktreeOf', args: [dirty, '$ROOT/repo/d', false, null], env: GIT },
    { fn: 'worktreeOf', args: [repo(''), '$ROOT/plain', true, null], env: GIT },
    { fn: 'worktreeOf', args: [dirty, '$ROOT/repo', true, null], env: { ...GIT, HERDR_PLUGIN_STATE_DIR: '' } },
    { fn: 'agentStatusIn', args: [dirty, [['w1:t1', 'p1', 'working', '$ROOT/repo/d'], ['w1:t1', 'p1', 'working', '$ROOT/repo'],
      ['w1:t1', 'p1', 'blocked', '$ROOT/repo'], ['w1:t1', 'p1', 'idle', '$ROOT/repo'], ['w1:t1', 'p1', 'idle', '$ROOT/repo'],
      ['w1:t1', 'p2', 'done', '$ROOT/repo'], ['w1:t1', 'p2', 'working', '$ROOT/repo']]], env: GIT },
    { fn: 'agentStatusIn', args: [repo(''), [['w1:t1', 'p1', 'working', '$ROOT/plain'], ['w1:t2', 'p1', 'idle', '$ROOT/repo']]], env: GIT },
    { fn: 'agentStatusIn', args: [repo(''), [['w1:t1', 'p1', 'done', '$ROOT/repo'], ['w1:t1', 'p1', 'working', '$ROOT/repo']]], env: GIT,
      files: { 'state/turn-w1_t1.json': turn(5) } },
    { fn: 'agentStatusIn', args: [repo(''), [['w1:t1', 'p1', 'idle', '$ROOT/repo']]], env: GIT,
      files: { 'state/turn-w1_t1.json': '{"pane":"p1"}' } },
    { fn: 'agentStatusIn', args: [repo(''), [['w1:t1', 'p1', 'working', '$ROOT/repo']]], env: GIT,
      files: { 'state/turn-w1_t1.json': turn(5, { ended: true }) } },
    { fn: 'agentStatusIn', args: [repo(''), [['', 'p1', 'working', '$ROOT/repo']]], env: GIT },
    ...[[{ kind: 'head' }, ''], [{ kind: 'branch' }, 'develop'], [{ kind: 'turn' }, ''], [{ kind: 'rev', rev: 'a..b' }, ''], [{ kind: 'ref', rev: 'main' }, 'x'], [{ kind: 'rev' }, '']]
      .map(a => ({ fn: 'itemsOf', args: a })),
    { fn: 'revKeyOf', args: ['  HEAD~2  ', ['x', 'left', 'enter', 'escape', 'ctrl+c', 'f9']] },
    { fn: 'revKeyOf', args: ['', ['enter', 'backspace']] },
  ],
};
