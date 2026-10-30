//! The explorer's settings view, in place of the tree, and the pages opened from it: the icon themes and the key
//! bindings. Each page has its view, its keys and its mouse.

use super::render::chevron;
use super::{Act, Explorer, HELP, later};
use crate::core::actions;
use crate::core::exec::{self, KeyMap};
use crate::core::icons::{self, Loaded};
use crate::core::ui::{self, Field, Hints, Mouse, Rgb, Seg, c, fg, g, line, seg, text_width};
use crate::json::{self, Value};

/// A page's list: its selected row, its first row shown, and its find (/).
#[derive(Default)]
pub(super) struct Page {
    sel: i64,
    top: i64,
    find: Option<Field>,
}

/// The settings view, and the page opened from it.
pub(super) struct View {
    page: Page,
    icons: Option<Icons>,
    keys: Option<Keys>,
}

/// The icon themes: the default icons and the VS Code themes found, each loaded once (None: not found).
pub(super) struct Icons {
    page: Page,
    list: Vec<Value>,
    loaded: Vec<(String, Option<Loaded>)>,
}

pub(super) struct Keys {
    page: Page,
    /// Waiting for the new key of the selected action.
    capture: bool,
}

/// What a setting's change does.
#[derive(Clone)]
enum Change {
    Toggle(&'static str),
    /// A setting among values, the index of its value now.
    Pick(&'static str, &'static [&'static str], usize),
    DiffLayout(Option<usize>),
    IconThemes,
    Editor,
    ScmCommand,
    Prefix,
    Keys,
}

/// A row of a page: a group title, or a label with its value ([text, style]), its help, and what it does.
#[derive(Clone)]
struct Row {
    group: Option<String>,
    label: String,
    value: Seg,
    help: Option<String>,
    action: Option<String>,
    change: Option<Change>,
}

impl Row {
    fn group(title: &str) -> Row {
        Row {
            group: Some(title.into()),
            label: String::new(),
            value: seg("", ""),
            help: None,
            action: None,
            change: None,
        }
    }

    fn item(label: &str, value: Seg) -> Row {
        Row { group: None, label: label.into(), value, help: None, action: None, change: None }
    }

    fn setting(label: &str, help: &str, (value, change): (Seg, Change)) -> Row {
        Row { help: Some(help.into()), change: Some(change), ..Row::item(label, value) }
    }
}

/// The rows a find keeps: those whose label, value or help has the text typed (any case), under their group title.
fn found(list: &[Row], find: &Option<Field>) -> Vec<Row> {
    let Some(q) = find.as_ref().filter(|f| !f.value.is_empty()).map(|f| f.value.to_lowercase()) else {
        return list.to_vec();
    };
    let mut out = Vec::new();
    let mut group = None;
    for r in list {
        if r.group.is_some() {
            group = Some(r);
            continue;
        }
        let text = format!(
            "{} {} {} {}",
            r.label,
            r.value.0,
            r.help.as_deref().unwrap_or(""),
            r.action.as_deref().unwrap_or("")
        );
        if !text.to_lowercase().contains(&q) {
            continue;
        }
        if let Some(g) = group.take() {
            out.push(g.clone());
        }
        out.push(r.clone());
    }
    out
}

fn at(list: &[Row], i: i64) -> Option<&Row> {
    usize::try_from(i).ok().and_then(|i| list.get(i))
}

fn first_row(list: &[Row]) -> i64 {
    list.iter().position(|r| r.group.is_none()).map_or(0, |i| i as i64)
}

fn same_row(a: Option<&Row>, b: Option<&Row>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.group.is_none() && b.group.is_none() && a.label == b.label && a.action == b.action,
        _ => false,
    }
}

/// The find of a page (/ or the key bound to filter): typed characters go to it and the list keeps the rows that
/// match; the arrows, Enter and the other keys that are not characters go to the page. Escape closes it, the selection
/// staying on its row. all: the page's rows. Returns true when the key was the find's.
fn find_key(p: &mut Page, k: &str, all: &[Row], keys: &KeyMap) -> bool {
    let Some(field) = &mut p.find else {
        if k != "/" && keys.get(k) != Some("filter") {
            return false;
        }
        p.find = Some(Field::new(""));
        return true;
    };
    let shown = found(all, &Some(field.clone()));
    let r = at(&shown, p.sel).cloned();
    if k == "escape" {
        p.find = None;
        p.sel = all.iter().position(|x| same_row(Some(x), r.as_ref())).map_or_else(|| first_row(all), |i| i as i64);
        return true;
    }
    if !ui::printable(k) && k != "space" && k != "backspace" && k != "delete" {
        return false;
    }
    if field.key(k) {
        let list = found(all, &p.find);
        p.sel = list.iter().position(|x| same_row(Some(x), r.as_ref())).map_or_else(|| first_row(&list), |i| i as i64);
        p.top = 0;
    }
    true
}

/// The find's line: the field, and the count of rows kept.
fn find_line(p: &Page, list: &[Row], w: usize) -> String {
    let Some(find) = &p.find else { return line(&[], w, None) };
    let count = if find.value.is_empty() {
        String::new()
    } else {
        format!(" {}", list.iter().filter(|r| r.group.is_none()).count())
    };
    let field = find.segments((w as i64 - 4 - count.len() as i64).max(1), "");
    let pad = (w as i64 - 3 - ui::seg_width(&field) as i64).max(0) as usize;
    let mut segs = vec![seg(format!(" {} ", g().search), fg(c().dim))];
    segs.extend(field);
    segs.push(seg(format!("{count:>pad$}"), fg(c().dim)));
    line(&segs, w, Some(c().header))
}

/// The rows of a list from p.top, n at most, the selection kept in view with the title of its group: a group title,
/// or a label with a value on the right.
fn list_lines(list: &[Row], p: &mut Page, n: i64, w: usize, focused: bool) -> Vec<String> {
    let w64 = w as i64;
    let first = if at(list, p.sel - 1).is_some_and(|r| r.group.is_some()) { p.sel - 1 } else { p.sel };
    p.top = p.top.min(first).min(list.len() as i64 - n).max(0);
    if p.sel >= p.top + n {
        p.top = p.sel - n + 1;
    }
    let mut lines: Vec<String> = list
        .iter()
        .enumerate()
        .skip(p.top as usize)
        .take(n as usize)
        .map(|(i, r)| {
            if let Some(group) = &r.group {
                return line(&[seg(format!(" {group}"), format!("\x1b[1m{}", fg(c().dim)))], w, None);
            }
            // The value keeps at least half the room, the label is cut first.
            let room = (w64 - 4 - (text_width(&r.label) as i64).min((w64 - 4).div_euclid(2))).max(1);
            let value = ui::ellipsis(&r.value.0, room);
            let label = ui::ellipsis(&r.label, (w64 - 4 - text_width(&value) as i64).max(1));
            let pad = " ".repeat((w64 - 3 - text_width(&label) as i64 - text_width(&value) as i64).max(1) as usize);
            let back = (i as i64 == p.sel).then(|| if focused { c().select } else { c().select_blur });
            line(&[seg(format!("  {label}{pad}"), ""), seg(value, r.value.1.clone())], w, back)
        })
        .collect();
    if list.is_empty() {
        lines.extend(ui::empty(w, n as usize, "No match", &[("esc", "clear")]));
    }
    while (lines.len() as i64) < n {
        lines.push(line(&[], w, None));
    }
    lines
}

/// The rule under a list, with ▴ ▾ when it scrolls.
fn rule(list: &[Row], p: &Page, n: i64, w: usize) -> String {
    let more = format!("{}{}", if p.top > 0 { "▴" } else { "" }, if p.top + n < list.len() as i64 { "▾" } else { "" });
    let dashes = (w as i64 - more.chars().count() as i64 - 1).max(0) as usize;
    line(&[seg(format!("{}{more}", "─".repeat(dashes)), fg(c().dim))], w, None)
}

/// The selection moved by dir rows, past the group titles, within the list.
fn move_in(list: &[Row], p: &mut Page, dir: i64) {
    let step = if dir < 0 { -1 } else { 1 };
    let ok = |i: i64| at(list, i).is_some_and(|r| r.group.is_none());
    let mut i = p.sel.saturating_add(dir).min(list.len() as i64 - 1).max(0);
    if !ok(i) {
        i = if ok(i + step) { i + step } else { i - step };
    }
    p.sel = i;
}

/// The keys that move in a page whatever the bindings.
fn arrow(k: &str) -> Option<&'static str> {
    Some(match k {
        "up" => "up",
        "down" => "down",
        "pageup" => "pageUp",
        "pagedown" => "pageDown",
        "home" => "top",
        "end" => "bottom",
        _ => return None,
    })
}

/// The move of a bound key, page by page of n rows.
fn moves(n: i64, to: &str) -> Option<i64> {
    let page = n - 1;
    let half = (page + 1).div_euclid(2);
    Some(match to {
        "up" => -1,
        "down" => 1,
        "pageUp" => -page,
        "pageDown" => page,
        "halfPageUp" => -half,
        "halfPageDown" => half,
        "top" => i64::MIN,
        "bottom" => i64::MAX,
        _ => return None,
    })
}

const NERD_FONT_LABEL: &str = "Nerd Font (default)";
const SAMPLE: [(usize, &str, bool, bool); 12] = [
    (0, "src", true, true),
    (1, "components", true, false),
    (1, "App.tsx", false, false),
    (1, "main.py", false, false),
    (1, "styles.css", false, false),
    (0, ".github", true, false),
    (0, "docs", true, false),
    (0, ".gitignore", false, false),
    (0, "Dockerfile", false, false),
    (0, "logo.png", false, false),
    (0, "package.json", false, false),
    (0, "README.md", false, false),
];
/// The lines of the longest help at this width, at most.
const MAX_DETAIL: i64 = 4;

fn settings_keys(w: usize) -> Hints {
    ui::keys(&[("←→", "change"), ("/", "find"), ("esc", "back")], w)
}

fn icons_keys(w: usize) -> Hints {
    ui::keys(&[("↑↓", "move"), ("⏎", "choose"), ("esc", "back")], w)
}

fn keys_keys(w: usize, capture: bool) -> Hints {
    if capture {
        ui::keys(&[("esc", "cancel")], w)
    } else {
        ui::keys(&[("⏎", "add"), ("del", "remove"), ("/", "find")], w)
    }
}

fn same_keys(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|k| b.contains(k))
}

