// Staging, unstaging and discarding parts of a diff, from the Changes pane and the preview's Diff view: the rows
// selected give whole files, hunks or lines, and a patch made from the parsed diff (diff.js parse) goes through
// git apply: --cached for the index, -R to take changes back.
//   stage    lines of the Unstaged section, added to the index
//   unstage  lines of the Staged section, taken out of the index
//   discard  lines of the Unstaged section, taken out of the working tree (the caller confirms first)
'use strict';
const fs = require('fs');
const path = require('path');
const { WIN, GIT, gitEnv, exec } = require('./exec');

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
const NO_NL = '\\ No newline at end of file';

// The lines of a hunk in the order the patch takes them. A run of changes kept in part pairs its removed and added
// lines (the first removed with the first added, and so on), as git add -p does: a removed line left out, turned
// context, then stays next to the line that replaced it instead of moving after all the added ones. A "\ No newline
// at end of file" stays after its line.
function order(hunk, keep) {
  const out = [];
  for (let i = 0; i < hunk.length;) {
    if (!isChange(hunk[i])) { out.push(hunk[i++]); continue; }
    const start = i, rem = [], add = [];
    while (i < hunk.length && hunk[i][0] !== ' ') {
      const item = [hunk[i++]];
      if (i < hunk.length && hunk[i][0] === '\\') item.push(hunk[i++]);
      (item[0][0] === '-' ? rem : add).push(item);
    }
    const run = hunk.slice(start, i).filter(isChange);
    if (!rem.length || !add.length || run.every(l => keep.has(l)) || !run.some(l => keep.has(l))) { out.push(...hunk.slice(start, i)); continue; }
    for (let k = 0; k < Math.max(rem.length, add.length); k++) out.push(...(rem[k] || []), ...(add[k] || []));
  }
  return out;
}

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
    for (const l of order(h.lines, keep)) {
      const [s, text, , , , cr] = l;
      // "\ No newline at end of file" follows the line it is about, when that line is in the patch.
      if (s === '\\') { if (shown) lines.push(NO_NL); continue; }
      const sign = s === ' ' || keep.has(l) ? s : (s === '-') !== reverse ? ' ' : null;
      shown = sign !== null;
      if (!shown) continue;
      lines.push(sign + text + (cr ? '\r' : ''));
      if (sign !== '+') oldN++;
      if (sign !== '-') newN++;
    }
    // A removed or added line keeps its marker only while it ends its side: in reverse, an added line not chosen
    // turns context after the removed one, which then gets its newline back.
    for (let i = lines.length - 1; i > 0; i--) {
      const s = lines[i - 1][0];
      if (lines[i][0] !== '\\' || s === ' ' || s === '\\') continue;
      if (lines.slice(i + 1).some(l => l[0] === ' ' || l[0] === s)) lines.splice(i, 1);
    }
    // A line without newline at the end of the file ends both sides when it is context: with lines kept after it
    // (an added line, the old last line turned context), it is the removed line, then the same line added with its
    // newline, as git would write it.
    const nl = lines.indexOf(NO_NL);
    if (nl > 0 && nl < lines.length - 1 && lines[nl - 1][0] === ' ') {
      const text = lines[nl - 1].slice(1);
      lines.splice(nl - 1, 2, `-${text}`, NO_NL, `+${text}`);
    }
    // The last lines of the file taken out leave the line before them last: without newline when the side made had
    // none (its last line, left out, had the marker), as the removed line, then the same line added without it.
    const gone = reverse ? '+' : '-', made = reverse ? '-' : '+';
    if (lines[lines.length - 1] === NO_NL && lines[lines.length - 2][0] === gone) {
      let j = lines.length - 2;
      while (j >= 0 && lines[j][0] === gone) j--;
      if (j >= 0 && lines[j][0] === ' ' && h.lines.some((l, k) => l[0] === made && !keep.has(l) && h.lines[k + 1] && h.lines[k + 1][0] === '\\')) {
        const text = lines[j].slice(1);
        if (reverse) lines.splice(j, 1, `-${text}`, NO_NL, `+${text}`);
        else { lines.splice(j, 1, `-${text}`); lines.push(`+${text}`, NO_NL); }
      }
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

// Runs git in top, input on its standard input: { code, out, err }, code null when it could not start or took more
// than 2 minutes.
const git = (top, env, args, input = '') => exec(GIT, args, { cwd: top, env, input, timeout: 120000 });

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
// Untracked files discarded whole go to the trash (opts.trash, actions.trash by default); without one, they are
// deleted for good only when opts.noTrash(names) says so, and kept otherwise.
async function apply(top, parsed, sel, mode, opts = {}) {
  const list = targets(parsed, sel, mode);
  if (!list.length) return mode === 'unstage' ? 'Nothing staged selected' : 'Nothing unstaged selected';
  const env = { ...await gitEnv(top), GIT_LITERAL_PATHSPECS: '1' };
  const why = res => (res.err || res.out).trim().split('\n')[0] || 'git failed';
  const fail = res => `Cannot ${mode}: ${why(res)}`;
  const nameOf = t => t.f.to || t.f.from;
  const patches = [], whole = [], ita = [], remove = [], unindex = [];
  // Added files the index already has (git add -N by the user): their entry is kept on a failure, and goes with the
  // file on a discard.
  const added = list.filter(t => t.f.sec === 'unstaged' && t.f.status === 'added').map(t => t.f.to || t.f.from);
  const indexed = new Set(added.length ? (await git(top, env, ['ls-files', '-z', '--', ...added])).out.split('\0') : []);
  for (const t of list) {
    const name = t.f.to || t.f.from;
    // An untracked file: added whole with git add, removed whole from the disk; part of it goes through the index.
    const untracked = t.f.sec === 'unstaged' && t.f.status === 'added';
    if (!t.f.hunks.length || (untracked && t.all)) {
      (untracked && mode === 'discard' ? remove : whole).push(name);
      if (untracked && mode === 'discard' && indexed.has(name)) unindex.push(name);
    } else {
      if (untracked && mode === 'stage' && !indexed.has(name)) ita.push(name);
      patches.push(patchOf(t, mode));
    }
  }
  if (ita.length) {
    const res = await git(top, env, ['add', '-N', '--', ...ita]);
    if (res.code !== 0) return fail(res);
  }
  const patch = patches.filter(Boolean).join(''), patched = list.filter(t => !whole.includes(nameOf(t)) && !remove.includes(nameOf(t)));
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
    // The patch already applied stays: said, not undone.
    if (res.code !== 0) return patch ? `${VERB[mode]} ${summary(patched)}; cannot ${mode} ${whole.length > 1 ? plural(whole.length, 'file') : whole[0]}: ${why(res)}` : fail(res);
  }
  // The files deleted for good that could not be: the others are still deleted, and their index entry goes.
  const failed = new Map();
  // Listed by the diff, files all: one made a folder since is not taken with what it now holds.
  const left = await removeFiles(top, remove, failed, opts.trash || require('./actions').trash, true);
  // Checked again by removeFiles: a folder may have been made a link while the question was open.
  if (left.length && opts.noTrash && await opts.noTrash(left)) await removeFiles(top, left.splice(0), failed, forGood, true);
  const gone = unindex.filter(name => !left.includes(name) && !failed.has(name));
  if (gone.length) {
    const res = await git(top, env, ['rm', '--cached', '-q', '--', ...gone]);
    if (res.code !== 0) return fail(res);
  }
  if (!left.length && !failed.size) return `${VERB[mode]} ${summary(list)}`;
  // What went through, then what did not.
  const done = list.filter(t => !failed.has(nameOf(t)) && !left.includes(nameOf(t)));
  const said = [done.length && `${VERB[mode]} ${summary(done)}`, left.length && `${plural(left.length, 'untracked file')} kept: no trash`,
    failed.size && `cannot ${cannotDelete(failed)}`].filter(Boolean).join('; ');
  return said[0].toUpperCase() + said.slice(1);
}

