// Keys and mouse of the source control pane, and the selection, which the diff beside the list follows.
'use strict';
const ui = require('../core/ui');
const diffs = require('../diff');
const { GIT, gitEnv } = require('../core/exec');
const { ROOT, SPLIT, VIEWS, S, frame, draw, wide, flash, confirm, quit, settings } = require('./context');
const {
  exec, english, errorOf, schedule, filterPicker, openPicker, compare, shareCompare, showDiff, closeDiff, comparable, diffable, showing, allFiles,
  setView, toggleAllBranches, toggleTree, toggle,
} = require('./state');
const {
  stage, unstage, stageAll, unstageAll, discard, commit, abort, toggleAmend, generate, pickBranch, newBranch, deleteBranch,
  fetch, pull, push, newStash, stashAction, integrate, cherryPick, newTag, setBase, openRow, search, fileAction, openMenu,
} = require('./ops');
const { listBar, diffBar, resizeList, openDiffView, switchLayout, toggleWrap } = require('./render');

function select(i) {
  const rows = frame.rows;
  S.cursor = Math.max(0, Math.min(i, rows.length - 1));
  S.selKey = rows[S.cursor] ? rows[S.cursor].key : null;
  if (S.cursor < S.top) S.top = S.cursor;
  if (S.cursor >= S.top + frame.listH) S.top = S.cursor - frame.listH + 1;
  follow();
}

// --- branch picker and names typed in place

function pickerKey(k) {
  const p = S.picker;
  const b = p.items[p.sel];
  const move = i => { p.sel = Math.max(0, Math.min(i, p.items.length - 1)); draw(); };
  if (k === 'up' || (!p.typing && k === 'k')) return move(p.sel - 1);
  if (k === 'down' || (!p.typing && k === 'j')) return move(p.sel + 1);
  if (k === 'pageup') return move(p.sel - p.box.body);
  if (k === 'pagedown') return move(p.sel + p.box.body);
  if (k === 'enter') return pickBranch(b);
  if (k === 'escape') {
    if (p.field.value || p.typing) { p.field.set(''); p.typing = false; filterPicker(); return draw(); }
    S.picker = null; return draw();
  }
  if (!p.typing) {
    if (k === 'q') { S.picker = null; return draw(); }
    if (k === 'n' && !p.mode) return newBranch('');
    if (k === 'D' && !p.mode) return b && !b.create && deleteBranch(b);
    if (k === '/') { p.typing = true; return draw(); }
  }
  if (p.field.key(k)) { p.typing = true; p.sel = p.field.value && !p.mode ? 1 : p.sel; filterPicker(); if (p.items.length < 2) p.sel = 0; draw(); }
}

function inlineKey(k) {
  const it = S.inline;
  if (k === 'escape' || k === 'ctrl+c') { S.inline = null; return draw(); }
  if (k === 'enter' || k === 'alt+enter' || k === 'ctrl+o') { S.inline = null; draw(); return it.done(it.field.value, k !== 'enter'); }
  it.field.key(k);
  draw();
}

// --- diffs

// Keys of a focused comparison: the selection moves over the commits, Enter shows the selected one.
function compareKey(k, d, page) {
  const at = (d.rows || []).map((r, i) => (r.commit ? i : -1)).filter(i => i >= 0);
  if (!at.length) return false;
  const pos = Math.max(0, at.indexOf(d.sel));
  const move = i => {
    d.sel = at[Math.max(0, Math.min(i, at.length - 1))];
    const body = ui.size().H - 3;
    if (d.sel < d.top + 1) d.top = Math.max(0, d.sel - 1);
    if (d.sel >= d.top + body) d.top = d.sel - body + 1;
    draw();
    return true;
  };
  if (k === 'up' || k === 'k') return move(d.sel < 0 ? 0 : pos - 1);
  if (k === 'down' || k === 'j') return move(d.sel < 0 ? 0 : pos + 1);
  if (k === 'pageup') return move(pos - page);
  if (k === 'pagedown') return move(pos + page);
  if (k === 'home' || k === 'g') return move(0);
  if (k === 'end' || k === 'G') return move(at.length - 1);
  if (k === 'enter' && d.sel >= 0) {
    const c = d.rows[d.sel].commit;
    showDiff(`commit:${c.hash}`, `${c.short} ${c.subject}`,
      ['show', ...diffs.DIFF_OPTS, '--stat', '-p', '--diff-merges=first-parent', '--format=fuller', c.hash]);
    S.diff.back = d;
    S.diff.focus = true;
    return true;
  }
  return false;
}

