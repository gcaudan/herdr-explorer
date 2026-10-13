// Image preview: format and size read from the header (PNG, JPEG, GIF, BMP, WebP, ICO, SVG), pure JS decoders
// for PNG, GIF, BMP, baseline JPEG and ICO, and rendering with half blocks (one cell = two pixels stacked).
// Files are untrusted: sizes are checked before allocating, every loop is bounded and nothing throws.
'use strict';
const zlib = require('zlib');
const { fg, bg } = require('./ui');

const FORMATS = new Set('png apng jpg jpeg jpe jfif gif bmp dib ico cur webp svg'.split(' '));
const MAX_BYTES = 50 * 1024 * 1024;
const MAX_PIXELS = 50e6;
const MAX_RAW = 256 * 1024 * 1024; // inflated PNG data: a few bytes of zlib can claim gigabytes
const ICON = 32;                   // small images are enlarged up to this many pixels on their longer side

const bad = (w, h) => !(w > 0 && h > 0) || w * h > MAX_PIXELS;
const u16 = (b, i) => b[i] << 8 | b[i + 1];
const u32 = (b, i) => (b[i] << 24 >>> 0) + (b[i + 1] << 16 | b[i + 2] << 8 | b[i + 3]);
const le16 = (b, i) => b[i] | b[i + 1] << 8;
const sle32 = (b, i) => b[i] | b[i + 1] << 8 | b[i + 2] << 16 | b[i + 3] << 24;
const le32 = (b, i) => sle32(b, i) >>> 0;
const ascii = (b, i, n) => String.fromCharCode(...b.subarray(i, i + n));
const isPng = b => b.length > 8 && b[0] === 0x89 && ascii(b, 1, 3) === 'PNG';

// --- header

// JPEG frame types other than baseline / extended sequential Huffman, which are not decoded.
const JPEG_NOTE = {
  0xc2: 'progressive', 0xc3: 'lossless', 0xc5: 'hierarchical', 0xc6: 'progressive', 0xc7: 'lossless',
  0xc9: 'arithmetic', 0xca: 'progressive', 0xcb: 'lossless', 0xcd: 'hierarchical', 0xce: 'progressive',
  0xcf: 'lossless',
};

// { format, width, height, note? } from the first bytes, or null when not a known image or its size is unknown.
function info(buf) {
  try {
    const m = sniff(buf);
    return m && m.width > 0 && m.height > 0 ? m : null;
  } catch { return null; }
}

function sniff(b) {
  if (!b || b.length < 12) return null;
  const n = b.length;
  if (isPng(b)) return n < 24 ? null : { format: 'PNG', width: u32(b, 16), height: u32(b, 20) };
  if (b[0] === 0xff && b[1] === 0xd8) {
    for (let i = 2; i + 9 < n;) {
      if (b[i] !== 0xff || b[i + 1] === 0xff) { i++; continue; }
      const m = b[i + 1];
      if (m === 0xd8 || m === 0x01 || (m >= 0xd0 && m <= 0xd7)) { i += 2; continue; }
      if (m === 0xd9 || m === 0xda) break;
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) {
        const note = JPEG_NOTE[m] || (b[i + 4] !== 8 ? `${b[i + 4]}-bit` : '');
        return { format: 'JPEG', width: u16(b, i + 7), height: u16(b, i + 5), note };
      }
      i += 2 + u16(b, i + 2);
    }
    return null;
  }
  if (ascii(b, 0, 4) === 'GIF8') return { format: 'GIF', width: le16(b, 6), height: le16(b, 8) };
  if (b[0] === 0x42 && b[1] === 0x4d && n >= 26) {
    return le32(b, 14) === 12 ? { format: 'BMP', width: le16(b, 18), height: le16(b, 20) }
      : { format: 'BMP', width: sle32(b, 18), height: Math.abs(sle32(b, 22)) };
  }
  if (ascii(b, 0, 4) === 'RIFF' && ascii(b, 8, 4) === 'WEBP' && n >= 30) {
    const chunk = ascii(b, 12, 4);
    if (chunk === 'VP8 ') return { format: 'WebP', width: le16(b, 26) & 0x3fff, height: le16(b, 28) & 0x3fff };
    if (chunk === 'VP8L' && b[20] === 0x2f) {
      const bits = le32(b, 21);
      return { format: 'WebP', width: (bits & 0x3fff) + 1, height: (bits >>> 14 & 0x3fff) + 1 };
    }
    if (chunk === 'VP8X') {
      return {
        format: 'WebP', width: 1 + (b[24] | b[25] << 8 | b[26] << 16), height: 1 + (b[27] | b[28] << 8 | b[29] << 16),
        note: b[20] & 2 ? 'animated' : '',
      };
    }
    return null;
  }
  if (b[0] === 0 && b[1] === 0 && (b[2] === 1 || b[2] === 2) && b[3] === 0) {
    const e = icoEntry(b);
    return e && { format: b[2] === 1 ? 'ICO' : 'CUR', width: e.w, height: e.h };
  }
  return svgSize(b);
}

