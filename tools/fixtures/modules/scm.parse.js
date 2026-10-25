'use strict';
// git's output read by the source control pane (src/scm/parse.js), on well-formed and odd lines. Times are in the
// future (ago gives "1 min") or missing ("NaN y"), so that the fixtures do not age. Lines keep every field: the JS's
// undefined for a field missing is the Rust's empty string. parseScopes's Map is given as its entries.
const parse = require('../../../src/scm/parse');

const OID = '5de5c897f4428a5692f5231cb6fc7d24c977d804';
const Z = '0'.repeat(40);
const z = lines => lines.join('\0') + '\0';
const FUTURE = 4102444800; // 2100
const US = '\x1f';

const cases = [];
const add = (fn, ...list) => { for (const args of list) cases.push({ fn, args }); };

add('parseRepo', ['/repo\n/repo/.git\n.git\n', '/repo'], ['/a/b\r\n/a/b/.git\r\n../../x/.git\r\n', '/a/b/c/d'],
  ['/w\n/r/.git/worktrees/w\n/r/.git\n', '/w'], ['', '/repo'], ['\n/x\n', '/repo'], ['/a/../b/\n/b/.git/\n./.git\n', '/b']);

add('parseStatus',
  [z(['# branch.oid (initial)', '# branch.head main'])],
  [z([`# branch.oid ${OID}`, '# branch.head (detached)'])],
  [z([`# branch.oid ${OID}`, '# branch.head feat', '# branch.upstream origin/feat', '# branch.ab +12 -0'])],
  [z(['# branch.ab +x -1', '# branch.ab -1 +2', '# branch.ab ++3 -4'])],
  [z([
    `1 MM N... 100644 100644 100644 ${OID} ${OID} file10.txt`,
    `1 M. N... 100644 100644 100644 ${OID} ${OID} file2.txt`,
    `1 .M N... 100644 100644 100644 ${OID} ${OID} File1.txt`,
    `1 A. N... 000000 100644 100644 ${Z} ${OID} b c  d.txt`,
    `1 D. N... 100644 000000 000000 ${OID} ${Z} gone.txt`,
    `1 .T N... 100644 100644 120000 ${OID} ${OID} link`,
    `1 AD N... 000000 100644 000000 ${Z} ${OID} added then deleted`,
    `1 X. N... 100644 100644 100644 ${OID} ${OID} unknown.txt`,
  ])],
  [z([
    `2 R. N... 100644 100644 100644 ${OID} ${OID} R100 new name.js`, 'old name.js',
    `2 RM N... 100644 100644 100644 ${OID} ${OID} R80 moved.js`, 'was.js',
    `2 .R N... 100644 100644 100644 ${OID} ${OID} R100 y.js`, 'x.js',
    `2 C. N... 100644 100644 100644 ${OID} ${OID} C75 copy.js`, 'orig.js',
  ])],
  [z([
    `u UU N... 100644 100644 100644 100644 ${OID} ${OID} ${OID} b.txt`,
    `u AA N... 000000 100644 100644 100644 ${Z} ${OID} ${OID} a.txt`,
    `u DD N... 100644 000000 000000 000000 ${OID} ${Z} ${Z} dd.txt`,
    `u AU N... 000000 100644 000000 100644 ${Z} ${OID} ${Z} au.txt`,
    `u UD N... 100644 100644 000000 100644 ${OID} ${OID} ${Z} ud.txt`,
    `u UA N... 000000 000000 100644 100644 ${Z} ${Z} ${OID} ua.txt`,
    `u DU N... 100644 000000 100644 100644 ${OID} ${Z} ${OID} du space.txt`,
    `u XY N... 100644 100644 100644 100644 ${OID} ${OID} ${OID} odd.txt`,
  ])],
  [z(['? z.txt', '? A.txt', '? a.txt', '? é.md', '? e.md', '? dir/x 2.txt', '? dir/x 10.txt', '! ignored.txt', '# other'])],
  [z(['1 M', '1 ', '2 R', 'next'])], [''], ['\0\0'],
);

add('parseNames', [''], ['a\0b\0'], ['\n\nx\0\ny\n\0z'], ['same\0same\0']);

const hist = rows => rows.map(f => f.join(US)).join('\n') + '\n';
add('parseHistory',
  [hist([
    ['a'.repeat(40), 'aaaaaaa', 'Merge', 'Zoë', FUTURE, 'HEAD -> refs/heads/main, refs/remotes/origin/main, refs/remotes/origin/HEAD, tag: refs/tags/v1', `${'b'.repeat(40)} ${'c'.repeat(40)}`],
    ['b'.repeat(40), 'bbbbbbb', 'fix: x | y', 'T', FUTURE, '', 'd'.repeat(40)],
    ['d'.repeat(40), 'ddddddd', 'root', 'T', FUTURE, 'HEAD', ''],
    ['e'.repeat(40), 'eeeeeee', 'heads', 'T', FUTURE, 'refs/heads/HEAD, refs/heads/x, HEADS', ''],
    ['f'.repeat(40), 'fffffff', 'odd', 'T', 'x', 'refs/notes/x, refs/stash, refs/heads/, tag: refs/tags/a b', ' a  b '],
  ])],
  [''], ['\n\n'],
);

