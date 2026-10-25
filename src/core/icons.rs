//! Icons of the files and folders: Nerd Font glyphs in the colours of the Seti theme, or a VS Code file icon theme
//! (Material Icon Theme…) read where VS Code or a fork of it installed it, each icon drawn in the colour of its SVG
//! or font glyph, as a Nerd Font glyph of its own when it has one. Without a Nerd Font, the icon set chosen in the
//! settings (`ui::g().set`): emoji, or a Unicode or ASCII mark in the colour of the file's type.

use super::ui::{self, Rgb};
use super::{exec, js, paths, which};
use crate::json::{self, Value};
use std::cell::RefCell;
use std::collections::HashMap;

/// A Nerd Font glyph, a colour (a name of the theme's palette), an emoji. The emoji are wide by default (no U+FE0F
/// selector): two columns.
type Own = (u32, &'static str, &'static str);

const FOLDER: [Own; 2] = [(0xf07b, "folder", "📁"), (0xf07c, "folder", "📂")];
const FILE: Own = (0xf15b, "grey", "📄");
const EXTS: &[(&str, Own)] = &[
    ("js mjs cjs jsx", (0xe74e, "yellow", "📜")),
    ("ts tsx mts cts", (0xe628, "blue", "📘")),
    ("json jsonc", (0xe60b, "yellow", "🧾")),
    ("md mdx", (0xe609, "blue", "📝")),
    ("py", (0xe606, "blue", "🐍")),
    ("html htm", (0xe736, "orange", "🌐")),
    ("css scss sass less", (0xe749, "blue", "🎨")),
    ("sh bash zsh fish", (0xe795, "green", "🐚")),
    ("ps1 psm1 psd1 bat cmd", (0xebc7, "blue", "🐚")),
    ("png jpg jpeg gif svg webp ico bmp", (0xf1c5, "purple", "📷")),
    ("pdf", (0xf1c1, "red", "📕")),
    ("zip gz tgz 7z tar rar xz", (0xf1c6, "orange", "📦")),
    ("xlsx xls csv", (0xf1c3, "green", "📊")),
    ("docx doc odt", (0xf1c2, "blue", "📃")),
    ("lock", (0xf023, "grey", "🔒")),
    ("toml yaml yml ini cfg conf env xml rules service", (0xf013, "grey", "🔧")),
    ("sql db sqlite", (0xf1c0, "orange", "💾")),
    ("rs", (0xe7a8, "orange", "🦀")),
    ("cs csproj sln", (0xf031b, "purple", "💜")),
    ("java", (0xe738, "red", "☕")),
    ("vue", (0xe6a0, "green", "💚")),
    ("txt log", (0xf0f6, "grey", "📋")),
    ("c h cpp hpp go rb php kt swift lua qml", (0xf121, "blue", "💻")),
];
const NAMES: &[(&str, Own)] = &[
    (".gitignore", (0xe702, "orange", "🔀")),
    (".gitattributes", (0xe702, "orange", "🔀")),
    (".gitmodules", (0xe702, "orange", "🔀")),
    ("package.json", (0xe718, "green", "📦")),
    ("dockerfile", (0xe7b0, "blue", "🐳")),
    ("readme.md", (0xf02d, "blue", "📖")),
];

