// File icons in each icon set: every one and its space take the same columns, two (three for emoji, two columns
// wide), so that the names line up.
const test = require('node:test');
const assert = require('node:assert');
const ui = require('../src/core/ui');
const icons = require('../src/core/icons');

const NAMES = ['a.js', 'b.ts', 'c.json', 'd.md', 'e.py', 'f.html', 'g.css', 'h.sh', 'i.ps1', 'j.png', 'k.pdf', 'l.zip',
  'm.csv', 'n.docx', 'o.lock', 'p.toml', 'q.sql', 'r.rs', 's.cs', 't.java', 'u.vue', 'v.txt', 'w.go', '.gitignore',
  'package.json', 'Dockerfile', 'README.md', 'noext', 'x.unknown'];

test('icons take the same width within a set', () => {
  try {
    for (const set of ['nerd', 'emoji', 'unicode', 'ascii']) {
      ui.useGlyphs(set);
      for (const [name, dir, open] of [...NAMES.map(n => [n, false, false]), ['src', true, false], ['src', true, true]]) {
        const [text, rgb] = icons.icon(null, name, dir, open);
        assert.strictEqual(ui.textWidth(text), set === 'emoji' ? 3 : 2, `${set} ${name}: ${JSON.stringify(text)}`);
        assert.ok(Array.isArray(rgb), `${set} ${name}: colour`);
      }
      for (const g of ['closed', 'open', 'gear', 'search', 'branch']) assert.strictEqual(ui.textWidth(ui.G[g]), 1, `${set} ${g}`);
    }
  } finally { ui.useGlyphs('nerd'); }
});

test('an unknown icon set falls back to Nerd Font', () => {
  ui.useGlyphs('nope');
  assert.strictEqual(ui.G.set, 'nerd');
  assert.strictEqual(ui.G.gear, String.fromCodePoint(0xf013));
});
