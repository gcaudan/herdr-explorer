// Terminal helpers shared by the plugin's panes: colours, text width, lines, input decoding, a one-line text
// field, and the alternate screen setup.
'use strict';

const fg = rgb => `\x1b[38;2;${rgb.join(';')}m`;
const bg = rgb => `\x1b[48;2;${rgb.join(';')}m`;
const glyph = code => String.fromCodePoint(code);
// The magnifier (Nerd Font) before every find and filter field.
const SEARCH = glyph(0xf002);

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
// CJK characters, emoji (✅ ❌ included) and the trigrams ☰ (wide since Unicode 16, as herdr counts them).
// prev: the character before, for the emoji selector U+FE0F, which makes a text symbol such as ⚠ an emoji two
// columns wide. The two regional indicators of a flag (🇫🇷) take one column each, the flag two.
const WIDE = /(?!\p{Regional_Indicator})[ᄀ-ᅟ⺀-〾ぁ-㏿㐀-䶿一-鿿ꀀ-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦\u{20000}-\u{3fffd}\u2630-\u2637\u268a-\u268f\p{Emoji_Presentation}]/u;
const cols = (ch, prev) => (ch === '\ufe0f' && prev && /\p{Emoji}/u.test(prev) && !WIDE.test(prev) ? 1
  : /[\p{M}​‍️]/u.test(ch) ? 0 : WIDE.test(ch) ? 2 : 1);

