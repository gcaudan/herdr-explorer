// Icons of the files and folders: Nerd Font glyphs in the colours of the Seti theme, or a VS Code file icon theme
// (Material Icon Theme…) read where VS Code or a fork of it installed it, each icon drawn in the colour of its SVG
// or font glyph, as a Nerd Font glyph of its own when it has one.
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { C } = require('./ui');
const { newer } = require('./exec');

const FOLDER = [0xf07b, 0xf07c];
const FILE = [0xf15b, C.grey];
// Without prototype: a file named "constructor" is a name like another.
const EXTS = Object.create(null);
for (const [exts, icon] of [
  ['js mjs cjs jsx', [0xe74e, C.yellow]], ['ts tsx mts cts', [0xe628, C.blue]], ['json jsonc', [0xe60b, C.yellow]],
  ['md mdx', [0xe609, C.blue]], ['py', [0xe606, C.blue]], ['html htm', [0xe736, C.orange]],
  ['css scss sass less', [0xe749, C.blue]], ['sh bash zsh fish', [0xe795, C.green]],
  ['ps1 psm1 psd1 bat cmd', [0xebc7, C.blue]], ['png jpg jpeg gif svg webp ico bmp', [0xf1c5, C.purple]],
  ['pdf', [0xf1c1, C.red]], ['zip gz tgz 7z tar rar xz', [0xf1c6, C.orange]], ['xlsx xls csv', [0xf1c3, C.green]],
  ['docx doc odt', [0xf1c2, C.blue]], ['lock', [0xf023, C.grey]],
  ['toml yaml yml ini cfg conf env xml rules service', [0xf013, C.grey]], ['sql db sqlite', [0xf1c0, C.orange]],
  ['rs', [0xe7a8, C.orange]], ['cs csproj sln', [0xf031b, C.purple]], ['java', [0xe738, C.red]],
  ['vue', [0xe6a0, C.green]], ['txt log', [0xf0f6, C.grey]],
  ['c h cpp hpp go rb php kt swift lua qml', [0xf121, C.blue]],
]) for (const ext of exts.split(' ')) EXTS[ext] = icon;
const NAMES = Object.assign(Object.create(null), {
  '.gitignore': [0xe702, C.orange], '.gitattributes': [0xe702, C.orange], '.gitmodules': [0xe702, C.orange],
  'package.json': [0xe718, C.green], 'dockerfile': [0xe7b0, C.blue], 'readme.md': [0xf02d, C.blue],
});

// Extensions installed by the user, then the ones built into the editors (Seti), found from their command.
const EDITORS = ['.vscode', '.vscode-insiders', '.vscode-oss', '.cursor', '.windsurf', '.vscode-server'];
const COMMANDS = ['code', 'code-insiders', 'codium', 'cursor', 'windsurf'];
const APPS = ['Visual Studio Code', 'Visual Studio Code - Insiders', 'VSCodium', 'Cursor', 'Windsurf'];

// Folder icons of Material Icon Theme with a glyph of their own; the other folders are a plain folder in the
// colour of their icon.
const FOLDERS = Object.assign(Object.create(null), {
  'folder-git': 0xe5fb, 'folder-github': 0xe5fd, 'folder-node': 0xe5fa, 'folder-config': 0xf107f,
  'folder-images': 0xf024f, 'folder-keys': 0xf08ac, 'folder-secure': 0xf0250, 'folder-private': 0xf0250,
  'folder-download': 0xf024d, 'folder-upload': 0xf0259, 'folder-home': 0xf10b5, 'folder-audio': 0xf1359,
  'folder-archive': 0xf06eb, 'folder-docs': 0xf0c82, 'folder-tools': 0xf19fc, 'folder-utils': 0xf19fc,
  'folder-admin': 0xf024c, 'folder-shared': 0xf0870,
});
// File icons of Material Icon Theme with a glyph of their own (present since Nerd Fonts 3.0); the other files keep
// the default glyph, in the colour of their icon.
const FILES = Object.assign(Object.create(null), {
  nodejs: 0xe718, npm: 0xe71e, yarn: 0xe6a7, tsconfig: 0xe69d, eslint: 0xe655, prettier: 0xe6b4, webpack: 0xe6a3,
  babel: 0xe639, editorconfig: 0xe652, license: 0xe60a, readme: 0xf405, changelog: 0xf464, docker: 0xe7b0,
  'go-mod': 0xe627, go: 0xe627, 'python-misc': 0xe606, makefile: 0xe673, git: 0xe702, tune: 0xf062e, nginx: 0xe776,
  gemfile: 0xe739, robots: 0xf06a9, favicon: 0xe623, react: 0xe7ba, react_ts: 0xe625, mdx: 0xe609, ruby: 0xe739,
  php: 0xe73d, kotlin: 0xe634, swift: 0xe755, dart: 0xe798, visualstudio: 0xe70c, fsharp: 0xe7a7, c: 0xe61e,
  h: 0xe649, cpp: 0xe61d, hpp: 0xe61d, lua: 0xe620, console: 0xf489, sass: 0xe74b, less: 0xe758, svelte: 0xe697,
  graphql: 0xe662, prisma: 0xe684, yaml: 0xe6a8, toml: 0xe6b2, xml: 0xe619, svg: 0xe698, video: 0xf1c8,
  audio: 0xf1c7, font: 0xf031, exe: 0xeae8, dll: 0xeae8, powerpoint: 0xf1c4, document: 0xf0f6, log: 0xeb9d,
  key: 0xf084, certificate: 0xf0a3, terraform: 0xe69a, nix: 0xf1105, r: 0xe68a, julia: 0xe624, elixir: 0xe62d,
  erlang: 0xe7b1, haskell: 0xe61f, clojure: 0xe768, scala: 0xe737, groovy: 0xe775, gradle: 0xe660, tex: 0xe69b,
  diff: 0xf440, claude: 0xf167a, agent: 0xf06a9,
});

