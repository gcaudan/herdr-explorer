// Markdown rendered for the terminal: headings, paragraphs with inline styles, lists, quotes, code blocks
// highlighted by highlight.js, GFM tables and task lists, simple Mermaid flowcharts. A small CommonMark-like
// parser, not a complete one. Output: lines of [text, style] segments that fit the width, for ui.line().
'use strict';
const { fg, bg, cleanLine, textWidth, fit } = require('./ui');

const BOLD = '\x1b[1m', ITALIC = '\x1b[3m', UNDER = '\x1b[4m', STRIKE = '\x1b[9m';
const COL = {
  h: [[86, 156, 214], [78, 201, 176], [220, 220, 170], [197, 134, 192], [156, 220, 254], [160, 160, 160]],
  link: [55, 148, 255], image: [197, 134, 192], code: [206, 145, 120], codeBg: [50, 50, 50], blockBg: [36, 36, 36],
  border: [90, 90, 90], dim: [140, 140, 140], marker: [86, 156, 214], green: [129, 184, 139],
};
const LINK = fg(COL.link) + UNDER, CODE = fg(COL.code) + bg(COL.codeBg), DIM = fg(COL.dim), BORDER = fg(COL.border);
const BULLETS = ['•', '◦', '▪'];
const ALERTS = {
  note: ['ℹ Note', [55, 148, 255]], tip: ['✔ Tip', [129, 184, 139]], important: ['❖ Important', [197, 134, 192]],
  warning: ['⚠ Warning', [220, 180, 90]], caution: ['⛔ Caution', [244, 135, 113]],
};
const MAX_INLINE = 20000; // longer paragraphs are shown without inline markup
const MAX_DEPTH = 16;     // nested inline spans

// --- segments

// Appends a segment, merged with the previous one when the style is the same.
function push(out, text, style) {
  if (!text) return;
  const last = out[out.length - 1];
  if (last && last[1] === style) last[0] += text; else out.push([text, style]);
}
const plain = segs => segs.map(s => s[0]).join('');
const widthOf = segs => segs.reduce((w, s) => w + textWidth(s[0]), 0);
// Prefixes the first line and the others (hanging indent); keeps heading anchors.
const prefixed = (lines, first, rest) => lines.map((l, k) => {
  const r = [...(k ? rest : first), ...l];
  if (l.anchor) r.anchor = l.anchor;
  return r;
});

// Cuts segments into lines of at most width columns, without looking for spaces (code).
function chop(segs, width) {
  const lines = [];
  let cur = [], w = 0;
  for (const [t, s] of segs) {
    let rest = t;
    while (rest) {
      const [part, pw] = fit(rest, width - w);
      if (!part) {
        if (!w) { rest = rest.slice(Array.from(rest)[0].length); continue; } // wider than the whole line: dropped
        lines.push(cur); cur = []; w = 0; continue;
      }
      push(cur, part, s); w += pw;
      rest = rest.slice(part.length);
    }
  }
  lines.push(cur);
  return lines;
}

// Word wrap of styled segments: breaks at spaces, '\n' forces a line break, words longer than the width are cut.
function wrap(segs, width) {
  width = Math.max(1, width);
  const lines = [];
  let cur = [], w = 0, space = null, word = [], ww = 0;
  const br = () => { lines.push(cur); cur = []; w = 0; space = null; };
  const place = () => {
    if (!word.length) return;
    const sw = space ? textWidth(space[0]) : 0;
    if (w && w + sw + ww > width) br();
    if (w && space) { push(cur, space[0], space[1]); w += sw; }
    space = null;
    if (w + ww <= width) for (const [t, s] of word) { push(cur, t, s); w += textWidth(t); }
    else {
      const parts = chop(word, width);
      parts.forEach((p, k) => {
        if (k) br();
        for (const [t, s] of p) push(cur, t, s);
        w = widthOf(p);
      });
    }
    word = []; ww = 0;
  };
  for (const [t, s] of segs) {
    for (const piece of t.split(/(\n| +)/)) {
      if (!piece) continue;
      if (piece === '\n') { place(); br(); } else if (piece[0] === ' ') {
        place();
        if (w) space = space ? [space[0] + piece, space[1]] : [piece, s];
      } else { word.push([piece, s]); ww += textWidth(piece); }
    }
  }
  place();
  lines.push(cur);
  return lines;
}

