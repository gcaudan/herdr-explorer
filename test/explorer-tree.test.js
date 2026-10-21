// The explorer's tree without the terminal: the rows of a folder with its expanded folders, their order, the filter,
// the changed files only, and the git decorations read from a status.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const Tree = require('../src/explorer/tree');

// A folder made of paths: a trailing / for a folder, anything else a file.
function folder(paths) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'explorer-tree-'));
  for (const p of paths) {
    if (p.endsWith('/')) fs.mkdirSync(path.join(root, p), { recursive: true });
    else { fs.mkdirSync(path.dirname(path.join(root, p)), { recursive: true }); fs.writeFileSync(path.join(root, p), ''); }
  }
  return root;
}
const shown = rows => rows.map(r => `${'  '.repeat(r.depth)}${r.name}${r.dir ? (r.open ? '/-' : '/+') : ''}`);
// A status line of git status --porcelain=v1 -z.
const status = lines => lines.map(l => `${l}\0`).join('');

test('rows: folders first, names in numeric order, expanded folders unfolded', t => {
  const root = folder(['b.txt', 'a10.txt', 'a9.txt', 'Zeta/', 'alpha/', 'alpha/inner/', 'alpha/inner/deep.md', 'alpha/x.js', '.hidden', '.git/HEAD']);
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const tree = Tree.create(() => root, { hidden: false, ignored: true });
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set(['']) })), ['alpha/+', 'Zeta/+', 'a9.txt', 'a10.txt', 'b.txt']);
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set(['', 'alpha', 'alpha/inner']) })),
    ['alpha/-', '  inner/-', '    deep.md', '  x.js', 'Zeta/+', 'a9.txt', 'a10.txt', 'b.txt']);
  // An expanded folder inside a collapsed one stays hidden.
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set(['alpha/inner']) })).slice(0, 2), ['alpha/+', 'Zeta/+']);
  assert.strictEqual(tree.rows({ expanded: new Set() })[1].rel, 'Zeta');
});

test('rows: dot files with the hidden setting, never .git; the cache until cleared', t => {
  const root = folder(['.hidden', '.git/HEAD', 'a.txt']);
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const settings = { hidden: true, ignored: true };
  const tree = Tree.create(() => root, settings);
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set() })), ['.hidden', 'a.txt']);
  settings.hidden = false;
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set() })), ['a.txt']);
  fs.writeFileSync(path.join(root, 'b.txt'), '');
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set() })), ['a.txt'], 'read from the cache');
  tree.cache.clear();
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set() })), ['a.txt', 'b.txt']);
});

test('rows: the filter keeps the matches and the folders holding one, unfolded, outside ignored folders', t => {
  const root = folder(['src/', 'src/main.js', 'src/util.js', 'docs/', 'docs/Main.md', 'node_modules/', 'node_modules/main.js', 'readme.md']);
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const tree = Tree.create(() => root, { hidden: false, ignored: true });
  tree.git.top = root;
  tree.applyStatus(status(['!! node_modules/']));
  const rows = tree.rows({ query: 'main', expanded: new Set() });
  // node_modules/main.js is not read: the ignored folder shows only when its own name matches.
  assert.deepStrictEqual(shown(rows), ['docs/-', '  Main.md', 'src/-', '  main.js']);
  assert.deepStrictEqual(rows.map(r => r.hit), [null, [0, 4], null, [0, 4]]);
  // The folder's own name matches: shown folded, its contents unread.
  assert.deepStrictEqual(shown(tree.rows({ query: 'node', expanded: new Set() })), ['node_modules/+']);
  assert.deepStrictEqual(tree.rows({ query: 'nothing', expanded: new Set() }), []);
});

test('git status: codes, folders with their strongest change, renames, prefix', () => {
  assert.deepStrictEqual(['??', '!!', 'UU', 'AA', 'DD', ' D', 'R ', 'A ', ' M', 'MM'].map(x => Tree.code(x[0], x[1])),
    ['U', '!', 'C', 'C', 'C', 'D', 'R', 'A', 'M', 'M']);
  const git = Tree.parseStatus(status([' M sub/a/x.js', 'D  sub/a/y.js', 'R  sub/new.js', 'sub/old.js', '?? sub/tmp/', '!! sub/dist/',
    ' M other/z.js']), 'sub');
  assert.deepStrictEqual([...git.files], [['a/x.js', 'M'], ['a/y.js', 'D'], ['new.js', 'R']]);
  assert.deepStrictEqual([...git.dirs], [['a', 'D'], ['', 'D']]);
  assert.deepStrictEqual([...git.untracked], ['tmp']);
  assert.deepStrictEqual([...git.ignored], ['dist']);
});