fn key_text(list: &[String]) -> String {
    let text = list.iter().map(|k| ui::key_label(k)).collect::<Vec<_>>().join(" ");
    if text.is_empty() { "—".into() } else { text }
}

/// The text of a setting, as `settings.x || fallback` shows it.
fn or(v: Option<&Value>, fallback: &str) -> String {
    v.filter(|v| v.truthy()).map_or_else(|| fallback.to_string(), crate::json::text)
}

/// Bindings changed: each action given its keys, or its default ones (None), in the order first changed.
type Changes = Vec<(String, Option<Vec<String>>)>;

fn put(changes: &mut Changes, action: &str, keys: Option<Vec<String>>) {
    match changes.iter_mut().find(|(a, _)| a == action) {
        Some((_, v)) => *v = keys,
        None => changes.push((action.to_string(), keys)),
    }
}

impl Explorer {
    /// The settings view in place of the tree.
    pub(super) fn open_settings(&mut self) {
        self.state.menu = None;
        self.state.settings = Some(View { page: Page { sel: 1, top: 0, find: None }, icons: None, keys: None });
        self.render();
    }

    /// The page shown, in place of the tree.
    pub(super) fn settings_view(&mut self, w: usize, h: usize) -> Vec<String> {
        let Some(view) = &self.state.settings else { return Vec::new() };
        if view.icons.is_some() {
            self.icons_view(w, h)
        } else if view.keys.is_some() {
            self.keys_view(w, h)
        } else {
            self.main_view(w, h)
        }
    }

    pub(super) fn settings_key(&mut self, k: &str) {
        let Some(view) = &self.state.settings else { return };
        if view.icons.is_some() {
            self.icons_key(k)
        } else if view.keys.is_some() {
            self.keys_key(k)
        } else {
            self.main_key(k)
        }
    }