// Pads a line of segments to the width with a background.
const fill = (segs, width, back) => [...segs, [' '.repeat(Math.max(0, width - widthOf(segs))), back]];

// --- inline

const ENTITIES = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', copy: '©', reg: '®', trade: '™', mdash: '—',
  ndash: '–', hellip: '…', larr: '←', rarr: '→', uarr: '↑', darr: '↓', times: '×', middot: '·', bull: '•',
  laquo: '«', raquo: '»', deg: '°', check: '✓', emsp: ' ', ensp: ' ', thinsp: ' ',
};
const SPECIAL = /[\\`*_~[!<&\n]|\bhttps?:\/\//g;
const PUNCT = /[!-/:-@[-`{-~]/;
const ENTITY = /&(#\d{1,7}|#[xX][\da-fA-F]{1,6}|[A-Za-z]{2,8});/y;
const AUTOLINK = /<((?:https?|ftp|mailto):[^\s<>]*|[^\s<>@]+@[^\s<>@]+\.\w+)>/y;
const TAG = /<(\/?)([A-Za-z][\w-]*)((?:\s+[^\s>/=<]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>"'<]+))?)*)\s*(\/?)>/y;
const URL = /https?:\/\/[^\s<>]*/y;
const TAG_STYLE = {
  b: BOLD, strong: BOLD, i: ITALIC, em: ITALIC, s: STRIKE, del: STRIKE, strike: STRIKE, u: UNDER, ins: UNDER,
  code: CODE, kbd: CODE, tt: CODE, samp: CODE, a: LINK, mark: '\x1b[7m', sup: '', sub: '', span: '', small: '',
  summary: BOLD, h1: BOLD, h2: BOLD, h3: BOLD, h4: BOLD, h5: BOLD, h6: BOLD, p: '', div: '', center: '',
};
const sticky = (re, s, i) => { re.lastIndex = i; return re.exec(s); };
const run = (text, i, ch) => { let j = i; while (text[j] === ch) j++; return j - i; };
const isSpace = ch => !ch || /\s/.test(ch);
const isWord = ch => !!ch && /[\p{L}\p{N}]/u.test(ch);
const attr = (attrs, name) => {
  const m = new RegExp(`(?:^|\\s)${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`, 'i').exec(attrs);
  return m ? m[1] ?? m[2] ?? m[3] : null;
};

// End of the code span opened at i (after its closing backticks), or 0.
function closeTicks(text, i) {
  const n = run(text, i, '`'), ticks = '`'.repeat(n);
  for (let k = i + n; ;) {
    const j = text.indexOf(ticks, k);
    if (j < 0) return 0;
    const r = run(text, j, '`');
    if (r === n) return j + n;
    k = j + r;
  }
}

// [text](url), [text][ref], [ref] and images; null when the brackets are not a link.
function link(text, i, base, ctx, depth, image) {
  const open = image ? i + 1 : i;
  let j = open, d = 0;
  for (; j < text.length; j++) {
    const c = text[j];
    if (c === '\\') j++;
    else if (c === '`') { const k = closeTicks(text, j); if (k) j = k - 1; }
    else if (c === '[') d++;
    else if (c === ']' && --d === 0) break;
  }
  if (j >= text.length) return null;
  const label = text.slice(open + 1, j);
  let url, end = j + 1;
  if (text[end] === '(') {
    let k = end;
    for (d = 0; k < text.length; k++) {
      if (text[k] === '\\') k++;
      else if (text[k] === '(') d++;
      else if (text[k] === ')' && --d === 0) break;
    }
    if (k >= text.length) return null;
    const m = /^\s*(?:<([^>]*)>|(\S*))/.exec(text.slice(end + 1, k)); // the title is left out
    url = m[1] ?? m[2];
    end = k + 1;
  } else {
    let ref = label;
    if (text[end] === '[') {
      const k = text.indexOf(']', end);
      if (k < 0) return null;
      ref = text.slice(end + 1, k) || label;
      end = k + 1;
    }
    url = ctx.refs[ref.trim().toLowerCase()];
    if (url === undefined) return null;
  }
  if (image) return [end, [['🖼 ' + (plain(inline(label, '', ctx, depth + 1)) || 'image'), base + fg(COL.image)]]];
  const segs = inline(label, base + LINK, ctx, depth + 1);
  const shown = plain(segs);
  if (url && url !== shown && url !== 'mailto:' + shown && !/^\s*!\[/.test(label)) push(segs, ` (${url})`, DIM);
  return [end, segs];
}

// **bold**, *italic*, ***both***, ~~strike~~ (also with _); the whole delimiter run when not closed.
function emphasis(text, i, base, ctx, depth, unclosed) {
  const ch = text[i], n = run(text, i, ch);
  const literal = [i + n, [[ch.repeat(n), base]]];
  if (n > 3 || (ch === '~' && n > 2) || isSpace(text[i + n]) || (ch === '_' && isWord(text[i - 1]))) return literal;
  if (unclosed.has(ch + n)) return literal; // no closer after an earlier opener: none after this one either
  for (let j = i + n; j < text.length;) {
    const c = text[j];
    if (c === '\\') { j += 2; continue; }
    if (c === '`') { const k = closeTicks(text, j); if (k) { j = k; continue; } }
    if (c !== ch) { j++; continue; }
    const r = run(text, j, ch);
    if (r === n && !isSpace(text[j - 1]) && !(ch === '_' && isWord(text[j + r]))) {
      const style = ch === '~' ? STRIKE : (n & 1 ? ITALIC : '') + (n >= 2 ? BOLD : '');
      return [j + r, inline(text.slice(i + n, j), base + style, ctx, depth + 1)];
    }
    j += r;
  }
  unclosed.add(ch + n);
  return literal;
}

// Inline HTML: <br>, <img alt>, a few formatting tags; other tags are hidden, their contents kept.
function tag(text, i, base, ctx, depth) {
  if (text.startsWith('<!--', i)) {
    const k = text.indexOf('-->', i + 4);
    return [k < 0 ? text.length : k + 3, []];
  }
  let m;
  if ((m = sticky(AUTOLINK, text, i))) return [i + m[0].length, [[m[1], base + LINK]]];
  if (!(m = sticky(TAG, text, i))) return null;
  const end = i + m[0].length, name = m[2].toLowerCase();
  if (m[1]) return [end, []];
  if (name === 'br') return [end, [['\n', '']]];
  if (name === 'img') return [end, [['🖼 ' + (attr(m[3], 'alt') || 'image'), base + fg(COL.image)]]];
  if (!(name in TAG_STYLE) || m[4]) return [end, []];
  const close = text.toLowerCase().indexOf('</' + name, end);
  if (close < 0) return [end, []];
  const inner = text.slice(end, close), after = text.indexOf('>', close) + 1 || text.length;
  const style = TAG_STYLE[name];
  const segs = style === CODE ? [[inner.replace(/\n/g, ' '), CODE]] : inline(inner, base + style, ctx, depth + 1);
  const href = name === 'a' && attr(m[3], 'href');
  if (href && href !== plain(segs)) push(segs, ` (${href})`, DIM);
  return [after, segs];
}

// Segments of a paragraph text; '\n' marks hard line breaks.
function inline(text, base, ctx, depth = 0) {
  if (depth > MAX_DEPTH || text.length > MAX_INLINE) return [[text, base]];
  const out = [];
  let buf = '', i = 0;
  const unclosed = new Set();
  const flush = () => { push(out, buf, base); buf = ''; };
  while (i < text.length) {
    SPECIAL.lastIndex = i;
    const m = SPECIAL.exec(text);
    if (!m) { buf += text.slice(i); break; }
    buf += text.slice(i, m.index);
    i = m.index;
    const c = text[i];
    let r = null, k;
    if (c === '\\') r = PUNCT.test(text[i + 1] || '') ? [i + 2, [[text[i + 1], base]]] : null;
    else if (c === '`') {
      if ((k = closeTicks(text, i))) {
        const n = run(text, i, '`');
        let code = text.slice(i + n, k - n).replace(/\n/g, ' ');
        if (code.length > 2 && code[0] === ' ' && code.endsWith(' ') && code.trim()) code = code.slice(1, -1);
        r = [k, [[code, CODE]]];
      } else r = [i + run(text, i, '`'), [['`'.repeat(run(text, i, '`')), base]]];
    } else if (c === '*' || c === '_' || c === '~') r = emphasis(text, i, base, ctx, depth, unclosed);
    else if (c === '[') r = link(text, i, base, ctx, depth, false);
    else if (c === '!') r = text[i + 1] === '[' ? link(text, i, base, ctx, depth, true) : null;
    else if (c === '<') r = tag(text, i, base, ctx, depth);
    else if (c === '\n') r = [i + 1, [['\n', '']]];
    else if (c === '&' && (k = sticky(ENTITY, text, i))) {
      const e = k[1];
      const cp = e[0] !== '#' ? null : /x/i.test(e[1]) ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
      const valid = cp > 0 && cp <= 0x10ffff && (cp < 0xd800 || cp > 0xdfff);
      const ch = cp === null ? ENTITIES[e] : valid ? String.fromCodePoint(cp) : null;
      if (ch) r = [i + k[0].length, [[cleanLine(ch), base]]];
    } else if (c === 'h' && (k = sticky(URL, text, i))) {
      // Bare URL (GFM autolink): trailing punctuation and unbalanced ')' are not part of it.
      let url = k[0].replace(/[.,:;!?"'*_~]+$/, '');
      while (url.endsWith(')') && url.split('(').length < url.split(')').length) url = url.slice(0, -1);
      r = [i + url.length, [[url, base + LINK]]];
    }
    if (r) { flush(); for (const [t, s] of r[1]) push(out, t, s); i = r[0]; } else { buf += m[0]; i += m[0].length; }
  }
  flush();
  return out;
}

// --- blocks

const BLANK = /^\s*$/;
const FENCE = /^( {0,3})(`{3,}|~{3,})(.*)$/;
const ATX = /^ {0,3}(#{1,6})(?:[ \t]+|$)(.*)$/;
const HR = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const SETEXT = /^ {0,3}(=+|-+)[ \t]*$/;
const QUOTE = /^ {0,3}> ?/;
const ITEM = /^( {0,3})([-+*]|\d{1,9}[.)])(?= |$)( *)/;
const TSEP = /^ {0,3}\|?(?:[ \t]*:?-+:?[ \t]*\|)*[ \t]*:?-+:?[ \t]*\|?[ \t]*$/;
const REF = /^ {0,3}\[([^\]]+)\]:[ \t]*<?(\S+?)>?(?:[ \t]+.*)?$/;
const COMMENT = /^ {0,3}<!--/;

function fence(l) {
  const m = FENCE.exec(l);
  if (!m || (m[2][0] === '`' && m[3].includes('`'))) return null;
  const lang = m[3].trim().replace(/^\{\.?|\}$/g, '').split(/[\s,{]/)[0].toLowerCase();
  return { indent: m[1].length, mark: m[2], lang };
}
const interrupts = l => !!(fence(l) || ATX.test(l) || HR.test(l) || QUOTE.test(l) || COMMENT.test(l)
  || (ITEM.test(l) && !BLANK.test(l.slice(ITEM.exec(l)[0].length))));

// Cells of a table row, split at unescaped pipes.
function cells(row) {
  let s = row.trim();
  if (s.startsWith('|')) s = s.slice(1);
  if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
  const out = [];
  let cur = '';
  for (let k = 0; k < s.length; k++) {
    if (s[k] === '\\' && s[k + 1] === '|') { cur += '|'; k++; }
    else if (s[k] === '|') { out.push(cur.trim()); cur = ''; }
    else cur += s[k];
  }
  out.push(cur.trim());
  return out;
}
const isTable = (lines, i) => i + 1 < lines.length && lines[i].includes('|') && lines[i + 1].includes('|')
  && TSEP.test(lines[i + 1]) && cells(lines[i]).length === cells(lines[i + 1]).length;

// Paragraph text: soft line breaks become spaces, hard ones (two trailing spaces or a backslash) '\n'.
const joinLines = ls => ls.map((l, k) => {
  const t = l.trim();
  if (k === ls.length - 1) return t.replace(/\\$/, '');
  return / {2,}$/.test(l) ? t + '\n' : t.endsWith('\\') ? t.slice(0, -1) + '\n' : t + ' ';
}).join('');

function parseList(lines, i, ctx) {
  const first = ITEM.exec(lines[i]);
  const ordered = /\d/.test(first[2]), delim = first[2].slice(-1);
  const list = { type: 'list', ordered, delim, start: ordered ? parseInt(first[2], 10) : 1, items: [], loose: false };
  while (i < lines.length) {
    const m = ITEM.exec(lines[i]);
    if (!m || /\d/.test(m[2]) !== ordered || m[2].slice(-1) !== delim) break;
    const marker = m[1].length + m[2].length, pad = m[3].length;
    const rest = lines[i].slice(marker + pad);
    const indent = pad === 0 || pad > 4 || BLANK.test(rest) ? marker + 1 : marker + pad;
    const body = [lines[i].slice(Math.min(indent, lines[i].length))];
    let blank = false, inFence = fence(body[0]);
    for (i++; i < lines.length; i++) {
      const l = lines[i];
      if (BLANK.test(l)) { body.push(''); blank = true; continue; }
      const ind = l.length - l.trimStart().length;
      if (ind >= indent) {
        body.push(l.slice(indent));
        blank = false;
        if (fence(l.slice(indent))) inFence = !inFence;
        continue;
      }
      if (blank || inFence || ITEM.test(l) || interrupts(l)) break;
      body.push(l.trimStart()); // lazy continuation of a paragraph
    }
    let trailing = false;
    while (body.length > 1 && body[body.length - 1] === '') { body.pop(); trailing = true; }
    const item = { task: null, loose: body.includes('') };
    const t = /^\[([ xX])\](?: +|$)/.exec(body[0]);
    if (t) { item.task = t[1] !== ' '; body[0] = body[0].slice(t[0].length); }
    item.blocks = parse(body, ctx);
    list.items.push(item);
    const next = i < lines.length && ITEM.exec(lines[i]);
    if (trailing && next && /\d/.test(next[2]) === ordered && next[2].slice(-1) === delim) list.loose = true;
  }
  return [list, i];
}

function parse(lines, ctx) {
  const blocks = [];
  let i = 0, m;
  while (i < lines.length) {
    const l = lines[i];
    if (BLANK.test(l)) { i++; continue; }
    let f;
    if ((f = fence(l))) {
      const code = [];
      for (i++; i < lines.length; i++) {
        const c = /^ {0,3}(`{3,}|~{3,})[ \t]*$/.exec(lines[i]);
        if (c && c[1][0] === f.mark[0] && c[1].length >= f.mark.length) { i++; break; }
        code.push(lines[i].replace(new RegExp(`^ {0,${f.indent}}`), ''));
      }
      blocks.push({ type: 'code', lang: f.lang, lines: code });
    } else if (/^ {4}/.test(l)) {
      const code = [];
      while (i < lines.length && (/^ {4}/.test(lines[i]) || BLANK.test(lines[i]))) code.push(lines[i++].slice(4));
      while (code.length && BLANK.test(code[code.length - 1])) code.pop();
      blocks.push({ type: 'code', lang: '', lines: code });
    } else if ((m = ATX.exec(l))) {
      blocks.push({ type: 'heading', level: m[1].length, text: m[2].replace(/(?:^|[ \t]+)#+[ \t]*$/, '').trim() });
      i++;
    } else if (HR.test(l)) { blocks.push({ type: 'hr' }); i++; }
    else if (QUOTE.test(l)) {
      const inner = [];
      for (; i < lines.length; i++) {
        if (QUOTE.test(lines[i])) inner.push(lines[i].replace(QUOTE, ''));
        else if (!BLANK.test(lines[i]) && inner.length && !BLANK.test(inner[inner.length - 1]) && !interrupts(lines[i])) {
          inner.push(lines[i]); // lazy continuation
        } else break;
      }
      blocks.push({ type: 'quote', blocks: parse(inner, ctx) });
    } else if (ITEM.test(l)) {
      let list;
      [list, i] = parseList(lines, i, ctx);
      blocks.push(list);
    } else if (COMMENT.test(l)) {
      while (i < lines.length && !lines[i].includes('-->')) i++;
      i++;
    } else if (isTable(lines, i)) {
      const t = { type: 'table', head: cells(l), rows: [] };
      t.align = cells(lines[i + 1]).map(c => (c.endsWith(':') ? (c.startsWith(':') ? 'center' : 'right') : 'left'));
      for (i += 2; i < lines.length && !BLANK.test(lines[i]) && lines[i].includes('|') && !interrupts(lines[i]); i++) {
        t.rows.push(cells(lines[i]));
      }
      blocks.push(t);
    } else if ((m = REF.exec(l))) {
      const key = m[1].trim().toLowerCase();
      if (!(key in ctx.refs)) ctx.refs[key] = m[2];
      i++;
    } else {
      const para = [l];
      let heading = null;
      for (i++; i < lines.length; i++) {
        const n = lines[i];
        if (BLANK.test(n)) break;
        if ((m = SETEXT.exec(n))) { heading = m[1][0] === '=' ? 1 : 2; i++; break; }
        if (interrupts(n) || isTable(lines, i)) break;
        para.push(n);
      }
      const text = joinLines(para);
      blocks.push(heading ? { type: 'heading', level: heading, text: text.trim() } : { type: 'para', text });
    }
  }
  return blocks;
}

// --- rendering

function slug(text, ctx) {
  const base = text.toLowerCase().trim().replace(/[^\p{L}\p{N}\s_-]/gu, '').replace(/\s/g, '-');
  const n = ctx.slugs[base] = (ctx.slugs[base] ?? -1) + 1;
  return n ? `${base}-${n}` : base;
}

function table(t, width, ctx) {
  const n = t.head.length;
  const rows = [t.head, ...t.rows]
    .map((r, k) => Array.from({ length: n }, (_, c) => inline(r[c] || '', k ? '' : BOLD, ctx)));
  const avail = width - 3 * n - 1;
  if (avail < n * 2) {
    // Too narrow for columns: one "header: value" line per cell, rows separated by rules.
    const out = [];
    rows.slice(1).forEach((r, k) => {
      if (k) out.push([['─'.repeat(width), BORDER]]);
      r.forEach((c, j) => out.push(...wrap([[plain(rows[0][j]) + ': ', BOLD + DIM], ...c], width)));
    });
    return out;
  }
  const column = (c, f) => rows.reduce((m, r) => Math.max(m, ...f(r[c])), 1);
  const natural = Array.from({ length: n }, (_, c) => column(c, segs => wrap(segs, 1e9).map(widthOf)));
  // Longest word of each column: cut words only when the table cannot fit otherwise.
  const word = natural.map((x, c) => Math.min(x, column(c, segs => plain(segs).split(/\s+/).map(textWidth))));
  const w = natural.slice();
  // Shrinks the widest column that can still shrink, one column at a time, until the table fits.
  for (let total = w.reduce((a, b) => a + b, 0), floor = word; total > avail; total--) {
    let k = -1;
    w.forEach((x, c) => { if (x > floor[c] && (k < 0 || x > w[k])) k = c; });
    if (k < 0) { floor = w.map(() => 1); total++; continue; }
    w[k]--;
  }
  const rule = (l, mid, r) => [[l + w.map(x => '─'.repeat(x + 2)).join(mid) + r, BORDER]];
  const out = [rule('┌', '┬', '┐')];
  rows.forEach((r, k) => {
    const wrapped = r.map((c, j) => wrap(c, w[j]));
    const h = Math.max(...wrapped.map(x => x.length));
    for (let y = 0; y < h; y++) {
      const line = [['│', BORDER]];
      wrapped.forEach((c, j) => {
        const segs = c[y] || [], gap = w[j] - widthOf(segs);
        const left = t.align[j] === 'right' ? gap : t.align[j] === 'center' ? gap >> 1 : 0;
        line.push([' '.repeat(left + 1), ''], ...segs, [' '.repeat(gap - left + 1), ''], ['│', BORDER]);
      });
      out.push(line);
    }
    if (!k) out.push(rule('├', '┼', '┤'));
  });
  out.push(rule('└', '┴', '┘'));
  return out;
}

// Mermaid flowchart (graph TD / LR): nodes in order of appearance, as boxes joined by arrows.
const M_NODE = /\s*([A-Za-z_]\w*)(?:\s*([[({>]+)\s*("[^"]*"|[^\])}"]*)\s*[\])}]+)?/y;
const M_ARROW = /\s*(?:--\s+([^-|>]+?)\s+-->|<?(?:--+|==+|-\.+-?)[>ox]?)\s*(?:\|([^|]*)\|)?/y;
function mermaid(lines, width) {
  if (!/^\s*(?:flowchart|graph)\b/.test(lines[0] || '')) return null;
  const nodes = new Map(), edges = [];
  const node = m => {
    const label = (m[3] || '').replace(/^"|"$/g, '').trim();
    if (!nodes.has(m[1])) nodes.set(m[1], label || m[1]);
    else if (label) nodes.set(m[1], label);
    return m[1];
  };
  for (const raw of lines.slice(1)) {
    for (const st of raw.replace(/%%.*/, '').split(';')) {
      if (/^\s*(?:subgraph|end|classDef|class|style|click|linkStyle|direction)\b/.test(st) || BLANK.test(st)) continue;
      let m = sticky(M_NODE, st, 0);
      if (!m) continue;
      let from = node(m), at = M_NODE.lastIndex, a;
      while ((a = sticky(M_ARROW, st, at)) && a[0].trim() && (m = sticky(M_NODE, st, M_ARROW.lastIndex))) {
        const to = node(m);
        edges.push([from, to, (a[1] || a[2] || '').trim()]);
        from = to; at = M_NODE.lastIndex;
      }
    }
  }
  if (!nodes.size) return null;
  const ids = [...nodes.keys()];
  const inner = Math.max(1, Math.min(width - 4, Math.max(...[...nodes.values()].map(textWidth))));
  const boxW = inner + 4, left = ' '.repeat(Math.max(0, (Math.min(width, boxW + 20) - boxW) >> 1));
  const mid = left + ' '.repeat(boxW >> 1);
  const out = [];
  ids.forEach((id, k) => {
    const text = wrap([[nodes.get(id), BOLD]], inner);
    out.push([[left + '┌' + '─'.repeat(inner + 2) + '┐', BORDER]]);
    for (const l of text) out.push([[left + '│ ', BORDER], ...l, [' '.repeat(inner - widthOf(l)) + ' │', BORDER]]);
    out.push([[left + '└' + '─'.repeat(inner + 2) + '┘', BORDER]]);
    const outs = edges.filter(e => e[0] === id);
    if (outs.length === 1 && outs[0][1] === ids[k + 1]) {
      out.push([[mid + '│', BORDER], [outs[0][2] ? ' ' + outs[0][2] : '', DIM]], [[mid + '▼', BORDER]]);
    } else {
      for (const [, to, label] of outs) {
        out.push([[left + '  └─▶ ', BORDER], [nodes.get(to), ''], [label ? ` (${label})` : '', DIM]]);
      }
      if (k < ids.length - 1) out.push([]);
    }
  });
  return out;
}

function code(b, width, ctx) {
  if (b.lang === 'mermaid') { const m = mermaid(b.lines, width); if (m) return m; }
  const hl = ctx.hl();
  const id = hl && b.lang ? hl.language(b.lang) || hl.language('x.' + b.lang) : null;
  const src = hl ? hl.highlight(b.lines.join('\n') + '\n', id) : b.lines.map(l => [[l, '']]);
  const back = bg(COL.blockBg), inner = Math.max(1, width - 2);
  const out = [];
  for (const segs of src) {
    for (const piece of chop(segs, inner)) {
      out.push(fill([[' ', back], ...piece.map(([t, s]) => [t, back + s])], width, back));
    }
  }
  // The language name at the right of the first line, when there is room.
  if (b.lang && out.length && widthOf(out[0].slice(0, -1)) + textWidth(b.lang) + 2 <= width) {
    const first = out[0].slice(0, -1);
    const gap = width - widthOf(first) - textWidth(b.lang) - 1;
    out[0] = [...first, [' '.repeat(gap), back], [b.lang, back + DIM], [' ', back]];
  }
  return out;
}

function block(b, width, ctx) {
  width = Math.max(1, width);
  switch (b.type) {
    case 'heading': {
      const color = COL.h[b.level - 1];
      const segs = inline(b.text, BOLD + fg(color), ctx);
      const out = wrap(segs, width);
      out[0].anchor = slug(plain(segs), ctx);
      if (b.level === 1) out.push([['━'.repeat(width), fg(color)]]);
      else if (b.level === 2) out.push([['─'.repeat(width), BORDER]]);
      return out;
    }
    case 'para': {
      const out = wrap(inline(b.text, '', ctx), width);
      while (out.length && !out[out.length - 1].length) out.pop();
      return out;
    }
    case 'hr': return [[['─'.repeat(width), BORDER]]];
    case 'code': return code(b, width, ctx);
    case 'table': return table(b, width, ctx);
    case 'front': return b.lines.flatMap(l => chop([[l, DIM]], width));
    case 'quote': {
      let color = COL.border, inner = b.blocks;
      const p = inner[0], a = p && p.type === 'para' && /^\[!(\w+)\][ \t]*\n?/.exec(p.text);
      const alert = a && ALERTS[a[1].toLowerCase()];
      if (alert) { color = alert[1]; inner = [{ ...p, text: p.text.slice(a[0].length) }, ...inner.slice(1)]; }
      let lines = blocks(inner, width - 2, ctx);
      if (alert) lines = [...wrap([[alert[0], BOLD + fg(color)]], width - 2), ...lines];
      return prefixed(lines, [['│ ', fg(color)]], [['│ ', fg(color)]]);
    }
    case 'list': {
      const marks = b.items.map((it, k) => (b.ordered ? `${b.start + k}${b.delim}` : BULLETS[ctx.level % 3]));
      const mw = Math.max(...marks.map(textWidth));
      const out = [];
      b.items.forEach((it, k) => {
        const mark = b.ordered ? marks[k].padStart(mw) : marks[k].padEnd(mw);
        const first = [[mark + ' ', fg(COL.marker)]];
        if (it.task !== null) first.push([it.task ? '☑ ' : '☐ ', it.task ? fg(COL.green) : DIM]);
        const indent = widthOf(first);
        let lines = blocks(it.blocks, width - indent, { ...ctx, level: ctx.level + 1, tight: !it.loose && !b.loose });
        if (!lines.length) lines = [[]];
        if (b.loose && k) out.push([]);
        out.push(...prefixed(lines, first, [[' '.repeat(indent), '']]));
      });
      return out;
    }
  }
  return [];
}

function blocks(list, width, ctx) {
  const out = [];
  for (const b of list) {
    const lines = block(b, width, ctx);
    if (!lines.length) continue;
    if (out.length && !ctx.tight) out.push([]);
    out.push(...lines);
  }
  return out;
}

// Truncates a line to the width (a last guard: blocks already fit, except in extremely narrow panes).
function clamp(segs, width) {
  const out = [];
  let w = 0;
  for (const [t, s] of segs) {
    const [part, pw] = fit(t, width - w);
    if (part) out.push([part, s]);
    w += pw;
    if (part.length < t.length) break;
  }
  if (segs.anchor) out.anchor = segs.anchor;
  return out;
}

// Rendered lines of a Markdown text for width columns, and the line index of each heading by its slug
// (GitHub style: "## Some title" → "some-title"). highlight: the highlight.js module, loaded when not given.
function render(text, width, { highlight } = {}) {
  width = Math.max(1, Math.floor(width) || 1);
  let hl = highlight;
  const load = () => { try { return require('./highlight'); } catch { return null; } };
  const ctx = { refs: {}, slugs: {}, level: 0, tight: false, hl: () => (hl === undefined ? (hl = load()) : hl) };
  const src = String(text).replace(/\r\n?/g, '\n').split('\n').map(l => cleanLine(l));
  const tree = [];
  // Front matter: a YAML block between --- lines at the very start.
  if (/^---\s*$/.test(src[0] || '')) {
    const end = src.findIndex((l, k) => k > 0 && /^(?:---|\.\.\.)\s*$/.test(l));
    if (end > 0) { tree.push({ type: 'front', lines: src.slice(0, end + 1) }); src.splice(0, end + 1); }
  }
  tree.push(...parse(src, ctx));
  const lines = blocks(tree, width, ctx).map(l => clamp(l, width));
  const anchors = {};
  lines.forEach((l, k) => { if (l.anchor && !(l.anchor in anchors)) anchors[l.anchor] = k; });
  return { lines, anchors };
}

module.exports = { render };
