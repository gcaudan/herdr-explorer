// Running external programs safely, and helpers shared by the plugin's scripts.
'use strict';
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');

const WIN = process.platform === 'win32';
const PLUGIN = process.env.HERDR_PLUGIN_ID || 'gcaudan.explorer';

// Absolute folders of PATH (relative entries would depend on the working directory).
const pathDirs = () => (process.env.PATH || '').split(path.delimiter)
  .map(d => d.replace(/^"(.*)"$/, '$1')).filter(d => path.isAbsolute(d));

// Absolute path of a program found in PATH, or null. On Windows, a bare name is looked up in the child's
// working directory before PATH, so a git.exe planted in an untrusted repository would run: resolve it first.
function which(name) {
  if (!WIN || path.isAbsolute(name)) return name;
  const file = path.extname(name) ? name : name + '.exe';
  for (const dir of pathDirs()) {
    const full = path.join(dir, file);
    if (fs.existsSync(full)) return full;
  }
  return null;
}

// Runs a program and resolves to its stdout, or null on failure (including a missing program).
const run = (cmd, args, opts = {}) => new Promise(resolve => {
  if (!cmd) return resolve(null);
  execFile(cmd, args, { windowsHide: true, maxBuffer: 64 << 20, ...opts }, (err, stdout) => resolve(err ? null : stdout));
});

// The result of a herdr command's JSON answer, or null.
function result(stdout) {
  try { return JSON.parse(stdout).result; } catch { return null; }
}

const HERDR = which(process.env.HERDR_BIN_PATH || 'herdr');
const GIT = which('git');

// Environment for every git call in a repository that may not be trusted, whose config could run commands:
// core.fsmonitor and hooks are disabled, and so are the filters (clean, smudge, process) the repository declares
// itself, in .git/config, config.worktree or an included file; global ones, such as git-lfs, are kept. Passed as
// GIT_CONFIG_KEY_n / GIT_CONFIG_VALUE_n, which, unlike -c, does not split a driver name containing '='.
// Submodules have their own config: callers pass --ignore-submodules=all. GIT_OPTIONAL_LOCKS=0 keeps git status
// from taking index.lock, which would make a concurrent git commit fail.
async function gitEnv(cwd) {
  const config = [['core.fsmonitor', 'false'], ['core.hooksPath', '/dev/null']];
  const listing = await run(GIT, ['config', '-z', '--includes', '--show-scope', '--name-only', '--get-regexp', '^filter\\.'],
    { cwd, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' } }) || '';
  const fields = listing.split('\0');
  const drivers = new Set();
  for (let i = 0; i + 1 < fields.length; i += 2) {
    const m = /^filter\.(.+)\.[^.]+$/s.exec(fields[i + 1]);
    if (m && fields[i] !== 'system' && fields[i] !== 'global') drivers.add(m[1]);
  }
  for (const d of drivers) {
    for (const k of ['clean', 'smudge', 'process']) config.push([`filter.${d}.${k}`, '']);
    config.push([`filter.${d}.required`, 'false']);
  }
  const env = { ...process.env, GIT_OPTIONAL_LOCKS: '0' };
  let n = parseInt(env.GIT_CONFIG_COUNT, 10) || 0;
  for (const [key, value] of config) {
    env[`GIT_CONFIG_KEY_${n}`] = key;
    env[`GIT_CONFIG_VALUE_${n++}`] = value;
  }
  env.GIT_CONFIG_COUNT = String(n);
  return env;
}

// Pane width remembered by the explorer (width file in the state folder), 32 columns by default.
const WIDTH_FILE = process.env.HERDR_PLUGIN_STATE_DIR && path.join(process.env.HERDR_PLUGIN_STATE_DIR, 'width');
function readWidth() {
  try { return Math.max(10, parseInt(fs.readFileSync(WIDTH_FILE, 'utf8'), 10)) || 32; } catch { return 32; }
}

module.exports = { WIN, PLUGIN, pathDirs, which, run, result, HERDR, GIT, gitEnv, WIDTH_FILE, readWidth };
