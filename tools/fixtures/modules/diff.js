'use strict';
// git's diff read by diff.js: parse, unquote and the paths of --- +++ lines. parse's files are given with every field,
// null when unset, and a "\ No newline" line as the JS keeps it (4 items).
const diff = require('../../../src/diff');

const file = f => ({ from: f.from, to: f.to, status: f.status, notes: f.notes, hunks: f.hunks, add: f.add, del: f.del,
  mode: f.mode ?? null, oldMode: f.oldMode ?? null, newMode: f.newMode ?? null });
// A diff given as text (parse) or as { staged, text } (changesOf, the sections apart).
const parsed = t => (typeof t === 'string' ? diff.parse(t) : diff.changesOf(t));
// render's rows, with what they select as indexes in the diff (file, [file, hunk], [file, hunk, line]) and redo made
// at column 7; view.viewed, the names of the files viewed.
function rows(t, width, mode, titled, view = {}) {
  const d = parsed(t);
  const fi = f => d.files.indexOf(f);
  const at = h => { for (const [i, f] of d.files.entries()) { const k = f.hunks.indexOf(h); if (k >= 0) return [i, k]; } return null; };
  const lineAt = l => {
    for (const [i, f] of d.files.entries()) for (const [k, h] of f.hunks.entries()) { const j = h.lines.indexOf(l); if (j >= 0) return [i, k, j]; }
    return null;
  };
  const names = view.viewed || [];
  const v = { ...view, viewed: names.length ? f => names.includes(f.to || f.from) : undefined };
  return diff.render(d, width, mode, titled, v).map(r => ({
    segs: r.segs, back: r.back ?? null, hunk: !!r.hunk, file: !!r.file, more: !!r.more, place: r.place || 0,
    ref: r.ref ? { file: r.ref.file, from: r.ref.from, to: r.ref.to, sec: r.ref.sec || '' } : null,
    f: r.f ? fi(r.f) : null, h: r.h ? at(r.h) : null, lines: (r.lines || []).map(lineAt),
    redo: r.redo ? r.redo(7) : null,
  }));
}
const impl = {
  parse: text => { const d = diff.parse(text); return { ...d, files: d.files.map(file) }; },
  render: rows,
  changesOf: res => { const d = diff.changesOf(res); return { ...d, files: d.files.map(f => ({ ...file(f), sec: f.sec })) }; },
  shownLines: (t, ref) => diff.shownLines(parsed(t), ref),
  diskLines: (t, ref) => diff.diskLines(parsed(t), ref),
  widestOf: t => diff.widestOf(parsed(t)),
  defaultMode: diff.defaultMode,
  showArgs: diff.showArgs,
  unquote: diff.unquote,
  strip: p => diff.parse(`diff --git x\n--- ${p}\n`).files[0].from,
};

const commit = `commit 0123456789abcdef
Author: A <a@b.c>
Date:   Mon Jan 1 00:00:00 2024 +0000

    subject

 a.txt | 2 +-
 1 file changed, 1 insertion(+), 1 deletion(-)
`;
const modify = `diff --git a/a.txt b/a.txt
index 1111111..2222222 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@ function f() {
 one
-two
+TWO
 three
@@ -10 +10,2 @@
 ten
+eleven
`;

const texts = [
  '',
  '\n',
  commit + '\n' + modify,
  modify,
  modify.replace(/\n/g, '\r\n'),
  `diff --git a/n b/n
new file mode 100755
index 0000000..1111111
--- /dev/null
+++ b/n
@@ -0,0 +1,2 @@
+a
+b
\\ No newline at end of file
`,
  `diff --git a/d b/d
deleted file mode 100644
index 1111111..0000000
--- a/d
+++ /dev/null
@@ -1 +0,0 @@
-gone
`,
  `diff --git a/old name b/new name
similarity index 90%
rename from old name
rename to new name
index 1..2 100644
--- a/old name
+++ b/new name
@@ -1,2 +1,2 @@
-x
+y
 z
`,
  `diff --git a/a b/c
similarity index 100%
copy from a
copy to c
`,
  `diff --git a/img.png b/img.png
index 1..2 100644
Binary files a/img.png and b/img.png differ
`,
  `diff --git a/s.sh b/s.sh
old mode 100644
new mode 100755
`,
  `diff --git a/bin b/bin
old mode 100644
new mode 100755
index 1..2
Binary files a/bin and b/bin differ
`,
  `diff --git a/x b/x
new mode 100755
`,
  `diff --git a/x b/x
Binary files a/x and b/x differ
new mode 100755
`,
  `diff --git "a/t\\tb" "b/t\\tb"
index 1..2 100644
--- "a/t\\tb"
+++ "b/t\\tb"
@@ -1 +1 @@
-a
+b
`,
  `diff --git "a/\\303\\251t\\303\\251" "b/\\303\\251t\\303\\251"
--- "a/\\303\\251t\\303\\251"\t
+++ "b/\\303\\251t\\303\\251"\t
`,
  `diff --git a/é b/é
--- a/é
+++ b/é
`,
  `diff --git a/😀 b/😀
`,
  `diff --git a/a b/b
`,
  `diff --git a/a b/a b/a
`,
  `diff --cc conflict.txt
index 1,2..3
--- a/conflict.txt
+++ b/conflict.txt
@@@ -1,1 -1,1 +1,5 @@@
++<<<<<<< ours
`,
  `diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,2 +1,2 @@ ctx more
-a
+b
@@ -5,2 +5,1 @@\tctx
 c
-d
\\ No newline at end of file
+d
\\  No newline at end of file  
-extra after the hunk
trailing pre line
`,
  `diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,1 +1,1 @@
-a
+b
diff --git a/g b/g
--- a/g
+++ b/g
@@ -3,0 +4,1 @@
+new
@@ -9007199254740991,1 +1 @@
 x
@@ -1,x +1 @@
@@ -1 +1,2 @@xyz
+q
+r
`,
  'just text\nbefore any file\n\n   \nindex 1..2\n',
];