// From SPLIT columns, the diff shows the selected file, commit, stash or tag (shortly after, when moving fast).
// A branch other than the base shows its comparison with the base.
function follow() {
  clearTimeout(follow.timer);
  follow.timer = setTimeout(() => {
    if (!S.repo || !wide()) return;
    const row = frame.rows[S.cursor];
    if (diffable(row)) {
      // showDiff sets S.diff before it waits for git.
      if (!showing(row)) { openRow(row, true); if (S.diff && S.diff.key === row.key) S.diff.auto = true; }
    } else if (S.diff) closeDiff();
  }, 120);
}

function diffKey(k) {
  const d = S.diff, page = ui.size().H - 4;
  const scroll = n => { d.top += n; draw(); };
  // A commit shown from a comparison: q and Escape go back to the comparison.
  if (d.back && (k === 'q' || k === 'escape')) { S.diff = d.back; S.diff.focus = true; return draw(); }
  if (d.compare && compareKey(k, d, page)) return;
  if (k === 'q' || k === 'escape' || k === 'tab' || k === 'shift+tab') {
    // Beside the list, the diff stays: the list takes the keys back.
    if (wide()) { d.focus = false; return draw(); }
    if (k === 'q' || k === 'escape') return closeDiff();
  }
  if (k === 'up' || k === 'k') return scroll(-1);
  if (k === 'down' || k === 'j' || k === 'enter') return scroll(1);
  if (k === 'pageup') return scroll(-page);
  if (k === 'pagedown' || k === 'space') return scroll(page);
  if (k === 'ctrl+u') return scroll(-Math.ceil(page / 2));
  if (k === 'ctrl+d') return scroll(Math.ceil(page / 2));
  if (k === 'home' || k === 'g') { d.top = 0; return draw(); }
  if (k === 'end' || k === 'G') { d.top = Infinity; return draw(); }
  if (k === 'v') return switchLayout();
  if (k === 'w') return toggleWrap();
  if (k === ',') return openDiffView();
  // Without wrap, the text 8 columns sideways, up to the longest line.
  if (k === 'left' || k === 'h') { if (!settings.wrap && d.left) { d.left = Math.max(0, d.left - 8); draw(); } return; }
  if (k === 'right' || k === 'l') { if (!settings.wrap && (d.left || 0) + 8 < diffs.widestOf(d.parsed)) { d.left = (d.left || 0) + 8; draw(); } return; }
  if (k === 'n' || k === 'N') {
    const r = d.rows || [];
    const i = k === 'n' ? r.findIndex((x, j) => j > d.top && x.hunk) : r.map((x, j) => (j < d.top && x.hunk ? j : -1)).filter(j => j >= 0).pop();
    if (i !== undefined && i >= 0) { d.top = i; draw(); }
  }
}

// --- views and keys

// A choice of the menu, by key or click: the menu closes, then the item runs; the diff's options show again with
// their new values (keep).
function menuChoice(r) {
  const view = S.menu && S.menu.view, sel = S.menu && S.menu.sel;
  if (r === 'close' || (r && r.run)) S.menu = null;
  draw();
  if (r && r.run) r.run();
  if (r && r.keep && view) openDiffView(sel);
}

function fieldKey(k) {
  if (k === 'enter') return commit();
  if (k === 'escape' || k === 'tab') { S.editing = false; return draw(); }
  if (k === 'ctrl+g') return generate();
  if (k === 'alt+a') return toggleAmend();
  if (k === 'ctrl+c') return quit();
  if (S.generating) return;
  S.field.key(k);
  draw();
}

