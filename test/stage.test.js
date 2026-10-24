// Staging, unstaging and discarding parts of a diff: the patches made from the rows selected, applied in a repository.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const diff = require('../src/diff');
const stage = require('../src/core/stage');

// A repository with one commit of the files given.
function repo(files) {
  const top = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'stage-')));
  const git = (...a) => spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'core.autocrlf=false', ...a],
    { cwd: top, encoding: 'utf8' });
  if (git('init', '-q').status !== 0) return null;
  git('config', 'core.autocrlf', 'false'); // the patches applied to the working tree too, whatever the global config
  for (const [f, text] of Object.entries(files)) fs.writeFileSync(path.join(top, f), text);
  git('add', '-A');
  git('commit', '-q', '-m', 'init');
  return { top, git, write: (f, text) => fs.writeFileSync(path.join(top, f), text), read: f => fs.readFileSync(path.join(top, f), 'utf8') };
}
// What the rows whose text includes one of the strings given select, in a section.
const plain = r => r.segs.map(([t]) => t).join('');
const pick = (list, sec, ...texts) => stage.picked(list.filter(r => r.ref && r.ref.sec === sec && texts.some(t => plain(r).includes(t))));
const lines = n => Array.from({ length: n }, (_, i) => `line ${i + 1}`).join('\n') + '\n';

test('a hunk, then a line, go to the index; the rest stays unstaged', async () => {
  const r = repo({ 'a.txt': lines(30) });
  if (!r) return;
  r.write('a.txt', r.read('a.txt').replace('line 2\n', 'line two\n').replace('line 28\n', 'line 28 changed\nline 28b\n'));
  const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  // The second hunk, from its @@ row.
  let list = diff.render(parsed, 200, 'inline');
  const hunks = list.filter(x => x.h && x.ref.sec === 'unstaged');
  assert.strictEqual(hunks.length, 2);
  assert.strictEqual(await stage.apply(r.top, parsed, stage.picked([hunks[1]]), 'stage'), 'Staged 3 lines');
  assert.match(r.git('diff', '--cached').stdout, /\+line 28b/);
  assert.doesNotMatch(r.git('diff', '--cached').stdout, /line two/);
  // One added line of the first hunk: the removed one stays in the index.
  const p2 = diff.changesOf(await diff.collect({ cwd: r.top }));
  list = diff.render(p2, 200, 'inline');
  assert.strictEqual(await stage.apply(r.top, p2, pick(list, 'unstaged', 'line two'), 'stage'), 'Staged 1 line');
  const index = r.git('show', ':a.txt').stdout;
  assert.match(index, /line 1\nline 2\nline two\nline 3\n/);
  assert.match(index, /line 28 changed\nline 28b\n/);
});

test('a staged line goes back, an unstaged one is discarded', async () => {
  const r = repo({ 'a.txt': lines(5) });
  if (!r) return;
  r.write('a.txt', 'line 1\nnew A\nline 2\nline 3\nnew B\nline 4\nline 5\n');
  r.git('add', 'a.txt');
  let parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  let list = diff.render(parsed, 200, 'split');
  assert.strictEqual(await stage.apply(r.top, parsed, pick(list, 'staged', 'new A'), 'unstage'), 'Unstaged 1 line');
  assert.strictEqual(r.git('show', ':a.txt').stdout, 'line 1\nline 2\nline 3\nnew B\nline 4\nline 5\n');
  parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  list = diff.render(parsed, 200, 'inline');
  assert.strictEqual(stage.count(parsed, pick(list, 'unstaged', 'new A'), 'discard'), '1 line');
  assert.strictEqual(await stage.apply(r.top, parsed, pick(list, 'unstaged', 'new A'), 'discard'), 'Discarded 1 line');
  assert.strictEqual(r.read('a.txt'), 'line 1\nline 2\nline 3\nnew B\nline 4\nline 5\n');
  // Nothing of the other section.
  assert.strictEqual(await stage.apply(r.top, parsed, pick(list, 'staged', 'new B'), 'stage'), 'Nothing unstaged selected');
});

