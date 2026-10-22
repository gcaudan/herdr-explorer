// Review features: the terminal's replies kept apart from the keys, the line a note set sends, the lines each diff
// row shows, and the TIFF decoder.
const test = require('node:test');
const assert = require('node:assert');
const zlib = require('zlib');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const ui = require('../src/core/ui');
const notes = require('../src/core/notes');
const diff = require('../src/diff');
const image = require('../src/render/image');

test('terminal replies never read as keys', () => {
  const keys = [], replies = [];
  ui.decode('a\x1b_Gi=31;OK\x1b\\b\x1b[6;16;8tc', { key: k => keys.push(k), reply: r => replies.push(r) });
  assert.deepStrictEqual(keys, ['a', 'b', 'c']);
  assert.deepStrictEqual(replies, ['\x1b_Gi=31;OK\x1b\\', '\x1b[6;16;8t']);
  const mouse = [];
  ui.decode('\x1b[<0;5;7M', { mouse: m => mouse.push(m) });
  assert.deepStrictEqual([mouse[0].x, mouse[0].y, mouse[0].press], [5, 7, true]);
});

test('notes go on one line, sorted, relative to the terminal', () => {
  const list = [
    { file: '/r/src/b.js', from: 3, to: 3, kind: '', text: 'why', t: 2 },
    { file: '/r/src/a.js', from: 12, to: 14, kind: 'fix', text: 'rename\nit', t: 1 },
    { file: '/r/README.md', from: 0, to: 0, kind: '', text: 'add an example', t: 3 },
  ];
  const line = notes.format(list, '/r');
  assert.strictEqual(line, 'Review notes: README.md: add an example; src/a.js:12-14 (fix): rename it; src/b.js:3: why ');
  assert.ok(!/[\x00-\x1f]/.test(line));
  assert.strictEqual(notes.where(list[0], '/elsewhere'), '/r/src/b.js:3');
});

test('notes follow their lines, or go stale once they are gone', () => {
  const n = { file: '/r/a.js', from: 3, to: 4, text: 'x', anchor: ['c', 'd'] };
  assert.strictEqual(notes.locate(n, ['a', 'b', 'c', 'd']), n, 'in place: the same note');
  assert.deepStrictEqual([notes.locate(n, ['new', 'a', 'b', 'c', 'd']).from, notes.locate(n, ['new', 'a', 'b', 'c', 'd']).to], [4, 5]);
  // In several places: the nearest to where it was.
  assert.strictEqual(notes.locate(n, ['c', 'd', 'x', 'x', 'x', 'c', 'd']).from, 1);
  assert.strictEqual(notes.locate({ ...n, from: 5, to: 6 }, ['c', 'd', 'x', 'x', 'x', 'c', 'd']).from, 6);
  const stale = notes.locate(n, ['a', 'b', 'c!', 'd']);
  assert.strictEqual(stale.stale, true);
  assert.strictEqual(stale.from, 3);
  // Back once the lines are: no longer stale.
  const back = notes.locate(stale, ['c', 'd']);
  assert.deepStrictEqual([back.from, back.stale], [1, undefined]);
  assert.strictEqual(notes.locate(n, null).stale, true, 'the file gone');
  assert.strictEqual(notes.locate(n, undefined), n, 'the lines not known: as it was');
  const whole = { file: '/r/a.js', from: 0, to: 0, text: 'x' };
  assert.strictEqual(notes.locate(whole, []), whole);
  assert.strictEqual(notes.format([{ ...stale, kind: 'fix' }], '/r', ' Fix these:'), 'Fix these: Review notes: a.js:3-4 (fix, stale): x ');
});

test('a file stays viewed until its content changes', () => {
  const viewed = require('../src/core/viewed');
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'viewed-')), 'a.txt');
  fs.writeFileSync(file, 'one\n');
  const map = { [file]: viewed.sign(file) };
  assert.ok(viewed.is(map, file));
  fs.writeFileSync(file, 'two\n');
  assert.ok(!viewed.is(map, file));
  fs.unlinkSync(file);
  assert.strictEqual(viewed.sign(file), 'gone');
});

