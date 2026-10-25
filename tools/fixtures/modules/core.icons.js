'use strict';
// Icons of the files and folders: the default ones in each icon set and theme, the icon themes installed under a
// home ($ROOT/home) and next to an editor's command on PATH ($ROOT/bin), a theme loaded (its names, extensions,
// languages, font and SVG colours), its JSON with comments and trailing commas.
const ui = require('../../../src/core/ui');
const icons = require('../../../src/core/icons');

const ITEMS = [['a.js', false, false], ['b.TS', false, false], ['README.md', false, false], ['Dockerfile', false, false],
  ['.gitignore', false, false], ['x.unknown', false, false], ['noext', false, false], ['constructor', false, false],
  ['src', true, false], ['src', true, true], ['x.d.ts', false, false], ['Makefile', false, false], ['a.svg', false, false],
  ['.git', true, false], ['Node_Modules', true, true], ['docs', true, false], ['icon.png', false, false]];
const inSet = (set, theme, fn) => {
  ui.useGlyphs(set); ui.useTheme(theme);
  try { return fn(); } finally { ui.useGlyphs('nerd'); ui.useTheme('dark'); }
};
const impl = {
  // [text, rgb] of each item, in an icon set and a theme, with the icon theme of entry loaded (or none).
  iconsOf: (set, theme, entry) => inSet(set, theme, () => {
    const loaded = entry && icons.load(entry);
    return ITEMS.map(([name, dir, open]) => icons.icon(loaded, name, dir, open));
  }),
  themesIn: () => icons.themes(),
  // The theme of entry: where it was found, then each folder's and file's [glyph, rgb] ([] for an unknown file).
  loadOf: (entry, folders, files) => {
    const t = icons.load(entry);
    return t && { entry: t.entry, folders: folders.map(n => t.folder(n)), files: files.map(n => t.file(n)) };
  },
};

const pkg = (name, version, themes, more = {}) => JSON.stringify({ publisher: 'Pub', name, version, contributes: { iconThemes: themes }, ...more });
const theme = JSON.stringify({
  iconDefinitions: {
    _folder: { iconPath: './icons/folder.svg' }, 'folder-git': { iconPath: './icons/git.svg' },
    'folder-node': { fontColor: '#8bc34a' }, _file: { iconPath: './icons/file.svg' }, react_ts: { fontColor: '#0288D1ff' },
    typescript: { iconPath: './icons/ts.SVG' }, makefile: { fontColor: '#abc' }, javascript: { iconPath: './icons/js.svg' },
    svg: { iconPath: './icons/missing.svg' }, image: { iconPath: './icons/image.png' }, tune: { fontColor: 'red' },
  },
  folder: '_folder', file: '_file',
  folderNames: { '.git': 'folder-git', '.GIT': 'folder-github', node_modules: 'folder-node', empty: '' },
  fileNames: { 'readme.md': 'readme', Makefile: 'makefile', '.env': 'tune' },
  fileExtensions: { 'd.ts': 'react_ts', ts: 'typescript', png: 'image', svg: 'svg' },
  languageIds: { javascript: 'javascript', undefined: 'nolang' },
});
// Comments and trailing commas, as VS Code writes theme files.
const jsonc = `﻿{
  // the folders
  "folder": "_folder", /* default */
  "iconDefinitions": { "_folder": { "fontColor": "#123456", }, },
  "fileExtensions": { "md": "_folder", },
}`;
const files = {
  'home/.vscode/extensions/pub.icons-1.2.0/package.json': pkg('icons', '1.2.0', [{ id: 'material', label: '%theme.label%', path: './dist/theme.json' }, { id: 'bad' }, null]),
  'home/.vscode/extensions/pub.icons-1.2.0/package.nls.json': JSON.stringify({ 'theme.label': { message: 'Material Icons', comment: 'c' } }),
  'home/.vscode/extensions/pub.icons-1.2.0/dist/theme.json': theme,
  'home/.vscode/extensions/pub.icons-1.2.0/dist/icons/folder.svg': '<svg><path fill="#90A4AE"/><path fill="#ff0000"/></svg>',
  'home/.vscode/extensions/pub.icons-1.2.0/dist/icons/git.svg': '<svg style="fill: #E64A19"/>',
  'home/.vscode/extensions/pub.icons-1.2.0/dist/icons/file.svg': '<svg><path fill="#999"/><path fill=\'#4caf50\'/><path FILL="#2196f3"/></svg>',
  'home/.vscode/extensions/pub.icons-1.2.0/dist/icons/ts.SVG': '<svg><path fill="#0288d1"/></svg>',
  'home/.vscode/extensions/pub.icons-1.2.0/dist/icons/js.svg': '<svg><path fill="#ffca28"/><path fill="#ffca28x"/></svg>',
  'home/.vscode/extensions/pub.icons-1.1.0/package.json': pkg('icons', '1.1.0', [{ id: 'material', label: 'Old', path: './old.json' }]),
  'home/.vscode/extensions/pub.icons-1.10.0/package.json': pkg('Icons', '1.10.0', [{ id: 'MATERIAL', label: 'Newest', path: './dist/theme.json' }]),
  'home/.vscode/extensions/pub.icons-1.10.0/dist/theme.json': jsonc,
  'home/.vscode/extensions/other/package.json': '{ "iconThemes": 1, // not a theme list\n "name": "x" }',
  'home/.vscode/extensions/noversion/package.json': JSON.stringify({ name: 'nov', contributes: { iconThemes: [{ id: 't', label: 5, path: 'p.json' }] } }),
  'home/.vscode/extensions/plain/package.json': JSON.stringify({ name: 'plain' }),
  'home/.cursor/extensions/b.seti/package.json': pkg('seti', '0.1.0', [{ id: 'seti', label: '%missing%', path: 'seti.json' }, { id: 'z', label: 'a', path: 'z.json' }]),
  'home/.cursor/extensions/b.seti/package.nls.json': '{ "other": "x", }',
  'home/.cursor/extensions/b.seti/seti.json': JSON.stringify({ iconDefinitions: { _seti: { fontColor: '#519aba' } }, file: '_seti', languageIds: { markdown: '_seti' } }),
  'app/bin/code': '#!/bin/sh\n',
  'app/resources/app/extensions/theme-builtin/package.json': pkg('builtin', '1.0.0', [{ id: 'vs-minimal', label: 'Minimal', path: 'm.json' }]),
  'app/1.90/resources/app/extensions/ver/package.json': pkg('ver', '2.0.0', [{ id: 'v', label: 'Versioned', path: 'v.json' }]),
  bin: null,
  'bin/code': { link: '$ROOT/app/bin/code' },
};
const env = { HOME: '$ROOT/home', PATH: '$ROOT/bin' };
const material = { id: 'pub.icons/material', label: 'Material Icons', file: '$ROOT/home/.vscode/extensions/pub.icons-1.2.0/dist/theme.json' };
const FOLDERS = ['.git', '.Git', 'node_modules', 'empty', 'src', 'SRC'];
const FILES = ['README.md', 'makefile', 'x.d.ts', 'a.TS', 'a.js', 'b.mjs', 'a.svg', 'p.png', 'x.unknown', 'noext', '.env', 'javascript', 'a.b.c'];

