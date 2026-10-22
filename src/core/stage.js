// Staging, unstaging and discarding parts of a diff, from the Changes pane and the preview's Diff view: the rows
// selected give whole files, hunks or lines, and a patch made from the parsed diff (diff.js parse) goes through
// git apply: --cached for the index, -R to take changes back.
//   stage    lines of the Unstaged section, added to the index
//   unstage  lines of the Staged section, taken out of the index
//   discard  lines of the Unstaged section, taken out of the working tree (the caller confirms first)
'use strict';
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { GIT, gitEnv } = require('./exec');

// The section each mode works on.
const SECTION = { stage: 'unstaged', unstage: 'staged', discard: 'unstaged' };

// What rows select (diff.js render): whole files from their name's row (f), whole hunks from their @@ row (h), lines
// from theirs (lines, both sides of a side by side row).
function picked(rows) {
  const sel = { files: new Set(), hunks: new Set(), lines: new Set() };
  for (const r of rows) {
    if (r.f) sel.files.add(r.f);
    if (r.h) sel.hunks.add(r.h);
    if (r.lines) for (const l of r.lines) sel.lines.add(l);
  }
  return sel;
}

const isChange = l => l[0] === '+' || l[0] === '-';

// The files of the diff a mode acts on, with the changed lines selected in each: [{ f, keep, all }], keep the Set of
// those lines, all when they are every change of the file (or the file has none, a binary file or a mode change).
function targets(parsed, sel, mode) {
  const out = [];
  for (const f of parsed.files) {
    if (f.sec !== SECTION[mode]) continue;
    const changes = f.hunks.flatMap(h => h.lines.filter(isChange));
    const keep = new Set(sel.files.has(f) ? changes
      : f.hunks.flatMap(h => h.lines.filter(l => isChange(l) && (sel.hunks.has(h) || sel.lines.has(l)))));
    if (!keep.size && !(sel.files.has(f) && !changes.length)) continue;
    out.push({ f, keep, all: keep.size === changes.length });
  }
  return out;
}

