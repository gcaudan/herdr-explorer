// Opens the explorer to the left (or the right, depending on the settings) of a tab.
//   node hook.js           tab.created event (creating a space also emits it for its first tab), unless the
//                          autoOpen setting is off
//   node hook.js --toggle  key binding action: opens the current tab's explorer, focuses it when the focus is
//                          elsewhere, closes it when it has the focus
//   node hook.js --all     adds it to existing tabs that have a single pane
//   node hook.js --restore startup hook: reopens the explorer and preview panes that herdr restored as shells
//   node hook.js --search  key binding action: Go to file popup of the current tab's folder
//   node hook.js --scm     key binding action: source control of the current tab's folder
//   node hook.js --closed  tab.closed and workspace.closed events: removes the state of the tabs and panes gone
// Each run also saves the state folder for peek.js.
'use strict';
const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const { PLUGIN, HERDR, result, OWN_PANES: OWN, readWidth, readSettings, readPaneState, sweepState } = require('./core/exec');
const { sideSplit } = require('./core/actions');
const LABEL = 'Explorer';
if (!HERDR) process.exit(0);

// The state folder, saved in the config folder for peek.js, which runs outside the plugin (from an agent's pane) and
// can find the config folder only (herdr plugin config-dir).
const { HERDR_PLUGIN_STATE_DIR: STATE, HERDR_PLUGIN_CONFIG_DIR: CONFIG } = process.env;
if (STATE && CONFIG) {
  const file = path.join(CONFIG, 'state-dir');
  try { if (fs.readFileSync(file, 'utf8') !== STATE) throw 0; } catch {
    try { fs.mkdirSync(CONFIG, { recursive: true }); fs.writeFileSync(file, STATE); } catch { }
  }
}

const herdr = (...args) => result(execFileSync(HERDR, args, { encoding: 'utf8' }));
const workspacePanes = workspaceId => herdr('pane', 'list', '--workspace', workspaceId).panes;
const tabPanes = (workspaceId, tabId) => workspacePanes(workspaceId).filter(p => p.tab_id === tabId);

// What the plugin keeps for tabs and panes that no longer exist (notes, pane state, preview tabs), removed.
function sweep() {
  const panes = herdr('workspace', 'list').workspaces.flatMap(ws => workspacePanes(ws.workspace_id));
  sweepState([...new Set(panes.map(p => p.tab_id))], panes.map(p => p.pane_id));
}

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
  try { settlePreview(panes, tabId, explorer); } catch { }
  herdr('pane', 'focus', '--direction', RIGHT ? 'left' : 'right', '--pane', explorer);
  if (before && before !== tabId) herdr('tab', 'focus', before);
  return explorer;
}

// Opened in a tab that already has a preview, the explorer shares a split with the pane it split, on one side of the
// split whose other side is the preview: dragging the preview's border then resizes the explorer too, herdr keeping
// each split's ratio. The preview moved out of the tab and back beside the terminal (herdr does not move a pane
// within its tab), at the same width, leaves the explorer a split of its own, at the side of the tab.
function settlePreview(panes, tabId, explorer) {
  const preview = panes.find(p => p.label === 'Preview');
  const terminals = panes.filter(p => !p.label);
  if (!preview || terminals.length !== 1) return;
  const layout = herdr('pane', 'layout', '--pane', explorer).layout;
  const found = sideSplit(layout, explorer);
  if (!found || found.split.rect.width === layout.area.width) return;
  const p = layout.panes.find(r => r.pane_id === preview.pane_id);
  const t = layout.panes.find(r => r.pane_id === terminals[0].pane_id);
  if (!p || !t) return;
  herdr('pane', 'move', preview.pane_id, '--new-tab', '--no-focus');
  herdr('pane', 'move', preview.pane_id, '--tab', tabId, '--target-pane', t.pane_id, '--split', 'right',
    '--ratio', (t.rect.width / (t.rect.width + p.rect.width)).toFixed(4), '--no-focus');
}

const ctx = JSON.parse(process.env.HERDR_PLUGIN_CONTEXT_JSON || '{}');