test('decorations: a file, a folder, and the contents of untracked and ignored folders', t => {
  const root = folder(['a/', 'a/x.js', 'a/y.js', 'new/', 'new/n.js', 'build/', 'build/out.js', 'clean.js']);
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const tree = Tree.create(() => root, { hidden: false, ignored: true });
  tree.applyStatus(status([' M a/x.js', 'A  a/y.js', '?? new/', '!! build/', ' D gone/old.js']));
  const deco = rel => tree.decoration({ rel, dir: fs.existsSync(path.join(root, rel)) && fs.statSync(path.join(root, rel)).isDirectory() });
  assert.deepStrictEqual(['a', 'a/x.js', 'a/y.js', 'new', 'new/n.js', 'build/out.js', 'clean.js'].map(deco),
    ['M', 'M', 'A', 'U', 'U', '!', undefined]);
  assert.deepStrictEqual(tree.git.gone, ['gone/old.js']);
  // A change inside a collapsed ignored folder is not visible.
  assert.strictEqual(tree.hidden('build/out.js', new Set()), true);
  assert.strictEqual(tree.hidden('build/out.js', new Set(['build'])), false);
  assert.strictEqual(tree.hidden('a/x.js', new Set()), false);
  // Ignored entries left out unless shown.
  assert.deepStrictEqual(shown(tree.rows({ expanded: new Set() })), ['a/+', 'build/+', 'new/+', 'clean.js']);
  const hiding = Tree.create(() => root, { hidden: false, ignored: false });
  hiding.applyStatus(status(['!! build/']));
  assert.deepStrictEqual(shown(hiding.rows({ expanded: new Set() })), ['a/+', 'new/+', 'clean.js']);
});

test('changed files only: every folder unfolded, deleted files among them, in the tree order', t => {
  const root = folder(['a/', 'a/x.js', 'a/same.js', 'b.js', 'c.js']);
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const tree = Tree.create(() => root, { hidden: false, ignored: true });
  tree.applyStatus(status([' M a/x.js', ' D a/gone.js', ' D old/f.js', '?? c.js']));
  const rows = tree.rows({ changed: true, expanded: new Set() });
  assert.deepStrictEqual(shown(rows), ['a/-', '  gone.js', '  x.js', 'old/-', '  f.js', 'c.js']);
  assert.deepStrictEqual(rows.filter(r => r.gone).map(r => r.rel), ['a/gone.js', 'old', 'old/f.js']);
  assert.deepStrictEqual(tree.changedFiles(), ['a/gone.js', 'a/x.js', 'old/f.js', 'c.js']);
  assert.deepStrictEqual(shown(tree.rows({ changed: true, query: 'x', expanded: new Set() })), ['a/-', '  x.js']);
});

test('tree order: folders before files at each level, names in numeric order', () => {
  assert.strictEqual(Tree.treeBefore('a/z.js', false, 'b.js', false), true);
  assert.strictEqual(Tree.treeBefore('b.js', false, 'a/z.js', false), false);
  assert.strictEqual(Tree.treeBefore('f2.js', false, 'f10.js', false), true);
  assert.strictEqual(Tree.treeBefore('a', true, 'a/x.js', false), true);
  assert.strictEqual(Tree.treeBefore('a/x.js', false, 'a', true), false);
  const entries = [{ name: 'b', dir: false }, { name: 'B2', dir: true }, { name: 'a10', dir: false }, { name: 'a9', dir: false }];
  assert.deepStrictEqual(entries.sort(Tree.ENTRY_ORDER).map(e => e.name), ['B2', 'a9', 'a10', 'b']);
  assert.deepStrictEqual(['', 'a', 'a/b', 'a/b/c'].map(Tree.parentOf), [null, '', 'a', 'a/b']);
});
