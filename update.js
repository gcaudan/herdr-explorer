// New releases of the plugin: a check of GitHub once a day (from the hook, never from the panes), the version found
// kept in the state folder, and the update itself, a fast-forward git pull of the plugin's clone. The explorer and
// preview panes restart on the new code by themselves when the version on disk changes (watch).
'use strict';
const fs = require('fs');
const path = require('path');
const https = require('https');
const { execFile } = require('child_process');
const { WIN, GIT, gitOperation, newer, readSettings } = require('./exec');

const DIR = __dirname; // the plugin's folder, a git clone
const MANIFEST = path.join(DIR, 'herdr-plugin.toml');
const URL = 'https://api.github.com/repos/gcaudan/herdr-explorer/releases/latest';
const DAY = 24 * 60 * 60 * 1000;

function readVersion() {
  try { return /^version\s*=\s*"([^"]+)"/m.exec(fs.readFileSync(MANIFEST, 'utf8'))[1]; } catch { return null; }
}
// The version of the code this process runs.
const CURRENT = readVersion();

// { checkedAt, latest } in the state folder.
const FILE = process.env.HERDR_PLUGIN_STATE_DIR ? path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'update.json') : null;
function read() {
  try { return JSON.parse(fs.readFileSync(FILE, 'utf8')) || {}; } catch { return {}; }
}
// Written to a temporary file then renamed, so that a pane never reads it half-written.
function write(data) {
  const tmp = `${FILE}.${process.pid}`;
  try {
    fs.mkdirSync(path.dirname(FILE), { recursive: true });
    fs.writeFileSync(tmp, JSON.stringify(data)); fs.renameSync(tmp, FILE);
  } catch { try { fs.unlinkSync(tmp); } catch { } }
}

// The version of the latest GitHub release, or null (offline, timeout, no release).
const latestRelease = () => new Promise(resolve => {
  const req = https.get(URL, {
    headers: { 'User-Agent': 'herdr-explorer', Accept: 'application/vnd.github+json' }, timeout: 3000,
  }, res => {
    if (res.statusCode !== 200) { res.resume(); return resolve(null); }
    let body = '';
    res.setEncoding('utf8');
    res.on('data', d => { body += d; });
    res.on('end', () => {
      let tag;
      try { tag = String(JSON.parse(body).tag_name).replace(/^v/, ''); } catch { }
      resolve(/^\d+(\.\d+)*$/.test(tag) ? tag : null);
    });
    res.on('error', () => resolve(null));
  });
  req.on('timeout', () => req.destroy());
  req.on('error', () => resolve(null));
});

// At most once a day, and not when the setting is off. A failed check is not retried before the next day either.
async function check() {
  if (!FILE || readSettings().updateCheck === false) return;
  const saved = read();
  if (Date.now() - (saved.checkedAt || 0) < DAY) return;
  const latest = await latestRelease();
  write({ ...read(), checkedAt: Date.now(), ...(latest ? { latest } : {}) });
}

// The newer release found, or null: compared with the version on disk, so that it goes away once pulled. A clone
// ahead of the release (development) shows nothing.
function available() {
  const { latest } = FILE ? read() : {};
  const current = readVersion();
  return latest && current && newer(latest, current) ? latest : null;
}