add('refList', [''], ['HEAD'], ['HEAD -> refs/heads/main'], ['tag: refs/tags/v1.0, refs/remotes/up/HEAD, refs/remotes/up/main'],
  ['refs/remotes/HEAD, refs/remotes/x/HEADS, refs/heads/a/HEAD'], ['HEAD, HEAD -> HEAD, refs/heads/x, ,refs/tags/t']);

const labels = (D, head, branch) => parse.labels(parse.refList(D), head, branch);
add('labels',
  ['tag: refs/tags/v1, refs/remotes/origin/old, refs/heads/main, HEAD -> refs/heads/feat, refs/remotes/origin/main, refs/remotes/origin/HEAD', true, 'feat'],
  ['tag: refs/tags/v1, refs/remotes/origin/old, refs/heads/main, HEAD -> refs/heads/feat, refs/remotes/origin/main', false, 'feat'],
  ['HEAD, refs/heads/main', true, ''],
  ['refs/heads/main, refs/remotes/origin/main, refs/remotes/up/main, refs/remotes/main', true, 'main'],
  ['refs/heads/a, refs/heads/b, refs/remotes/o/b, refs/remotes/o/a, refs/remotes/o/c, tag: refs/tags/x, tag: refs/tags/y', true, 'b'],
  ['', true, 'main'],
);

add('parseStashes', [''], ['stash@{0}\x1fWIP on main: a\x1f2 minutes ago\nstash@{1}\x1fOn feat: été\x1f3 days ago\n'],
  ['stash@{0}\x1fa\x1fb\x1fextra\n\n']);

const refs = rows => rows.map(f => f.join(US)).join('\n') + '\n';
add('parseRefs',
  [refs([
    ['refs/heads/feat/x', 'd5c4e81', 'origin/feat/x', '[ahead 1, behind 2]', '2 days ago', 'fix: readme'],
    ['refs/heads/main', 'ae3311f', '', '', '31 seconds ago', 'chore: z'],
    ['refs/remotes/origin/HEAD', 'ae3311f', '', '', '1 day ago', 'chore: z'],
    ['refs/remotes/origin/feat/x', '0520c69', '', '[gone]', '1 day ago', 'feat: initial'],
    ['refs/tags/v1', 'd5c4e81', '', '', '2 days ago', 'fix: readme'],
    ['refs/notes/commits', 'aaaaaaa', '', '', '', ''],
    ['refs/heads/', 'aaaaaaa', '', '', '', ''],
  ]),
  'origin\tgit@example.com:me/repo.git (fetch)\norigin\tgit@example.com:me/repo.git (push)\nup  https://e.com/r x (fetch)\nnone (fetch)\nempty  (fetch)\n\tlead x (fetch)\nlocal\t/path/to/repo (fetch)\n',
  `worktree /src/repo\nHEAD ${OID}\nbranch refs/heads/main\n\nworktree /src/wt\nHEAD ${OID}\ndetached\n\nworktree /src/wt é\nHEAD ${OID}\nbranch refs/heads/feat/x\nlocked\n\nworktree /bare\nbare\n\nnot a worktree\n\nworktree /e\nbranch refs/heads/\n`],
  ['', '', ''],
);

add('parseCompare', [''], [`>${US}d5c4e81${US}${'d'.repeat(40)}${US}fix: readme${US}${FUTURE}\n=${US}ae3311f${US}${'a'.repeat(40)}${US}chore: z${US}${FUTURE}\n<${US}1234567${US}${'1'.repeat(40)}${US}x\n`]);

add('parseCount', ['3\t1\n'], [''], ['  5  '], ['0\t0'], ['a b'], ['1\n\n2']);

add('parseNumstat', [''], ['1\t0\tREADME.md\n-\t-\timg.png\n2\t2\ta\tb.txt\n'], ['0\t0\told => new\n\n']);

add('parseScopes', [''], ['global\0credential.helper\nstore\0local\0credential.helper\nevil\0local\0core.sshcommand\nsh -c x\0worktree\0http.proxy\0'],
  ['local\0a.b\nline1\nline2\0command\0a.b\nz\0dangling']);

add('repoKeys', [''], ['global\0credential.helper\nstore\0local\0credential.helper\nevil\0local\0core.sshcommand\nsh -c x\0worktree\0http.proxy\0system\0x.y\n1\0command\0p.q\n2\0']);

add('errorOf',
  [{ code: 128, err: 'hint: x\nfatal: not a git repository\n', out: '' }],
  [{ code: 1, err: "error: pathspec 'x' did not match\n", out: '' }],
  [{ code: 1, err: 'To x\n ! [rejected]        main -> main (non-fast-forward)\nerror: failed to push some refs\n', out: '' }],
  [{ code: 1, err: ' ! [remote rejected] main -> main (pre-receive hook declined)\n', out: '' }],
  [{ code: 1, err: '! [rejected]\n', out: '' }],
  [{ code: 1, err: '', out: 'nothing to commit\n' }],
  [{ code: null, err: '', out: '' }],
  [{ code: 1, err: '', out: '' }],
  [{ code: 1, err: '  \r\n\r\n  warning: x  \r\nerror:no space\r\n', out: '' }],
  [{ code: 1, err: 'fatal: \n', out: '' }],
);

const impl = { ...parse, labels, parseScopes: out => [...parse.parseScopes(out)] };
module.exports = { module: 'scm/parse', cases, impl };
