// Opens the explorer to the left (or the right, depending on the settings) of a tab.
//   node hook.js           tab.created event (creating a space also emits it for its first tab), unless the
//                          autoOpen setting is off
//   node hook.js --toggle  key binding action: closes the current tab's explorer, or opens it
//   node hook.js --all     adds it to existing tabs that have a single pane
//   node hook.js --restore startup hook: reopens the explorer and preview panes that herdr restored as shells
//   node hook.js --search  key binding action: Go to file popup of the current tab's folder
//   node hook.js --scm     key binding action: source control of the current tab's folder
'use strict';
const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const { PLUGIN, HERDR, result, readWidth, readSettings, PANES_DIR, paneFile, readPaneState } = require('./exec');
const LABEL = 'Explorer';
if (!HERDR) process.exit(0);

const herdr = (...args) => result(execFileSync(HERDR, args, { encoding: 'utf8' }));
const workspacePanes = workspaceId => herdr('pane', 'list', '--workspace', workspaceId).panes;
const tabPanes = (workspaceId, tabId) => workspacePanes(workspaceId).filter(p => p.tab_id === tabId);

function focusedTab() {
  const ws = herdr('workspace', 'list').workspaces.find(w => w.focused);
  return ws && ws.active_tab_id;
}

const RIGHT = readSettings().side === 'right';

// tree: the tree root, when Set as root had moved it away from the space folder before a herdr restart.
// Returns the explorer's pane.
function open(panes, tabId, root, tree) {
  // The top-left pane: the explorer takes its place and pushes it to the right. Docked on the right: the top-right
  // pane, which the explorer splits.
  const layout = herdr('pane', 'layout', '--pane', panes[0].pane_id).layout;
  const corner = layout.panes.find(r => r.rect.y === 0
    && (RIGHT ? r.rect.x + r.rect.width === layout.area.width : r.rect.x === 0));
  const target = panes.find(p => corner && p.pane_id === corner.pane_id) || panes[0];
  const before = focusedTab();

  const opened = herdr('plugin', 'pane', 'open', '--plugin', PLUGIN, '--entrypoint', 'tree',
    '--placement', 'split', '--target-pane', target.pane_id, '--direction', 'right',
    '--env', `EXPLORER_ROOT=${root || target.cwd}`, ...(tree ? ['--env', `EXPLORER_TREE=${tree}`] : []), '--no-focus');
  const explorer = opened.plugin_pane.pane.pane_id;

  // The split starts half-and-half of the target pane's space: set it right away to the wanted width (including
  // the 2-column border), without waiting for the explorer to start, which then corrects any rounding.
  // The ratio belongs to the left slot, which the explorer takes over on the swap.
  if (corner) {
    const part = Math.min(0.9, Math.max(0.1, (readWidth() + 2) / corner.rect.width));
    const delta = (RIGHT ? 1 - part : part) - 0.5;
    try {
      herdr('pane', 'resize', '--pane', target.pane_id, '--direction', delta > 0 ? 'right' : 'left',
        '--amount', Math.abs(delta).toFixed(6));
    } catch { }
  }

  // herdr only splits to the right or downwards: swap to move the explorer to the left.
  // The swap leaves focus on the left, so give it back to the terminal, which displays this tab;
  // then return to the tab the user was viewing.
  if (!RIGHT) herdr('pane', 'swap', '--source-pane', explorer, '--target-pane', target.pane_id);
  herdr('pane', 'focus', '--direction', RIGHT ? 'left' : 'right', '--pane', explorer);
  if (before && before !== tabId) herdr('tab', 'focus', before);
  return explorer;
}

const ctx = JSON.parse(process.env.HERDR_PLUGIN_CONTEXT_JSON || '{}');