// SVG size from the width / height attributes of the root element, else its viewBox.
const UNITS = { '': 1, px: 1, pt: 4 / 3, pc: 16, mm: 96 / 25.4, cm: 96 / 2.54, in: 96 };
function svgSize(b) {
  const head = Buffer.from(b.buffer, b.byteOffset, Math.min(b.length, 65536)).toString('latin1');
  if (!/^(\xef\xbb\xbf)?\s*</.test(head)) return null;
  const tag = (/<svg\b[^>]*>/i.exec(head) || [])[0];
  if (!tag) return null;
  const attr = name => (new RegExp(`\\s${name}\\s*=\\s*["']([^"']*)["']`, 'i').exec(tag) || [])[1];
  const length = v => {
    const m = /^\s*([\d.]+(?:e[+-]?\d+)?)\s*(px|pt|pc|mm|cm|in|)\s*$/i.exec(v || '');
    return m ? +m[1] * UNITS[m[2].toLowerCase()] || 0 : 0;
  };
  let w = length(attr('width')), h = length(attr('height'));
  const vb = (attr('viewBox') || '').trim().split(/[\s,]+/).map(Number);
  if (vb.length === 4 && vb[2] > 0 && vb[3] > 0) {
    if (!w && !h) { w = vb[2]; h = vb[3]; } else if (!w) w = h * vb[2] / vb[3]; else if (!h) h = w * vb[3] / vb[2];
  }
  return { format: 'SVG', width: Math.round(w), height: Math.round(h) };
}

// The largest image of an ICO / CUR directory (then the deepest colours).
function icoEntry(b) {
  const count = le16(b, 4);
  let best = null;
  for (let k = 0; k < count && 6 + 16 * k + 16 <= b.length; k++) {
    const e = 6 + 16 * k;
    const w = b[e] || 256, h = b[e + 1] || 256, bpp = le16(b, e + 6);
    if (!best || w * h > best.w * best.h || (w * h === best.w * best.h && bpp > best.bpp)) {
      best = { w, h, bpp, size: le32(b, e + 8), off: le32(b, e + 12) };
    }
  }
  return best;
}

// --- decoding

// { width, height, data: RGBA bytes } or null when the format is not supported or the file is broken.
function decode(buf) {
  try {
    if (!buf || buf.length < 12 || buf.length > MAX_BYTES) return null;
    const b = buf;
    if (isPng(b)) return decodePng(b);
    if (b[0] === 0xff && b[1] === 0xd8) return decodeJpeg(b);
    if (ascii(b, 0, 4) === 'GIF8') return decodeGif(b);
    if (b[0] === 0x42 && b[1] === 0x4d) return decodeDib(b, 14, le32(b, 10), false);
    if (b[0] === 0 && b[1] === 0 && (b[2] === 1 || b[2] === 2) && b[3] === 0) return decodeIco(b);
    return null;
  } catch { return null; }
}

// --- PNG

const ADAM7 = [[0, 0, 8, 8], [4, 0, 8, 8], [0, 4, 4, 8], [2, 0, 4, 4], [0, 2, 2, 4], [1, 0, 2, 2], [0, 1, 1, 2]];
const CHANNELS = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 };