/// The marks of a set without glyphs: a folder (closed, open), a file of a known type, another file.
fn marks(set: &str) -> Option<([&'static str; 2], &'static str, &'static str)> {
    match set {
        "unicode" => Some((["■", "□"], "●", "○")),
        "ascii" => Some((["/", "/"], "-", "-")),
        _ => None,
    }
}

/// Extensions installed by the user, then the ones built into the editors (Seti), found from their command.
const EDITORS: &[&str] = &[".vscode", ".vscode-insiders", ".vscode-oss", ".cursor", ".windsurf", ".vscode-server"];
const COMMANDS: &[&str] = &["code", "code-insiders", "codium", "cursor", "windsurf"];
const APPS: &[&str] = &["Visual Studio Code", "Visual Studio Code - Insiders", "VSCodium", "Cursor", "Windsurf"];

/// Folder icons of Material Icon Theme with a glyph of their own; the other folders are a plain folder in the colour
/// of their icon.
const FOLDERS: &[(&str, u32)] = &[
    ("folder-git", 0xe5fb),
    ("folder-github", 0xe5fd),
    ("folder-node", 0xe5fa),
    ("folder-config", 0xf107f),
    ("folder-images", 0xf024f),
    ("folder-keys", 0xf08ac),
    ("folder-secure", 0xf0250),
    ("folder-private", 0xf0250),
    ("folder-download", 0xf024d),
    ("folder-upload", 0xf0259),
    ("folder-home", 0xf10b5),
    ("folder-audio", 0xf1359),
    ("folder-archive", 0xf06eb),
    ("folder-docs", 0xf0c82),
    ("folder-tools", 0xf19fc),
    ("folder-utils", 0xf19fc),
    ("folder-admin", 0xf024c),
    ("folder-shared", 0xf0870),
];
/// File icons of Material Icon Theme with a glyph of their own (present since Nerd Fonts 3.0); the other files keep
/// the default glyph, in the colour of their icon.
const FILES: &[(&str, u32)] = &[
    ("nodejs", 0xe718),
    ("npm", 0xe71e),
    ("yarn", 0xe6a7),
    ("tsconfig", 0xe69d),
    ("eslint", 0xe655),
    ("prettier", 0xe6b4),
    ("webpack", 0xe6a3),
    ("babel", 0xe639),
    ("editorconfig", 0xe652),
    ("license", 0xe60a),
    ("readme", 0xf405),
    ("changelog", 0xf464),
    ("docker", 0xe7b0),
    ("go-mod", 0xe627),
    ("go", 0xe627),
    ("python-misc", 0xe606),
    ("makefile", 0xe673),
    ("git", 0xe702),
    ("tune", 0xf062e),
    ("nginx", 0xe776),
    ("gemfile", 0xe739),
    ("robots", 0xf06a9),
    ("favicon", 0xe623),
    ("react", 0xe7ba),
    ("react_ts", 0xe625),
    ("mdx", 0xe609),
    ("ruby", 0xe739),
    ("php", 0xe73d),
    ("kotlin", 0xe634),
    ("swift", 0xe755),
    ("dart", 0xe798),
    ("visualstudio", 0xe70c),
    ("fsharp", 0xe7a7),
    ("c", 0xe61e),
    ("h", 0xe649),
    ("cpp", 0xe61d),
    ("hpp", 0xe61d),
    ("lua", 0xe620),
    ("console", 0xf489),
    ("sass", 0xe74b),
    ("less", 0xe758),
    ("svelte", 0xe697),
    ("graphql", 0xe662),
    ("prisma", 0xe684),
    ("yaml", 0xe6a8),
    ("toml", 0xe6b2),
    ("xml", 0xe619),
    ("svg", 0xe698),
    ("video", 0xf1c8),
    ("audio", 0xf1c7),
    ("font", 0xf031),
    ("exe", 0xeae8),
    ("dll", 0xeae8),
    ("powerpoint", 0xf1c4),
    ("document", 0xf0f6),
    ("log", 0xeb9d),
    ("key", 0xf084),
    ("certificate", 0xf0a3),
    ("terraform", 0xe69a),
    ("nix", 0xf1105),
    ("r", 0xe68a),
    ("julia", 0xe624),
    ("elixir", 0xe62d),
    ("erlang", 0xe7b1),
    ("haskell", 0xe61f),
    ("clojure", 0xe768),
    ("scala", 0xe737),
    ("groovy", 0xe775),
    ("gradle", 0xe660),
    ("tex", 0xe69b),
    ("diff", 0xf440),
    ("claude", 0xf167a),
    ("agent", 0xf06a9),
];

/// Language of a file, by extension or by name, for the themes that give icons per language (Seti): the ones VS
/// Code knows without extensions.
const LANGUAGES: &[(&str, &str)] = &[
    ("javascript", "js mjs cjs es6"),
    ("javascriptreact", "jsx"),
    ("typescript", "ts mts cts"),
    ("typescriptreact", "tsx"),
    ("json", "json"),
    ("jsonc", "jsonc"),
    ("markdown", "md markdown"),
    ("python", "py pyw pyi"),
    ("html", "html htm xhtml"),
    ("css", "css"),
    ("scss", "scss"),
    ("less", "less"),
    ("shellscript", "sh bash zsh ksh"),
    ("powershell", "ps1 psm1 psd1"),
    ("bat", "bat cmd"),
    ("csharp", "cs csx"),
    ("fsharp", "fs fsi fsx"),
    ("java", "java"),
    ("rust", "rs"),
    ("go", "go"),
    ("c", "c"),
    ("cpp", "cpp cc cxx hpp hh hxx h"),
    ("php", "php"),
    ("ruby", "rb"),
    ("lua", "lua"),
    ("perl", "pl pm"),
    ("r", "r"),
    ("sql", "sql"),
    ("xml", "xml xsd xsl"),
    ("yaml", "yml yaml"),
    ("ini", "ini"),
    ("properties", "properties conf cfg"),
    ("swift", "swift"),
    ("dart", "dart"),
    ("groovy", "groovy gradle"),
    ("vb", "vb"),
    ("coffeescript", "coffee"),
    ("clojure", "clj cljs"),
    ("objective-c", "m"),
    ("diff", "diff patch"),
    ("log", "log"),
    ("plaintext", "txt"),
    ("razor", "cshtml"),
    ("handlebars", "hbs handlebars"),
    ("pug", "pug jade"),
    ("latex", "tex"),
    ("julia", "jl"),
    ("dockerfile", "dockerfile"),
    ("makefile", "makefile"),
];

/// A word of a space-separated list's table, the last that has it.
fn lookup<T: Copy>(table: &[(&str, T)], word: &str, list: bool) -> Option<T> {
    table.iter().rev().find(|(k, _)| if list { k.split(' ').any(|w| w == word) } else { *k == word }).map(|&(_, v)| v)
}

fn language(ext: &str) -> Option<&'static str> {
    LANGUAGES.iter().rev().find(|(_, exts)| exts.split(' ').any(|e| e == ext)).map(|&(l, _)| l)
}

