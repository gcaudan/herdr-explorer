// Terminal helpers shared by the plugin's panes: colours, text width, lines, input decoding, a one-line text
// field, and the alternate screen setup.
'use strict';

const fg = rgb => `\x1b[38;2;${rgb.join(';')}m`;
const bg = rgb => `\x1b[48;2;${rgb.join(';')}m`;
const glyph = code => String.fromCodePoint(code);

// Colours from the VS Code Dark+ theme (git decorations) and the Seti theme (icons).
const C = {
  yellow: [203, 203, 65], blue: [81, 154, 186], orange: [227, 121, 51], green: [141, 193, 73],
  purple: [160, 116, 196], red: [204, 62, 68], grey: [109, 128, 134], folder: [220, 182, 122],
  dim: [140, 140, 140], text: [212, 212, 212], select: [4, 57, 94], selectBlur: [55, 55, 61],
  header: [37, 37, 38], added: [35, 60, 36], removed: [72, 30, 30], find: [120, 90, 20],
};
const GIT = {
  M: [226, 192, 141], A: [129, 184, 139], U: [115, 201, 145], R: [115, 201, 145],
  D: [199, 78, 57], C: [228, 103, 107], '!': [140, 140, 140],
};

// Control characters from file names or contents are never written to the terminal: they could move the
// cursor, change colours or hide text.
const CONTROL = /[\x00-\x1f\x7f-\x9f]/g;
const clean = text => text.replace(CONTROL, '?');
// Tabs expanded to spaces, other control characters removed: for file contents.
function cleanLine(text, tab = 4) {
  if (!/[\x00-\x1f\x7f-\x9f]/.test(text)) return text;
  let s = '';
  for (const ch of text) {
    if (ch === '\t') s += ' '.repeat(tab - (Array.from(s).length % tab));
    else if (!/[\x00-\x1f\x7f-\x9f]/.test(ch)) s += ch;
  }
  return s;
}

// Terminal columns of a character: none for combining marks (accents of decomposed names), two for wide
// CJK characters and emoji.
const WIDE = /[ᄀ-ᅟ⺀-〾ぁ-㏿㐀-䶿一-鿿ꀀ-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦\u{1f300}-\u{1f64f}\u{1f900}-\u{1f9ff}\u{20000}-\u{3fffd}]/u;
const cols = ch => (/[\p{M}​‍️]/u.test(ch) ? 0 : WIDE.test(ch) ? 2 : 1);

// The longest start of a text that fits in max columns, and its width.
function fit(text, max) {
  let s = '', w = 0;
  for (const ch of text) {
    const c = cols(ch);
    if (w + c > max) break;
    s += ch; w += c;
  }
  return [s, w];
}
const textWidth = text => fit(text, Infinity)[1];
// Cut to max columns with an ellipsis when too long.
const ellipsis = (text, max) => (textWidth(text) <= max ? text : fit(text, Math.max(0, max - 1))[0] + '…');

// A screen line of exactly width columns. segments: [text, style sequence]; the text is truncated to the width,
// control characters replaced; a style may set a background, reset after its segment. back: background colour
// of the whole line.
function line(segments, width, back) {
  let s = back ? bg(back) : '', used = 0;
  for (const [text, style] of segments) {
    const [part, w] = fit(clean(text), width - used);
    if (!part) continue;
    s += (style || '') + part + '\x1b[0m' + (back ? bg(back) : '');
    used += w;
  }
  return s + ' '.repeat(Math.max(0, width - used)) + '\x1b[0m';
}

// Splits a plain text into lines of at most width columns, at spaces when possible.
function wrap(text, width) {
  width = Math.max(1, width);
  const lines = [];
  for (const para of text.split('\n')) {
    let cur = '', w = 0;
    for (const word of para.split(/(?<=\s)/)) {
      const ww = textWidth(word);
      if (w + ww <= width) { cur += word; w += ww; continue; }
      if (cur) { lines.push(cur.trimEnd()); cur = ''; w = 0; }
      let rest = word;
      while (textWidth(rest) > width) {
        const [part] = fit(rest, width);
        lines.push(part);
        rest = rest.slice(part.length);
      }
      cur = rest; w = textWidth(rest);
    }
    lines.push(cur.trimEnd());
  }
  return lines;
}