function decodePng(b) {
  if (b.length < 33 || ascii(b, 12, 4) !== 'IHDR') return null;
  const w = u32(b, 16), h = u32(b, 20), depth = b[24], type = b[25], interlace = b[28];
  const channels = CHANNELS[type];
  // Bit depths allowed by the specification for each colour type.
  if (!channels || ![1, 2, 4, 8, 16].includes(depth) || (type === 3 ? depth > 8 : type && depth < 8)) return null;
  if (bad(w, h)) return null;
  let plte = null, trns = null;
  const idat = [];
  for (let i = 8; i + 8 <= b.length;) {
    const len = u32(b, i), name = ascii(b, i + 4, 4), data = b.subarray(i + 8, i + 8 + len); // truncated: what is there
    if (name === 'IDAT') idat.push(data);
    else if (name === 'PLTE') plte = data;
    else if (name === 'tRNS') trns = data;
    else if (name === 'IEND') break;
    i += 12 + len;
  }
  if (!idat.length || (type === 3 && !plte)) return null;

  const bpp = channels * depth; // bits per pixel
  const passes = (interlace ? ADAM7 : [[0, 0, 1, 1]]).map(([x0, y0, dx, dy]) => {
    const pw = Math.max(0, Math.ceil((w - x0) / dx)), ph = Math.max(0, Math.ceil((h - y0) / dy));
    return { x0, y0, dx, dy, pw, ph, stride: Math.ceil(pw * bpp / 8) };
  });
  const size = passes.reduce((s, p) => s + (p.pw && p.ph ? p.ph * (1 + p.stride) : 0), 0);
  if (size > MAX_RAW) return null;
  // Sync flush: a truncated file still gives its first rows.
  const raw = zlib.inflateSync(Buffer.concat(idat),
    { finishFlush: zlib.constants.Z_SYNC_FLUSH, maxOutputLength: size + 1024 });

  // Palette, or the transparent colour, as RGBA.
  const pal = new Uint8Array(256 * 4);
  if (plte) for (let k = 0; k < 256 && k * 3 + 2 < plte.length; k++) {
    pal.set([plte[k * 3], plte[k * 3 + 1], plte[k * 3 + 2], trns && k < trns.length ? trns[k] : 255], k * 4);
  }
  const key = trns && type === 0 && trns.length >= 2 ? [u16(trns, 0)] : trns && type === 2 && trns.length >= 6
    ? [u16(trns, 0), u16(trns, 2), u16(trns, 4)] : null;

  const out = new Uint8Array(w * h * 4);
  const dist = Math.max(1, bpp >> 3);
  const mask = (1 << depth) - 1;
  let pos = 0;
  for (const { x0, y0, dx, dy, pw, ph, stride } of passes) {
    if (!pw || !ph) continue;
    let prev = new Uint8Array(stride), cur = new Uint8Array(stride);
    for (let y = 0; y < ph && pos < raw.length; y++, pos += stride + 1) {
      const f = raw[pos];
      cur.fill(0);
      cur.set(raw.subarray(pos + 1, pos + 1 + stride));
      unfilter(f, cur, prev, dist);
      const row = (y0 + y * dy) * w;
      for (let x = 0; x < pw; x++) {
        const o = (row + x0 + x * dx) * 4;
        if (depth === 8) {
          const s = x * channels;
          if (type === 6) copy4(out, o, cur, s);
          else if (type === 3) copy4(out, o, pal, cur[s] * 4);
          else if (type === 2) {
            out[o] = cur[s]; out[o + 1] = cur[s + 1]; out[o + 2] = cur[s + 2];
            out[o + 3] = key && cur[s] === key[0] && cur[s + 1] === key[1] && cur[s + 2] === key[2] ? 0 : 255;
          } else {
            out[o] = out[o + 1] = out[o + 2] = cur[s];
            out[o + 3] = type === 4 ? cur[s + 1] : key && cur[s] === key[0] ? 0 : 255;
          }
        } else if (depth === 16) {
          const s = x * channels * 2;
          const v = k => cur[s + 2 * k] << 8 | cur[s + 2 * k + 1];
          if (type === 0 || type === 4) {
            out[o] = out[o + 1] = out[o + 2] = cur[s];
            out[o + 3] = type === 4 ? cur[s + 2] : key && v(0) === key[0] ? 0 : 255;
          } else {
            out[o] = cur[s]; out[o + 1] = cur[s + 2]; out[o + 2] = cur[s + 4];
            out[o + 3] = type === 6 ? cur[s + 6]
              : key && v(0) === key[0] && v(1) === key[1] && v(2) === key[2] ? 0 : 255;
          }
        } else { // 1, 2 or 4 bits: grey or palette
          const bit = x * depth;
          const v = cur[bit >> 3] >> (8 - depth - (bit & 7)) & mask;
          if (type === 3) copy4(out, o, pal, v * 4);
          else {
            out[o] = out[o + 1] = out[o + 2] = v * 255 / mask;
            out[o + 3] = key && v === key[0] ? 0 : 255;
          }
        }
      }
      [prev, cur] = [cur, prev];
    }
  }
  return { width: w, height: h, data: out };
}

const copy4 = (out, o, src, s) => {
  out[o] = src[s]; out[o + 1] = src[s + 1]; out[o + 2] = src[s + 2]; out[o + 3] = src[s + 3];
};

function unfilter(f, cur, prev, d) {
  const n = cur.length;
  if (f === 1) for (let i = d; i < n; i++) cur[i] += cur[i - d];
  else if (f === 2) for (let i = 0; i < n; i++) cur[i] += prev[i];
  else if (f === 3) for (let i = 0; i < n; i++) cur[i] += ((i >= d ? cur[i - d] : 0) + prev[i]) >> 1;
  else if (f === 4) {
    for (let i = 0; i < n; i++) {
      const a = i >= d ? cur[i - d] : 0, b = prev[i], c = i >= d ? prev[i - d] : 0;
      const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c);
      cur[i] += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
    }
  }
}

// --- GIF (first frame)

function decodeGif(b) {
  let sw = le16(b, 6), sh = le16(b, 8), pos = 13, gct = null, transparent = -1;
  if (b[10] & 0x80) { gct = b.subarray(pos, pos + (3 << ((b[10] & 7) + 1))); pos += gct.length; }
  while (pos < b.length) {
    const t = b[pos++];
    if (t === 0x21) {
      if (b[pos] === 0xf9 && b[pos + 1] >= 4 && b[pos + 2] & 1) transparent = b[pos + 5];
      pos = skipBlocks(b, pos + 1);
    } else if (t === 0x2c) {
      const fx = le16(b, pos), fy = le16(b, pos + 2), fw = le16(b, pos + 4), fh = le16(b, pos + 6), flags = b[pos + 8];
      pos += 9;
      let pal = gct;
      if (flags & 0x80) { pal = b.subarray(pos, pos + (3 << ((flags & 7) + 1))); pos += pal.length; }
      // A frame larger than the screen enlarges it, as browsers do.
      sw = Math.max(sw, fx + fw); sh = Math.max(sh, fy + fh);
      if (bad(sw, sh)) return null;
      const min = b[pos++];
      const end = skipBlocks(b, pos);
      const data = new Uint8Array(end - pos);
      let n = 0;
      for (let p = pos; p < end && b[p];) { data.set(b.subarray(p + 1, p + 1 + b[p]), n); n += b[p]; p += 1 + b[p]; }
      const idx = new Uint8Array(fw * fh);
      const count = lzw(data.subarray(0, n), min, idx);
      const rows = new Uint32Array(fh);
      if (flags & 0x40) {
        let r = 0;
        for (const [start, step] of [[0, 8], [4, 8], [2, 4], [1, 2]]) {
          for (let y = start; y < fh; y += step) rows[r++] = y;
        }
      } else for (let y = 0; y < fh; y++) rows[y] = y;
      const out = new Uint8Array(sw * sh * 4);
      for (let i = 0; i < count; i++) {
        const k = idx[i], c = k * 3;
        if (k === transparent || !pal || c + 2 >= pal.length) continue;
        const o = ((fy + rows[(i / fw) | 0]) * sw + fx + i % fw) * 4;
        out[o] = pal[c]; out[o + 1] = pal[c + 1]; out[o + 2] = pal[c + 2]; out[o + 3] = 255;
      }
      return { width: sw, height: sh, data: out };
    } else return null;
  }
  return null;
}