// A path in a patch header, quoted as git does when it holds a quote, a backslash or a control character.
function quote(p) {
  if (!/["\\\x00-\x1f\x7f]/.test(p)) return p;
  const esc = { '"': '\\"', '\\': '\\\\', '\t': '\\t', '\n': '\\n', '\r': '\\r' };
  return `"${p.replace(/["\\\x00-\x1f\x7f]/g, c => esc[c] || `\\${c.charCodeAt(0).toString(8).padStart(3, '0')}`)}"`;
}

// The patch of a file for the lines kept, or null when none is left. Applied forward (stage), lines left out stay as
// they are in the index: a removed line becomes context, an added one goes. Applied with -R (unstage, discard), the
// patch must match what is there now, so the other way round: an added line left out becomes context, a removed one
// goes. A whole added or deleted file keeps its headers; part of one is a change of the file (for an untracked file,
// once git add -N has put it in the index, empty), but for part of a deletion taken back (-R), which brings back the
// file with the lines kept. A whole file takes its mode change with it.
function patchOf({ f, keep, all }, mode) {
  const reverse = mode !== 'stage';
  const name = f.to || f.from, a = `a/${name}`, b = `b/${name}`;
  const head = [`diff --git ${quote(a)} ${quote(b)}`];
  if (all && f.status === 'added') head.push(`new file mode ${f.mode || '100644'}`, '--- /dev/null', `+++ ${quote(b)}`);
  else if ((all || reverse) && f.status === 'deleted') head.push(`deleted file mode ${f.mode || '100644'}`, `--- ${quote(a)}`, '+++ /dev/null');
  else {
    if (all && f.oldMode && f.newMode) head.push(`old mode ${f.oldMode}`, `new mode ${f.newMode}`);
    head.push(`--- ${quote(a)}`, `+++ ${quote(b)}`);
  }
  const body = [];
  let delta = 0; // lines added by the hunks before, on the side that changes
  for (const h of f.hunks) {
    const m = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/.exec(h.head);
    if (!m || !h.lines.some(l => keep.has(l))) continue;
    const lines = [];
    let shown = false, oldN = 0, newN = 0;
    for (const l of h.lines) {
      const [s, text, , , , cr] = l;
      // "\ No newline at end of file" follows the line it is about, when that line is in the patch.
      if (s === '\\') { if (shown) lines.push('\\ No newline at end of file'); continue; }
      const sign = s === ' ' || keep.has(l) ? s : (s === '-') !== reverse ? ' ' : null;
      shown = sign !== null;
      if (!shown) continue;
      lines.push(sign + text + (cr ? '\r' : ''));
      if (sign !== '+') oldN++;
      if (sign !== '-') newN++;
    }
    // A line without newline at the end of the file ends both sides when it is context: with lines kept after it
    // (an added line, the old last line turned context), it is the removed line, then the same line added with its
    // newline, as git would write it.
    const nl = lines.indexOf('\\ No newline at end of file');
    if (nl > 0 && nl < lines.length - 1 && lines[nl - 1][0] === ' ') {
      const text = lines[nl - 1].slice(1);
      lines.splice(nl - 1, 2, `-${text}`, '\\ No newline at end of file', `+${text}`);
    }
    // The side the patch is matched on keeps its place; the other moves with the lines kept before. A side with no
    // line starts at the line before (git's convention).
    const [o0, oc, n0, nc] = [+m[1], m[2] === undefined ? 1 : +m[2], +m[3], m[4] === undefined ? 1 : +m[4]];
    let oldStart = o0, newStart = n0;
    if (!reverse) { const first = (oc ? o0 : o0 + 1) + delta; newStart = newN ? first : first - 1; }
    else { const first = (nc ? n0 : n0 + 1) - delta; oldStart = oldN ? first : first - 1; }
    delta += newN - oldN;
    body.push(`@@ -${oldStart},${oldN} +${newStart},${newN} @@${m[5]}`, ...lines);
  }
  return body.length ? [...head, ...body, ''].join('\n') : null;
}

// Runs git in top, input on its standard input: { code, out, err }.
function git(top, env, args, input = '') {
  return new Promise(resolve => {
    if (!GIT) return resolve({ code: -1, out: '', err: 'git not found' });
    const p = spawn(GIT, args, { cwd: top, env, windowsHide: true });
    let out = '', err = '';
    p.stdout.on('data', d => { out += d; });
    p.stderr.on('data', d => { err += d; });
    p.on('error', e => resolve({ code: -1, out, err: e.message }));
    p.on('close', code => resolve({ code, out, err }));
    p.stdin.on('error', () => { });
    p.stdin.end(input);
  });
}

const VERB = { stage: 'Staged', unstage: 'Unstaged', discard: 'Discarded' };
const plural = (n, w) => `${n} ${w}${n > 1 ? 's' : ''}`;

// What targets take: '2 files' (whole ones), '3 lines', '1 file' (without lines, a binary file).
function summary(list) {
  const lines = list.reduce((n, t) => n + t.keep.size, 0);
  return list.every(t => t.all) && list.length > 1 ? plural(list.length, 'file') : lines ? plural(lines, 'line') : plural(list.length, 'file');
}
// What a selection would take, for the question before discarding; '' for nothing.
function count(parsed, sel, mode) {
  const list = targets(parsed, sel, mode);
  return list.length ? summary(list) : '';
}

// Does it: top, the repository's folder, which the diff's paths are relative to. Returns the message to show.
async function apply(top, parsed, sel, mode) {
  const list = targets(parsed, sel, mode);
  if (!list.length) return mode === 'unstage' ? 'Nothing staged selected' : 'Nothing unstaged selected';
  const env = { ...await gitEnv(top), GIT_LITERAL_PATHSPECS: '1' };
  const fail = res => `Cannot ${mode}: ${(res.err || res.out).trim().split('\n')[0] || 'git failed'}`;
  const patches = [], whole = [], ita = [], remove = [];
  for (const t of list) {
    const name = t.f.to || t.f.from;
    // An untracked file: added whole with git add, removed whole from the disk; part of it goes through the index.
    const untracked = t.f.sec === 'unstaged' && t.f.status === 'added';
    if (!t.f.hunks.length || (untracked && t.all)) (untracked && mode === 'discard' ? remove : whole).push(name);
    else {
      if (untracked && mode === 'stage') ita.push(name);
      patches.push(patchOf(t, mode));
    }
  }
  if (ita.length) {
    const res = await git(top, env, ['add', '-N', '--', ...ita]);
    if (res.code !== 0) return fail(res);
  }
  const patch = patches.filter(Boolean).join('');
  if (patch) {
    const res = await git(top, env, ['apply', ...(mode === 'discard' ? [] : ['--cached']), ...(mode === 'stage' ? [] : ['-R']),
      '--whitespace=nowarn', '-'], patch);
    // The files git add -N put in the index for it go back to untracked.
    if (res.code !== 0) { if (ita.length) await git(top, env, ['rm', '--cached', '-q', '--', ...ita]); return fail(res); }
  }
  if (whole.length) {
    const hasHead = (await git(top, env, ['rev-parse', '--verify', '-q', 'HEAD'])).code === 0;
    const args = mode === 'stage' ? ['add', '-A', '--'] : mode === 'unstage' ? (hasHead ? ['reset', '-q', '--'] : ['rm', '--cached', '-q', '--'])
      : ['checkout', '-q', '--'];
    const res = await git(top, env, [...args, ...whole]);
    if (res.code !== 0) return fail(res);
  }
  for (const name of remove) {
    try { fs.rmSync(path.join(top, name)); } catch (e) { return `Cannot discard: ${e.code || e.message}`; }
  }
  return `${VERB[mode]} ${summary(list)}`;
}

module.exports = { SECTION, picked, targets, patchOf, quote, count, apply };