/// `os.homedir()`: HOME, but USERPROFILE on Windows (libuv never reads HOME there).
pub fn homedir() -> String {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default()
}

/// A folder's names in Node's readdir order (sorted by bytes), empty when it cannot be read.
fn read_dir(dir: &str) -> Option<Vec<String>> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    names.sort();
    Some(names)
}

fn extension_dirs() -> Vec<String> {
    let home = homedir();
    let mut roots: Vec<String> = EDITORS.iter().map(|e| paths::join(&[&home, e, "extensions"])).collect();
    for app in APPS {
        roots.push(format!("/Applications/{app}.app/Contents/Resources/app/extensions"));
    }
    // The command is in resources/app/bin (macOS, Cursor) or in a bin folder next to resources/app, or to a version
    // folder holding it (VS Code on Windows).
    for dir in which::path_dirs() {
        for command in COMMANDS {
            let Ok(real) = exec::node_realpath(&paths::join(&[&dir, command])) else { continue };
            let up = paths::dirname(&paths::dirname(&real));
            roots.push(paths::join(&[&up, "extensions"]));
            roots.push(paths::join(&[&up, "resources", "app", "extensions"]));
            for v in read_dir(&up).unwrap_or_default() {
                roots.push(paths::join(&[&up, &v, "resources", "app", "extensions"]));
            }
        }
    }
    let mut seen = Vec::new();
    let mut dirs = Vec::new();
    for root in roots {
        if seen.contains(&root) {
            continue;
        }
        for name in read_dir(&root).unwrap_or_default() {
            dirs.push(paths::join(&[&root, &name]));
        }
        seen.push(root);
    }
    dirs
}

/// An icon theme installed: its id ('publisher.extension/theme'), label, the version of its extension (None when
/// its package.json has none) and its file.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub label: String,
    pub version: Option<Value>,
    pub file: String,
}

impl Theme {
    /// The entry the settings keep: { id, label, file }.
    pub fn entry(&self) -> Value {
        crate::obj! { "id" => self.id.as_str(), "label" => self.label.as_str(), "file" => self.file.as_str() }
    }
}

/// `${x}` of a value that may be missing (undefined).
fn tpl(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".into(), js::string)
}