    pub(super) fn settings_mouse(&mut self, m: &Mouse) {
        let Some(view) = &self.state.settings else { return };
        if view.icons.is_some() {
            self.icons_mouse(m)
        } else if view.keys.is_some() {
            self.keys_mouse(m)
        } else {
            self.main_mouse(m)
        }
    }

    fn page_title(&self, title: &str, w: usize) -> String {
        let segs = match &self.state.flash {
            Some(text) => ui::message(text, None),
            None => vec![seg(" ‹ ", fg(c().dim)), seg(title, "\x1b[1m")],
        };
        line(&segs, w, None)
    }

    // --- the settings

    fn on_off(&self, name: &'static str) -> (Seg, Change) {
        (ui::on_off(self.settings.get(name).is_some_and(Value::truthy)), Change::Toggle(name))
    }

    fn choice(&self, name: &'static str, values: &'static [&'static str], shown: fn(&str) -> &str) -> (Seg, Change) {
        let now = values.iter().position(|v| self.settings.get(name).and_then(Value::as_str) == Some(v)).unwrap_or(0);
        (seg(shown(values[now]), fg(c().blue)), Change::Pick(name, values, now))
    }

    /// diffLayout: one value, or { narrow, wide } once v chose a layout in a diff (diff::save_mode), read again as
    /// other panes write it. Both shown when they differ; a choice here holds for both.
    fn diff_layout(&self) -> (Seg, Change) {
        const VALUES: [&str; 3] = ["", "split", "inline"];
        let m = exec::read_settings().get("diffLayout").cloned().unwrap_or(Value::Null);
        let (narrow, wide) = match &m {
            Value::Obj(_) => (m.get("narrow").cloned(), m.get("wide").cloned()),
            Value::Arr(_) => (None, None),
            _ => (Some(m.clone()), Some(m.clone())),
        };
        let pick = |v: Option<Value>| v.and_then(|v| v.as_str().filter(|s| VALUES.contains(s)).map(String::from));
        let (narrow, wide) = (pick(narrow).unwrap_or_default(), pick(wide).unwrap_or_default());
        let now = (narrow == wide).then(|| VALUES.iter().position(|v| *v == narrow).unwrap_or(0));
        let auto = |v: &str| if v.is_empty() { "auto".to_string() } else { v.to_string() };
        let text = match now {
            Some(i) => auto(VALUES[i]),
            None => format!("narrow {} · wide {}", auto(&narrow), auto(&wide)),
        };
        (seg(text, fg(c().blue)), Change::DiffLayout(now))
    }

    /// The rows of the settings view: a group title, or a setting with its value and its help.
    fn settings_rows(&self) -> Vec<Row> {
        let s = &self.settings;
        let blue = |text: String, change: Change| (seg(text, fg(c().blue)), change);
        let images = format!(
            "Images in the preview: pixelated (half blocks), or sharp with the Kitty graphics protocol of Kitty, Ghostty or WezTerm (p). {}",
            match self.graphics {
                None => "Checking this terminal…",
                Some(true) => "This terminal has it.",
                Some(false) => "This terminal does not have it: sharp images would not show.",
            }
        );
        let changed = self.changed_actions().len();
        let icon_theme = or(s.get("icons").filter(|v| v.truthy()).and_then(|i| i.get("label")), "Nerd Font");
        vec![
            Row::group("APPEARANCE"),
            Row::setting(
                "Theme",
                "Colours for a dark or a light terminal background. Auto: by the terminal's background, as each pane reads it. Other panes at their next opening.",
                self.choice("theme", &["auto", "dark", "light"], |v| v),
            ),
            Row::setting(
                "Icon set",
                "Nerd Font glyphs, or emoji, Unicode or ASCII marks for a terminal without a Nerd Font. Other panes at their next opening.",
                self.choice("glyphs", &["nerd", "emoji", "unicode", "ascii"], |v| match v {
                    "nerd" => "Nerd Font",
                    "unicode" => "Unicode",
                    "ascii" => "ASCII",
                    _ => "emoji",
                }),
            ),
            Row::setting(
                "Icon theme",
                "Nerd Font icons, or a file icon theme installed for VS Code (its colours only with Unicode or ASCII marks, none with emoji).",
                blue(icon_theme, Change::IconThemes),
            ),
            Row::group("TREE"),
            Row::setting("Hidden files", "Show dot files in the tree.", self.on_off("hidden")),
            Row::setting("Git-ignored files", "Show the files git ignores, greyed out.", self.on_off("ignored")),
            Row::setting(
                "Compact folders",
                "Show a folder whose only entry is a folder on one row with it (src/main/java). Not while filtering.",
                self.on_off("compact"),
            ),
            Row::setting(
                "Follow terminal",
                "Reveal the folder of the terminal beside the explorer, moving the root when it leaves the tree.",
                self.on_off("follow"),
            ),
            Row::group("OPENING"),
            Row::setting(
                "Enter opens",
                "Enter and double-click on a text file: show it in the preview or in the editor.",
                self.choice("open", &["preview", "editor"], |v| v),
            ),
            Row::setting(
                "Preview follows selection",
                "A file selected by a click or the arrow keys shows in the preview: arrow keys only update an open one.",
                self.on_off("singleClick"),
            ),
            Row::setting(
                "Editor",
                "Command run with the path by e (and o for code files). Empty: VS Code if installed, else a terminal editor. A terminal editor (vim, nano, hx…) opens in the Editor pane.",
                blue(or(s.get("editor"), "default"), Change::Editor),
            ),
            Row::group("PREVIEW"),
            Row::setting(
                "Wrap lines",
                "Wrap long lines in the preview (w). Other previews at their next opening.",
                self.on_off("wrap"),
            ),
            Row::setting(
                "Line numbers",
                "Line numbers in the preview (L). Other previews at their next opening.",
                self.on_off("lineNumbers"),
            ),
            Row::setting(
                "Changes",
                "Lines added, changed or removed since HEAD, beside the lines of the preview and on its scrollbar (c); ] and [ go through them. Other previews at their next opening.",
                self.on_off("changes"),
            ),
            Row::setting(
                "Images",
                &images,
                self.choice("images", &["auto", "blocks", "pixels"], |v| match v {
                    "blocks" => "pixelated",
                    "auto" => "sharp if possible",
                    _ => "always sharp",
                }),
            ),
            Row::setting(
                "Diff layout",
                "Diffs inline or side by side, for narrow and wide panes alike (v in a diff: for its size only). Auto: side by side when the pane is wide enough.",
                self.diff_layout(),
            ),
            Row::group("REVIEW"),
            Row::setting(
                "Notes prefix",
                "Text typed before the review notes sent to the agent (A), such as an instruction: \"Fix these:\", or a command of the agent.",
                blue(or(s.get("reviewPrefix"), "none"), Change::Prefix),
            ),
            Row::setting(
                "Source control tool",
                "Command run by prefix+d in the review tab, at the explorer's root (lazygit, gitui, tig…). Empty: the plugin's source control. An external tool runs the repository's hooks and filters.",
                blue(or(s.get("scmCommand"), "default"), Change::ScmCommand),
            ),
            Row::group("NEW TABS"),
            Row::setting(
                "Open automatically",
                "Open the explorer in new tabs that have a single pane. Off: only with the toggle key (prefix+f with the installer).",
                self.on_off("autoOpen"),
            ),
            Row::setting(
                "Dock side",
                "Where new explorers dock. An open explorer keeps its side.",
                self.choice("side", &["left", "right"], |v| v),
            ),
            Row::group("ADVANCED"),
            Row::setting(
                "Key bindings",
                "Change the explorer keys, saved in keys.json.",
                blue(if changed > 0 { format!("{changed} changed") } else { "default".into() }, Change::Keys),
            ),
            Row::setting(
                "Check for updates",
                "Look for a newer release on GitHub, once a day.",
                self.on_off("updateCheck"),
            ),
        ]
    }

    /// A setting changed: the next value (1: enter, → or a click) or the previous one (-1: ←), or the setting's
    /// dialog (left alone by ←).
    fn change(&mut self, change: &Change, dir: i64) -> Act {
        match *change {
            Change::Toggle(name) => {
                let on = self.settings.get(name).is_some_and(Value::truthy);
                self.set_setting(name, Value::Bool(!on));
            }
            Change::Pick(name, values, now) => {
                let len = values.len() as i64;
                self.set_setting(name, values[(now as i64 + dir).rem_euclid(len) as usize].into());
            }
            Change::DiffLayout(now) => {
                const VALUES: [&str; 3] = ["", "split", "inline"];
                let i = match now {
                    None if dir > 0 => 0,
                    None => VALUES.len() - 1,
                    Some(n) => (n as i64 + dir).rem_euclid(VALUES.len() as i64) as usize,
                };
                self.set_setting("diffLayout", VALUES[i].into());
            }
            _ if dir <= 0 => {}
            Change::IconThemes => return self.open_icon_themes(),
            Change::Keys => return self.open_keys(),
            Change::Editor => {
                return self.ask_text("Editor", "editor", |v| match v {
                    "" => "Editor: default".into(),
                    v => format!("Editor: {v}"),
                });
            }
            Change::ScmCommand => {
                return self.ask_text("Source control tool", "scmCommand", |v| match v {
                    "" => "Source control tool: default".into(),
                    v => format!("Source control tool: {v}"),
                });
            }
            Change::Prefix => {
                return self.ask_text("Notes prefix", "reviewPrefix", |v| match v {
                    "" => "Notes prefix: none".into(),
                    v => format!("Notes prefix: {v}"),
                });
            }
        }
        Act::Now(None)
    }

    /// A text setting asked for, saved trimmed. A value that is not a text (a settings.json edited by hand) fails
    /// ("Action failed"): null before the prompt opens; a number, a boolean or an object after, the prompt left open
    /// and empty; an array does not fail, its prompt opens empty.
    fn ask_text(&mut self, label: &str, name: &'static str, said: fn(&str) -> String) -> Act {
        let value = exec::read_settings().get(name).cloned().unwrap_or(Value::Null);
        let failed = match &value {
            Value::Null => {
                self.state.menu = None;
                return Act::Now(Some("Action failed".into()));
            }
            Value::Str(_) | Value::Arr(_) => false,
            _ => true,
        };
        let done = move |ex: &mut Explorer, text: String| {
            let text = text.trim_matches(crate::unicode::is_space).to_string();
            ex.settings.set(name, text.clone().into());
            let mut saved = exec::read_settings();
            saved.set(name, text.clone().into());
            exec::write_settings(&saved);
            Act::Now(Some(said(&text)))
        };
        self.ask(label, value.as_str().unwrap_or(""), done, true, None);
        Act::Now(failed.then(|| "Action failed".into()))
    }

    /// The newer release to install: its text, colour and key label.
    fn pending_update(&self) -> Option<(String, Rgb, &'static str)> {
        if !self.settings.get("updateCheck").is_some_and(Value::truthy) {
            return None;
        }
        if let Some(v) = &self.latest {
            return Some((format!("↑ {v} available"), c().green, "update"));
        }
        self.skipped.as_ref().map(|v| (format!("{v} skipped"), c().dim, "update anyway"))
    }

    /// The settings view: the find when open and the update when there is one under the title, the settings, the
    /// help of the selected one, the keys, and a prompt. at: screen row of the first setting, n: the rows they get,
    /// update: the row of the update (0: none), detail: the help's lines.
    fn settings_layout(&self, h: usize, w: usize) -> (i64, i64, i64, i64) {
        let find = self.state.settings.as_ref().is_some_and(|v| v.page.find.is_some()) as i64;
        let update = if self.pending_update().is_some() { 1 + find } else { 0 };
        let at = 1 + find + (update > 0) as i64;
        let detail = self.detail_lines(w);
        let n = (h as i64 - at - 2 - detail - self.state.prompt.is_some() as i64).max(1);
        (at, n, update, detail)
    }

    /// The lines of the longest help at this width, whichever setting is selected, so that the list does not move:
    /// from 1 to MAX_DETAIL.
    fn detail_lines(&self, w: usize) -> i64 {
        let rows = self.settings_rows();
        let most = rows.iter().filter_map(|r| r.help.as_ref()).map(|h| ui::wrap(h, w as i64 - 2).len() as i64).max();
        most.unwrap_or(1).clamp(1, MAX_DETAIL)
    }

    fn main_view(&mut self, w: usize, h: usize) -> Vec<String> {
        let rows = self.settings_rows();
        let up = self.pending_update();
        let (_, n, _, detail) = self.settings_layout(h, w);
        let title = self.page_title("Settings", w);
        let focused = self.state.focused;
        let Some(view) = &mut self.state.settings else { return Vec::new() };
        let s = &mut view.page;
        let list = found(&rows, &s.find);
        let mut lines = vec![title];
        if s.find.is_some() {
            lines.push(find_line(s, &list, w));
        }
        if let Some((text, rgb, label)) = &up {
            // The key on the right, left out when it does not fit.
            let mut hint = ui::keys(&[("u", *label)], usize::MAX).segs;
            if text_width(text) + 2 + ui::seg_width(&hint) > w {
                hint = Vec::new();
            }
            let room = (w as i64 - 1 - ui::seg_width(&hint) as i64).max(0) as usize;
            let mut segs = vec![seg(pad_end(&format!(" {text}"), room), fg(*rgb))];
            segs.extend(hint);
            lines.push(line(&segs, w, None));
        }
        lines.extend(list_lines(&list, s, n, w, focused));
        lines.push(rule(&list, s, n, w));
        // The help wrapped, its last line cut when it needs more.
        let r = at(&list, s.sel).filter(|r| r.group.is_none());
        let mut help = ui::wrap(r.and_then(|r| r.help.as_deref()).unwrap_or(""), w as i64 - 2);
        let detail = detail as usize;
        if help.len() > detail {
            let rest = help.split_off(detail - 1).join(" ");
            help.push(ui::ellipsis(&rest, w as i64 - 2));
        }
        for i in 0..detail {
            lines.push(line(&[seg(format!(" {}", help.get(i).map_or("", String::as_str)), fg(c().dim))], w, None));
        }
        lines.push(line(&settings_keys(w).segs, w, Some(c().header)));
        if self.state.prompt.is_some() {
            lines.push(self.prompt_line(w));
        }
        lines.resize(h, line(&[], w, None));
        lines
    }

    /// ← → and enter change the selected setting, u installs the update. Escape, and the keys bound to close and
    /// settings, go back to the tree.
    fn main_key(&mut self, k: &str) {
        let rows = self.settings_rows();
        let Some(view) = &mut self.state.settings else { return };
        if find_key(&mut view.page, k, &rows, &self.keys) {
            return self.render();
        }
        let list = found(&rows, &view.page.find);
        let r = at(&list, view.page.sel).filter(|r| r.group.is_none()).cloned();
        let action = self.keys.get(k).map(String::from);
        let up = self.pending_update();
        let (w, h) = self.size();
        let n = self.settings_layout(h, w).1;
        let to = arrow(k).or(action.as_deref());
        let Some(view) = &mut self.state.settings else { return };
        if k == "escape" || matches!(action.as_deref(), Some("close" | "settings")) {
            self.state.settings = None;
        } else if k == "u" && up.is_some() {
            return self.run_action(|ex| ex.ask_update());
        } else if let Some(d) = to.and_then(|t| moves(n, t)) {
            move_in(&list, &mut view.page, d);
        } else if let Some(change) = r.as_ref().and_then(|r| r.change.clone()) {
            let a = action.as_deref();
            if k == "enter" || k == "right" || matches!(a, Some("open" | "expand")) {
                return self.run_action(|ex| ex.change(&change, 1));
            }
            if k == "left" || a == Some("collapse") {
                return self.run_action(|ex| ex.change(&change, -1));
            }
        }
        self.render();
    }

    /// The wheel moves the selection. A click on the title goes back, on the update installs it, on a setting
    /// changes it, on a key hint acts as its key; elsewhere, it cancels a prompt.
    fn main_mouse(&mut self, m: &Mouse) {
        let rows = self.settings_rows();
        let (w, h) = self.size();
        let (at_row, n, update, detail) = self.settings_layout(h, w);
        let y = m.y - 1;
        let Some(view) = &mut self.state.settings else { return };
        let s = &mut view.page;
        let list = found(&rows, &s.find);
        if m.wheel != 0 {
            move_in(&list, s, m.wheel);
            return self.render();
        }
        if !m.press || m.drag || m.button != 0 {
            return;
        }
        if self.state.prompt.is_some() {
            self.state.prompt = None;
            return self.render();
        }
        if y == 0 {
            self.state.settings = None;
            return self.render();
        }
        if update > 0 && y == update {
            return self.run_action(|ex| ex.ask_update());
        }
        let i = s.top + y - at_row;
        if y >= at_row
            && y < at_row + n
            && let Some(change) = at(&list, i).filter(|r| r.group.is_none()).and_then(|r| r.change.clone())
        {
            s.sel = i;
            return self.run_action(|ex| ex.change(&change, 1));
        }
        // The keys come after the list, its rule and the help lines.
        if y == at_row + n + 1 + detail
            && let Some(k) = ui::key_at(&settings_keys(w), m.x - 1, 0)
        {
            self.main_key(&k);
        }
    }

    // --- icon themes

    /// The default icons and the VS Code themes installed (a scan of the editors' extensions, made when the page
    /// opens), above a sample tree drawn with the theme under the selection. Enter saves it and goes back to the
    /// settings; Escape goes back without a change.
    fn open_icon_themes(&mut self) -> Act {
        // Closed by a key typed meanwhile.
        if self.state.settings.is_none() {
            return Act::Now(None);
        }
        let nerd = crate::obj! { "id" => "", "label" => NERD_FONT_LABEL };
        let saved = self.settings.get("icons").filter(|v| v.truthy());
        let current = saved.filter(|i| i.get("id").is_some_and(Value::truthy)).cloned().unwrap_or_else(|| nerd.clone());
        let found: Vec<Value> = icons::themes().iter().map(icons::Theme::entry).collect();
        let id = current.get("id");
        let mut list = vec![nerd];
        // The saved theme kept in the list when the scan misses it (an editor folder out of reach).
        if id.is_some_and(Value::truthy) && !found.iter().any(|t| t.get("id") == id) {
            list.push(current.clone());
        }
        list.extend(found);
        let sel = list.iter().position(|t| t.get("id") == id).unwrap_or(0) as i64;
        if let Some(view) = &mut self.state.settings {
            view.icons = Some(Icons { page: Page { sel, top: 0, find: None }, list, loaded: Vec::new() });
        }
        Act::Now(None)
    }

    /// The theme of an entry of the list, loaded once (None: the default icons, or a theme not found). Returns its
    /// key in the loaded ones.
    fn load_theme(&mut self, i: i64) -> Option<String> {
        let icons = self.state.settings.as_mut()?.icons.as_mut()?;
        let t = usize::try_from(i).ok().and_then(|i| icons.list.get(i))?;
        let key = json::stringify(t.get("id").unwrap_or(&Value::Null));
        if !icons.loaded.iter().any(|(k, _)| *k == key) {
            let theme = if t.get("id").is_some_and(Value::truthy) { icons::load(t) } else { None };
            icons.loaded.push((key.clone(), theme));
        }
        Some(key)
    }

    fn choose_theme(&mut self, i: i64) -> Act {
        let Some(key) = self.load_theme(i) else { return Act::Now(None) };
        let Some(icons) = self.state.settings.as_mut().and_then(|v| v.icons.as_mut()) else { return Act::Now(None) };
        let t = icons.list[i as usize].clone();
        let at = icons.loaded.iter().position(|(k, _)| *k == key).unwrap_or(0);
        if t.get("id").is_some_and(Value::truthy) && icons.loaded[at].1.is_none() {
            return Act::Now(Some(format!("{}: not found", label_of(&t))));
        }
        // The page closes: its theme goes to the tree.
        let chosen = icons.loaded.swap_remove(at).1;
        let entry = chosen.as_ref().map_or_else(|| "".into(), |t| t.entry.clone());
        self.theme = chosen;
        self.settings.set("icons", entry.clone());
        let mut saved = exec::read_settings();
        saved.set("icons", entry);
        exec::write_settings(&saved);
        if let Some(view) = &mut self.state.settings {
            view.icons = None;
        }
        Act::Now(None)
    }

    /// The themes as list rows, the saved one checked.
    fn theme_rows(&self) -> Vec<Row> {
        let saved = self.settings.get("icons").filter(|v| v.truthy()).and_then(|i| i.get("id")).filter(|v| v.truthy());
        let saved = saved.cloned().unwrap_or_else(|| "".into());
        let Some(icons) = self.state.settings.as_ref().and_then(|v| v.icons.as_ref()) else { return Vec::new() };
        icons
            .list
            .iter()
            .map(|t| {
                let on = t.get("id") == Some(&saved);
                Row::item(&label_of(t), seg(if on { "✓" } else { "" }, fg(c().green)))
            })
            .collect()
    }

    /// The sample gets at least 4 rows, the themes what remains. at: screen row of the first theme, n: the rows they
    /// get.
    fn icons_layout(&self, h: usize) -> (i64, i64, i64) {
        let count = self.state.settings.as_ref().and_then(|v| v.icons.as_ref()).map_or(0, |i| i.list.len()) as i64;
        let room = h as i64 - 3;
        let sample = (room - 1).min(4).max(room - count).min(SAMPLE.len() as i64).max(0);
        (1, (room - sample).min(count).max(1), sample)
    }

    fn icons_view(&mut self, w: usize, h: usize) -> Vec<String> {
        let rows = self.theme_rows();
        let (_, n, sample) = self.icons_layout(h);
        let title = self.page_title("Icon theme", w);
        let focused = self.state.focused;
        let sel = self.state.settings.as_ref().and_then(|v| v.icons.as_ref()).map_or(0, |i| i.page.sel);
        let key = self.load_theme(sel);
        let Some(icons) = self.state.settings.as_mut().and_then(|v| v.icons.as_mut()) else { return Vec::new() };
        let mut lines = vec![title];
        lines.extend(list_lines(&rows, &mut icons.page, n, w, focused));
        lines.push(rule(&rows, &icons.page, n, w));
        // The sample tree, in the theme under the selection.
        let th = icons.loaded.iter().find(|(k, _)| Some(k) == key.as_ref()).and_then(|(_, t)| t.as_ref());
        for &(depth, name, dir, open) in SAMPLE.iter().take(sample as usize) {
            let icon = icons::icon(th, name, dir, open);
            let mark = if dir { chevron(open) } else { " " };
            let segs = [
                seg(format!("{}{mark} ", " ".repeat(1 + depth * 2)), fg(c().dim)),
                seg(icon.0, fg(icon.1)),
                seg(name, ""),
            ];
            lines.push(line(&segs, w, None));
        }
        while lines.len() + 1 < h {
            lines.push(line(&[], w, None));
        }
        lines.push(line(&icons_keys(w).segs, w, Some(c().header)));
        lines.truncate(h);
        lines
    }

    fn icons_key(&mut self, k: &str) {
        let rows = self.theme_rows();
        let action = self.keys.get(k).map(String::from);
        let n = self.icons_layout(self.size().1).1;
        let Some(icons) = self.state.settings.as_mut().and_then(|v| v.icons.as_mut()) else { return };
        let a = action.as_deref();
        let to = arrow(k).or(a);
        if k == "escape" || matches!(a, Some("close" | "settings" | "collapse")) {
            if let Some(view) = &mut self.state.settings {
                view.icons = None;
            }
        } else if let Some(d) = to.and_then(|t| moves(n, t)) {
            move_in(&rows, &mut icons.page, d);
        } else if k == "enter" || matches!(a, Some("open" | "expand")) {
            let sel = icons.page.sel;
            return self.run_action(|ex| ex.choose_theme(sel));
        }
        self.render();
    }

    /// The wheel moves the selection; a click on the title goes back, on a theme chooses it, on a key hint acts as
    /// its key.
    fn icons_mouse(&mut self, m: &Mouse) {
        let rows = self.theme_rows();
        let (w, h) = self.size();
        let (at_row, n, _) = self.icons_layout(h);
        let y = m.y - 1;
        let Some(icons) = self.state.settings.as_mut().and_then(|v| v.icons.as_mut()) else { return };
        if m.wheel != 0 {
            move_in(&rows, &mut icons.page, m.wheel);
            return self.render();
        }
        if !m.press || m.drag || m.button != 0 {
            return;
        }
        if y == 0 {
            if let Some(view) = &mut self.state.settings {
                view.icons = None;
            }
            return self.render();
        }
        let i = icons.page.top + y - at_row;
        if y >= at_row && y < at_row + n && at(&rows, i).is_some() {
            icons.page.sel = i;
            return self.run_action(|ex| ex.choose_theme(i));
        }
        if y == h as i64 - 1
            && let Some(k) = ui::key_at(&icons_keys(w), m.x - 1, 0)
        {
            self.icons_key(&k);
        }
    }

    // --- key bindings

    /// The explorer's actions by group, as in the key help, each with its keys; below the rule, the selected action's
    /// keys and its default ones. Enter waits for a key and adds it to the action (taken from the action that had
    /// it), Delete removes the action's last key, r gives it back its default keys. Saved in keys.json, which the last
    /// row opens in the editor. The page's own keys are the arrows and the bound ones, so that a rebinding cannot
    /// lock it.
    fn open_keys(&mut self) -> Act {
        if let Some(view) = &mut self.state.settings {
            view.keys = Some(Keys { page: Page { sel: 1, top: 0, find: None }, capture: false });
        }
        Act::Now(None)
    }

    fn defaults_of(&self, action: &str) -> Vec<String> {
        self.defaults.iter().find(|(a, _)| a == action).map(|(_, k)| k.clone()).unwrap_or_default()
    }

    fn changed_actions(&self) -> Vec<String> {
        self.defaults.iter().filter(|(a, d)| !same_keys(&self.keys_of(a), d)).map(|(a, _)| a.clone()).collect()
    }

    fn key_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (group, items) in HELP {
            rows.push(Row::group(&group.to_uppercase()));
            for (action, text) in items {
                let mut chars = text.chars();
                let label = chars.next().map_or(String::new(), |f| f.to_uppercase().chain(chars).collect());
                let keys = self.keys_of(action);
                let colour = if same_keys(&keys, &self.defaults_of(action)) { c().blue } else { c().yellow };
                rows.push(Row {
                    action: Some(action.to_string()),
                    ..Row::item(&label, seg(key_text(&keys), fg(colour)))
                });
            }
        }
        rows.push(Row::group("FILE"));
        rows.push(Row::item("Open keys.json", seg("edit…", fg(c().blue))));
        rows
    }

