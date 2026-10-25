'use strict';
const path = require('path');
const Tree = require(path.join(__dirname, '..', '..', '..', 'src', 'explorer', 'tree'));

// Maps and Sets as arrays of their entries, in their order.
const plain = git => ({ files: [...git.files], dirs: [...git.dirs], untracked: [...git.untracked], ignored: [...git.ignored] });
// A row as the Rust Entry holds it, the fields left out when unset.
const row = r => ({ name: r.name, dir: r.dir, rel: r.rel, link: !!r.link, gone: !!r.gone, from: r.from ?? null,
  chain: r.chain ?? null, depth: r.depth, hit: r.hit ?? null, open: r.open });

const impl = {
  code: (x, y) => Tree.code(x, y),
  parseStatus: (status, prefix) => plain(Tree.parseStatus(status, prefix)),
  treeBefore: (a, aDir, b, bDir) => Tree.treeBefore(a, aDir, b, bDir),
  parentOf: rel => Tree.parentOf(rel),
  holds: (r, rel) => Tree.holds(r, rel),
  entryOrder: entries => [...entries].sort(Tree.ENTRY_ORDER).map(e => e.name),
  // The rows of a folder after a status: { query, changed, expanded }, settings { hidden, ignored, compact }.
  rows: (root, settings, status, view) => {
    const tree = Tree.create(() => root, settings);
    tree.applyStatus(status);
    return tree.rows({ query: view.query, changed: view.changed, expanded: new Set(view.expanded) }).map(row);
  },
  changedFiles: (root, status) => {
    const tree = Tree.create(() => root, { hidden: false, ignored: true });
    tree.applyStatus(status);
    return tree.changedFiles();
  },
};

const z = lines => lines.map(l => `${l}\0`).join('');
const files = {
  'b.txt': '', 'B.txt': '', 'README': '', 'readme': '', 'E': '', 'e': '', 'é': '', 'A': '', 'a': '', 'a10.txt': '', 'a9.txt': '', 'Zeta/x': '', 'alpha/inner/deep.md': '', 'alpha/x.js': '', '.hidden': '',
  '.git/HEAD': '', 'Été.md': '', 'eta.md': '', 'émile/f': '', 'src/main/java/App.java': '', 'only/one.txt': '',
  'node_modules/main.js': '', 'link': { link: '$ROOT/alpha' }, 'dead': { link: '$ROOT/nowhere' },
};
const rows = (settings, status, view) => ({ fn: 'rows', args: ['$ROOT', settings, z(status), view], files });
const all = { hidden: true, ignored: true, compact: false };

module.exports = {
  module: 'explorer/tree',
  impl,
  cases: [
    ...['??', '!!', 'UU', 'AA', 'DD', 'AU', 'UD', ' D', 'D ', 'R ', ' R', 'RM', 'C ', 'A ', 'AM', ' M', 'MM', ' T', 'XY']
      .map(x => ({ fn: 'code', args: [x[0], x[1]] })),
    { fn: 'parseStatus', args: [z([' M sub/a/x.js', 'D  sub/a/y.js', 'R  sub/new.js', 'sub/old.js', '?? sub/tmp/', '!! sub/dist/', ' M other/z.js']), 'sub'] },
    { fn: 'parseStatus', args: [z(['?? a/b/c.js', ' M a/d.js', 'UU a/b/e.js', 'A  f.js', 'C  g.js', 'h.js', ' D a/b/c/d/e.js']), ''] },
    { fn: 'parseStatus', args: [z(['?? x', ' M', 'M  é/ü.js', '!! .cache/']) + 'garbage', ''] },
    { fn: 'parseStatus', args: ['', ''] },
    { fn: 'parseStatus', args: [z(['é ab', '😀 cd', 'a😀b c', ' M ok']), ''] },
    { fn: 'parseStatus', args: [z([' M subdir/x', ' M sub/y']), 'sub'] },
    ...[['a/z.js', false, 'b.js', false], ['b.js', false, 'a/z.js', false], ['f2.js', false, 'f10.js', false],
      ['a', true, 'a/x.js', false], ['a/x.js', false, 'a', true], ['a', false, 'a', false], ['A/x', false, 'a/y', false],
      ['é', false, 'f', false], ['x/b', false, 'x/a/c', false], ['', false, 'a', false]]
      .map(args => ({ fn: 'treeBefore', args })),
    ...['', 'a', 'a/b', 'a/b/c', '/a', 'a/'].map(rel => ({ fn: 'parentOf', args: [rel] })),
    ...[[{ rel: 'src/main/java', from: 'src' }, 'src/main'], [{ rel: 'src/main/java', from: 'src' }, 'src'],
      [{ rel: 'src/main/java', from: 'src' }, 'src/main/java/App.java'], [{ rel: 'a', from: '' }, ''],
      [{ rel: 'lib' }, 'lib'], [{ rel: 'lib' }, 'lib/sub'], [{ rel: 'srcx/a', from: 'src' }, 'src']]
      .map(args => ({ fn: 'holds', args })),
    { fn: 'entryOrder', args: [[{ name: 'b', dir: false }, { name: 'B2', dir: true }, { name: 'a10', dir: false }, { name: 'a9', dir: false },
      { name: 'Été', dir: false }, { name: 'ete', dir: false }, { name: '_x', dir: false }, { name: '.git', dir: true }, { name: 'A', dir: false }, { name: 'a', dir: false }]] },
    rows(all, [], { expanded: [''] }),
    rows({ hidden: false, ignored: true, compact: false }, [], { expanded: ['', 'alpha', 'link', 'Zeta'] }),
    rows({ hidden: false, ignored: false, compact: true }, ['!! node_modules/'], { expanded: ['', 'src/main/java'] }),
    rows({ hidden: false, ignored: true, compact: true }, [], { expanded: ['', 'src'] }),
    rows(all, ['!! node_modules/'], { query: 'main', expanded: [] }),
    rows(all, [], { query: 'été', expanded: [] }),
    rows(all, [], { query: 'a', expanded: [] }),
    rows(all, [' M alpha/x.js', ' D alpha/gone.js', ' D old/f.js', '?? eta.md', '!! node_modules/'], { changed: true, expanded: [] }),
    rows({ hidden: false, ignored: true, compact: true }, [' M src/main/java/App.java', ' D .old/x'], { changed: true, expanded: [] }),
    rows(all, [' M alpha/x.js', ' D alpha/gone.js'], { changed: true, query: 'o', expanded: [] }),
    { fn: 'changedFiles', args: ['$ROOT', z([' M alpha/x.js', ' D alpha/gone.js', ' D old/f.js', '?? émile/', 'M  b.txt'])], files },
  ],
};
