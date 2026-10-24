'use strict';
module.exports = {
  module: 'core/exec',
  cases: [
    ...['code --wait', 'subl -w "a b"', "vim 'x y' z", '  ', '"unclosed arg', 'a"b"c', '', 'é  ü'].map(s => ({ fn: 'words', args: [s] })),
    ...['1.2.3', '1.10', 'x.2', '', '1..2', '01.2a'].map(v => ({ fn: 'version', args: [v] })),
    ...[['1.10.0', '1.9.2'], ['1.9.2', '1.10.0'], ['1.2', '1.2.0'], ['1.2.1', '1.2'], ['2', '1.9.9'], ['0.7.1', '0.7.1'], ['a', '0']]
      .map(args => ({ fn: 'newer', args })),
    { fn: 'addConfig', args: [{}, [['a.b', '1'], ['c.d', 'x=y']]] },
    { fn: 'addConfig', args: [{ GIT_CONFIG_COUNT: '2', X: 'y' }, [['e.f', '']]] },
    { fn: 'addConfig', args: [{ GIT_CONFIG_COUNT: 'zz' }, []] },
    ...['{"result":{"a":1}}', '{"x":1}', 'nope', '', '{"result":null}'].map(s => ({ fn: 'result', args: [s] })),
    { fn: 'staleState', args: ['S', ['notes-w1_t1.json', 'notes-w1_t2.json', 'preview-w1_t2.json', 'settings.json', 'width', 'panes'],
      ['w1_p1.json', 'w1_p7.json'], ['w1:t1'], ['w1:p1']] },
    { fn: 'staleState', args: ['S', ['notes-a.json', 'search.json', 'expanded.json', 'turn-x.y.json', 'viewed-.json'], [], [], []] },
    ...[{}, { MERGE_HEAD: '' }, { 'rebase-apply/applying': '' }, { 'rebase-merge': null }, { 'rebase-apply': null },
      { CHERRY_PICK_HEAD: '' }, { REVERT_HEAD: '' }, { 'sequencer/todo': 'revert abc x\n' }, { 'sequencer/todo': 'pick abc\n' },
      { sequencer: null }, { BISECT_LOG: '' }, { MERGE_HEAD: '', REVERT_HEAD: '' }]
      .map(files => ({ fn: 'gitOperation', args: ['$ROOT'], files })),
    { fn: 'remoteLink', args: ['$ROOT/a/b'], files: { 'a/b': '' } },
    { fn: 'remoteLink', args: ['$ROOT/l/x'], files: { l: { link: '//server/share' } } },
    { fn: 'remoteLink', args: ['$ROOT/l/x'], files: { l: { link: '$ROOT/d' }, d: null } },
    { fn: 'remoteLink', args: ['$ROOT/l'], files: { l: { link: 'm' }, m: { link: '//host/s' } } },
    { fn: 'remoteLink', args: ['$ROOT/l'], files: { l: { link: 'm' }, m: { link: 'l' } } },
    { fn: 'remoteLink', args: ['$ROOT/l/x', '$ROOT/l'], files: { l: { link: '//server/share' } } },
    { fn: 'remoteLink', args: ['$ROOT/missing/x'] },
    { fn: 'realPath', args: ['$ROOT/l/x/y'], files: { l: { link: 'd' }, d: null } },
    { fn: 'realPath', args: ['$ROOT/r/x'], files: { r: { link: '//server/share' } } },
    { fn: 'realPath', args: ['$ROOT/a/../b'], files: { b: '' } },
  ],
};