    /// The changes saved, then read back at once rather than at the next check of the file.
    fn save_keys(&mut self, changes: Changes) -> Option<String> {
        let changes: Vec<(String, Option<Value>)> = changes
            .into_iter()
            .map(|(a, k)| (a, k.map(|k| Value::Arr(k.into_iter().map(Value::Str).collect()))))
            .collect();
        if !exec::write_keys("explorer", &changes) {
            return Some("Cannot write keys.json".into());
        }
        self.keys = exec::read_keys("explorer", &self.defaults);
        exec::keys_problem("explorer")
    }

    /// The default keys of an action given back, taken from the actions that have them now; the keys it took from
    /// other actions given back to them.
    fn reset_keys(&mut self, action: &str) -> Option<String> {
        let mut changes: Changes = vec![(action.to_string(), None)];
        let now = |ex: &Explorer, changes: &Changes, a: &str| match changes.iter().find(|(x, _)| x == a) {
            Some((_, Some(keys))) => keys.clone(),
            Some((_, None)) => ex.defaults_of(a),
            None => ex.keys_of(a),
        };
        for k in self.defaults_of(action) {
            if let Some(owner) = self.keys.get(&k).filter(|o| *o != action) {
                let keys = now(self, &changes, owner).into_iter().filter(|x| *x != k).collect();
                put(&mut changes, owner, Some(keys));
            }
        }
        for k in self.keys_of(action) {
            let home = self.defaults.iter().find(|(a, d)| a != action && d.contains(&k)).map(|(a, _)| a.clone());
            if let Some(home) = home
                && !now(self, &changes, &home).contains(&k)
            {
                let mut keys = now(self, &changes, &home);
                keys.push(k);
                put(&mut changes, &home, Some(keys));
            }
        }
        for (a, keys) in &mut changes {
            if keys.as_ref().is_some_and(|k| same_keys(k, &self.defaults_of(a))) {
                *keys = None;
            }
        }
        self.save_keys(changes)
    }

