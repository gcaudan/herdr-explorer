'use strict';
// Images built here, byte by byte: the header read (info) of every format, and the pixels decoded (decode) of PNG
// (each colour type and depth, interlaced, transparency, truncated or broken streams), GIF (palettes, transparency,
// interlacing, broken LZW), BMP (each depth, bit fields, OS/2, top-down) and ICO / CUR. The files go in base64, the
// pixels come out in hex (a hash and their first bytes when large).
const zlib = require('zlib');
const image = require('../../../src/render/image');

const hex = b => Buffer.from(b).toString('hex');
const fnv = b => { let h = 0x811c9dc5; for (const x of b) h = Math.imul(h ^ x, 0x01000193) >>> 0; return h.toString(16); };
const pixels = img => (img ? { width: img.width, height: img.height, data: img.data.length <= 512 ? hex(img.data)
  : { size: img.data.length, fnv: fnv(img.data), head: hex(img.data.subarray(0, 64)) } } : null);
const impl = {
  info: b64 => image.info(Buffer.from(b64, 'base64')),
  decode: b64 => pixels(image.decode(Buffer.from(b64, 'base64'))),
};

// Deterministic bytes.
let seed = 1;
const rand = n => { seed = (Math.imul(seed, 1103515245) + 12345) >>> 0; return (seed >>> 8) % n; };
const bytes = n => Buffer.from(Array.from({ length: n }, () => rand(256)));
const u16 = v => Buffer.from([v >> 8 & 255, v & 255]);
const u32 = v => Buffer.from([v >>> 24 & 255, v >> 16 & 255, v >> 8 & 255, v & 255]);
const le16 = v => Buffer.from([v & 255, v >> 8 & 255]);
const le32 = v => Buffer.from([v & 255, v >> 8 & 255, v >> 16 & 255, v >>> 24 & 255]);
const b64 = b => Buffer.from(b).toString('base64');

// --- PNG

const chunk = (name, data) => Buffer.concat([u32(data.length), Buffer.from(name, 'latin1'), data, u32(0)]);
const ihdr = (w, h, depth, type, interlace = 0) =>
  chunk('IHDR', Buffer.concat([u32(w), u32(h), Buffer.from([depth, type, 0, 0, interlace])]));
const SIG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const CH = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 };
const ADAM7 = [[0, 0, 8, 8], [4, 0, 8, 8], [0, 4, 4, 8], [2, 0, 4, 4], [0, 2, 2, 4], [1, 0, 2, 2], [0, 1, 1, 2]];
// Raw scanlines, each with a filter byte (0 to 4, or the one given), pixel bytes at random (palette indexes below
// the palette's size).
function scanlines(w, h, depth, type, interlace, filter, colours) {
  const bpp = CH[type] * depth, rows = [];
  for (const [x0, y0, dx, dy] of interlace ? ADAM7 : [[0, 0, 1, 1]]) {
    const pw = Math.max(0, Math.ceil((w - x0) / dx)), ph = Math.max(0, Math.ceil((h - y0) / dy));
    if (!pw || !ph) continue;
    const stride = Math.ceil(pw * bpp / 8);
    for (let y = 0; y < ph; y++) {
      const row = type === 3 ? Buffer.alloc(stride) : bytes(stride);
      if (type === 3) for (let x = 0; x < pw; x++) {
        const bit = x * depth;
        row[bit >> 3] |= rand(colours) << (8 - depth - (bit & 7));
      }
      rows.push(Buffer.from([filter ?? (type === 3 ? 0 : rand(5))]), row);
    }
  }
  return Buffer.concat(rows);
}
function png({ w = 5, h = 3, depth = 8, type = 6, interlace = 0, filter, plte, trns, raw, idat, before = [], after = [],
  cut } = {}) {
  const colours = plte ? plte.length / 3 : 1 << Math.min(depth, 8);
  raw ??= scanlines(w, h, depth, type, interlace, filter, colours);
  let z = idat ?? zlib.deflateSync(raw);
  if (cut !== undefined) z = z.subarray(0, cut);
  const chunks = [ihdr(w, h, depth, type, interlace), ...before];
  if (plte) chunks.push(chunk('PLTE', plte));
  if (trns) chunks.push(chunk('tRNS', trns));
  chunks.push(chunk('IDAT', z), ...after, chunk('IEND', Buffer.alloc(0)));
  return Buffer.concat([SIG, ...chunks]);
}

