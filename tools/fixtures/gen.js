// Writes the module fixtures of the Rust rewrite: each case calls an exported function of a JS module on chosen
// inputs and records what it returned (or the message it threw), in rust/tests/fixtures/modules/<module>.json. The
// Rust tests (rust/tests/modules.rs) replay them. Run from the repository: node tools/fixtures/gen.js
//
// A fixture file: { module, cases: [{ fn, args, out } | { fn, args, throws }] }, with optional per case:
//   files  { "<path>": "<text>" | null (a folder) | { link: "<target>" } }, made under a fresh folder before the call;
//   env    variables set for the call ("$ROOT" replaced);
//   after  { "<path>": "<text>" | null (missing) }, files read back after the call.
// "$ROOT" in args, files (their text and link targets), out and after stands for that folder, so that fixtures do
// not depend on where they ran.
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');

const repo = path.join(__dirname, '..', '..');
// The modules read herdr's variables at load: those of the session the generator runs in must not reach them.
for (const k of Object.keys(process.env)) if (/^(HERDR|EXPLORER)_/.test(k)) delete process.env[k];
const out = path.join(repo, 'rust', 'tests', 'fixtures', 'modules');

const sub = (v, from, to) => JSON.parse(JSON.stringify(v === undefined ? null : v).split(from).join(to));

function setup(files = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'fixture-')));
  for (const [name, content] of Object.entries(files)) {
    const p = path.join(root, name);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    if (content === null) fs.mkdirSync(p, { recursive: true });
    else if (typeof content === 'object') fs.symlinkSync(content.link.split('$ROOT').join(root), p);
    else fs.writeFileSync(p, content.split('$ROOT').join(root));
  }
  return root;
}

// Calls fn for one case; fn gets the args with $ROOT replaced.
async function record(fn, c) {
  const root = setup(c.files);
  const saved = {};
  for (const [k, v] of Object.entries(c.env || {})) { saved[k] = process.env[k]; process.env[k] = v.split('$ROOT').join(root); }
  const res = { ...c };
  try {
    res.out = sub(await fn(...sub(c.args, '$ROOT', root)), root, '$ROOT');
  } catch (e) {
    res.throws = e.message;
  } finally {
    for (const [k, v] of Object.entries(saved)) if (v === undefined) delete process.env[k]; else process.env[k] = v;
  }
  if (c.after) {
    res.after = {};
    for (const name of Object.keys(c.after)) {
      try { res.after[name] = fs.readFileSync(path.join(root, name), 'utf8').split(root).join('$ROOT'); } catch { res.after[name] = null; }
    }
  }
  fs.rmSync(root, { recursive: true, force: true });
  return res;
}

async function write(module, exports, cases) {
  const done = [];
  for (const c of cases) done.push(await record(exports[c.fn], c));
  fs.mkdirSync(out, { recursive: true });
  const file = path.join(out, `${module.replace(/\//g, '.')}.json`);
  fs.writeFileSync(file, `${JSON.stringify({ module, cases: done }, null, 1)}\n`);
  console.log(`${path.relative(repo, file)}: ${done.length} cases`);
}

// The modules, one file each under tools/fixtures/modules/, exporting { module, cases }, and impl for functions
// that are not a module of src/ (Node's own, called as the JS calls them).
(async () => {
  if (process.platform === 'win32') throw new Error('fixtures are generated on Linux');
  const only = process.argv.slice(2);
  for (const f of fs.readdirSync(path.join(__dirname, 'modules')).sort()) {
    const { module, cases, impl } = require(path.join(__dirname, 'modules', f));
    if (only.length && !only.includes(module)) continue;
    await write(module, impl || require(path.join(repo, 'src', module)), cases);
  }
})().catch(e => { console.error(e); process.exit(1); });
