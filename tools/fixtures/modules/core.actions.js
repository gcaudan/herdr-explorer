'use strict';
// The file actions (create, rename, duplicate, remove), the paths and quotes they give, and the git actions against
// real repositories. quote, target and the web page test are not exported: taken from the source as written. The git
// cases build a repository under $ROOT/repo with a shell script, and a fake gio first on PATH (trash) that moves a
// file to $ROOT/trash, or fails when $ROOT/no-trash exists: the real trash is never reached.
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const actions = require('../../../src/core/actions');

const source = fs.readFileSync(path.join(__dirname, '..', '..', '..', 'src', 'core', 'actions.js'), 'utf8');
const lift = name => {
  const text = new RegExp(`\\nfunction ${name}\\([\\s\\S]*?\\n}\\n`).exec(source)[0];
  // eslint-disable-next-line no-new-func
  return new Function('WIN', 'path', `return (${text});`)(false, path);
};
const quote = lift('quote');
const target = lift('target');
const webPage = new RegExp(/if \(!\/(.*?)\/\.test\(url\)\)/.exec(source)[1]);

const sh = (cwd, script) => execFileSync('sh', ['-c', script], { cwd, encoding: 'utf8' });
const FAKES = `mkdir -p ../bin ../trash
printf '#!/bin/sh\\n[ -e "%s/no-trash" ] && exit 1\\n[ "$1" = trash ] && exec /bin/mv "$3" "%s/trash/"\\nexit 1\\n' "$PWD/.." "$PWD/.." > ../bin/gio
printf '#!/bin/sh\\nexit 1\\n' > ../bin/trash-put
chmod +x ../bin/gio ../bin/trash-put
git init -q -b main . && git config user.name t && git config user.email t@t && git config commit.gpgsign false
`;
const git = async (name, root, setup, rel) => {
  const top = path.join(root, 'repo');
  fs.mkdirSync(top);
  sh(top, FAKES + setup);
  const msg = await actions[name](path.join(top, rel), top);
  return [msg, sh(top, 'git status --porcelain=v1 --untracked-files=all'), fs.readdirSync(path.join(root, 'trash')).sort()];
};

const tree = dir => {
  const out = [];
  const walk = (d, rel) => {
    for (const n of fs.readdirSync(d).sort()) {
      const p = path.join(d, n), r = rel ? `${rel}/${n}` : n, st = fs.lstatSync(p);
      if (st.isSymbolicLink()) out.push(`${r} -> ${fs.readlinkSync(p)}`);
      else if (st.isDirectory()) { out.push(`${r}/`); walk(p, r); } else out.push(`${r}: ${fs.readFileSync(p, 'utf8')}`);
    }
  };
  walk(dir, '');
  return out;
};
// The call's result, then what the folder holds.
const after = fn => async (root, ...args) => [await fn(...args.map(a => (typeof a === 'string' ? a.split('@').join(root) : a))), tree(root)];

const impl = {
  quote,
  target,
  webPage: url => webPage.test(url),
  canPreview: actions.canPreview,
  canOpen: actions.canOpen,
  relativeTo: actions.relativeTo,
  create: after(actions.create),
  rename: after(actions.rename),
  duplicate: after(actions.duplicate),
  remove: after(actions.remove),
  git,
  sideSplit: (layout, pane) => {
    const f = actions.sideSplit(layout, pane);
    return f && { width: f.me.rect.width, splitWidth: f.split.rect.width, ratio: f.split.ratio, left: f.left };
  },
};

// Layouts: panes (id, x, y, width, height) and splits (direction, ratio, x, y, width, height).
const lay = (panes, splits) => ({
  panes: panes.map(([pane_id, x, y, width, height]) => ({ pane_id, rect: { x, y, width, height } })),
  splits: splits.map(([direction, ratio, x, y, width, height]) => ({ direction, ratio, rect: { x, y, width, height } })),
});
const TWO = lay([['a', 0, 0, 30, 40], ['b', 30, 0, 70, 40]], [['right', 0.3, 0, 0, 100, 40]]);
const NESTED = lay([['a', 0, 0, 25, 40], ['b', 25, 0, 50, 40], ['c', 75, 0, 25, 40]],
  [['right', 0.25, 0, 0, 100, 40], ['right', 0.6667, 25, 0, 75, 40]]);
