'use strict';
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const os = require('os');

// The state folder, where a review is recorded (review-<tab>.json), read when exec.js loads.
const STATE = fs.mkdtempSync(path.join(os.tmpdir(), 'full-tab-'));
process.env.HERDR_PLUGIN_STATE_DIR = STATE;

// herdr's answers faked, for a tab explorer (p2) | terminal (p3) | preview (p5): actions.js takes run from exec.js
// when it loads.
const calls = [];
let panes = [];
const pane = (pane_id, label, tab_id = 'w1:t1') => ({ pane_id, label, tab_id, workspace_id: 'w1' });
const exec = require('../src/core/exec');
exec.run = async (cmd, args) => {
  calls.push(args);
  const answer = result => JSON.stringify({ result });
  if (args[0] === 'pane' && args[1] === 'get') {
    const found = panes.find(p => p.pane_id === args[2]);
    return found ? answer({ pane: found }) : null;
  }
  if (args[0] === 'pane' && args[1] === 'list') return answer({ panes });
  if (args[0] === 'pane' && args[1] === 'neighbor') {
    const next = { 'w1:p2': 'w1:p3', 'w1:p3': 'w1:p5' }[args[args.indexOf('--pane') + 1]];
    return answer({ neighbor: { neighbor_pane_id: args[3] === 'right' ? next : undefined } });
  }
  return answer({ plugin_pane: { pane: { pane_id: 'w1:p9' } } });
};
const actions = require('../src/core/actions');

const opened = () => calls.find(a => a[0] === 'plugin' && a[2] === 'open');
const closed = () => calls.filter(a => a[0] === 'pane' && a[1] === 'close').map(a => a[2]);
const option = (args, name) => args.includes(name) ? args[args.indexOf(name) + 1] : undefined;
const reset = list => { calls.length = 0; panes = list; };

// Zoomed in the agent's tab, herdr's zoom (one per tab, following the focus) went to the agent when the focus came
// back to it: the agent alone, the explorer hidden. Source control opens a review in a tab of its own instead.
test('source control opens a review of the tab, in a tab of its own', async () => {
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3'), pane('w1:p5', 'Preview')]);
  assert.strictEqual(await actions.scm(os.tmpdir(), 'w1:p2'), null);
  const args = opened();
  assert.strictEqual(option(args, '--placement'), 'tab');
  assert.strictEqual(option(args, '--workspace'), 'w1');
  assert.ok(args.includes('EXPLORER_REVIEW=w1:t1'));
  assert.ok(args.includes('--focus'));
  assert.deepStrictEqual(closed(), []);
});

test('a review already open is focused, not opened again', async () => {
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3'), pane('w1:p7', 'Source Control', 'w1:t2')]);
  fs.writeFileSync(path.join(STATE, 'review-w1_t1.json'), JSON.stringify({ pane: 'w1:p7' }));
  assert.strictEqual(await actions.scm(os.tmpdir(), 'w1:p2'), null);
  assert.strictEqual(opened(), undefined);
  assert.ok(calls.some(a => a.join(' ') === 'plugin pane focus w1:p7'));
  // Its pane gone (the review closed without its cleanup): a new one opens.
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3')]);
  assert.strictEqual(await actions.scm(os.tmpdir(), 'w1:p2'), null);
  assert.strictEqual(option(opened(), '--placement'), 'tab');
  fs.rmSync(path.join(STATE, 'review-w1_t1.json'));
});

test('changes open on the right of the terminal, in place of the preview', async () => {
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3'), pane('w1:p5', 'Preview')]);
  assert.strictEqual(await actions.diff(path.join(os.tmpdir(), 'a.txt'), os.tmpdir(), 'w1:p2'), null);
  const args = opened();
  assert.strictEqual(option(args, '--placement'), 'split');
  assert.strictEqual(option(args, '--target-pane'), 'w1:p3');
  assert.strictEqual(option(args, '--direction'), 'right');
  assert.deepStrictEqual(closed(), ['w1:p5']);
});