async function key(k) {
  if (S.confirm) {
    const q = S.confirm, answer = ui.confirmKey(k, q);
    if (answer !== 'move') S.confirm = null;
    draw();
    if (answer === 'yes') return q.yes();
    return;
  }
  if (S.inline) return inlineKey(k);
  if (S.menu) return menuChoice(S.menu.key(k, ui.size().H));
  if (S.picker) return pickerKey(k);
  if (S.editing && S.repo) return fieldKey(k);
  if (k === 'ctrl+c') return quit();
  // The key help (ui.helpKey): scrolls, finds with /, closes with esc, q, ? or f1.
  if (S.help) { if (!ui.helpKey(k, S.help)) S.help = null; return draw(); }
  if ((k === '?' || k === 'f1') && S.repo) { S.help = { top: 0, find: null }; return draw(); }
  if (S.diff && (S.diff.focus || ui.size().W < SPLIT)) return diffKey(k);
  if (!S.repo) {
    if (k === 'q' || k === 'escape') return quit();
    if (k === 'i' && S.repo === false && !S.noGit) {
      return confirm({ title: 'Create a repository?', name: ROOT, where: 'git init', action: 'Create', tone: 'go' }, async () => {
        const res = await exec(GIT, ['init'], { cwd: ROOT, env: english(await gitEnv(ROOT)) });
        flash(res.code === 0 ? 'Repository created' : `git init failed: ${errorOf(res)}`, res.code !== 0);
        schedule();
      });
    }
    if (k === 'r') schedule();
    return;
  }
  const rows = frame.rows;
  const row = rows[S.cursor];
  const page = Math.max(1, frame.listH - 1);
  switch (k) {
    case 'q': return quit();
    case 'escape': return quit();
    case 'up': case 'k': select(S.cursor - 1); break;
    case 'down': case 'j': select(S.cursor + 1); break;
    case 'pageup': select(S.cursor - page); break;
    case 'pagedown': select(S.cursor + page); break;
    case 'home': case 'g': select(0); break;
    case 'end': case 'G': select(rows.length - 1); break;
    case 'left': case 'h':
      if (row && row.open !== undefined && row.open) return toggle(row, false);
      for (let i = S.cursor - 1; i >= 0; i--) if (rows[i].open !== undefined && (rows[i].depth || 0) < (row.depth || 0)) { select(i); break; }
      break;
    case 'right': case 'l':
      if (row && row.open === false) return toggle(row, true);
      break;
    case 'tab': if (S.diff) S.diff.focus = true; break;
    case 'enter':
      // Beside the list, the diff already shows the selection: Enter goes into it.
      if (wide() && diffable(row) && row.type !== 'branch') {
        if (!S.diff || S.diff.key !== row.key) await openRow(row);
        if (S.diff && S.diff.key === row.key) { S.diff.focus = true; S.diff.auto = false; }
        break;
      }
      return openRow(row);
    case '1': case '2': case '3': case '4': return setView(VIEWS[+k - 1][0]);
    case 'r': flash('Refreshed'); return schedule();
    case 'f': return fetch();
    case 'P': return push();
    case 'L': return pull();
    case 'ctrl+u': select(S.cursor - Math.ceil(page / 2)); break;
    case 'ctrl+d': select(S.cursor + Math.ceil(page / 2)); break;
    case 'ctrl+p': return search('files');
    case 'ctrl+f': return search('content');
    case 'b': return openPicker();
    case 'm': case 'menu': case 'shift+f10': return openMenu();
    case 'T': return newTag(null);
    case '<': case '>': if (wide()) resizeList(frame.listW + (k === '<' ? -4 : 4)); break;
    case 'shift+left': case 'shift+right': if (wide()) resizeList(frame.listW + (k === 'shift+left' ? -4 : 4)); break;
    case 'c': if (S.view !== 'changes') setView('changes'); S.editing = true; break;
    case 'ctrl+g': if (S.view !== 'changes') setView('changes'); return generate();
    case 'alt+a': return toggleAmend();
    default: return viewKey(k, row);
  }
  draw();
}

function viewKey(k, row) {
  if (S.view === 'changes') {
    if (k === 's' || k === 'space') return row && row.items && (row.sec === 'staged' ? flash('Already staged') : stage(row.items));
    if (k === 'u') return row && row.items && (row.sec === 'staged' ? unstage(row.items) : flash('Not staged'));
    if (k === 'a') return stageAll();
    if (k === 'A') return unstageAll();
    if (k === 'd') return row && row.items && discard(row.items, row.sec);
    if (k === 't') return toggleTree();
    if (k === 'X') return abort();
    if (['y', 'Y', 'e', 'E', 'o'].includes(k)) return fileAction(k, row);
  } else if (S.view === 'history') {
    if (k === 'a') return toggleAllBranches();
    if (k === 'x' && S.histFile) return allFiles();
    if (k === 't' && row && row.type === 'commit') return newTag(row.commit);
    if (k === 'C' && row && row.type === 'commit') return cherryPick(row.commit);
  } else if (S.view === 'stashes') {
    if (k === 'n') return newStash();
    if (row && row.type === 'stash') {
      if (k === 'a') return stashAction(row.stash, 'apply');
      if (k === 'o') return stashAction(row.stash, 'pop');
      if (k === 'd') return stashAction(row.stash, 'drop');
    }
  } else if (S.view === 'refs') {
    if (k === 'D' && row && row.type === 'branch') return deleteBranch(row.ref);
    if (k === '=' && row && row.type === 'branch') {
      if (!comparable(row)) return flash(`${row.ref.name} is the base of the comparisons: select another branch`);
      shareCompare(row);
      return compare(row).then(() => { if (S.diff) { S.diff.focus = true; draw(); } });
    }
    if (k === 'B' && row && row.type === 'branch') return setBase(row);
    if ((k === 'M' || k === 'R') && row && row.type === 'branch') return integrate(row.ref, k === 'M' ? 'merge' : 'rebase');
  }
}

