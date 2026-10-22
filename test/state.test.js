// The state files that tabs and panes leave behind once closed.
'use strict';
const test = require('node:test');
const assert = require('node:assert');
const path = require('path');
const { staleState } = require('../src/core/exec');

test('the files of closed tabs and panes go, the others stay', () => {
  const names = ['notes-w1_t1.json', 'notes-w1_t2.json', 'preview-w1_t2.json', 'settings.json', 'width', 'panes'];
  const panes = ['w1_p1.json', 'w1_p7.json'];
  assert.deepStrictEqual(staleState('S', names, panes, ['w1:t1'], ['w1:p1']).sort(),
    [path.join('S', 'notes-w1_t2.json'), path.join('S', 'panes', 'w1_p7.json'), path.join('S', 'preview-w1_t2.json')]);
});

test('no live tab: every tab file goes, nothing else', () => {
  assert.deepStrictEqual(staleState('S', ['notes-a.json', 'search.json', 'expanded.json'], [], [], []), [path.join('S', 'notes-a.json')]);
});