// --- segments: [text, style] lists, as given to line()

const segWidth = segs => segs.reduce((w, [text]) => w + textWidth(text), 0);
const segText = segs => segs.map(([text]) => text).join('');

// The columns [from, from + width) of a segment list, for horizontal scrolling. A wide character cut by the left
// edge leaves a space, so that the text keeps its columns.
function slice(segs, from, width) {
  const res = [];
  let col = 0;
  for (const [text, style] of segs) {
    let part = '';
    for (const ch of text) {
      const c = cols(ch);
      if (col >= from && col + c <= from + width) part += ch;
      else if (col < from && col + c > from) part += ' '.repeat(Math.max(0, Math.min(col + c - from, width)));
      col += c;
      if (col >= from + width) break;
    }
    if (part) res.push([part, style]);
    if (col >= from + width) break;
  }
  return res;
}

// A segment list cut into lines of at most width columns.
function split(segs, width) {
  const lines = [[]];
  let w = 0;
  for (const [text, style] of segs) {
    let part = '';
    for (const ch of text) {
      const c = cols(ch);
      if (w + c > width && w > 0) {
        if (part) lines[lines.length - 1].push([part, style]);
        lines.push([]); part = ''; w = 0;
      }
      part += ch; w += c;
    }
    if (part) lines[lines.length - 1].push([part, style]);
  }
  return lines;
}

// The character ranges [start, end) of the segments' text restyled with style added (search matches).
function mark(segs, ranges, style) {
  if (!ranges.length) return segs;
  const res = [];
  let pos = 0;
  for (const [text, base] of segs) {
    let cut = 0;
    const end = pos + text.length;
    const points = new Set();
    for (const [a, b] of ranges) {
      if (a > pos && a < end) points.add(a - pos);
      if (b > pos && b < end) points.add(b - pos);
    }
    for (const p of [...points].sort((a, b) => a - b).concat(text.length)) {
      if (p <= cut) continue;
      const at = pos + cut;
      const hit = ranges.some(([a, b]) => at >= a && at < b);
      res.push([text.slice(cut, p), hit ? base + style : base]);
      cut = p;
    }
    pos = end;
  }
  return res;
}

// --- input