// Language of a file, by extension or by name, for the themes that give icons per language (Seti): the ones VS
// Code knows without extensions.
const LANGUAGES = Object.create(null);
for (const [language, exts] of Object.entries({
  javascript: 'js mjs cjs es6', javascriptreact: 'jsx', typescript: 'ts mts cts', typescriptreact: 'tsx',
  json: 'json', jsonc: 'jsonc', markdown: 'md markdown', python: 'py pyw pyi', html: 'html htm xhtml', css: 'css',
  scss: 'scss', less: 'less', shellscript: 'sh bash zsh ksh', powershell: 'ps1 psm1 psd1', bat: 'bat cmd',
  csharp: 'cs csx', fsharp: 'fs fsi fsx', java: 'java', rust: 'rs', go: 'go', c: 'c', cpp: 'cpp cc cxx hpp hh hxx h',
  php: 'php', ruby: 'rb', lua: 'lua', perl: 'pl pm', r: 'r', sql: 'sql', xml: 'xml xsd xsl', yaml: 'yml yaml',
  ini: 'ini', properties: 'properties conf cfg', swift: 'swift', dart: 'dart', groovy: 'groovy gradle', vb: 'vb',
  coffeescript: 'coffee', clojure: 'clj cljs', 'objective-c': 'm', diff: 'diff patch', log: 'log',
  plaintext: 'txt', razor: 'cshtml', handlebars: 'hbs handlebars', pug: 'pug jade', latex: 'tex', julia: 'jl',
  dockerfile: 'dockerfile', makefile: 'makefile',
})) for (const ext of exts.split(' ')) LANGUAGES[ext] = language;

function extensionDirs() {
  const roots = EDITORS.map(e => path.join(os.homedir(), e, 'extensions'));
  for (const app of APPS) roots.push(`/Applications/${app}.app/Contents/Resources/app/extensions`);
  // The command is in resources/app/bin (macOS, Cursor) or in a bin folder next to resources/app, or to a version
  // folder holding it (VS Code on Windows).
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    if (!path.isAbsolute(dir)) continue;
    for (const command of COMMANDS) {
      let up;
      try { up = path.dirname(path.dirname(fs.realpathSync(path.join(dir, command)))); } catch { continue; }
      roots.push(path.join(up, 'extensions'), path.join(up, 'resources', 'app', 'extensions'));
      try { for (const v of fs.readdirSync(up)) roots.push(path.join(up, v, 'resources', 'app', 'extensions')); } catch { }
    }
  }
  const dirs = [];
  for (const root of new Set(roots)) {
    try { for (const name of fs.readdirSync(root)) dirs.push(path.join(root, name)); } catch { }
  }
  return dirs;
}

// Icon themes installed, by name: [{ id: 'publisher.extension/theme', label, file }], from the newest version of
// each extension.
function themes() {
  const found = new Map();
  for (const dir of extensionDirs()) {
    let pkg;
    try {
      const text = fs.readFileSync(path.join(dir, 'package.json'), 'utf8');
      if (!text.includes('"iconThemes"')) continue;
      pkg = parseJsonc(text);
    } catch { continue; }
    let nls = {};
    try { nls = parseJsonc(fs.readFileSync(path.join(dir, 'package.nls.json'), 'utf8')); } catch { }
    for (const t of (pkg.contributes && pkg.contributes.iconThemes) || []) {
      if (!t || !t.id || !t.path) continue;
      const id = `${pkg.publisher}.${pkg.name}/${t.id}`.toLowerCase();
      // A translated label: a string, or { message, comment }.
      const text = key => (nls[key] && typeof nls[key] === 'object' ? nls[key].message : nls[key]);
      const label = String(t.label || t.id).replace(/^%(.+)%$/, (m, key) => text(key) || m);
      const old = found.get(id);
      if (!old || newer(pkg.version, old.version)) found.set(id, { id, label, version: pkg.version, file: path.join(dir, t.path) });
    }
  }
  return [...found.values()].sort((a, b) => a.label.localeCompare(b.label));
}

