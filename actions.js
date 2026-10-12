// Explorer actions on a file or folder: open, insert into the terminal, copy the path, reveal in the file
// manager, view git changes. Each one returns a short message or null.
'use strict';
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { WIN, PLUGIN, pathDirs, which, run, result, HERDR } = require('./exec');

const MAC = process.platform === 'darwin';

// Documents and media: opened with the associated application on double-click. Everything else goes to
// VS Code, so a double-click never runs an executable or a script, even when VS Code is missing.
const SYSTEM = new Set(('png jpg jpeg gif webp bmp ico svg pdf doc docx xls xlsx ppt pptx odt ods odp '
  + 'zip 7z rar gz tgz mp3 mp4 wav mkv avi mov').split(' '));
// Also allowed from the menu (Open with default app): text formats opened by an editor or a browser.
const SAFE = new Set([...SYSTEM, ...'txt md csv log json xml html htm'.split(' ')]);
const ext = abs => path.extname(abs).slice(1).toLowerCase();
const canOpen = abs => SAFE.has(ext(abs));

function detached(cmd, args, opts = {}) {
  return new Promise(resolve => {
    if (!cmd) return resolve(false);
    const child = spawn(cmd, args, { detached: true, stdio: 'ignore', windowsHide: true, ...opts });
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
  if (!WIN) return (codeCommand = ['code', [], {}]);
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

async function vscode(abs) {
  const code = findCode();
  if (code && await detached(code[0], [...code[1], abs], code[2])) return `VS Code: ${path.basename(abs)}`;
  return 'VS Code not found';
}

async function system(abs) {
  if (!canOpen(abs)) return 'Not opened: file type not allowed';
  const ok = WIN ? await detached(which('explorer.exe'), [abs], { windowsHide: false })
    : await detached(MAC ? 'open' : 'xdg-open', [abs]);
  return ok ? `Opening ${path.basename(abs)}` : 'Cannot open';
}

// Double-click or Enter on a file.
function open(abs) {
  return SYSTEM.has(ext(abs)) ? system(abs) : vscode(abs);
}

async function reveal(abs) {
  let ok;
  // windowsHide would hide the File Explorer window itself. A path cannot contain '"' on Windows.
  if (WIN) ok = await detached(which('explorer.exe'), [`/select,"${abs}"`], { windowsVerbatimArguments: true, windowsHide: false });
  else if (MAC) ok = await detached('open', ['-R', abs]);
  else ok = await detached('xdg-open', [path.dirname(abs)]);
  return ok ? null : 'File manager not found';
}

async function copy(text) {
  let ok;
  if (WIN) {
    ok = await run(which('powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command', 'Set-Clipboard -Value $env:EXPLORER_CLIP'],
      { env: { ...process.env, EXPLORER_CLIP: text } }) !== null;
  } else if (MAC) ok = await pipe('pbcopy', [], text);
  else ok = await pipe('wl-copy', [], text) || await pipe('xclip', ['-selection', 'clipboard'], text);
  return ok ? 'Path copied' : 'Clipboard unavailable';
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
  const found = await run(HERDR, ['pane', 'neighbor', '--direction', 'right', '--pane', pane]).then(result);
  const target = found && found.neighbor && found.neighbor.neighbor_pane_id;
  if (!target) return 'No terminal on the right';
  const p = await run(HERDR, ['pane', 'get', target]).then(result).then(r => r && r.pane);
  let ref = abs;
  if (p && p.cwd) {
    const rel = path.relative(p.cwd, abs);
    if (!rel.startsWith('..') && !path.isAbsolute(rel)) ref = rel || '.';
  }
  ref = ref.split(path.sep).join('/');
  if (/[\x00-\x1f\x7f-\x9f]/.test(ref)) return 'Not inserted: control character in name';
  const text = (p && p.agent && !/\s/.test(ref) ? `@${ref}` : quote(ref)) + ' ';
  if (!await run(HERDR, ['pane', 'send-text', target, text]).then(result)) return 'Cannot insert';
  await run(HERDR, ['pane', 'focus', '--direction', 'right', '--pane', pane]);
  return null;
}

// Git changes in a temporary full-tab pane (q to close it). Zoomed placement: unlike overlay, it targets the
// explorer's tab rather than the active pane.
async function diff(abs, root, pane) {
  const res = await run(HERDR, ['plugin', 'pane', 'open', '--plugin', PLUGIN, '--entrypoint', 'diff', '--placement', 'zoomed',
    '--target-pane', pane, '--env', `EXPLORER_FILE=${abs}`, '--env', `EXPLORER_ROOT=${root}`]);
  return result(res) ? null : 'Cannot show changes';
}

module.exports = { open, vscode, system, canOpen, reveal, copy, insert, diff, REVEAL: WIN ? 'Reveal in Explorer' : 'Open folder' };
