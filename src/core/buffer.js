// The text of a file edited in the preview (its edit mode, i): lines, a cursor and a selection, moves over wide
// characters, combining marks and tabs, edits with undo and redo, find and replace, the lines changed since the
// file was opened, and the file read and written back as it was (line endings, final newline, byte order mark).
'use strict';
const fs = require('fs');
const path = require('path');
const { cols } = require('./ui');

const MAX_BYTES = 2 << 20; // larger files are not edited
const TAB = 4;             // columns between tab stops

// --- the file

// A file's bytes as lines to edit: { lines, eol, final, bom, mixed }, or { error } for a file too large, binary (a NUL
// byte) or not UTF-8. eol: the line ending most lines have ('\n' or '\r\n'), written back on every line; final:
// whether the text ends with one; mixed: both kinds were found.
function parse(buf) {
  if (buf.length > MAX_BYTES) return { error: `over ${MAX_BYTES >> 20} MB` };
  if (buf.includes(0)) return { error: 'binary file' };
  let text;
  try { text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(buf); } catch { return { error: 'not UTF-8' }; }
  const bom = text.startsWith('\ufeff');
  if (bom) text = text.slice(1);
  const crlf = (text.match(/\r\n/g) || []).length, lf = (text.match(/\n/g) || []).length - crlf;
  const final = text.endsWith('\n');
  const lines = text.split(/\r?\n/);
  if (final) lines.pop();
  return { lines, eol: crlf > lf ? '\r\n' : '\n', final, bom, mixed: crlf > 0 && lf > 0 };
}

// The text to write: the lines joined with the file's line ending, its final one and byte order mark kept.
const serialize = ({ lines, eol, final, bom }) => (bom ? '\ufeff' : '') + lines.join(eol) + (final ? eol : '');

// The file read whole, as parse() gives it, with its stat: { doc, stat }, or { error }.
function read(file) {
  let fd;
  try {
    fd = fs.openSync(file, 'r');
    const stat = fs.fstatSync(fd);
    if (!stat.isFile()) return { error: 'not a regular file' };
    if (stat.size > MAX_BYTES) return { error: `over ${MAX_BYTES >> 20} MB` };
    const doc = parse(fs.readFileSync(fd));
    return doc.error ? doc : { doc, stat };
  } catch (e) { return { error: e.code || e.message }; } finally { if (fd !== undefined) fs.closeSync(fd); }
}

// The text written over the file at once: to a new temporary file beside it, with the file's permissions and group,
// then renamed over it. A link keeps pointing to it: its target is written. A file with other names (hard links), owned by
// another user, or in a folder that cannot be written, is written in place instead, which keeps all of that. Returns
// the file's new stat.
function save(file, text) {
  let real = file, stat = null;
  try { real = fs.realpathSync(file); stat = fs.statSync(real); } catch { }
  const mine = !stat || (stat.nlink <= 1 && (!process.getuid || stat.uid === process.getuid()));
  if (mine) {
    const tmp = path.join(path.dirname(real), `.${path.basename(real)}.${process.pid}-${Date.now().toString(36)}.tmp`);
    try {
      // wx: never through a file or link already there.
      fs.writeFileSync(tmp, text, { flag: 'wx', mode: stat ? stat.mode & 0o7777 : 0o666 });
      // Its group too (EPERM when not one of the user's: written in place), before chmod as chown clears setgid.
      if (stat && process.getuid && fs.statSync(tmp).gid !== stat.gid) fs.chownSync(tmp, stat.uid, stat.gid);
      if (stat) fs.chmodSync(tmp, stat.mode & 0o7777);
      fs.renameSync(tmp, real);
      return fs.statSync(real);
    } catch (e) {
      try { fs.unlinkSync(tmp); } catch { }
      if (!stat || !['EACCES', 'EPERM', 'EROFS', 'EXDEV', 'EBUSY'].includes(e.code)) throw e;
    }
  }
  fs.writeFileSync(real, text);
  return fs.statSync(real);
}

// --- characters and columns

// What joins the character before it, for the cursor: combining marks, zero-width spaces and joiners, variation
// selectors, skin tones; and any character after a zero-width joiner (👩‍💻). The two regional indicators of a
// flag go together.
const JOINS = /[\p{M}\u200b-\u200d\ufe00-\ufe0f\u{1f3fb}-\u{1f3ff}]/u;
const flagPart = cp => cp >= 0x1f1e6 && cp <= 0x1f1ff;
const CONTROL = /[\x00-\x1f\x7f-\x9f]/;