const PNGS = [];
for (const [type, depths] of [[0, [1, 2, 4, 8, 16]], [2, [8, 16]], [3, [1, 2, 4, 8]], [4, [8, 16]], [6, [8, 16]]]) {
  for (const depth of depths) {
    for (const interlace of [0, 1]) {
      const plte = type === 3 ? bytes(3 * Math.min(1 << depth, 7)) : undefined;
      PNGS.push(png({ w: 9, h: 7, depth, type, interlace, plte }));
    }
    PNGS.push(png({ w: 1, h: 1, depth, type, plte: type === 3 ? bytes(3) : undefined }));
  }
}
// A code of code lengths without any code, which zlib takes (each length 0, on 1 bit).
PNGS.push(png({ w: 1, h: 1, idat: Buffer.from('789c9d12000000160013000000', 'hex') }), png({ w: 1, h: 1, idat: Buffer.from('789c7c6000', 'hex') }));
const grey = scanlines(4, 2, 8, 0, 0, 0);
grey[1] = 0x12; grey[2] = 0x34;
const rgb16 = Buffer.from([0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0, 1, 2, 3, 4, 5]);
PNGS.push(
  // Transparency: palette alpha (shorter than the palette), grey and RGB keys at 8 and 16 bits.
  png({ w: 4, h: 2, depth: 8, type: 3, plte: bytes(12), trns: Buffer.from([0, 128]) }),
  png({ w: 4, h: 2, depth: 8, type: 0, raw: grey, trns: u16(0x12) }),
  png({ w: 4, h: 2, depth: 8, type: 0, raw: grey, trns: Buffer.from([0x12]) }),
  png({ w: 2, h: 1, depth: 16, type: 2, raw: rgb16, trns: Buffer.concat([u16(0x1234), u16(0x5678), u16(0x9abc)]) }),
  png({ w: 2, h: 1, depth: 8, type: 2, raw: Buffer.from([0, 1, 2, 3, 4, 5, 6]), trns: Buffer.concat([u16(1), u16(2), u16(3)]) }),
  png({ w: 3, h: 1, depth: 2, type: 0, raw: Buffer.from([0, 0b00011011]), trns: u16(2) }),
  png({ w: 2, h: 1, depth: 16, type: 0, raw: Buffer.from([0, 0xab, 0xcd, 0x12, 0x34]), trns: u16(0xabcd) }),
  // Palette shorter than the indexes; palette with a trailing partial colour.
  png({ w: 4, h: 1, depth: 8, type: 3, plte: bytes(4), raw: Buffer.from([0, 0, 1, 2, 255]) }),
  // Filters: each one alone, an unknown one; every filter on the first row (no row above).
  ...[0, 1, 2, 3, 4, 5, 9].map(filter => png({ w: 6, h: 4, depth: 8, type: 2, filter })),
  ...[1, 2, 3, 4].map(filter => png({ w: 6, h: 4, depth: 16, type: 6, filter })),
  // The stream: cut short at several places, split over IDAT chunks, a chunk before, chunks after IEND ignored.
  ...[0, 1, 2, 10, 30, 60].map(cut => png({ w: 9, h: 7, cut })),
  png({ w: 9, h: 7, idat: zlib.deflateSync(scanlines(9, 7, 8, 6, 0)).subarray(0, -4) }),
  png({ w: 9, h: 7, before: [chunk('tEXt', Buffer.from('a\0b'))] }),
  (() => { const z = zlib.deflateSync(scanlines(5, 3, 8, 6, 0)); return png({ w: 5, h: 3, idat: z.subarray(0, 7), after: [chunk('IDAT', z.subarray(7))] }); })(),
  Buffer.concat([png({ w: 2, h: 2 }), chunk('IDAT', Buffer.from([1, 2, 3]))]),
  // A stream with fewer or more rows than the image; raw data past the bound (size + 1024).
  png({ w: 5, h: 3, raw: scanlines(5, 2, 8, 6, 0) }),
  png({ w: 5, h: 3, raw: Buffer.concat([scanlines(5, 3, 8, 6, 0), Buffer.alloc(1000)]) }),
  png({ w: 5, h: 3, raw: Buffer.concat([scanlines(5, 3, 8, 6, 0), Buffer.alloc(1100)]) }),
  png({ w: 5, h: 3, raw: Buffer.concat([scanlines(5, 3, 8, 6, 0), Buffer.alloc(40000)]) }),
  // Broken: zlib header, data, checksum; stored blocks.
  png({ w: 5, h: 3, idat: Buffer.from([0x78, 0x9d, 1, 2, 3]) }),
  png({ w: 5, h: 3, idat: Buffer.from([0x78, 0x9c, 0xff, 0xff, 0xff, 0xff]) }),
  (() => { const z = zlib.deflateSync(scanlines(5, 3, 8, 6, 0)); z[z.length - 1] ^= 1; return png({ w: 5, h: 3, idat: z }); })(),
  png({ w: 5, h: 3, idat: zlib.deflateSync(scanlines(5, 3, 8, 6, 0), { level: 0 }) }),
  png({ w: 5, h: 3, idat: zlib.deflateSync(scanlines(5, 3, 8, 6, 0), { level: 0 }).subarray(0, 20) }),
  // Refused: depth and colour type, size, no IDAT, no palette, not IHDR first, too short.
  png({ w: 2, h: 2, depth: 16, type: 3, plte: bytes(6), raw: Buffer.alloc(10) }),
  png({ w: 2, h: 2, depth: 4, type: 2, raw: Buffer.alloc(10) }),
  png({ w: 2, h: 2, depth: 8, type: 5, raw: Buffer.alloc(10) }),
  png({ w: 2, h: 2, depth: 3, type: 0, raw: Buffer.alloc(10) }),
  png({ w: 0, h: 2, raw: Buffer.alloc(10) }),
  png({ w: 8000, h: 8000, raw: Buffer.alloc(10) }),
  png({ w: 7000, h: 7000, depth: 16, raw: Buffer.alloc(10) }),
  png({ w: 2, h: 2, type: 3, depth: 8, raw: Buffer.alloc(6) }),
  Buffer.concat([SIG, ihdr(2, 2, 8, 6), chunk('IEND', Buffer.alloc(0))]),
  Buffer.concat([SIG, chunk('tEXt', Buffer.from('x')), ihdr(2, 2, 8, 6)]),
  Buffer.concat([SIG, ihdr(2, 2, 8, 6)]).subarray(0, 30),
  Buffer.concat([SIG, ihdr(2, 2, 8, 6), u32(0xffffffff), Buffer.from('IDAT'), Buffer.alloc(8)]),
);