function skipBlocks(b, pos) {
  while (pos < b.length) {
    const len = b[pos];
    pos += 1 + len;
    if (!len) break;
  }
  return Math.min(pos, b.length);
}

// Decodes into out; returns the number of pixels written.
function lzw(data, min, out) {
  if (min < 1 || min > 11) return 0;
  const clear = 1 << min, eoi = clear + 1, count = out.length;
  const prefix = new Uint16Array(4096), lens = new Uint16Array(4096);
  const suffix = new Uint8Array(4096), first = new Uint8Array(4096);
  for (let k = 0; k < clear; k++) { suffix[k] = first[k] = k; lens[k] = 1; }
  let size = min + 1, next = eoi + 1, prev = -1, acc = 0, bits = 0, o = 0;
  const emit = c => {
    const L = lens[c];
    for (let j = L - 1, k = c; j >= 0; j--, k = prefix[k]) if (o + j < count) out[o + j] = suffix[k];
    o += L;
  };
  for (let i = 0; i < data.length && o < count;) {
    acc |= data[i++] << bits; bits += 8;
    while (bits >= size && o < count) {
      const code = acc & ((1 << size) - 1);
      acc >>>= size; bits -= size;
      if (code === clear) { size = min + 1; next = eoi + 1; prev = -1; continue; }
      if (code === eoi) return Math.min(o, count);
      if (prev < 0) {
        if (code >= clear) return Math.min(o, count);
        out[o++] = code; prev = code; continue;
      }
      if (code > next) return Math.min(o, count);
      const fc = code < next ? first[code] : first[prev];
      if (code < next) emit(code); else { emit(prev); if (o < count) out[o] = fc; o++; }
      if (next < 4096) {
        prefix[next] = prev; suffix[next] = fc; first[next] = first[prev]; lens[next] = lens[prev] + 1;
        if (++next === 1 << size && size < 12) size++;
      }
      prev = code;
    }
  }
  return Math.min(o, count);
}

// --- BMP and the bitmaps inside ICO files

const shiftOf = m => { let s = 0; while (s < 32 && !((m >>> s) & 1)) s++; return s; };
const bitsOf = m => { let n = 0; for (; m; m >>>= 1) n += m & 1; return n; };

