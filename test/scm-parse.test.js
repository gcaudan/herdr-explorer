// The parsers of the source control pane (src/scm/parse.js), on output taken from git.
const test = require('node:test');
const assert = require('node:assert');
const path = require('path');
const parse = require('../src/scm/parse');

const OID = '5de5c897f4428a5692f5231cb6fc7d24c977d804';
const z = lines => lines.join('\0') + '\0';

test('status: empty repository', () => {
  const s = parse.parseStatus(z(['# branch.oid (initial)', '# branch.head main']));
  assert.strictEqual(s.branch, 'main');
  assert.strictEqual(s.hasHead, false);
  assert.strictEqual(s.detached, false);
  assert.deepStrictEqual([s.conflicts, s.staged, s.changes], [[], [], []]);
  assert.strictEqual(s.upstream, '');
  assert.deepStrictEqual([s.ahead, s.behind], [0, 0]);
});

test('status: upstream, staged and unstaged changes of one file, sorted by path', () => {
  const s = parse.parseStatus(z([
    `# branch.oid ${OID}`, '# branch.head feat/x', '# branch.upstream origin/feat/x', '# branch.ab +2 -1',
    `1 MM N... 100644 100644 100644 ${OID} ${OID} src/b.js`,
    `1 A. N... 000000 100644 100644 ${'0'.repeat(40)} ${OID} a.txt`,
    `1 .D N... 100644 100644 000000 ${OID} ${OID} Z.md`,
    `1 .T N... 100644 100644 120000 ${OID} ${OID} link`,
  ]));
  assert.strictEqual(s.branch, 'feat/x');
  assert.strictEqual(s.oid, OID);
  assert.strictEqual(s.upstream, 'origin/feat/x');
  assert.deepStrictEqual([s.ahead, s.behind], [2, 1]);
  assert.strictEqual(s.hasHead, true);
  assert.deepStrictEqual(s.staged.map(i => [i.path, i.code, i.orig]), [['a.txt', 'A', null], ['src/b.js', 'M', null]]);
  assert.deepStrictEqual(s.changes.map(i => [i.path, i.code]), [['link', 'M'], ['src/b.js', 'M'], ['Z.md', 'D']]);
  assert.ok(s.changes.every(i => i.sec === 'changes') && s.staged.every(i => i.sec === 'staged'));
});

test('status -z: rename to a unicode path with spaces, conflict, untracked names kept literal', () => {
  const s = parse.parseStatus(z([
    `# branch.oid ${OID}`, '# branch.head main',
    `2 R. N... 100644 100644 100644 ${OID} ${OID} R100 dir/new é.js`, 'old.js',
    `u UU N... 100644 100644 100644 100644 ${OID} ${OID} ${OID} f.txt`,
    `u DU N... 100644 000000 100644 100644 ${OID} ${'0'.repeat(40)} ${OID} gone by us.txt`,
    '? tab\tq.txt', '? ünï cødé.md', '? "quoted".txt',
  ]));
  assert.deepStrictEqual(s.staged, [{ path: 'dir/new é.js', orig: 'old.js', code: 'R', sec: 'staged' }]);
  assert.deepStrictEqual(s.conflicts.map(i => [i.path, i.code, i.label]),
    [['f.txt', 'C', 'both modified'], ['gone by us.txt', 'C', 'deleted by us']]);
  assert.deepStrictEqual(s.changes.map(i => [i.path, i.code, i.untracked]),
    [['"quoted".txt', 'U', true], ['tab\tq.txt', 'U', true], ['ünï cødé.md', 'U', true]]);
});

test('status: a copy is shown as added, without its source', () => {
  const s = parse.parseStatus(z([`2 C. N... 100644 100644 100644 ${OID} ${OID} C75 copy.js`, 'orig.js']));
  assert.deepStrictEqual(s.staged, [{ path: 'copy.js', orig: null, code: 'A', sec: 'staged' }]);
});

test('status: detached HEAD', () => {
  const s = parse.parseStatus(z([`# branch.oid ${OID}`, '# branch.head (detached)']));
  assert.strictEqual(s.detached, true);
  assert.strictEqual(s.hasHead, true);
});