test('the editor opens in a tab of its own, in the pane\'s workspace', async () => {
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3')]);
  assert.strictEqual(await actions.edit(path.join(os.tmpdir(), 'a.txt'), 'w1:p2', 'nano'), null);
  const args = opened();
  assert.strictEqual(option(args, '--placement'), 'tab');
  assert.strictEqual(option(args, '--workspace'), 'w1');
  assert.ok(!args.includes('--target-pane'));
});

test('a preview asked from a review opens beside its source control, at full height', async () => {
  reset([pane('w1:p4', 'Source Control', 'w1:t2')]);
  assert.strictEqual(await actions.preview(path.join(os.tmpdir(), 'a.txt'), os.tmpdir(), 'w1:p4', 0, false, true,
    { view: 'diff', beside: 'w1:p4' }), null);
  const args = opened();
  assert.strictEqual(option(args, '--entrypoint'), 'preview');
  assert.strictEqual(option(args, '--target-pane'), 'w1:p4');
  assert.strictEqual(option(args, '--direction'), 'right');
  assert.deepStrictEqual(closed(), []);
});

test('a commit asked from a review opens the preview on it, beside its source control', async () => {
  reset([pane('w1:p4', 'Source Control', 'w1:t2')]);
  const show = { kind: 'commit', ref: 'a91ff98', paths: [], title: 'a91ff98 fix', cwd: os.tmpdir() };
  assert.strictEqual(await actions.preview('', os.tmpdir(), 'w1:p4', 0, false, true, { show, beside: 'w1:p4' }), null);
  const args = opened();
  assert.strictEqual(option(args, '--target-pane'), 'w1:p4');
  const env = args.find(a => a.startsWith('EXPLORER_SHOW='));
  assert.deepStrictEqual(JSON.parse(env.slice('EXPLORER_SHOW='.length)), show);
  // In a review, q in the preview closes it, whatever it shows.
  assert.ok(args.includes('EXPLORER_REVIEW=1'));
  // A preview already open: the request file carries it.
  reset([pane('w1:p4', 'Source Control', 'w1:t2'), pane('w1:p5', 'Preview', 'w1:t2')]);
  assert.strictEqual(await actions.preview('', os.tmpdir(), 'w1:p4', 0, false, false, { show, beside: 'w1:p4' }), null);
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(actions.requestFile('w1:t2'), 'utf8')).show, show);
});

test('the search opens in a tab of its own, or focuses the one already open for the tab', async () => {
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3')]);
  assert.strictEqual(await actions.search(os.tmpdir(), 'w1:p2', 'content', 'foo'), null);
  const args = opened();
  assert.strictEqual(option(args, '--placement'), 'tab');
  assert.strictEqual(option(args, '--workspace'), 'w1');
  assert.ok(args.includes('EXPLORER_ORIGIN=w1:t1') && args.includes('EXPLORER_PANE=w1:p2') && args.includes('--focus'));
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3'), pane('w1:p8', 'Search', 'w1:t3')]);
  fs.writeFileSync(path.join(STATE, 'search-w1_t1.json'), JSON.stringify({ pane: 'w1:p8' }));
  assert.strictEqual(await actions.search(os.tmpdir(), 'w1:p2'), null);
  assert.strictEqual(opened(), undefined);
  assert.ok(calls.some(a => a.join(' ') === 'plugin pane focus w1:p8'));
  // Asked from the search's own tab (its preview): that search, not another one.
  reset([pane('w1:p8', 'Search', 'w1:t3'), pane('w1:p9', 'Preview', 'w1:t3')]);
  assert.strictEqual(await actions.search(os.tmpdir(), 'w1:p9'), null);
  assert.strictEqual(opened(), undefined);
  fs.rmSync(path.join(STATE, 'search-w1_t1.json'));
});

test('a file opened from the search is written for the tab\'s explorer to select, even when the preview opens', async () => {
  reset([pane('w1:p2', 'Explorer'), pane('w1:p3')]);
  const file = path.join(os.tmpdir(), 'a.txt');
  assert.strictEqual(await actions.preview(file, os.tmpdir(), 'w1:p2', 3, false, true, { reveal: true }), null);
  const req = JSON.parse(fs.readFileSync(actions.requestFile('w1:t1'), 'utf8'));
  assert.strictEqual(req.file, file);
  assert.strictEqual(req.reveal, true);
});