    /// The key pressed while waiting, added to the action and taken from the one that had it.
    fn bind_key(&mut self, action: &str, k: &str) -> Option<String> {
        let owner = self.keys.get(k).map(String::from);
        if owner.as_deref() == Some(action) {
            return Some(format!("{}: already bound", ui::key_label(k)));
        }
        let mut keys = self.keys_of(action);
        keys.push(k.to_string());
        let mut changes: Changes = vec![(action.to_string(), Some(keys))];
        if let Some(owner) = &owner {
            put(&mut changes, owner, Some(self.keys_of(owner).into_iter().filter(|x| x != k).collect()));
        }
        let label = owner
            .as_deref()
            .and_then(|o| HELP.iter().flat_map(|(_, items)| items.iter()).find(|(a, _)| *a == o).map(|(_, t)| *t));
        self.save_keys(changes)
            .or_else(|| owner.map(|o| format!("{} taken from {}", ui::key_label(k), label.unwrap_or(&o))))
    }

    fn keys_layout(&self, h: usize) -> (i64, i64) {
        let find = self.state.settings.as_ref().and_then(|v| v.keys.as_ref()).is_some_and(|p| p.page.find.is_some());
        let find = find as i64;
        (1 + find, (h as i64 - 7 - find).max(1))
    }