// off: start of the DIB header; data: start of the pixels (null: right after the palette). ico: the height counts
// the colour bitmap and the 1-bit transparency mask that follows it.
function decodeDib(b, off, data, ico) {
  const hs = le32(b, off);
  let w, h, bpp, comp = 0, used = 0;
  if (hs === 12) { w = le16(b, off + 4); h = le16(b, off + 6); bpp = le16(b, off + 10); } else if (hs >= 40 && hs <= 256) {
    w = sle32(b, off + 4); h = sle32(b, off + 8); bpp = le16(b, off + 14);
    comp = le32(b, off + 16); used = le32(b, off + 32);
  } else return null;
  if (ico) h = (h / 2) | 0;
  const topDown = h < 0;
  h = Math.abs(h);
  if (bad(w, h) || ![1, 2, 4, 8, 16, 24, 32].includes(bpp)) return null;
  let masks = null, alpha = false, extra = 0;
  if (comp === 3 || comp === 6) {
    // Masks in the header (V2 and later) or right after the 40-byte one.
    const m = hs >= 52 ? off + 40 : off + hs;
    masks = [le32(b, m), le32(b, m + 4), le32(b, m + 8), hs >= 56 || comp === 6 ? le32(b, m + 12) : 0];
    if (hs < 52) extra = comp === 6 ? 16 : 12;
    alpha = !!masks[3];
  } else if (comp !== 0) return null; // RLE, embedded JPEG / PNG
  else if (bpp === 16) masks = [0x7c00, 0x3e0, 0x1f, 0];
  else if (bpp === 32) { masks = [0xff0000, 0xff00, 0xff, 0xff000000]; alpha = ico; }
  if ((bpp === 16 || bpp === 32) && !masks) return null;
  const entry = hs === 12 ? 3 : 4, pal = off + hs + extra;
  const colours = bpp <= 8 ? Math.min(used || 1 << bpp, 1 << bpp) : 0;
  if (data === null) data = pal + colours * entry;
  const stride = ((w * bpp + 31) >>> 5) * 4;
  const out = new Uint8Array(w * h * 4);
  const chan = masks && masks.map(m => [m, shiftOf(m), (1 << bitsOf(m)) - 1 || 1]);
  let anyAlpha = false;
  for (let y = 0; y < h; y++) {
    const p = data + (topDown ? y : h - 1 - y) * stride;
    if (p + stride > b.length) continue;
    for (let x = 0, o = y * w * 4; x < w; x++, o += 4) {
      if (bpp <= 8) {
        const bit = x * bpp, k = b[p + (bit >> 3)] >> (8 - bpp - (bit & 7)) & ((1 << bpp) - 1);
        const c = pal + k * entry;
        if (k < colours && c + 2 < b.length) { out[o] = b[c + 2]; out[o + 1] = b[c + 1]; out[o + 2] = b[c]; }
        out[o + 3] = 255;
      } else if (bpp === 24) {
        const q = p + x * 3;
        out[o] = b[q + 2]; out[o + 1] = b[q + 1]; out[o + 2] = b[q]; out[o + 3] = 255;
      } else {
        const v = bpp === 16 ? le16(b, p + x * 2) : le32(b, p + x * 4);
        for (let k = 0; k < 3; k++) { const [m, s, max] = chan[k]; out[o + k] = ((v & m) >>> s) * 255 / max; }
        if (!alpha) { out[o + 3] = 255; continue; }
        const [m, s, max] = chan[3];
        out[o + 3] = ((v & m) >>> s) * 255 / max;
        anyAlpha ||= out[o + 3] > 0;
      }
    }
  }
  // An alpha channel that is all zero is unused, not a fully transparent image.
  if (alpha && !anyAlpha) for (let i = 3; i < out.length; i += 4) out[i] = 255;
  if (ico && !anyAlpha) {
    const ms = ((w + 31) >>> 5) * 4, mask = data + stride * h;
    for (let y = 0; y < h; y++) {
      const p = mask + (topDown ? y : h - 1 - y) * ms;
      if (p + ms > b.length) continue;
      for (let x = 0; x < w; x++) if (b[p + (x >> 3)] >> (7 - (x & 7)) & 1) out[(y * w + x) * 4 + 3] = 0;
    }
  }
  return { width: w, height: h, data: out };
}

function decodeIco(b) {
  const e = icoEntry(b);
  if (!e || e.off >= b.length) return null;
  const d = b.subarray(e.off, e.off + e.size);
  return isPng(d) ? decodePng(d) : decodeDib(d, 0, null, true);
}

// --- JPEG (baseline and extended sequential Huffman, 8-bit)

const ZIGZAG = Uint8Array.from([
  0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28,
  35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
]);

// Codes of up to 9 bits are found in one lookup (length << 8 | value); longer ones by length.
function huffman(counts, values) {
  const fast = new Uint16Array(512), max = new Int32Array(17).fill(-1);
  const min = new Int32Array(17), ptr = new Int32Array(17);
  let code = 0, k = 0;
  for (let len = 1; len <= 16; len++) {
    ptr[len] = k; min[len] = code;
    for (let j = 0; j < counts[len - 1]; j++, k++, code++) {
      if (len <= 9 && code < 1 << len) {
        const base = code << (9 - len);
        for (let f = 0; f < 1 << (9 - len); f++) fast[base + f] = len << 8 | (values[k] | 0);
      }
    }
    if (counts[len - 1]) max[len] = code - 1;
    code <<= 1;
  }
  return { fast, max, min, ptr, values };
}

function decodeJpeg(b) {
  const n = b.length, qt = [], dcs = [], acs = [];
  let frame = null, restart = 0, adobe = -1;
  for (let i = 2; i + 4 <= n;) {
    if (b[i] !== 0xff || b[i + 1] === 0xff) { i++; continue; }
    const m = b[i + 1];
    if (m === 0xd8 || m === 0x01 || m === 0x00 || (m >= 0xd0 && m <= 0xd7)) { i += 2; continue; }
    if (m === 0xd9) break;
    const len = u16(b, i + 2), s = i + 4, e = Math.min(i + 2 + len, n);
    if (m === 0xdb) {
      for (let p = s; p < e;) {
        const wide = b[p] >> 4, t = new Int32Array(64), id = b[p] & 3;
        p++;
        for (let k = 0; k < 64; k++) t[ZIGZAG[k]] = wide ? u16(b, p + 2 * k) : b[p + k] | 0;
        p += wide ? 128 : 64;
        qt[id] = t;
      }
    } else if (m === 0xc4) {
      for (let p = s; p + 17 <= e;) {
        const counts = b.subarray(p + 1, p + 17), total = counts.reduce((a, c) => a + c, 0);
        (b[p] >> 4 ? acs : dcs)[b[p] & 3] = huffman(counts, b.subarray(p + 17, p + 17 + total));
        p += 17 + total;
      }
    } else if (m === 0xdd) restart = u16(b, s);
    else if (m === 0xee && ascii(b, s, 5) === 'Adobe') adobe = b[s + 11];
    else if (m === 0xc0 || m === 0xc1) {
      if (frame || b[s] !== 8) return null;
      frame = jpegFrame(b, s);
      if (!frame) return null;
    } else if (m >= 0xc2 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) return null;
    else if (m === 0xda) {
      if (!frame) return null;
      const comps = [];
      for (let k = 0; k < b[s]; k++) {
        const c = frame.comps.find(x => x.id === b[s + 1 + 2 * k]), sel = b[s + 2 + 2 * k];
        if (!c || !dcs[sel >> 4] || !acs[sel & 15] || !qt[c.tq]) return null;
        comps.push({ c, dc: dcs[sel >> 4], ac: acs[sel & 15], q: qt[c.tq] });
      }
      if (!comps.length) return null;
      i = jpegScan(b, e, frame, comps, restart);
      continue;
    }
    i += 2 + len;
  }
  return frame && jpegColours(frame, adobe);
}