// Escape sequences from the terminal and their key names. Printable characters are their own name;
// control characters are ctrl+a … ctrl+z; ESC followed by a character is alt+<character>.
const SEQ = {
  '\x1b[A': 'up', '\x1b[B': 'down', '\x1b[C': 'right', '\x1b[D': 'left', '\x1bOA': 'up', '\x1bOB': 'down',
  '\x1bOC': 'right', '\x1bOD': 'left', '\x1b[H': 'home', '\x1b[F': 'end', '\x1bOH': 'home', '\x1bOF': 'end',
  '\x1b[1~': 'home', '\x1b[4~': 'end', '\x1b[7~': 'home', '\x1b[8~': 'end', '\x1b[2~': 'insert', '\x1b[3~': 'delete',
  '\x1b[5~': 'pageup', '\x1b[6~': 'pagedown', '\x1b[Z': 'shift+tab', '\x1b[29~': 'menu', '\x1b[21;2~': 'shift+f10',
  '\x1b[1;5A': 'ctrl+up', '\x1b[1;5B': 'ctrl+down', '\x1b[1;5C': 'ctrl+right', '\x1b[1;5D': 'ctrl+left',
  '\x1b[1;2A': 'shift+up', '\x1b[1;2B': 'shift+down', '\x1b[1;3A': 'alt+up', '\x1b[1;3B': 'alt+down',
  '\x1b[1;3C': 'alt+right', '\x1b[1;3D': 'alt+left', '\x1b[1;5H': 'ctrl+home', '\x1b[1;5F': 'ctrl+end',
  '\x1bOP': 'f1', '\x1bOQ': 'f2', '\x1bOR': 'f3', '\x1bOS': 'f4', '\x1b[11~': 'f1', '\x1b[12~': 'f2', '\x1b[15~': 'f5',
  '\r': 'enter', '\n': 'enter', '\t': 'tab', '\x7f': 'backspace', '\x08': 'backspace', '\x1b': 'escape',
  '\x1b\x7f': 'alt+backspace', '\x1b\r': 'alt+enter', ' ': 'space',
};
const TOKEN = /\x1b\[200~[\s\S]*?\x1b\[201~|\x1b\[<(\d+);(\d+);(\d+)([Mm])|\x1b\[([IO])|\x1b\[[\d;]*[A-Za-z~]|\x1bO[A-Za-z]|\x1b[^\x1b]|[\s\S]/gu;

function keyName(k) {
  if (SEQ[k]) return SEQ[k];
  if (k.length === 1 && k < ' ') return `ctrl+${String.fromCharCode(k.charCodeAt(0) + 96)}`;
  if (k.length === 2 && k[0] === '\x1b') return `alt+${k[1]}`;
  if (k.startsWith('\x1b')) return null; // unknown sequence
  return k;
}

// Decodes terminal input and calls the handlers:
//   key(name)                      'up', 'enter', 'ctrl+p', 'alt+x', or the typed character itself ('a', 'É')
//   mouse({ button, x, y, press, drag, wheel })   x and y from 1; wheel: -1 up, 1 down; drag: motion with a button
//   focus(bool)                    focus reports (mode 1004)
//   paste(text)                    bracketed paste (mode 2004), control characters removed
function decode(data, handlers) {
  for (const m of data.matchAll(TOKEN)) {
    const t = m[0];
    if (t.startsWith('\x1b[200~')) {
      if (handlers.paste) handlers.paste(t.slice(6, -6).replace(/\r\n?/g, '\n').replace(/[\x00-\x09\x0b-\x1f\x7f-\x9f]/g, ''));
      continue;
    }
    if (m[1] !== undefined) {
      if (!handlers.mouse) continue;
      const b = +m[1];
      const wheel = b & 64 ? ((b & 3) === 0 ? -1 : (b & 3) === 1 ? 1 : 0) : 0;
      handlers.mouse({ button: b & 3, mods: b & 28, x: +m[2], y: +m[3], press: m[4] === 'M', drag: !!(b & 32), wheel });
      continue;
    }
    if (m[5]) { if (handlers.focus) handlers.focus(m[5] === 'I'); continue; }
    const name = keyName(t);
    if (name && handlers.key) handlers.key(name);
  }
}

const printable = name => Array.from(name).length === 1 && name !== ' ' ? true : name === 'space';

// A one-line text field: value and cursor, edited with the usual keys.
class Field {
  constructor(value = '') { this.value = value; this.cursor = Array.from(value).length; }
  set(value) { this.value = value; this.cursor = Array.from(value).length; }
  // Returns true when the key changed the field or its cursor.
  key(name) {
    const chars = Array.from(this.value);
    const at = this.cursor;
    if (printable(name)) {
      chars.splice(at, 0, name === 'space' ? ' ' : name);
      this.cursor++;
    } else if (name === 'backspace') { if (!at) return true; chars.splice(at - 1, 1); this.cursor--; }
    else if (name === 'delete' || name === 'ctrl+d') chars.splice(at, 1);
    else if (name === 'left' || name === 'ctrl+b') this.cursor = Math.max(0, at - 1);
    else if (name === 'right' || name === 'ctrl+f') this.cursor = Math.min(chars.length, at + 1);
    else if (name === 'home' || name === 'ctrl+a') this.cursor = 0;
    else if (name === 'end' || name === 'ctrl+e') this.cursor = chars.length;
    else if (name === 'ctrl+u') { chars.splice(0, at); this.cursor = 0; }
    else if (name === 'ctrl+k') chars.splice(at);
    else if (name === 'ctrl+w' || name === 'alt+backspace') {
      let i = at;
      while (i > 0 && chars[i - 1] === ' ') i--;
      while (i > 0 && chars[i - 1] !== ' ') i--;
      chars.splice(i, at - i); this.cursor = i;
    } else return false;
    this.value = chars.join('');
    return true;
  }
  insert(text) { for (const ch of text.replace(/\n/g, ' ')) this.key(ch === ' ' ? 'space' : ch); }
  // Segments for line(): the visible part of the value in width columns, the cursor shown in reverse video.
  segments(width, style = '') {
    const chars = Array.from(this.value);
    let start = 0;
    while (start < this.cursor && textWidth(chars.slice(start, this.cursor + 1).join('')) > width - 1) start++;
    const before = chars.slice(start, this.cursor).join('');
    const under = chars[this.cursor] || ' ';
    const after = chars.slice(this.cursor + 1).join('');
    return [[before, style], [under, '\x1b[7m'], [after, style]];
  }
}

// --- screen

// Alternate screen, hidden cursor, no line wrap, mouse (SGR), focus reports and bracketed paste;
// raw input decoded into handlers; everything restored on exit. Returns a paint function.
let off = '', child = null; // child: the process the terminal was given to by restart()
function screen(handlers, { mouse = true, motion = false, focus = true } = {}) {
  const out = process.stdout;
  const on = '\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h' + (mouse ? (motion ? '\x1b[?1002h' : '\x1b[?1000h') + '\x1b[?1006h' : '')
    + (focus ? '\x1b[?1004h' : '');
  off = (focus ? '\x1b[?1004l' : '') + (mouse ? '\x1b[?1006l\x1b[?1002l\x1b[?1000l' : '')
    + '\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l';
  out.write(on);
  process.on('exit', () => { if (!child) out.write(off); });
  // After a restart, passed on to the new process, which this one exits with.
  for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT']) {
    process.on(sig, () => {
      if (!child) process.exit(0);
      try { child.kill(sig); } catch { child.kill(); }
    });
  }
  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  process.stdin.setEncoding('utf8');
  // A long paste arrives in several reads: held until its end marker, so that its text never reads as keys.
  let pending = '';
  process.stdin.on('data', data => {
    data = pending + data;
    pending = '';
    const start = data.lastIndexOf('\x1b[200~');
    if (start >= 0 && data.indexOf('\x1b[201~', start) < 0) { pending = data.slice(start); data = data.slice(0, start); }
    if (data) decode(data, handlers);
  });
  if (handlers.resize) out.on('resize', handlers.resize);
  return lines => { if (!child) out.write('\x1b[?2026h\x1b[H' + lines.join('\r\n') + '\x1b[?2026l'); };
}

// Runs the pane's script again, on the code updated on disk, in the same terminal: the pane keeps its place, width
// and id. This process hands the terminal over, paints nothing more and exits with the new one. env: what the new
// process must show, added to this one's environment.
function restart(env) {
  if (child) return;
  process.stdout.write(off);
  // The terminal stays in raw mode, which the new process takes as it is: leaving it would start a line-mode read on
  // the shared console on Windows, and on Unix let a ctrl+c typed before the new process is ready kill it.
  process.stdin.removeAllListeners('data');
  process.stdin.pause();
  process.stdout.removeAllListeners('resize');
  child = require('child_process').spawn(process.execPath, process.argv.slice(1),
    { stdio: 'inherit', env: { ...process.env, ...env } });
  child.on('exit', code => process.exit(code ?? 0));
  child.on('error', () => process.exit(1));
}

const size = () => ({ W: process.stdout.columns || 80, H: process.stdout.rows || 24 });

module.exports = {
  fg, bg, glyph, C, GIT, clean, cleanLine, cols, fit, textWidth, ellipsis, line, wrap,
  segWidth, segText, slice, split, mark,
  keyName, decode, printable, Field, screen, restart, size,
};
