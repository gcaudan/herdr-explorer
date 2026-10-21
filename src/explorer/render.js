// Drawing of the explorer's tree view: the header, the rows with their git decorations, the name being typed, the
// filter's and the changed files' lines, and the action menu. Each function returns or fills lines; render() in
// explorer.js paints them. ctx: see explorer.js.
'use strict';
const path = require('path');
const icons = require('../core/icons');
const ui = require('../core/ui');

const { C, GIT, fg, line, fit, textWidth } = ui;
const G = ui.G;
const chevron = open => (open ? G.open : G.closed);

// A row being named: the field's colours.
const FIELD = [60, 60, 60];
// The part of a name matching the filter.
const HIT = '\x1b[1m' + fg([24, 163, 255]);

module.exports = ctx => {
  const { state, out, scrollbar, tree, git, settings, HOME, keysOf } = ctx;

  const bodyHeight = () => (out.rows || 24) - 1 - (state.filter || state.prompt || state.changed ? 1 : 0);

  // The tree, with the name being typed in it (Rename, New file), then at the bottom the prompt, its keys or the filter.
  function treeView(W, H) {
    const lines = [header(W)];
    const listH = bodyHeight();
    const shown = shownRows(), edit = shown.findIndex(e => e.edit);
    if (edit >= 0 && edit < state.top) state.top = edit;
    if (edit >= state.top + listH) state.top = edit - listH + 1;
    if (state.top > shown.length - listH) state.top = Math.max(0, shown.length - listH);
    const bw = scrollbar.set(shown.length, listH, state.top);
    const selected = state.rows[state.cursor];
    for (let i = state.top; i < Math.min(shown.length, state.top + listH); i++) {
      const e = shown[i];
      lines.push((e.edit ? editRow(e, W - bw) : row(e.r, W - bw, e.r === selected)) + scrollbar.cell(i - state.top));
    }
    if (!shown.length) {
      const first = a => ui.keyLabel(keysOf(a)[0] || '');
      lines.push(...(state.filter ? ui.empty(W, listH, 'No match', [['esc', 'clear']])
        : state.changed ? ui.empty(W, listH, git.top ? 'No changes' : 'Not a git repository', [['esc', 'all files']])
        : ui.empty(W, listH, 'Empty folder', [[first('newFile'), 'new file'], [first('newFolder'), 'folder']].filter(([k]) => k))));
    }
    while (lines.length < listH + 1) lines.push(line([], W));
    if (state.prompt) lines.push(state.prompt.inline ? line(promptKeys(W), W, C.header) : promptLine(W));
    else if (state.filter) {
      lines.push(filterLine(W));
    } else if (state.changed) lines.push(changedLine(W));
    if (state.menu) state.menu.draw(lines, W, H);
    while (lines.length < H) lines.push(line([], W));
    lines.length = H;
    return lines;
  }

  // The rows shown: the tree's, the one being renamed made a field, or a new one at the top of the folder it goes to.
  function shownRows() {
    const shown = state.rows.map(r => ({ r }));
    const at = state.prompt && state.prompt.inline;
    if (!at) return shown;
    if (at.rel !== undefined) {
      const i = state.rows.findIndex(r => r.rel === at.rel);
      if (i >= 0) shown[i] = { r: state.rows[i], edit: true };
      return shown;
    }
    const i = at.dir ? state.rows.findIndex(r => r.rel === at.dir) : -1;
    shown.splice(i + 1, 0, { edit: true, depth: i >= 0 ? state.rows[i].depth + 1 : 0, dir: at.folder });
    return shown;
  }

  // A row being named, its icon following the name typed.
  function editRow(e, W) {
    const f = state.prompt.field, r = e.r, dir = r ? r.dir : e.dir, open = r ? r.open : false;
    const icon = icons.icon(ctx.theme, f.value || (r ? r.name : ''), dir, open);
    const lead = ' '.repeat(1 + (r ? r.depth : e.depth) * 2) + (dir ? chevron(open) : ' ') + ' ';
    return line([[lead, fg(C.dim)], [icon[0], fg(icon[1])], ...f.segments(Math.max(1, W - textWidth(lead) - textWidth(icon[0]) - 1))], W, FIELD);
  }

  // The keys of a name typed in the tree, and where a new entry goes.
  function promptKeys(W) {
    const at = state.prompt.inline;
    const what = at.rel !== undefined ? 'rename' : `create in ${path.basename(at.dir || ctx.ROOT)}`;
    return ui.keys([['⏎', what], ['esc', 'cancel']], W);
  }

  // A prompt at the bottom (the editor command).
  function promptLine(W) {
    const label = ` ${state.prompt.label}: `;
    return line([[label, fg(C.dim)], ...state.prompt.field.segments(W - textWidth(label))], W, C.header);
  }

  // Header: space name (or an action message), then the icons: ! when the installer must finish an
  // update, else ↑ when a newer release exists (left out while a message shows), and the settings gear.
  const showUpdate = () => !state.flash && (ctx.finish || (ctx.latest && settings.updateCheck && !ctx.updating));
  function header(W) {
    const ROOT = ctx.ROOT;
    const up = !showUpdate() ? [] : ctx.finish ? [['! ', fg(C.yellow)]] : [['↑ ', fg(C.green)]];
    const icons = [...up, [`${G.gear} `, fg(C.dim)]];
    const room = W - 3 - (up.length ? 2 : 0);
    if (state.flash) {
      const msg = ui.slice(ui.message(state.flash), 0, room);
      return line([...msg, [' '.repeat(Math.max(0, room - ui.segWidth(msg) + 1)), ''], ...icons], W);
    }
    const [name, nw] = fit(` ${path.basename(ROOT).toUpperCase() || ROOT}`, room);
    return line([[name, ROOT === HOME ? '\x1b[1m' : '\x1b[1;3m'], [' '.repeat(Math.max(0, room - nw + 1)), ''], ...icons], W);
  }

  // The filter's line: while typed, the field and the count on the right; once kept (Tab), the filter in blue, the
  // count, and how to clear it.
  function filterLine(W) {
    const f = state.filter, n = state.rows.filter(r => r.hit).length;
    const count = f.field.value ? `${n} match${n === 1 ? '' : 'es'}` : '';
    const lead = [` ${G.search} `, fg(C.dim)];
    if (f.active) {
      const field = f.field.segments(Math.max(1, W - 5 - count.length));
      return line([lead, ...field, [count.padStart(Math.max(0, W - 4 - ui.segWidth(field))), fg(C.dim)]], W, C.header);
    }
    const left = [lead, [f.field.value, fg(C.blue)], [count && `  ${count}`, fg(C.dim)]];
    const hint = ui.keys([['esc', 'clear']]);
    const room = W - ui.segWidth(left) - ui.segWidth(hint) - 1;
    return line(room >= 0 ? [...left, [' '.repeat(room), ''], ...hint] : left, W, C.header);
  }

  // The line of the changed files only view: how many, and how to leave it.
  function changedLine(W) {
    const n = state.rows.filter(r => !r.dir).length;
    const left = [[' ● ', fg(C.blue)], ['Changes', fg(C.blue)], [`  ${n} file${n === 1 ? '' : 's'}`, fg(C.dim)]];
    const hint = ui.keys([['esc', 'all files']]);
    const room = W - ui.segWidth(left) - ui.segWidth(hint) - 1;
    return line(room >= 0 ? [...left, [' '.repeat(room), ''], ...hint] : left, W, C.header);
  }

  function row(r, W, selected) {
    const deco = tree.decoration(r);
    // A deleted entry no longer on disk: struck through.
    const color = (deco ? fg(GIT[deco]) : '') + (r.gone ? '\x1b[9m' : '');
    const icon = icons.icon(ctx.theme, r.name, r.dir, r.open);
    const indent = ' '.repeat(1 + r.depth * 2);
    const mark = r.dir ? chevron(r.open) : ' ';
    const badge = !deco || deco === '!' ? '' : r.dir ? '●' : deco === 'C' ? '!' : deco;
    const lead = indent.length + 2 + textWidth(icon[0]); // the chevron, a space and the icon
    const room = W - lead - (badge ? 2 : 0) - 1;
    let name = r.name;
    if (textWidth(name) > room) name = fit(name, Math.max(1, room - 1))[0] + '…';
    const pad = Math.max(0, W - lead - textWidth(name) - (badge ? 2 : 0) - 1);
    // The part matching the filter in bold blue, as in Go to file; a folder shown only for what it holds, dimmed.
    const parts = r.hit && r.hit[1] <= name.length
      ? [[name.slice(0, r.hit[0]), color], [name.slice(r.hit[0], r.hit[1]), HIT], [name.slice(r.hit[1]), color]]
      : [[name, state.filter && state.filter.field.value && !r.hit ? fg(C.dim) : color]];
    return line([
      [indent + mark + ' ', fg(C.dim)], [icon[0], fg(deco === '!' ? GIT['!'] : icon[1])],
      ...parts, [' '.repeat(pad) + (badge ? ' ' + badge : ''), color],
    ], W, selected ? (state.focused ? C.select : C.selectBlur) : null);
  }

  return { chevron, bodyHeight, treeView, promptLine, showUpdate };
};