function jpegFrame(b, s) {
  const h = u16(b, s + 1), w = u16(b, s + 3), count = b[s + 5];
  if (bad(w, h) || ![1, 3, 4].includes(count)) return null;
  const comps = [];
  for (let k = 0; k < count; k++) {
    const p = s + 6 + 3 * k;
    comps.push({ id: b[p], h: b[p + 1] >> 4, v: b[p + 1] & 15, tq: b[p + 2] & 3 });
  }
  if (comps.some(c => c.h < 1 || c.h > 4 || c.v < 1 || c.v > 4)) return null;
  const hmax = Math.max(...comps.map(c => c.h)), vmax = Math.max(...comps.map(c => c.v));
  const mx = Math.ceil(w / (8 * hmax)), my = Math.ceil(h / (8 * vmax));
  for (const c of comps) {
    c.stride = mx * c.h * 8;
    c.plane = new Uint8Array(c.stride * my * c.v * 8);
    // Blocks of a scan with this component alone: no padding to whole MCUs.
    c.bw = Math.ceil(w * c.h / hmax / 8); c.bh = Math.ceil(h * c.v / vmax / 8);
  }
  return { w, h, comps, hmax, vmax, mx, my };
}

// Decodes one scan; returns the position of the marker that follows it.
function jpegScan(b, pos, frame, comps, restart) {
  const n = b.length, blk = new Int32Array(64), tmp = new Int32Array(64);
  let buf = 0, cnt = 0, marker = false;
  // Keeps at least 25 bits in buf; zeros once a marker or the end is reached.
  const fill = () => {
    while (cnt <= 24) {
      let v = 0;
      if (!marker && pos < n) {
        v = b[pos];
        if (v === 0xff) { if (b[pos + 1] === 0) pos += 2; else { marker = true; v = 0; } } else pos++;
      }
      buf = buf << 8 | v; cnt += 8;
    }
  };
  const symbol = t => {
    if (cnt < 16) fill();
    const f = t.fast[(buf >>> (cnt - 9)) & 511];
    if (f) { cnt -= f >> 8; return f & 255; }
    for (let len = 10; len <= 16; len++) {
      const code = (buf >>> (cnt - len)) & ((1 << len) - 1);
      if (code <= t.max[len]) { cnt -= len; return t.values[t.ptr[len] + code - t.min[len]] | 0; }
    }
    cnt -= 16;
    return 0;
  };
  const receive = s => {
    if (!s) return 0;
    if (cnt < s) fill();
    const v = (buf >>> (cnt - s)) & ((1 << s) - 1);
    cnt -= s;
    return v < 1 << (s - 1) ? v - (1 << s) + 1 : v;
  };
  const block = (s, bx, by) => {
    const { c, q } = s;
    blk.fill(0);
    c.pred += receive(symbol(s.dc) & 15);
    blk[0] = c.pred * q[0];
    for (let k = 1; k < 64;) {
      const rs = symbol(s.ac), size = rs & 15, run = rs >> 4;
      if (!size) { if (run !== 15) break; k += 16; continue; }
      k += run;
      if (k > 63) break;
      const z = ZIGZAG[k++];
      blk[z] = receive(size) * q[z];
    }
    idct(blk, tmp, c.plane, by * 8 * c.stride + bx * 8, c.stride);
  };

  const single = comps.length === 1;
  const total = single ? comps[0].c.bw * comps[0].c.bh : frame.mx * frame.my;
  for (const s of comps) s.c.pred = 0;
  for (let m = 0; m < total; m++) {
    if (restart && m && m % restart === 0) {
      // Resynchronise on the next RST marker; any other marker ends the scan.
      while (pos + 1 < n && !(b[pos] === 0xff && b[pos + 1] !== 0 && b[pos + 1] !== 0xff)) pos++;
      if (!(b[pos + 1] >= 0xd0 && b[pos + 1] <= 0xd7)) break;
      pos += 2; marker = false; buf = 0; cnt = 0;
      for (const s of comps) s.c.pred = 0;
    }
    if (single) { const c = comps[0].c; block(comps[0], m % c.bw, (m / c.bw) | 0); continue; }
    const mx = m % frame.mx, my = (m / frame.mx) | 0;
    for (const s of comps) {
      for (let v = 0; v < s.c.v; v++) for (let h = 0; h < s.c.h; h++) block(s, mx * s.c.h + h, my * s.c.v + v);
    }
  }
  while (pos + 1 < n && !(b[pos] === 0xff && b[pos + 1] !== 0 && b[pos + 1] !== 0xff
    && !(b[pos + 1] >= 0xd0 && b[pos + 1] <= 0xd7))) pos++;
  return pos;
}

