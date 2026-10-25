//! What the explorer does with a file or a line, as the JS's src/core/actions.js: open it (editor, application,
//! browser, file manager), copy its path, create, rename, duplicate, trash or delete it, stage, unstage or discard its
//! changes. Each one returns a short message or None.

use super::exec::{self, Env, Opts, WIN, io_code, node_realpath, read_settings, remote_link, run, words};
use super::which::which;
use super::{js, paths};
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
    js::lower(paths::extname(abs).get(1..).unwrap_or(""))
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
    let real = node_realpath(abs).unwrap_or_else(|_| abs.to_string());
    safe_type(&ext(abs)) && safe_type(&ext(&real))
}

/// Shown in the preview: everything but documents and media other than images (Office, PDF, archives, sound, video).
pub fn can_preview(abs: &str) -> bool {
    let e = ext(abs);
    !system_type(&e) || IMAGES.contains(&e.as_str())
}

/// A program started on its own, its outputs dropped, not waited for (Node's detached spawn, unref'd): whether it
/// started. env: its whole environment (the process's when None); verbatim: its arguments passed as written, on
/// Windows (explorer's /select,"…"). A .cmd or .bat cannot run without a shell on Windows: refused, as Node does.
fn detached(cmd: Option<&str>, args: &[&str], env: Option<Env>, verbatim: bool) -> bool {
    let Some(cmd) = cmd else { return false };
    if WIN && matches!(js::lower(&paths::extname(cmd)).as_str(), ".cmd" | ".bat") {
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

/// VS Code, found once. On Windows code.cmd runs Code.exe in Node mode on cli.js; done the same, without cmd.
fn find_code() -> Option<&'static Code> {
    static CODE: OnceLock<Option<Code>> = OnceLock::new();
    CODE.get_or_init(|| {
        if !WIN {
            return which("code").map(|c| (c, Vec::new(), None));
        }
        let mut dirs = super::which::path_dirs();
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
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
    let name = js::lower(&paths::basename(cmd, None));
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
        let real = node_realpath(abs).unwrap_or_else(|_| abs.to_string());
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
        run(which("powershell.exe").as_deref(), &["-NoProfile", "-NonInteractive", "-Command", script], &opts)
            .map(|t| String::from_utf8_lossy(&crate::codec::base64::decode(t.trim_matches(js::is_space))).into_owned())
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
/// typographic ones count as quotes too, each doubled to escape it) and in POSIX shells ('\'' to escape). A leading
/// dash would read as an option of the command being typed.
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
        format!("'{}'", r.replace('\'', "'\\''"))
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
        workspace = me.get("workspace_id").map_or_else(|| "undefined".into(), js::string);
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

/// The file in the editor pane, a tab of its own; command: the terminal editor to run there (None: the plugin's).
pub fn edit(abs: &str, pane: &str, command: Option<&str>) -> Option<String> {
    if remote_link(abs, "") {
        return Some(NETWORK.into());
    }
    let mut env = vec![("EXPLORER_FILE", abs)];
    if let Some(c) = command.filter(|c| !c.is_empty()) {
        env.push(("EXPLORER_EDITOR", c));
    }
    (!open_pane("edit", "tab", pane, &env, &[]).truthy()).then(|| "Cannot open the editor".into())
}

// --- files

fn control(c: char) -> bool {
    c <= '\x1f' || ('\x7f'..='\u{9f}').contains(&c)
}

/// A name typed by the user, relative to a folder: subfolders are allowed (a/b.txt), leaving the folder is not.
pub fn target(dir: &str, name: &str) -> Option<String> {
    let name = name.trim_matches(js::is_space);
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
    // The JS throws when lstat fails otherwise than on a missing file (a file on the way): an error message here.
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
/// (pipe, socket, device) is refused when it is the one copied, passed over inside a folder, as Node 26's native
/// cpSync does. The error's code as Node names it.
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
    // The JS throws on a missing file or a broken link (statSync): an error message here.
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
        // The JS's statSync throws on a missing file: taken as a file here.
        let what = if std::fs::metadata(abs).is_ok_and(|m| m.is_dir()) { "DeleteDirectory" } else { "DeleteFile" };
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

/// Deleted for good. A link goes itself, its target untouched: unlink, or rmdir for a folder link on Windows, which
/// unlink refuses.
pub fn remove(abs: &str) -> Option<String> {
    let gone = match std::fs::symlink_metadata(abs) {
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(abs).or_else(|e| {
            if matches!(io_code(&e).as_str(), "EPERM" | "EISDIR") { std::fs::remove_dir(abs) } else { Err(e) }
        }),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(abs),
        Ok(_) => std::fs::remove_file(abs),
        // force: a missing file is no error; any other lstat error is.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    };
    gone.err().map(|e| format!("Cannot delete: {}", io_code(&e)))
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