// --- GIF

function lzwEncode(indices, min) {
  const clear = 1 << min, eoi = clear + 1, out = [];
  let size = min + 1, next = eoi + 1, acc = 0, bits = 0, dict = new Map(), w = null;
  const put = c => { acc |= c << bits; bits += size; while (bits >= 8) { out.push(acc & 255); acc >>>= 8; bits -= 8; } };
  put(clear);
  for (const k of indices) {
    if (w === null) { w = k; continue; }
    const key = w * 4096 + k;
    if (dict.has(key)) { w = dict.get(key); continue; }
    put(w);
    if (next < 4096) {
      dict.set(key, next++);
      if (next > 1 << size && size < 12) size++;
    } else {
      put(clear); dict = new Map(); size = min + 1; next = eoi + 1;
    }
    w = k;
  }
  if (w !== null) put(w);
  put(eoi);
  if (bits) out.push(acc & 255);
  return Buffer.from(out);
}
const blocks = data => {
  const out = [];
  for (let i = 0; i < data.length; i += 255) { const b = data.subarray(i, i + 255); out.push(Buffer.from([b.length]), b); }
  return Buffer.concat([...out, Buffer.from([0])]);
};
const palette = (n, depth) => { n = Math.min(n, 1 << depth); return Buffer.concat([bytes(3 * n), Buffer.alloc(3 * (1 << depth) - 3 * n)]); };
function gif({ sw = 6, sh = 4, gct = 3, x = 0, y = 0, w = sw, h = sh, lct = null, interlace = false, min = 2, colours = 4,
  trans = null, ext = [], data, idx, trailer = true, cut } = {}) {
  idx ??= Array.from({ length: w * h }, () => rand(colours));
  let order = idx;
  if (interlace) {
    order = [];
    for (const [s, step] of [[0, 8], [4, 8], [2, 4], [1, 2]]) for (let r = s; r < h; r += step) order.push(...idx.slice(r * w, r * w + w));
  }
  const parts = [Buffer.from('GIF89a', 'latin1'), le16(sw), le16(sh), Buffer.from([gct === null ? 0 : 0x80 | gct, 0, 0])];
  if (gct !== null) parts.push(palette(colours, gct + 1));
  if (trans !== null) parts.push(Buffer.from([0x21, 0xf9, 4, 1, 0, 0, trans, 0]));
  parts.push(...ext, Buffer.from([0x2c]), le16(x), le16(y), le16(w), le16(h));
  parts.push(Buffer.from([(lct === null ? 0 : 0x80 | lct) | (interlace ? 0x40 : 0)]));
  if (lct !== null) parts.push(palette(colours, lct + 1));
  parts.push(Buffer.from([min]), blocks(data ?? lzwEncode(order, min)));
  if (trailer) parts.push(Buffer.from([0x3b]));
  const b = Buffer.concat(parts);
  return cut === undefined ? b : b.subarray(0, cut);
}
// The encoder checked against the JS decoder.
for (const [w, h, min, colours] of [[6, 4, 2, 4], [40, 30, 2, 3], [100, 80, 8, 256], [5, 5, 1, 2]]) {
  const idx = Array.from({ length: w * h }, () => rand(colours));
  const img = image.decode(gif({ sw: w, sh: h, gct: Math.max(0, min - 1), min, colours, idx }));
  if (!img || img.width !== w) throw new Error('lzw encoder');
}