// Deletes the repository's files names (paths from its folder top, a folder listed as nested/) with del (abs, folder
// → true once deleted, false when it could not, as the trash; for good when left out): those refused (refused) or
// failing are set in failed (name → reason), those already gone are left out, and those del did not take returned,
// but for folders, set in failed as never going for good (folderless). The folders they leave empty go too. files:
// all of them listed as files, a folder among them refused.
async function removeFiles(top, names, failed, del = forGood, files = false) {
  const left = [];
  for (const name of names) {
    const { abs, folder } = located(top, name), kind = folder || (files ? false : undefined), why = refused(abs, top, kind);
    if (why === 'gone') continue;
    if (why) { failed.set(name, why); continue; }
    let done;
    try { done = await del(abs, folder); } catch (e) { failed.set(name, e.code || e.message); continue; }
    // Gone meanwhile (the trash command then fails): nothing left to delete.
    if (!done && refused(abs, top, kind) === 'gone') done = true;
    if (done) pruneDirs(abs, top); else left.push(name);
  }
  return folderless(top, left, failed);
}

// Deletes the file abs for good, for removeFiles: a file, or a link itself (actions.unlink). Never a folder with what
// it holds (an embedded repository and its history, or a file made a folder since it was listed): the trash only;
// unlink fails on one made a folder after the check.
async function forGood(abs) {
  const st = fs.lstatSync(abs, { throwIfNoEntry: false });
  if (!st) return true;
  if (st.isDirectory()) throw new Error(NO_TRASH_FOLDER);
  try { require('./actions').unlink(abs, st); } catch (e) { if (e.code !== 'ENOENT') throw e; }
  return true;
}