    fn keys_view(&mut self, w: usize, h: usize) -> Vec<String> {
        let rows = self.key_rows();
        let (_, n) = self.keys_layout(h);
        let title = self.page_title("Key bindings", w);
        let focused = self.state.focused;
        let Some(p) = self.state.settings.as_mut().and_then(|v| v.keys.as_mut()) else { return Vec::new() };
        let list = found(&rows, &p.page.find);
        let mut lines = vec![title];
        if p.page.find.is_some() {
            lines.push(find_line(&p.page, &list, w));
        }
        lines.extend(list_lines(&list, &mut p.page, n, w, focused));
        lines.push(rule(&list, &p.page, n, w));
        let (capture, finding) = (p.capture, p.page.find.is_some());
        match at(&list, p.page.sel) {
            Some(r) if r.action.is_some() => {
                let action = r.action.as_deref().unwrap_or("");
                let (now, default) = (self.keys_of(action), self.defaults_of(action));
                let changed = !same_keys(&now, &default);
                let segs = [
                    seg(format!(" {}", r.label), "\x1b[1m"),
                    seg(if changed { "  changed" } else { "" }, fg(c().yellow)),
                ];
                lines.push(line(&segs, w, None));
                let colour = if changed { c().yellow } else { c().blue };
                lines.push(line(&[seg(" Keys     ", fg(c().dim)), seg(key_text(&now), fg(colour))], w, None));
                lines.push(line(&[seg(" Default  ", fg(c().dim)), seg(key_text(&default), fg(c().dim))], w, None));
                // r, which the keys line has no room for, told where it applies (typed in the find while it is open).
                let last = if capture {
                    vec![seg(" Press the new key…", fg(c().yellow))]
                } else if changed && !finding {
                    ui::keys(&[("r", "back to the default keys")], w).segs
                } else {
                    Vec::new()
                };
                lines.push(line(&last, w, None));
            }
            Some(r) if r.group.is_none() => {
                lines.push(line(&[seg(" keys.json", "\x1b[1m")], w, None));
                let text = ui::ellipsis("Opened in the editor", w as i64 - 2);
                lines.push(line(&[seg(format!(" {text}"), fg(c().dim))], w, None));
            }
            _ => {}
        }
        while lines.len() + 1 < h {
            lines.push(line(&[], w, None));
        }
        lines.push(line(&keys_keys(w, capture).segs, w, Some(c().header)));
        lines.truncate(h);
        lines
    }