const GIFS = [
  gif(), gif({ sw: 40, sh: 30, colours: 3 }), gif({ sw: 120, sh: 90, min: 8, colours: 256, gct: 7 }),
  gif({ sw: 64, sh: 64, min: 8, colours: 200, gct: 7, idx: Array.from({ length: 4096 }, (_, i) => (i * 7) % 200) }),
  gif({ min: 1, colours: 2, gct: 0 }), gif({ gct: null, lct: 2, colours: 6 }), gif({ gct: null, colours: 4 }),
  gif({ trans: 1 }), gif({ trans: 0, ext: [Buffer.from([0x21, 0xfe, 3, 0x61, 0x62, 0x63, 0])] }),
  gif({ ext: [Buffer.from([0x21, 0xf9, 3, 1, 0, 0, 0])] }), gif({ ext: [Buffer.from([0x21, 0xf9, 4, 0, 0, 0, 1, 0])] }),
  gif({ sw: 7, sh: 11, interlace: true }), gif({ sw: 4, sh: 3, x: 3, y: 2, w: 4, h: 3 }), gif({ sw: 10, sh: 8, x: 2, y: 1, w: 3, h: 4 }),
  gif({ colours: 4, gct: 0 }), gif({ idx: Array(24).fill(3), gct: 0 }),
  gif({ data: Buffer.from([0x4c]) }), gif({ data: Buffer.from([0x0c, 0x0f]) }), gif({ data: Buffer.from([]) }),
  gif({ data: Buffer.from([0xff, 0xff, 0xff]) }), gif({ min: 0 }), gif({ min: 12, data: Buffer.from([1, 2]) }),
  gif({ min: 11, data: Buffer.from([0, 8, 0, 0]) }), gif({ trailer: false }), gif({ w: 0, h: 0, idx: [] }),
  ...[13, 20, 30, 34, 36, 40, 45].map(cut => gif({ cut })), gif({ ext: [Buffer.from([0x99])] }),
  gif({ sw: 9000, sh: 9000, w: 1, h: 1 }), gif({ sw: 1, sh: 1, x: 7999, y: 7999, w: 1, h: 1 }),
];

