//! What the explorer does with a file or a line: open it (editor, application,
//! browser, file manager), copy its path, create, rename, duplicate, trash or delete it, stage, unstage or discard its
//! changes; insert it into the terminal next to the explorer; open the plugin's panes (preview, changes, search, source
//! control, editor) and keep their widths. Each one returns a short message or None.

use super::exec::{self, Env, Opts, WIN, io_code, read_settings, remote_link, run, walk_realpath, words};
use super::paths;
use super::which::which;
use crate::json::Value;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

const MAC: bool = cfg!(target_os = "macos");

/// The menu's entry that shows a file in the file manager.
pub const REVEAL: &str = if WIN { "Reveal in Explorer" } else { "Open folder" };

// Documents and media: opened with the associated application on double-click. Everything else goes to the editor, so
// a double-click never runs an executable or a script.
const SYSTEM: [&str; 29] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "odt", "ods", "odp", "zip", "7z", "rar", "gz", "tgz", "mp3", "mp4", "wav", "mkv", "avi", "mov",
];
// Also allowed from the menu (Open in app): text formats opened by an editor or a browser.
const TEXTS: [&str; 8] = ["txt", "md", "csv", "log", "json", "xml", "html", "htm"];
const IMAGES: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"];

fn ext(abs: &str) -> String {
    paths::extname(abs).get(1..).unwrap_or("").to_lowercase()
}

fn system_type(e: &str) -> bool {
    SYSTEM.contains(&e)
}

fn safe_type(e: &str) -> bool {
    system_type(e) || TEXTS.contains(&e)
}

/// Whether a file may go to its system application: its type and its link's target's are allowed (doc.pdf linking to a
/// script would be run by macOS's open, which follows it).
pub fn can_open(abs: &str) -> bool {
    let real = walk_realpath(abs).unwrap_or_else(|_| abs.to_string());
    safe_type(&ext(abs)) && safe_type(&ext(&real))
}

/// Shown in the preview: everything but documents and media other than images (Office, PDF, archives, sound, video).
pub fn can_preview(abs: &str) -> bool {
    let e = ext(abs);
    !system_type(&e) || IMAGES.contains(&e.as_str())
}