// Integer IDCT (the jidctint algorithm, as in stb_image): fixed point with 12 fractional bits.
const F = x => Math.round(x * 4096);
const [K0, K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11] = [0.5411961, -1.847759065, 0.765366865, 1.175875602,
  0.298631336, 2.053119869, 3.072711026, 1.50132111, -0.899976223, -2.562915447, -1.96157056, -0.390180644].map(F);

function idct(d, v, out, off, stride) {
  for (let pass = 0; pass < 2; pass++) {
    const src = pass ? v : d;
    for (let i = 0; i < 8; i++) {
      // First pass on columns (step 8), second on rows (step 1).
      const a = pass ? i * 8 : i, st = pass ? 1 : 8;
      const s0 = src[a], s1 = src[a + st], s2 = src[a + 2 * st], s3 = src[a + 3 * st], s4 = src[a + 4 * st],
        s5 = src[a + 5 * st], s6 = src[a + 6 * st], s7 = src[a + 7 * st];
      if (!pass && !(s1 | s2 | s3 | s4 | s5 | s6 | s7)) {
        const dc = s0 * 4;
        for (let k = 0; k < 8; k++) v[a + k * 8] = dc;
        continue;
      }
      let p1 = (s2 + s6) * K0;
      let t2 = p1 + s6 * K1, t3 = p1 + s2 * K2;
      let t0 = (s0 + s4) * 4096, t1 = (s0 - s4) * 4096;
      let x0 = t0 + t3, x3 = t0 - t3, x1 = t1 + t2, x2 = t1 - t2;
      t0 = s7; t1 = s5; t2 = s3; t3 = s1;
      let p3 = t0 + t2, p4 = t1 + t3;
      p1 = t0 + t3;
      let p2 = t1 + t2;
      const p5 = (p3 + p4) * K3;
      t0 *= K4; t1 *= K5; t2 *= K6; t3 *= K7;
      p1 = p5 + p1 * K8; p2 = p5 + p2 * K9; p3 *= K10; p4 *= K11;
      t3 += p1 + p4; t2 += p2 + p3; t1 += p2 + p4; t0 += p1 + p3;
      if (!pass) {
        x0 += 512; x1 += 512; x2 += 512; x3 += 512;
        v[a] = (x0 + t3) >> 10; v[a + 56] = (x0 - t3) >> 10; v[a + 8] = (x1 + t2) >> 10; v[a + 48] = (x1 - t2) >> 10;
        v[a + 16] = (x2 + t1) >> 10; v[a + 40] = (x2 - t1) >> 10; v[a + 24] = (x3 + t0) >> 10; v[a + 32] = (x3 - t0) >> 10;
      } else {
        // Rounding, the 1 << 17 scale of both passes, and the +128 level shift.
        const r = 65536 + (128 << 17), o = off + i * stride;
        x0 += r; x1 += r; x2 += r; x3 += r;
        out[o] = clamp((x0 + t3) / 131072); out[o + 7] = clamp((x0 - t3) / 131072);
        out[o + 1] = clamp((x1 + t2) / 131072); out[o + 6] = clamp((x1 - t2) / 131072);
        out[o + 2] = clamp((x2 + t1) / 131072); out[o + 5] = clamp((x2 - t1) / 131072);
        out[o + 3] = clamp((x3 + t0) / 131072); out[o + 4] = clamp((x3 - t0) / 131072);
      }
    }
  }
}
const clamp = x => (x <= 0 ? 0 : x >= 255 ? 255 : Math.floor(x));

// Planes to RGBA: chroma upsampled by repetition, then YCbCr, RGB, CMYK or YCCK (Adobe) to RGB.
function jpegColours({ w, h, comps, hmax, vmax }, adobe) {
  const out = new Uint8ClampedArray(w * h * 4);
  const cols = comps.map(c => Int32Array.from({ length: w }, (_, x) => ((x * c.h / hmax) | 0)));
  const rgb = comps.length === 3 && (adobe === 0 || comps.map(c => c.id).join() === '82,71,66');
  const ycc = comps.length === 3 ? !rgb : adobe === 2;
  const [A, B, C, D] = comps.map(c => c.plane);
  for (let y = 0; y < h; y++) {
    const rows = comps.map(c => ((y * c.v / vmax) | 0) * c.stride);
    for (let x = 0, o = y * w * 4; x < w; x++, o += 4) {
      const a = A[rows[0] + cols[0][x]];
      if (comps.length === 1) { out[o] = out[o + 1] = out[o + 2] = a; out[o + 3] = 255; continue; }
      let r = a, g = B[rows[1] + cols[1][x]], bl = C[rows[2] + cols[2][x]];
      if (ycc) {
        const cb = g - 128, cr = bl - 128;
        r = a + 1.402 * cr; g = a - 0.344136 * cb - 0.714136 * cr; bl = a + 1.772 * cb;
      }
      if (comps.length === 4) {
        // Adobe stores CMYK inverted: each channel already is 255 - ink.
        const k = D[rows[3] + cols[3][x]] / 255;
        if (ycc) { r = 255 - r; g = 255 - g; bl = 255 - bl; }
        r *= k; g *= k; bl *= k;
      }
      out[o] = r; out[o + 1] = g; out[o + 2] = bl; out[o + 3] = 255;
    }
  }
  return { width: w, height: h, data: new Uint8Array(out.buffer) };
}