// Where the character after the one at i starts (i itself at the end).
function nextAt(text, i) {
  if (i >= text.length) return text.length;
  const cp = text.codePointAt(i);
  let j = i + (cp > 0xffff ? 2 : 1);
  if (flagPart(cp) && j < text.length && flagPart(text.codePointAt(j))) j += 2;
  while (j < text.length) {
    const ch = String.fromCodePoint(text.codePointAt(j));
    if (!JOINS.test(ch) && text.charCodeAt(j - 1) !== 0x200d) break;
    j += ch.length;
  }
  return j;
}
// Where the character before the one at i starts: found from the start of the line, where the boundaries are known.
function prevAt(text, i) {
  let p = 0;
  for (let q = nextAt(text, 0); q < i; q = nextAt(text, q)) p = q;
  return Math.min(p, i);
}
// The start of the character holding i (i itself when it is one).
function snap(text, i) {
  let p = 0;
  for (let q = 0; q <= i && q < text.length; q = nextAt(text, q)) p = q;
  return i >= text.length ? text.length : p;
}

// A line as the screen shows it: tabs as spaces to the next stop, other control characters as ? (one column).
// map[i]: where the character at i starts in the shown text; col[i]: its column; both with an entry for the end.
function shown(text) {
  const map = new Int32Array(text.length + 1), col = new Int32Array(text.length + 1);
  let out = '', w = 0, prev;
  for (let i = 0; i < text.length;) {
    const ch = String.fromCodePoint(text.codePointAt(i));
    map[i] = map[i + ch.length - 1] = out.length; col[i] = col[i + ch.length - 1] = w;
    if (ch === '\t') { const n = TAB - (w % TAB); out += ' '.repeat(n); w += n; prev = undefined; }
    else if (CONTROL.test(ch)) { out += '?'; w++; prev = undefined; }
    else { out += ch; w += cols(ch, prev); prev = ch; }
    i += ch.length;
  }
  map[text.length] = out.length; col[text.length] = w;
  return { text: out, map, col, width: w };
}
const colOf = (text, at) => shown(text).col[Math.min(at, text.length)];
// The character under a column (its start), or the end of the line past it.
function atCol(text, want) {
  const { col } = shown(text);
  let i = 0;
  while (i < text.length) {
    const j = nextAt(text, i);
    if (col[j] > want) break;
    i = j;
  }
  return i;
}

// Words for ctrl+← →, double-clicks: letters, digits and _; spaces; anything else.
const kind = ch => (/[\p{L}\p{N}\p{M}_$]/u.test(ch) ? 1 : /\s/.test(ch) ? 0 : 2);
const charAt = (text, i) => String.fromCodePoint(text.codePointAt(i));
// The end of the word after at, spaces before it skipped; the start of the one before it.
function wordEnd(text, at) {
  while (at < text.length && !kind(charAt(text, at))) at = nextAt(text, at);
  const k = at < text.length && kind(charAt(text, at));
  while (at < text.length && kind(charAt(text, at)) === k) at = nextAt(text, at);
  return at;
}
function wordStart(text, at) {
  while (at > 0 && !kind(charAt(text, prevAt(text, at)))) at = prevAt(text, at);
  const k = at > 0 && kind(charAt(text, prevAt(text, at)));
  while (at > 0 && kind(charAt(text, prevAt(text, at))) === k) at = prevAt(text, at);
  return at;
}

// The indentation a file uses: a tab when most indented lines start with one, else spaces, as many as the smallest
// indentation of two or more (4 when none).
function indentOf(lines) {
  let tabs = 0, spaces = 0, least = Infinity;
  for (const l of lines.length > 5000 ? lines.slice(0, 5000) : lines) {
    if (l[0] === '\t') tabs++;
    else if (l[0] === ' ') {
      spaces++;
      const n = l.search(/[^ ]/);
      if (n >= 2 && n < least) least = n;
    }
  }
  return tabs > spaces ? '\t' : ' '.repeat(least <= 8 ? least : 4);
}

// --- lines changed