// --- BMP

function dib({ hs = 40, w = 5, h = 3, bpp = 24, comp = 0, used = 0, masks, pal, rows, alphaMask, extraMasks } = {}) {
  const full = Buffer.alloc(Math.max(hs, 40)), head = full; // fields past hs cut below
  if (hs === 12) { head.writeUInt32LE(12, 0); head.writeUInt16LE(w, 4); head.writeUInt16LE(h & 0xffff, 6); head.writeUInt16LE(1, 8); head.writeUInt16LE(bpp, 10); }
  else {
    head.writeUInt32LE(hs, 0); head.writeInt32LE(w, 4); head.writeInt32LE(h, 8); head.writeUInt16LE(1, 12);
    head.writeUInt16LE(bpp, 14); head.writeUInt32LE(comp, 16); head.writeUInt32LE(used, 32);
    if (masks && hs >= 52) masks.forEach((m, i) => { if (40 + i * 4 + 4 <= hs) head.writeUInt32LE(m >>> 0, 40 + i * 4); });
  }
  const after = masks && hs < 52 ? Buffer.concat(masks.slice(0, extraMasks ?? 3).map(m => le32(m))) : Buffer.alloc(0);
  const colours = bpp <= 8 ? (used || 1 << bpp) : 0;
  pal ??= bytes(colours * (hs === 12 ? 3 : 4));
  const stride = ((Math.abs(w) * bpp + 31) >>> 5) * 4;
  rows ??= bytes(stride * Math.abs(h));
  return Buffer.concat([full.subarray(0, hs), after, pal, rows, alphaMask ?? Buffer.alloc(0)]);
}
const bmp = (opts, cut) => {
  const d = dib(opts);
  const b = Buffer.concat([Buffer.from('BM', 'latin1'), le32(14 + d.length), le32(0), le32(0), d]);
  const hs = opts.hs ?? 40;
  const masks = opts.masks && hs < 52 ? 4 * (opts.extraMasks ?? 3) : 0;
  const bpp = opts.bpp ?? 24, colours = bpp <= 8 ? (opts.used || 1 << bpp) : 0;
  b.writeUInt32LE(opts.data ?? 14 + hs + masks + (opts.pal ? opts.pal.length : colours * (hs === 12 ? 3 : 4)), 10);
  return cut === undefined ? b : b.subarray(0, cut);
};
const BMPS = [
  ...[1, 2, 4, 8, 16, 24, 32].map(bpp => bmp({ bpp })), ...[1, 4, 8, 24].map(bpp => bmp({ hs: 12, bpp })),
  bmp({ bpp: 8, used: 3 }), bmp({ bpp: 4, used: 20 }), bmp({ bpp: 8, used: 2, rows: Buffer.from([0, 1, 2, 255, 0, 0, 0, 0, 1, 1, 1, 1, 3, 2, 1, 0, 9, 9, 9, 9]).subarray(0, 8 * 3), w: 5, h: 3 }),
  bmp({ h: -3 }), bmp({ h: -3, bpp: 8 }), bmp({ w: -5 }), bmp({ w: 0 }), bmp({ hs: 124 }), bmp({ hs: 108, bpp: 32 }),
  bmp({ bpp: 16, comp: 3, masks: [0xf800, 0x7e0, 0x1f] }), bmp({ bpp: 16, comp: 3, masks: [0xf00, 0xf0, 0xf], extraMasks: 3 }),
  bmp({ bpp: 32, comp: 3, masks: [0xff0000, 0xff00, 0xff] }), bmp({ bpp: 32, comp: 6, masks: [0xff0000, 0xff00, 0xff, 0xff000000], extraMasks: 4 }),
  bmp({ hs: 56, bpp: 32, comp: 3, masks: [0xff0000, 0xff00, 0xff, 0xff000000] }),
  bmp({ hs: 56, bpp: 32, comp: 3, masks: [0xff0000, 0xff00, 0xff, 0xff000000], rows: Buffer.alloc(60) }),
  bmp({ hs: 52, bpp: 32, comp: 3, masks: [0xff0000, 0xff00, 0xff] }),
  bmp({ hs: 56, bpp: 32, comp: 3, masks: [0x7fffffff, 0x80000000, 0, 1] }), bmp({ hs: 56, bpp: 32, comp: 3, masks: [0xffffffff, 0x3ff, 0xffc00, 0] }),
  bmp({ bpp: 16, comp: 3, masks: [0, 0, 0] }), bmp({ bpp: 24, comp: 3, masks: [0xff, 0xff00, 0xff0000] }),
  bmp({ comp: 1 }), bmp({ comp: 4 }), bmp({ bpp: 3 }), bmp({ hs: 20 }), bmp({ hs: 300 }),
  bmp({ bpp: 24 }, 60), bmp({ bpp: 24 }, 20), bmp({ bpp: 8 }, 100), bmp({ bpp: 8, data: 5000 }), bmp({ w: 9000, h: 9000, rows: Buffer.alloc(200) }),
];