/// Icon themes installed, by label, from the newest version of each extension.
pub fn themes() -> Vec<Theme> {
    let mut found: Vec<Theme> = Vec::new();
    for dir in extension_dirs() {
        let Ok(text) = exec::read_text(paths::join(&[&dir, "package.json"])) else { continue };
        if !text.contains("\"iconThemes\"") {
            continue;
        }
        let Ok(pkg) = parse_jsonc(&text) else { continue };
        let nls = exec::read_text(paths::join(&[&dir, "package.nls.json"]))
            .ok()
            .and_then(|t| parse_jsonc(&t).ok())
            .unwrap_or(Value::Obj(Vec::new()));
        let list = pkg.get("contributes").and_then(|c| c.get("iconThemes")).and_then(Value::as_array);
        for t in list.map_or(&[][..], Vec::as_slice) {
            let (Some(tid), Some(tpath)) = (t.get("id"), t.get("path")) else { continue };
            if !tid.truthy() || !tpath.truthy() {
                continue;
            }
            // path.join throws on anything but a string.
            let Some(tpath) = tpath.as_str() else { continue };
            let id = js::lower(&format!("{}.{}/{}", tpl(pkg.get("publisher")), tpl(pkg.get("name")), js::string(tid)));
            // A translated label: a string, or { message, comment }.
            let text = |key: &str| {
                let v = nls.get(key)?;
                if matches!(v, Value::Obj(_) | Value::Arr(_)) { v.get("message").cloned() } else { Some(v.clone()) }
            };
            let raw = js::string(t.get("label").filter(|l| l.truthy()).unwrap_or(tid));
            let label = match raw.strip_prefix('%').and_then(|r| r.strip_suffix('%')) {
                Some(key) if !key.is_empty() && !key.chars().any(js::is_line_terminator) => {
                    text(key).filter(Value::truthy).map_or(raw.clone(), |v| js::string(&v))
                }
                _ => raw.clone(),
            };
            let version = pkg.get("version").cloned();
            let theme = Theme { id: id.clone(), label, version: version.clone(), file: paths::join(&[&dir, tpath]) };
            match found.iter_mut().find(|f| f.id == id) {
                None => found.push(theme),
                Some(old) if exec::newer(&tpl(version.as_ref()), &tpl(old.version.as_ref())) => *old = theme,
                Some(_) => {}
            }
        }
    }
    found.sort_by(|a, b| js::locale_compare(&a.label, &b.label, false));
    found
}

/// JSON with comments and trailing commas, as VS Code accepts for theme files; a byte order mark taken off.
pub fn parse_jsonc(text: &str) -> Result<Value, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Ok(v) = json::parse(text) {
        return Ok(v);
    }
    json::parse(&trailing_commas(&comments(text)))
}

/// The length of the string literal at the start of s ("(?:\\.|[^"\\])*"), if it ends.
fn string_at(s: &str) -> Option<usize> {
    let mut it = s.char_indices().skip(1);
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Some(i + 1),
            '\\' => match it.next() {
                Some((_, n)) if !js::is_line_terminator(n) => {}
                _ => return None,
            },
            _ => {}
        }
    }
    None
}

