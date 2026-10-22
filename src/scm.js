// Source Control of the space's repository, in a full-tab pane opened from the explorer, VS Code-style:
// merge conflicts, staged and unstaged changes as a list or a folder tree; stage, unstage, discard, commit (with a
// message written by Claude on request); branch picker, fetch, pull, push; history, stashes and refs views; a menu (m)
// for merge, rebase, cherry-pick, tags, force push with lease, undo commit and reset --hard. Diffs open in a split on
// the right (diff.js renderer). q or Esc closes the pane.
//
// The pane's modules are in scm/: parse.js (git's output turned into data), context.js (state S, settings, what the
// modules share), state.js (git and the loaders), ops.js (what the pane does to the repository), render.js (drawing),
// input.js (keys and mouse). This file starts the pane.
'use strict';
const ui = require('./core/ui');
const { S, late, wide } = require('./scm/context');
const { schedule, refresh, filterPicker } = require('./scm/state');
const { draw } = require('./scm/render');
const { select, follow, key, mouse } = require('./scm/input');

// --- startup

Object.assign(late, { draw, select, follow, key });
late.paint = ui.screen({
  key, mouse,
  paste(text) {
    const f = S.inline ? S.inline.field : S.picker ? S.picker.field : S.editing ? S.field : null;
    if (f) { f.insert(text); if (S.picker && f === S.picker.field) { S.picker.typing = true; filterPicker(); } draw(); }
  },
  focus(on) { if (on) schedule(); },
  // Below SPLIT columns, a diff the selection opened by itself would hide the list.
  resize() { if (S.diff && S.diff.auto && !wide()) S.diff = null; draw(); follow(); },
  theme() { if (S.diff) S.diff.cache = ''; draw(); },
}, { motion: true });
draw();
refresh();
setInterval(schedule, 5000); // the working tree is not watched: edits show up within 5 s
