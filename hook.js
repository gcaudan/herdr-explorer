// Opens the explorer to the left of a tab.
//   node hook.js           tab.created event (creating a space also emits it for its first tab)
//   node hook.js --toggle  key binding action: closes the current tab's explorer, or opens it
//   node hook.js --all     adds it to existing tabs that have a single pane
'use strict';
const { execFileSync } = require('child_process');

const { PLUGIN, HERDR, result, readWidth } = require('./exec');
const LABEL = 'Explorer';
if (!HERDR) process.exit(0);

const herdr = (...args) => result(execFileSync(HERDR, args, { encoding: 'utf8' }));
const workspacePanes = workspaceId => herdr('pane', 'list', '--workspace', workspaceId).panes;
const tabPanes = (workspaceId, tabId) => workspacePanes(workspaceId).filter(p => p.tab_id === tabId);

function focusedTab() {
  const ws = herdr('workspace', 'list').workspaces.find(w => w.focused);
  return ws && ws.active_tab_id;
}

function open(panes, tabId, root) {
  // The top-left pane: the explorer takes its place and pushes it to the right.
  const rects = herdr('pane', 'layout', '--pane', panes[0].pane_id).layout.panes;
  const corner = rects.find(r => r.rect.x === 0 && r.rect.y === 0);
  const target = panes.find(p => corner && p.pane_id === corner.pane_id) || panes[0];
  const before = focusedTab();

  const opened = herdr('plugin', 'pane', 'open', '--plugin', PLUGIN, '--entrypoint', 'tree',
    '--placement', 'split', '--target-pane', target.pane_id, '--direction', 'right',
    '--env', `EXPLORER_ROOT=${root || target.cwd}`, '--no-focus');
  const explorer = opened.plugin_pane.pane.pane_id;

  // The split starts half-and-half of the target pane's space: set it right away to the wanted width (including
  // the 2-column border), without waiting for the explorer to start, which then corrects any rounding.
  // The ratio belongs to the left slot, which the explorer takes over on the swap.
  if (corner) {
    const delta = Math.min(0.9, Math.max(0.1, (readWidth() + 2) / corner.rect.width)) - 0.5;
    try {
      herdr('pane', 'resize', '--pane', target.pane_id, '--direction', delta > 0 ? 'right' : 'left',
        '--amount', Math.abs(delta).toFixed(6));
    } catch { }
  }

  // herdr only splits to the right or downwards: swap to move the explorer to the left.
  // The swap leaves focus on the left, so give it back to the terminal, which displays this tab;
  // then return to the tab the user was viewing.
  herdr('pane', 'swap', '--source-pane', explorer, '--target-pane', target.pane_id);
  herdr('pane', 'focus', '--direction', 'right', '--pane', explorer);
  if (before && before !== tabId) herdr('tab', 'focus', before);
}

const ctx = JSON.parse(process.env.HERDR_PLUGIN_CONTEXT_JSON || '{}');

if (process.argv.includes('--toggle')) {
  if (!ctx.tab_id) process.exit(0);
  const panes = tabPanes(ctx.workspace_id, ctx.tab_id);
  const explorer = panes.find(p => p.label === LABEL);
  if (explorer) herdr('pane', 'close', explorer.pane_id);
  else if (panes.length) open(panes, ctx.tab_id, ctx.workspace_cwd);
} else if (process.argv.includes('--all')) {
  for (const ws of herdr('workspace', 'list').workspaces) {
    const tabs = new Map();
    for (const p of workspacePanes(ws.workspace_id)) tabs.set(p.tab_id, [...tabs.get(p.tab_id) || [], p]);
    for (const [tab, panes] of tabs) {
      if (panes.length === 1 && panes[0].label !== LABEL) open(panes, tab, null);
    }
  }
} else {
  const ev = JSON.parse(process.env.HERDR_PLUGIN_EVENT_JSON || '{}');
  const tab = ev.data && ev.data.tab;
  if (tab) {
    const panes = tabPanes(tab.workspace_id, tab.tab_id);
    const root = ctx.workspace_id === tab.workspace_id ? ctx.workspace_cwd : null;
    if (panes.length === 1 && panes[0].label !== LABEL) open(panes, tab.tab_id, root);
  }
}