const SPLITS = [[TWO, 'a'], [TWO, 'b'], [TWO, 'c'], [NESTED, 'a'], [NESTED, 'b'], [NESTED, 'c'],
  [lay([['a', 0, 0, 31, 40]], [['right', 0.3, 0, 0, 100, 40]]), 'a'],
  [lay([['a', 0, 0, 32, 40]], [['right', 0.3, 0, 0, 100, 40]]), 'a'],
  [lay([['a', 0, 0, 30, 40]], [['down', 0.3, 0, 0, 100, 40]]), 'a'],
  [lay([['a', 0, 0, 30, 20]], [['right', 0.3, 0, 0, 100, 40]]), 'a'],
  [lay([['a', 0, 0, 50, 40]], [['right', 0.505, 0, 0, 100, 40]]), 'a'],
  [lay([['b', 50, 0, 50, 40]], [['right', 0.495, 0, 0, 100, 40]]), 'b'],
  [lay([], []), 'a']];

const QUOTES = ['a.txt', 'src/app.js', 'é/日本.md', 'a b.txt', "it's", '-rf', '--x y', '', '$HOME', 'a;b', 'a`b`', 'a\\b',
  'a"b', '~/x', 'a*', '[id].ts', '@scope/pkg', 'v1.2+3,x=y:z', '‘q’', 'á', '½', 'tab\tx', 'a b'];
const TARGETS = [['/r', 'a.txt'], ['/r', ' a/b.txt '], ['/r', '../x'], ['/r', 'a/../../x'], ['/r', 'a/..'], ['/r', '.'],
  ['/r', ''], ['/r', '   '], ['/r', '/abs'], ['/r', 'a\nb'], ['/r', 'a\u0085b'], ['/r', 'a '], ['/r', 'x//y/'],
  ['/r', '..x'], ['/', 'etc'], ['/r', ' a']];
const URLS = ['https://github.com/gcaudan/herdr-explorer#readme', 'https://a.b/', 'https://a.b', 'http://a.b/x',
  'https:///x', 'https://a.b/x y', 'https://a.b/x?q=1', 'https://a_b-c.d/x/%20#y', 'https://a.b/x\n', 'file:///etc',
  'https://é.fr/x', 'https://a.b/./../x', 'javascript:alert(1)//https://a.b/'];
const NAMES = ['a.png', 'A.PDF', 'x.docx', 'x.mov', 'x.txt', 'x.md', 'x.sh', 'x', '.png', 'x.tar.gz', 'x.svg', 'x.HTML',
  'x.exe', 'x.ico.', 'dir.zip/x.js'];

const files = { 'a.txt': 'A', 'f': 'F', 'd/x.md': 'X', 'd/s/y': 'Y', 'd/l': { link: 'x.md' }, 'broken': { link: 'none' },
  'up': { link: '$ROOT/d' }, 'a copy.txt': 'C', '.rc': 'R', 'k.tar.gz': 'K' };
const at = (fn, args, extra = {}) => ({ fn, args: ['$ROOT', ...args], files: { ...files, ...extra } });

