// Explorer actions on a file or folder: open, preview, edit, insert into the terminal, copy the path, reveal in the
// file manager, create, rename, trash, git stage and changes; the search and source control panes.
// Each one returns a short message or null.
'use strict';
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { WIN, PLUGIN, pathDirs, which, words, run, result, HERDR, GIT, gitEnv, readSettings } = require('./exec');

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

async function editor(abs, pane) {
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

async function system(abs) {
  if (!canOpen(abs)) return 'Not opened: file type not allowed';
  const ok = WIN ? await detached(which('explorer.exe'), [abs], { windowsHide: false })
    : await detached(MAC ? 'open' : 'xdg-open', [abs]);
  return ok ? `Opening ${path.basename(abs)}` : 'Cannot open';
}

// Double-click or Enter on a file.
function open(abs, pane) {
  return SYSTEM.has(ext(abs)) ? system(abs) : editor(abs, pane);
}

async function reveal(abs) {
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
  let ref = abs;
  if (p && p.cwd) {
    const rel = path.relative(p.cwd, abs);
    if (!rel.startsWith('..') && !path.isAbsolute(rel)) ref = rel || '.';
  }
  ref = ref.split(path.sep).join('/');
  if (/[\x00-\x1f\x7f-\x9f]/.test(ref)) return 'Not inserted: control character in name';
  const text = (p && p.agent && !/\s/.test(ref) ? `@${ref}` : quote(ref)) + ' ';
  // send-text prints nothing on success: its exit code tells.
  if (await run(HERDR, ['pane', 'send-text', target, text]) === null) return 'Cannot insert';
  // By direction from the terminal's other neighbour would be fragile: move the focus step by step until it lands.
  for (let from = pane, n = 0; n < 4; n++) {
    const res = await run(HERDR, ['pane', 'focus', '--direction', side, '--pane', from]).then(result);
    const now = res && res.focus && res.focus.focused_pane_id;
    if (!now || now === target) break;
    from = now;
  }
  return null;
}

// The terminal next to the explorer and its side: on the right of an explorer docked on the left, on the left of
// one docked on the right. Both are tried, as the setting may have changed since the explorer opened. The plugin's
// own panes are passed over: docked on the right, the preview sits between the terminal and the explorer.
const OWN = new Set(['Explorer', 'Preview', 'Source Control', 'Changes', 'Editor']);
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
    '--target-pane', pane, '--env', `EXPLORER_FILE=${abs}`, '--env', `EXPLORER_ROOT=${root}`]);
  return result(res) ? null : 'Cannot show changes';
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
const requestFile = tab => process.env.HERDR_PLUGIN_STATE_DIR
  && path.join(process.env.HERDR_PLUGIN_STATE_DIR, `preview-${String(tab).replace(/[^\w-]/g, '_')}.json`);

// Shows a file in the tab's preview pane, opened next to the terminal when there is none: explorer | terminal |
// preview. line: 1-based line to scroll to. focus: give the focus to the preview.
async function preview(abs, root, pane, line = 0, focus = false) {
  const tab = pane && await tabOf(pane);
  if (!tab) return 'Cannot preview';
  const request = { file: abs, root, line, t: Date.now() };
  const existing = tab.panes.find(p => p.label === 'Preview');
  if (existing) {
    try { fs.writeFileSync(requestFile(tab.me.tab_id), JSON.stringify(request)); } catch { return 'Cannot preview'; }
    if (focus) await herdr('plugin', 'pane', 'focus', existing.pane_id);
    return null;
  }
  const target = (await terminalOf(pane))[0] || pane;
  const res = await openPane('preview', 'split', target, {
    EXPLORER_FILE: abs, EXPLORER_ROOT: root, EXPLORER_LINE: line, EXPLORER_TAB: tab.me.tab_id,
  }, ['--direction', 'right', focus ? '--focus' : '--no-focus']);
  return res ? null : 'Cannot open the preview';
}

// The terminal editor ($VISUAL, $EDITOR, else nano or vi) in a full-tab pane, closed when the editor exits.
// command: the editor of the settings when it is a terminal one (see editor), else $VISUAL, $EDITOR…
async function edit(abs, pane, command = terminalEditor()) {
  return await openPane('edit', 'zoomed', pane, { EXPLORER_FILE: abs, ...(command ? { EXPLORER_EDITOR: command } : {}) })
    ? null : 'Cannot open the editor';
}

// Go to file (mode files) or search in files (mode content), in a popup over the tab.
async function search(root, pane, mode = 'files', query = '') {
  const env = { EXPLORER_ROOT: root, EXPLORER_PANE: pane, EXPLORER_MODE: mode };
  if (query) env.EXPLORER_QUERY = query;
  return await openPane('search', 'popup', pane, env) ? null : 'Cannot open the search';
}

async function scm(root, pane) {
  return await openPane('scm', 'zoomed', pane, { EXPLORER_ROOT: root, EXPLORER_PANE: pane }) ? null
    : 'Cannot open source control';
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
  if (fs.existsSync(to) && !sameFile(abs, to)) return [null, 'Already exists'];
  try { fs.mkdirSync(path.dirname(to), { recursive: true }); fs.renameSync(abs, to); } catch (e) {
    return [null, `Cannot rename: ${e.code || e.message}`];
  }
  return [to, null];
}

function sameFile(a, b) {
  try {
    const [x, y] = [fs.statSync(a, { bigint: true }), fs.statSync(b, { bigint: true })];
    return x.ino === y.ino && x.dev === y.dev && x.ino !== 0n;
  } catch { return false; }
}

// A copy next to the original: "name copy.ext", "name copy 2.ext"…
function duplicate(abs) {
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
async function trash(abs) {
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
  return ok && !fs.existsSync(abs);
}

function remove(abs) {
  try { fs.rmSync(abs, { recursive: true, force: true }); return null; } catch (e) { return `Cannot delete: ${e.code || e.message}`; }
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
  open, editor, system, canOpen, canPreview, reveal, copy, insert, diff, preview, edit, search, scm, requestFile, tabOf, terminalOf,
  create, rename, duplicate, trash, remove, stage, unstage, discard, REVEAL: WIN ? 'Reveal in Explorer' : 'Open folder',
};