    fn keys_key(&mut self, k: &str) {
        let rows = self.key_rows();
        let (_, n) = self.keys_layout(self.size().1);
        let Some(p) = self.state.settings.as_mut().and_then(|v| v.keys.as_mut()) else { return };
        if p.capture {
            let r = at(&found(&rows, &p.page.find), p.page.sel).map(|r| r.action.clone());
            p.capture = false;
            if k == "escape" {
                return self.render();
            }
            let k = k.to_string();
            // The row gone meanwhile (keys.json read again, the find open): the action fails.
            return self.run_action(|ex| match r {
                None => Act::Now(Some("Action failed".into())),
                Some(a) => Act::Now(a.and_then(|a| ex.bind_key(&a, &k))),
            });
        }
        if find_key(&mut p.page, k, &rows, &self.keys) {
            return self.render();
        }
        let list = found(&rows, &p.page.find);
        let r = at(&list, p.page.sel).cloned();
        let action = self.keys.get(k).map(String::from);
        let a = action.as_deref();
        let to = arrow(k).or(a);
        if k == "escape" || k == "left" || matches!(a, Some("close" | "settings" | "collapse")) {
            if let Some(view) = &mut self.state.settings {
                view.keys = None;
            }
        } else if let Some(d) = to.and_then(|t| moves(n, t)) {
            move_in(&list, &mut p.page, d);
        } else if let Some(r) = r.filter(|r| r.group.is_none()) {
            if k == "enter" || matches!(a, Some("open" | "expand")) {
                if r.action.is_some() {
                    p.capture = true;
                } else {
                    let pane = self.pane.clone();
                    return self.run_action(|ex| match exec::keys_template("explorer", &ex.defaults) {
                        Some(file) => later(move || actions::edit(&file, &pane, None)),
                        None => Act::Now(Some("No config folder".into())),
                    });
                }
            } else if let Some(action) = r.action.clone().filter(|_| k == "delete" || k == "backspace") {
                let mut now = self.keys_of(&action);
                return self.run_action(|ex| {
                    Act::Now(if now.pop().is_some() {
                        ex.save_keys(vec![(action, Some(now))])
                    } else {
                        Some("No key to remove".into())
                    })
                });
            } else if let Some(action) = r.action.filter(|_| k == "r") {
                return self.run_action(|ex| Act::Now(ex.reset_keys(&action)));
            }
        }
        self.render();
    }

