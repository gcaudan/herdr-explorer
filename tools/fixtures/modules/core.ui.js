'use strict';
// The drawing helpers of the panes: widths, lines, segments, key hints, messages, the input decoder and the field,
// the key help, the cards, the scrollbar and the menu. The theme is the dark one (no settings); light(fn, ...args)
// calls fn with the light one. Objects with methods or state are driven by a list of steps, each step's outcome
// recorded: decode gives [kind, value] events; the scrollbar's scrollTo sets the bar again, as a render does.
const ui = require('../../../src/core/ui');

const zones = segs => segs.zones;
const events = data => {
  const out = [];
  ui.decode(data, {
    key: k => out.push(['key', k]),
    mouse: m => out.push(['mouse', m]),
    focus: f => out.push(['focus', f]),
    paste: (text, raw) => out.push(['paste', [text, raw]]),
    reply: r => out.push(['reply', r]),
  });
  return out;
};
const impl = {
  shade: ui.shade,
  glyph: ui.glyph,
  backgroundOf: ui.backgroundOf,
  clean: ui.clean,
  cleanLine: (text, tab, withAt) => { const at = withAt ? [] : null; const s = ui.cleanLine(text, tab, at); return withAt ? [s, at] : s; },
  cols: (ch, prev) => ui.cols(ch, prev === null ? undefined : prev),
  fit: (text, max, prev) => ui.fit(text, max === null ? Infinity : max, prev === null ? undefined : prev),
  textWidth: ui.textWidth,
  ellipsis: ui.ellipsis,
  line: (segs, width, back) => ui.line(segs, width, back || undefined),
  keys: (pairs, max) => { const segs = ui.keys(pairs, max === null ? Infinity : max); return { segs: [...segs], zones: zones(segs) }; },
  keyAt: (pairs, x, from) => ui.keyAt(ui.keys(pairs), x, from),
  keyLabel: ui.keyLabel,
  empty: ui.empty,
  messageKind: ui.messageKind,
  messageTime: ui.messageTime,
  message: (text, kind) => ui.message(text, kind || undefined),
  wrap: ui.wrap,
  segText: ui.segText,
  segWidth: ui.segWidth,
  slice: ui.slice,
  split: ui.split,
  mark: ui.mark,
  onOff: ui.onOff,
  switchStyle: ui.switchStyle,
  segmented: ui.segmented,
  head: ui.head,
  tail: ui.tail,
  keyName: ui.keyName,
  printable: ui.printable,
  decode: events,
  // A field from value, each key's [changed, value, cursor], then its segments in width columns.
  field: (value, keys, width, style) => {
    const f = new ui.Field(value);
    const steps = keys.map(k => (k.startsWith('insert:') ? (f.insert(k.slice(7)), [true, f.value, f.cursor]) : [f.key(k), f.value, f.cursor]));
    return { steps, segments: f.segments(width, style) };
  },
  // The help drawn on H lines W wide, then after each step ['key', k] or ['mouse', m] what it returned and the help
  // drawn again (while open).
  help: (W, H, title, groups, steps) => {
    const state = { top: 0, find: null };
    const draw = () => { const lines = new Array(H).fill(''); ui.help(lines, W, title, groups, state); return lines; };
    const out = [draw()];
    for (const [kind, v] of steps) {
      const open = kind === 'key' ? ui.helpKey(v, state) : ui.helpMouse(v, state);
      out.push([open, state.top, state.find ? state.find.value : null, open ? draw() : null]);
    }
    return out;
  },
  confirm: (W, H, q, keys, clicks) => {
    const lines = Array.from({ length: H }, () => ' '.repeat(W));
    ui.confirm(lines, W, H, q);
    const after = clicks.map(m => ui.confirmMouse(m, q));
    return { lines, zones: q.zones, clicks: after, keys: keys.map(k => [ui.confirmKey(k, q), q.focus]) };
  },
  ask: (W, H, q, keys, clicks) => {
    const lines = Array.from({ length: H }, () => ' '.repeat(W));
    ui.ask(lines, W, H, q);
    const after = clicks.map(m => ui.askMouse(m, q));
    return { lines, zones: q.zones && q.zones.buttons, box: q.box, clicks: after, keys: keys.map(k => [ui.askKey(k, q), q.focus]) };
  },
  // A bar set from total, height, top and marks: its cells and topAt of each row, then each [m, k, on] mouse event:
  // [taken, the top scrolled to or null, the cells after].
  scrollbar: (total, height, top, marks, steps) => {
    let scrolled = null;
    const bar = new ui.Scrollbar(t => { scrolled = t; bar.set(total, height, t, marks); });
    const cells = () => Array.from({ length: height }, (_, k) => bar.cell(k));
    const out = { cols: bar.set(total, height, top, marks), cells: cells(), topAt: bar.bar ? Array.from({ length: height }, (_, r) => bar.topAt(r)) : null, steps: [] };
    for (const [m, k, on] of steps) {
      scrolled = null;
      out.steps.push([bar.mouse(m, k, on), scrolled, cells()]);
    }
    bar.stop();
    return out;
  },
  // A menu drawn over lines, then each step ['key', k] or ['mouse', m]: [what it returned (an item's run as its
  // name), sel, off] and the menu drawn again.
  menu: (opts, lines, W, H, steps) => {
    const items = opts.items.map(it => (it ? { ...it, run: it.run } : null));
    const m = new ui.Menu({ ...opts, items });
    const draw = () => { const l = [...lines]; m.draw(l, W, H); return l; };
    const res = r => (r && typeof r === 'object' ? { run: r.run === undefined ? null : r.run, keep: !!r.keep } : r);
    const out = [draw(), m.x, m.inner];
    for (const [kind, v] of steps) {
      const r = kind === 'key' ? m.key(v, H) : kind === 'move' ? (m.move(v, H), null) : m.mouse(v, H);
      out.push([res(r), m.sel, m.off, draw()]);
    }
    return out;
  },
  light: (fn, ...args) => { ui.useTheme('light'); try { return impl[fn](...args); } finally { ui.useTheme('dark'); } },
};

