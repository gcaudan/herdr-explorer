'use strict';
// git's diff read by diff.js: parse, unquote and the paths of --- +++ lines. parse's files are given with every field,
// null when unset, and a "\ No newline" line as the JS keeps it (4 items).
const diff = require('../../../src/diff');

const file = f => ({ from: f.from, to: f.to, status: f.status, notes: f.notes, hunks: f.hunks, add: f.add, del: f.del,
  mode: f.mode ?? null, oldMode: f.oldMode ?? null, newMode: f.newMode ?? null });
const impl = {
  parse: text => { const d = diff.parse(text); return { ...d, files: d.files.map(file) }; },
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

module.exports = {
  module: 'diff',
  impl,
  cases: [
    ...texts.map(t => ({ fn: 'parse', args: [t] })),
    ...quoted.map(q => ({ fn: 'unquote', args: [q] })),
    ...stripped.map(p => ({ fn: 'strip', args: [p] })),
  ],
};