test('notes stay relative from a folder reached through a link', () => {
  const real = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'notes-')));
  fs.mkdirSync(path.join(real, 'src'));
  const link = `${real}-link`;
  try { fs.symlinkSync(real, link, 'dir'); } catch { return; } // links not allowed (Windows without developer mode)
  const n = { file: notes.real(path.join(link, 'src', 'a.js')), from: 12, to: 12 };
  assert.strictEqual(n.file, path.join(real, 'src', 'a.js'));
  assert.strictEqual(notes.where(n, link), 'src/a.js:12');
  fs.unlinkSync(link);
});

test('untracked files of a subfolder are named from the repository', async () => {
  const top = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'diff-')));
  if (spawnSync('git', ['init', '-q'], { cwd: top }).status !== 0) return;
  fs.mkdirSync(path.join(top, 'app', 'src'), { recursive: true });
  fs.writeFileSync(path.join(top, 'app', 'src', 'new.js'), 'x\n');
  const { text } = await diff.collect({ cwd: path.join(top, 'app') });
  assert.deepStrictEqual(diff.parse(text).files.map(f => f.to), ['app/src/new.js']);
});

test('diff scopes: a commit, a range, a branch, the last agent turn', async () => {
  const top = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'scope-')));
  const git = (...a) => spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { cwd: top, encoding: 'utf8' });
  if (git('init', '-q', '-b', 'main').status !== 0) return;
  const commit = (file, text) => { fs.writeFileSync(path.join(top, file), text); git('add', '-A'); git('commit', '-q', '-m', file); };
  commit('a.txt', 'one\n');
  commit('b.txt', 'two\n');
  git('switch', '-q', '-c', 'feature');
  commit('c.txt', 'three\n');
  const names = async scope => diff.parse((await diff.collect({ cwd: top, scope })).text).files.map(f => f.to);
  assert.deepStrictEqual(await names({ kind: 'rev', rev: 'main' }), ['b.txt']);
  assert.deepStrictEqual(await names({ kind: 'rev', rev: 'main...feature' }), ['c.txt']);
  assert.deepStrictEqual(await names({ kind: 'rev', rev: 'HEAD~2..HEAD' }), ['b.txt', 'c.txt']);
  fs.writeFileSync(path.join(top, 'new.txt'), 'x\n');
  assert.deepStrictEqual(await names({ kind: 'ref', rev: 'main' }), ['c.txt', 'new.txt']);
  assert.ok((await diff.collect({ cwd: top, scope: { kind: 'rev', rev: 'nope' } })).error);
  // The turn: what changed since its snapshot, untracked files included; the index stays as it was.
  const Scope = require('../src/core/scope');
  const tree = await Scope.worktree(top, process.env);
  assert.strictEqual(git('status', '--porcelain').stdout, '?? new.txt\n');
  fs.writeFileSync(path.join(top, 'a.txt'), 'one!\n');
  fs.writeFileSync(path.join(top, 'other.txt'), 'y\n');
  const turn = { top, tree };
  const res = await diff.collect({ cwd: top, scope: { kind: 'turn' }, turn });
  assert.deepStrictEqual(diff.parse(res.text).files.map(f => f.to), ['a.txt', 'other.txt']);
  assert.ok((await diff.collect({ cwd: top, scope: { kind: 'turn' }, turn: null })).error);
});

test('diff rows know the lines they show', () => {
  const text = ['diff --git a/x.js b/x.js', '--- a/x.js', '+++ b/x.js', '@@ -1,3 +1,3 @@', ' one', '-two', '+2', ' three', ''].join('\n');
  const parsed = diff.parse(text);
  for (const mode of ['inline', 'split']) {
    const refs = diff.render(parsed, 100, mode).filter(r => r.ref).map(r => `${r.ref.from}-${r.ref.to}`);
    // The file, the hunk, then its lines: the removed line is at the place of the added one.
    assert.deepStrictEqual(refs, mode === 'inline' ? ['0-0', '1-3', '1-1', '2-2', '2-2', '3-3'] : ['0-0', '1-3', '1-1', '2-2', '3-3'], mode);
  }
});