const NO_TRASH_FOLDER = 'a folder, never deleted for good';

// The names (from removeFiles' left) that may be asked about for good: the folders set in failed instead.
function folderless(top, names, failed) {
  return names.filter(name => {
    let st;
    try { st = fs.lstatSync(located(top, name).abs, { throwIfNoEntry: false }); } catch { }
    if (st?.isDirectory()) { failed.set(name, NO_TRASH_FOLDER); return false; }
    return true;
  });
}

// What failed (name → reason, from removeFiles) says: delete <name or count>: <the first reason>.
function cannotDelete(failed) {
  const [name, why] = [...failed][0];
  return `delete ${failed.size > 1 ? plural(failed.size, 'file') : name}: ${why}`;
}

// The folders left empty by the file abs gone, up to the repository's folder top (kept).
function pruneDirs(abs, top) {
  const root = path.resolve(top);
  for (let d = path.dirname(path.resolve(abs)); d.startsWith(root + path.sep); d = path.dirname(d)) {
    try { fs.rmdirSync(d); } catch { break; }
  }
}

// The repository's file name (a path from its folder top, a folder listed as nested/): abs, its path without the final
// separator, which would have lstat follow a folder made a link, and folder, whether it is listed as one. A backslash
// is a separator on Windows only: elsewhere, the last character of a file name.
function located(top, name) {
  const sep = WIN ? /[\\/]+$/ : /\/+$/;
  return { abs: path.join(top, name).replace(sep, ''), folder: sep.test(name) };
}

// Why the repository's file abs is not to be deleted as the file of that name: 'gone' once it is no longer there (its
// folder too, or made a file), else the reason to show (outside the repository, or reached through a symbolic link:
// a folder made a link since the diff was read would lead to another file), or null when it can go. folder: true when
// listed as a folder, which must still be one (not a link to one), false as a file, which must not be one.
function refused(abs, top, folder) {
  if (!inside(abs, top)) return 'outside the repository';
  const link = linked(abs, top);
  if (link) return link;
  let st;
  try { st = fs.lstatSync(abs, { throwIfNoEntry: false }); } catch (e) { return e.code === 'ENOTDIR' ? 'gone' : e.code || e.message; }
  if (!st) return 'gone';
  if (folder === true && !st.isDirectory()) return 'no longer a folder';
  return folder === false && st.isDirectory() ? 'no longer a file' : null;
}

// Whether the file abs is in the repository's folder top, its folder too once symbolic links are followed (an index
// naming sub/x while sub links to the home folder is not). A folder gone is judged by the nearest one left; one that
// is there but cannot be followed (a link to nothing) is not in it.
function inside(abs, top) {
  const real = p => { try { return fs.realpathSync(p); } catch { return null; } };
  const root = real(top);
  if (!root || !path.resolve(abs).startsWith(path.resolve(top) + path.sep)) return false;
  for (let d = path.dirname(path.resolve(abs)); ; d = path.dirname(d)) {
    const dir = real(d);
    if (dir) return dir === root || dir.startsWith(root + path.sep);
    // Up to the root of the drive without one to follow (top resolved, so only through a folder that cannot be read).
    if (d === path.dirname(d)) return false;
    // A folder higher up made a file: this one cannot be there either.
    try { if (fs.lstatSync(d, { throwIfNoEntry: false })) return false; } catch (e) { if (e.code !== 'ENOTDIR') return false; }
  }
}

// Why a folder between top and the file abs keeps it from being reached as is: 'through a symbolic link' when one
// is a link (the file itself may be one), the error when one cannot be read (EACCES), else null.
function linked(abs, top) {
  let d = path.resolve(top);
  for (const part of path.relative(d, path.dirname(path.resolve(abs))).split(path.sep).filter(Boolean)) {
    d = path.join(d, part);
    let st;
    // A folder higher up made a file: nothing there to reach.
    try { st = fs.lstatSync(d, { throwIfNoEntry: false }); } catch (e) { return e.code === 'ENOTDIR' ? null : e.code || e.message; }
    if (!st) return null;
    if (st.isSymbolicLink()) return 'through a symbolic link';
  }
  return null;
}

module.exports = { SECTION, picked, targets, patchOf, quote, count, apply, pruneDirs, located, refused, removeFiles, cannotDelete };
