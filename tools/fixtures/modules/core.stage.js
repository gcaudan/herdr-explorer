'use strict';
// Staging, unstaging and discarding against real repositories. Each case builds one under $ROOT/repo with a shell
// script (setup), reads the diffs the Changes pane would show (diffs: [section, ...git args]), selects files, hunks
// and lines by their place, maybe changes the repository again (then), and calls the function. apply gives back its
// message and what git then shows; its trash moves the file to $ROOT/trash, or has none (trash: false), and noTrash
// answers with noTrash.
const fs = require('fs');
const path = require('path');
const { execFileSync, spawnSync } = require('child_process');
const stage = require('../../../src/core/stage');
const { parse, DIFF_OPTS } = require('../../../src/diff');

const sh = script => execFileSync('sh', ['-c', script], { encoding: 'utf8' });
const git = (top, args) => spawnSync('git', args, { cwd: top, encoding: 'utf8' }).stdout;

function load({ top, setup, diffs = [], sel = {}, then }) {
  sh(setup);
  const files = [];
  for (const [sec, ...args] of diffs) for (const f of parse(git(top, args)).files) files.push({ ...f, sec });
  const s = {
    files: new Set((sel.files || []).map(i => files[i])),
    hunks: new Set((sel.hunks || []).map(([f, h]) => files[f].hunks[h])),
    lines: new Set((sel.lines || []).map(([f, h, l]) => files[f].hunks[h].lines[l])),
  };
  if (then) sh(then);
  return { parsed: { pre: [], files, add: 0, del: 0 }, s };
}

// What the repository holds after: the status, both diffs, and what went to the trash.
const state = (top, trashDir) => ({
  status: git(top, ['status', '--porcelain=v1', '-uall']),
  diff: git(top, ['diff', ...DIFF_OPTS]),
  cached: git(top, ['diff', '--cached', ...DIFF_OPTS]),
  trash: fs.existsSync(trashDir) ? fs.readdirSync(trashDir).sort() : [],
});

const impl = {
  patches: c => { const { parsed, s } = load(c); return stage.targets(parsed, s, c.mode).map(t => stage.patchOf(t, c.mode)); },
  count: c => { const { parsed, s } = load(c); return stage.count(parsed, s, c.mode); },
  apply: async c => {
    const { parsed, s } = load(c);
    const trashDir = path.join(c.top, '..', 'trash');
    const trash = async p => {
      if (c.trash === false) return false;
      fs.mkdirSync(trashDir, { recursive: true });
      fs.renameSync(p, path.join(trashDir, path.basename(p)));
      return true;
    };
    const noTrash = c.noTrash === undefined ? undefined : async () => c.noTrash;
    const message = await stage.apply(c.top, parsed, s, c.mode, { trash, noTrash });
    return { message, ...state(c.top, trashDir) };
  },
  quote: stage.quote,
  refused: stage.refused,
  located: stage.located,
  pruneDirs: (abs, top) => { stage.pruneDirs(abs, top); return sh(`cd '${top}' && find . | sort`); },
};

const ENV = {
  HOME: '$ROOT/home', XDG_CONFIG_HOME: '$ROOT/home', GIT_CONFIG_NOSYSTEM: '1', LC_ALL: 'C', LANG: 'C',
  GIT_AUTHOR_NAME: 'A', GIT_AUTHOR_EMAIL: 'a@example.com', GIT_AUTHOR_DATE: '2024-01-01T00:00:00Z',
  GIT_COMMITTER_NAME: 'A', GIT_COMMITTER_EMAIL: 'a@example.com', GIT_COMMITTER_DATE: '2024-01-01T00:00:00Z',
};
const top = '$ROOT/repo';
const init = `set -e; mkdir -p $ROOT/home $ROOT/repo; cd $ROOT/repo; git init -q -b main;`;
const lines = n => Array.from({ length: n }, (_, i) => `l${i + 1}`).join('\\n');
const repo = (more = '', file = `${lines(12)}\\n`) => `${init} printf '${file}' > a.txt; git add -A; git commit -qm init; ${more}`;
const U = ['unstaged', 'diff', ...DIFF_OPTS];
const S = ['staged', 'diff', '--cached', ...DIFF_OPTS];
const untracked = name => ['unstaged', 'diff', '--no-index', ...DIFF_OPTS, '--', '/dev/null', name];