/// A program started on its own, its outputs dropped, not waited for (a detached child): whether it
/// started. env: its whole environment (the process's when None); verbatim: its arguments passed as written, on
/// Windows (explorer's /select,"…"). A .cmd or .bat cannot run without a shell on Windows: refused.
fn detached(cmd: Option<&str>, args: &[&str], env: Option<Env>, verbatim: bool) -> bool {
    let Some(cmd) = cmd else { return false };
    if WIN && matches!(paths::extname(cmd).to_lowercase().as_str(), ".cmd" | ".bat") {
        return false;
    }
    let mut c = Command::new(cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if verbatim {
            for a in args {
                c.raw_arg(a);
            }
        } else {
            c.args(args);
        }
        c.creation_flags(0x8 | 0x200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = verbatim;
        c.args(args);
        // SAFETY: setsid is async-signal-safe.
        unsafe {
            c.pre_exec(|| {
                crate::sys::setsid();
                Ok(())
            })
        };
    }
    if let Some(env) = env {
        c.env_clear();
        c.envs(env);
    }
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match c.spawn() {
        Ok(mut child) => {
            // Reaped when it ends, without holding the caller.
            std::thread::spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
}

/// Feeds text to a clipboard tool; whether it ended with 0. stdout is dropped: xclip and wl-copy stay in the background
/// holding it.
fn pipe(cmd: &str, args: &[&str], text: &str) -> bool {
    use std::io::Write;
    let spawned =
        Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = spawned else { return false };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(text.as_bytes());
    }
    child.wait().is_ok_and(|s| s.code() == Some(0))
}

/// VS Code: its program, the arguments before the file's, and its environment (None: the process's).
type Code = (String, Vec<String>, Option<Env>);

/// The cli.js that code.cmd runs (`%~dp0..\…cli.js`), relative to code.cmd's parent.
fn cli_of(cmd: &str) -> Option<&str> {
    const AT: &str = "%~dp0..\\";
    let mut from = 0;
    while let Some(i) = cmd[from..].find(AT) {
        let rest = &cmd[from + i + AT.len()..];
        let rest = &rest[..rest.find('"').unwrap_or(rest.len())];
        if let Some(j) = rest.rfind("cli.js") {
            return Some(&rest[..j + "cli.js".len()]);
        }
        from += i + 1;
    }
    None
}

/// VS Code, found once. On Windows code.cmd runs Code.exe on cli.js (ELECTRON_RUN_AS_NODE); done the same, without cmd.
fn find_code() -> Option<&'static Code> {
    static CODE: OnceLock<Option<Code>> = OnceLock::new();
    CODE.get_or_init(|| {
        if !WIN {
            return which("code").map(|c| (c, Vec::new(), None));
        }
        let mut dirs = super::which::path_dirs();
        let local = exec::env_var("LOCALAPPDATA").unwrap_or_default();
        dirs.push(paths::join(&[&local, "Programs", "Microsoft VS Code", "bin"]));
        for dir in dirs {
            let Ok(cmd) = exec::read_text(paths::join(&[&dir, "code.cmd"])) else { continue };
            let exe = paths::join(&[&dir, "..", "Code.exe"]);
            if let Some(cli) = cli_of(&cmd)
                && std::path::Path::new(&exe).exists()
            {
                let mut env = exec::process_env();
                exec::env_set(&mut env, "ELECTRON_RUN_AS_NODE", "1");
                exec::env_set(&mut env, "VSCODE_DEV", "");
                return Some((exe, vec![paths::join(&[&dir, "..", cli])], Some(env)));
            }
        }
        None
    })
    .as_ref()
}

// Editors that need a terminal: given in the settings, they run in the editor pane.
const TERMINAL: [&str; 13] =
    ["vi", "vim", "nvim", "nano", "micro", "hx", "helix", "kak", "emacs", "ne", "joe", "mcedit", "edit"];

fn editor_name(cmd: &str) -> String {
    let name = paths::basename(cmd, None).to_lowercase();
    name.strip_suffix(".exe").map(String::from).unwrap_or(name)
}

fn is_terminal(cmd: Option<&String>) -> bool {
    cmd.is_some_and(|c| !c.is_empty() && TERMINAL.contains(&editor_name(c).as_str()))
}

fn editor_setting() -> String {
    read_settings().get("editor").and_then(Value::as_str).unwrap_or("").to_string()
}

/// The editor of the settings when it needs a terminal.
pub fn terminal_editor() -> Option<String> {
    let command = editor_setting();
    is_terminal(words(&command).first()).then_some(command)
}

// A link to a network share is never handed to a program, which would follow it (see remote_link).
const NETWORK: &str = "Link to a network share: not opened";

/// External editor: the command of the settings, else VS Code, else the terminal editor (files only). The command runs
/// without a shell, the path as its last argument.
pub fn editor(abs: &str, pane: &str) -> Option<String> {
    if remote_link(abs, "") {
        return Some(NETWORK.into());
    }
    let command = editor_setting();
    let w = words(&command);
    let cmd = w.first().filter(|c| !c.is_empty());
    let args: Vec<&str> = w.iter().skip(1).map(String::as_str).collect();
    if is_terminal(cmd) {
        return edit(abs, pane, Some(&command));
    }
    // VS Code's command is a .cmd launcher on Windows, which cannot run without a shell: found as without a setting.
    let code = match cmd {
        None => find_code(),
        Some(c) if WIN && matches!(editor_name(c).as_str(), "code" | "code.cmd") => find_code(),
        _ => None,
    };
    let file = paths::basename(abs, None);
    if let (Some(cmd), None) = (cmd, code) {
        if detached(which(cmd).as_deref(), &[&args[..], &[abs]].concat(), None, false) {
            return Some(format!("{}: {file}", paths::basename(cmd, None)));
        }
        return Some(if WIN { format!("{cmd} not found (an .exe is needed)") } else { format!("{cmd} not found") });
    }
    if let Some((exe, pre, env)) = code {
        let all: Vec<&str> = pre.iter().map(String::as_str).chain(args.iter().copied()).chain([abs]).collect();
        if detached(Some(exe), &all, env.clone(), false) {
            return Some(format!("VS Code: {file}"));
        }
    }
    if cmd.is_some() {
        return Some("VS Code not found".into());
    }
    if std::fs::metadata(abs).is_ok_and(|m| m.is_dir()) {
        return Some("No editor: set one in the settings".into());
    }
    edit(abs, pane, None)
}

/// Whether a URL is a web page the plugin may open: https only, never a path or another scheme.
pub fn web_page(url: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let Some(slash) = rest.find('/') else { return false };
    slash > 0
        && rest[..slash].chars().all(|c| word(c) || c == '.' || c == '-')
        && rest[slash + 1..].chars().all(|c| word(c) || ".#%/-".contains(c))
}

/// A page of the plugin's documentation (or another of its pages) in the browser.
pub fn web(url: &str, what: &str) -> String {
    if !web_page(url) {
        return "Not opened: not a web page".into();
    }
    let ok = if WIN {
        detached(which("explorer.exe").as_deref(), &[url], None, false)
    } else {
        detached(Some(if MAC { "open" } else { "xdg-open" }), &[url], None, false)
    };
    if ok { format!("Opening {what}") } else { "Cannot open a browser".into() }
}

/// A file in its system application, when its type allows it.
pub fn system(abs: &str) -> String {
    if !can_open(abs) {
        return "Not opened: file type not allowed".into();
    }
    if remote_link(abs, "") {
        return NETWORK.into();
    }
    let ok = if WIN {
        detached(which("explorer.exe").as_deref(), &[abs], None, false)
    } else {
        detached(Some(if MAC { "open" } else { "xdg-open" }), &[abs], None, false)
    };
    if ok { format!("Opening {}", paths::basename(abs, None)) } else { "Cannot open".into() }
}

/// Double-click or Enter on a file.
pub fn open(abs: &str, pane: &str) -> Option<String> {
    if system_type(&ext(abs)) { Some(system(abs)) } else { editor(abs, pane) }
}

/// o: a folder in the file manager, a file of the types allowed (documents, media, text formats) in its system
/// application, any other in the editor, so that a script is never handed to the system, which could run it. On macOS,
/// open would launch a folder that is an application (x.app, a link to one): a Finder window shows it instead, or,
/// when Finder cannot be told, it is revealed in its parent.
pub fn app(abs: &str, pane: &str) -> Option<String> {
    if !std::fs::metadata(abs).is_ok_and(|m| m.is_dir()) {
        return if can_open(abs) { Some(system(abs)) } else { editor(abs, pane) };
    }
    if remote_link(abs, "") {
        return Some(NETWORK.into());
    }
    let ok = if WIN {
        detached(which("explorer.exe").as_deref(), &[abs], None, false)
    } else if MAC {
        let script = "on run argv\ntell application \"Finder\"\nactivate\n\
                      make new Finder window to (POSIX file (item 1 of argv))\nend tell\nend run";
        let real = walk_realpath(abs).unwrap_or_else(|_| abs.to_string());
        run(Some("osascript"), &["-e", script, &real], &Opts::default()).is_some()
            || detached(Some("open"), &["-R", abs], None, false)
    } else {
        detached(Some("xdg-open"), &[abs], None, false)
    };
    Some(if ok { format!("Opening {}", paths::basename(abs, None)) } else { "File manager not found".into() })
}

/// The file shown in the file manager: selected in File Explorer or the Finder, its folder opened elsewhere.
pub fn reveal(abs: &str) -> Option<String> {
    if remote_link(abs, "") {
        return Some(NETWORK.into());
    }
    let ok = if WIN {
        // A path cannot contain '"' on Windows.
        detached(which("explorer.exe").as_deref(), &[&format!("/select,\"{abs}\"")], None, true)
    } else if MAC {
        detached(Some("open"), &["-R", abs], None, false)
    } else {
        detached(Some("xdg-open"), &[&paths::dirname(abs)], None, false)
    };
    (!ok).then(|| "File manager not found".into())
}

/// The text to the clipboard; done: the message then. Without a clipboard tool (over SSH), OSC 52 asks the outer
/// terminal to set its clipboard, if herdr passes it on.
pub fn copy(text: &str, done: &str) -> String {
    let ok = if WIN {
        let mut env = exec::process_env();
        exec::env_set(&mut env, "EXPLORER_CLIP", text);
        let args = ["-NoProfile", "-NonInteractive", "-Command", "Set-Clipboard -Value $env:EXPLORER_CLIP"];
        run(which("powershell.exe").as_deref(), &args, &Opts { env: Some(env), ..Default::default() }).is_some()
    } else if MAC {
        pipe("pbcopy", &[], text)
    } else {
        pipe("wl-copy", &[], text) || pipe("xclip", &["-selection", "clipboard"], text)
    };
    if !ok && crate::sys::stdout_is_tty() {
        let _ = crate::sys::write_stdout(
            format!("\x1b]52;c;{}\x07", crate::codec::base64::encode(text.as_bytes())).as_bytes(),
        );
        return format!("{done} (OSC 52)");
    }
    if ok { done.into() } else { "Clipboard unavailable".into() }
}

/// The clipboard's text, line breaks as \n, or None without a tool to read it (over SSH, the terminal's own paste still
/// reaches the pane, as a bracketed paste).
pub fn clipboard() -> Option<String> {
    let opts = Opts { timeout: Some(Duration::from_millis(2000)), ..Default::default() };
    let text = if WIN {
        // In base64: Windows PowerShell writes its output in the console's OEM code page, which loses é or €.
        let script = "[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string](Get-Clipboard -Raw)))";
        run(which("powershell.exe").as_deref(), &["-NoProfile", "-NonInteractive", "-Command", script], &opts).map(
            |t| {
                String::from_utf8_lossy(&crate::codec::base64::decode(t.trim_matches(crate::unicode::is_space)))
                    .into_owned()
            },
        )
    } else if MAC {
        run(which("pbpaste").as_deref(), &[], &opts)
    } else {
        let wayland =
            exec::env_var("WAYLAND_DISPLAY").and_then(|_| run(which("wl-paste").as_deref(), &["--no-newline"], &opts));
        wayland.or_else(|| {
            exec::env_var("DISPLAY")
                .and_then(|_| run(which("xclip").as_deref(), &["-selection", "clipboard", "-o"], &opts))
        })
    };
    text.map(|t| t.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Quoted for the shell unless made of plain characters: single quotes are literal in PowerShell (where the
/// typographic ones count as quotes too, each doubled to escape it) and in POSIX shells ('\'' to escape). fish reads
/// \' and \\ as escapes even there: a backslash goes outside the quotes, as \\, which every shell reads as one. A
/// leading dash would read as an option of the command being typed.
pub fn quote(reference: &str) -> String {
    let r = if reference.starts_with('-') { format!("./{reference}") } else { reference.to_string() };
    let plain = |c: char| crate::unicode::is_letter(c) || crate::unicode::is_number(c) || "_./@:+,=-".contains(c);
    if !r.is_empty() && r.chars().all(plain) {
        return r;
    }
    if WIN {
        let mut out = String::from("'");
        for c in r.chars() {
            out.push(c);
            if matches!(c, '\'' | '‘' | '’' | '‚' | '‛') {
                out.push(c);
            }
        }
        out + "'"
    } else {
        let mut out = String::from("'");
        for c in r.chars() {
            match c {
                '\'' => out += "'\\''",
                '\\' => out += "'\\\\'",
                c => out.push(c),
            }
        }
        out + "'"
    }
}

/// A path relative to a folder when inside it, with '/' separators; else the absolute path.
pub fn relative_to(dir: Option<&str>, abs: &str) -> String {
    let mut reference = abs.to_string();
    if let Some(dir) = dir.filter(|d| !d.is_empty()) {
        let rel = paths::relative(dir, abs);
        if !rel.starts_with("..") && !paths::is_absolute(&rel) {
            reference = if rel.is_empty() { ".".into() } else { rel };
        }
    }
    reference.split(paths::SEP).collect::<Vec<_>>().join("/")
}

// --- plugin panes

/// A herdr command's result, or null; with a time limit, as a herdr that never answers (a restart) would hold the
/// next calls.
fn ask(args: &[&str]) -> Value {
    let opts = Opts { timeout: Some(Duration::from_secs(10)), ..Default::default() };
    exec::result(run(exec::herdr(), args, &opts).as_deref())
}

/// Opens a pane of the plugin. herdr opens a popup over the active pane and refuses a target for it. A tab of its own
/// (the editor) goes in the target's workspace, as herdr takes no target for it either.
pub fn open_pane(entry: &str, placement: &str, target: &str, env: &[(&str, &str)], extra: &[&str]) -> Value {
    let workspace;
    let mut where_: Vec<&str> = vec!["--target-pane", target];
    if placement == "popup" {
        where_.clear();
    } else if placement == "tab" {
        let me = ask(&["pane", "get", target]);
        let Some(me) = me.get("pane").filter(|p| p.truthy()) else { return Value::Null };
        workspace = me.get("workspace_id").map_or_else(|| "undefined".into(), crate::json::text);
        where_ = vec!["--workspace", &workspace];
    }
    let plugin = exec::plugin();
    let pairs: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut args = vec!["plugin", "pane", "open", "--plugin", &plugin, "--entrypoint", entry, "--placement", placement];
    args.extend(where_);
    args.extend(extra);
    for p in &pairs {
        args.extend(["--env", p.as_str()]);
    }
    ask(&args)
}

/// The file in the editor pane, a tab of its own named and focused as source control's and the search's (hold_tab),
/// from the tab they were opened from when asked within one; command: the terminal editor to run there (None: the
/// plugin's).
pub fn edit(abs: &str, pane: &str, command: Option<&str>) -> Option<String> {
    if remote_link(abs, "") {
        return Some(NETWORK.into());
    }
    let from = origin_of(pane);
    let pane = from.as_ref().map_or(pane, |f| f.0.as_str());
    let origin = match &from {
        Some(f) => f.1.clone(),
        None => pane_of(pane).map_or_else(String::new, |p| str_of(&p, "tab_id")),
    };
    let mut env = vec![("EXPLORER_FILE", abs), ("EXPLORER_ORIGIN", &origin)];
    if let Some(c) = command.filter(|c| !c.is_empty()) {
        env.push(("EXPLORER_EDITOR", c));
    }
    (!open_pane("edit", "tab", pane, &env, &["--focus"]).truthy()).then(|| "Cannot open the editor".into())
}

/// The panes of the tab holding a pane, with their labels, and that pane's own record.
pub struct Tab {
    pub me: Value,
    pub panes: Vec<Value>,
}

impl Tab {
    pub fn id(&self) -> String {
        self.me.get("tab_id").map_or_else(|| "undefined".into(), crate::json::text)
    }
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).map_or_else(|| "undefined".into(), crate::json::text)
}

fn label_is(p: &Value, label: &str) -> bool {
    p.get("label").and_then(Value::as_str) == Some(label)
}

fn own_label(p: &Value) -> bool {
    p.get("label").and_then(Value::as_str).is_some_and(|l| exec::OWN_PANES.contains(&l))
}

fn list(v: Value, key: &str) -> Vec<Value> {
    v.get(key).and_then(Value::as_array).cloned().unwrap_or_default()
}

pub fn tab_of(pane: &str) -> Option<Tab> {
    Some(tab_with(pane_of(pane)?))
}

/// The pane itself, as tab_with's input. Apart for a caller whose other herdr calls go between the two.
pub fn pane_of(pane: &str) -> Option<Value> {
    ask(&["pane", "get", pane]).get("pane").filter(|p| p.truthy()).cloned()
}

/// The panes of the tab of me (pane_of).
pub fn tab_with(me: Value) -> Tab {
    let all = list(ask(&["pane", "list", "--workspace", &str_of(&me, "workspace_id")]), "panes");
    let panes = all.into_iter().filter(|p| p.get("tab_id") == me.get("tab_id")).collect();
    Tab { me, panes }
}

/// The preview of a tab reads its requests from a file of the state folder, so that the explorer and the search reuse
/// one preview pane per tab instead of opening a new one for each file.
pub fn request_file(tab: &str) -> Option<String> {
    exec::tab_file("preview", tab)
}

/// What else a preview is asked (showPreview's last argument). marks: the parts to highlight, [{ from, to, a, b }]
/// (1-based lines, and columns a to b included, 0 for whole lines). view: "diff" to show the file's git changes rather
/// than the file. keep: in a kept tab of the preview, rather than the temporary one that the next file replaces (a
/// double-click, Enter). tabs: the tabs of a new preview, { list, current }, as the preview saved them (restore after
/// a herdr restart). beside: the pane on the right of which a new preview opens (a review's source control), rather
/// than the terminal. show: a commit, stash or tag to show rather than a file (diffs.showArgs, with its title and the
/// repository's cwd), or { blank: true, text, cwd } for a line saying what to select (text); the file then "".
/// reveal: the tab's explorer selects the file too (the search's Enter).
#[derive(Clone)]
pub struct Shown {
    pub marks: Vec<Value>,
    pub view: String,
    pub keep: bool,
    pub tabs: Option<Value>,
    pub beside: Option<String>,
    pub show: Option<Value>,
    pub reveal: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Shown {
            marks: Vec::new(),
            view: "file".into(),
            keep: false,
            tabs: None,
            beside: None,
            show: None,
            reveal: false,
        }
    }
}

/// Shows a file in the tab's preview pane, opened next to the terminal when there is none: explorer | terminal |
/// preview. line: 1-based line to scroll to. focus: give the focus to the preview. open: false to only update a preview
/// already open, doing nothing without one. Each call ends before the next, as clicks made while a preview opens would
/// otherwise each find none in the tab and open their own.
#[allow(clippy::too_many_arguments)]
pub fn preview(abs: &str, root: &str, pane: &str, line: f64, focus: bool, open: bool, o: &Shown) -> Option<String> {
    let tab = if pane.is_empty() { None } else { tab_of(pane) };
    let Some(tab) = tab else { return Some("Cannot preview".into()) };
    let mut request = vec![
        ("file".to_string(), Value::Str(abs.into())),
        ("root".into(), Value::Str(root.into())),
        ("line".into(), Value::Num(line)),
        ("marks".into(), Value::Arr(o.marks.clone())),
        ("view".into(), Value::Str(o.view.clone())),
        ("keep".into(), Value::Bool(o.keep)),
    ];
    if let Some(show) = &o.show {
        request.push(("show".into(), show.clone()));
    }
    if o.reveal {
        request.push(("reveal".into(), Value::Bool(true)));
    }
    request.push(("t".into(), Value::Num(crate::core::clock::now_ms())));
    let request = crate::json::stringify(&Value::Obj(request));
    let file = request_file(&tab.id());
    let write = || file.as_deref().is_some_and(|f| exec::write_atomic(f, &request));
    if let Some(existing) = tab.panes.iter().find(|p| label_is(p, "Preview")) {
        // Whole or not at all: the preview polls it.
        if !write() {
            return Some("Cannot preview".into());
        }
        if focus {
            ask(&["plugin", "pane", "focus", &str_of(existing, "pane_id")]);
        }
        return None;
    }
    if !open {
        return None;
    }
    let target = match o.beside.as_deref().filter(|b| !b.is_empty()) {
        Some(b) => b.to_string(),
        None => terminal_of(pane).0.unwrap_or_else(|| pane.to_string()),
    };
    // Written for the explorer too when it must reveal the file; the preview, which starts on it, then skips it (seen).
    if o.reveal {
        write();
    }
    // seen: the request file as it is now, so that a request written while the preview starts is still taken.
    let seen = file
        .as_deref()
        .and_then(|f| std::fs::metadata(f).ok())
        .and_then(|m| m.modified().ok())
        .map_or(0.0, crate::core::clock::ms);
    let (line, seen, tab_id) = (crate::json::number(line), crate::json::number(seen), tab.id());
    let marks = crate::json::stringify(&Value::Arr(o.marks.clone()));
    let tabs = o.tabs.as_ref().map(crate::json::stringify);
    let show = o.show.as_ref().map(crate::json::stringify);
    let mut env = vec![
        ("EXPLORER_FILE", abs),
        ("EXPLORER_ROOT", root),
        ("EXPLORER_LINE", &line),
        ("EXPLORER_TAB", &tab_id),
        ("EXPLORER_SEEN", &seen),
    ];
    if !o.marks.is_empty() {
        env.push(("EXPLORER_MARKS", &marks));
    }
    if o.view == "diff" {
        env.push(("EXPLORER_VIEW", "diff"));
    }
    if o.keep {
        env.push(("EXPLORER_KEEP", "1"));
    }
    if let Some(t) = &tabs {
        env.push(("EXPLORER_TABS", t));
    }
    if let Some(s) = &show {
        env.push(("EXPLORER_SHOW", s));
    }
    if o.beside.as_deref().is_some_and(|b| !b.is_empty()) {
        env.push(("EXPLORER_REVIEW", "1"));
    }
    let extra = ["--direction", "right", if focus { "--focus" } else { "--no-focus" }];
    if open_pane("preview", "split", &target, &env, &extra).truthy() {
        None
    } else {
        Some("Cannot open the preview".into())
    }
}

// The changes pane on the right of the terminal, as the preview: one view at a time there, the others closed, after
// the opening, as the pane asking may be one of them. Zoomed in the terminal's tab instead, herdr's zoom (one per tab,
// following the focus) went to the agent when the focus came back to it, hiding the explorer.
const SIDE: [&str; 2] = ["Preview", "Changes"];

fn side_pane(entry: &str, pane: &str, env: &[(&str, &str)]) -> Value {
    let Some(tab) = tab_of(pane) else { return Value::Null };
    let target = terminal_of(pane).0.unwrap_or_else(|| pane.to_string());
    let res = open_pane(entry, "split", &target, env, &["--direction", "right", "--focus"]);
    if !res.truthy() {
        return Value::Null;
    }
    for p in &tab.panes {
        let id = str_of(p, "pane_id");
        if p.get("label").and_then(Value::as_str).is_some_and(|l| SIDE.contains(&l)) && id != target {
            ask(&["pane", "close", &id]);
        }
    }
    res
}

/// Git changes in a temporary pane on the right of the terminal (q to close it, z to zoom it).
pub fn diff(abs: &str, root: &str, pane: &str) -> Option<String> {
    let env = [("EXPLORER_FILE", abs), ("EXPLORER_ROOT", root), ("EXPLORER_PANE", pane)];
    (!side_pane("diff", pane, &env).truthy()).then(|| "Cannot show changes".into())
}

/// The tab zoomed in and out on the pane (z).
pub fn zoom(pane: &str) -> Value {
    if pane.is_empty() { Value::Null } else { ask(&["pane", "zoom", pane, "--toggle"]) }
}

/// Zoomed out when the pane loses the focus: left zoomed, the tab would show alone whatever pane takes the focus, the
/// agent when one comes back to it.
pub fn unzoom(pane: &str) {
    if !pane.is_empty() && layout_of(pane).get("zoomed").is_some_and(Value::truthy) {
        ask(&["pane", "zoom", pane, "--off"]);
    }
}

/// Go to file (mode files) or search in files (mode content), in a tab of its own, beside the preview of the result
/// selected; the result opened goes to the preview of the pane's tab (EXPLORER_PANE), the search then closed. A search
/// already open for the tab, or asked from within one (its preview), is focused instead.
pub fn search(root: &str, pane: &str, mode: &str, query: &str) -> Option<String> {
    let from = origin_of(pane);
    let pane = from.as_ref().map_or(pane, |f| f.0.as_str());
    let Some(tab) = tab_of(pane) else { return Some("Cannot open the search".into()) };
    let open =
        tab.panes.iter().find(|p| label_is(p, "Search")).cloned().or_else(|| own_of("search", &tab.id(), "Search"));
    if let Some(open) = open {
        return (!focus_pane(&open)).then(|| "Cannot open the search".into());
    }
    let tab_id = tab.id();
    let mut env =
        vec![("EXPLORER_ROOT", root), ("EXPLORER_PANE", pane), ("EXPLORER_MODE", mode), ("EXPLORER_ORIGIN", &tab_id)];
    if !query.is_empty() {
        env.push(("EXPLORER_QUERY", query));
    }
    (!open_pane("search", "tab", pane, &env, &["--focus"]).truthy()).then(|| "Cannot open the search".into())
}

/// Source control opens a review of the pane's tab: a tab of its own, source control then the preview of the files
/// chosen, at full height. Selecting the reviewed tab leads back to it while it runs (hook --focused). A review already
/// open, or source control asked from within one, is focused instead. file: a file or folder to select among the
/// changes; history: show the commits of that file instead. Without a file, the tool of the settings (scmCommand)
/// runs in that tab when there is one (edit::tool).
pub fn scm(root: &str, pane: &str, file: &str, history: bool) -> Option<String> {
    let from = origin_of(pane);
    let pane = from.as_ref().map_or(pane, |f| f.0.as_str());
    let Some(tab) = tab_of(pane) else { return Some("Cannot open source control".into()) };
    let open = tab
        .panes
        .iter()
        .find(|p| label_is(p, "Source Control"))
        .cloned()
        .or_else(|| own_of("review", &tab.id(), "Source Control"));
    if let Some(open) = open {
        return (!focus_pane(&open)).then(|| "Cannot open source control".into());
    }
    let tab_id = tab.id();
    let mut env = vec![("EXPLORER_ROOT", root), ("EXPLORER_PANE", pane), ("EXPLORER_REVIEW", &tab_id)];
    let tool = read_settings().get("scmCommand").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if !file.is_empty() {
        env.push((if history { "EXPLORER_HISTORY" } else { "EXPLORER_FILE" }, file));
    } else if !tool.is_empty() {
        env.push(("EXPLORER_TOOL", &tool));
    }
    (!open_pane("scm", "tab", pane, &env, &["--focus"]).truthy()).then(|| "Cannot open source control".into())
}

// --- tabs of their own: the review (source control), the search and the editor

fn saved_pane(kind: &str, tab: &str) -> Option<Value> {
    let text = exec::read_text(exec::tab_file(kind, tab)?).ok()?;
    crate::json::parse(&text).ok()?.get("pane").cloned()
}

/// For a pane of a tab of its own (source control, the search, the editor), the tab it was opened from: its explorer, else its
/// first terminal, else its first pane, (pane id, tab id); None for any other pane. Source control and the search open
/// from there, so that each never opens from the other's tab, one tab leading to the next.
pub fn origin_of(pane: &str) -> Option<(String, String)> {
    let tab = tab_of(pane)?;
    let mine: Vec<&Value> = tab.panes.iter().filter_map(|p| p.get("pane_id")).collect();
    let ws = str_of(&tab.me, "workspace_id");
    let tabs = list(ask(&["tab", "list", "--workspace", &ws]), "tabs");
    for t in &tabs {
        if t.get("tab_id") == tab.me.get("tab_id") {
            continue;
        }
        let t_id = str_of(t, "tab_id");
        for kind in ["review", "search", "edit"] {
            let Some(saved) = saved_pane(kind, &t_id) else { continue };
            if !mine.iter().any(|m| **m == saved) {
                continue;
            }
            let panes: Vec<Value> = list(ask(&["pane", "list", "--workspace", &ws]), "panes")
                .into_iter()
                .filter(|p| p.get("tab_id") == t.get("tab_id"))
                .collect();
            let p = panes.iter().find(|x| label_is(x, "Explorer")).or_else(|| panes.iter().find(|x| !own_label(x)));
            return p.or(panes.first()).map(|p| (str_of(p, "pane_id"), t_id.clone()));
        }
    }
    None
}

/// A pane of a tab of its own focused, with its tab: herdr's plugin pane focus moves the focus there, but the screen
/// stays on the tab shown.
pub fn focus_pane(p: &Value) -> bool {
    ask(&["plugin", "pane", "focus", &str_of(p, "pane_id")]).truthy()
        && ask(&["tab", "focus", &str_of(p, "tab_id")]).truthy()
}

/// The pane of kind recorded for a tab (hold_tab), with the label it must have; None when there is none or it is gone.
pub fn own_of(kind: &str, tab_id: &str, label: &str) -> Option<Value> {
    let saved = saved_pane(kind, tab_id)?;
    let saved = saved.as_str()?;
    let p = ask(&["pane", "get", saved]).get("pane").cloned()?;
    label_is(&p, label).then_some(p)
}

pub fn review_of(tab_id: &str) -> Option<Value> {
    own_of("review", tab_id, "Source Control")
}

/// The running pane, made from the tab origin in a tab of its own, recorded in <kind>-<origin>.json for own_of. Its tab
/// named "(<name> <origin's label>)", in brackets to set it apart from the agents' tabs, then shown: herdr focuses it
/// on opening, but the screen does not follow a focus given during a key binding's action, and does once it is over.
/// At the exit (exec::exit_hooks), the record goes; shown, the tab hands over to the origin, not to the tab before it
/// as herdr would (it cannot place a tab right after another); the previews it holds close, so the tab.
pub fn hold_tab(kind: &str, origin: &str, name: &str) {
    let Some(pane) = exec::env_var("HERDR_PANE_ID") else { return };
    if origin.is_empty() {
        return;
    }
    let Some(file) = exec::tab_file(kind, origin) else { return };
    let record = Value::Obj(vec![("pane".into(), Value::Str(pane.clone()))]);
    if !exec::write_atomic(&file, crate::json::stringify(&record)) {
        return;
    }
    let (me_pane, from) = (pane.clone(), origin.to_string());
    exec::at_exit(move || {
        let pane = me_pane;
        if exec::read_text(&file)
            .ok()
            .and_then(|t| crate::json::parse(&t).ok())
            .is_some_and(|v| v.get("pane").and_then(Value::as_str) == Some(pane.as_str()))
        {
            let _ = std::fs::remove_file(&file);
        }
        // Synchronous: a failing call ends the clean-up.
        let sync = |args: &[&str]| -> Option<Value> {
            let opts = Opts { timeout: Some(Duration::from_secs(5)), ..Default::default() };
            run(exec::herdr(), args, &opts).map(|out| exec::result(Some(&out)))
        };
        let _ = (|| -> Option<()> {
            let me = sync(&["pane", "get", &pane])?.get("pane").filter(|p| p.truthy())?.clone();
            let workspaces = sync(&["workspace", "list"])?.get("workspaces")?.as_array()?.clone();
            let shown = workspaces
                .iter()
                .any(|w| w.get("focused").is_some_and(Value::truthy) && w.get("active_tab_id") == me.get("tab_id"));
            if shown {
                sync(&["tab", "focus", &from]);
            }
            for p in sync(&["pane", "list", "--workspace", &str_of(&me, "workspace_id")])?.get("panes")?.as_array()? {
                if p.get("tab_id") == me.get("tab_id") && label_is(p, "Preview") {
                    sync(&["pane", "close", &str_of(p, "pane_id")])?;
                }
            }
            Some(())
        })();
    });
    // Not waited for.
    let (kind_name, origin) = (name.to_string(), origin.to_string());
    // Once the process is ending, it asks nothing more: a late focus would
    // come after the exit's and hand the tab back to the one before it.
    let ask = |args: &[&str]| if exec::exiting() { Value::Null } else { ask(args) };
    std::thread::spawn(move || {
        let from = ask(&["tab", "get", &origin]);
        let me = ask(&["pane", "get", &pane]);
        if !me.truthy() {
            return;
        }
        let Some(tab_id) = me.get("pane").and_then(|p| p.get("tab_id")).map(crate::json::text) else { return };
        if from.truthy() {
            // Without its tab: nothing, not even the focus.
            let Some(tab) = from.get("tab").filter(|t| t.truthy()) else { return };
            let label = tab.get("label").map_or_else(|| "undefined".into(), crate::json::text);
            ask(&["tab", "rename", &tab_id, &format!("({kind_name} {label})")]);
        }
        ask(&["tab", "focus", &tab_id]);
    });
}

// --- the terminal next to the explorer

/// The terminal next to the explorer and its side: on the right of an explorer docked on the left, on the left of one
/// docked on the right. Both are tried, as the setting may have changed since the explorer opened. The plugin's own
/// panes are passed over: docked on the right, the preview sits between the terminal and the explorer.
pub fn terminal_of(pane: &str) -> (Option<String>, Option<&'static str>) {
    let right = read_settings().get("side").and_then(Value::as_str) == Some("right");
    let order = if right { ["left", "right"] } else { ["right", "left"] };
    for side in order {
        let mut from = pane.to_string();
        for _ in 0..4 {
            let found = exec::result(
                run(exec::herdr(), &["pane", "neighbor", "--direction", side, "--pane", &from], &Opts::default())
                    .as_deref(),
            );
            let Some(target) = found
                .get("neighbor")
                .and_then(|n| n.get("neighbor_pane_id"))
                .filter(|t| t.truthy())
                .map(crate::json::text)
            else {
                break;
            };
            let p = exec::result(run(exec::herdr(), &["pane", "get", &target], &Opts::default()).as_deref());
            if !p.get("pane").is_some_and(own_label) {
                return (Some(target), Some(side));
            }
            from = target;
        }
    }
    (None, None)
}

fn type_into(target: &str, side: &str, pane: &str, text: &str) -> bool {
    // send-text prints nothing on success: its exit code tells.
    if run(exec::herdr(), &["pane", "send-text", target, text], &Opts::default()).is_none() {
        return false;
    }
    // By direction from the terminal's other neighbour would be fragile: move the focus step by step until it lands.
    let mut from = pane.to_string();
    for _ in 0..4 {
        let args = ["pane", "focus", "--direction", side, "--pane", &from];
        let res = exec::result(run(exec::herdr(), &args, &Opts::default()).as_deref());
        let Some(now) =
            res.get("focus").and_then(|f| f.get("focused_pane_id")).filter(|n| n.truthy()).map(crate::json::text)
        else {
            break;
        };
        if now == target {
            break;
        }
        from = now;
    }
    true
}

fn cwd_of(target: &str) -> Option<String> {
    let p = exec::result(run(exec::herdr(), &["pane", "get", target], &Opts::default()).as_deref());
    p.get("pane").and_then(|p| p.get("cwd")).and_then(Value::as_str).map(String::from)
}

/// Types the path into the terminal next to the explorer, relative to its folder, and focuses it. Prefixed with @ when
/// an agent runs in that terminal (Claude Code), quoted for the shell otherwise; a name with spaces is quoted for the
/// agent too, so that its end is clear and the rest cannot read as instructions. A name with a control character
/// (newline, escape…) could run a command or submit a prompt: refused.
pub fn insert(abs: &str, pane: &str) -> Option<String> {
    let (Some(target), Some(side)) = terminal_of(pane) else { return Some("No terminal next to the explorer".into()) };
    let p = exec::result(run(exec::herdr(), &["pane", "get", &target], &Opts::default()).as_deref());
    let p = p.get("pane").cloned().unwrap_or(Value::Null);
    let reference = relative_to(p.get("cwd").and_then(Value::as_str), abs);
    if reference.chars().any(control) {
        return Some("Not inserted: control character in name".into());
    }
    let agent = p.get("agent").is_some_and(Value::truthy);
    let text = if agent && !reference.chars().any(crate::unicode::is_space) {
        format!("@{reference}")
    } else {
        quote(&reference)
    } + " ";
    (!type_into(&target, side, pane, &text)).then(|| "Cannot insert".into())
}

/// What `type_text` types: a line, or a function of the terminal's folder giving it.
pub enum Line<'a> {
    Text(&'a str),
    Of(&'a dyn Fn(Option<&str>) -> String),
}

/// Types one line of text into the terminal next to the explorer, without submitting it, and focuses that terminal.
/// It must hold no control character: a newline would submit a prompt or run a command.
pub fn type_text(text: Line, pane: &str) -> Option<String> {
    let (Some(target), Some(side)) = terminal_of(pane) else { return Some("No terminal next to the explorer".into()) };
    let text = match text {
        Line::Text(t) => t.to_string(),
        Line::Of(f) => f(cwd_of(&target).as_deref()),
    };
    if text.chars().any(control) {
        return Some("Not typed: control character".into());
    }
    (!type_into(&target, side, pane, &text)).then(|| "Cannot type into the terminal".into())
}

// --- pane widths

/// The tab's layout as seen from a pane: its panes and splits, with their rects; null without one.
pub fn layout_of(pane: &str) -> Value {
    let res = exec::result(run(exec::herdr(), &["pane", "layout", "--pane", pane], &Opts::default()).as_deref());
    res.get("layout").cloned().unwrap_or(Value::Null)
}

/// The split of which a pane is a whole side: left when the pane is its left part. A pane's width includes its border.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// The pane's width.
    pub width: f64,
    /// The split's width and ratio (that of its left part).
    pub split_width: f64,
    pub ratio: f64,
    pub left: bool,
}

fn num(v: &Value, path: &[&str]) -> f64 {
    let mut at = Some(v);
    for k in path {
        at = at.and_then(|x| x.get(k));
    }
    crate::json::num(at)
}

pub fn side_split(layout: &Value, pane: &str) -> Option<Found> {
    let panes = layout.get("panes")?.as_array()?;
    let me = panes.iter().find(|p| p.get("pane_id").and_then(Value::as_str) == Some(pane))?;
    let (x, y, w, h) =
        (num(me, &["rect", "x"]), num(me, &["rect", "y"]), num(me, &["rect", "width"]), num(me, &["rect", "height"]));
    for s in layout.get("splits")?.as_array()? {
        let (sx, sy, sw, sh) =
            (num(s, &["rect", "x"]), num(s, &["rect", "y"]), num(s, &["rect", "width"]), num(s, &["rect", "height"]));
        let ratio = num(s, &["ratio"]);
        if s.get("direction").and_then(Value::as_str) != Some("right") || sy != y || sh != h {
            continue;
        }
        let found = |left| Some(Found { width: w, split_width: sw, ratio, left });
        if sx == x && ((sw * ratio).round() - w).abs() <= 1.0 {
            return found(true);
        }
        if sx + sw == x + w && ((sw * (1.0 - ratio)).round() - w).abs() <= 1.0 {
            return found(false);
        }
    }
    None
}

/// The pane given want columns (its border included) by moving the border of its split; the ratio is that of the
/// split's left part. Returns the ratio asked for.
pub fn set_width(pane: &str, found: &Found, want: f64) -> f64 {
    let ratio = if found.left { (want + 0.25) / found.split_width } else { 1.0 - (want + 0.25) / found.split_width };
    let delta = ratio - found.ratio;
    let amount = format!("{:.6}", delta.abs());
    let direction = if delta > 0.0 { "right" } else { "left" };
    run(
        exec::herdr(),
        &["pane", "resize", "--pane", pane, "--direction", direction, "--amount", &amount],
        &Opts::default(),
    );
    ratio
}

// The plugin's panes that keep their width, and the file where a pane changing its own width names the panes it moves,
// for half a second: those panes are then put back by it, in the same burst (no frame shows them moved), rather than by
// themselves as well, which would move them twice (herdr resizes by a relative amount).
const KEEPING: [&str; 2] = ["Explorer", "Preview"];

fn resizing_file() -> Option<String> {
    exec::state_dir().map(|d| paths::join(&[&d, "resizing"]))
}

fn resizing_elsewhere(pane: &str) -> bool {
    let Some(text) = resizing_file().and_then(|f| exec::read_text(f).ok()) else { return false };
    let Ok(v) = crate::json::parse(&text) else { return false };
    let moved = v.get("moved").and_then(Value::as_array);
    moved.is_some_and(|m| m.iter().any(|p| p.as_str() == Some(pane)))
        && crate::core::clock::now_ms() - crate::json::num(v.get("at")) < 500.0
}

fn mark_resizing(moved: &[String]) {
    // Whole or not at all, as the other panes read it at any time.
    if let Some(f) = resizing_file() {
        let moved = Value::Arr(moved.iter().map(|m| Value::Str(m.clone())).collect());
        let record = Value::Obj(vec![("moved".into(), moved), ("at".into(), Value::Num(crate::core::clock::now_ms()))]);
        exec::write_atomic(&f, crate::json::stringify(&record));
    }
}

/// The widths (borders included) of the other panes of the layout that keep theirs.
fn kept_widths(layout: &Value, pane: &str) -> Vec<(String, f64)> {
    let all = list(ask(&["pane", "list"]), "panes");
    let keeping: Vec<&Value> = all
        .iter()
        .filter(|p| p.get("label").and_then(Value::as_str).is_some_and(|l| KEEPING.contains(&l)))
        .filter_map(|p| p.get("pane_id"))
        .collect();
    let mut out: Vec<(String, f64)> = Vec::new();
    for p in layout.get("panes").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = p.get("pane_id") else { continue };
        if id.as_str() == Some(pane) || !keeping.contains(&id) {
            continue;
        }
        let (id, width) = (crate::json::text(id), num(p, &["rect", "width"]));
        match out.iter_mut().find(|(k, _)| *k == id) {
            Some(e) => e.1 = width,
            None => out.push((id, width)),
        }
    }
    out
}

/// The panes of widths moved by a resize, put back.
fn restore_widths(pane: &str, widths: &[(String, f64)]) {
    let layout = layout_of(pane);
    for (id, width) in widths {
        if let Some(found) = side_split(&layout, id)
            && found.width != *width
        {
            set_width(id, &found, *width);
        }
    }
}

/// The width a pane keeps, as the pane holds it.
pub trait Kept {
    /// The pane's width now, without border.
    fn columns(&self) -> f64;
    /// The width it keeps (None at first: the width it has).
    fn get(&self) -> Option<f64>;
    /// The width taken.
    fn set(&mut self, width: f64);
}

/// Keeps a pane at its width. herdr keeps the ratio of each split, so that moving a border, resizing the window, or
/// opening or closing a pane rescales the panes nested beside it. check(), after each resize of the pane, puts its
/// width back when its own split kept its ratio, and takes the new width when that ratio changed (its own border
/// dragged); check(true) puts it back in any case, as after < > changed the width kept, and puts back the panes this
/// moves (KEEPING). The width kept is at least 10 columns, and leaves 20 to the other side of the split. side: "left"
/// or "right", the part of its split the pane must be (None: either). Each check runs to its end, and the pane's loop reads its pending events before checking.
pub struct WidthKeeper {
    pane: String,
    side: Option<String>,
    ratio: Option<f64>,
    area: f64,
    panes: String,
    // border: the columns of the pane's border, the difference between its width in the layout and its terminal's,
    // learnt when both are up to date: not right after a resize, the terminal hearing of it a little later.
    border: Option<f64>,
    // left: the pane is the left part of its split (its inner border on its right), as last seen; None before.
    left: Option<bool>,
    recheck: Option<std::time::Instant>,
}

impl WidthKeeper {
    pub fn new(pane: &str, side: Option<&str>) -> Self {
        WidthKeeper {
            pane: pane.into(),
            side: side.map(String::from),
            ratio: None,
            area: 0.0,
            panes: String::new(),
            border: None,
            left: None,
            recheck: None,
        }
    }

    pub fn left(&self) -> Option<bool> {
        self.left
    }

    /// When the check asked after panes opened, closed or moved is due.
    pub fn due(&self) -> Option<std::time::Instant> {
        self.recheck
    }

    /// That check, when it is due.
    pub fn tick(&mut self, now: std::time::Instant, kept: &mut impl Kept) {
        if self.recheck.is_some_and(|t| t <= now) {
            self.recheck = None;
            self.check(false, kept);
        }
    }

    pub fn check(&mut self, force: bool, kept: &mut impl Kept) {
        if self.pane.is_empty() {
            return;
        }
        let layout = layout_of(&self.pane);
        if !layout.truthy() || layout.get("zoomed").is_some_and(Value::truthy) {
            return;
        }
        let Some(found) = side_split(&layout, &self.pane) else { return };
        if self.side.as_deref().is_some_and(|s| found.left != (s == "left")) {
            return;
        }
        self.left = Some(found.left);
        if !force || self.border.is_none() {
            self.border = Some(found.width - kept.columns());
        }
        let border = self.border.unwrap_or(0.0);
        let now = found.width - border;
        // A layout without its area: the side and border taken, nothing else changed.
        let Some(area) = layout.get("area").filter(|a| a.truthy()).map(|a| crate::json::num(a.get("width"))) else {
            return;
        };
        let mut ids: Vec<String> = layout
            .get("panes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| p.get("pane_id").map_or_else(|| "undefined".into(), crate::json::text))
            .collect();
        ids.sort();
        let set = ids.join(" ");
        let own = !force
            && self.ratio.is_some_and(|r| (found.ratio - r).abs() >= 0.001)
            && area == self.area
            && set == self.panes;
        // Panes opened, closed or moved: checked again once the layout has settled, as a move made in two steps may
        // leave nothing to report the last one.
        if !self.panes.is_empty() && set != self.panes {
            self.recheck = Some(std::time::Instant::now() + Duration::from_millis(500));
        }
        self.area = area;
        self.panes = set;
        self.ratio = Some(found.ratio);
        let Some(get) = kept.get().filter(|_| !own) else {
            kept.set(now);
            return;
        };
        // Cut down to fit the split, kept as it is: a narrow window does not lower the width wanted back afterwards,
        // only the pane's own resizing (< >) does, so that a key held down stops adding up at the limit.
        let want = (found.split_width - border - 20.0).min(get).max(10.0);
        if force && want != get {
            kept.set(want);
        }
        if now == want || (!force && resizing_elsewhere(&self.pane)) {
            return;
        }
        // Changing its own width (< >), the pane puts back the others it moves.
        let others = if force { kept_widths(&layout, &self.pane) } else { Vec::new() };
        if !others.is_empty() {
            mark_resizing(&others.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
        }
        self.ratio = Some(set_width(&self.pane, &found, want + border));
        if !others.is_empty() {
            restore_widths(&self.pane, &others);
        }
    }
}

/// The share of its tab's width that a pane of its own tab keeps beside its preview: the setting key (35 % at first),
/// set when the preview opens, changed by change (< >) or by dragging the border, and kept for the next ones. seen:
/// the split's ratio as set or seen last, None while the pane is alone or zoomed.
pub struct Sharer {
    pane: String,
    key: String,
    seen: Option<f64>,
}

impl Sharer {
    pub fn new(pane: &str, key: &str) -> Self {
        Sharer { pane: pane.into(), key: key.into(), seen: None }
    }

    pub fn share(&mut self, change: f64) {
        if self.pane.is_empty() {
            return;
        }
        let layout = layout_of(&self.pane);
        let found =
            if layout.get("zoomed").is_some_and(Value::truthy) { None } else { side_split(&layout, &self.pane) };
        let Some(found) = found else {
            self.seen = None;
            return;
        };
        let saved = read_settings();
        // A number between 0 and 1.
        let stored = saved.get(&self.key).map(|v| crate::json::num(Some(v)));
        let mut share = stored.filter(|s| *s > 0.0 && *s < 1.0).unwrap_or(0.35);
        if change != 0.0 {
            share += change;
        } else if self.seen.is_some_and(|s| (found.ratio - s).abs() >= 0.005) {
            share = if found.left { found.ratio } else { 1.0 - found.ratio };
        }
        share = ((share * 1000.0).round() / 1000.0).clamp(0.15, 0.8);
        let want = if found.left { share } else { 1.0 - share };
        let delta = want - found.ratio;
        self.seen = Some(found.ratio);
        if delta.abs() >= 0.005 {
            let amount = format!("{:.6}", delta.abs());
            let direction = if delta > 0.0 { "right" } else { "left" };
            let args = ["pane", "resize", "--pane", &self.pane, "--direction", direction, "--amount", &amount];
            run(exec::herdr(), &args, &Opts::default());
            // herdr rounds the split to whole columns: the ratio it took is the one kept.
            if let Some(after) = side_split(&layout_of(&self.pane), &self.pane) {
                self.seen = Some(after.ratio);
                share = ((if after.left { after.ratio } else { 1.0 - after.ratio }) * 1000.0).round() / 1000.0;
            }
        }
        if saved.get(&self.key) != Some(&Value::Num(share)) {
            let mut settings = read_settings();
            settings.set(&self.key, Value::Num(share));
            exec::write_settings(&settings);
        }
    }
}

// --- files

fn control(c: char) -> bool {
    c <= '\x1f' || ('\x7f'..='\u{9f}').contains(&c)
}

/// A name typed by the user, relative to a folder: subfolders are allowed (a/b.txt), leaving the folder is not.
pub fn target(dir: &str, name: &str) -> Option<String> {
    let name = name.trim_matches(crate::unicode::is_space);
    if name.is_empty() || name.chars().any(control) || paths::is_absolute(name) {
        return None;
    }
    let abs = paths::resolve(&[dir, name]);
    let rel = paths::relative(dir, &abs);
    (!rel.is_empty() && !rel.starts_with("..") && !paths::is_absolute(&rel)).then_some(abs)
}

/// A path made, or why not.
pub type Made = (Option<String>, Option<String>);

/// A file, or a folder (folder, or a name ending with a separator), with the folders it needs.
pub fn create(dir: &str, name: &str, folder: bool) -> Made {
    let Some(abs) = target(dir, name.trim_end_matches(['/', '\\'])) else { return (None, Some("Invalid name".into())) };
    if std::path::Path::new(&abs).exists() {
        return (None, Some("Already exists".into()));
    }
    let made = if folder || name.ends_with(['/', '\\']) {
        std::fs::create_dir_all(&abs)
    } else {
        std::fs::create_dir_all(paths::dirname(&abs))
            .and_then(|_| std::fs::OpenOptions::new().write(true).create_new(true).open(&abs).map(drop))
    };
    match made {
        Ok(()) => (Some(abs), None),
        Err(e) => (None, Some(format!("Cannot create: {}", io_code(&e)))),
    }
}

/// A new name, in the same folder or a subfolder of it; a link is renamed, not followed.
pub fn rename(abs: &str, name: &str) -> Made {
    let Some(to) = target(&paths::dirname(abs), name) else { return (None, Some("Invalid name".into())) };
    if to == abs {
        return (Some(to), None);
    }
    // A case-only change on a case-insensitive file system finds the file itself: the same file, not another one.
    // lstat failing otherwise than on a missing file (a file on the way): an error message.
    match std::fs::symlink_metadata(&to) {
        Ok(_) if !same_file(abs, &to) => return (None, Some("Already exists".into())),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return (None, Some(format!("Cannot rename: {}", io_code(&e))));
        }
        _ => {}
    }
    match std::fs::create_dir_all(paths::dirname(&to)).and_then(|_| std::fs::rename(abs, &to)) {
        Ok(()) => (Some(to), None),
        Err(e) => (None, Some(format!("Cannot rename: {}", io_code(&e)))),
    }
}

/// Whether two paths name the same file (links not followed).
pub fn same_file(a: &str, b: &str) -> bool {
    match (crate::sys::file_id(a), crate::sys::file_id(b)) {
        (Some(x), Some(y)) => x == y && x.1 != 0,
        _ => false,
    }
}

/// fs.cpSync(src, dest, { recursive, errorOnExist, force: false, verbatimSymlinks }): folders made and their mode set
/// after their content, files copied with their mode, links made again with their target as written. A special file
/// (pipe, socket, device) is refused when it is the one copied, passed over inside a folder. The error is
/// its code (io_code).
fn copy_tree(src: &str, dest: &str, top: bool) -> Result<(), String> {
    let code = |e: std::io::Error| io_code(&e);
    let meta = std::fs::symlink_metadata(src).map_err(code)?;
    if std::fs::symlink_metadata(dest).is_ok() {
        return Err("ERR_FS_CP_EEXIST".into());
    }
    let kind = meta.file_type();
    if kind.is_dir() {
        std::fs::create_dir(dest).map_err(code)?;
        for entry in std::fs::read_dir(src).map_err(code)? {
            let name = entry.map_err(code)?.file_name().to_string_lossy().into_owned();
            copy_tree(&paths::join(&[src, &name]), &paths::join(&[dest, &name]), false)?;
        }
        std::fs::set_permissions(dest, meta.permissions()).map_err(code)
    } else if kind.is_file() {
        std::fs::copy(src, dest).map(drop).map_err(code)
    } else if kind.is_symlink() {
        let to = std::fs::read_link(src).map_err(code)?;
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(to, dest);
        // The type of the link copied, rather than that of its target, which would be followed (a share).
        #[cfg(windows)]
        let made = {
            use std::os::windows::fs::FileTypeExt;
            if kind.is_symlink_dir() {
                std::os::windows::fs::symlink_dir(to, dest)
            } else {
                std::os::windows::fs::symlink_file(to, dest)
            }
        };
        made.map_err(code)
    } else if !top {
        Ok(())
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if kind.is_fifo() {
                return Err("ERR_FS_CP_FIFO_PIPE".into());
            }
            if kind.is_socket() {
                return Err("ERR_FS_CP_SOCKET".into());
            }
        }
        Err("ERR_FS_CP_UNKNOWN".into())
    }
}

/// A copy next to the original: "name copy.ext", "name copy 2.ext"…
pub fn duplicate(abs: &str) -> Made {
    if remote_link(abs, "") {
        return (None, Some(NETWORK.into()));
    }
    // A missing file or a broken link: an error message.
    let meta = match std::fs::metadata(abs) {
        Ok(m) => m,
        Err(e) => return (None, Some(format!("Cannot duplicate: {}", io_code(&e)))),
    };
    let ext = if meta.is_dir() { String::new() } else { paths::extname(abs) };
    let base = &abs[..abs.len() - ext.len()];
    let mut to = format!("{base} copy{ext}");
    let mut n = 2;
    while std::path::Path::new(&to).exists() {
        to = format!("{base} copy {n}{ext}");
        n += 1;
    }
    match copy_tree(abs, &to, true) {
        Ok(()) => (Some(to), None),
        Err(code) => (None, Some(format!("Cannot duplicate: {code}"))),
    }
}

/// To the trash of the desktop: gio on Linux, trash (macOS 14) or Finder on macOS, the recycle bin on Windows; whether
/// it went. Never a link to a network share: the recycle bin would follow it, and on Windows a folder link reads as a
/// file to lstat; remove() deletes the link itself.
pub fn trash(abs: &str) -> bool {
    if remote_link(abs, "") {
        return false;
    }
    let none = Opts::default();
    let ok = if WIN {
        // A link (a junction too) as a file: never its target; a missing file is taken as a
        // file.
        let what =
            if std::fs::symlink_metadata(abs).is_ok_and(|m| m.is_dir()) { "DeleteDirectory" } else { "DeleteFile" };
        let mut env = exec::process_env();
        exec::env_set(&mut env, "EXPLORER_TRASH", abs);
        let script = format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::{what}($env:EXPLORER_TRASH, 'OnlyErrorDialogs', 'SendToRecycleBin')"
        );
        let args = ["-NoProfile", "-NonInteractive", "-Command", &script];
        run(which("powershell.exe").as_deref(), &args, &Opts { env: Some(env), ..Default::default() }).is_some()
    } else if MAC {
        let script = "on run argv\ntell application \"Finder\" to delete POSIX file (item 1 of argv)\nend run";
        run(Some("/usr/bin/trash"), &[abs], &none).is_some()
            || run(Some("osascript"), &["-e", script, abs], &none).is_some()
    } else {
        run(Some("gio"), &["trash", "--", abs], &none).is_some()
            || run(Some("trash-put"), &["--", abs], &none).is_some()
    };
    ok && std::fs::symlink_metadata(abs).is_err()
}

