'use strict';
// Syntax highlighting of a sample of each language, in both themes, and the language of file names.
const ui = require('../../../src/core/ui');
const hl = require('../../../src/render/highlight');

const impl = {
  highlight: (theme, text, lang) => {
    ui.useTheme(theme);
    try { return hl.highlight(text, lang); } finally { ui.useTheme('dark'); }
  },
  language: name => hl.language(name),
  styles: theme => {
    ui.useTheme(theme);
    try { return { ...hl.STYLES }; } finally { ui.useTheme('dark'); }
  },
};

const SAMPLES = {
  js: `import { a, b as c } from './x.js'; // comment 😀
const re = /ab+c[/\\]]\\/x/gi, d = a / b / c;
if (x) return /^\\s*$/.test(s) ? 'it\\'s' : "q\\"q";
const t = \`multi \${line}
  goes on\`;
/* block
   comment */ class Foo extends Bar { #priv = 0x1F_FF; static get y() { return this.#priv + 1.5e-3n; } }
@decorator.call obj.method(arg).prop; Type<T> new Map();
typeof /x/ + 'unterminated`,
  ts: 'interface A { readonly b: string; c?: number | undefined }\nexport type X<T> = keyof T;\nlet v: any = <Foo>bar;',
  json: '{\n  "name": "x", // c\n  "list": [1, 2.5e3, true, null],\n  "nested": { "k" : "v\\"" , "u": "\\u00e9" },\n  /* c */ "end": false\n}',
  python: 'import os\n@dataclass\nclass A(Base):\n    """doc\n    string"""\n    def f(self, x=1):\n        return f"{x}" + r\'\\n\' + b"x"  # comment\nprint(None, True, len([]))\n\'\'\'open',
  shell: '#!/bin/sh\nexport A=1 # comment\nif [ -n "$HOME" ]; then echo "${PATH} $1 $(pwd) it\'s"; fi\nls -la --color=auto a#b\'raw $x\nstill raw\' done\nfunction f() { local x; }',
  powershell: 'function Get-Thing { param([string]$Name) }\n# comment\n<# block\n#> $x = $true -and $null\nif ($a -eq 1) { Write-Host "a`"b $env:PATH" }\n@"\nhere $x\n"@\n@\'\nraw\n\'@',
  rust: '#[derive(Debug)]\npub fn f<\'a>(x: &\'a str) -> Option<String> {\n    let c = b\'x\'; let d = \'\\n\';\n    println!("{} \\"q\\"", x); // c\n    /* multi\n    line */ Some(String::new())\n}\nlet s = "multi\nline";',
  go: 'package main\nimport "fmt"\nfunc main() {\n\tx := `raw\nstring`\n\tfmt.Println(x, nil, iota) // c\n\tvar r rune = \'a\'\n}',
  c: '#include <stdio.h>\n  #  define MAX 10\nint main(void) {\n    size_t n = sizeof(int); /* c */\n    printf("%d\\n", n); return 0;\n}',
  cpp: '#include "x.h"\nnamespace std { template<typename T> class V : public B { virtual ~V() override; }; }\nauto p = std::make_unique<int>(nullptr);',
  csharp: 'using System;\n#region R\npublic record P(int X) { public string S => @"verb\n""atim"; }\nvar s = """raw\ntext""";\nforeach (var x in xs) yield return x;',
  java: '@Override\npublic final class A extends B implements C {\n    String s = """\n        text""";\n    char c = \'x\'; // c\n}',
  kotlin: 'package a\n@Suppress("x")\ndata class P(val x: Int) { fun f() = """raw $x\n""" }\nval n: String? = null',
  swift: 'import UIKit\n@objc class A: B { let x = """\nmulti\n"""; func f() async throws -> Int { guard let y else { return 0 } } }',
  php: '<?php\nnamespace App;\nclass A { public function f($x) { return $this->y . "s $x"; } } # comment\n#[Attr]\n$a = \'multi\nline\'; ?>\n<?= $v ?>',
  ruby: 'require \'x\'\nclass A < B\n  attr_reader :name\n  def valid?(x) = @x && $g && @@c\n  # comment\n  :sym!\nend\n=begin\nblock\n=end\nputs "a#{b}"',
  lua: 'local function f(x) -- comment\n  return [[long\nstring]] .. "s"\nend\n--[[ block\ncomment ]] print(nil)',
  sql: "SELECT a, COUNT(*) FROM t -- c\nWHERE x = 'it''s\nmulti' AND y IN (1, 2)\n/* block */ ORDER BY a DESC LIMIT 10;\nselect `col` from \"T\";",
  yaml: '---\nkey: value # comment\n- item: "x\\"y"\n  other: \'single\'\nanchors: &a [1, 2]\nref: *a\ntag: !!str 3\nmulti: |-\n  text\nyes_no: yes\n...\nlist:\n  - a\n  - b: c',
  ini: '[section]\nkey = "value" ; comment\n# comment\nexport PATH=${HOME}/bin\n[[array.of.tables]]\nmulti = """\nlines\n"""\nflag = true',
  dockerfile: 'FROM node:20 AS build\n# comment\nRUN --mount=type=cache apt-get install -y x \\\n    && echo "$HOME ${X}"\nCOPY --from=build /a /b\nENTRYPOINT ["node"]',
  makefile: 'CC := gcc\nCFLAGS += -O2 # comment\nall: main.o $(OBJS)\n\t$(CC) -o $@ $^ ${LDFLAGS}\nifeq ($(X),1)\nendif\n.PHONY: all',
  diff: 'diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n context\n-old\n+new\n\\ No newline at end of file',
  markdown: '# Title\n\nSome **bold**, __b__, *it*, `code`, ``a`b``, [link](http://x) ![img](a.png) <https://a.b> <b>t</b>\n> quote *x*\n- item\n1. one\n- [x] task\n***\n```js\nconst x = 1; /* open\nstill */\n```\n~~~\nplain\n~~~\n```nope\ncode\n```\n<!-- comment\nspans -->## after\n  ## h2\n<!-- one --> text',
  html: '<!DOCTYPE html>\n<html lang="en">\n<!-- comment\n-->\n<script type="module">\nconst x = "</p>"; /* c\n*/ </SCRIPT><style>\na { color: red; }\n</style>\n<p class="a" data-x=\'y\n z\'>Text &amp; &#123; &#x1F; &nope</p>\n<![CDATA[ raw\n]]><br/> < 3 <?xml?>',
  xml: '<?xml version="1.0"?>\n<root xmlns:x="u"><x:item id=\'1\'>v</x:item></root>',
  css: '@media (max-width: 600px) {\n  .a > #b:hover, a[href] { color: #fff; margin: -1.5em 0 !important; background: url("x.png") }\n}\n/* comment\n */ :root { --var: 10px; }\nbody{font:12px/1 sans-serif;content:\'x\'}\n// line comment\na:not(.b) { width: calc(100% - 2px) }\n.x { y: z',
};