// The lines of b changed from a: kinds[i] 'add' or 'mod' (or ''), and deleted, the lines of b before which lines of a
// were removed (b.length: at the end). The lines both start and end with are skipped, then the rest compared line by
// line (myers), at most limit differences: past it, all of it counts as changed.
function lineDiff(a, b, limit = 1000) {
  const kinds = new Array(b.length).fill(''), deleted = new Set();
  let s = 0, ea = a.length, eb = b.length;
  while (s < ea && s < eb && a[s] === b[s]) s++;
  while (ea > s && eb > s && a[ea - 1] === b[eb - 1]) { ea--; eb--; }
  // Lines as numbers, quicker to compare.
  const ids = new Map(), id = l => { let v = ids.get(l); if (v === undefined) ids.set(l, v = ids.size); return v; };
  const x = a.slice(s, ea).map(id), y = b.slice(s, eb).map(id);
  const same = myers(x, y, limit) || [];
  let i = 0, j = 0;
  for (const [pi, pj] of [...same, [x.length, y.length]]) {
    const add = pj - j, del = pi - i, at = s + j;
    for (let k = 0; k < add; k++) kinds[at + k] = k < del ? 'mod' : 'add';
    if (del > add) deleted.add(at + add);
    i = pi + 1; j = pj + 1;
  }
  return { kinds, deleted };
}

// Myers' diff of two lists of numbers: the pairs [i, j] of equal items it keeps, in order; null past limit differences.
function myers(a, b, limit) {
  const n = a.length, m = b.length, max = n + m, off = max + 1;
  if (!n || !m) return [];
  const v = new Int32Array(2 * max + 3), trace = [];
  for (let d = 0; d <= max && d <= limit; d++) {
    // The diagonals d can reach from, kept for the way back.
    trace.push(v.slice(off - d - 1, off + d + 2));
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[off + k - 1] < v[off + k + 1]) ? v[off + k + 1] : v[off + k - 1] + 1, y = x - k;
      while (x < n && y < m && a[x] === b[y]) { x++; y++; }
      v[off + k] = x;
      if (x >= n && y >= m) return back(trace, n, m);
    }
  }
  return null;
}
function back(trace, n, m) {
  const pairs = [];
  let x = n, y = m;
  for (let d = trace.length - 1; d >= 0; d--) {
    const t = trace[d], at = k => t[k + d + 1], k = x - y;
    const pk = k === -d || (k !== d && at(k - 1) < at(k + 1)) ? k + 1 : k - 1;
    const px = at(pk), py = px - pk;
    while (x > px && y > py && x > 0 && y > 0) { pairs.push([x - 1, y - 1]); x--; y--; }
    x = px; y = py;
  }
  return pairs.reverse();
}

// --- find

// The matches of a query in the lines: [{ line, a, b }], a and b the start and end in the line. Plain text,
// ignoring case unless the query has a capital letter, as the preview's find.
function matches(lines, query) {
  const out = [];
  if (!query) return out;
  const re = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), query !== query.toLowerCase() ? 'g' : 'gi');
  lines.forEach((l, line) => { for (const m of l.matchAll(re)) if (m[0]) out.push({ line, a: m.index, b: m.index + m[0].length }); });
  return out;
}

// --- the editor

// Positions are { line, at }: a line (from 0) and an offset in its text, always at the start of a character.
const cmp = (p, q) => p.line - q.line || p.at - q.at;
const copy = p => ({ line: p.line, at: p.at });
// Where a text put at p ends.
function endOf(p, text) {
  const parts = text.split('\n');
  return parts.length === 1 ? { line: p.line, at: p.at + text.length } : { line: p.line + parts.length - 1, at: parts[parts.length - 1].length };
}

class Editor {
  // doc: parse()'s result. original: the lines as opened, for changes().
  constructor(doc) {
    this.reset(doc);
  }
  // The text replaced by doc's (a reload): the cursor stays where it can, the history and changes start over.
  reset(doc) {
    Object.assign(this, { lines: doc.lines.length ? doc.lines.slice() : [''], eol: doc.eol, final: doc.final, bom: doc.bom, mixed: doc.mixed });
    this.original = this.lines.slice();
    this.indent = indentOf(this.lines);
    this.cur = this.clamp(this.cur || { line: 0, at: 0 });
    this.anchor = null;   // the selection's other end, null when none
    this.goal = null;     // the column ↑ ↓ keep, from the first of them
    this.undos = []; this.redos = [];
    this.id = 0;          // the last step's id
    this.saved = 0;       // the id of the step saved last (0: as opened)
    this.sealed = true;   // the next typing starts a step of its own
    this.version = (this.version || 0) + 1; // changes at each edit, for the caches
    this.diff = null;
  }
  get text() { return this.lines.join('\n'); }
  clamp(p) {
    const line = Math.max(0, Math.min(p.line, this.lines.length - 1));
    return { line, at: snap(this.lines[line], Math.max(0, Math.min(p.at, this.lines[line].length))) };
  }
  // The id of the step the text is at: equal to saved when nothing changed since the save.
  top() { return this.undos.length ? this.undos[this.undos.length - 1].id : 0; }
  dirty() { return this.top() !== this.saved; }
  markSaved() { this.saved = this.top(); this.sealed = true; }

