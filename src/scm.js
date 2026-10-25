// Source Control of the space's repository, in a review tab opened from the explorer, VS Code-style:
// merge conflicts, staged and unstaged changes as a list or a folder tree; stage, unstage, discard, commit (with a
// message written by Claude on request); branch picker, fetch, pull, push; history, stashes and refs views; a menu (m)
// for merge, rebase, cherry-pick, tags, force push with lease, undo commit and reset --hard. In a review (the
// default, EXPLORER_REVIEW), a file's changes open in the preview on the right of the pane, the other diffs over the
// list (diff.js renderer); otherwise, from 100 columns, in a split on the right. q or Esc closes the pane.
//
// The pane's modules are in scm/: parse.js (git's output turned into data), context.js (state S, settings, what the
// modules share), state.js (git and the loaders), ops.js (what the pane does to the repository), render.js (drawing),
// input.js (keys and mouse). This file starts the pane.
'use strict';
const ui = require('./core/ui');
const actions = require('./core/actions');
const { S, late, wide, settings, REVIEW } = require('./scm/context');
const { readSettings, SETTINGS_FILE } = require('./core/exec');
const { schedule, refresh, filterPicker } = require('./scm/state');
const { draw } = require('./scm/render');
const { select, follow, key, mouse, keepShare } = require('./scm/input');

// --- startup

Object.assign(late, { draw, select, follow, key });
late.paint = ui.screen({
  key, mouse,
  paste(text) {
    const f = S.inline ? S.inline.field : S.picker ? S.picker.field : S.editing ? S.field : null;
    if (f) { f.insert(text); if (S.picker && f === S.picker.field) { S.picker.typing = true; filterPicker(); } draw(); }
  },
  focus(on) { if (on) schedule(); else actions.unzoom(process.env.HERDR_PANE_ID).catch(() => { }); },
  // Below SPLIT columns, a diff the selection opened by itself would hide the list.
  resize() { if (S.diff && S.diff.auto && !wide()) S.diff = null; draw(); follow(); keepShare(); },
  theme() { if (S.diff) S.diff.cache = ''; draw(); },
}, { motion: true });
draw();
refresh();
// The review recorded for the tab reviewed, which leads back here while this pane runs (hook.js --focused), in a tab
// named "(SC 2)", closed with the preview it holds at any exit.
if (REVIEW) actions.holdTab('review', REVIEW, 'SC');
setInterval(schedule, 5000); // the working tree is not watched: edits show up within 5 s
// Wrap lines and the diff layout chosen in another pane (the preview, the Changes pane): shown at once.
if (SETTINGS_FILE) require('fs').watchFile(SETTINGS_FILE, { interval: 1000 }, () => {
  const wrap = !!readSettings().wrap;
  if (wrap !== !!settings.wrap) { settings.wrap = wrap; if (S.diff) S.diff.left = 0; }
  if (S.diff) draw();
});