// JSON with comments and trailing commas, as VS Code accepts for theme files.
function parseJsonc(text) {
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1); // a BOM, which VS Code accepts too
  try { return JSON.parse(text); } catch { }
  return JSON.parse(text
    .replace(/("(?:\\.|[^"\\])*")|\/\/[^\n]*|\/\*[\s\S]*?\*\//g, (m, str) => str || '')
    .replace(/("(?:\\.|[^"\\])*")|,(\s*[}\]])/g, (m, str, end) => str || end));
}

const rgb = hex => {
  const h = hex.length === 3 ? [...hex].map(c => c + c).join('') : hex;
  return [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16));
};

// { entry, folder(name), file(name) → [glyph or undefined, rgb or undefined] } for a theme of the list ({ id, label,
// file }), or null when there is none or it is no longer installed. Its file is read directly; moved by an update
// of its extension, it is searched again, and entry gives its new place.
function load(entry) {
  if (!entry || !entry.id) return null;
  let found = entry, theme;
  try { theme = parseJsonc(fs.readFileSync(found.file, 'utf8')); } catch {
    found = themes().find(t => t.id === entry.id);
    if (!found) return null;
    try { theme = parseJsonc(fs.readFileSync(found.file, 'utf8')); } catch { return null; }
  }
  // Names matched whatever their case, like VS Code on Windows and macOS.
  const lower = map => {
    const m = new Map();
    for (const [k, v] of Object.entries(map || {})) if (!m.has(k.toLowerCase())) m.set(k.toLowerCase(), v);
    return m;
  };
  const folders = lower(theme.folderNames), names = lower(theme.fileNames), exts = lower(theme.fileExtensions);
  const languages = Object.assign(Object.create(null), theme.languageIds);
  // Colour of an icon: the one of its font glyph, else one of its SVG: the first for a folder (the folder itself),
  // the most saturated for a file (the logo rather than a grey detail). None for the icons drawn in the text colour.
  const colours = new Map();
  const colour = (icon, folder) => {
    if (!colours.has(icon)) {
      const def = (theme.iconDefinitions || {})[icon] || {};
      const font = def.fontColor && /^#([0-9a-f]{6}|[0-9a-f]{3})/i.exec(def.fontColor);
      let fills = font ? [rgb(font[1])] : [];
      if (!font && /\.svg$/i.test(def.iconPath || '')) {
        try {
          const svg = fs.readFileSync(path.join(path.dirname(found.file), def.iconPath), 'utf8');
          fills = [...svg.matchAll(/fill[=:]\s*["']?#([0-9a-f]{6}|[0-9a-f]{3})\b/gi)].map(m => rgb(m[1]));
        } catch { }
      }
      const saturation = c => Math.max(...c) - Math.min(...c);
      colours.set(icon, folder ? fills[0] : fills.reduce((best, c) => (!best || saturation(c) > saturation(best) ? c : best), undefined));
    }
    return colours.get(icon);
  };
  return {
    entry: { id: found.id, label: found.label, file: found.file },
    folder(name) {
      const icon = folders.get(name.toLowerCase()) || theme.folder;
      return [FOLDERS[icon], icon && colour(icon, true)];
    },
    // The file name, else its longest known extension ("d.ts" before "ts"), else its language; unknown files keep
    // their colour.
    file(name) {
      const parts = name.toLowerCase().split('.');
      let icon = names.get(name.toLowerCase());
      for (let i = 1; !icon && i < parts.length; i++) icon = exts.get(parts.slice(i).join('.'));
      if (!icon) icon = languages[LANGUAGES[parts.length > 1 ? parts[parts.length - 1] : parts[0]]];
      return icon ? [FILES[icon], colour(icon)] : [];
    },
  };
}

// [glyph, rgb] of a file or folder, from the theme loaded when it has one, else the default icons.
function icon(theme, name, dir, open) {
  const [code, rgb] = theme ? (dir ? theme.folder(name) : theme.file(name)) : [];
  const own = dir ? [FOLDER[open ? 1 : 0], C.folder] : NAMES[name.toLowerCase()] || EXTS[path.extname(name).slice(1).toLowerCase()] || FILE;
  return [code || own[0], rgb || own[1]];
}

module.exports = { themes, load, icon };