module.exports = {
  module: 'core/icons',
  impl,
  cases: [
    ...['nerd', 'emoji', 'unicode', 'ascii'].flatMap(set => ['dark', 'light'].map(t => ({ fn: 'iconsOf', args: [set, t, null] }))),
    ...['nerd', 'emoji', 'unicode', 'ascii'].map(set => ({ fn: 'iconsOf', args: [set, 'dark', material], files, env })),
    { fn: 'iconsOf', args: ['nerd', 'light', material], files, env },
    { fn: 'themesIn', args: [], files, env },
    { fn: 'themesIn', args: [], env },
    { fn: 'loadOf', args: [material, FOLDERS, FILES], files, env },
    // Moved by an update: searched again by id, found at its new place.
    { fn: 'loadOf', args: [{ ...material, file: '$ROOT/gone.json' }, FOLDERS, FILES], files, env },
    { fn: 'loadOf', args: [{ id: 'pub.icons/material' }, FOLDERS, FILES], files, env },
    { fn: 'loadOf', args: [{ id: 'b.seti/seti', file: 7 }, FOLDERS, FILES], files, env },
    { fn: 'loadOf', args: [{ id: 'nope/none', file: '$ROOT/gone.json' }, FOLDERS, FILES], files, env },
    { fn: 'loadOf', args: [{ file: material.file }, FOLDERS, FILES], files, env },
    { fn: 'loadOf', args: [null, FOLDERS, FILES] },
    { fn: 'loadOf', args: [{ id: 'x', label: null, file: '$ROOT/t.json', extra: 1 }, FOLDERS, FILES], files: { 't.json': jsonc } },
    { fn: 'loadOf', args: [{ id: 'x', file: '$ROOT/t.json' }, ['md'], ['a.md']], files: { 't.json': '{"a": "//not a comment", "b": "/* nor this */", "folder": "f", "iconDefinitions": {"f": {"fontColor": "#f00"},},}' } },
    { fn: 'loadOf', args: [{ id: 'x', file: '$ROOT/t.json' }, ['md'], ['a.md']], files: { 't.json': '{"folder": "x\\"// y", // z\n"iconDefinitions": {"x\\"// y": {"fontColor": "#0f0"}}}' } },
    { fn: 'loadOf', args: [{ id: 'x', file: '$ROOT/t.json' }, ['md'], ['a.md']], files: { 't.json': '{ /* unended' } },
    { fn: 'loadOf', args: [{ id: 'x', file: '$ROOT/t.json' }, ['md'], ['a.md']], files: { 't.json': '[1, , ]' } },
    { fn: 'loadOf', args: [{ id: 'x', file: '$ROOT/t.json' }, ['md'], ['a.md']], files: { 't.json': '{"folder": "f" , \n "iconDefinitions": {"f": {"fontColor": "#00f" }\t,\n} }' } },
    { fn: 'loadOf', args: [{ id: 'x', file: '$ROOT/t.json' }, ['md'], ['a.md']], files: { 't.json': '"str"' } },
  ],
};