// --- ICO / CUR

const ico = (entries, type = 1) => {
  const head = Buffer.concat([le16(0), le16(type), le16(entries.length)]);
  let off = 6 + 16 * entries.length;
  const dir = [], data = [];
  for (const e of entries) {
    const d = e.data;
    dir.push(Buffer.from([e.w & 255, e.h & 255, 0, 0]), le16(1), le16(e.bpp ?? 32), le32(e.size ?? d.length), le32(e.off ?? off));
    data.push(d); off += d.length;
  }
  return Buffer.concat([head, ...dir, ...data]);
};
const icoDib = (w, h, bpp, opts = {}) => dib({ w, h: h * 2, bpp, alphaMask: opts.mask ?? bytes(((w + 31) >>> 5) * 4 * h), ...opts });
const ICOS = [
  ico([{ w: 4, h: 4, bpp: 32, data: icoDib(4, 4, 32) }]),
  ico([{ w: 4, h: 4, bpp: 32, data: icoDib(4, 4, 32, { rows: Buffer.concat([Buffer.alloc(32), bytes(32)]).map((x, i) => (i % 4 === 3 ? 0 : x)) }) }]),
  ico([{ w: 3, h: 2, bpp: 24, data: icoDib(3, 2, 24) }]), ico([{ w: 8, h: 2, bpp: 4, data: icoDib(8, 2, 4) }]),
  ico([{ w: 8, h: 2, bpp: 1, data: icoDib(8, 2, 1) }]), ico([{ w: 2, h: 2, bpp: 24, data: icoDib(2, 2, 24, { mask: Buffer.alloc(2) }) }]),
  ico([{ w: 2, h: 2, bpp: 32, data: icoDib(2, 2, 32) }, { w: 4, h: 4, bpp: 24, data: icoDib(4, 4, 24) }, { w: 4, h: 4, bpp: 32, data: icoDib(4, 4, 32) }]),
  ico([{ w: 5, h: 3, bpp: 32, data: png({ w: 5, h: 3 }) }]), ico([{ w: 0, h: 0, bpp: 32, data: icoDib(2, 2, 32) }]),
  ico([{ w: 3, h: 2, bpp: 24, data: icoDib(3, 2, 24) }], 2), ico([{ w: 3, h: 2, bpp: 24, data: icoDib(3, 2, 24), off: 9999 }]),
  ico([{ w: 3, h: 2, bpp: 24, data: icoDib(3, 2, 24), size: 20 }]), ico([]),
  ico([{ w: 3, h: 2, bpp: 24, data: icoDib(3, 2, 24, { h: -4 }) }]), ico([{ w: 3, h: 2, bpp: 24, data: icoDib(3, 2, 24) }]).subarray(0, 30),
];

// --- headers only