test('part of an untracked file is staged; a whole one is discarded from the disk', async () => {
  const r = repo({ 'a.txt': 'x\n' });
  if (!r) return;
  r.write('new.txt', 'one\ntwo\nthree\n');
  r.write('gone.txt', 'bye\n');
  let parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  let list = diff.render(parsed, 200, 'inline');
  assert.strictEqual(await stage.apply(r.top, parsed, pick(list, 'unstaged', '+ two'), 'stage'), 'Staged 1 line');
  assert.strictEqual(r.git('show', ':new.txt').stdout, 'two\n');
  parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  list = diff.render(parsed, 200, 'inline');
  const file = list.filter(x => x.f && x.f.to === 'gone.txt');
  // To the trash; without one, kept unless deleted for good once asked.
  const trashed = [];
  const trash = async abs => { trashed.push(abs); fs.rmSync(abs); return true; };
  assert.strictEqual(await stage.apply(r.top, parsed, stage.picked(file), 'discard', { trash }), 'Discarded 1 line');
  assert.deepStrictEqual(trashed, [path.join(r.top, 'gone.txt')]);
  r.write('gone.txt', 'bye\n');
  const noTrash = { trash: async () => false };
  assert.strictEqual(await stage.apply(r.top, parsed, stage.picked(file), 'discard', { ...noTrash, noTrash: async () => false }), '1 untracked file kept: no trash');
  assert.ok(fs.existsSync(path.join(r.top, 'gone.txt')));
  assert.strictEqual(await stage.apply(r.top, parsed, stage.picked(file), 'discard', { ...noTrash, noTrash: async () => true }), 'Discarded 1 line');
  assert.ok(!fs.existsSync(path.join(r.top, 'gone.txt')));
});

test('a file added with git add -N keeps its entry when staged in part, loses it when discarded', async () => {
  const r = repo({ 'a.txt': 'x\n' });
  if (!r) return;
  r.write('n.txt', 'one\ntwo\n');
  r.git('add', '-N', 'n.txt');
  let parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  let list = diff.render(parsed, 200, 'inline');
  assert.strictEqual(await stage.apply(r.top, parsed, pick(list, 'unstaged', '+ one'), 'stage'), 'Staged 1 line');
  assert.strictEqual(r.git('show', ':n.txt').stdout, 'one\n');
  r.git('reset', '-q', '--', 'n.txt');
  r.git('add', '-N', 'n.txt');
  parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  list = diff.render(parsed, 200, 'inline');
  const file = list.filter(x => x.f && x.f.to === 'n.txt' && x.f.sec === 'unstaged');
  assert.strictEqual(await stage.apply(r.top, parsed, stage.picked(file), 'discard', { trash: async abs => { fs.rmSync(abs); return true; } }), 'Discarded 2 lines');
  assert.strictEqual(r.git('status', '--porcelain').stdout, '');
});

test('lines ending with CRLF keep it in the patch', async () => {
  const r = repo({ 'w.txt': 'a\r\nb\r\nc\r\n' });
  if (!r) return;
  r.write('w.txt', 'a\r\nB\r\nc\r\nd\r\n');
  const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  const list = diff.render(parsed, 200, 'inline');
  assert.strictEqual(await stage.apply(r.top, parsed, pick(list, 'unstaged', '+ d'), 'stage'), 'Staged 1 line');
  assert.strictEqual(r.git('show', ':w.txt').stdout, 'a\r\nb\r\nc\r\nd\r\n');
});

test('paths with quotes or backslashes are quoted as git does', () => {
  assert.strictEqual(stage.quote('a/plain name.txt'), 'a/plain name.txt');
  assert.strictEqual(stage.quote('a/say "hi"\\.txt'), '"a/say \\"hi\\"\\\\.txt"');
  assert.strictEqual(stage.quote('a/tab\there'), '"a/tab\\there"');
});

test('a line added after a last line without newline is staged on a line of its own', async () => {
  const r = repo({ 'f.txt': 'a\nb' });
  if (!r) return;
  r.write('f.txt', 'a\nb\nc\n');
  const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  const f = parsed.files.find(x => x.sec === 'unstaged');
  const c = f.hunks[0].lines.find(l => l[0] === '+' && l[1] === 'c');
  const sel = { files: new Set(), hunks: new Set(), lines: new Set([c]) };
  assert.strictEqual(await stage.apply(r.top, parsed, sel, 'stage'), 'Staged 1 line');
  assert.strictEqual(r.git('show', ':f.txt').stdout, 'a\nb\nc\n');
});

