// The explorer's settings view, in place of the tree, and the pages opened from it: the icon themes and the key
// bindings. Each page has its view(W, H), key(k) and mouse(m). ctx: see explorer.js.
'use strict';
const actions = require('../core/actions');
const icons = require('../core/icons');
const ui = require('../core/ui');
const { readSettings, writeSettings, readKeys, keysTemplate, writeKeys } = require('../core/exec');

const { C, fg, line, textWidth } = ui;
const G = ui.G;

module.exports = ctx => {
  const { state, settings, out, WIDTH, PANE, DEFAULT_KEYS, HELP, keysOf, ask, runAction, render, setSetting, askUpdate,
    finishUpdate, promptLine, chevron } = ctx;

  // The value of a setting and its change(dir): the next value (1: enter, → or a click) or the previous one (-1: ←),
  // or the setting's dialog (left alone by ←); change returns a message or null.
  const onOff = name => ({
    value: ui.onOff(settings[name]),
    change: () => setSetting(name, !settings[name]),
  });
  const choice = (name, values, shown = v => v) => {
    const now = Math.max(0, values.indexOf(settings[name]));
    return { value: [shown(values[now]), fg(C.blue)], change: dir => setSetting(name, values[(now + dir + values.length) % values.length]) };
  };
  const dialog = (value, open) => ({ value: [value, fg(C.blue)], change: dir => (dir > 0 ? open() : null) });
  // diffLayout: one value, or { narrow, wide } once v chose a layout in a diff (diff.js saveMode), read again as other
  // panes write it. Both shown when they differ; a choice here holds for both.
  const diffLayout = () => {
    const values = ['', 'split', 'inline'], m = readSettings().diffLayout;
    const [narrow, wide] = (m && typeof m === 'object' ? [m.narrow, m.wide] : [m, m]).map(v => (values.includes(v) ? v : ''));
    const now = narrow === wide ? values.indexOf(narrow) : -1;
    return { value: [now >= 0 ? values[now] || 'auto' : `narrow ${narrow || 'auto'} · wide ${wide || 'auto'}`, fg(C.blue)],
      change: dir => setSetting('diffLayout', values[now < 0 ? (dir > 0 ? 0 : values.length - 1) : (now + dir + values.length) % values.length]) };
  };

  // The rows of the settings view: a group title, or a setting with its value and its help.
  function settingsRows() {
    const editor = () => ask('Editor', readSettings().editor, cmd => {
      settings.editor = cmd.trim();
      writeSettings({ ...readSettings(), editor: settings.editor });
      return settings.editor ? `Editor: ${settings.editor}` : 'Editor: default';
    }, true);
    const prefix = () => ask('Notes prefix', readSettings().reviewPrefix, text => {
      settings.reviewPrefix = text.trim();
      writeSettings({ ...readSettings(), reviewPrefix: settings.reviewPrefix });
      return settings.reviewPrefix ? `Notes prefix: ${settings.reviewPrefix}` : 'Notes prefix: none';
    }, true);
    const graphics = ctx.graphics;
    return [
      { group: 'APPEARANCE' },
      { label: 'Theme', help: 'Colours for a dark or a light terminal background. Auto: by the terminal\'s background, as each pane reads it. Other panes at their next opening.',
        ...choice('theme', ['auto', 'dark', 'light']) },
      { label: 'Icon set', help: 'Nerd Font glyphs, or emoji, Unicode or ASCII marks for a terminal without a Nerd Font. Other panes at their next opening.',
        ...choice('glyphs', ['nerd', 'emoji', 'unicode', 'ascii'], v => ({ nerd: 'Nerd Font', emoji: 'emoji', unicode: 'Unicode', ascii: 'ASCII' })[v]) },
      { label: 'Icon theme', help: 'Nerd Font icons, or a file icon theme installed for VS Code (its colours only with Unicode or ASCII marks, none with emoji).',
        ...dialog((settings.icons && settings.icons.label) || 'Nerd Font', () => openIconThemes()) },
      { group: 'TREE' },
      { label: 'Hidden files', help: 'Show dot files in the tree.', ...onOff('hidden') },
      { label: 'Git-ignored files', help: 'Show the files git ignores, greyed out.', ...onOff('ignored') },
      { label: 'Compact folders', help: 'Show a folder whose only entry is a folder on one row with it (src/main/java). Not while filtering.', ...onOff('compact') },
      { label: 'Follow terminal', help: 'Reveal the folder of the terminal beside the explorer, moving the root when it leaves the tree.', ...onOff('follow') },
      { group: 'OPENING' },
      { label: 'Enter opens', help: 'Enter and double-click on a text file: show it in the preview or in the editor.', ...choice('open', ['preview', 'editor']) },
      { label: 'Preview follows selection', help: 'A file selected by a click or the arrow keys shows in the preview: arrow keys only update an open one.', ...onOff('singleClick') },
      { label: 'Editor', help: 'Command run with the path by e (and o for code files). Empty: VS Code if installed, else a terminal editor. A terminal editor (vim, nano, hx…) opens in the Editor pane.', ...dialog(settings.editor || 'default', editor) },
      { group: 'PREVIEW' },
      { label: 'Wrap lines', help: 'Wrap long lines in the preview (w). Other previews at their next opening.', ...onOff('wrap') },
      { label: 'Line numbers', help: 'Line numbers in the preview (L). Other previews at their next opening.', ...onOff('lineNumbers') },
      { label: 'Images', help: `Images in the preview: pixelated (half blocks), or sharp with the Kitty graphics protocol of Kitty, Ghostty or WezTerm (p). ${
        graphics === null ? 'Checking this terminal…' : graphics ? 'This terminal has it.' : 'This terminal does not have it: sharp images would not show.'}`,
      ...choice('images', ['auto', 'blocks', 'pixels'], v => ({ blocks: 'pixelated', auto: 'sharp if possible', pixels: 'always sharp' })[v]) },
      { label: 'Diff layout', help: 'Diffs inline or side by side, for narrow and wide panes alike (v in a diff: for its size only). Auto: side by side when the pane is wide enough.', ...diffLayout() },
      { group: 'REVIEW' },
      { label: 'Notes prefix', help: 'Text typed before the review notes sent to the agent (A), such as an instruction: "Fix these:", or a command of the agent.',
        ...dialog(settings.reviewPrefix || 'none', prefix) },
      { group: 'NEW TABS' },
      { label: 'Open automatically', help: 'Open the explorer in new tabs that have a single pane. Off: only with the toggle key (prefix+f with the installer).', ...onOff('autoOpen') },
      { label: 'Dock side', help: 'Where new explorers dock. An open explorer keeps its side.', ...choice('side', ['left', 'right']) },
      { group: 'ADVANCED' },
      { label: 'Key bindings', help: 'Change the explorer keys, saved in keys.json.',
        ...dialog(changedActions().length ? `${changedActions().length} changed` : 'default', () => openKeys()) },
      { label: 'Check for updates', help: 'Look for a newer release on GitHub, once a day.', ...onOff('updateCheck') },
    ];
  }

  // The update to install (or skipped, still installable), or the one the installer must finish: [text, colour,
  // action, key label], or null.
  const pendingUpdate = () => (ctx.finish ? ['! Finish the update', C.yellow, finishUpdate, 'finish']
    : ctx.updating || !settings.updateCheck ? null
      : ctx.latest ? [`↑ ${ctx.latest} available`, C.green, askUpdate, 'update']
        : ctx.skipped ? [`${ctx.skipped} skipped`, C.dim, askUpdate, 'update anyway'] : null);

  function openSettings() {
    state.menu = null;
    state.settings = { sel: 1, top: 0 };
    render();
    return null;
  }

  // --- settings pages: the settings, then the icon themes and the key bindings, opened from them. Each one takes the
  // place of the tree: its title (or an action message), a list scrolled with the selection, a rule, what the page
  // shows of the selection, its keys.

  const pageTitle = (title, W) => line(state.flash ? ui.message(state.flash) : [[' ‹ ', fg(C.dim)], [title, '\x1b[1m']], W);

  // The rows a find keeps: those whose label, value or help has the text typed (any case), under their group title.
  function found(list, p) {
    if (!p.find || !p.find.value) return list;
    const q = p.find.value.toLowerCase(), out = [];
    let group = null;
    for (const r of list) {
      if (r.group) { group = r; continue; }
      if (!`${r.label} ${r.value[0]} ${r.help || ''} ${r.action || ''}`.toLowerCase().includes(q)) continue;
      if (group) { out.push(group); group = null; }
      out.push(r);
    }
    return out;
  }
  const firstRow = list => Math.max(0, list.findIndex(r => !r.group));
  const sameRow = (a, b) => a && b && !a.group && !b.group && a.label === b.label && a.action === b.action;

  // The find of a page (/ or the key bound to filter): typed characters go to it and the list keeps the rows that
  // match; the arrows, Enter and the other keys that are not characters go to the page. Escape closes it, the selection
  // staying on its row. all: the page's rows. Returns true when the key was the find's.
  function findKey(p, k, all) {
    if (!p.find) {
      if (k !== '/' && ctx.keys.get(k) !== 'filter') return false;
      p.find = new ui.Field('');
      return true;
    }
    const r = found(all, p)[p.sel];
    if (k === 'escape') {
      p.find = null;
      const i = all.findIndex(x => sameRow(x, r));
      p.sel = i >= 0 ? i : firstRow(all);
      return true;
    }
    if (!ui.printable(k) && k !== 'space' && k !== 'backspace' && k !== 'delete') return false;
    if (p.find.key(k)) {
      const list = found(all, p), i = list.findIndex(x => sameRow(x, r));
      p.sel = i >= 0 ? i : firstRow(list);
      p.top = 0;
    }
    return true;
  }

  // The find's line: the field, and the count of rows kept.
  function findLine(p, list, W) {
    const count = p.find.value ? ` ${list.filter(r => !r.group).length}` : '';
    const field = p.find.segments(Math.max(1, W - 4 - count.length));
    return line([[` ${G.search} `, fg(C.dim)], ...field, [count.padStart(Math.max(0, W - 3 - ui.segWidth(field))), fg(C.dim)]], W, C.header);
  }

  // The rows of a list from p.top, n at most, the selection kept in view with the title of its group: a group title,
  // or a label with a value on the right ([text, style]).
  function listLines(list, p, n, W) {
    const first = list[p.sel - 1] && list[p.sel - 1].group ? p.sel - 1 : p.sel;
    p.top = Math.max(0, Math.min(p.top, first, list.length - n));
    if (p.sel >= p.top + n) p.top = p.sel - n + 1;
    const lines = list.slice(p.top, p.top + n).map((r, k) => {
      if (r.group) return line([[` ${r.group}`, '\x1b[1m' + fg(C.dim)]], W);
      // The value keeps at least half the room, the label is cut first.
      const value = ui.ellipsis(r.value[0], Math.max(1, W - 4 - Math.min(textWidth(r.label), Math.floor((W - 4) / 2))));
      const label = ui.ellipsis(r.label, Math.max(1, W - 4 - textWidth(value)));
      const pad = ' '.repeat(Math.max(1, W - 3 - textWidth(label) - textWidth(value)));
      const back = p.top + k === p.sel ? (state.focused ? C.select : C.selectBlur) : null;
      return line([[`  ${label}${pad}`, ''], [value, r.value[1]]], W, back);
    });
    if (!list.length) lines.push(...ui.empty(W, n, 'No match', [['esc', 'clear']]));
    while (lines.length < n) lines.push(line([], W));
    return lines;
  }

  // The rule under a list, with ▴ ▾ when it scrolls.
  function rule(list, p, n, W) {
    const more = (p.top > 0 ? '▴' : '') + (p.top + n < list.length ? '▾' : '');
    return line([['─'.repeat(Math.max(0, W - more.length - 1)) + more, fg(C.dim)]], W);
  }

  // The selection moved by dir rows, past the group titles, within the list.
  function moveIn(list, p, dir) {
    const step = dir < 0 ? -1 : 1, ok = i => list[i] && !list[i].group;
    let i = Math.max(0, Math.min(list.length - 1, p.sel + dir));
    if (!ok(i)) i = ok(i + step) ? i + step : i - step;
    p.sel = i;
  }

  // The page shown: the settings, or one opened from them.
  const settingsPage = () => (state.settings.icons ? { view: iconsView, key: iconsKey, mouse: iconsMouse }
    : state.settings.keys ? { view: keysView, key: keysKey, mouse: keysMouse }
      : { view: settingsView, key: settingsKey, mouse: settingsMouse });

  // The moves of the bound keys, page by page of n rows. ARROWS: the keys that move in a page whatever the bindings.
  const ARROWS = { up: 'up', down: 'down', pageup: 'pageUp', pagedown: 'pageDown', home: 'top', end: 'bottom' };
  function moves(n) {
    const page = n - 1;
    return { up: -1, down: 1, pageUp: -page, pageDown: page, halfPageUp: -Math.ceil(page / 2), halfPageDown: Math.ceil(page / 2),
      top: -Infinity, bottom: Infinity };
  }

  // --- the settings

  // The settings view: the find when open and the update when there is one under the title, the settings, the help of
  // the selected one, the keys, and a prompt. at: screen row of the first setting, n: the rows they get, update: the
  // row of the update (0: none), detail: the help's lines.
  function settingsLayout(H, W) {
    const find = state.settings.find ? 1 : 0, update = pendingUpdate() ? 1 + find : 0;
    const at = 1 + find + (update ? 1 : 0), detail = detailLines(W);
    return { at, n: Math.max(1, H - at - 2 - detail - (state.prompt ? 1 : 0)), update, detail };
  }
  // The lines of the longest help at this width, whichever setting is selected, so that the list does not move: from
  // 1 to MAX_DETAIL.
  const MAX_DETAIL = 4;
  const detailLines = W => Math.min(MAX_DETAIL, Math.max(1, ...settingsRows().filter(r => r.help).map(r => ui.wrap(r.help, W - 2).length)));
  const settingsKeys = W => ui.keys([['←→', 'change'], ['/', 'find'], ['esc', 'back']], W);

  function settingsView(W, H) {
    const s = state.settings, list = found(settingsRows(), s), up = pendingUpdate(), { n, detail } = settingsLayout(H, W);
    const lines = [pageTitle('Settings', W)];
    if (s.find) lines.push(findLine(s, list, W));
    if (up) {
      // The key on the right, left out when it does not fit.
      let hint = ui.keys([['u', up[3]]]);
      if (textWidth(up[0]) + 2 + ui.segWidth(hint) > W) hint = [];
      lines.push(line([[` ${up[0]}`.padEnd(Math.max(0, W - 1 - ui.segWidth(hint))), fg(up[1])], ...hint], W));
    }
    lines.push(...listLines(list, s, n, W), rule(list, s, n, W));
    const r = list[s.sel];
    // The help wrapped, its last line cut when it needs more.
    const help = ui.wrap(r && !r.group ? r.help : '', W - 2);
    if (help.length > detail) help.splice(detail - 1, help.length, ui.ellipsis(help.slice(detail - 1).join(' '), W - 2));
    for (let i = 0; i < detail; i++) lines.push(line([[` ${help[i] || ''}`, fg(C.dim)]], W));
    lines.push(line(settingsKeys(W), W, C.header));
    if (state.prompt) lines.push(promptLine(W));
    while (lines.length < H) lines.push(line([], W));
    lines.length = H;
    return lines;
  }

  // ← → and enter change the selected setting, u installs the update. Escape, and the keys bound to close and
  // settings, go back to the tree.
  function settingsKey(k) {
    const s = state.settings;
    if (findKey(s, k, settingsRows())) return render();
    const list = found(settingsRows(), s), r = list[s.sel], action = ctx.keys.get(k), up = pendingUpdate();
    const move = moves(settingsLayout(out.rows || 24, out.columns || 80).n), to = ARROWS[k] || action;
    if (k === 'escape' || action === 'close' || action === 'settings') state.settings = null;
    else if (k === 'u' && up) return runAction(up[2]);
    else if (to in move) moveIn(list, s, move[to]);
    else if (r && !r.group && (k === 'enter' || k === 'right' || action === 'open' || action === 'expand')) return runAction(() => r.change(1));
    else if (r && !r.group && (k === 'left' || action === 'collapse')) return runAction(() => r.change(-1));
    render();
  }

  // The wheel moves the selection. A click on the title goes back, on the update installs it, on a setting changes it,
  // on a key hint acts as its key; elsewhere, it cancels a prompt.
  function settingsMouse(m) {
    const s = state.settings, list = found(settingsRows(), s), W = out.columns || WIDTH, y = m.y - 1;
    const { at, n, update, detail } = settingsLayout(out.rows || 24, out.columns || 80);
    if (m.wheel) { moveIn(list, s, m.wheel); return render(); }
    if (!m.press || m.drag || m.button !== 0) return;
    if (state.prompt) { state.prompt = null; return render(); }
    if (y === 0) { state.settings = null; return render(); }
    if (update && y === update) return runAction(pendingUpdate()[2]);
    const r = list[s.top + y - at];
    if (y >= at && y < at + n && r && !r.group) {
      s.sel = s.top + y - at;
      return runAction(() => r.change(1));
    }
    // The keys come after the list, its rule and the help lines.
    const k = y === at + n + 1 + detail && ui.keyAt(settingsKeys(W), m.x - 1);
    if (k) settingsKey(k);
  }

  // --- icon themes

  // The default icons and the VS Code themes installed (a scan of the editors' extensions, made when the page opens),
  // above a sample tree drawn with the theme under the selection. Enter saves it and goes back to the settings; Escape
  // goes back without a change.
  const NERD_FONT = { id: '', label: 'Nerd Font (default)' };
  const SAMPLE = [
    [0, 'src', true, true], [1, 'components', true], [1, 'App.tsx'], [1, 'main.py'], [1, 'styles.css'],
    [0, '.github', true], [0, 'docs', true], [0, '.gitignore'], [0, 'Dockerfile'], [0, 'logo.png'],
    [0, 'package.json'], [0, 'README.md'],
  ];

  function openIconThemes() {
    if (!state.settings) return null; // closed by a key typed meanwhile (the action runs after the keys already read)
    const current = (settings.icons && settings.icons.id) ? settings.icons : NERD_FONT;
    const found = icons.themes();
    // The saved theme kept in the list when the scan misses it (an editor folder out of reach).
    const list = [NERD_FONT, ...(current.id && !found.some(t => t.id === current.id) ? [current] : []), ...found];
    state.settings.icons = { list, sel: Math.max(0, list.findIndex(t => t.id === current.id)), top: 0, loaded: new Map() };
    return null;
  }

  // The theme of an entry of the list, loaded once (null: the default icons, or a theme not found).
  function loadTheme(t) {
    const { loaded } = state.settings.icons;
    if (!loaded.has(t.id)) loaded.set(t.id, t.id ? icons.load(t) : null);
    return loaded.get(t.id);
  }

  function chooseTheme(t) {
    const chosen = loadTheme(t);
    if (t.id && !chosen) return `${t.label}: not found`;
    ctx.theme = chosen;
    settings.icons = chosen ? chosen.entry : '';
    writeSettings({ ...readSettings(), icons: settings.icons });
    if (state.settings) state.settings.icons = null;
    return null;
  }

  // The themes as list rows, the saved one checked.
  const themeRows = () => state.settings.icons.list.map(t => ({
    label: t.label, value: [t.id === ((settings.icons && settings.icons.id) || '') ? '✓' : '', fg(C.green)],
  }));

  // The sample gets at least 4 rows, the themes what remains. at: screen row of the first theme, n: the rows they get.
  function iconsLayout(H) {
    const room = H - 3, count = state.settings.icons.list.length;
    const sample = Math.max(0, Math.min(SAMPLE.length, Math.max(Math.min(4, room - 1), room - count)));
    return { at: 1, n: Math.max(1, Math.min(count, room - sample)), sample };
  }
  const iconsKeys = W => ui.keys([['↑↓', 'move'], ['⏎', 'choose'], ['esc', 'back']], W);

  function iconsView(W, H) {
    const p = state.settings.icons, list = themeRows(), { n, sample } = iconsLayout(H);
    const lines = [pageTitle('Icon theme', W), ...listLines(list, p, n, W), rule(list, p, n, W)];
    // The sample tree, in the theme under the selection.
    const th = loadTheme(p.list[p.sel]);
    for (const [depth, name, dir, open] of SAMPLE.slice(0, sample)) {
      const icon = icons.icon(th, name, dir, open);
      const mark = dir ? chevron(open) : ' ';
      lines.push(line([[' '.repeat(1 + depth * 2) + mark + ' ', fg(C.dim)], [icon[0], fg(icon[1])], [name, '']], W));
    }
    while (lines.length < H - 1) lines.push(line([], W));
    lines.push(line(iconsKeys(W), W, C.header));
    lines.length = H;
    return lines;
  }

  function iconsKey(k) {
    const p = state.settings.icons, action = ctx.keys.get(k), move = moves(iconsLayout(out.rows || 24).n);
    if (k === 'escape' || ['close', 'settings', 'collapse'].includes(action)) state.settings.icons = null;
    else if (action in move) moveIn(p.list, p, move[action]);
    else if (action === 'open' || action === 'expand') return runAction(() => chooseTheme(p.list[p.sel]));
    render();
  }

  // The wheel moves the selection; a click on the title goes back, on a theme chooses it, on a key hint acts as its key.
  function iconsMouse(m) {
    const p = state.settings.icons, W = out.columns || WIDTH, H = out.rows || 24, { at, n } = iconsLayout(H), y = m.y - 1;
    if (m.wheel) { moveIn(p.list, p, m.wheel); return render(); }
    if (!m.press || m.drag || m.button !== 0) return;
    if (y === 0) { state.settings.icons = null; return render(); }
    if (y >= at && y < at + n && p.list[p.top + y - at]) {
      p.sel = p.top + y - at;
      return runAction(() => chooseTheme(p.list[p.sel]));
    }
    const k = y === H - 1 && ui.keyAt(iconsKeys(W), m.x - 1);
    if (k) iconsKey(k);
  }

  // --- key bindings

  // The explorer's actions by group, as in the key help, each with its keys; below the rule, the selected action's
  // keys and its default ones. Enter waits for a key and adds it to the action (taken from the action that had it),
  // Delete removes the action's last key, r gives it back its default keys. Saved in keys.json, which the last row
  // opens in the editor. The page's own keys are the arrows and the bound ones, so that a rebinding cannot lock it.
  function openKeys() {
    if (!state.settings) return null;
    state.settings.keys = { sel: 1, top: 0, find: null, capture: false };
    return null;
  }

  const sameKeys = (a, b) => a.length === b.length && a.every(k => b.includes(k));
  const changedActions = () => Object.keys(DEFAULT_KEYS).filter(a => !sameKeys(keysOf(a), DEFAULT_KEYS[a]));
  const keyText = list => list.map(ui.keyLabel).join(' ') || '—';

  function keyRows() {
    return [
      ...HELP.flatMap(([group, items]) => [{ group: group.toUpperCase() }, ...items.map(([action, text]) => ({
        action, label: text[0].toUpperCase() + text.slice(1),
        value: [keyText(keysOf(action)), fg(sameKeys(keysOf(action), DEFAULT_KEYS[action]) ? C.blue : C.yellow)],
      }))]),
      { group: 'FILE' },
      { label: 'Open keys.json', value: ['edit…', fg(C.blue)] },
    ];
  }

  // The changes saved, then read back at once rather than at the next check of the file.
  function saveKeys(changes) {
    if (!writeKeys('explorer', changes)) return 'Cannot write keys.json';
    ctx.keys = readKeys('explorer', DEFAULT_KEYS);
    return null;
  }

  // The default keys of an action given back, taken from the actions that have them now; the keys it took from other
  // actions given back to them.
  function resetKeys(action) {
    const changes = { [action]: null };
    const now = a => (a in changes ? changes[a] || DEFAULT_KEYS[a] : keysOf(a));
    for (const k of DEFAULT_KEYS[action]) {
      const owner = ctx.keys.get(k);
      if (owner && owner !== action) changes[owner] = now(owner).filter(x => x !== k);
    }
    for (const k of keysOf(action)) {
      const home = Object.keys(DEFAULT_KEYS).find(a => a !== action && DEFAULT_KEYS[a].includes(k));
      if (home && !now(home).includes(k)) changes[home] = [...now(home), k];
    }
    for (const a of Object.keys(changes)) if (changes[a] && sameKeys(changes[a], DEFAULT_KEYS[a])) changes[a] = null;
    return saveKeys(changes);
  }

  // The key pressed while waiting, added to the action and taken from the one that had it.
  function bindKey(action, k) {
    const owner = ctx.keys.get(k);
    if (owner === action) return `${ui.keyLabel(k)}: already bound`;
    const changes = { [action]: [...keysOf(action), k] };
    if (owner) changes[owner] = keysOf(owner).filter(x => x !== k);
    const label = owner && HELP.flatMap(([, items]) => items).find(([a]) => a === owner);
    return saveKeys(changes) || (owner ? `${ui.keyLabel(k)} taken from ${label ? label[1] : owner}` : null);
  }

  function keysLayout(H) {
    const find = state.settings.keys.find ? 1 : 0;
    return { at: 1 + find, n: Math.max(1, H - 7 - find) };
  }
  const keysKeys = (W, capture) => ui.keys(capture ? [['esc', 'cancel']] : [['⏎', 'add'], ['del', 'remove'], ['/', 'find']], W);

  function keysView(W, H) {
    const p = state.settings.keys, list = found(keyRows(), p), r = list[p.sel], { n } = keysLayout(H);
    const lines = [pageTitle('Key bindings', W), ...(p.find ? [findLine(p, list, W)] : []), ...listLines(list, p, n, W), rule(list, p, n, W)];
    if (r && r.action) {
      const now = keysOf(r.action), changed = !sameKeys(now, DEFAULT_KEYS[r.action]);
      lines.push(line([[` ${r.label}`, '\x1b[1m'], [changed ? '  changed' : '', fg(C.yellow)]], W));
      lines.push(line([[' Keys     ', fg(C.dim)], [keyText(now), fg(changed ? C.yellow : C.blue)]], W));
      lines.push(line([[' Default  ', fg(C.dim)], [keyText(DEFAULT_KEYS[r.action]), fg(C.dim)]], W));
      // r, which the keys line has no room for, told where it applies (typed in the find while it is open).
      lines.push(line(p.capture ? [[' Press the new key…', fg(C.yellow)]] : changed && !p.find ? ui.keys([['r', 'back to the default keys']], W) : [], W));
    } else if (r && !r.group) {
      lines.push(line([[' keys.json', '\x1b[1m']], W), line([[` ${ui.ellipsis('Opened in the editor', W - 2)}`, fg(C.dim)]], W));
    }
    while (lines.length < H - 1) lines.push(line([], W));
    lines.push(line(keysKeys(W, p.capture), W, C.header));
    lines.length = H;
    return lines;
  }

  function keysKey(k) {
    const p = state.settings.keys;
    if (p.capture) {
      const r = found(keyRows(), p)[p.sel];
      p.capture = false;
      return k === 'escape' ? render() : runAction(() => bindKey(r.action, k));
    }
    if (findKey(p, k, keyRows())) return render();
    const list = found(keyRows(), p), r = list[p.sel], action = ctx.keys.get(k);
    const move = moves(keysLayout(out.rows || 24).n), to = ARROWS[k] || action;
    if (k === 'escape' || k === 'left' || ['close', 'settings', 'collapse'].includes(action)) state.settings.keys = null;
    else if (to in move) moveIn(list, p, move[to]);
    else if (!r || r.group) return render();
    else if (k === 'enter' || action === 'open' || action === 'expand') {
      if (!r.action) return runAction(() => { const file = keysTemplate('explorer', DEFAULT_KEYS); return file ? actions.edit(file, PANE) : 'No config folder'; });
      p.capture = true;
    } else if (r.action && (k === 'delete' || k === 'backspace')) {
      const now = keysOf(r.action);
      return runAction(() => (now.length ? saveKeys({ [r.action]: now.slice(0, -1) }) : 'No key to remove'));
    } else if (r.action && k === 'r') return runAction(() => resetKeys(r.action));
    render();
  }

  // The wheel moves the selection. A click on the title goes back, on an action selects it (on the selected one, waits
  // for its new key), on a key hint acts as its key; any click while waiting cancels.
  function keysMouse(m) {
    const p = state.settings.keys, list = found(keyRows(), p), W = out.columns || WIDTH, H = out.rows || 24, { at, n } = keysLayout(H), y = m.y - 1;
    if (m.wheel) { if (!p.capture) moveIn(list, p, m.wheel); return render(); }
    if (!m.press || m.drag || m.button !== 0) return;
    if (p.capture) { p.capture = false; return render(); }
    if (y === 0) { state.settings.keys = null; return render(); }
    const i = p.top + y - at;
    if (y >= at && y < at + n && list[i] && !list[i].group) {
      if (i === p.sel) return keysKey('enter');
      p.sel = i;
      return render();
    }
    const k = y === H - 1 && ui.keyAt(keysKeys(W, false), m.x - 1);
    if (k) keysKey(k);
  }

  return { settingsPage, openSettings };
};