test('rev-parse: the repository folders, the common one relative to the folder', () => {
  const root = path.resolve('/repo/sub');
  const r = parse.parseRepo(`${path.resolve('/repo')}\n${path.resolve('/repo/.git')}\n../.git\n`, root);
  assert.deepStrictEqual(r, { top: path.resolve('/repo'), gitdir: path.resolve('/repo/.git'), common: path.resolve('/repo/.git') });
  assert.strictEqual(parse.parseRepo('', root), null);
});

test('log --follow names', () => {
  assert.deepStrictEqual(parse.parseNames('\nsrc/new.js\0\nsrc/old.js\0'), ['src/new.js', 'src/old.js']);
});

test('history: decorations, HEAD, parents, merge commits', () => {
  const now = Math.floor(Date.now() / 1000);
  const out = [
    ['a'.repeat(40), 'aaaaaaa', 'Merge branch \'feat\'', 'Zoë', now - 120,
      'HEAD -> refs/heads/main, refs/remotes/origin/main, refs/remotes/origin/HEAD, tag: refs/tags/v1.0', `${'b'.repeat(40)} ${'c'.repeat(40)}`],
    ['b'.repeat(40), 'bbbbbbb', 'fix: x | y', 'T', now - 3 * 86400, '', 'd'.repeat(40)],
    ['d'.repeat(40), 'ddddddd', 'feat: initial', 'T', now - 400 * 86400, 'HEAD', ''],
  ].map(f => f.join('\x1f')).join('\n') + '\n';
  const h = parse.parseHistory(out);
  assert.strictEqual(h.length, 3);
  assert.deepStrictEqual(h[0].refs, [{ name: 'main', kind: 'heads' }, { name: 'origin/main', kind: 'remotes' }, { name: 'v1.0', kind: 'tags' }]);
  assert.strictEqual(h[0].head, true);
  assert.deepStrictEqual(h[0].parents, ['b'.repeat(40), 'c'.repeat(40)]);
  assert.strictEqual(h[0].date, '2 min');
  assert.strictEqual(h[1].subject, 'fix: x | y');
  assert.deepStrictEqual([h[1].refs, h[1].head, h[1].date], [[], false, '3 d']);
  // Detached HEAD: HEAD alone.
  assert.deepStrictEqual([h[2].refs, h[2].head, h[2].parents, h[2].date], [[{ name: 'HEAD', kind: 'heads' }], true, [], '1 y']);
  assert.deepStrictEqual(parse.parseHistory(''), []);
});

test('ago', () => {
  const now = Date.now() / 1000;
  assert.strictEqual(parse.ago(now), '1 min');
  assert.strictEqual(parse.ago(now - 7200), '2 h');
  assert.strictEqual(parse.ago(now - 60 * 86400), '2 mo');
  assert.strictEqual(parse.ago(now + 999), '1 min');
});

test('stash list', () => {
  const s = parse.parseStashes('stash@{0}\x1fWIP on main: ae3311f chore: z\x1f2 minutes ago\nstash@{1}\x1fOn feat: été\x1f3 days ago\n');
  assert.deepStrictEqual(s, [{ ref: 'stash@{0}', subject: 'WIP on main: ae3311f chore: z', date: '2 minutes ago' },
    { ref: 'stash@{1}', subject: 'On feat: été', date: '3 days ago' }]);
  assert.deepStrictEqual(parse.parseStashes(''), []);
});