if (process.argv.includes('--toggle')) {
  if (!ctx.tab_id) process.exit(0);
  const panes = tabPanes(ctx.workspace_id, ctx.tab_id);
  const explorer = panes.find(p => p.label === LABEL);
  if (explorer && ctx.focused_pane_id && ctx.focused_pane_id !== explorer.pane_id) {
    herdr('plugin', 'pane', 'focus', explorer.pane_id);
  } else if (explorer) {
    // The preview closes with the explorer (see close in explorer.js).
    for (const p of panes) if (p.label === 'Preview') herdr('pane', 'close', p.pane_id);
    herdr('pane', 'close', explorer.pane_id);
  } else if (panes.length) open(panes, ctx.tab_id, ctx.workspace_cwd);
} else if (process.argv.includes('--search') || process.argv.includes('--scm')) {
  // From the tab's explorer when there is one, so that the search opens files in that tab's preview.
  const actions = require('./core/actions');
  const panes = ctx.tab_id ? tabPanes(ctx.workspace_id, ctx.tab_id) : [];
  const explorer = panes.find(p => p.label === LABEL);
  const pane = (explorer && explorer.pane_id) || process.env.HERDR_PANE_ID || ctx.pane_id || (panes[0] && panes[0].pane_id);
  // The explorer's root: workspace_cwd is the folder of the focused pane, the plugin's own when it is one of its panes.
  const saved = explorer && readPaneState(explorer.pane_id);
  // The focused terminal first, else the tab's first one.
  const focused = panes.find(p => p.pane_id === (process.env.HERDR_PANE_ID || ctx.pane_id));
  const terminal = focused && !OWN.has(focused.label) ? focused : panes.find(p => !OWN.has(p.label));
  const root = (saved && saved.root) || (terminal && terminal.cwd) || ctx.workspace_cwd;
  if (pane && root) (process.argv.includes('--scm') ? actions.scm(root, pane) : actions.search(root, pane, 'files'));
} else if (process.argv.includes('--closed')) {
  sweep();
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
  const actions = require('./core/actions');
  const exists = (p, dir) => { try { return p && fs.statSync(p).isDirectory() === dir ? p : null; } catch { return null; } };
  const running = p => herdr('pane', 'process-info', '--pane', p.pane_id).process_info.foreground_processes
    .some(f => /^node(\.exe)?$/i.test(path.basename((f.argv && f.argv[0]) || '')));
  // A newer release, looked for alongside (at most once a day).
  require('./core/update').check().catch(() => { });
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
        // The preview's tabs whose file still exists, the one shown first among them.
        const preview = stale.find(p => p.label === 'Preview');
        const shown = preview && readPaneState(preview.pane_id);
        const saved = shown && shown.tabs && Array.isArray(shown.tabs.list) ? shown.tabs : null;
        const list = saved ? saved.list.filter(t => t && exists(t.file, false)) : [];
        const at = saved ? list.indexOf(saved.list[saved.current]) : -1;
        const first = at >= 0 ? list[at] : list[0] || (shown && exists(shown.file, false) && shown);
        if (first) {
          await actions.preview(first.file, first.root, anchor, first.line, false, true,
            { view: first.view === 'diff' ? 'diff' : 'file', keep: !!first.kept, tabs: list.length ? { list, current: Math.max(0, at) } : null });
        }
      }
    }
    // The state of tabs and panes that no longer exist, closed while the plugin was off or before a restart.
    sweep();
  })();
} else {
  // Also here, for a herdr session running for days: the check itself happens once a day at most.
  require('./core/update').check().catch(() => { });
  const ev = JSON.parse(process.env.HERDR_PLUGIN_EVENT_JSON || '{}');
  const tab = ev.data && ev.data.tab;
  if (tab && readSettings().autoOpen) {
    const panes = tabPanes(tab.workspace_id, tab.tab_id);
    const root = ctx.workspace_id === tab.workspace_id ? ctx.workspace_cwd : null;
    if (panes.length === 1 && panes[0].label !== LABEL) open(panes, tab.tab_id, root);
  }
}