const quoted = ['plain', '"a"', '"', '""', '"\\""', '"a\\\\b"', '"\\a\\b\\t\\n\\v\\f\\r\\q\\e"', '"\\303\\251"', '"\\303"',
  '"\\777x"', '"\\19"', '"\\7"', '"😀"', '"é"', '"a\\"', '"a\\1"', 'x"', '"x'];
const stripped = ['a/x', 'b/x', 'c/x', '/dev/null', '"/dev/null"', 'a/x\t', 'a/x\t\t', '"a/\\303\\251"', 'a/a/x', '"b/q"\t'];

// Lines that pair word by word, or not; tabs, controls, wide characters; a line too long for its row.
const wide = `diff --git a/w.js b/w.js
--- a/w.js
+++ b/w.js
@@ -1,9 +1,10 @@ section
 const a = 1;
-let value = compute(alpha, beta);
-removed alone
+let value = compute(alpha, gamma);
+totally different text here
+an extra added line
 \tindented\twith tabs\u0007bell
-漢字の行です、とても長い
+漢字の行でした、とても長い
-${'long '.repeat(40)}end
+${'long '.repeat(40)}END
 x
-
+ 
`;
const staged = `diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,3 +1,3 @@
 a
-b
+B
 c
`;
const unstaged = `diff --git a/f b/f
--- a/f
+++ b/f
@@ -0,0 +1 @@
+zero
@@ -2 +3 @@
-B
+deux
diff --git a/e b/e
new file mode 100644
--- /dev/null
+++ b/e
@@ -0,0 +1 @@
+e
`;
const sections = [{ staged, text: unstaged }, { staged: '', text: '' }, { staged: '\n', text: '\n' }, { text: modify }, { staged: modify, text: '' }];
const renders = [];
for (const t of [commit + '\n' + modify, wide, texts[5], texts[6], texts[7], texts[8], texts[9], texts[10], texts[22], { staged, text: unstaged }]) {
  for (const width of [30, 80, 140]) for (const mode of ['inline', 'split']) renders.push([t, width, mode, false, {}]);
}
for (const mode of ['inline', 'split']) {
  renders.push([wide, 60, mode, true, { wrap: true }], [wide, 60, mode, true, { left: 12 }], [wide, 3, mode, false, { wrap: true }]);
  renders.push([modify, 80, mode, true, {}], [{ staged, text: unstaged }, 80, mode, true, {}], [modify, 80, mode, false, { viewed: ['a.txt'] }]);
  renders.push([modify, 80, mode, true, { viewed: ['a.txt'] }], [wide, 1, mode, false, {}], [wide, 0, mode, false, { left: 400 }]);
}
const refs = [
  [{ staged, text: unstaged }, { file: 'f', sec: 'staged', from: 1, to: 1 }],
  [{ staged, text: unstaged }, { file: 'f', sec: 'staged', from: 2, to: 3 }],
  [{ staged, text: unstaged }, { file: 'f', sec: 'staged', from: 3, to: 4 }],
  [{ staged, text: unstaged }, { file: 'f', sec: 'unstaged', from: 1, to: 1 }],
  [{ staged, text: unstaged }, { file: 'g', sec: 'staged', from: 1, to: 1 }],
  [{ staged, text: '' }, { file: 'f', sec: 'staged', from: 2, to: 2 }],
  [modify, { file: 'a.txt', sec: '', from: 10, to: 11 }],
  [modify, { file: 'a.txt', sec: 'staged', from: 2, to: 2 }],
];
const shows = [
  { kind: 'commit', ref: 'abc' }, { kind: 'commit', ref: 'abc', paths: ['a', 'b'] }, { kind: 'commit', ref: 'abc', paths: [] },
  { kind: 'stash', ref: 'stash@{0}' }, { kind: 'tag', ref: 'v1' }, { kind: 'commit', ref: '-x' }, { kind: 'commit', ref: '' },
  { kind: 'commit', ref: 'a', paths: [''] }, { kind: 'commit', ref: 'a', paths: 'a' }, { kind: 'branch', ref: 'a' }, {}, { kind: 'commit', ref: 'a', paths: null },
  { kind: 'commit', ref: 3 },
];

module.exports = {
  module: 'diff',
  impl,
  cases: [
    ...texts.map(t => ({ fn: 'parse', args: [t] })),
    ...quoted.map(q => ({ fn: 'unquote', args: [q] })),
    ...stripped.map(p => ({ fn: 'strip', args: [p] })),
    ...renders.map(args => ({ fn: 'render', args })),
    ...sections.map(res => ({ fn: 'changesOf', args: [res] })),
    ...refs.map(([t, ref]) => ({ fn: 'shownLines', args: [t, ref] })),
    ...refs.map(([t, ref]) => ({ fn: 'diskLines', args: [t, ref] })),
    ...texts.map(t => ({ fn: 'widestOf', args: [t] })),
    { fn: 'widestOf', args: [wide] },
    ...[0, 80, 119, 120, 300].map(w => ({ fn: 'defaultMode', args: [w] })),
    ...shows.map(what => ({ fn: 'showArgs', args: [what] })),
  ],
};