// Two hunks in a.txt: l2 changed, l10 removed and two lines added after l11.
const edit = `sed -i -e 's/^l2$/L2/' -e '/^l10$/d' -e 's/^l11$/l11\\nn1\\nn2/' a.txt;`;
const noNl = `${init} printf 'a\\nb\\nc' > f; git add -A; git commit -qm init;`;

const cases = [
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { files: [0] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { hunks: [[0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { lines: [[0, 0, 2]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { lines: [[0, 1, 3], [0, 1, 5]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { lines: [[0, 1, 4]] }, mode: 'discard' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { files: [0] }, mode: 'discard' } },
  { fn: 'apply', c: { setup: repo(`${edit} git add a.txt;`), diffs: [S], sel: { lines: [[0, 0, 1], [0, 1, 3]] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: repo(`${edit} git add a.txt;`), diffs: [S], sel: { files: [0] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: {}, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { files: [0] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: repo(edit), diffs: [U], sel: { hunks: [[0, 0]] }, mode: 'stage', then: "cd $ROOT/repo; printf 'x\\n' > a.txt" } },
  // Untracked files.
  { fn: 'apply', c: { setup: repo("printf 'n1\\nn2\\nn3\\n' > new.txt;"), diffs: [untracked('new.txt')], sel: { files: [0] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo("printf 'n1\\nn2\\nn3\\n' > new.txt;"), diffs: [untracked('new.txt')], sel: { lines: [[0, 0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo("mkdir -p d/e; printf 'n1\\n' > d/e/new.txt;"), diffs: [untracked('d/e/new.txt')], sel: { files: [0] }, mode: 'discard' } },
  { fn: 'apply', c: { setup: repo("mkdir -p d; printf 'n1\\n' > d/new.txt;"), diffs: [untracked('d/new.txt')], sel: { files: [0] }, mode: 'discard', trash: false } },
  { fn: 'apply', c: { setup: repo("mkdir -p d; printf 'n1\\n' > d/new.txt;"), diffs: [untracked('d/new.txt')], sel: { files: [0] }, mode: 'discard', trash: false, noTrash: true } },
  { fn: 'apply', c: { setup: repo("mkdir -p d; printf 'n1\\n' > d/new.txt;"), diffs: [untracked('d/new.txt')], sel: { files: [0] }, mode: 'discard', trash: false, noTrash: false } },
  { fn: 'apply', c: { setup: repo("printf 'n1\\n' > new.txt; git add -N new.txt;"), diffs: [U], sel: { files: [0] }, mode: 'discard' } },
  { fn: 'apply', c: { setup: repo("printf 'n1\\nn2\\n' > new.txt; git add -N new.txt;"), diffs: [U], sel: { lines: [[0, 0, 0]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(`${edit} printf 'n1\\n' > new.txt;`), diffs: [U, untracked('new.txt')], sel: { files: [0, 1] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(`${edit} printf 'n1\\n' > new.txt;`), diffs: [U, untracked('new.txt')], sel: { files: [0, 1] }, mode: 'discard', trash: false, noTrash: true } },
  { fn: 'apply', c: { setup: repo(`${edit} printf 'n1\\n' > new.txt;`), diffs: [U, untracked('new.txt')], sel: { files: [0, 1] }, mode: 'discard', trash: false, noTrash: true, then: 'rm $ROOT/repo/new.txt' } },
  // Deletions.
  { fn: 'apply', c: { setup: repo('rm a.txt;'), diffs: [U], sel: { lines: [[0, 0, 0], [0, 0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo('rm a.txt;'), diffs: [U], sel: { files: [0] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo('git rm -q a.txt;'), diffs: [S], sel: { lines: [[0, 0, 0], [0, 0, 1]] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: repo('rm a.txt;'), diffs: [U], sel: { lines: [[0, 0, 11]] }, mode: 'discard' } },
  // No newline at the end of the file.
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\nb\\nc\\nd' > f;`, diffs: [U], sel: { lines: [[0, 0, 3]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\nb\\nc\\nd' > f;`, diffs: [U], sel: { lines: [[0, 0, 5]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\nb\\nc\\nd' > f;`, diffs: [U], sel: { lines: [[0, 0, 3]] }, mode: 'discard' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\nb\\nc\\nd' > f; git add f;`, diffs: [S], sel: { lines: [[0, 0, 5]] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\n' > f;`, diffs: [U], sel: { lines: [[0, 0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\n' > f;`, diffs: [U], sel: { lines: [[0, 0, 2]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\nB\\nc\\n' > f;`, diffs: [U], sel: { lines: [[0, 0, 2]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: `${noNl} printf 'a\\nB\\nc\\n' > f; git add f;`, diffs: [S], sel: { lines: [[0, 0, 3]] }, mode: 'unstage' } },
  // Carriage returns, modes, binary files, special names, a repository without commit.
  { fn: 'apply', c: { setup: repo("printf 'x\\r\\ny\\r\\nz\\r\\n' > c.txt; git add c.txt; git commit -qm crlf; printf 'X\\r\\ny\\r\\nZ\\r\\n' > c.txt;"), diffs: [U], sel: { lines: [[0, 0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo('chmod +x a.txt;'), diffs: [U], sel: { files: [0] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(`chmod +x a.txt; ${edit}`), diffs: [U], sel: { lines: [[0, 0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo(`chmod +x a.txt; ${edit}`), diffs: [U], sel: { files: [0] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo("printf '\\000\\001' > b.bin; git add b.bin; git commit -qm bin; printf '\\000\\002' > b.bin;"), diffs: [U], sel: { files: [0] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: repo("printf 'q\\n' > 't\"q'; git add -A; git commit -qm q; printf 'Q\\n' > 't\"q';"), diffs: [U], sel: { lines: [[0, 0, 1]] }, mode: 'stage' } },
  { fn: 'apply', c: { setup: `${init} printf 'a\\n' > f; git add f;`, diffs: [S], sel: { files: [0] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: `${init} printf 'a\\nb\\n' > f; git add f;`, diffs: [S], sel: { lines: [[0, 0, 1]] }, mode: 'unstage' } },
  { fn: 'apply', c: { setup: repo(`${edit} git add a.txt; sed -i 's/^l5$/L5/' a.txt;`), diffs: [U, S], sel: { files: [0, 1] }, mode: 'unstage' } },
  { fn: 'patches', c: { setup: repo(edit), diffs: [U], sel: { lines: [[0, 1, 3], [0, 1, 5]] }, mode: 'stage' } },
  { fn: 'patches', c: { setup: repo(edit), diffs: [U], sel: { lines: [[0, 1, 3], [0, 1, 5]] }, mode: 'discard' } },
  { fn: 'patches', c: { setup: `${noNl} printf 'a\\nb\\nc\\nd' > f;`, diffs: [U], sel: { lines: [[0, 0, 5]] }, mode: 'discard' } },
  { fn: 'patches', c: { setup: `${noNl} printf 'a\\nb\\nc\\nd' > f;`, diffs: [U], sel: { lines: [[0, 0, 3]] }, mode: 'stage' } },
  { fn: 'patches', c: { setup: `${noNl} printf 'a\\n' > f;`, diffs: [U], sel: { lines: [[0, 0, 2]] }, mode: 'discard' } },
  { fn: 'count', c: { setup: repo(edit), diffs: [U], sel: { lines: [[0, 1, 3], [0, 1, 5]] }, mode: 'discard' } },
  { fn: 'count', c: { setup: repo(`${edit} printf 'n1\\n' > new.txt;`), diffs: [U, untracked('new.txt')], sel: { files: [0, 1] }, mode: 'stage' } },
  { fn: 'count', c: { setup: repo(edit), diffs: [U], sel: { files: [0] }, mode: 'unstage' } },
  { fn: 'count', c: { setup: repo("printf '\\000' > b.bin;"), diffs: [untracked('b.bin')], sel: { files: [0] }, mode: 'stage' } },
].map(({ fn, c }) => ({ fn, args: [{ top, ...c }], env: ENV }));

module.exports = {
  module: 'core/stage',
  impl,
  cases: [
    ...cases,
    ...['plain', 'a"b', 'a\\b', 'tab\there', 'nl\n', 'cr\r', '\x01\x7f', 'é'].map(p => ({ fn: 'quote', args: [p] })),
    { fn: 'refused', args: ['$ROOT/r/a', '$ROOT/r'], files: { r: null } },
    { fn: 'refused', args: ['$ROOT/r/a', '$ROOT/r'], files: { 'r/a': 'x' } },
    { fn: 'refused', args: ['$ROOT/r/d/a', '$ROOT/r'], files: { 'r/d': null } },
    { fn: 'refused', args: ['$ROOT/r', '$ROOT/r'], files: { r: null } },
    { fn: 'refused', args: ['$ROOT/other/a', '$ROOT/r'], files: { r: null, other: null } },
    { fn: 'refused', args: ['$ROOT/r/l/a', '$ROOT/r'], files: { r: null, out: null, 'r/l': { link: '$ROOT/out' } } },
    { fn: 'refused', args: ['$ROOT/r/l/a', '$ROOT/r'], files: { 'r/d': null, 'r/l': { link: '$ROOT/r/d' } } },
    { fn: 'refused', args: ['$ROOT/r/l/a', '$ROOT/r'], files: { r: null, 'r/l': { link: '$ROOT/none' } } },
    { fn: 'refused', args: ['$ROOT/r/l', '$ROOT/r'], files: { 'r/d': null, 'r/l': { link: '$ROOT/r/d' } } },
    { fn: 'refused', args: ['$ROOT/r/missing/a', '$ROOT/r'], files: { r: null } },
    { fn: 'refused', args: ['$ROOT/r/f/a', '$ROOT/r'], files: { 'r/f': 'x' } },
    { fn: 'refused', args: ['$ROOT/r/../r/a', '$ROOT/r/'], files: { 'r/a': 'x' } },
    { fn: 'refused', args: ['$ROOT/r/d', '$ROOT/r', true], files: { 'r/d': null } },
    { fn: 'refused', args: ['$ROOT/r/d', '$ROOT/r', false], files: { 'r/d': null } },
    { fn: 'refused', args: ['$ROOT/r/a', '$ROOT/r', true], files: { 'r/a': 'x' } },
    { fn: 'refused', args: ['$ROOT/r/a', '$ROOT/r', false], files: { 'r/a': 'x' } },
    { fn: 'refused', args: ['$ROOT/r/l', '$ROOT/r', true], files: { 'r/d': null, 'r/l': { link: '$ROOT/r/d' } } },
    ...['a', 'd/', 'd//', 'd/a', 'a\\', './x/../a'].map(n => ({ fn: 'located', args: ['$ROOT/r', n] })),
    { fn: 'pruneDirs', args: ['$ROOT/r/a/b/c/f', '$ROOT/r'], files: { 'r/a/b/c': null, 'r/a/keep': 'k' } },
    { fn: 'pruneDirs', args: ['$ROOT/r/f', '$ROOT/r'], files: { r: null } },
    { fn: 'pruneDirs', args: ['$ROOT/r/a/f', '$ROOT/r'], files: { 'r/a/x': 'x' } },
  ],
};