/// The comments taken off (// to the end of the line, /* */), strings kept.
fn comments(text: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with('"')
            && let Some(n) = string_at(rest)
        {
            out += &rest[..n];
            i += n;
        } else if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("/*")
            && let Some(n) = rest[2..].find("*/")
        {
            i += n + 4;
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// A comma before } or ] (spaces between) taken off, strings kept.
fn trailing_commas(text: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with('"')
            && let Some(n) = string_at(rest)
        {
            out += &rest[..n];
            i += n;
            continue;
        }
        if let Some(after) = rest.strip_prefix(',') {
            let end = after.trim_start_matches(js::is_space);
            if end.starts_with('}') || end.starts_with(']') {
                i += 1;
                continue;
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// A hex colour of 6 or 3 digits.
fn rgb(hex: &str) -> Rgb {
    let h: String = if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect() } else { hex.into() };
    let at = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    [at(0), at(2), at(4)]
}

/// The hex digits of a colour at the start of s: 6, else 3; with end, only followed by no word character.
fn hex_at(s: &str, end: bool) -> Option<&str> {
    let n = s.bytes().take_while(u8::is_ascii_hexdigit).count();
    let ends = |k: usize| !end || !s[k..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
    [6, 3].into_iter().find(|&k| n >= k && ends(k)).map(|k| &s[..k])
}

/// The colours of an SVG's fills (fill="#…", fill: #…).
fn fills(svg: &str) -> Vec<Rgb> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < svg.len() {
        let rest = &svg[i..];
        let hit = rest.get(..4).filter(|p| p.eq_ignore_ascii_case("fill")).and_then(|_| {
            let r = rest[4..].strip_prefix(['=', ':'])?;
            let r = r.trim_start_matches(js::is_space);
            let r = r.strip_prefix(['"', '\'']).unwrap_or(r);
            let r = r.strip_prefix('#')?;
            let hex = hex_at(r, true)?;
            Some((hex, rest.len() - r.len() + hex.len()))
        });
        match hit {
            Some((hex, n)) => {
                out.push(rgb(hex));
                i += n;
            }
            None => i += rest.chars().next().unwrap().len_utf8(),
        }
    }
    out
}

/// A theme loaded: the icon of a folder or a file by name.
pub struct Loaded {
    /// Where the theme was found, { id, label, file }: the entry given, or the one searched again when it moved.
    pub entry: Value,
    /// Its file, whose folder holds the SVG of its icons.
    file: String,
    theme: Value,
    folders: HashMap<String, Value>,
    names: HashMap<String, Value>,
    exts: HashMap<String, Value>,
    colours: RefCell<HashMap<String, Option<Rgb>>>,
}

/// A map's keys in lower case, the first of each kept: names matched whatever their case, like VS Code on Windows
/// and macOS.
fn lowered(map: Option<&Value>) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    if let Some(Value::Obj(fields)) = map {
        for (k, v) in fields {
            m.entry(js::lower(k)).or_insert_with(|| v.clone());
        }
    }
    m
}

/// The theme of an entry of the settings ({ id, label, file }), or None when there is none or it is no longer
/// installed. Its file is read directly; moved by an update of its extension, it is searched again, and entry gives
/// its new place.
pub fn load(entry: &Value) -> Option<Loaded> {
    let id = entry.get("id").filter(|v| v.truthy())?;
    let read = |file: &str| exec::read_text(file).ok().and_then(|t| parse_jsonc(&t).ok());
    let file = entry.get("file").and_then(Value::as_str);
    let (found, file, theme) = match file.and_then(read) {
        Some(theme) => {
            let keep = |k: &str| entry.get(k).map(|v| (k.to_string(), v.clone()));
            let found = Value::Obj(["id", "label", "file"].into_iter().filter_map(keep).collect());
            (found, file.unwrap_or("").to_string(), theme)
        }
        None => {
            let found = themes().into_iter().find(|t| Some(t.id.as_str()) == id.as_str())?;
            let theme = read(&found.file)?;
            (found.entry(), found.file, theme)
        }
    };
    Some(Loaded {
        file,
        folders: lowered(theme.get("folderNames")),
        names: lowered(theme.get("fileNames")),
        exts: lowered(theme.get("fileExtensions")),
        entry: found,
        theme,
        colours: RefCell::new(HashMap::new()),
    })
}

impl Loaded {
    /// The colour of an icon: the one of its font glyph, else one of its SVG: the first for a folder (the folder
    /// itself), the most saturated for a file (the logo rather than a grey detail). None for the icons drawn in the
    /// text colour.
    fn colour(&self, icon: &Value, folder: bool) -> Option<Rgb> {
        let key = js::string(icon);
        if let Some(c) = self.colours.borrow().get(&key) {
            return *c;
        }
        let def = self.theme.get("iconDefinitions").and_then(|d| d.get(&key));
        let font = def
            .and_then(|d| d.get("fontColor"))
            .filter(|c| c.truthy())
            .and_then(|c| js::string(c).strip_prefix('#').and_then(|h| hex_at(h, false).map(str::to_string)));
        let mut all = font.as_deref().map(rgb).into_iter().collect::<Vec<_>>();
        let icon_path = def.and_then(|d| d.get("iconPath")).filter(|p| p.truthy());
        if font.is_none()
            && let Some(p) = icon_path
            && js::lower(&js::string(p)).ends_with(".svg")
            && let Some(p) = p.as_str()
        {
            let file = paths::join(&[&paths::dirname(&self.file), p]);
            all = exec::read_text(file).map(|svg| fills(&svg)).unwrap_or_default();
        }
        let saturation = |c: &Rgb| c.iter().max().unwrap() - c.iter().min().unwrap();
        let c = if folder {
            all.first().copied()
        } else {
            all.into_iter().fold(None, |best: Option<Rgb>, c| match best {
                Some(b) if saturation(&c) <= saturation(&b) => Some(b),
                _ => Some(c),
            })
        };
        self.colours.borrow_mut().insert(key, c);
        c
    }

    /// A folder's glyph (when its icon has one of its own) and colour.
    pub fn folder(&self, name: &str) -> (Option<u32>, Option<Rgb>) {
        let icon = self.folders.get(&js::lower(name)).filter(|v| v.truthy()).or_else(|| self.theme.get("folder"));
        let Some(icon) = icon.filter(|v| v.truthy()) else { return (None, None) };
        (lookup(FOLDERS, &js::string(icon), false), self.colour(icon, true))
    }

    /// A file's glyph and colour, by its name, else its longest known extension ("d.ts" before "ts"), else its
    /// language; None for an unknown file, which keeps its colour.
    pub fn file(&self, name: &str) -> Option<(Option<u32>, Option<Rgb>)> {
        let lower = js::lower(name);
        let parts: Vec<&str> = lower.split('.').collect();
        let mut icon = self.names.get(&lower).filter(|v| v.truthy()).cloned();
        for i in 1..parts.len() {
            if icon.is_some() {
                break;
            }
            icon = self.exts.get(&parts[i..].join(".")).filter(|v| v.truthy()).cloned();
        }
        if icon.is_none() {
            let ext = if parts.len() > 1 { parts[parts.len() - 1] } else { parts[0] };
            let key = language(ext).unwrap_or("undefined");
            icon = self.theme.get("languageIds").and_then(|l| l.get(key)).filter(|v| v.truthy()).cloned();
        }
        let icon = icon?;
        Some((lookup(FILES, &js::string(&icon), false), self.colour(&icon, false)))
    }
}

/// The text (its icon and a space) and colour of a file or folder, from the theme loaded when it has one, else the
/// default icons, in the icon set chosen: two columns, three with an emoji. A Unicode or ASCII mark keeps the colour
/// of the theme.
pub fn icon(theme: Option<&Loaded>, name: &str, dir: bool, open: bool) -> (String, Rgb) {
    let set = ui::g().set;
    let (code, colour) = match theme {
        Some(t) if set != "emoji" => {
            if dir {
                t.folder(name)
            } else {
                t.file(name).unwrap_or((None, None))
            }
        }
        _ => (None, None),
    };
    let known = if dir {
        None
    } else {
        lookup(NAMES, &js::lower(name), false).or_else(|| {
            let ext = paths::extname(name);
            lookup(EXTS, &js::lower(ext.get(1..).unwrap_or("")), true)
        })
    };
    let own = if dir { FOLDER[open as usize] } else { known.unwrap_or(FILE) };
    // Every colour of the tables is one of the palette (the JS would give undefined).
    let palette = |n: &str| ui::c().get(n).unwrap_or_else(|| panic!("no colour {n} in the palette"));
    if set == "emoji" {
        return (format!("{} ", own.2), palette(own.1));
    }
    let mark = match marks(set) {
        None => ui::glyph(code.unwrap_or(own.0)),
        Some((folder, _, _)) if dir => folder[open as usize].into(),
        Some((_, k, _)) if known.is_some() || code.is_some() => k.into(),
        Some((_, _, other)) => other.into(),
    };
    (format!("{mark} "), colour.unwrap_or_else(|| palette(own.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas() {
        let v = parse_jsonc("\u{feff}{\n// a\n\"a\": \"//x\", /* b */ \"b\": [1, 2,],\n}").unwrap();
        assert_eq!(json::stringify(&v), r#"{"a":"//x","b":[1,2]}"#);
        assert_eq!(json::stringify(&parse_jsonc(r#"{"s": "a\"//b", }"#).unwrap()), r#"{"s":"a\"//b"}"#);
        assert!(parse_jsonc("{ /* open").is_err());
    }

    #[test]
    fn svg_fills() {
        let svg =
            r##"<path fill="#ff0000"/><path FILL='#0f0'/><g style="fill: #00f"/><x fill="#abcdefg"/><y fill=#123>"##;
        assert_eq!(fills(svg), [[255, 0, 0], [0, 255, 0], [0, 0, 255], [17, 34, 51]]);
    }
}