const jpeg = (...segments) => Buffer.concat([Buffer.from([0xff, 0xd8]), ...segments, Buffer.from([0xff, 0xd9])]);
const seg = (m, data) => Buffer.concat([Buffer.from([0xff, m]), u16(data.length + 2), data]);
const sof = (m, w, h, bits = 8) => seg(m, Buffer.concat([Buffer.from([bits]), u16(h), u16(w), Buffer.from([1, 1, 0x11, 0])]));
const tiff = (le, entries, extra = Buffer.alloc(0)) => {
  const w16 = v => (le ? le16(v) : u16(v)), w32 = v => (le ? le32(v) : u32(v));
  const head = Buffer.concat([Buffer.from(le ? 'II' : 'MM', 'latin1'), w16(42), w32(8), w16(entries.length)]);
  const dir = entries.map(([tag, type, n, value]) => Buffer.concat([w16(tag), w16(type), w32(n),
    typeof value === 'number' ? (type === 3 && n === 1 ? Buffer.concat([w16(value), w16(0)]) : w32(value)) : value]));
  return Buffer.concat([head, ...dir, w32(0), extra]);
};
const riff = (chunkName, body) => Buffer.concat([Buffer.from('RIFF', 'latin1'), le32(4 + 8 + body.length),
  Buffer.from('WEBP', 'latin1'), Buffer.from(chunkName, 'latin1'), le32(body.length), body]);
