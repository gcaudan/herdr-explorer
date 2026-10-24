'use strict';
// Files marked viewed in a tab.
const env = { HERDR_PLUGIN_STATE_DIR: '$ROOT' };
const V = 'viewed-w1_t1.json';
const sha = text => require('crypto').createHash('sha1').update(text).digest('hex');

module.exports = {
  module: 'core/viewed',
  cases: [
    { fn: 'sign', args: ['$ROOT/a'], files: { a: 'hello' } },
    { fn: 'sign', args: ['$ROOT/missing'] },
    { fn: 'sign', args: ['$ROOT/d'], files: { d: null } },
    { fn: 'read', args: ['w1:t1'], env },
    { fn: 'read', args: ['w1:t1'], env, files: { [V]: '[1]' } },
    { fn: 'read', args: ['w1:t1'], env, files: { [V]: 'null' } },
    { fn: 'read', args: ['w1:t1'], env, files: { [V]: '{"x":"y"}' } },
    { fn: 'read', args: ['w1:t1'], files: { [V]: '{"x":"y"}' } },
    { fn: 'is', args: [{ '$ROOT/a': sha('hello') }, '$ROOT/a'], files: { a: 'hello' } },
    { fn: 'is', args: [{ '$ROOT/a': sha('old') }, '$ROOT/a'], files: { a: 'hello' } },
    { fn: 'is', args: [{ '$ROOT/a': 'gone' }, '$ROOT/a'] },
    { fn: 'is', args: [{ '$ROOT/a': 1 }, '$ROOT/a'], files: { a: 'x' } },
    { fn: 'toggle', args: ['w1:t1', '$ROOT/a'], env, files: { a: 'hello' }, after: { [V]: null } },
    { fn: 'toggle', args: ['w1:t1', '$ROOT/a'], env, files: { a: 'hello', [V]: `{"$ROOT/a":"${sha('hello')}"}` }, after: { [V]: null } },
    { fn: 'toggle', args: ['w1:t1', '$ROOT/a'], env, files: { a: 'hello', b: 'b', [V]: `{"$ROOT/b":"${sha('b')}","$ROOT/c":"${sha('c')}","$ROOT/a":"x"}` },
      after: { [V]: null } },
    { fn: 'toggle', args: ['w1:t1', '$ROOT/gone'], env, after: { [V]: null } },
    { fn: 'toggle', args: ['w1:t1', '$ROOT/a'], files: { a: 'hello' } },
  ],
};
