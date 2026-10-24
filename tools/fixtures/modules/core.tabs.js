'use strict';
// The preview's tabs. busy is given as the files of the busy tabs; each tab is { file, kept, ...the preview's }.
const tabs = require('../../../src/core/tabs');

const impl = { ...tabs, open: (t, file, keep, busy = []) => tabs.open(t, file, keep, tab => busy.includes(tab.file)) };
const T = (file, kept = true, more = {}) => ({ file, kept, ...more });
const many = n => Array.from({ length: n }, (_, i) => T(`/f${i}`));

module.exports = {
  module: 'core/tabs',
  impl,
  cases: [
    { fn: 'open', args: [{ list: [], current: 0 }, '/a', false] },
    { fn: 'open', args: [{ list: [T('/a'), T('/b', false)], current: 0 }, '/c', false] },
    { fn: 'open', args: [{ list: [T('/a'), T('/b')], current: 0 }, '/c', true] },
    { fn: 'open', args: [{ list: [T('/a', false, { scroll: 3 }), T('/b')], current: 1 }, '/a', true] },
    { fn: 'open', args: [{ list: [T('/a', false, { scroll: 3 }), T('/b')], current: 1 }, '/a', false] },
    { fn: 'open', args: [{ list: [T('/a'), T('/b')], current: 1 }, '/b', true] },
    { fn: 'open', args: [{ list: many(50), current: 0 }, '/new', true] },
    { fn: 'open', args: [{ list: many(50), current: 49 }, '/new', true] },
    { fn: 'open', args: [{ list: many(50), current: 25 }, '/new', true] },
    { fn: 'open', args: [{ list: many(50), current: 0 }, '/new', true, ['/f49', '/f48']] },
    { fn: 'open', args: [{ list: many(50), current: 0 }, '/new', true, many(50).map(t => t.file)] },
    ...[[0, 1], [1, 1], [2, 1], [2, 2], [0, 0], [5, 1], [-1, 1]].map(([i, current]) =>
      ({ fn: 'close', args: [{ list: [T('/a'), T('/b'), T('/c')], current }, i] })),
    { fn: 'close', args: [{ list: [T('/a')], current: 0 }, 0] },
    ...[[0, 1], [0, -1], [2, 1], [1, -1]].map(([current, dir]) => ({ fn: 'step', args: [{ list: [T('/a'), T('/b'), T('/c')], current }, dir] })),
    { fn: 'step', args: [{ list: [], current: 0 }, 1] },
    { fn: 'keep', args: [{ list: [T('/a', false, { v: 1 }), T('/b', false)], current: 0 }] },
    { fn: 'keep', args: [{ list: [T('/a', false), T('/b', false)], current: 0 }, 1] },
    { fn: 'labels', args: [['/x/src/index.js', '/x/test/index.js', '/x/a.js', '/b.js', '/index.js']] },
    { fn: 'labels', args: [[]] },
    ...[[[10, 10, 10], 0, 40, 0], [[10, 10, 10], 2, 15, 0], [[10, 10, 10], 1, 15, 0], [[10, 10, 10, 10, 10], 4, 24, 0],
      [[10, 10, 10, 10, 10], 2, 24, 3], [[10, 10, 10, 10, 10], 0, 24, 3], [[50], 0, 20, 0], [[], 0, 10, 0], [[5, 5], 1, 3, 0],
      [[8, 8, 8, 8], 3, 20, 1]].map(args => ({ fn: 'strip', args })),
  ],
};