/// Deleted for good. A link goes itself, its target untouched (unlink).
pub fn remove(abs: &str) -> Option<String> {
    let gone = match std::fs::symlink_metadata(abs) {
        Ok(m) if m.file_type().is_symlink() => unlink(abs, &m),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(abs),
        Ok(_) => std::fs::remove_file(abs),
        // force: a missing file is no error; any other lstat error is.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    };
    gone.err().map(|e| format!("Cannot delete: {}", io_code(&e)))
}

/// Deletes the file or link abs (st, its lstat), never a folder. unlink refuses a folder link on Windows (and on some
/// mounts, EISDIR): rmdir; and a read-only file on Windows: made writable first, as rmSync does, and back to read-only
/// if it still cannot go. What failed first is what is said.
pub fn unlink(abs: &str, st: &std::fs::Metadata) -> std::io::Result<()> {
    let Err(e) = std::fs::remove_file(abs) else { return Ok(()) };
    let (link, code) = (st.file_type().is_symlink(), io_code(&e));
    if !(WIN && code == "EPERM") && !(link && code == "EISDIR") {
        return Err(e);
    }
    let again = if link {
        std::fs::remove_dir(abs)
    } else {
        let mut writable = st.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        writable.set_readonly(false);
        std::fs::set_permissions(abs, writable).and_then(|()| {
            std::fs::remove_file(abs).inspect_err(|_| {
                let _ = std::fs::set_permissions(abs, st.permissions());
            })
        })
    };
    again.map_err(|_| e)
}

// --- git

fn git(root: &str, args: &[&str]) -> Option<String> {
    // The paths are literal: a file named '*' or 'app/[id]' means itself, not a pattern.
    let mut env = exec::git_env(root);
    exec::env_set(&mut env, "GIT_LITERAL_PATHSPECS", "1");
    run(exec::git(), args, &Opts { cwd: Some(root.into()), env: Some(env), ..Default::default() })
}

pub fn stage(abs: &str, root: &str) -> String {
    if git(root, &["add", "-A", "--", abs]).is_some() { "Staged" } else { "Cannot stage" }.into()
}

/// Without any commit, there is no HEAD to restore the index from: the paths leave the index instead.
pub fn unstage(abs: &str, root: &str) -> String {
    let head = git(root, &["rev-parse", "--verify", "-q", "HEAD"]).filter(|h| !h.is_empty());
    let res = if head.is_some() {
        git(root, &["restore", "--staged", "--", abs])
    } else {
        git(root, &["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--", abs])
    };
    if res.is_some() { "Unstaged" } else { "Cannot unstage" }.into()
}

/// Tracked files go back to their last commit, staged changes included. Files added since (staged but not in the last
/// commit, or all of them before the first commit) leave the index, then go to the trash with the untracked ones:
/// restoring them would delete them for good.
pub fn discard(abs: &str, root: &str) -> String {
    let list = |args: &[&str]| -> Vec<String> {
        git(root, args).unwrap_or_default().split('\0').filter(|f| !f.is_empty()).map(String::from).collect()
    };
    let head = git(root, &["rev-parse", "--verify", "-q", "HEAD"]).filter(|h| !h.is_empty());
    if head.is_some() {
        let added = list(&["diff", "--cached", "--relative", "--name-only", "-z", "--diff-filter=A", "--", abs]);
        if !added.is_empty() {
            let full: Vec<String> = added.iter().map(|f| paths::resolve(&[root, f])).collect();
            let mut args = vec!["restore", "--staged", "--"];
            args.extend(full.iter().map(String::as_str));
            if git(root, &args).is_none() {
                return "Cannot discard".into();
            }
        }
    } else if git(root, &["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--", abs]).is_none() {
        return "Cannot discard".into();
    }
    // Restored from HEAD when it holds something there, even a path whose deletion is staged (not in the index).
    if head.is_some()
        && !list(&["ls-files", "-z", "--with-tree=HEAD", "--", abs]).is_empty()
        && git(root, &["restore", "--source=HEAD", "--staged", "--worktree", "--", abs]).is_none()
    {
        return "Cannot discard".into();
    }
    // Listed after the restore: a file whose deletion was staged is tracked again, not trashed.
    let untracked = list(&["ls-files", "-z", "--others", "--exclude-standard", "--", abs]);
    let kept = untracked.iter().filter(|f| !trash(&paths::resolve(&[root, f]))).count();
    match kept {
        0 => "Changes discarded".into(),
        1 => "1 untracked file kept: no trash".into(),
        n => format!("{n} untracked files kept: no trash"),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A name typed quoted into sh, and fish when installed, comes out as it is.
    #[test]
    fn a_quoted_name_reads_back_alike_in_sh_and_fish() {
        for name in ["\\'; echo INJECTED; echo \\'", "a\\b", "it's", "a\\\\'b"] {
            let script = format!("printf %s {}", quote(name));
            for shell in ["sh", "fish"] {
                let Some(sh) = which(shell) else { continue };
                let mut cmd = Command::new(sh);
                if shell == "fish" {
                    cmd.arg("--no-config");
                }
                let out = cmd.args(["-c", &script]).output().unwrap();
                assert_eq!(String::from_utf8_lossy(&out.stdout), name, "{shell}: {script}");
            }
        }
    }
}