test('diff rows wrap long lines, or scroll them from a column', () => {
  const long = 'abcdefghij'.repeat(6);
  const text = ['diff --git a/x.js b/x.js', '--- a/x.js', '+++ b/x.js', '@@ -1,2 +1,2 @@', ' short', `-${long}`, `+${long}!`, ''].join('\n');
  const parsed = diff.parse(text);
  const plain = r => r.segs.map(([t]) => t).join('');
  for (const mode of ['inline', 'split']) {
    const rows = diff.render(parsed, 40, mode, true, { wrap: true });
    // Every row fills the width; a wrapped line goes on with rows marked more, at the same place.
    assert.ok(rows.filter(r => r.place).every(r => ui.segWidth(r.segs) === 40), mode);
    const more = rows.filter(r => r.more);
    assert.ok(more.length >= 2 && more.every(r => r.place === 2), mode);
    assert.strictEqual(diff.render(parsed, 40, mode, true).filter(r => r.more).length, 0, mode);
  }
  const cut = diff.render(parsed, 40, 'inline', true, { left: 10 });
  // The hunk's header, then the lines: the gutter stays, the text starts at its 11th column.
  assert.ok(!plain(cut[1]).includes('short'), 'the short line scrolled out');
  assert.ok(plain(cut[2]).includes(` ${long.slice(10, 30)}`));
  assert.ok(cut.filter(r => r.place).every(r => ui.segWidth(r.segs) === 40));
});

// A little-endian TIFF of the given pixels, one strip, with the tags given (tag → [type, values]).
function tiff(strip, tags) {
  const entries = Object.entries(tags).map(([t, v]) => [+t, ...v]).sort((a, b) => a[0] - b[0]);
  const dir = 8 + strip.length, head = Buffer.alloc(8);
  head.write('II'); head.writeUInt16LE(42, 2); head.writeUInt32LE(dir, 4);
  const ifd = Buffer.alloc(2 + entries.length * 12 + 4);
  ifd.writeUInt16LE(entries.length, 0);
  entries.forEach(([tag, type, values], k) => {
    const e = 2 + k * 12;
    ifd.writeUInt16LE(tag, e); ifd.writeUInt16LE(type, e + 2); ifd.writeUInt32LE(values.length, e + 4);
    values.forEach((v, i) => (type === 3 ? ifd.writeUInt16LE(v, e + 8 + i * 2) : ifd.writeUInt32LE(v, e + 8 + i * 4)));
  });
  return Buffer.concat([head, strip, ifd]);
}
const rgbTags = (w, h, comp, size) => ({ 256: [3, [w]], 257: [3, [h]], 258: [3, [8]], 259: [3, [comp]], 262: [3, [2]],
  273: [4, [8]], 277: [3, [3]], 278: [3, [h]], 279: [4, [size]] });
// 9-bit LZW codes, most significant bit first: Clear, each byte as itself, End. Short data keeps the code size at 9.
function lzw(bytes) {
  let bits = '';
  for (const c of [256, ...bytes, 257]) bits += c.toString(2).padStart(9, '0');
  bits = bits.padEnd(Math.ceil(bits.length / 8) * 8, '0');
  return Buffer.from(bits.match(/.{8}/g).map(b => parseInt(b, 2)));
}

test('TIFF strips decode, whatever their compression', () => {
  const px = [255, 0, 0, 0, 255, 0, 0, 0, 255, 10, 20, 30];
  const want = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 255];
  const strips = {
    1: Buffer.from(px),
    // PackBits: 2 bytes as they are, then 0 repeated twice, then the rest as it is.
    32773: Buffer.from([1, 255, 0, -1 & 255, 0, 7, 255, 0, 0, 0, 255, 10, 20, 30]),
    5: lzw(px),
    8: zlib.deflateSync(Buffer.from(px)),
  };
  for (const [comp, strip] of Object.entries(strips)) {
    const img = image.decode(tiff(strip, rgbTags(2, 2, +comp, strip.length)));
    assert.ok(img, `compression ${comp}`);
    assert.deepStrictEqual([img.width, img.height, ...img.data], [2, 2, ...want], `compression ${comp}`);
  }
  assert.deepStrictEqual(image.info(tiff(strips[1], rgbTags(2, 2, 1, 12))), { format: 'TIFF', width: 2, height: 2 });
});