module.exports = {
  module: 'core/actions',
  impl,
  cases: [
    ...QUOTES.map(q => ({ fn: 'quote', args: [q] })),
    ...TARGETS.map(args => ({ fn: 'target', args })),
    ...SPLITS.map(args => ({ fn: 'sideSplit', args })),
    ...URLS.map(u => ({ fn: 'webPage', args: [u] })),
    ...NAMES.map(n => ({ fn: 'canPreview', args: [`/x/${n}`] })),
    ...NAMES.map(n => ({ fn: 'canOpen', args: [`/nowhere/${n}`] })),
    { fn: 'canOpen', args: ['$ROOT/doc.pdf'], files: { 'doc.pdf': { link: 'run.sh' }, 'run.sh': '' } },
    { fn: 'canOpen', args: ['$ROOT/doc.pdf'], files: { 'doc.pdf': { link: 'real.PDF' }, 'real.PDF': '' } },
    { fn: 'canOpen', args: ['$ROOT/notes.txt'], files: { 'notes.txt': { link: 'missing.sh' } } },
    { fn: 'relativeTo', args: ['/a/b', '/a/b/c/d.txt'] },
    { fn: 'relativeTo', args: ['/a/b', '/a/b'] },
    { fn: 'relativeTo', args: ['/a/b', '/a/x.txt'] },
    { fn: 'relativeTo', args: ['', '/a/x.txt'] },
    { fn: 'relativeTo', args: ['/a/b', '/a/b/..x'] },

    ...['new.txt', 'n/e/w.txt', 'folder/', 'folder\\', 'a.txt', 'broken', '../out', '', 'f/x', 'd/s', 'sp ', 'x/ ']
      .map(n => at('create', ['$ROOT', n, false])),
    at('create', ['$ROOT', 'dir', true]),
    at('create', ['$ROOT', 'd/s/z', true]),

    ...[['a.txt', 'b.txt'], ['a.txt', 'a.txt'], ['a.txt', ' a.txt '], ['a.txt', 'f'], ['a.txt', 'sub/b.txt'],
      ['a.txt', '../b.txt'], ['a.txt', 'A.txt'], ['d/l', 'm'], ['d', 'e/f'], ['broken', 'b2'], ['a.txt', 'broken'],
      ['nothing', 'x']]
      .map(([from, to]) => at('rename', [`@/${from}`, to])),

    ...['a.txt', 'f', 'd', 'up', 'd/l', '.rc', 'k.tar.gz', 'd/s/y'].map(n => at('duplicate', [`@/${n}`])),
    at('duplicate', ['@/a.txt'], { 'a copy 2.txt': '2', 'a copy 3.txt': '3' }),
    at('duplicate', ['@/x'], { x: 'x', 'x copy': null }),

    ...['a.txt', 'd', 'up', 'd/l', 'broken', 'nothing'].map(n => at('remove', [`@/${n}`])),

    { fn: 'git', args: ['stage', '$ROOT', 'echo a > a; echo b > b', 'a'] },
    { fn: 'git', args: ['stage', '$ROOT', 'echo a > a; git add a; git commit -qm 1; rm a', 'a'] },
    { fn: 'git', args: ['stage', '$ROOT', 'mkdir d; echo x > "d/*"; echo y > d/y', 'd/*'] },
    { fn: 'git', args: ['stage', '$ROOT', 'true', 'missing'] },
    { fn: 'git', args: ['unstage', '$ROOT', 'echo a > a; git add a', 'a'] },
    { fn: 'git', args: ['unstage', '$ROOT', 'echo a > a; git add a; git commit -qm 1; echo b > a; git add a', 'a'] },
    { fn: 'git', args: ['unstage', '$ROOT', 'mkdir d; echo a > d/a; echo b > d/b; git add d', 'd'] },
    { fn: 'git', args: ['unstage', '$ROOT', 'echo a > a; git add a; git commit -qm 1; git rm -q a', 'a'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a; git commit -qm 1; echo b > a; git add a; echo c > a', 'a'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a; git commit -qm 1; echo n > n; git add n', 'n'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a; git commit -qm 1; git rm -q a', 'a'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a; git commit -qm 1; rm a', 'a'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a; git commit -qm 1; echo u > u', 'u'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a', 'a'] },
    { fn: 'git', args: ['discard', '$ROOT', 'mkdir d; echo a > d/a; git add d; git commit -qm 1; echo x > d/a; echo u > d/u; echo v > d/v', 'd'] },
    { fn: 'git', args: ['discard', '$ROOT', 'touch ../no-trash; mkdir d; echo a > d/a; git add d; git commit -qm 1; echo u > d/u; echo v > d/v', 'd'] },
    { fn: 'git', args: ['discard', '$ROOT', 'touch ../no-trash; echo a > a; git add a; git commit -qm 1; echo u > u', 'u'] },
    { fn: 'git', args: ['discard', '$ROOT', 'echo a > a; git add a; git commit -qm 1; echo i > .gitignore; echo z > i; git add .gitignore; git commit -qm 2', 'i'] },
  ].map(c => (c.fn === 'git' ? { ...c, env: { PATH: '$ROOT/bin:/usr/bin:/bin' } } : c)),
};
