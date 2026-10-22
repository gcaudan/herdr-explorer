// Explorer actions on a file or folder: open, preview, edit, insert into the terminal, copy the path, reveal in the
// file manager, create, rename, trash, git stage and changes; the search and source control panes.
// Each one returns a short message or null.
'use strict';
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { WIN, PLUGIN, pathDirs, which, remoteLink, writeAtomic, OWN_PANES: OWN, words, run, result, HERDR, GIT, gitEnv, readSettings, tabFile } = require('./exec');

const MAC = process.platform === 'darwin';

// Documents and media: opened with the associated application on double-click. Everything else goes to
// the editor, so a double-click never runs an executable or a script.
const SYSTEM = new Set(('png jpg jpeg gif webp bmp ico svg pdf doc docx xls xlsx ppt pptx odt ods odp '
  + 'zip 7z rar gz tgz mp3 mp4 wav mkv avi mov').split(' '));
// Also allowed from the menu (Open in app): text formats opened by an editor or a browser.
const SAFE = new Set([...SYSTEM, ...'txt md csv log json xml html htm'.split(' ')]);
const ext = abs => path.extname(abs).slice(1).toLowerCase();
const canOpen = abs => SAFE.has(ext(abs));
// Shown in the preview: everything but documents and media other than images (Office, PDF, archives, sound, video).
const IMAGES = new Set('png jpg jpeg gif webp bmp ico svg'.split(' '));
const canPreview = abs => !SYSTEM.has(ext(abs)) || IMAGES.has(ext(abs));

function detached(cmd, args, opts = {}) {
  return new Promise(resolve => {
    if (!cmd) return resolve(false);
    // A .cmd or .bat without a shell throws EINVAL on Windows instead of emitting 'error'.
    let child;
    try { child = spawn(cmd, args, { detached: true, stdio: 'ignore', windowsHide: true, ...opts }); } catch { return resolve(false); }
    child.on('error', () => resolve(false));
    child.on('spawn', () => { child.unref(); resolve(true); });
  });
}

// Feeds text to a clipboard tool. stdout is ignored: xclip and wl-copy stay in the background holding it.
function pipe(cmd, args, text) {
  return new Promise(resolve => {
    const child = spawn(cmd, args, { stdio: ['pipe', 'ignore', 'ignore'], windowsHide: true });
    child.on('error', () => resolve(false));
    child.on('exit', code => resolve(code === 0));
    child.stdin.on('error', () => { });
    child.stdin.end(text);
  });
}