test('refs: branches, remote branches without HEAD, tags, remotes, worktrees', () => {
  const refs = [
    ['refs/heads/feat/x', 'd5c4e81', 'origin/feat/x', '[ahead 1, behind 2]', '2 days ago', 'fix: readme'],
    ['refs/heads/main', 'ae3311f', '', '', '31 seconds ago', 'chore: z'],
    ['refs/remotes/origin/HEAD', 'ae3311f', '', '', '1 day ago', 'chore: z'],
    ['refs/remotes/origin/feat/x', '0520c69', '', '', '1 day ago', 'feat: initial'],
    ['refs/tags/v1', 'd5c4e81', '', '', '2 days ago', 'fix: readme'],
  ].map(f => f.join('\x1f')).join('\n') + '\n';
  const remotes = 'origin\tgit@example.com:me/repo.git (fetch)\norigin\tgit@example.com:me/repo.git (push)\nup\thttps://example.com/r (fetch)\nup\thttps://example.com/r (push)\n';
  const trees = 'worktree /src/repo\nHEAD ' + OID + '\nbranch refs/heads/main\n\nworktree /src/wt\nHEAD ' + OID + '\ndetached\n\n'
    + 'worktree /src/wt é\nHEAD ' + OID + '\nbranch refs/heads/feat/x\nlocked\n\n';
  const R = parse.parseRefs(refs, remotes, trees);
  assert.deepStrictEqual(R.heads.map(b => [b.name, b.upstream, b.track, b.remote, b.tag]),
    [['feat/x', 'origin/feat/x', '[ahead 1, behind 2]', false, false], ['main', '', '', false, false]]);
  assert.deepStrictEqual(R.remotes.map(b => [b.name, b.remote]), [['origin/feat/x', true]]);
  assert.deepStrictEqual(R.tags.map(t => [t.name, t.tag, t.subject]), [['v1', true, 'fix: readme']]);
  assert.deepStrictEqual(R.urls, [{ name: 'origin', url: 'git@example.com:me/repo.git' }, { name: 'up', url: 'https://example.com/r' }]);
  assert.deepStrictEqual(R.worktrees, [{ path: '/src/repo', branch: 'main' }, { path: '/src/wt', branch: '(detached)' },
    { path: '/src/wt é', branch: 'feat/x' }]);
  assert.deepStrictEqual(parse.parseRefs('', '', ''), { heads: [], remotes: [], tags: [], urls: [], worktrees: [] });
});

test('branch comparison: cherry-marked commits, counts, numstat with binary and tab names', () => {
  const now = Math.floor(Date.now() / 1000);
  const c = parse.parseCompare(`>\x1fd5c4e81\x1f${'d'.repeat(40)}\x1ffix: readme\x1f${now - 60}\n=\x1fae3311f\x1f${'a'.repeat(40)}\x1fchore: z\x1f${now - 60}\n`);
  assert.deepStrictEqual(c.map(x => [x.same, x.short, x.subject]), [[false, 'd5c4e81', 'fix: readme'], [true, 'ae3311f', 'chore: z']]);
  assert.deepStrictEqual(parse.parseCount('3\t1\n'), [3, 1]);
  assert.deepStrictEqual(parse.parseNumstat('1\t0\tREADME.md\n-\t-\timg.png\n2\t2\ta\tb.txt\n'),
    [{ add: '1', del: '0', path: 'README.md' }, { add: '-', del: '-', path: 'img.png' }, { add: '2', del: '2', path: 'a\tb.txt' }]);
});

test('config scopes: values by key in order, and the keys the repository sets', () => {
  const out = 'global\0credential.helper\nstore\0local\0credential.helper\nevil\0local\0core.sshcommand\nsh -c x\0worktree\0http.proxy\0';
  const keys = parse.parseScopes(out);
  assert.deepStrictEqual([...keys], [['credential.helper', [['global', 'store'], ['local', 'evil']]],
    ['core.sshcommand', [['local', 'sh -c x']]], ['http.proxy', [['worktree', '']]]]);
  assert.deepStrictEqual(parse.repoKeys(out), ['credential.helper', 'core.sshcommand', 'http.proxy']);
  assert.deepStrictEqual(parse.repoKeys(''), []);
});

test('errorOf: fatal and error lines, refused pushes, no output', () => {
  assert.strictEqual(parse.errorOf({ code: 128, err: 'hint: x\nfatal: not a git repository\n' }), 'not a git repository');
  assert.strictEqual(parse.errorOf({ code: 1, err: 'error: pathspec \'x\' did not match\n' }), 'pathspec \'x\' did not match');
  assert.strictEqual(parse.errorOf({ code: 1, err: 'To x\n ! [rejected]        main -> main (non-fast-forward)\nerror: failed to push some refs\n' }),
    'rejected main -> main (non-fast-forward)');
  assert.strictEqual(parse.errorOf({ code: 1, err: '', out: 'nothing to commit\n' }), 'nothing to commit');
  assert.strictEqual(parse.errorOf({ code: null, err: '', out: '' }), 'timed out or not run');
  assert.strictEqual(parse.errorOf({ code: 1, err: '', out: '' }), 'failed');
});