  // The selection's ends in order, or null.
  selection() {
    if (!this.anchor || !cmp(this.anchor, this.cur)) return null;
    return cmp(this.anchor, this.cur) < 0 ? [this.anchor, this.cur] : [this.cur, this.anchor];
  }
  textOf(a, b) {
    if (a.line === b.line) return this.lines[a.line].slice(a.at, b.at);
    return [this.lines[a.line].slice(a.at), ...this.lines.slice(a.line + 1, b.line), this.lines[b.line].slice(0, b.at)].join('\n');
  }
  selected() { const s = this.selection(); return s ? this.textOf(s[0], s[1]) : ''; }

  // The text between a and b replaced, without history: returns where the new text ends.
  splice(a, b, text) {
    const head = this.lines[a.line].slice(0, a.at), tail = this.lines[b.line].slice(b.at);
    const parts = text.split('\n'), end = endOf(a, text);
    parts[0] = head + parts[0];
    parts[parts.length - 1] += tail;
    if (parts.length < 10000) this.lines.splice(a.line, b.line - a.line + 1, ...parts);
    else this.lines = [...this.lines.slice(0, a.line), ...parts, ...this.lines.slice(b.line + 1)];
    this.version++;
    return end;
  }
  // An edit of the current step (see step()): returns where the new text ends.
  replace(a, b, text) {
    const removed = this.textOf(a, b);
    if (!removed && !text) return copy(a);
    this.changes.push({ from: copy(a), removed, inserted: text });
    return this.splice(a, b, text);
  }
  // fn's edits as one step of the history. kind: typed characters ('type') and deletions ('back', 'del') of the same
  // kind join the step before them while the cursor stayed there; a space after a word starts a new one.
  step(kind, fn) {
    const before = { cur: copy(this.cur), anchor: this.anchor && copy(this.anchor) };
    this.changes = [];
    fn();
    const changes = this.changes;
    this.changes = null;
    if (!changes.length) return false;
    this.redos = [];
    this.goal = null;
    const last = this.undos[this.undos.length - 1];
    const word = kind === 'type' && /^\s/.test(changes[0].inserted) && last && !/\s$/.test(last.changes[last.changes.length - 1].inserted);
    if (kind && last && last.kind === kind && !this.sealed && last.id !== this.saved && !word) last.changes.push(...changes);
    else this.undos.push({ id: ++this.id, kind, changes, before });
    this.undos[this.undos.length - 1].after = { cur: copy(this.cur), anchor: null };
    this.sealed = !kind;
    return true;
  }
  undo() {
    const s = this.undos.pop();
    if (!s) return false;
    for (const c of [...s.changes].reverse()) this.splice(c.from, endOf(c.from, c.inserted), c.removed);
    this.redos.push(s);
    this.restore(s.before);
    return true;
  }
  redo() {
    const s = this.redos.pop();
    if (!s) return false;
    for (const c of s.changes) this.splice(c.from, endOf(c.from, c.removed), c.inserted);
    this.undos.push(s);
    this.restore(s.after);
    return true;
  }
  restore({ cur, anchor }) {
    this.cur = this.clamp(cur); this.anchor = anchor && this.clamp(anchor);
    this.goal = null; this.sealed = true;
  }

  // --- edits