const RED = [200, 0, 0], GREEN = [0, 200, 0];
const mouse = (x, y, more = {}) => ({ button: 0, mods: 0, x, y, press: true, drag: false, wheel: 0, ...more });
const segs = [['abc', 'S1'], ['中文', 'S2'], ['⚠', ''], ['️x', 'S3']];
const groups = [['Move', [['up down', 'move'], ['enter', 'open'], ['pageup', 'page up']]], ['Files', [['ctrl+n', 'new file'], ['del', 'delete'], ['a very long key name', 'cut']]]];
const confirmQ = (more = {}) => ({ title: 'Move to the trash?', name: 'a name long enough to wrap on a few lines.txt', where: 'in src/', note: 'This cannot be undone.', action: 'Move', focus: 'no', ...more });
const askQ = (more = {}) => ({ title: 'Update herdr-explorer', name: '0.7.1 → 0.9.0', list: ['a change', 'another change'], note: 'and 2 more',
  buttons: [{ key: 'y', label: 'Update', tone: 'go' }, { key: 'r', label: 'Notes' }, { key: 's', label: 'Skip version', tone: 'danger' }, { key: 'n', label: 'Later' }], focus: 0, ...more });
const row = ui.line([['abcdefghijklmnopqrstuvwxyz', ui.fg([1, 2, 3])], ['界xyz', '\x1b[1m']], 30);
const menuItems = [{ label: 'Wrap', value: ['● on', 'V'], keep: true, run: 'wrap' }, null, { label: 'Push', key: 'P', run: 'push' },
  { label: 'Delete everything at once', key: 'del', danger: true, run: 'delete' }, { label: 'Last', run: 'last' }];