// --- rendering

const describe = (meta, why) => (meta
  ? `${meta.format} ${meta.width}×${meta.height} (${meta.note ? meta.note + ', ' : ''}${why})`
  : `Unknown image format (${why})`);

// Lines of half blocks fitting in cols × rows cells: { lines, width, height (cells), imageWidth, imageHeight,
// format }; or { info } when the image cannot be shown. Each line is a list of [text, style] segments for ui.line.
function render(buffer, cols, rows, { background = [30, 30, 30] } = {}) {
  try {
    const meta = info(buffer);
    if (!buffer || !buffer.length) return { info: 'Empty file' };
    if (buffer.length > MAX_BYTES) return { info: describe(meta, 'file too large for preview') };
    if (meta && meta.width * meta.height > MAX_PIXELS) return { info: describe(meta, 'too large for preview') };
    const img = decode(buffer);
    if (!img) return { info: describe(meta, 'no preview') };
    const { width: w, height: h, data } = img;
    const maxW = Math.max(1, cols | 0), maxH = Math.max(1, rows | 0) * 2;
    let scale = Math.min(maxW / w, maxH / h);
    // Never enlarged, except small icons by a whole factor, up to ICON pixels.
    if (scale >= 1) scale = Math.max(1, Math.min(Math.floor(scale), Math.floor(ICON / Math.max(w, h)), 4));
    const tw = Math.max(1, Math.min(maxW, Math.round(w * scale)));
    const th = Math.max(1, Math.min(maxH, Math.round(h * scale)));

    // Box filter: each target pixel averages its source area, colours weighted by alpha, then blended over the
    // background.
    const x0 = new Int32Array(tw), x1 = new Int32Array(tw);
    for (let x = 0; x < tw; x++) {
      x0[x] = Math.floor(x * w / tw);
      x1[x] = Math.max(x0[x] + 1, Math.floor((x + 1) * w / tw));
    }
    const [br, bgc, bb] = background;
    const pix = new Int32Array(tw * th);
    const acc = new Float64Array(tw * 4);
    for (let y = 0; y < th; y++) {
      const y0 = Math.floor(y * h / th), y1 = Math.max(y0 + 1, Math.floor((y + 1) * h / th));
      acc.fill(0);
      for (let sy = y0; sy < y1; sy++) {
        const row = sy * w * 4;
        for (let x = 0; x < tw; x++) {
          let r = 0, g = 0, b = 0, a = 0;
          for (let p = row + x0[x] * 4, end = row + x1[x] * 4; p < end; p += 4) {
            const al = data[p + 3];
            r += data[p] * al; g += data[p + 1] * al; b += data[p + 2] * al; a += al;
          }
          const q = x * 4;
          acc[q] += r; acc[q + 1] += g; acc[q + 2] += b; acc[q + 3] += a;
        }
      }
      for (let x = 0; x < tw; x++) {
        const q = x * 4, n = 255 * (x1[x] - x0[x]) * (y1 - y0), rest = n - acc[q + 3];
        const r = Math.round((acc[q] + br * rest) / n), g = Math.round((acc[q + 1] + bgc * rest) / n);
        const b = Math.round((acc[q + 2] + bb * rest) / n);
        pix[y * tw + x] = r << 16 | g << 8 | b;
      }
    }

    // One cell: top pixel as foreground of '▀', bottom pixel as background; equal neighbours merged.
    const back = br << 16 | bgc << 8 | bb;
    const rgb = c => [c >> 16 & 255, c >> 8 & 255, c & 255];
    const lines = [];
    for (let y = 0; y < th; y += 2) {
      const segs = [];
      let run = 0, top = -1, bottom = -1;
      for (let x = 0; x <= tw; x++) {
        const t = x < tw ? pix[y * tw + x] : -1, u = x < tw ? (y + 1 < th ? pix[(y + 1) * tw + x] : back) : -1;
        if (t === top && u === bottom) { run++; continue; }
        if (run) segs.push(['▀'.repeat(run), fg(rgb(top)) + bg(rgb(bottom))]);
        top = t; bottom = u; run = 1;
      }
      lines.push(segs);
    }
    return { lines, width: tw, height: lines.length, imageWidth: w, imageHeight: h, format: meta ? meta.format : '' };
  } catch {
    return { info: 'No preview' };
  }
}

module.exports = { FORMATS, info, decode, render };
