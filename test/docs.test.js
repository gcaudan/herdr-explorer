// Relative links of the Markdown docs, files and #anchors, point at something that exists.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const docs = ['README.md', 'SECURITY.md', 'CONTRIBUTING.md',
  ...fs.readdirSync(path.join(root, 'docs')).filter(f => f.endsWith('.md')).map(f => `docs/${f}`)];
// GitHub's heading anchors: lower case, punctuation dropped, spaces as dashes.
const slug = h => h.replace(/^#+ /, '').toLowerCase().replace(/[^\w\- ]/g, '').replace(/ /g, '-');
const anchors = file => new Set(fs.readFileSync(file, 'utf8').split('\n').filter(l => /^#+ /.test(l)).map(slug));

for (const doc of docs) {
  test(doc, () => {
    const file = path.join(root, doc);
    for (const [, url] of fs.readFileSync(file, 'utf8').matchAll(/\]\(([^)\s]+)\)/g)) {
      if (/^[a-z]+:/.test(url)) continue;
      const [p, anchor] = url.split('#');
      const target = p ? path.join(path.dirname(file), p) : file;
      assert.ok(fs.existsSync(target), `${doc}: ${url} (missing file)`);
      if (anchor) assert.ok(anchors(target).has(anchor), `${doc}: ${url} (missing anchor)`);
    }
  });
}
