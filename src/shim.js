// What every entry of the manifest runs (`node src/<entry>.js`): the plugin's binary, bin/herdr-explorer, with the
// entry's subcommand. The binary is fetched first (scripts/fetch-bin) when it is missing or of another version than
// the manifest's, never over one of bin/.dev (scripts/dev-build).
// A pane's shim is its first process, the one herdr reads the pane's folder from (home). The explorer's and the
// preview's stays as the launcher of their binary: started again on exit code 75 (a restart on new code, the binary
// found again by its path), with what it asked for. The binary writes its messages, a line of JSON each, to the file
// EXPLORER_IPC_FILE names. Under the launcher of 0.8.0 and earlier (src/launch.js, still running in a pane opened
// before the update), the shim is its child: it hands the messages on through process.send, and gives back the code.
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const RESTART = 75; // the exit code of a binary that asks to be started again
const WIN = process.platform === 'win32';
const ROOT = path.join(__dirname, '..');
const BIN = path.join(ROOT, 'bin', WIN ? 'herdr-explorer.exe' : 'herdr-explorer');

// Herdr starts plugin panes in the plugin's folder and reads a pane's folder from its first process: in the space
// folder, a new space, tab or pane made from this one starts there instead of in the plugin's checkout.
function home(dir = process.env.EXPLORER_ROOT) {
  if (dir) try { process.chdir(dir); } catch { }
}

function wanted() {
  try {
    const m = /^version\s*=\s*"([^"]*)"/m.exec(fs.readFileSync(path.join(ROOT, 'herdr-plugin.toml'), 'utf8'));
    return m ? m[1] : null;
  } catch { return null; }
}

function installed() {
  const res = spawnSync(BIN, ['--version'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
  return res.status === 0 ? res.stdout.trim() : null;
}

// The binary there, of the manifest's version, or fetched; fetch-bin's own output goes to stderr, as stdout may be
// read (peek). By absolute paths: the shell and PowerShell of the system, never one found in a folder of PATH.
function ready() {
  const dev = fs.existsSync(path.join(ROOT, 'bin', '.dev'));
  if (fs.existsSync(BIN) && (dev || installed() === wanted())) return true;
  if (dev) return false;
  const res = WIN
    ? spawnSync(path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'),
      ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(ROOT, 'scripts', 'fetch-bin.ps1'), ROOT],
      { stdio: ['ignore', 2, 2], windowsHide: true })
    : spawnSync('/bin/sh', [path.join(ROOT, 'scripts', 'fetch-bin.sh'), ROOT], { stdio: ['ignore', 2, 2] });
  return res.status === 0 && fs.existsSync(BIN);
}

function missing() {
  console.error(`herdr-explorer: no binary of version ${wanted()} in ${path.dirname(BIN)}: run scripts/fetch-bin to see why`);
  process.exit(1);
}

// The hook and peek: the binary, its code given back.
function run(args) {
  if (!ready()) missing();
  const res = spawnSync(BIN, args, { stdio: 'inherit', windowsHide: true });
  process.exit(res.status ?? 1);
}

// A pane: the binary with the pane's id; launcher: the explorer and the preview. dir: the pane's folder.
function pane(id, { launcher = false, dir } = {}) {
  home(dir);
  if (!ready()) missing();
  // The messages handed on in order; the shim exits once the last one is sent, as Node's channel writes later.
  let sent = Promise.resolve();
  const relay = process.send ? m => {
    sent = sent.then(() => new Promise(done => { try { process.send(m, () => done()); } catch { done(); } }));
  } : null;
  launcher = launcher && !relay;
  // Raw mode from one binary to the next, never read here: in line mode, a ctrl+c typed while the next one starts
  // would kill the pane on Unix. Each binary restores at exit the mode it found.
  if (launcher && process.stdin.isTTY) { process.stdin.setRawMode(true); process.stdin.pause(); }
  // In a folder of its own, made new (mode 0700): in the shared temporary folder, another user could write the
  // messages first (an environment for the next binary).
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'herdr-explorer-'));
  const ipc = path.join(folder, 'ipc');
  let env = { ...process.env, EXPLORER_IPC_FILE: ipc, EXPLORER_LAUNCHER: String(process.pid) };
  let child, read = 0, rest = Buffer.alloc(0);
  const finish = code => {
    try { fs.rmSync(folder, { recursive: true, force: true }); } catch { }
    sent.then(() => process.exit(code));
  };
  const message = m => {
    if (!m || typeof m !== 'object') return;
    if (relay) relay(m);
    else if (m.restart && typeof m.restart === 'object') env = { ...env, ...m.restart };
    if (typeof m.cwd === 'string') home(m.cwd);
  };
  // Whole lines only, cut on the bytes: a line still being written is read at the next look.
  const take = () => {
    try {
      const fd = fs.openSync(ipc, 'r');
      try {
        const size = fs.fstatSync(fd).size;
        if (size <= read) return;
        const buf = Buffer.alloc(size - read);
        const n = fs.readSync(fd, buf, 0, buf.length, read);
        read += n;
        rest = Buffer.concat([rest, buf.subarray(0, n)]);
      } finally { fs.closeSync(fd); }
    } catch { return; }
    const end = rest.lastIndexOf(10);
    if (end < 0) return;
    const lines = rest.subarray(0, end).toString('utf8').split('\n');
    rest = rest.subarray(end + 1);
    for (const line of lines) try { message(JSON.parse(line)); } catch { }
  };
  const start = () => {
    fs.writeFileSync(ipc, '');
    read = 0; rest = Buffer.alloc(0);
    child = spawn(BIN, ['pane', id, ...process.argv.slice(2)], { stdio: 'inherit', env });
    const look = setInterval(take, 250);
    child.on('error', () => { clearInterval(look); finish(1); });
    // Killed by a signal (no code): a failure, not a quit.
    child.on('close', code => {
      clearInterval(look);
      take();
      if (launcher && code === RESTART) return ready() ? start() : finish(1);
      finish(code ?? 1);
    });
  };
  for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT']) process.on(sig, () => { try { child.kill(sig); } catch { child.kill(); } });
  // The old launcher gone (killed), the shim and its binary go too, instead of running on in a terminal nobody shows.
  if (relay) {
    process.on('disconnect', () => { try { child.kill(); } catch { } finish(0); });
    process.channel.unref();
  }
  start();
}

module.exports = { run, pane, home, RESTART };