    /// The wheel moves the selection. A click on the title goes back, on an action selects it (on the selected one,
    /// waits for its new key), on a key hint acts as its key; any click while waiting cancels.
    fn keys_mouse(&mut self, m: &Mouse) {
        let rows = self.key_rows();
        let (w, h) = self.size();
        let (at_row, n) = self.keys_layout(h);
        let y = m.y - 1;
        let Some(p) = self.state.settings.as_mut().and_then(|v| v.keys.as_mut()) else { return };
        let list = found(&rows, &p.page.find);
        if m.wheel != 0 {
            if !p.capture {
                move_in(&list, &mut p.page, m.wheel);
            }
            return self.render();
        }
        if !m.press || m.drag || m.button != 0 {
            return;
        }
        if p.capture {
            p.capture = false;
            return self.render();
        }
        if y == 0 {
            if let Some(view) = &mut self.state.settings {
                view.keys = None;
            }
            return self.render();
        }
        let i = p.page.top + y - at_row;
        if y >= at_row && y < at_row + n && at(&list, i).is_some_and(|r| r.group.is_none()) {
            if i == p.page.sel {
                return self.keys_key("enter");
            }
            p.page.sel = i;
            return self.render();
        }
        if y == h as i64 - 1
            && let Some(k) = ui::key_at(&keys_keys(w, false), m.x - 1, 0)
        {
            self.keys_key(&k);
        }
    }
}

/// A theme's label, as `${t.label}` gives it.
fn label_of(t: &Value) -> String {
    t.get("label").map_or_else(|| "undefined".into(), crate::json::text)
}

/// Text left-aligned on n columns.
fn pad_end(text: &str, n: usize) -> String {
    let len = ui::text_width(text);
    format!("{text}{}", " ".repeat(n.saturating_sub(len)))
}
