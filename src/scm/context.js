// What the modules of the source control pane share: its folder, settings and constants, the state S, the last frame
// drawn, and the functions of the modules above that the ones below call back (drawing, the selection), which
// scm.js sets at startup.
'use strict';
const path = require('path');
const ui = require('../core/ui');
const icons = require('../core/icons');
const { readSettings } = require('../core/exec');

const ROOT = path.resolve(process.env.EXPLORER_ROOT || process.argv[2] || process.cwd());
require('../launch').home(); // the pane's folder for new spaces, tabs and panes made from it
const { C, fg, bg } = ui;
const BOLD = '\x1b[1m';
let DIM;
ui.onTheme(() => { DIM = fg(C.dim); });
const G = ui.G;
const settings = readSettings();
ui.useGlyphs(settings.glyphs);
const theme = icons.load(settings.icons); // the explorer's icon theme
const SPLIT = 100;      // columns from which the diff opens on the right of the list
const MAX_PROMPT = 60000; // characters of diff sent to Claude
const NET_TIMEOUT = 120000;
const VIEWS = [['changes', 'Changes'], ['history', 'History'], ['stashes', 'Stashes'], ['refs', 'Refs']];

// Share of the width taken by the list beside the diff, per view ({ changes: 0.3, history: 0.6 }); a view left out
// has the default width.
function shares(saved) {
  const out = {};
  if (saved && typeof saved === 'object') {
    for (const [id] of VIEWS) if (typeof saved[id] === 'number' && saved[id] > 0 && saved[id] < 1) out[id] = saved[id];
  }
  return out;
}

const S = {
  view: process.env.EXPLORER_HISTORY ? 'history' : 'changes', repo: null, // repo: null until loaded, false outside a repository
  tree: settings.scmTree === true, collapsed: new Set(), cursor: 0, top: 0, selKey: null,
  field: new ui.Field(), editing: false, amend: false, generating: false,
  diff: null,   // { key, title, parsed, top, mode, focus }
  confirm: null, // a question in a card (ui.confirm)
  inline: null, // a name typed in place: { kind, field, right, keys, done(value, alt) }
  picker: null, // branch picker: { mode, field, typing, sel, top, items }; mode 'merge' or 'rebase' picks the branch to
                // merge or rebase onto, else it switches
  menu: null,   // actions of the selected row and of the repository (ui.Menu) while open
  flash: null, busy: null, lastClick: null,
  help: false,  // key help overlay (?)
  pulse: null,  // { keys, until }: rows just staged or unstaged, briefly highlighted
  netOp: null,  // fetch, pull or push running: its header button spins
  // Share of the width taken by the list beside the diff, per view, set by dragging the separator or with < and >.
  listShare: shares(settings.scmListShare),
  resizing: false, // the separator is being dragged
  compareBase: null, // Refs: the branch the others are compared with, null for the current one
  allBranches: settings.scmGraphAll !== false, // History: every branch and remote branch, or the current one
  // History: the commits of one file only (its absolute path), from the explorer's or the preview's File history.
  histFile: process.env.EXPLORER_HISTORY || null,
  histNames: [],   // the names the file had, from git log --follow, its current one first
  history: [], stashes: [], refs: null,
};

// The last frame drawn (render.js): its clickable areas { y, x0, x1, fn }, the rows of the current view, and the
// list's first row, height and width.
const frame = { hits: [], rows: [], listY: 0, listH: 0, listW: 0 };

// Set by scm.js: the screen (ui.screen), draw (render.js), select and follow (input.js), key (input.js).
const late = { paint: null, draw: null, select: null, follow: null, key: null };
const draw = () => late.draw();
const select = i => late.select(i);
const follow = () => late.follow();

const wide = () => ui.size().W >= SPLIT;
const base = p => p.slice(p.lastIndexOf('/') + 1);
const dirOf = p => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');
const files = n => `${n} file${n > 1 ? 's' : ''}`;

function flash(text, error = false) {
  S.flash = { text, error };
  draw();
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { S.flash = null; draw(); }, ui.messageTime(error ? 'error' : ui.messageKind(text)));
}

// A question in a card over the pane (ui.confirm): q: { title, name, where, note, action, tone }.
function confirm(q, yes) { S.confirm = { ...q, yes, focus: 'no' }; draw(); }

function quit() { process.exit(0); }

module.exports = {
  ROOT, C, fg, bg, BOLD, G, settings, theme, SPLIT, MAX_PROMPT, NET_TIMEOUT, VIEWS, shares, S, frame, late, draw,
  select, follow, wide, base, dirOf, files, flash, confirm, quit,
};