// VS Code on Windows: code.cmd runs Code.exe in Node mode on cli.js; do the same, without going through cmd.
let codeCommand;
function findCode() {
  if (codeCommand !== undefined) return codeCommand;
  codeCommand = null;
  if (!WIN) return (codeCommand = which('code') ? [which('code'), [], {}] : null);
  const dirs = pathDirs();
  dirs.push(path.join(process.env.LOCALAPPDATA || '', 'Programs', 'Microsoft VS Code', 'bin'));
  for (const dir of dirs) {
    let cmd;
    try { cmd = fs.readFileSync(path.join(dir, 'code.cmd'), 'utf8'); } catch { continue; }
    const cli = /%~dp0\.\.\\([^"]*cli\.js)/.exec(cmd);
    const exe = path.join(dir, '..', 'Code.exe');
    if (cli && fs.existsSync(exe)) {
      return (codeCommand = [exe, [path.join(dir, '..', cli[1])], { env: { ...process.env, ELECTRON_RUN_AS_NODE: '1', VSCODE_DEV: '' } }]);
    }
  }
  return codeCommand;
}

// Editors that need a terminal: given in the settings, they run in the editor pane.
const TERMINAL = new Set('vi vim nvim nano micro hx helix kak emacs ne joe mcedit edit'.split(' '));

// External editor: the command of the settings, else VS Code, else the terminal editor (files only). The command
// runs without a shell, the path as its last argument.
const editorName = cmd => cmd && path.basename(cmd).toLowerCase().replace(/\.exe$/, '');
function terminalEditor() {
  const command = readSettings().editor || '';
  return TERMINAL.has(editorName(words(command)[0])) ? command : null;
}

// A link to a network share is never handed to a program, which would follow it (see remoteLink).
const NETWORK = 'Link to a network share: not opened';

async function editor(abs, pane) {
  if (remoteLink(abs)) return NETWORK;
  const command = readSettings().editor || '';
  const [cmd, ...args] = words(command);
  const name = editorName(cmd);
  if (TERMINAL.has(name)) return edit(abs, pane, command);
  // VS Code's command is a .cmd launcher on Windows, which cannot run without a shell: found as without a setting.
  const code = (!cmd || (WIN && /^code(\.cmd)?$/.test(name))) && findCode();
  if (cmd && !code) {
    if (await detached(which(cmd), [...args, abs])) return `${path.basename(cmd)}: ${path.basename(abs)}`;
    return WIN ? `${cmd} not found (an .exe is needed)` : `${cmd} not found`;
  }
  if (code && await detached(code[0], [...code[1], ...args, abs], code[2])) return `VS Code: ${path.basename(abs)}`;
  if (cmd) return 'VS Code not found';
  if (fs.statSync(abs, { throwIfNoEntry: false })?.isDirectory()) return 'No editor: set one in the settings';
  return edit(abs, pane);
}

// A page of the plugin's documentation in the browser: https only, never a path or another scheme.
async function web(url) {
  if (!/^https:\/\/[\w.-]+\/[\w./#%-]*$/.test(url)) return 'Not opened: not a web page';
  const ok = WIN ? await detached(which('explorer.exe'), [url], { windowsHide: false })
    : await detached(MAC ? 'open' : 'xdg-open', [url]);
  return ok ? 'Opening the documentation' : 'Cannot open a browser';
}

async function system(abs) {
  if (!canOpen(abs)) return 'Not opened: file type not allowed';
  if (remoteLink(abs)) return NETWORK;
  const ok = WIN ? await detached(which('explorer.exe'), [abs], { windowsHide: false })
    : await detached(MAC ? 'open' : 'xdg-open', [abs]);
  return ok ? `Opening ${path.basename(abs)}` : 'Cannot open';
}

// Double-click or Enter on a file.
function open(abs, pane) {
  return SYSTEM.has(ext(abs)) ? system(abs) : editor(abs, pane);
}

async function reveal(abs) {
  if (remoteLink(abs)) return NETWORK;
  let ok;
  // windowsHide would hide the File Explorer window itself. A path cannot contain '"' on Windows.
  if (WIN) ok = await detached(which('explorer.exe'), [`/select,"${abs}"`], { windowsVerbatimArguments: true, windowsHide: false });
  else if (MAC) ok = await detached('open', ['-R', abs]);
  else ok = await detached('xdg-open', [path.dirname(abs)]);
  return ok ? null : 'File manager not found';
}

// Without a clipboard tool (over SSH), OSC 52 asks the outer terminal to set its clipboard, if herdr passes it on.
async function copy(text, done = 'Path copied') {
  let ok;
  if (WIN) {
    ok = await run(which('powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command', 'Set-Clipboard -Value $env:EXPLORER_CLIP'],
      { env: { ...process.env, EXPLORER_CLIP: text } }) !== null;
  } else if (MAC) ok = await pipe('pbcopy', [], text);
  else ok = await pipe('wl-copy', [], text) || await pipe('xclip', ['-selection', 'clipboard'], text);
  if (!ok && process.stdout.isTTY) {
    process.stdout.write(`\x1b]52;c;${Buffer.from(text).toString('base64')}\x07`);
    return `${done} (OSC 52)`;
  }
  return ok ? done : 'Clipboard unavailable';
}

// The clipboard's text, line breaks as \n, or null without a tool to read it (over SSH, the terminal's own paste still
// reaches the pane, as a bracketed paste).
async function clipboard() {
  const opts = { timeout: 2000 };
  let text;
  if (WIN) {
    // In base64: Windows PowerShell writes its output in the console's OEM code page, which loses é or €.
    text = await run(which('powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command',
      '[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string](Get-Clipboard -Raw)))'], opts);
    if (text !== null) text = Buffer.from(text.trim(), 'base64').toString('utf8');
  } else if (MAC) text = await run(which('pbpaste'), [], opts);
  else {
    text = process.env.WAYLAND_DISPLAY ? await run(which('wl-paste'), ['--no-newline'], opts) : null;
    if (text === null && process.env.DISPLAY) text = await run(which('xclip'), ['-selection', 'clipboard', '-o'], opts);
  }
  return text === null ? null : text.replace(/\r\n?/g, '\n');
}

// Quoted for the shell unless made of plain characters: single quotes are literal in PowerShell (where the
// typographic ones count as quotes too, each doubled to escape it) and in POSIX shells ('\'' to escape).
// A leading dash would read as an option of the command being typed.
function quote(ref) {
  if (ref.startsWith('-')) ref = './' + ref;
  if (/^[\p{L}\p{N}_./@:+,=-]+$/u.test(ref)) return ref;
  if (WIN) return `'${ref.replace(/['‘’‚‛]/g, q => q + q)}'`;
  return `'${ref.replace(/'/g, "'\\''")}'`;
}

// Types the path into the terminal to the right of the explorer, relative to its folder, and focuses it.
// Prefixed with @ when an agent runs in that terminal (Claude Code), quoted for the shell otherwise; a name with
// spaces is quoted for the agent too, so that its end is clear and the rest cannot read as instructions.
// A name with a control character (newline, escape…) could run a command or submit a prompt: refused.
async function insert(abs, pane) {
  const [target, side] = await terminalOf(pane);
  if (!target) return 'No terminal next to the explorer';
  const p = await run(HERDR, ['pane', 'get', target]).then(result).then(r => r && r.pane);
  const ref = relativeTo(p && p.cwd, abs);
  if (/[\x00-\x1f\x7f-\x9f]/.test(ref)) return 'Not inserted: control character in name';
  const text = (p && p.agent && !/\s/.test(ref) ? `@${ref}` : quote(ref)) + ' ';
  return await typeInto(target, side, pane, text) ? null : 'Cannot insert';
}

// A path relative to a folder when inside it, with '/' separators; else the absolute path.
function relativeTo(dir, abs) {
  let ref = abs;
  if (dir) {
    const rel = path.relative(dir, abs);
    if (!rel.startsWith('..') && !path.isAbsolute(rel)) ref = rel || '.';
  }
  return ref.split(path.sep).join('/');
}

// Types one line of text into the terminal next to the explorer, without submitting it, and focuses that terminal.
// text: the line, or a function of the terminal's folder giving it. It must hold no control character: a newline
// would submit a prompt or run a command.
async function type(text, pane) {
  const [target, side] = await terminalOf(pane);
  if (!target) return 'No terminal next to the explorer';
  if (typeof text === 'function') {
    const p = await run(HERDR, ['pane', 'get', target]).then(result).then(r => r && r.pane);
    text = text(p && p.cwd);
  }
  if (/[\x00-\x1f\x7f-\x9f]/.test(text)) return 'Not typed: control character';
  return await typeInto(target, side, pane, text) ? null : 'Cannot type into the terminal';
}

async function typeInto(target, side, pane, text) {
  // send-text prints nothing on success: its exit code tells.
  if (await run(HERDR, ['pane', 'send-text', target, text]) === null) return false;
  // By direction from the terminal's other neighbour would be fragile: move the focus step by step until it lands.
  for (let from = pane, n = 0; n < 4; n++) {
    const res = await run(HERDR, ['pane', 'focus', '--direction', side, '--pane', from]).then(result);
    const now = res && res.focus && res.focus.focused_pane_id;
    if (!now || now === target) break;
    from = now;
  }
  return true;
}

// The terminal next to the explorer and its side: on the right of an explorer docked on the left, on the left of
// one docked on the right. Both are tried, as the setting may have changed since the explorer opened. The plugin's
// own panes are passed over: docked on the right, the preview sits between the terminal and the explorer.
async function terminalOf(pane) {
  const order = readSettings().side === 'right' ? ['left', 'right'] : ['right', 'left'];
  for (const side of order) {
    for (let from = pane, n = 0; n < 4; n++) {
      const found = await run(HERDR, ['pane', 'neighbor', '--direction', side, '--pane', from]).then(result);
      const target = found && found.neighbor && found.neighbor.neighbor_pane_id;
      if (!target) break;
      const p = await run(HERDR, ['pane', 'get', target]).then(result);
      if (!(p && p.pane && OWN.has(p.pane.label))) return [target, side];
      from = target;
    }
  }
  return [null, null];
}

// Git changes in a temporary full-tab pane (q to close it). Zoomed placement: unlike overlay, it targets the
// explorer's tab rather than the active pane.
async function diff(abs, root, pane) {
  const res = await run(HERDR, ['plugin', 'pane', 'open', '--plugin', PLUGIN, '--entrypoint', 'diff', '--placement', 'zoomed',
    '--target-pane', pane, '--env', `EXPLORER_FILE=${abs}`, '--env', `EXPLORER_ROOT=${root}`, '--env', `EXPLORER_PANE=${pane}`]);
  return result(res) ? null : 'Cannot show changes';
}

// --- pane widths

// The tab's layout as seen from a pane: its panes and splits, with their rects.
async function layoutOf(pane) {
  const res = result(await run(HERDR, ['pane', 'layout', '--pane', pane]));
  return res && res.layout;
}

// The split of which the pane is a whole side: { split, left, me }, left when the pane is its left part, or null.
// A pane's width includes its border.
function sideSplit(layout, pane) {
  const me = layout.panes.find(p => p.pane_id === pane);
  if (!me) return null;
  for (const s of layout.splits) {
    if (s.direction !== 'right' || s.rect.y !== me.rect.y || s.rect.height !== me.rect.height) continue;
    if (s.rect.x === me.rect.x && Math.abs(Math.round(s.rect.width * s.ratio) - me.rect.width) <= 1) return { split: s, left: true, me };
    if (s.rect.x + s.rect.width === me.rect.x + me.rect.width
      && Math.abs(Math.round(s.rect.width * (1 - s.ratio)) - me.rect.width) <= 1) return { split: s, left: false, me };
  }
  return null;
}

// The pane given want columns (its border included) by moving the border of its split; the ratio is that of the
// split's left part. Returns the ratio asked for.
async function setWidth(pane, { split, left }, want) {
  const ratio = left ? (want + 0.25) / split.rect.width : 1 - (want + 0.25) / split.rect.width;
  const delta = ratio - split.ratio;
  await run(HERDR, ['pane', 'resize', '--pane', pane, '--direction', delta > 0 ? 'right' : 'left', '--amount', Math.abs(delta).toFixed(6)]);
  return ratio;
}

// Keeps a pane at its width. herdr keeps the ratio of each split, so that moving a border, resizing the window, or
// opening or closing a pane rescales the panes nested beside it. check(), after each resize of the pane, puts its
// width back when its own split kept its ratio, and takes the new width when that ratio changed (its own border
// dragged); check(true) puts it back in any case, as after < > changed the width kept, and puts back the panes this
// moves (KEEPING). Checks asked while a resize
// runs make one more once it ends, with the panes' new sizes and the last width asked for: a key held down (< >)
// moves the border as fast as herdr follows. The width kept is at least 10 columns, and leaves 20 to the other side
// of the split. columns(): the pane's width now, without border; get(): the width it keeps (null at first: the
// width it has); set(width): the width taken; side: 'left' or 'right', the part of its split the pane must be
// (null: either).
// The plugin's panes that keep their width, and the file where a pane changing its own width names the panes it
// moves, for half a second: those panes are then put back by it, in the same burst (no frame shows them moved), rather than
// by themselves as well, which would move them twice (herdr resizes by a relative amount).
const KEEPING = new Set(['Explorer', 'Preview']);
const RESIZING = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'resizing');
function resizingElsewhere(pane) {
  try {
    const { moved, at } = JSON.parse(fs.readFileSync(RESIZING, 'utf8'));
    return Array.isArray(moved) && moved.includes(pane) && Date.now() - at < 500;
  } catch { return false; }
}
function markResizing(moved) {
  if (!RESIZING) return;
  // Whole or not at all, as the other panes read it at any time.
  writeAtomic(RESIZING, JSON.stringify({ moved, at: Date.now() }));
}

// The widths (borders included) of the other panes of the layout that keep theirs.
async function keptWidths(layout, pane) {
  const list = ((await herdr('pane', 'list')) || {}).panes || [];
  const keeping = new Set(list.filter(p => KEEPING.has(p.label)).map(p => p.pane_id));
  return new Map(layout.panes.filter(p => p.pane_id !== pane && keeping.has(p.pane_id)).map(p => [p.pane_id, p.rect.width]));
}

// The panes of widths moved by a resize, put back.
async function restoreWidths(pane, widths) {
  const layout = await layoutOf(pane);
  for (const [id, width] of widths) {
    const found = layout && sideSplit(layout, id);
    if (found && found.me.rect.width !== width) await setWidth(id, found, width);
  }
}

function widthKeeper(pane, { columns, get, set, side = null }) {
  // border: the columns of the pane's border, the difference between its width in the layout and its terminal's,
  // learnt when both are up to date: not right after a resize, the terminal hearing of it a little later.
  // left: the pane is the left part of its split (its inner border on its right), as last seen; null before.
  let ratio = null, area = 0, panes = '', border = null, left = null, busy = false, again = null;
  async function check(force = false) {
    if (!pane) return;
    if (busy) { again = again || force; return; }
    busy = true;
    try {
      const layout = await layoutOf(pane);
      if (!layout || layout.zoomed) return;
      const found = sideSplit(layout, pane);
      if (!found || (side && found.left !== (side === 'left'))) return;
      left = found.left;
      if (!force || border === null) border = found.me.rect.width - columns();
      const now = found.me.rect.width - border;
      const set_ = layout.panes.map(p => p.pane_id).sort().join(' ');
      const own = !force && ratio !== null && layout.area.width === area && set_ === panes
        && Math.abs(found.split.ratio - ratio) >= 0.001;
      // Panes opened, closed or moved: checked again once the layout has settled, as a move made in two steps may
      // leave nothing to report the last one.
      if (panes && set_ !== panes) setTimeout(() => check(), 500);
      area = layout.area.width; panes = set_; ratio = found.split.ratio;
      if (own || get() === null) return set(now);
      // Cut down to fit the split, kept as it is: a narrow window does not lower the width wanted back afterwards,
      // only the pane's own resizing (< >) does, so that a key held down stops adding up at the limit.
      const want = Math.max(10, Math.min(found.split.rect.width - border - 20, get()));
      if (force && want !== get()) set(want);
      if (now === want) return;
      if (!force && resizingElsewhere(pane)) return;
      // Changing its own width (< >), the pane puts back the others it moves.
      const others = force ? await keptWidths(layout, pane) : null;
      if (others && others.size) markResizing([...others.keys()]);
      ratio = await setWidth(pane, found, want + border);
      if (others && others.size) await restoreWidths(pane, others);
    } catch { } finally {
      busy = false;
      if (again !== null) { const f = again; again = null; check(f); }
    }
  }
  return { check, left: () => left };
}

// --- plugin panes

const herdr = (...args) => run(HERDR, args).then(result);
// herdr opens a popup over the active pane and refuses a target for it.
const openPane = (entry, placement, target, env, extra = []) => herdr('plugin', 'pane', 'open', '--plugin', PLUGIN,
  '--entrypoint', entry, '--placement', placement, ...(placement === 'popup' ? [] : ['--target-pane', target]), ...extra,
  ...Object.entries(env).flatMap(([k, v]) => ['--env', `${k}=${v}`]));

// The panes of the tab holding a pane, with their labels, and that pane's own record.
async function tabOf(pane) {
  const me = await herdr('pane', 'get', pane).then(r => r && r.pane);
  if (!me) return null;
  const all = await herdr('pane', 'list', '--workspace', me.workspace_id).then(r => (r && r.panes) || []);
  return { me, panes: all.filter(p => p.tab_id === me.tab_id) };
}

// The preview of a tab reads its requests from a file of the state folder, so that the explorer and the search
// reuse one preview pane per tab instead of opening a new one for each file.
const requestFile = tab => tabFile('preview', tab);

// Shows a file in the tab's preview pane, opened next to the terminal when there is none: explorer | terminal |
// preview. line: 1-based line to scroll to. focus: give the focus to the preview. open: false to only update a
// preview already open, doing nothing without one. marks: the parts to highlight, [{ from, to, a, b }] (1-based lines,
// and columns a to b included, 0 for whole lines). view: 'diff' to show the file's git changes rather than the file.
// keep: in a kept tab of the preview, rather than the temporary one that the next file replaces (a double-click, Enter).
// tabs: the tabs of a new preview, { list, current }, as the preview saved them (restore after a herdr restart).
async function preview(abs, root, pane, line = 0, focus = false, open = true, { marks = [], view = 'file', keep = false, tabs = null } = {}) {
  const tab = pane && await tabOf(pane);
  if (!tab) return 'Cannot preview';
  const request = { file: abs, root, line, marks, view, keep, t: Date.now() };
  const existing = tab.panes.find(p => p.label === 'Preview');
  if (existing) {
    // Whole or not at all: the preview polls it.
    if (!writeAtomic(requestFile(tab.me.tab_id), JSON.stringify(request))) return 'Cannot preview';
    if (focus) await herdr('plugin', 'pane', 'focus', existing.pane_id);
    return null;
  }
  if (!open) return null;
  const target = (await terminalOf(pane))[0] || pane;
  const res = await openPane('preview', 'split', target, {
    EXPLORER_FILE: abs, EXPLORER_ROOT: root, EXPLORER_LINE: line, EXPLORER_TAB: tab.me.tab_id,
    ...(marks.length ? { EXPLORER_MARKS: JSON.stringify(marks) } : {}), ...(view === 'diff' ? { EXPLORER_VIEW: 'diff' } : {}),
    ...(keep ? { EXPLORER_KEEP: '1' } : {}), ...(tabs ? { EXPLORER_TABS: JSON.stringify(tabs) } : {}),
  }, ['--direction', 'right', focus ? '--focus' : '--no-focus']);
  return res ? null : 'Cannot open the preview';
}

// The terminal editor ($VISUAL, $EDITOR, else nano, nvim, vim or vi; see edit.js) in a full-tab pane, closed when the
// editor exits.
// command: the editor of the settings when it is a terminal one (see editor), else $VISUAL, $EDITOR…
async function edit(abs, pane, command = terminalEditor()) {
  if (remoteLink(abs)) return NETWORK;
  return await openPane('edit', 'zoomed', pane, { EXPLORER_FILE: abs, ...(command ? { EXPLORER_EDITOR: command } : {}) })
    ? null : 'Cannot open the editor';
}

// Go to file (mode files) or search in files (mode content), in a popup over the tab.
async function search(root, pane, mode = 'files', query = '') {
  const env = { EXPLORER_ROOT: root, EXPLORER_PANE: pane, EXPLORER_MODE: mode };
  if (query) env.EXPLORER_QUERY = query;
  return await openPane('search', 'popup', pane, env) ? null : 'Cannot open the search';
}

// file: a file or folder to select among the changes; history: show the commits of that file instead.
async function scm(root, pane, file = '', history = false) {
  const env = { EXPLORER_ROOT: root, EXPLORER_PANE: pane, ...(file ? { [history ? 'EXPLORER_HISTORY' : 'EXPLORER_FILE']: file } : {}) };
  return await openPane('scm', 'zoomed', pane, env) ? null : 'Cannot open source control';
}

// --- files

// A name typed by the user, relative to a folder: subfolders are allowed (a/b.txt), leaving the folder is not.
function target(dir, name) {
  name = name.trim();
  if (!name || /[\x00-\x1f\x7f-\x9f]/.test(name) || path.isAbsolute(name)) return null;
  const abs = path.resolve(dir, name);
  const rel = path.relative(dir, abs);
  return rel && !rel.startsWith('..') && !path.isAbsolute(rel) ? abs : null;
}

function create(dir, name, folder) {
  const abs = target(dir, name.replace(/[\\/]+$/, ''));
  if (!abs) return [null, 'Invalid name'];
  if (fs.existsSync(abs)) return [null, 'Already exists'];
  try {
    if (folder || /[\\/]$/.test(name)) fs.mkdirSync(abs, { recursive: true });
    else { fs.mkdirSync(path.dirname(abs), { recursive: true }); fs.writeFileSync(abs, '', { flag: 'wx' }); }
  } catch (e) { return [null, `Cannot create: ${e.code || e.message}`]; }
  return [abs, null];
}

function rename(abs, name) {
  const to = target(path.dirname(abs), name);
  if (!to) return [null, 'Invalid name'];
  if (to === abs) return [abs, null];
  // A case-only change on a case-insensitive file system finds the file itself: the same file, not another one.
  // lstat: a link is renamed, not followed.
  if (fs.lstatSync(to, { throwIfNoEntry: false }) && !sameFile(abs, to)) return [null, 'Already exists'];
  try { fs.mkdirSync(path.dirname(to), { recursive: true }); fs.renameSync(abs, to); } catch (e) {
    return [null, `Cannot rename: ${e.code || e.message}`];
  }
  return [to, null];
}

function sameFile(a, b) {
  try {
    const [x, y] = [fs.lstatSync(a, { bigint: true }), fs.lstatSync(b, { bigint: true })];
    return x.ino === y.ino && x.dev === y.dev && x.ino !== 0n;
  } catch { return false; }
}

// A copy next to the original: "name copy.ext", "name copy 2.ext"…
function duplicate(abs) {
  if (remoteLink(abs)) return [null, NETWORK];
  const ext = fs.statSync(abs).isDirectory() ? '' : path.extname(abs);
  const base = abs.slice(0, abs.length - ext.length);
  let to = `${base} copy${ext}`;
  for (let n = 2; fs.existsSync(to); n++) to = `${base} copy ${n}${ext}`;
  try { fs.cpSync(abs, to, { recursive: true, errorOnExist: true, force: false, verbatimSymlinks: true }); } catch (e) {
    return [null, `Cannot duplicate: ${e.code || e.message}`];
  }
  return [to, null];
}

// To the trash of the desktop: gio on Linux, trash (macOS 14) or Finder on macOS, the recycle bin on Windows.
// Never a link to a network share: the recycle bin would follow it, and on Windows a folder link reads as a file to
// lstat; remove() deletes the link itself.
async function trash(abs) {
  if (remoteLink(abs)) return false;
  let ok;
  if (WIN) {
    const fn = fs.statSync(abs).isDirectory() ? 'DeleteDirectory' : 'DeleteFile';
    ok = await run(which('powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command',
      `Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::${fn}($env:EXPLORER_TRASH, 'OnlyErrorDialogs', 'SendToRecycleBin')`],
    { env: { ...process.env, EXPLORER_TRASH: abs } }) !== null;
  } else if (MAC) {
    ok = await run('/usr/bin/trash', [abs]) !== null || await run('osascript', ['-e',
      'on run argv\ntell application "Finder" to delete POSIX file (item 1 of argv)\nend run', abs]) !== null;
  } else ok = await run('gio', ['trash', '--', abs]) !== null || await run('trash-put', ['--', abs]) !== null;
  return ok && !fs.lstatSync(abs, { throwIfNoEntry: false });
}

// A link goes itself, its target untouched: unlink, or rmdir for a folder link on Windows, which unlink refuses.
function remove(abs) {
  try {
    if (fs.lstatSync(abs, { throwIfNoEntry: false })?.isSymbolicLink()) {
      try { fs.unlinkSync(abs); } catch (e) { if (e.code !== 'EPERM' && e.code !== 'EISDIR') throw e; fs.rmdirSync(abs); }
    } else fs.rmSync(abs, { recursive: true, force: true });
    return null;
  } catch (e) { return `Cannot delete: ${e.code || e.message}`; }
}

// --- git

async function git(root, args) {
  // The paths are literal: a file named '*' or 'app/[id]' means itself, not a pattern.
  return run(GIT, args, { cwd: root, env: { ...await gitEnv(root), GIT_LITERAL_PATHSPECS: '1' } });
}

async function stage(abs, root) {
  return await git(root, ['add', '-A', '--', abs]) !== null ? 'Staged' : 'Cannot stage';
}

// Without any commit, there is no HEAD to restore the index from: the paths leave the index instead.
async function unstage(abs, root) {
  const head = await git(root, ['rev-parse', '--verify', '-q', 'HEAD']);
  const res = head ? await git(root, ['restore', '--staged', '--', abs])
    : await git(root, ['rm', '-r', '-q', '--cached', '--ignore-unmatch', '--', abs]);
  return res !== null ? 'Unstaged' : 'Cannot unstage';
}

// Tracked files go back to their last commit, staged changes included. Files added since (staged but not in the
// last commit, or all of them before the first commit) leave the index, then go to the trash with the untracked
// ones: restoring them would delete them for good.
async function discard(abs, root) {
  const list = async args => ((await git(root, args)) || '').split('\0').filter(Boolean);
  const head = await git(root, ['rev-parse', '--verify', '-q', 'HEAD']);
  if (head) {
    const added = await list(['diff', '--cached', '--relative', '--name-only', '-z', '--diff-filter=A', '--', abs]);
    if (added.length && await git(root, ['restore', '--staged', '--', ...added.map(f => path.resolve(root, f))]) === null) {
      return 'Cannot discard';
    }
  } else if (await git(root, ['rm', '-r', '-q', '--cached', '--ignore-unmatch', '--', abs]) === null) return 'Cannot discard';
  // Restored from HEAD when it holds something there, even a path whose deletion is staged (not in the index).
  if (head && (await list(['ls-files', '-z', '--with-tree=HEAD', '--', abs])).length
    && await git(root, ['restore', '--source=HEAD', '--staged', '--worktree', '--', abs]) === null) return 'Cannot discard';
  // Listed after the restore: a file whose deletion was staged is tracked again, not trashed.
  const untracked = await list(['ls-files', '-z', '--others', '--exclude-standard', '--', abs]);
  let kept = 0;
  for (const f of untracked) if (!await trash(path.resolve(root, f))) kept++;
  return kept ? `${kept} untracked file${kept > 1 ? 's' : ''} kept: no trash` : 'Changes discarded';
}

module.exports = {
  open, editor, system, web, canOpen, canPreview, reveal, copy, clipboard, insert, type, relativeTo, diff, preview, edit, search, scm, requestFile, tabOf, terminalOf,
  layoutOf, sideSplit, setWidth, widthKeeper,
  create, rename, duplicate, trash, remove, stage, unstage, discard, REVEAL: WIN ? 'Reveal in Explorer' : 'Open folder',
};
