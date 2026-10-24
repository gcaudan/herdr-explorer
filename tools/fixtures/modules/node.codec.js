'use strict';
// Node's codecs as the JS calls them: Buffer's base64, crypto's hashes, zlib's inflate (Z_SYNC_FLUSH, bounded).
const crypto = require('crypto');
const zlib = require('zlib');

const impl = {
  base64Encode: text => Buffer.from(text, 'utf8').toString('base64'),
  base64Decode: text => [...Buffer.from(text, 'base64')],
  sha1: text => crypto.createHash('sha1').update(text).digest('hex'),
  sha256: text => crypto.createHash('sha256').update(text).digest('hex'),
  // Input and output in base64; an error gives its kind: too large, or a data error.
  inflate: (b64, max) => {
    try {
      return zlib.inflateSync(Buffer.from(b64, 'base64'), { finishFlush: zlib.constants.Z_SYNC_FLUSH, maxOutputLength: max }).toString('base64');
    } catch (e) {
      throw new Error(e.code === 'ERR_BUFFER_TOO_LARGE' || /larger than/.test(e.message) ? 'too large' : 'data');
    }
  },
};

const z = (data, opts) => zlib.deflateSync(Buffer.from(data), opts).toString('base64');
const text = 'The quick brown fox jumps over the lazy dog. '.repeat(40);
const bytes = Buffer.from(Array.from({ length: 5000 }, (_, i) => (i * 7919) % 251));
const full = z(text);
const raw = Buffer.from(full, 'base64');
// The stream with its checksum wrong.
const bad = b64 => { const b = Buffer.from(b64, 'base64'); b[b.length - 1] ^= 1; return b.toString('base64'); };
const big = Buffer.from(Array.from({ length: 40000 }, (_, i) => (i * i) % 7));
const run300 = Array.from({ length: 300 }, (_, i) => (i * 7) % 251);
const lengths = Buffer.from([...run300, ...Array.from({ length: 256 }, (_, n) => [...run300.slice(0, n + 3), 255]).flat()]);
const far = Buffer.concat([bytes, Buffer.alloc(30000, 1), bytes]);

module.exports = {
  module: 'node/codec',
  impl,
  cases: [
    ...['', 'f', 'fo', 'foo', 'foob', 'héllo ✓'].map(t => ({ fn: 'base64Encode', args: [t] })),
    ...['Zm9vYg==', 'Zm9vYg', 'Zm9v Yg==', 'Zm9vYg==Zm9v', '-_8=', 'Zm\n9v', '!!Zm9v', 'Z', 'Zm', 'Zm9', '', '====', 'Zm9vYmFy',
      'Zm9v😀Zm9v', 'Zm9Ŷ', 'Zm9vĀYg']
      .map(t => ({ fn: 'base64Decode', args: [t] })),
    ...['', 'abc', text].map(t => ({ fn: 'sha1', args: [t] })),
    ...['', 'abc', text].map(t => ({ fn: 'sha256', args: [t] })),
    { fn: 'inflate', args: [full, 1 << 20] },
    { fn: 'inflate', args: [z(text, { level: 0 }), 1 << 20] },
    { fn: 'inflate', args: [z(text, { level: 1 }), 1 << 20] },
    { fn: 'inflate', args: [z(bytes, { level: 9 }), 1 << 20] },
    { fn: 'inflate', args: [z(bytes, { strategy: zlib.constants.Z_FIXED }), 1 << 20] },
    { fn: 'inflate', args: [z(bytes, { strategy: zlib.constants.Z_HUFFMAN_ONLY }), 1 << 20] },
    { fn: 'inflate', args: [z(''), 1 << 20] },
    // Cut short: what was decoded so far.
    { fn: 'inflate', args: [raw.subarray(0, raw.length - 4).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [raw.subarray(0, 40).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [raw.subarray(0, 2).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [z(text, { level: 0 }).slice(0, 200), 1 << 20] },
    // Too much output, a bad header, a bad checksum, bad data.
    { fn: 'inflate', args: [full, 100] },
    { fn: 'inflate', args: [full, text.length] },
    { fn: 'inflate', args: [Buffer.from([0x78, 0x9d, 1, 2]).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [Buffer.concat([raw.subarray(0, raw.length - 1), Buffer.from([raw[raw.length - 1] ^ 1])]).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [Buffer.from([0x78, 0x9c, 0xff, 0xff, 0xff]).toString('base64'), 1 << 20] },
    // An incomplete code; a data error and a checksum error past the limit, in the same 16 KiB chunk.
    { fn: 'inflate', args: ['eJwFwAEJAAAAgKBt/T+VCABCAEI=', 1 << 20] },
    { fn: 'inflate', args: ['eJxzdAQ+', 1] },
    { fn: 'inflate', args: [bad(z('A'.repeat(100))), 50] },
    { fn: 'inflate', args: [bad(z(big)), 20000] },
    { fn: 'inflate', args: [bad(z(big)), 40000] },
    { fn: 'inflate', args: [z(big), big.length - 1] },
    { fn: 'inflate', args: [z(big), 16384] },
    { fn: 'inflate', args: [z(big), 16383] },
    // A preset dictionary, its id cut short or not.
    { fn: 'inflate', args: [Buffer.from([0x78, 0xbb]).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [Buffer.from([0x78, 0xbb, 1, 2, 3]).toString('base64'), 1 << 20] },
    { fn: 'inflate', args: [Buffer.from([0x78, 0xbb, 1, 2, 3, 4]).toString('base64'), 1 << 20] },
    // Every match length (zlib's own stream), and long distances.
    { fn: 'inflate', args: [z(lengths, { level: 9 }), 1 << 20] },
    { fn: 'inflate', args: [z(far), 1 << 20] },
    { fn: 'inflate', args: ['', 10] },
    { fn: 'inflate', args: ['eA==', 10] },
  ],
};