// The longest start of a text that fits in max columns, and its width. prev: the character before the text, in
// the segment before.
function fit(text, max, prev) {
  let s = '', w = 0;
  for (const ch of text) {
    const c = cols(ch, prev);
    if (w + c > max) break;
    s += ch; w += c; prev = ch;
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
  let s = back ? bg(back) : '', used = 0, prev;
  for (const [text, style] of segments) {
    const [part, w] = fit(clean(text), width - used, prev);
    if (!part) continue;
    prev = Array.from(part).pop();
    s += (style || '') + part + '\x1b[0m' + (back ? bg(back) : '');
    used += w;
  }
  return s + ' '.repeat(Math.max(0, width - used)) + '\x1b[0m';
}

// Key hints: each key in bold, its action dimmed, as many pairs as fit in max columns (all without max).
// pairs: [key, action].
// segs.zones: the columns of each pair from the start of the segments, [{ x0, x1, key }], to click them.
function keys(pairs, max = Infinity) {
  const segs = [];
  segs.zones = [];
  let used = 0;
  for (const [k, action] of pairs) {
    const lead = segs.length ? '   ' : ' ';
    const w = textWidth(lead + k + ' ' + action);
    if (used + w > max) break;
    segs.push([lead + k, '\x1b[1m' + fg(C.text)], [` ${action}`, fg(C.dim)]);
    const key = keyOf(k);
    if (key) segs.zones.push({ x0: used + lead.length, x1: used + w, key });
    used += w;
  }
  return segs;
}

// The key a click on a hint sends: its first key, by its name for the key handlers; none for a range of keys
// (↑↓, 1-4) or arrows.
const SENDS = { '⏎': 'enter', '↵': 'enter', esc: 'escape', del: 'delete', pgup: 'pageup', pgdn: 'pagedown' };
function keyOf(label) {
  const k = label.split(' ')[0];
  return /[↑↓←→]/.test(k) || /^\d-\d$/.test(k) ? null : SENDS[k] || k;
}

// The key of the hint at column x (0-based) of a line whose hints start at column from, or null.
const keyAt = (segs, x, from = 0) => ((segs.zones || []).find(z => x >= from + z.x0 && x < from + z.x1) || {}).key || null;

// A key name as shown to the user: arrows and Enter as symbols, the long names shortened.
const SHORT = { up: '↑', down: '↓', left: '←', right: '→', enter: '⏎', escape: 'esc', pageup: 'pgup', pagedown: 'pgdn' };
const keyLabel = k => k.replace(/(^|\+)(up|down|left|right|enter|escape|pageup|pagedown)$/, (m, mod, name) => mod + SHORT[name]);

// The key help of a pane (? or f1), over the whole pane: the title, a find field once / is pressed, the keys by group
// (the keys on the left, what they do on the right), then a rule and the keys of the help. groups: [title, [[key,
// action]]]. state: { top, find } (find: a Field, null when closed); help() keeps in it the rows the list gets and the
// pane's size, for helpKey and helpMouse.
const HELP_KEYS = [['↑↓', 'scroll'], ['/', 'find'], ['esc', 'close']];
function helpRows(groups, state) {
  const q = state.find && state.find.value.toLowerCase();
  const out = [];
  for (const [g, pairs] of groups) {
    const kept = pairs.map(([k, a]) => [k.split(' ').map(keyLabel).join(' '), a])
      .filter(([k, a]) => !q || `${k} ${a}`.toLowerCase().includes(q));
    if (kept.length) out.push([g], ...kept);
  }
  return out;
}
function help(lines, W, title, groups, state) {
  const H = lines.length, rows = helpRows(groups, state);
  const at = state.find ? 2 : 1, n = Math.max(1, H - at - 2);
  state.top = Math.max(0, Math.min(state.top || 0, rows.length - n));
  Object.assign(state, { n, count: rows.length, W, H });
  // The keys' column: wide enough for most, a longer list of keys cut.
  const kw = Math.min(11, Math.floor(W / 2), Math.max(6, ...rows.filter(r => r.length === 2).map(([k]) => textWidth(k) + 2)));
  const out = [line([[' ‹ ', fg(C.dim)], ['Keys', '\x1b[1m'], [title ? ` · ${title}` : '', fg(C.dim)]], W)];
  if (state.find) {
    const count = state.find.value ? ` ${rows.filter(r => r.length === 2).length}` : '';
    const field = state.find.segments(Math.max(1, W - 4 - count.length));
    out.push(line([[` ${SEARCH} `, fg(C.dim)], ...field, [count.padStart(Math.max(0, W - 3 - segWidth(field))), fg(C.dim)]], W, C.header));
  }
  for (const r of rows.slice(state.top, state.top + n)) {
    if (r.length === 1) { out.push(line([[` ${r[0]}`, '\x1b[1m' + fg(C.blue)]], W)); continue; }
    const k = ellipsis(r[0], kw - 2);
    out.push(line([[`  ${k}${' '.repeat(Math.max(1, kw - textWidth(k)))}`, '\x1b[1m' + fg(C.text)], [r[1], fg(C.dim)]], W));
  }
  if (!rows.length) out.push(...empty(W, n, 'No match', [['esc', 'clear']]));
  while (out.length < H - 2) out.push(line([], W));
  const more = (state.top > 0 ? '▴' : '') + (state.top + n < rows.length ? '▾' : '');
  out.push(line([['─'.repeat(Math.max(0, W - more.length - 1)) + more, fg(C.dim)]], W));
  out.push(line(keys(HELP_KEYS, W), W, C.header));
  out.slice(0, H).forEach((l, i) => { lines[i] = l; });
}

const helpScroll = (state, by) => { state.top = Math.max(0, Math.min((state.count || 0) - (state.n || 1), (state.top || 0) + by)); };

// Keys of an open help: / opens the find, where characters are typed (Escape closes it); the arrows, page keys,
// home and end scroll. Returns false when the help is to close: Escape, q, ? or f1.
function helpKey(k, state) {
  if (state.find) {
    if (k === 'escape') { state.find = null; return true; }
    if (printable(k) || k === 'space' || k === 'backspace' || k === 'delete') { if (state.find.key(k)) state.top = 0; return true; }
  } else if (k === '/') { state.find = new Field(''); state.top = 0; return true; }
  else if (['escape', 'q', '?', 'f1'].includes(k)) return false;
  const page = Math.max(1, (state.n || 10) - 1);
  const move = { up: -1, k: -1, down: 1, j: 1, pageup: -page, pagedown: page, 'ctrl+u': -Math.ceil(page / 2), 'ctrl+d': Math.ceil(page / 2),
    home: -Infinity, g: -Infinity, end: Infinity, G: Infinity }[k];
  if (move) helpScroll(state, move);
  return true;
}

// Mouse on an open help: the wheel scrolls, a click on a key hint acts as its key. Returns false when the help is to
// close: a click on the title, or on esc.
function helpMouse(m, state) {
  if (m.wheel) { helpScroll(state, m.wheel * 3); return true; }
  if (!m.press || m.drag || m.button !== 0) return true;
  if (m.y === 1) return false;
  const k = m.y === state.H && keyAt(keys(HELP_KEYS, state.W), m.x - 1);
  return k ? helpKey(k, state) : true;
}

// A question in a card over the pane: q: { title, name, where, note, action, tone ('danger', 'go'), focus ('yes',
// 'no') }. Its title, the name concerned and where it is, a note, then the buttons,
// the action in red (green for tone 'go'), the focused one lit, the other dimmed. Their columns are kept for clicks
// (zones).
const BUTTON = { danger: [138, 47, 42], go: [46, 110, 58] }, BUTTON_ON = [96, 96, 96], BUTTON_OFF = [45, 45, 45];
const DANGER_TEXT = { danger: [229, 120, 109], go: [111, 191, 142] };
function confirm(lines, W, H, q) {
  const inner = Math.max(12, Math.min(W - 4, 44)), room = inner - 2;
  const text = [[[` ${ellipsis(q.title, room)}`, '\x1b[1m']], []];
  for (const l of wrap(q.name, room)) text.push([[` ${l}`, '']]);
  if (q.where) text.push([[` ${ellipsis(q.where, room)}`, fg(C.dim)]]);
  if (q.note) text.push([[` ${ellipsis(q.note, room)}`, fg(q.tone === 'go' ? C.dim : C.red)]]);
  const yes = ` y ${q.action} `, no = ' n Cancel ';
  const gap = Math.max(1, Math.min(3, inner - 3 - textWidth(yes) - textWidth(no)));
  const tone = BUTTON[q.tone] ? q.tone : 'danger', lit = '\x1b[1m' + fg([255, 255, 255]);
  const yesStyle = q.focus === 'yes' ? lit + bg(BUTTON[tone]) : fg(DANGER_TEXT[tone]) + bg(BUTTON_OFF);
  const noStyle = q.focus === 'yes' ? fg(C.dim) + bg(BUTTON_OFF) : lit + bg(BUTTON_ON);
  text.push([], [['  ', ''], [yes, yesStyle], [' '.repeat(gap), ''], [no, noStyle]]);
  const h = text.length + 2, x = Math.max(0, Math.floor((W - inner - 2) / 2));
  const y = Math.max(1, Math.min(H - h, Math.floor((H - h) / 3)));
  const edge = fg(C.dim), left = ' '.repeat(x), right = ' '.repeat(Math.max(0, W - x - inner - 2));
  lines[y] = left + line([['╭' + '─'.repeat(inner) + '╮', edge]], inner + 2) + right;
  text.forEach((segs, k) => { lines[y + 1 + k] = left + line([['│', edge], ...segs], inner + 1) + line([['│', edge]], 1) + right; });
  lines[y + h - 1] = left + line([['╰' + '─'.repeat(inner) + '╯', edge]], inner + 2) + right;
  // Screen columns from 1: the card's edge, then 2 spaces before the first button.
  const bx = x + 4;
  q.zones = { y: y + h - 2, yes: [bx, bx + textWidth(yes) - 1], no: [bx + textWidth(yes) + gap, bx + textWidth(yes) + gap + textWidth(no) - 1] };
}

// A key for the question: ← → (and tab, h, l) move between the buttons ('move'), Enter presses the focused one, y
// confirms ('yes'); any other key cancels ('no').
function confirmKey(k, q) {
  if (['left', 'right', 'tab', 'shift+tab', 'h', 'l'].includes(k)) { q.focus = q.focus === 'yes' ? 'no' : 'yes'; return 'move'; }
  return k === 'y' || (k === 'enter' && q.focus === 'yes') ? 'yes' : 'no';
}
// A click: on the action's button 'yes', elsewhere 'no'; null for what is not a click.
function confirmMouse(m, q) {
  if (!m.press || m.drag || m.wheel) return null;
  const z = q.zones;
  return m.button === 0 && z && m.y - 1 === z.y && m.x >= z.yes[0] && m.x <= z.yes[1] ? 'yes' : 'no';
}

// An empty area of h lines, width w: what it lacks, dimmed and centred a third of the way down, and under it the
// keys that go on from there (pairs as for keys).
function empty(w, h, text, pairs = []) {
  const keyLine = pairs.length ? keys(pairs) : [];
  const body = [[[ellipsis(text, w - 2), fg(C.dim)]], ...(pairs.length ? [[], keyLine] : [])];
  const at = Math.max(0, Math.min(h - body.length, Math.floor((h - body.length) / 3)));
  const out = [];
  for (let i = 0; i < h; i++) {
    const segs = body[i - at];
    const sw = segs ? segWidth(segs) : 0;
    out.push(line(segs ? [[' '.repeat(Math.max(0, Math.floor((w - sw) / 2))), ''], ...segs] : [], w));
  }
  return out;
}

// A message after an action, with its mark: ✓ done (the text plain), ✕ error (in red), ⋯ going on (an ellipsis at the
// end, dimmed), · nothing to do. kind: one of those, else guessed from the text.
const ERROR = /^(Cannot|Can't|Could not|Not |No (editor|terminal)|Action failed|Update failed|Already exists|VS Code not found)|\bnot found\b|: already bound$/;
const messageKind = text => (/…$/.test(text) ? 'busy' : ERROR.test(text) ? 'error' : /^(No |Nothing)/.test(text) ? 'info' : 'ok');
// How long it shows: an error twice as long.
const messageTime = kind => (kind === 'error' ? 4000 : 2000);
function message(text, kind = messageKind(text)) {
  if (kind === 'error') return [[' ✕ ', fg(C.red)], [text, fg(C.red)]];
  if (kind === 'busy') return [[` ⋯ ${text}`, fg(C.dim)]];
  if (kind === 'info') return [[' · ', fg(C.dim)], [text, '']];
  return [[' ✓ ', fg(C.green)], [text, '']];
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

const segText = segs => segs.map(([text]) => text).join('');

const segWidth = segs => fit(segText(segs), Infinity)[1];

// The columns [from, from + width) of a segment list, for horizontal scrolling. A wide character cut by the left
// edge leaves a space, so that the text keeps its columns.
function slice(segs, from, width) {
  const res = [];
  let col = 0, prev, pw = 0;
  for (const [text, style] of segs) {
    let part = '';
    for (const ch of text) {
      const c = cols(ch, prev);
      // An emoji selector whose character was cut by the left edge: a space in its place.
      const lone = c && ch === '\ufe0f' && col - pw < from;
      prev = ch; pw = c;
      if (col >= from && col + c <= from + width) part += lone ? ' ' : ch;
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
  let w = 0, prev, pw = 0;
  for (const [text, style] of segs) {
    let part = '';
    for (const ch of text) {
      const c = cols(ch, prev);
      // The emoji selector widens the character before it (pw columns, maybe in the previous segment): both go to
      // the next line.
      const base = c && ch === '\ufe0f' ? prev : '';
      const bw = base ? pw : 0;
      if (w + c > width && w - bw > 0) {
        let moved = [], keep = '';
        if (base && part.endsWith(base)) { part = part.slice(0, -base.length); keep = base; }
        else if (base) {
          const line = lines[lines.length - 1], last = line[line.length - 1];
          if (last && last[0].endsWith(base)) {
            last[0] = last[0].slice(0, -base.length);
            if (!last[0]) line.pop();
            moved = [[base, last[1]]];
          }
        }
        if (part) lines[lines.length - 1].push([part, style]);
        lines.push(moved); part = keep; w = moved.length || keep ? bw : 0;
      }
      part += ch; w += c; prev = ch; pw = c;
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

// --- scrollbar

// The scrollbar of a list shown height rows at a time, in a column on its right, hidden while the list fits: a thin
// rail, its thumb, and ▴ ▾ buttons at its ends when there is room. One per list, set again at each render; it keeps a
// drag or a held button going from one mouse event to the next. scrollTo(top): moves the list and renders it.
const RAIL = fg([60, 60, 60]) + '│\x1b[0m', THUMB = fg([130, 130, 130]) + '┃\x1b[0m';
const UP = fg([140, 140, 140]) + '▴\x1b[0m', DOWN = fg([140, 140, 140]) + '▾\x1b[0m';
const scrollbars = new Set(); // all of them, let go when the pane loses the focus (see screen())
class Scrollbar {
  constructor(scrollTo) { this.scrollTo = scrollTo; this.bar = null; this.held = null; scrollbars.add(this); }
  // The list's length, the rows shown and the first shown; returns the columns the bar takes: 1, or 0 when hidden.
  set(total, height, top) {
    this.bar = null;
    if (height < 2 || total <= height) return 0;
    const max = total - height, arrows = height >= 4 ? 1 : 0, track = height - 2 * arrows;
    const size = Math.max(1, Math.min(track - 1, Math.round(track * height / total)));
    top = Math.max(0, Math.min(top, max));
    this.bar = { height, max, top, arrows, track, size, at: arrows + Math.round((track - size) * top / max) };
    return 1;
  }
  // The bar's cell on row k of the list (from 0), to add after the row's line; nothing when hidden.
  cell(k) {
    const b = this.bar;
    if (!b) return '';
    if (b.arrows && k === 0) return UP;
    if (b.arrows && k === b.height - 1) return DOWN;
    return k >= b.at && k < b.at + b.size ? THUMB : RAIL;
  }
  // The top that puts the thumb at row r, kept within the list.
  topAt(r) {
    const b = this.bar;
    return Math.max(0, Math.min(b.max, Math.round((r - b.arrows) * b.max / Math.max(1, b.track - b.size))));
  }
  // A mouse event on row k of the list (from 0); on: whether it is on the bar's column. Returns whether the event was
  // the bar's. A press on the thumb grabs it, the drag going on wherever the mouse goes until the release; elsewhere on
  // the rail, the thumb first jumps under the mouse. A button scrolls one row, then again while held on it.
  mouse(m, k, on) {
    const b = this.bar, h = this.held;
    if (m.wheel) return false;
    if (h && !m.press) { this.stop(); return true; }
    if (h && m.drag) {
      if (h.step) { if (!on || k !== h.k) clearTimeout(h.timer); }
      else if (b) this.scrollTo(Math.max(0, Math.min(b.max, h.top + Math.round((k - h.k) * b.max / Math.max(1, b.track - b.size)))));
      return true;
    }
    this.stop();
    if (!b || !on || !m.press || m.drag || m.button !== 0 || k < 0 || k >= b.height) return false;
    const step = b.arrows && k === 0 ? -1 : b.arrows && k === b.height - 1 ? 1 : 0;
    if (step) {
      // The repeat stops at an end of the list, or when the list did not take the last move (gone from the screen).
      const held = this.held = { k, step, last: null, timer: null };
      const move = () => {
        const t = this.bar;
        const top = t ? Math.max(0, Math.min(t.max, t.top + step)) : null;
        if (top === null || top === held.last || (t && top === t.top)) return clearTimeout(held.timer);
        held.last = top;
        this.scrollTo(top);
      };
      move();
      held.timer = setTimeout(() => { held.timer = setInterval(move, 50); }, 400);
      return true;
    }
    const top = k >= b.at && k < b.at + b.size ? b.top : this.topAt(k - Math.floor(b.size / 2));
    this.held = { k, top };
    if (top !== b.top) this.scrollTo(top);
    return true;
  }
  stop() {
    if (this.held) clearTimeout(this.held.timer);
    this.held = null;
  }
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
  '\x1b[1;2A': 'shift+up', '\x1b[1;2B': 'shift+down', '\x1b[1;2C': 'shift+right', '\x1b[1;2D': 'shift+left', '\x1b[1;3A': 'alt+up', '\x1b[1;3B': 'alt+down',
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
// raw input decoded into handlers; everything restored on exit. Returns a paint function, given the screen's lines.
let off = '', handed = false; // handed: the terminal given over to the next process by restart()
function screen(handlers, { mouse = true, motion = false, focus = true } = {}) {
  const out = process.stdout;
  const on = '\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h' + (mouse ? (motion ? '\x1b[?1002h' : '\x1b[?1000h') + '\x1b[?1006h' : '')
    + (focus ? '\x1b[?1004h' : '');
  off = (focus ? '\x1b[?1004l' : '') + (mouse ? '\x1b[?1006l\x1b[?1002l\x1b[?1000l' : '')
    + '\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l';
  out.write(on);
  process.on('exit', () => { if (!handed) out.write(off); });
  for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT']) process.on(sig, () => process.exit(0));
  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  process.stdin.setEncoding('utf8');
  // A button released out of the pane may never be reported: a held scrollbar stops with the focus.
  const input = { ...handlers, focus: f => { if (!f) for (const b of scrollbars) b.stop(); if (handlers.focus) handlers.focus(f); } };
  // A long paste arrives in several reads: held until its end marker, so that its text never reads as keys.
  let pending = '';
  process.stdin.on('data', data => {
    data = pending + data;
    pending = '';
    const start = data.lastIndexOf('\x1b[200~');
    if (start >= 0 && data.indexOf('\x1b[201~', start) < 0) { pending = data.slice(start); data = data.slice(0, start); }
    if (data) decode(data, input);
  });
  // Only the lines changed since the last frame are written, each at its row: a whole frame is large, and on Windows
  // a write to the terminal blocks until it is read. After a resize, the whole frame.
  let shown = [];
  out.on('resize', () => { shown = []; if (handlers.resize) handlers.resize(); });
  const paint = lines => {
    if (handed) return;
    let s = '';
    lines.forEach((l, i) => { if (l !== shown[i]) s += `\x1b[${i + 1};1H${l}`; });
    shown = lines.slice();
    if (s) out.write('\x1b[?2026h' + s + '\x1b[?2026l');
  };
  // The next frame whole: after something else wrote on the screen.
  paint.again = () => { shown = []; };
  return paint;
}

// Runs the pane's script again, on the code updated on disk, in the same terminal: the pane keeps its place, width
// and id. This process hands the terminal over, paints nothing more and exits; its launcher starts the new one
// (launch.js). env: what the new process must show, added to this one's environment.
function restart(env) {
  if (handed) return;
  handed = true;
  process.stdout.write(off);
  process.stdin.removeAllListeners('data');
  process.stdin.pause();
  process.stdout.removeAllListeners('resize');
  process.send({ restart: env }, () => process.exit(require('./launch').RESTART));
}

const size = () => ({ W: process.stdout.columns || 80, H: process.stdout.rows || 24 });

module.exports = {
  fg, bg, glyph, SEARCH, C, GIT, clean, cleanLine, cols, fit, textWidth, ellipsis, line, keys, keyAt, help, helpKey, helpMouse, wrap, empty, message, messageKind, messageTime, confirm, confirmKey, confirmMouse,
  segWidth, segText, slice, split, mark, Scrollbar,
  keyName, keyLabel, decode, printable, Field, screen, restart, size,
};