test('a last line without newline discarded alone comes back before the line added', async () => {
  for (const now of ['a\nnew\n', 'a\nnew']) {
    const r = repo({ 'f.txt': 'a\nold' });
    if (!r) return;
    r.write('f.txt', now);
    const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
    const f = parsed.files.find(x => x.sec === 'unstaged');
    const old = f.hunks[0].lines.find(l => l[0] === '-' && l[1] === 'old');
    const sel = { files: new Set(), hunks: new Set(), lines: new Set([old]) };
    assert.strictEqual(await stage.apply(r.top, parsed, sel, 'discard'), 'Discarded 1 line');
    assert.strictEqual(r.read('f.txt'), `a\nold\n${now.slice(2)}`);
  }
});

test('part of a staged deletion goes back to the index', async () => {
  const r = repo({ 'g.txt': 'x\ny\nz\n' });
  if (!r) return;
  r.git('rm', '-q', 'g.txt');
  const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  const g = parsed.files.find(x => x.sec === 'staged');
  const y = g.hunks[0].lines.find(l => l[1] === 'y');
  const sel = { files: new Set(), hunks: new Set(), lines: new Set([y]) };
  assert.strictEqual(await stage.apply(r.top, parsed, sel, 'unstage'), 'Unstaged 1 line');
  assert.strictEqual(r.git('show', ':g.txt').stdout, 'y\n');
});

test('a whole file takes its mode change with it', () => {
  const l = ['+', 'n'];
  const f = { from: 'h.sh', to: 'h.sh', status: 'modified', oldMode: '100644', newMode: '100755', hunks: [{ head: '@@ -1 +1,2 @@', lines: [[' ', 'm'], l] }] };
  assert.match(stage.patchOf({ f, keep: new Set([l]), all: true }, 'stage'), /^old mode 100644\nnew mode 100755\n--- a\/h.sh/m);
  assert.doesNotMatch(stage.patchOf({ f, keep: new Set([l]), all: false }, 'stage'), /old mode/);
});

test('part of a replaced block keeps the lines in their order', async () => {
  for (const [mode, pickText, index, tree] of [
    ['discard', 'c', 'a\nb\nc\nz\n', 'a\nB\nc\nz\n'],
    ['stage', 'b', 'a\nB\nc\nz\n', 'a\nB\nC\nz\n'],
    ['unstage', 'c', 'a\nB\nc\nz\n', 'a\nB\nC\nz\n'],
  ]) {
    const r = repo({ 'f.txt': 'a\nb\nc\nz\n' });
    if (!r) return;
    r.write('f.txt', 'a\nB\nC\nz\n');
    if (mode === 'unstage') r.git('add', 'f.txt');
    const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
    const f = parsed.files.find(x => x.sec === stage.SECTION[mode]);
    const picks = f.hunks[0].lines.filter(l => l[1].toLowerCase() === pickText);
    assert.strictEqual(await stage.apply(r.top, parsed, { files: new Set(), hunks: new Set(), lines: new Set(picks) }, mode), `${stage.SECTION[mode] === 'staged' ? 'Unstaged' : mode === 'stage' ? 'Staged' : 'Discarded'} 2 lines`);
    assert.strictEqual(r.git('show', ':f.txt').stdout, index);
    assert.strictEqual(r.read('f.txt'), tree);
  }
});

test('the last line added to a file without newline is taken out without adding one', async () => {
  for (const mode of ['discard', 'unstage']) {
    const r = repo({ 'f.txt': 'a\nb' });
    if (!r) return;
    r.write('f.txt', 'a\nb\nc');
    if (mode === 'unstage') r.git('add', 'f.txt');
    const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
    const f = parsed.files.find(x => x.sec === stage.SECTION[mode]);
    const c = f.hunks[0].lines.find(l => l[0] === '+' && l[1] === 'c');
    await stage.apply(r.top, parsed, { files: new Set(), hunks: new Set(), lines: new Set([c]) }, mode);
    assert.strictEqual(mode === 'discard' ? r.read('f.txt') : r.git('show', ':f.txt').stdout, 'a\nb');
  }
});

// chmod does not stop root, nor Windows.
const noPerms = process.platform === 'win32' || (process.getuid && process.getuid() === 0);