if (process.argv.includes('--toggle')) {
  if (!ctx.tab_id) process.exit(0);
  const panes = tabPanes(ctx.workspace_id, ctx.tab_id);
  const explorer = panes.find(p => p.label === LABEL);
  if (explorer) herdr('pane', 'close', explorer.pane_id);
  else if (panes.length) open(panes, ctx.tab_id, ctx.workspace_cwd);
} else if (process.argv.includes('--search') || process.argv.includes('--scm')) {
  // From the tab's explorer when there is one, so that the search opens files in that tab's preview.
  const actions = require('./actions');
  const panes = ctx.tab_id ? tabPanes(ctx.workspace_id, ctx.tab_id) : [];
  const explorer = panes.find(p => p.label === LABEL);
  const pane = (explorer && explorer.pane_id) || process.env.HERDR_PANE_ID || ctx.pane_id || (panes[0] && panes[0].pane_id);
  const root = ctx.workspace_cwd || (panes[0] && panes[0].cwd);
  if (pane && root) (process.argv.includes('--scm') ? actions.scm(root, pane) : actions.search(root, pane, 'files'));
} else if (process.argv.includes('--all')) {
  for (const ws of herdr('workspace', 'list').workspaces) {
    const tabs = new Map();
    for (const p of workspacePanes(ws.workspace_id)) tabs.set(p.tab_id, [...tabs.get(p.tab_id) || [], p]);
    for (const [tab, panes] of tabs) {
      if (panes.length === 1 && panes[0].label !== LABEL) open(panes, tab, null);
    }
  }
} else if (process.argv.includes('--restore')) {
  // herdr restores the plugin's panes as plain shells, its session keeping only their label and folder: close
  // them, then reopen the explorer and the preview in the tabs that had them, at what they saved under the same
  // pane id (the preview's file only when it still exists).
  const actions = require('./actions');
  const OWN = new Set(['Explorer', 'Preview', 'Source Control', 'Changes', 'Editor']);
  const exists = (p, dir) => { try { return p && fs.statSync(p).isDirectory() === dir ? p : null; } catch { return null; } };
  const running = p => herdr('pane', 'process-info', '--pane', p.pane_id).process_info.foreground_processes
    .some(f => /^node(\.exe)?$/i.test(path.basename((f.argv && f.argv[0]) || '')));
  // A newer release, looked for alongside (at most once a day).
  require('./update').check().catch(() => { });
  (async () => {
    for (const ws of herdr('workspace', 'list').workspaces) {
      const tabs = new Map();
      for (const p of workspacePanes(ws.workspace_id)) tabs.set(p.tab_id, [...tabs.get(p.tab_id) || [], p]);
      for (const [tab, panes] of tabs) {
        const stale = panes.filter(p => OWN.has(p.label) && !running(p));
        if (!stale.length) continue;
        for (const p of stale) herdr('pane', 'close', p.pane_id);
        const rest = panes.filter(p => !stale.includes(p));
        if (!rest.length) continue;
        let anchor = rest[0].pane_id;
        const explorer = stale.find(p => p.label === LABEL);
        if (explorer) {
          const saved = readPaneState(explorer.pane_id) || {};
          const home = exists(saved.home, true);
          anchor = open(rest, tab, home, home && exists(saved.root, true));
        }
        const preview = stale.find(p => p.label === 'Preview');
        const shown = preview && readPaneState(preview.pane_id);
        if (shown && exists(shown.file, false)) await actions.preview(shown.file, shown.root, anchor, shown.line);
      }
    }
    // The state of panes that no longer exist.
    const live = new Set(herdr('workspace', 'list').workspaces
      .flatMap(ws => workspacePanes(ws.workspace_id).map(p => paneFile(p.pane_id))));
    try {
      for (const f of fs.readdirSync(PANES_DIR)) if (!live.has(path.join(PANES_DIR, f))) fs.unlinkSync(path.join(PANES_DIR, f));
    } catch { }
  })();
} else {
  // Also here, for a herdr session running for days: the check itself happens once a day at most.
  require('./update').check().catch(() => { });
  const ev = JSON.parse(process.env.HERDR_PLUGIN_EVENT_JSON || '{}');
  const tab = ev.data && ev.data.tab;
  if (tab && readSettings().autoOpen) {
    const panes = tabPanes(tab.workspace_id, tab.tab_id);
    const root = ctx.workspace_id === tab.workspace_id ? ctx.workspace_cwd : null;
    if (panes.length === 1 && panes[0].label !== LABEL) open(panes, tab.tab_id, root);
  }
}