test('images scale to fit, never above their size', () => {
  const png = tiff(Buffer.from(new Array(4 * 4 * 3).fill(128)), rgbTags(4, 4, 1, 48));
  const small = image.pixels(png, 2, 2);
  assert.deepStrictEqual([small.width, small.height, small.data.length], [2, 2, 16]);
  const big = image.pixels(png, 1000, 1000);
  assert.deepStrictEqual([big.width, big.height], [16, 16]); // a 4-pixel icon, enlarged 4 times at most
});

// A 24 × 16 progressive JPEG (orange to navy, a white disc), 4:2:0, with refinement scans: made by ImageMagick, whose
// own decoding gives the expected colours.
const PROGRESSIVE = '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAMCAgMCAgMDAwMEAwMEBQgFBQQEBQoHBwYIDAoMDAsKCwsNDhIQDQ4RDgsLEBYQERMUFRUVDA8XGBYUGBIUFRT/2wBDAQMEBAUEBQkFBQkUDQsNFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBT/wgARCAAQABgDASIAAhEBAxEB/8QAGAAAAwEBAAAAAAAAAAAAAAAAAAcIBAb/xAAWAQEBAQAAAAAAAAAAAAAAAAAFAwT/2gAMAwEAAhADEAAAAdzgjOoDV+VEyT0f/8QAGhAAAwEBAQEAAAAAAAAAAAAAAwQFAgYHI//aAAgBAQABBQIdTRdj5f4OuEQaXv6WOD1mHtKv1OqtL//EABsRAAEFAQEAAAAAAAAAAAAAAAEAAgMFERIh/9oACAEDAQE/AZrOR2cOxC1Oelf/xAAbEQACAQUAAAAAAAAAAAAAAAAAAgMGEhYyYv/aAAgBAgEBPwFKUSHeK4w7k//EACIQAAIBBAEEAwAAAAAAAAAAAAECAwAEESITEiExQVFxgf/aAAgBAQAGPwJUXuzHAFb3Lc3yo1FS28p3jODUcyNvGwYfYrlkaWOb3b9GT+HxVxdtpyNkLnwPQr//xAAcEAEAAgMBAQEAAAAAAAAAAAABETEAQWFxUZH/2gAIAQEAAT8hlJMNatGUDIoSeW/pgkBsVPTjeR+mt+kmBDcIG7vYOqeZ3YQapNwAZ//aAAwDAQACAAMAAAAQkA//xAAWEQEBAQAAAAAAAAAAAAAAAAAAETH/2gAIAQMBAT8QhEkaP//EABwRAQACAQUAAAAAAAAAAAAAAAERIQAxQXGRsf/aAAgBAgEBPxCabVaNde7YhZInGf/EABwQAQEBAQEAAwEAAAAAAAAAAAERIQAxQVFhcf/aAAgBAQABPxBH9WNaB+qnUAKKlBpUAfnT6POuWaDh5SMIKGJwf9rgBZP6HY9foLg4RsVTUecC1OHFDMKqoWWHf//Z';

test('progressive JPEG decodes', () => {
  const img = image.decode(Buffer.from(PROGRESSIVE, 'base64'));
  assert.ok(img);
  assert.deepStrictEqual([img.width, img.height], [24, 16]);
  for (const [[x, y], want] of [[[0, 0], [247, 172, 5]], [[12, 8], [251, 255, 255]], [[23, 15], [9, 3, 129]]]) {
    const o = (y * 24 + x) * 4, got = [...img.data.slice(o, o + 3)];
    assert.ok(got.every((v, i) => Math.abs(v - want[i]) <= 24), `(${x}, ${y}): ${got} instead of ${want}`);
  }
});