function mouse(m) {
  const { W } = ui.size();
  const x = m.x - 1, y = m.y - 1;
  const { rows, hits, listY, listH, listW } = frame;
  if (S.confirm) {
    const q = S.confirm, answer = ui.confirmMouse(m, q);
    if (!answer) return;
    S.confirm = null;
    draw();
    if (answer === 'yes') q.yes();
    return;
  }
  if (S.help) { if (!ui.helpMouse(m, S.help)) S.help = null; return draw(); }
  if (S.menu) return menuChoice(S.menu.mouse(m, ui.size().H));
  if (m.wheel) {
    if (S.picker) { S.picker.sel = Math.max(0, Math.min(S.picker.sel + m.wheel, S.picker.items.length - 1)); return draw(); }
    if (S.diff && (W < SPLIT || x > listW)) { S.diff.top += m.wheel * 3; return draw(); }
    S.top = Math.max(0, Math.min(S.top + m.wheel * 3, rows.length - listH));
    return draw();
  }
  // The separator between the list and the diff: dragged to resize them, saved at the release.
  // A motion without the button means its release was not reported (outside the pane).
  if (S.resizing) {
    if (m.drag && m.button === 0) return resizeList(x, false);
    S.resizing = false;
    return resizeList(listW);
  }
  if (W >= SPLIT && listW < W && x === listW && y >= 1 && y < ui.size().H - 1 && m.press && !m.drag && m.button === 0 && !S.picker && !S.help) {
    S.resizing = true;
    return draw();
  }
  // The diff's scrollbar in the last column, the list's in the last column of the list.
  if (S.diff && !S.picker && diffBar.mouse(m, y - 2, x === W - 1)) return;
  if (listW && !S.picker && listBar.mouse(m, y - listY, x === listW - 1)) return;
  if (!m.press || m.drag || m.button !== 0) return;
  // A key of the status line acts as that key, also while a name is typed or the branch picker is open.
  const hint = (S.inline || S.picker) && y === ui.size().H - 1 && hits.find(h => h.y === y && x >= h.x0 && x < h.x1);
  if (hint) { hint.fn(); return draw(); }
  if (S.inline) { S.inline = null; return draw(); }
  if (S.picker) {
    const b = S.picker.box;
    const i = S.picker.top + y - b.y - 2;
    if (x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h) {
      if (y === b.y + 1) { S.picker.typing = true; return draw(); }
      if (i >= S.picker.top && i < S.picker.top + b.body && S.picker.items[i]) { S.picker.sel = i; return pickBranch(S.picker.items[i]); }
      return;
    }
    S.picker = null;
    return draw();
  }
  const hit = hits.find(h => h.y === y && x >= h.x0 && x < h.x1);
  const wasEditing = S.editing;
  // The message field, its buttons and the hints (which then act on the message) keep it being typed.
  if (!(hit && (y === 2 || y === 3 || y === ui.size().H - 1))) S.editing = false;
  if (hit) { hit.fn(); return draw(); }
  if (wasEditing) draw();
  if (S.diff && (W < SPLIT || x > listW)) { S.diff.focus = true; return draw(); }
  if (W >= SPLIT && x >= listW) return; // the diff side, empty
  if (y < listY || y >= listY + listH) return;
  const i = S.top + y - listY;
  if (i >= rows.length) return;
  if (S.diff) S.diff.focus = false;
  const now = Date.now(), double = S.lastClick && S.lastClick.i === i && now - S.lastClick.t < 400;
  S.lastClick = double ? null : { i, t: now };
  select(i);
  const row = rows[i];
  if (row.type === 'section' || row.type === 'folder') toggle(row);
  else if (double || row.type === 'action') openRow(row);
  draw();
}

module.exports = { select, follow, key, mouse };