  // Text typed or pasted in place of the selection. kind: 'type' for typed characters, which join in one step.
  type(text, kind = '') {
    return this.step(kind, () => {
      const [a, b] = this.selection() || [this.cur, this.cur];
      this.cur = this.replace(a, b, text);
      this.anchor = null;
    });
  }
  // Enter: a new line indented as the one the cursor leaves (up to the cursor).
  newline() {
    const [a] = this.selection() || [this.cur];
    const line = this.lines[a.line], lead = line.slice(0, Math.min(a.at, line.search(/\S|$/)));
    return this.type('\n' + lead);
  }
  // Backspace and delete: the selection, else the character before or after the cursor (or the word, with word),
  // joining two lines at their ends.
  erase(dir, word) {
    if (this.selection()) return this.type('');
    this.anchor = null;
    const p = this.cur, text = this.lines[p.line];
    let q;
    if (dir < 0) q = p.at ? { line: p.line, at: word ? wordStart(text, p.at) : prevAt(text, p.at) } : p.line ? { line: p.line - 1, at: this.lines[p.line - 1].length } : null;
    else q = p.at < text.length ? { line: p.line, at: word ? wordEnd(text, p.at) : nextAt(text, p.at) } : p.line < this.lines.length - 1 ? { line: p.line + 1, at: 0 } : null;
    if (!q) return false;
    return this.step(word ? '' : dir < 0 ? 'back' : 'del', () => {
      const [a, b] = dir < 0 ? [q, p] : [p, q];
      this.cur = this.replace(a, b, '');
    });
  }
  // Tab and shift+tab: without a selection over lines, tab puts the indentation at the cursor (spaces to the next
  // stop); otherwise each line of the selection (or the cursor's) gets one more level, or one less.
  indentLines(dir) {
    const s = this.selection();
    if (dir > 0 && (!s || s[0].line === s[1].line)) {
      const unit = this.indent === '\t' ? '\t' : ' '.repeat(this.indent.length - (colOf(this.lines[this.cur.line], this.cur.at) % this.indent.length));
      return this.type(unit);
    }
    const [a, b] = s || [this.cur, this.cur];
    // A selection ending at the start of a line leaves that line out.
    const last = b.line > a.line && b.at === 0 ? b.line - 1 : b.line;
    const shift = new Map();
    const done = this.step('', () => {
      for (let i = a.line; i <= last; i++) {
        const l = this.lines[i];
        if (dir > 0) {
          if (!l) continue;
          this.replace({ line: i, at: 0 }, { line: i, at: 0 }, this.indent);
          shift.set(i, this.indent.length);
        } else {
          const n = l[0] === '\t' ? 1 : Math.min(l.search(/[^ ]|$/), this.indent === '\t' ? TAB : this.indent.length);
          if (!n) continue;
          this.replace({ line: i, at: 0 }, { line: i, at: n }, '');
          shift.set(i, -n);
        }
      }
      const move = p => p && { line: p.line, at: Math.max(0, p.at + (shift.get(p.line) || 0)) };
      this.cur = move(this.cur); this.anchor = move(this.anchor);
    });
    if (done) this.undos[this.undos.length - 1].after = { cur: copy(this.cur), anchor: this.anchor && copy(this.anchor) };
    return done;
  }

  // --- moves

