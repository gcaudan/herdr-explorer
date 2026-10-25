'use strict';
// Markdown rendered at several widths, in both themes, with and without highlighting, with images drawn by the
// caller (each line of an image carrying a figure): every block and inline span, and their edge cases.
const ui = require('../../../src/core/ui');
const markdown = require('../../../src/render/markdown');

// An image of 2 rows for a.png, none for none.png, a failure for throw.png.
const image = (url, width) => {
  if (url === 'throw.png') throw new Error('bad');
  if (url !== 'a.png') return null;
  return [0, 1].map(row => Object.assign([[' ', ''], [' '.repeat(Math.min(4, width)), '']], { figure: { url, row } }));
};
const impl = {
  render: (theme, text, width, opts = {}) => {
    ui.useTheme(theme);
    try {
      const res = markdown.render(text, width, { ...(opts.plain ? { highlight: null } : {}), ...(opts.image ? { image } : {}) });
      return {
        lines: res.lines.map(l => ({ segs: [...l], anchor: l.anchor ?? null, figure: l.figure ? [l.figure.url, l.figure.row] : null })),
        anchors: res.anchors,
      };
    } finally { ui.useTheme('dark'); }
  },
};

const DOC = `---
title: Front matter
tags: [a, b]
---
# Main title

Intro with **bold**, *italic*, ***both***, __under bold__, _under it_, ~~strike~~, \`code\`, \`\` a\`b \`\`, a\\*literal\\* and a \\q.
A soft break,
a hard break and a backslash\\
break. Snake_case_word and 2*3*4 stay.

## Links & images

[inline](http://example.com "title") [same](same) [ref link][ref] [Ref] [missing][nope] <https://auto.link/x>
<mail@example.com> https://bare.url/path(1)). ![alt *text*](a.png) ![](none.png) <img src="a.png" alt="tag img">
[![badge](b.svg)](http://x) [mailto:me@x.y](mailto:mailto:me@x.y) [\`code\` link](<http://a b>)

[ref]: http://ref.example "Title"
[REF]: http://shadowed

### HTML and entities

<b>bold</b> <em>em</em> <code>a\nb</code> <kbd>Ctrl</kbd> <a href="http://h">anchor</a> <a href="same">same</a>
<span class=x>span</span> <unknown>kept</unknown> <br> line<br/>break <!-- hidden --> <del>gone</del> <mark>m</mark>
&amp; &lt; &copy; &#169; &#x1F600; &#0; &#xD800; &bogus; &nbsp;| &#9;tab <u>under <i>nested</i></u> <sup>1</sup> <b>unclosed

#### Lists

- one
- two
  continued lazily
- [ ] task
- [x] done

1. first
2. second

   loose paragraph
3) other delimiter

* nested
  - deeper
    + deepest
      1. numbered
10. ten
11. eleven

- item

- loose item

> quote with **bold**
lazy line
> > nested quote

> [!NOTE]
> Alert body
> [!warning] not an alert here

> [!TIP] same line
> [!UNKNOWN] x

| Left | Center | Right |
|:-----|:------:|------:|
| a | **b** | \`c\` |
| long cell text that wraps around | x | 1 |
| escaped \\| pipe | | |

Setext heading
==============

Second setext
---

***
- - -
___

\`\`\`js
const x = 1; // comment
function f() { return "s"; }
\`\`\`

~~~python title
def f(): pass
~~~

\`\`\`{.rust}
fn main() {}
\`\`\`

    indented code
    more

\`\`\`mermaid
graph TD
  A[Start] --> B{Decide}
  B -- yes --> C(Done)
  B -->|no| D((Again))
  D --> A
  classDef x fill:#f00;
  E
\`\`\`

\`\`\`mermaid
flowchart LR
  A --> B --> C
\`\`\`

\`\`\`mermaid
sequenceDiagram
  A->>B: hi
\`\`\`

<!-- a comment
spanning lines -->

## Links & images
# Main title
## 123
## !!!
#   Closed heading ###
#No space
\`\`\`
unterminated fence`;

const SMALL = [
  '', 'plain', '# h', '#', '######## seven', 'a\n\n\nb', 'a\r\nb\rc', 'tab\tin\ttext', '**unclosed', '*a **b* c**',
  '_a_b_ *a*b* a*b*', '**a** **b**', '****', '~~a~~ ~a~ ~~~a~~~', '`a`` ``b`', '```', '```\nx', '````\n```\n````',
  '[a](b', '[a]', '[a][]', '[a]: <http://x>\n[a]', '![x](a.png) text ![y](throw.png)', '<a href="u">u</a>',
  '<https://x.y> <ftp:x> <a@b.c> <x@y>', 'http://a.b/c.,;!? http://a.b/(c)', '\\', 'a\\', '1. a\n1) b', '- \n- x',
  '-x', '2) a\n   b\n3) c', '- a\n\n\n- b', '> ', '>\n> a', '| a |\n|---|', '| a | b |\n|---|', 'a | b\n--|--\n1 | 2',
  '---\nnot closed', '---\na\n...\nafter', '   ```\n   code\n  ```', '- ```\n  code\n  ```\n- next', '* * *',
  '<!--\nx', 'Héading ünïcode\n===', '## A-b_c d', '## a\n## a\n## a-1', '#\tTab heading', '- [X] cap', 'x  \ny',
  'line\\\nnext', '&#x110000; &#1114111; &#55296;', 'emoji 😀 **bold 😀**', '界界界 wide **字**', 'a_😀_b',
  '`` ` ``', '`  `', 'url http://x.y/a_b_ end', '<b>a</b', '<B>up</B> <Code>c</Code>', '<code>x</CODE>', '<img src=a.png>',
  '    a\n\u00a0\u00a0\u00a0\n    b', '    a\n\u3000\u3000\n    b', '    a\n \u{1F600}\n    b',
  '[l](u "t") [l](<u v>) [l]( u )', '[a](b)(c)', '**[a](u)**', '[*a*](u)', '[a\\]b](u)', '[a`]`](u)', '!x ![', '[[a]](u)',
];

const cases = [];
for (const theme of ['dark', 'light']) {
  for (const width of [80, 40, 20, 6, 1]) cases.push({ fn: 'render', args: [theme, DOC, width, { image: true }] });
}
cases.push({ fn: 'render', args: ['dark', DOC, 60, { plain: true }] });
cases.push({ fn: 'render', args: ['dark', DOC, 60, {}] });
for (const width of [0, -3, 2.7]) cases.push({ fn: 'render', args: ['dark', '# t\n- a', width, {}] });
for (const text of SMALL) {
  cases.push({ fn: 'render', args: ['dark', text, 30, { image: true }] });
  cases.push({ fn: 'render', args: ['light', text, 3, {}] });
}

module.exports = { module: 'render/markdown', impl, cases };