test('discarding for good goes on after a file it cannot delete', { skip: noPerms }, async () => {
  const r = repo({ 'a.txt': 'x\n' });
  if (!r) return;
  fs.mkdirSync(path.join(r.top, 'ro'));
  r.write('ro/a.txt', 'a\n');
  r.write('b.txt', 'b\n');
  const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  const list = diff.render(parsed, 200, 'inline');
  const files = list.filter(x => x.f && (x.f.to === 'ro/a.txt' || x.f.to === 'b.txt'));
  fs.chmodSync(path.join(r.top, 'ro'), 0o555);
  try {
    const said = await stage.apply(r.top, parsed, stage.picked(files), 'discard', { trash: async () => false, noTrash: async () => true });
    assert.strictEqual(said, 'Discarded 1 line; cannot delete ro/a.txt: EACCES');
    assert.ok(fs.existsSync(path.join(r.top, 'ro/a.txt')));
    assert.ok(!fs.existsSync(path.join(r.top, 'b.txt')));
  } finally { fs.chmodSync(path.join(r.top, 'ro'), 0o755); }
});

test('whole files failing after a patch say what was staged', { skip: noPerms }, async () => {
  const r = repo({ 'a.txt': 'one\n' });
  if (!r) return;
  r.write('a.txt', 'one\ntwo\n');
  r.write('u.txt', 'u\n');
  const parsed = diff.changesOf(await diff.collect({ cwd: r.top }));
  const list = diff.render(parsed, 200, 'inline');
  const sel = stage.picked([...list.filter(x => x.ref && x.ref.sec === 'unstaged' && plain(x).includes('+ two')),
    ...list.filter(x => x.f && x.f.to === 'u.txt')]);
  fs.chmodSync(path.join(r.top, 'u.txt'), 0);
  try {
    assert.match(await stage.apply(r.top, parsed, sel, 'stage'), /^Staged 1 line; cannot stage u\.txt: /);
    assert.match(r.git('diff', '--cached').stdout, /\+two/);
  } finally { fs.chmodSync(path.join(r.top, 'u.txt'), 0o644); }
});

test('within refuses a path whose folder links outside the repository', { skip: process.platform === 'win32' }, () => {
  const top = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'within-')));
  const out = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'outside-')));
  fs.mkdirSync(path.join(top, 'dir'));
  fs.symlinkSync(out, path.join(top, 'sub'));
  assert.ok(stage.within(path.join(top, 'a.txt'), top));
  assert.ok(stage.within(path.join(top, 'dir/a.txt'), top));
  assert.ok(!stage.within(path.join(top, 'sub/x'), top));
  assert.ok(!stage.within(path.join(top, '../x'), top));
});

test('the lines changed since HEAD, staged or not, as the File view marks them', async () => {
  const r = repo({ 'a.txt': lines(10), '.gitignore': 'ignored.txt\n' });
  if (!r) return;
  r.write('a.txt', r.read('a.txt').replace('line 2\n', 'line two\n').replace('line 5\n', '').replace('line 10\n', '').replace('line 7\n', 'line 7\nline 7b\n'));
  r.git('add', 'a.txt');
  r.write('a.txt', `line 0\n${r.read('a.txt')}`);
  const res = await diff.lineChanges(path.join(r.top, 'a.txt'), 10);
  // line 0, line 1, line two, line 3, line 4, line 6, line 7, line 7b, line 8, line 9
  assert.deepStrictEqual([...res.kinds.keys()].filter(i => res.kinds[i]).map(i => [i, res.kinds[i]]), [[0, 'add'], [2, 'mod'], [7, 'add']]);
  assert.deepStrictEqual([...res.deleted].sort((a, b) => a - b), [5, 10]);
  r.write('new.txt', 'a\nb\n');
  assert.deepStrictEqual((await diff.lineChanges(path.join(r.top, 'new.txt'), 2)).kinds, ['add', 'add']);
  r.write('ignored.txt', 'a\n');
  assert.deepStrictEqual((await diff.lineChanges(path.join(r.top, 'ignored.txt'), 1)).kinds, []);
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'nogit-'));
  fs.writeFileSync(path.join(out, 'f.txt'), 'a\n');
  if (spawnSync('git', ['rev-parse'], { cwd: out }).status !== 0) assert.strictEqual(await diff.lineChanges(path.join(out, 'f.txt'), 1), null);
});