const svg = s => Buffer.from(s, 'latin1');
const HEADERS = [
  jpeg(sof(0xc0, 640, 480)), jpeg(seg(0xe0, Buffer.from('JFIF\0')), sof(0xc2, 10, 20)), jpeg(sof(0xc1, 3, 4, 12)),
  jpeg(sof(0xc3, 5, 6)), jpeg(sof(0xc5, 5, 6)), jpeg(sof(0xc6, 5, 6)), jpeg(sof(0xc9, 5, 6)), jpeg(sof(0xca, 5, 6)),
  jpeg(sof(0xcb, 5, 6)), jpeg(sof(0xcd, 5, 6)), jpeg(sof(0xce, 5, 6)), jpeg(sof(0xcf, 5, 6)), jpeg(sof(0xc0, 0, 6)),
  jpeg(seg(0xc4, Buffer.alloc(20)), seg(0xcc, Buffer.alloc(4)), sof(0xc0, 7, 8)), jpeg(seg(0xda, Buffer.alloc(10)), sof(0xc0, 7, 8)),
  Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xff, 0xff, 0xd0, 0xff, 0x01, 0x00, 0x12]), sof(0xc0, 9, 9), Buffer.alloc(4)]),
  jpeg(seg(0xe1, Buffer.alloc(300)), sof(0xc0, 1, 1)), jpeg(sof(0xc0, 1, 1)).subarray(0, 14),
  Buffer.concat([Buffer.from('GIF87a', 'latin1'), le16(300), le16(200), Buffer.alloc(6)]),
  Buffer.concat([Buffer.from('GIF89a', 'latin1'), le16(0), le16(5), Buffer.alloc(6)]),
  tiff(true, [[256, 3, 1, 120], [257, 4, 1, 80]]), tiff(false, [[256, 4, 1, 120], [257, 3, 1, 80]]),
  tiff(true, [[256, 5, 1, 26], [257, 5, 1, 34]], Buffer.concat([le32(7), le32(2), le32(9), le32(0)])),
  tiff(true, [[256, 3, 1, 5]]), tiff(true, [[256, 3, 0, 0], [257, 3, 1, 5]]), tiff(true, [[256, 3, 1, 5], [257, 3, 1, 6], [256, 3, 1, 9]]),
  tiff(true, [[256, 1, 1, 7], [257, 6, 1, 8]]), tiff(true, [[256, 3, 1, 4], [257, 4, 1, 5], [258, 3, 3, 100]]),
  tiff(true, [[256, 12, 1, 4], [257, 3, 1, 5]]), tiff(true, [[256, 4, 1 << 25, 4], [257, 3, 1, 5]]), tiff(true, []),
  Buffer.concat([Buffer.from('II*\0', 'latin1'), le32(9999), Buffer.alloc(8)]),
  Buffer.concat([Buffer.from('MM\0*', 'latin1'), u32(8), u16(2000), Buffer.alloc(8)]),
  Buffer.concat([Buffer.from('BM', 'latin1'), Buffer.alloc(12), le32(12), le16(30), le16(20), Buffer.alloc(8)]),
  Buffer.concat([Buffer.from('BM', 'latin1'), Buffer.alloc(12), le32(40), le32(30), le32(-20 >>> 0), Buffer.alloc(8)]),
  Buffer.concat([Buffer.from('BM', 'latin1'), Buffer.alloc(12), le32(40), le32(30)]),
  riff('VP8 ', Buffer.concat([Buffer.alloc(6), le16(0x4000 | 320), le16(240), Buffer.alloc(4)])),
  riff('VP8L', Buffer.concat([Buffer.from([0x2f]), le32((99) | (49 << 14)), Buffer.alloc(8)])),
  riff('VP8L', Buffer.concat([Buffer.from([0x2e]), le32(5), Buffer.alloc(8)])),
  riff('VP8X', Buffer.concat([Buffer.from([2, 0, 0, 0]), Buffer.from([0x3f, 1, 0, 0xc7, 0, 0]), Buffer.alloc(4)])),
  riff('VP8X', Buffer.concat([Buffer.from([0x10, 0, 0, 0]), Buffer.from([0, 0, 0, 0, 0, 0]), Buffer.alloc(4)])),
  riff('ALPH', Buffer.alloc(20)), riff('VP8 ', Buffer.alloc(2)),
  svg('<svg width="100" height="50"></svg>'), svg('\xef\xbb\xbf  \n<svg xmlns="x" viewBox="0 0 30 20">'),
  svg('<?xml version="1.0"?>\n<SVG Width=\'2in\' HEIGHT="3cm" ViewBox="0,0,1,1">'), svg('<svg width="10mm" viewBox="0 0 40 20">'),
  svg('<svg height="12pt" viewBox="0 0 40 20">'), svg('<svg width="100%" height="2pc">'), svg('<svg width="1e999" height="5">'),
  svg('<svg width="1.2.3" height="5" viewBox="0 0 8 4">'), svg('<svg width=" 7.5 px " height="2.5e1">'),
  svg('<svg viewBox="0 0 -3 4">'), svg('<svg viewBox="0 0 3">'), svg('<svg viewBox=" 1, 2 ,3 4 ">'), svg('<svgx width="3" height="4">'),
  svg('<svg width="3"height="4">'), svg('<svg  width = "0.4" height = "0.5" >'), svg('<html><svg width="3" height="4"></html>'),
  svg('text <svg width="3" height="4">'), svg('<svg width="3" height="4"'), svg('<svg width="3" data-width="9" height="4">'),
  svg(`<!-- ${'x'.repeat(70000)} --><svg width="3" height="4">`), svg('<svg\twidth="0xff"\theight="4">'),
  svg('\xa0<svg width="3" height="4">'),
  svg('<' + '<svg '.repeat(3000)), svg('<svgx width="1"><svg width="3" height="4">'), svg('<SVG_ width="1"><Svg width="5" height="4">'),
  svg('<svg'), svg('<svg>'), svg('<svg\n width="3" height="4"\n>'),
  Buffer.alloc(0), Buffer.alloc(11), Buffer.from('not an image at all'), bytes(64), Buffer.from([0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
  Buffer.from([0xff, 0xd8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
];

const cases = [];
for (const b of [...PNGS, ...GIFS, ...BMPS, ...ICOS]) {
  cases.push({ fn: 'info', args: [b64(b)] }, { fn: 'decode', args: [b64(b)] });
}
for (const b of HEADERS) cases.push({ fn: 'info', args: [b64(b)] });
for (const b of HEADERS.slice(-6)) cases.push({ fn: 'decode', args: [b64(b)] });

module.exports = { module: 'render/image', impl, cases };