module.exports = {
  module: 'core/ui',
  impl,
  cases: [
    ...[[0, 0, 0], [255, 255, 255], [10, 20, 30], [200, 100, 50]].map(c => ({ fn: 'shade', args: [c] })),
    { fn: 'light', args: ['shade', [10, 20, 30]] },
    ...[0xe5fe, 0x1f600, 0x41].map(c => ({ fn: 'glyph', args: [c] })),
    ...['\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\', '\x1b]11;rgb:ffff/ffff/ffff\x07', '\x1b]11;rgb:fd/f6/e3\x07', '\x1b]11;rgb:8080/8080/8080\x07',
      '\x1b]11;rgb:7f7f/7f7f/7f7f\x07', '\x1b]10;rgb:ffff/ffff/ffff\x07', '\x1b]11;rgba:ffff/ffff/ffff/ffff\x07', '\x1b]11;#ffffff\x07', ''].map(r => ({ fn: 'backgroundOf', args: [r] })),
    ...['plain', 'a\tb\x01c\x7fd\x9be', 'é\n'].map(t => ({ fn: 'clean', args: [t] })),
    ...[['plain', 4, false], ['a\tb', 4, false], ['\tab\tc', 4, true], ['中\tx', 4, true], ['a\x01b\x9bc', 8, true], ['x\ty', 2, true], ['😀\tb', 4, true], ['', 4, true]]
      .map(a => ({ fn: 'cleanLine', args: a })),
    ...[['a', null], ['中', null], ['́', 'e'], ['️', '⚠'], ['️', '中'], ['️', 'a'], ['‍', null], ['🇫', null], ['☰', null], ['😀', null]].map(a => ({ fn: 'cols', args: a })),
    ...[['abc中def', 4, null], ['abc中def', 5, null], ['⚠️x', 1, null], ['️x', 3, '⚠'], ['', 3, null], ['abc', null, null], ['abc', 0, null]].map(a => ({ fn: 'fit', args: a })),
    ...['', 'abc', '中文', 'é', '👨‍👩‍👧', '🇫🇷🇩🇪', '1️⃣', '\t'].map(t => ({ fn: 'textWidth', args: [t] })),
    ...[['abcdef', 6], ['abcdef', 5], ['abcdef', 1], ['abcdef', 0], ['abcdef', -3], ['中文字', 4], ['中文字', 3]].map(a => ({ fn: 'ellipsis', args: a })),
    ...[[segs, 10, null], [segs, 4, null], [segs, 0, null], [[['a\tb', 'X'], ['', 'Y'], ['c', '']], 6, [1, 2, 3]], [[], 3, [9, 9, 9]], [[['中', '']], 1, null]]
      .map(a => ({ fn: 'line', args: a })),
    { fn: 'light', args: ['line', segs, 12, [5, 5, 5]] },
    ...[[[['s', 'stage'], ['d', 'discard'], ['q', 'quit']], null], [[['s', 'stage'], ['d', 'discard'], ['q', 'quit']], 20], [[['⏎', 'open'], ['↑↓', 'move'], ['1-4', 'tabs'], ['esc', 'close'], ['pgdn', 'page']], null], [[], 5]]
      .map(a => ({ fn: 'keys', args: a })),
    ...[1, 2, 5, 11, 21, 22, 40].map(x => ({ fn: 'keyAt', args: [[['⏎', 'open'], ['↑↓', 'move'], ['space s', 'stage'], ['del', 'delete']], x, x === 22 ? 1 : 0] })),
    ...['up', 'ctrl+up', 'enter', 'escape', 'pagedown', 'x', 'upx', 'shift+left'].map(k => ({ fn: 'keyLabel', args: [k] })),
    { fn: 'empty', args: [20, 6, 'Nothing here', [['n', 'new'], ['q', 'quit']]] },
    { fn: 'empty', args: [8, 2, 'Nothing here at all', []] },
    ...['Saved', 'Cannot write x', 'Working…', 'No change', 'Nothing to do', 'git: not found', 'x: already bound', 'Staged 1 line; cannot stage u', 'Not now'].map(t => ({ fn: 'messageKind', args: [t] })),
    ...['error', 'ok'].map(k => ({ fn: 'messageTime', args: [k] })),
    ...[['Saved', null], ['Cannot write', null], ['Busy…', null], ['No change', null], ['x', 'error']].map(a => ({ fn: 'message', args: a })),
    ...[['a few words to wrap here', 8], ['averyveryverylongword and more', 6], ['two\nparagraphs  here', 5], ['', 4], ['中文中文中文', 3], ['a  b', 0]].map(a => ({ fn: 'wrap', args: a })),
    { fn: 'segText', args: [segs] },
    { fn: 'segWidth', args: [segs] },
    ...[[segs, 0, 3], [segs, 2, 4], [segs, 4, 2], [segs, 5, 10], [[['a⚠️bc', '']], 2, 3], [segs, 20, 3]].map(a => ({ fn: 'slice', args: a })),
    ...[[segs, 3], [segs, 1], [[['ab⚠️cd', '']], 3], [[['', 'x']], 3], [[['abcdef', 'A'], ['gh', 'B']], 2]].map(a => ({ fn: 'split', args: a })),
    ...[[segs, [[1, 4]], 'M'], [segs, [], 'M'], [segs, [[0, 2], [3, 5]], 'M'], [[['hello world', 'B']], [[6, 11], [0, 1]], 'M']].map(a => ({ fn: 'mark', args: a })),
    ...[true, false].map(on => ({ fn: 'onOff', args: [on] })),
    ...[true, false].map(on => ({ fn: 'switchStyle', args: [on] })),
    { fn: 'segmented', args: [[['file', 'File'], ['diff', 'Diff'], ['md', '中文']], 'diff'] },
    ...[0, 3, 7, 12, 30, 40].map(n => ({ fn: 'head', args: [row, n] })),
    ...[0, 3, 26, 27, 29, 40].map(n => ({ fn: 'tail', args: [row, n] })),
    ...['\x1b[A', '\x1b[1;5C', '\r', '\x01', '\x1bx', '\x1b[99~', 'a', 'É', ' ', '\x1b', '\x1b[122;6u'].map(k => ({ fn: 'keyName', args: [k] })),
    ...['a', 'É', 'space', 'up', 'ctrl+a', '中', ''].map(k => ({ fn: 'printable', args: [k] })),
    ...['a\x1b_Gi=31;OK\x1b\\b\x1b[6;16;8tc', '\x1b[<0;5;7M\x1b[<64;1;1M\x1b[<65;2;2M\x1b[<32;3;3M\x1b[<0;4;4m\x1b[<16;5;5M', '\x1b[I\x1b[O',
      '\x1b[200~line1\r\nline2\x01\x1b[201~', '\x1b]11;rgb:0/0/0\x07x', '\x1b[1;5A\x1bOP\x1b\x7f\x1bx\x1b', '\x1b_', '\x1b]', '😀é', '\x1b[1;2H\x1b[27;6;90~'].map(d => ({ fn: 'decode', args: [d] })),
    { fn: 'field', args: ['hello world', ['left', 'left', 'x', 'home', 'delete', 'end', 'backspace', 'ctrl+w', 'space', 'é', 'ctrl+u', 'f1', 'insert:a\nb'], 6, 'S'] },
    { fn: 'field', args: ['中文中文中文', ['home', 'right', 'right', 'ctrl+k', 'alt+backspace', 'backspace'], 4, ''] },
    { fn: 'field', args: ['abcdefghij', ['ctrl+b', 'ctrl+f', 'ctrl+d'], 4, ''] },
    { fn: 'help', args: [40, 12, 'Explorer', groups, [['key', 'down'], ['key', 'end'], ['key', 'home'], ['key', '/'], ['key', 'd'], ['key', 'e'], ['key', 'backspace'], ['key', 'escape'], ['key', 'escape']]] },
    { fn: 'help', args: [24, 6, '', groups, [['mouse', mouse(0, 0, { wheel: 1, press: false })], ['key', 'pagedown'], ['key', 'ctrl+u'], ['key', '/'], ['key', 'z'], ['mouse', mouse(2, 6)], ['mouse', mouse(4, 1)]]] },
    { fn: 'help', args: [30, 8, 'T', groups, [['mouse', mouse(14, 8)], ['key', 'q']]] },
    ...[[80, 30], [26, 12], [12, 8], [10, 5], [30, 4], [30, 3], [2, 10], [1, 10]].map(([W, H]) => ({ fn: 'confirm', args: [W, H, confirmQ(), ['left', 'enter', 'x'], [mouse(1, 1)]] })),
    { fn: 'confirm', args: [40, 12, confirmQ({ tone: 'go', focus: 'yes', where: undefined, note: undefined }), ['tab', 'y'], []] },
    ...[[80, 30], [34, 24], [20, 12], [30, 5], [6, 4]].map(([W, H]) => ({ fn: 'ask', args: [W, H, askQ({ focus: 3 }), ['left', 'right', 'enter', 's', 'escape', 'x'], [mouse(1, 1), mouse(Math.floor(W / 2), Math.floor(H / 2))]] })),
    { fn: 'ask', args: [40, 14, askQ({ buttons: [], list: [], note: undefined }), ['x', 'escape'], []] },
    { fn: 'scrollbar', args: [100, 10, 0, [{ row: 50, color: GREEN, weight: 1 }, { row: 51, color: RED, weight: 3 }, { row: 99, color: GREEN, weight: 1 }],
      [[mouse(1, 1), 9, true], [mouse(1, 1, { press: false }), 9, true], [mouse(1, 1), 5, true], [mouse(1, 1, { drag: true }), 8, true], [mouse(1, 1, { drag: true }), 2, false],
        [mouse(1, 1, { press: false }), 2, false], [mouse(1, 1), 0, true], [mouse(1, 1, { drag: true }), 3, true], [mouse(1, 1, { press: false }), 3, true],
        [mouse(1, 1), 4, false], [mouse(1, 1, { button: 2 }), 4, true], [mouse(1, 1, { wheel: 1 }), 4, true], [mouse(1, 1), 12, true]]] },
    { fn: 'scrollbar', args: [100, 10, 45, [{ row: 50, color: RED, weight: 3 }], [[mouse(1, 1), 5, true], [mouse(1, 1, { drag: true }), 9, true]]] },
    { fn: 'scrollbar', args: [5, 3, 1, [{ row: -1, color: RED, weight: 1 }, { row: 7, color: RED, weight: 1 }], [[mouse(1, 1), 0, true], [mouse(1, 1), 2, true]]] },
    { fn: 'scrollbar', args: [10, 10, 0, [], [[mouse(1, 1), 0, true]]] },
    { fn: 'scrollbar', args: [1000, 4, 999, [], [[mouse(1, 1), 3, true], [mouse(1, 1), 0, true]]] },
    { fn: 'menu', args: [{ title: 'Settings', items: menuItems, at: 1, right: 30 }, [row, row, row, row, row, row, row, row], 30, 8,
      [['key', 'down'], ['key', 'down'], ['key', 'up'], ['key', 'right'], ['key', 'enter'], ['key', 'P'], ['key', 'z'], ['key', 'q'], ['mouse', mouse(0, 0, { wheel: 1, press: false })],
        ['mouse', mouse(20, 3)], ['mouse', mouse(20, 5)], ['mouse', mouse(2, 3)], ['mouse', mouse(20, 3, { drag: true })], ['move', -1]]] },
    { fn: 'menu', args: [{ title: 'A title longer than the pane', items: menuItems, at: 0, left: 3 }, ['', 'xyz'], 16, 5, [['key', 'down'], ['key', 'down'], ['key', 'down']]] },
    { fn: 'menu', args: [{ title: 'M', items: [null, { label: 'Only', run: 'only' }, null], at: 0, left: 1 }, [], 20, 10, [['key', 'down'], ['key', 'h'], ['key', 'space']]] },
    { fn: 'light', args: ['menu', { title: 'M', items: menuItems, at: 0, right: 20 }, [], 20, 10, []] },
  ],
};