// git in the plugin's folder: { ok, out, err }. detached (not on Windows): a session without terminal, so that ssh
// or a credential helper can never prompt in the pane. GCM_INTERACTIVE: Git Credential Manager (Windows) otherwise
// opens its sign-in window, GIT_TERMINAL_PROMPT not reaching it, and the update waits for it until the timeout.
const git = (args, timeout = 10000, env = {}) => new Promise(resolve => {
  if (!GIT) return resolve({ ok: false, out: '', err: 'git not found' });
  execFile(GIT, args, { cwd: DIR, windowsHide: true, detached: !WIN, timeout,
    env: { ...process.env, GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', ...env } },
    (err, out, stderr) => resolve({ ok: !err, out: String(out), err: String(stderr) }));
});

const real = p => { try { return fs.realpathSync.native(p); } catch { return path.resolve(p); } };
const samePath = (a, b) => (WIN ? a.toLowerCase() === b.toLowerCase() : a === b);

// One update at a time, across the explorers of every tab: a lock file in the state folder, taken over after 2 min
// (a process killed during the pull). Without a state folder, or when it cannot be written, no lock.
const LOCK = FILE && path.join(path.dirname(FILE), 'update.lock');
function lock() {
  if (!LOCK) return true;
  try { fs.mkdirSync(path.dirname(LOCK), { recursive: true }); } catch { }
  try { fs.closeSync(fs.openSync(LOCK, 'wx')); return true; } catch (e) { if (e.code !== 'EEXIST') return true; }
  try {
    if (Date.now() - fs.statSync(LOCK).mtimeMs < 2 * 60 * 1000) return false;
    fs.writeFileSync(LOCK, '');
    return true;
  } catch { return false; }
}

// The command that runs the installer again, to finish an update that changed the manifest or the installer.
const INSTALLER = WIN ? `powershell -ExecutionPolicy Bypass -File "${path.join(DIR, 'install.ps1')}"` : `"${path.join(DIR, 'install.sh')}"`;
// The version whose update the installer must finish, or null: kept in update.json, so that every explorer shows it
// after the restart, until the command is taken (finished).
const finishing = () => (FILE && read().finish) || null;
function finished() {
  const { finish, ...rest } = read();
  if (finish) write(rest);
}

// Fast-forward of the clone to its remote main branch: { ok, message, retry }. retry: the pull itself failed, and
// running it in a terminal may succeed (a passphrase to type, the network back); a refusal has none. The installer is
// not run again: it unlinks and links the plugin, with an unknown effect on the panes running; the user does it when
// it changed.
async function install() {
  if (!lock()) return { ok: false, message: 'Update in progress' };
  try { return await pull(); } finally { try { fs.unlinkSync(LOCK); } catch { } }
}
async function pull() {
  // The plugin's own clone, not a repository holding a copy of it (dotfiles), whose pull would run its hooks.
  const top = await git(['rev-parse', '--show-toplevel']);
  if (!top.ok || !samePath(real(top.out.trim()), real(DIR))) return { ok: false, message: 'Not a git clone' };
  const head = await git(['rev-parse', '--abbrev-ref', 'HEAD']);
  if (head.out.trim() !== 'main') return { ok: false, message: 'Not on main' };
  const gitdir = await git(['rev-parse', '--absolute-git-dir']);
  if (!gitdir.ok || gitOperation(gitdir.out.trim())) return { ok: false, message: 'A git operation is in progress' };
  const status = await git(['status', '--porcelain', '--untracked-files=no']);
  if (!status.ok || status.out.trim()) return { ok: false, message: 'Local changes in the plugin folder' };
  const before = (await git(['rev-parse', 'HEAD'])).out.trim();
  // ssh in batch mode: GIT_TERMINAL_PROMPT does not reach it, and on Windows git shares the pane's console, where a
  // passphrase prompt would read the keys. Left alone when the user sets an ssh command.
  const own = process.env.GIT_SSH_COMMAND || process.env.GIT_SSH || (await git(['config', '--get', 'core.sshCommand'])).ok;
  const pull = await git(['pull', '--ff-only', '--no-rebase'], 60000, own ? {} : { GIT_SSH_COMMAND: 'ssh -o BatchMode=yes' });
  if (!pull.ok) {
    const lines = pull.err.split(/\r?\n/).filter(Boolean);
    const error = lines.find(l => /^(fatal|error):/.test(l)) || lines[0];
    return { ok: false, retry: true, message: error ? error.replace(/^(fatal|error):\s*/, '') : 'Update failed' };
  }
  const after = (await git(['rev-parse', 'HEAD'])).out.trim();
  if (!before || after === before) return { ok: true, message: 'Already up to date' };
  // The manifest (other than its version) or the installer changed: herdr and the installer must take it.
  const diff = await git(['diff', '-U0', before, after, '--', 'herdr-plugin.toml', 'install.sh', 'install.ps1']);
  const changed = !diff.ok || diff.out.split('\n')
    .some(l => /^[+-]/.test(l) && !/^(\+\+\+|---)/.test(l) && !/^[+-]version\s*=/.test(l));
  const to = readVersion();
  if (!changed) return { ok: true, message: `Updated to ${to}` };
  if (FILE) write({ ...read(), finish: to });
  return { ok: true, message: `Updated to ${to}: rerun the installer` };
}

// Runs restart once the version on disk changed (an update, or a git pull in a terminal), 2 s later so that the pull
// has written every file, and when idle() allows it; checked again every second until then.
function watch(idle, restart) {
  let timer = null;
  const due = () => { const v = readVersion(); return v && v !== CURRENT; };
  const attempt = () => {
    timer = null;
    if (!due()) return;
    if (!idle()) { timer = setTimeout(attempt, 1000); return; }
    fs.unwatchFile(MANIFEST);
    restart();
  };
  fs.watchFile(MANIFEST, { interval: 2000 }, () => {
    if (!timer && due()) timer = setTimeout(attempt, 2000);
  });
}

module.exports = { DIR, CURRENT, FILE, INSTALLER, readVersion, check, available, install, finishing, finished, watch };