const cases = [];
for (const theme of ['dark', 'light']) {
  cases.push({ fn: 'styles', args: [theme] });
  for (const [lang, text] of Object.entries(SAMPLES)) cases.push({ fn: 'highlight', args: [theme, text, lang] });
}
for (const [text, lang] of [
  ['', 'js'], ['\n', 'js'], ['a\n\nb\n', 'js'], ['x\r\ny\tz\u0001', 'js'], ['const x = 1;', null], ['x', 'nope'],
  ['/* open\nstill', 'c'], [`${'x'.repeat(2001)}\n/* c */`, 'js'], [`/* a\n${'é'.repeat(1500)}\n*/ x`, 'js'],
  ['"a\\', 'json'], ["'a\\\n'", 'shell'], ['x = "unterminated\ny = 1', 'python'], ['<a b="c', 'html'],
  ['<script>\nlet x;', 'html'], ['a { b', 'css'], ['/* x', 'css'], ['"a\nb"', 'css'], ['```\n```', 'markdown'],
  ['````md\n```\n````', 'markdown'], ['```python\n"""\nx\n"""\n```', 'markdown'], ['<style>\n@x', 'markdown'],
]) cases.push({ fn: 'highlight', args: ['dark', text, lang] });
for (const name of ['a.js', 'A.TSX', 'dir/x.json5', 'c:\\x\\y.PY', 'Dockerfile', 'dockerfile.prod', '.env.local', '.ENV',
  'Makefile', 'GNUmakefile', 'CMakeLists.txt', 'x', 'javascript', 'c++', 'c#', 'pwsh', 'x.unknown', '', 'a.', '.gitignore',
  'x.vue', 'x.svg', 'x.md', 'x.patch', 'x.toml', 'x.mk', 'PKGBUILD', 'x.d.ts', 'shellsession']) {
  cases.push({ fn: 'language', args: [name] });
}

module.exports = { module: 'render/highlight', impl, cases };
