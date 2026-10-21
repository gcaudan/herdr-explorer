// The explorer's keyboard and mouse: a question or a prompt first, then the menu, the settings, the key help, the
// filter, and the tree's actions by their keys. ctx: see explorer.js.
'use strict';
const path = require('path');
const actions = require('../core/actions');
const ui = require('../core/ui');

// While filtering, typed characters go to the filter; arrows and Enter act on the list.
const FILTER_EDIT = new Set(['backspace', 'delete', 'ctrl+u', 'ctrl+d']);

// Preview follows selection, from the keyboard: once the cursor stops (a key held down sends a burst), and only in a
// preview already open, so that moving never splits the tab.
const MOVES = new Set(['up', 'down', 'pageUp', 'pageDown', 'halfPageUp', 'halfPageDown', 'top', 'bottom', 'expand', 'collapse']);

// The actions that need the entry on disk, refused on a deleted one.
const ON_DISK = new Set(['rename', 'delete', 'edit', 'openInEditor', 'openInApp', 'preview', 'copyPath', 'newFile', 'newFolder', 'searchInFiles']);

module.exports = ctx => {
  const { state, settings, out, WIDTH, PANE, git, scrollbar, render, runAction, flash, buildRows, reveal, revealRel, select,
    toggle, close, schedule, openFile, openMenu, openHelp, openSettings, setSetting, newEntry, renameEntry, deleteEntry,
    setRoot, showChanged, viewChanges, nextChange, widen, absOf, parentOf, settingsPage, menuRows, bodyHeight, showUpdate,
    finishUpdate, askUpdate } = ctx;

  // --- action menu

  function moveMenu(dir) {
    const m = state.menu;
    do m.sel = (m.sel + dir + m.items.length) % m.items.length; while (!m.items[m.sel]);
    const n = menuRows();
    if (m.sel < m.off) m.off = m.sel;
    if (m.sel >= m.off + n) m.off = m.sel - n + 1;
  }

  function closeMenu() {
    state.menu = null;
  }

  // Escape always closes the menu, whatever the bindings; the key of an item runs it.
  function menuKey(k) {
    const m = state.menu, action = ctx.keys.get(k);
    if (k === 'escape') closeMenu();
    else if (action === 'up') moveMenu(-1);
    else if (action === 'down') moveMenu(1);
    else if (action === 'open' || action === 'expand') return runAction(m.items[m.sel][1]);
    else {
      // The key shown beside an item runs it (Settings… on ,); the others close the menu.
      const item = action && m.items.find(it => it && it[2] === action);
      if (item) return runAction(item[1]);
      if (['close', 'collapse', 'menu', 'settings'].includes(action)) closeMenu();
    }
    render();
  }

  // --- keyboard and mouse

  // A question (ui.confirmKey), or a prompt.
  function promptKey(k) {
    if (state.confirm) {
      const q = state.confirm, answer = ui.confirmKey(k, q);
      if (answer === 'move') return render();
      state.confirm = null;
      if (answer === 'yes') return runAction(q.yes);
      return render();
    }
    const p = state.prompt;
    if (k === 'escape') state.prompt = null;
    else if (k === 'enter') { state.prompt = null; const v = p.field.value; if (v.trim() || p.empty) runAction(() => p.done(v)); }
    else p.field.key(k);
    render();
  }

  function filterKey(k) {
    const f = state.filter;
    if (k === 'escape') return closeFilter();
    // Bound keys that are not characters (arrows, Enter, ctrl+…) act on the list; characters and the field's editing
    // keys go to the filter (ctrl+u and ctrl+d move half a page in the tree only).
    if (ctx.keys.has(k) && !ui.printable(k) && !FILTER_EDIT.has(k)) return listKey(k);
    if (k === 'tab') { f.active = false; return render(); }
    if (f.field.key(k)) {
      state.selected = null;
      if (!f.field.value) { state.filter = { field: f.field, active: true }; }
      buildRows();
      state.top = 0;
      reveal();
      render();
    }
  }

  function closeFilter() {
    const sel = state.selected;
    state.filter = null;
    if (sel) revealRel(sel); else buildRows();
    render();
  }

  function key(k) {
    if (state.confirm || state.prompt) return promptKey(k);
    if (state.menu) return menuKey(k);
    if (state.settings) return settingsPage().key(k);
    if (state.help) { if (!ui.helpKey(k, state.help)) state.help = null; return render(); }
    if (state.filter && state.filter.active) return filterKey(k);
    if (state.filter && (k === 'escape' || ctx.keys.get(k) === 'close')) return closeFilter();
    if (state.changed && (k === 'escape' || ctx.keys.get(k) === 'close')) return showChanged(false);
    listKey(k);
  }

  function listKey(k) {
    listAction(ctx.keys.get(k));
  }

  let following = null;
  function followSelection() {
    clearTimeout(following);
    if (!settings.singleClick) return;
    following = setTimeout(() => {
      const r = state.rows[state.cursor];
      if (!r || r.dir || !actions.canPreview(absOf(r.rel))) return;
      actions.preview(absOf(r.rel), ctx.ROOT, PANE, 0, false, false, { view: state.changed ? 'diff' : 'file' }).then(flash, () => { });
    }, 150);
  }

  // An action of the tree, from its key.
  function listAction(action) {
    const r = state.rows[state.cursor];
    const page = bodyHeight() - 1;
    const dirOf = () => (r ? (r.dir ? r.rel : parentOf(r.rel)) : '');
    if (r && r.gone && ON_DISK.has(action)) return flash('Deleted: Discard changes in the menu restores it');
    switch (action) {
      case 'up': select(state.cursor - 1); break;
      case 'down': select(state.cursor + 1); break;
      case 'pageUp': select(state.cursor - page); break;
      case 'pageDown': select(state.cursor + page); break;
      case 'halfPageUp': select(state.cursor - Math.ceil(page / 2)); break;
      case 'halfPageDown': select(state.cursor + Math.ceil(page / 2)); break;
      case 'help': return openHelp();
      case 'close': return close();
      case 'top': select(0); break;
      case 'bottom': select(state.rows.length - 1); break;
      case 'open':
        if (r && !r.dir) return openFile(r);
        toggle(r);
        break;
      case 'menu': return openMenu(state.rows.length ? state.cursor : null);
      case 'expand':
        if (r && r.dir && !r.open) toggle(r, true);
        else if (r && r.dir) select(state.cursor + 1);
        break;
      case 'collapse':
        if (r && r.dir && r.open && !state.filter) toggle(r, false);
        else if (r) { const p = state.rows.findIndex(x => x.rel === parentOf(r.rel)); if (p >= 0) select(p); }
        break;
      case 'refresh': schedule(); return;
      case 'filter': state.filter = { field: new ui.Field(state.filter ? state.filter.field.value : ''), active: true }; buildRows(); break;
      case 'goToFile': return runAction(() => actions.search(ctx.ROOT, PANE, 'files'));
      case 'searchInFiles': return runAction(() => actions.search(r && r.dir ? absOf(r.rel) : ctx.ROOT, PANE, 'content'));
      case 'sourceControl': return runAction(() => (git.top ? actions.scm(ctx.ROOT, PANE, r ? absOf(r.rel) : '') : 'Not a git repository'));
      case 'settings': return openSettings();
      case 'toggleHidden': setSetting('hidden', !settings.hidden); flash(settings.hidden ? 'Hidden files shown' : 'Hidden files hidden'); return;
      case 'toggleIgnored': setSetting('ignored', !settings.ignored); flash(settings.ignored ? 'Ignored files shown' : 'Ignored files hidden'); return;
      case 'newFile': return newEntry(dirOf(), false);
      case 'newFolder': return newEntry(dirOf(), true);
      case 'rename': if (r) renameEntry(r); return;
      case 'delete': if (r) deleteEntry(r); return;
      case 'edit': if (r && !r.dir) runAction(() => actions.edit(absOf(r.rel), PANE)); return;
      case 'openInEditor': if (r) runAction(() => actions.editor(absOf(r.rel), PANE)); return;
      case 'openInApp': if (r && !r.dir) runAction(() => actions.system(absOf(r.rel))); return;
      case 'preview': if (r && !r.dir) runAction(() => actions.preview(absOf(r.rel), ctx.ROOT, PANE, 0, true, true, { keep: true })); return;
      case 'copyPath': if (r) runAction(() => actions.copy(absOf(r.rel))); return;
      case 'copyRelativePath': if (r) runAction(() => actions.copy(r.rel)); return;
      case 'parentRoot': return setRoot(path.dirname(ctx.ROOT));
      case 'changedOnly': return showChanged(!state.changed);
      case 'viewChanges':
        if (r) runAction(() => (git.top ? viewChanges(r) : 'Not a git repository'));
        return;
      case 'nextChange': return nextChange(1);
      case 'previousChange': return nextChange(-1);
      case 'fileHistory':
        if (r && !r.dir) runAction(() => (git.top ? actions.scm(ctx.ROOT, PANE, absOf(r.rel), true) : 'Not a git repository'));
        return;
      case 'narrower': return widen(-4);
      case 'wider': return widen(4);
      default: return;
    }
    if (MOVES.has(action)) followSelection();
    reveal();
    render();
  }

  function mouse(m) {
    const H = out.rows || 24, W = out.columns || WIDTH;
    // A question: a click on its first button confirms, any other click cancels.
    if (state.confirm) {
      const q = state.confirm, answer = ui.confirmMouse(m, q);
      if (!answer) return;
      state.confirm = null;
      if (answer === 'yes') return runAction(q.yes);
      return render();
    }
    if (m.wheel && state.menu) { moveMenu(m.wheel); return render(); }
    if (state.settings) return settingsPage().mouse(m);
    if (state.help) { if (!ui.helpMouse(m, state.help)) state.help = null; return render(); }
    if (m.wheel) {
      const max = Math.max(0, state.rows.length - bodyHeight());
      state.top = Math.max(0, Math.min(max, state.top + m.wheel * 3));
      return render();
    }
    if (scrollbar.mouse(m, m.y - 2, m.x === W && !state.menu)) return;
    if (!m.press || m.drag || (m.button !== 0 && m.button !== 2)) return;
    const btn = m.button, { x, y } = m;
    if (state.prompt) { state.prompt = null; return render(); }
    if (state.menu) {
      // Click in the menu: action; elsewhere: close the menu (a right-click reopens it on the new row).
      const k = y - 2 - state.menu.at;
      if (btn === 0 && k >= 0 && k < menuRows()) {
        const item = state.menu.items[state.menu.off + k];
        return item ? runAction(item[1]) : undefined;
      }
      closeMenu();
      if (btn === 0) return render();
    }
    if (y === 1) {
      if (btn !== 0) return render();
      // Each icon and the cell after it: a Nerd Font glyph often spills over the next cell.
      if (x >= W - 2) return openSettings();
      if (x >= W - 4 && showUpdate()) return runAction(ctx.finish ? finishUpdate : askUpdate);
      return render();
    }
    if (y > bodyHeight() + 1) { if (state.filter) { state.filter.active = true; render(); } return; }
    const i = state.top + y - 2;
    if (i >= state.rows.length) return btn === 2 ? openMenu(null) : render();
    if (btn === 2) return openMenu(i);
    if (state.filter) state.filter.active = false;
    const r = state.rows[i];
    const now = Date.now(), double = state.lastClick && state.lastClick.i === i && now - state.lastClick.t < 400;
    state.lastClick = double ? null : { i, t: now };
    select(i);
    if (r.dir) toggle(r);
    else if (double) return openFile(r);
    else if (settings.singleClick && actions.canPreview(absOf(r.rel))) {
      return runAction(() => actions.preview(absOf(r.rel), ctx.ROOT, PANE, 0, false, true, { view: state.changed ? 'diff' : 'file' }));
    }
    render();
  }

  return { key, mouse };
};