  // The cursor to p; extend: the selection grows to it (shift), else it goes.
  place(p, extend = false) {
    p = this.clamp(p);
    if (extend && !this.anchor) this.anchor = copy(this.cur);
    if (!extend) this.anchor = null;
    this.cur = p;
    this.sealed = true;
    this.goal = null;
  }
  // A move by name: left, right, up, down, home, end, pageup, pagedown (by page lines), wordleft, wordright, top,
  // bottom. Without extend, ← → first fold a selection to its start or end.
  move(name, extend = false, page = 10) {
    const p = this.cur, text = this.lines[p.line], s = this.selection();
    const vertical = n => {
      if (this.goal === null) this.goal = colOf(text, p.at);
      const line = p.line + n;
      if (line < 0) return { line: 0, at: 0 };
      if (line >= this.lines.length) return { line: this.lines.length - 1, at: this.lines[this.lines.length - 1].length };
      return { line, at: atCol(this.lines[line], this.goal) };
    };
    let to;
    switch (name) {
      case 'left':
        if (s && !extend) to = s[0];
        else to = p.at ? { line: p.line, at: prevAt(text, p.at) } : p.line ? { line: p.line - 1, at: this.lines[p.line - 1].length } : p;
        break;
      case 'right':
        if (s && !extend) to = s[1];
        else to = p.at < text.length ? { line: p.line, at: nextAt(text, p.at) } : p.line < this.lines.length - 1 ? { line: p.line + 1, at: 0 } : p;
        break;
      case 'wordleft': to = p.at ? { line: p.line, at: wordStart(text, p.at) } : p.line ? { line: p.line - 1, at: this.lines[p.line - 1].length } : p; break;
      case 'wordright': to = p.at < text.length ? { line: p.line, at: wordEnd(text, p.at) } : p.line < this.lines.length - 1 ? { line: p.line + 1, at: 0 } : p; break;
      // Home: the first character that is not a space, then the start of the line.
      case 'home': { const first = text.search(/\S|$/); to = { line: p.line, at: p.at === first ? 0 : first }; break; }
      case 'end': to = { line: p.line, at: text.length }; break;
      case 'top': to = { line: 0, at: 0 }; break;
      case 'bottom': to = { line: this.lines.length - 1, at: this.lines[this.lines.length - 1].length }; break;
      case 'up': case 'down': case 'pageup': case 'pagedown': {
        to = vertical({ up: -1, down: 1, pageup: -page, pagedown: page }[name]);
        const goal = this.goal;
        this.place(to, extend);
        this.goal = goal;
        return;
      }
      default: return;
    }
    this.place(to, extend);
  }
  selectAll() {
    this.anchor = { line: 0, at: 0 };
    this.cur = { line: this.lines.length - 1, at: this.lines[this.lines.length - 1].length };
    this.sealed = true;
  }
  // The word (or run of spaces, or character) under p selected: a double-click.
  selectWord(p) {
    p = this.clamp(p);
    const text = this.lines[p.line];
    if (p.at >= text.length) return this.place(p);
    const k = kind(charAt(text, p.at));
    let a = p.at, b = nextAt(text, p.at);
    if (k !== 2) {
      while (a > 0 && kind(charAt(text, prevAt(text, a))) === k) a = prevAt(text, a);
      while (b < text.length && kind(charAt(text, b)) === k) b = nextAt(text, b);
    }
    this.anchor = { line: p.line, at: a }; this.cur = { line: p.line, at: b }; this.sealed = true;
  }
  // The whole line under p, with its line break: a triple click.
  selectLine(p) {
    p = this.clamp(p);
    this.anchor = { line: p.line, at: 0 };
    this.cur = p.line < this.lines.length - 1 ? { line: p.line + 1, at: 0 } : { line: p.line, at: this.lines[p.line].length };
    this.sealed = true;
  }
  goto(line) { this.place({ line: line - 1, at: 0 }); }

  // --- find and replace

  // The next match of query after the cursor (dir 1), or the one before it (-1), going round, selected. from: the
  // cursor's own place counts (a find as the query is typed). Returns the match, or null.
  find(query, dir = 1, from = false) {
    const list = matches(this.lines, query);
    if (!list.length) return null;
    const s = this.selection() || [this.cur, this.cur];
    const start = x => ({ line: x.line, at: x.a });
    const m = dir > 0
      ? list.find(x => cmp(start(x), from ? s[0] : s[1]) >= 0) || list[0]
      : [...list].reverse().find(x => cmp(start(x), s[0]) < 0) || list[list.length - 1];
    this.anchor = { line: m.line, at: m.a }; this.cur = { line: m.line, at: m.b }; this.sealed = true; this.goal = null;
    return m;
  }
  // The selected match replaced by with, then the next one selected; without a match selected, the next one first.
  // Returns whether one was replaced.
  replaceOne(query, withText) {
    const s = this.selection();
    const on = s && s[0].line === s[1].line && matches([this.lines[s[0].line]], query).some(m => m.a === s[0].at && m.b === s[1].at);
    if (!on) { this.find(query, 1, true); return false; }
    this.type(withText);
    this.find(query, 1, true);
    return true;
  }
  // Every match replaced, as one step: returns how many.
  replaceAll(query, withText) {
    const list = matches(this.lines, query);
    if (!list.length) return 0;
    const cur = copy(this.cur);
    // From the last, so that the positions of the others still hold.
    this.step('', () => { for (const m of [...list].reverse()) this.replace({ line: m.line, at: m.a }, { line: m.line, at: m.b }, withText); });
    this.place(cur);
    return list.length;
  }

  // The lines changed since the file was opened (lineDiff), computed once per version.
  changed() {
    if (!this.diff || this.diff.version !== this.version) this.diff = { version: this.version, ...lineDiff(this.original, this.lines) };
    return this.diff;
  }
}

module.exports = { MAX_BYTES, TAB, parse, serialize, read, save, nextAt, prevAt, snap, shown, colOf, atCol, wordStart, wordEnd, indentOf, lineDiff, matches, Editor };
