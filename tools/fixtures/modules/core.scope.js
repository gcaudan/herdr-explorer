'use strict';
const env = { HERDR_PLUGIN_STATE_DIR: '$ROOT' };
const read = (text, tab = 'w1:t1') => ({ fn: 'read', args: [tab], env, ...(text !== undefined ? { files: { 'scope-w1_t1.json': text } } : {}) });
const write = (scope, files) => ({ fn: 'write', args: ['w1:t1', scope], env, ...(files ? { files } : {}), after: { 'scope-w1_t1.json': null } });

module.exports = {
  module: 'core/scope',
  cases: [
    { fn: 'label', args: [{ kind: 'head' }] },
    { fn: 'label', args: [{ kind: 'branch' }] },
    { fn: 'label', args: [{ kind: 'branch' }, 'develop'] },
    { fn: 'label', args: [{ kind: 'turn' }] },
    { fn: 'label', args: [{ kind: 'rev', rev: 'refs/heads/a..refs/remotes/origin/b' }] },
    { fn: 'label', args: [{ kind: 'ref', rev: 'feature' }] },
    { fn: 'label', args: [{ kind: 'other' }] },
    { fn: 'same', args: [{ kind: 'rev', rev: 'a' }, { kind: 'rev', rev: 'a' }] },
    { fn: 'same', args: [{ kind: 'rev', rev: 'a' }, { kind: 'rev', rev: 'b' }] },
    { fn: 'same', args: [{ kind: 'head' }, { kind: 'head', rev: '' }] },
    read(),
    read('{"kind":"branch"}'),
    read('{"kind":"rev","rev":"a..b"}'),
    read('{"kind":"rev","rev":""}'),
    read('{"kind":"ref"}'),
    read('{"kind":"nope"}'),
    read('not json'),
    read('{"kind":"turn","extra":1}'),
    { fn: 'read', args: [''], env },
    write({ kind: 'head' }, { 'scope-w1_t1.json': '{"kind":"turn"}' }),
    write({ kind: 'branch' }),
    write({ kind: 'rev', rev: 'a...b' }),
    write({ kind: 'turn', rev: '' }),
    { fn: 'write', args: ['w1:t1', { kind: 'branch' }] },
  ],
};
