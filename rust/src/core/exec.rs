//! Running external programs safely, and helpers shared by the plugin's panes: files written atomically, git's
//! environment for untrusted repositories, versions, the settings, the state kept per pane and per tab, the key
//! bindings. The state and config folders are read from the environment at each call (the JS read them when the
//! module loaded, before any change: peek sets the state folder before loading it).

use super::js;
use super::paths;
use super::which::which;
use crate::json::{self, Value};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

pub const WIN: bool = cfg!(windows);

/// The plugin's id in herdr.
pub fn plugin() -> String {
    std::env::var("HERDR_PLUGIN_ID").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "gcaudan.explorer".into())
}

/// A variable of the environment, None when unset or empty (as `process.env.X && …`).
pub fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// The plugin's state folder (HERDR_PLUGIN_STATE_DIR).
pub fn state_dir() -> Option<String> {
    env_var("HERDR_PLUGIN_STATE_DIR")
}

/// The plugin's config folder (HERDR_PLUGIN_CONFIG_DIR).
pub fn config_dir() -> Option<String> {
    env_var("HERDR_PLUGIN_CONFIG_DIR")
}

/// A symbolic link's target as Node's readlink gives it: on Windows without the `\\?\` prefix Rust adds (and a
/// `\\?\UNC\` share back to `\\server\share`).
pub fn read_link(p: &str) -> std::io::Result<String> {
    let t = std::fs::read_link(p)?.to_string_lossy().into_owned();
    Ok(strip_verbatim(t))
}

/// A path without the verbatim prefix (`\\?\`) Windows' canonical paths carry; unchanged elsewhere.
pub fn strip_verbatim(p: String) -> String {
    if !WIN {
        return p;
    }
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match p.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => p,
    }
}

/// A path that reaches a network share or a device through a symbolic link (or junction) on its way: a link to
/// \\host\share, \\?\… or \\.\…. Opening it would reach the network, which on Windows sends the user's credentials,
/// and a repository can hold such a link. The links are read (readlink), never followed; the folders of top (the
/// tree's root, trusted) and above are not looked at. A loop of links counts as remote.
pub fn remote_link(p: &str, top: &str) -> bool {
    let trusted = (!top.is_empty()).then(|| format!("{}{}", js::lower(&paths::resolve(&[top])), paths::SEP));
    let mut cur = paths::resolve(&[p]);
    for _ in 0..=32 {
        let root = paths::root(&cur);
        let parts: Vec<String> =
            cur[root.len()..].split(paths::SEP).filter(|s| !s.is_empty()).map(String::from).collect();
        let mut next = None;
        let mut at = root.clone();
        for (i, part) in parts.iter().enumerate() {
            at = paths::join(&[&at, part]);
            if let Some(t) = &trusted
                && t.starts_with(&format!("{}{}", js::lower(&at), paths::SEP))
            {
                continue;
            }
            let target = match std::fs::symlink_metadata(&at) {
                Err(_) => return false,
                Ok(m) if m.file_type().is_symlink() => match read_link(&at) {
                    Ok(t) => t,
                    Err(_) => return false,
                },
                Ok(_) => continue,
            };
            let b = target.as_bytes();
            if b.len() >= 2 && matches!(b[0], b'/' | b'\\') && matches!(b[1], b'/' | b'\\') {
                return true;
            }
            let dir = paths::dirname(&at);
            let mut all: Vec<&str> = vec![&dir, &target];
            all.extend(parts[i + 1..].iter().map(String::as_str));
            next = Some(paths::resolve(&all));
            break;
        }
        match next {
            None => return false,
            Some(n) => cur = n,
        }
    }
    true
}

/// A text file read as `readFileSync(file, 'utf8')` does: bytes that are not UTF-8 read as U+FFFD.
pub fn read_text(file: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    std::fs::read(file)
        .map(|b| String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// A path with its symbolic links resolved, as far as it exists; one that reaches a network share (remote_link) is
/// left as given, since resolving it would reach the network.
pub fn real_path(p: &str) -> String {
    if remote_link(p, "") {
        return paths::resolve(&[p]);
    }
    fn real(q: &str) -> String {
        match node_realpath(q) {
            Ok(r) => r,
            Err(_) => {
                let up = paths::dirname(q);
                if up == q { q.to_string() } else { paths::join(&[&real(&up), &paths::basename(q, None)]) }
            }
        }
    }
    real(p)
}

fn pipe_or_socket(m: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        m.file_type().is_fifo() || m.file_type().is_socket()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        false
    }
}

/// Node's `fs.realpathSync` (its JS walk, not the system's realpath): the path resolved first (`..` taken off by
/// name), then each part lstat'ed from the root, a link read and followed, and the walk started over from its target.
/// On Windows the case, short names and mapped drives stay as written.
pub fn node_realpath(p: &str) -> std::io::Result<String> {
    let is_sep = |c: u8| c == b'/' || (WIN && c == b'\\');
    // The root: / on Unix; C:, \\server\share and the separators after it on Windows.
    let split_root = |s: &str| -> usize {
        let b = s.as_bytes();
        let mut i = 0;
        if WIN {
            if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
                i = 2;
            } else if b.len() >= 2 && is_sep(b[0]) && is_sep(b[1]) {
                let server = b[2..].iter().take_while(|&&c| !is_sep(c)).count();
                let at = 2 + server;
                if server > 0 && at < b.len() {
                    let share = b[at + 1..].iter().take_while(|&&c| !is_sep(c)).count();
                    if share > 0 {
                        i = at + 1 + share;
                    }
                }
            }
        }
        i + b[i..].iter().take_while(|&&c| is_sep(c)).count()
    };
    // A root that must exist on Windows: \\?\C:\ probed as C:\.
    let probe = |base: &str| -> std::io::Result<()> {
        let b = base.as_bytes();
        let drive = b.len() == 7 && base.starts_with("\\\\?\\") && b[4].is_ascii_alphabetic() && &base[5..] == ":\\";
        std::fs::symlink_metadata(if drive { &base[4..] } else { base }).map(|_| ())
    };
    let mut p = paths::resolve(&[p]);
    let mut known = std::collections::HashSet::new();
    let mut pos = split_root(&p);
    let (mut current, mut base) = (p[..pos].to_string(), p[..pos].to_string());
    if WIN {
        probe(&base)?;
        known.insert(base.clone());
    }
    let mut piped = false;
    while pos < p.len() {
        let previous = current.clone();
        match p[pos..].bytes().position(is_sep) {
            None => {
                current += &p[pos..];
                base = format!("{previous}{}", &p[pos..]);
                pos = p.len();
            }
            Some(i) => {
                current += &p[pos..=pos + i];
                base = format!("{previous}{}", &p[pos..pos + i]);
                pos += i + 1;
            }
        }
        if known.contains(&base) {
            if piped {
                break;
            }
            continue;
        }
        if !std::fs::symlink_metadata(&base)?.file_type().is_symlink() {
            known.insert(base.clone());
            continue;
        }
        piped = pipe_or_socket(&std::fs::metadata(&base)?);
        let link = read_link(&base)?;
        let resolved = paths::resolve(&[&previous, &link]);
        p = paths::resolve(&[&resolved, &p[pos..]]);
        pos = split_root(&p);
        current = p[..pos].to_string();
        base = current.clone();
        if WIN && !known.contains(&base) {
            probe(&base)?;
            known.insert(base.clone());
        }
    }
    Ok(p)
}

/// An environment for a child, in order, as `{ ...process.env }`.
pub type Env = Vec<(String, String)>;

/// The process's environment.
pub fn process_env() -> Env {
    std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect()
}

pub fn env_get<'a>(env: &'a Env, key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Sets a variable in place, or at the end.
pub fn env_set(env: &mut Env, key: &str, value: &str) {
    match env.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value.to_string(),
        None => env.push((key.to_string(), value.to_string())),
    }
}

pub fn env_remove(env: &mut Env, key: &str) {
    env.retain(|(k, _)| k != key);
}

/// [key, value] pairs added to env as git config (GIT_CONFIG_KEY_n, GIT_CONFIG_VALUE_n), after those already there.
pub fn add_config(env: &mut Env, pairs: &[(&str, &str)]) {
    let mut n = js::int_or_zero(env_get(env, "GIT_CONFIG_COUNT").unwrap_or(""));
    for (key, value) in pairs {
        env_set(env, &format!("GIT_CONFIG_KEY_{}", json::number(n)), key);
        env_set(env, &format!("GIT_CONFIG_VALUE_{}", json::number(n)), value);
        n += 1.0;
    }
    env_set(env, "GIT_CONFIG_COUNT", &json::number(n));
}

/// A file written to a temporary file then renamed, so that another pane never reads it half-written; its folder made
/// when missing (only then, as some files are written often: the resizing marker). Returns whether it was written.
pub fn write_atomic(file: &str, text: impl AsRef<[u8]>) -> bool {
    let tmp = format!("{file}.{}", std::process::id());
    let put = || -> std::io::Result<()> {
        std::fs::write(&tmp, text.as_ref())?;
        std::fs::rename(&tmp, file)
    };
    let res = match put() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(paths::dirname(file)).and_then(|_| put())
        }
        r => r,
    };
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res.is_ok()
}

/// A new folder in the system's temporary folder, its name the prefix and 6 random characters (fs.mkdtempSync).
pub fn mkdtemp(prefix: &str) -> std::io::Result<String> {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let base = std::env::temp_dir().to_string_lossy().into_owned();
    let mut seed = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
        ^ (std::process::id() as u64) << 32;
    loop {
        let mut name = String::from(prefix);
        for _ in 0..6 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            name.push(CHARS[(seed >> 33) as usize % CHARS.len()] as char);
        }
        let dir = paths::join(&[&base, &name]);
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// The labels of the plugin's own panes (herdr-plugin.toml), told apart from the terminals and agents.
pub const OWN_PANES: [&str; 6] = ["Explorer", "Preview", "Source Control", "Changes", "Editor", "Search"];

/// Command line split at spaces outside quotes: "code --wait" → ["code", "--wait"].
pub fn words(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if js::is_space(chars[i]) {
            i += 1;
            continue;
        }
        let c = chars[i];
        let quoted = (c == '"' || c == '\'').then(|| chars[i + 1..].iter().position(|&x| x == c)).flatten();
        let end = match quoted {
            Some(k) => i + 1 + k + 1,
            None => i + chars[i..].iter().position(|&x| js::is_space(x)).unwrap_or(chars.len() - i),
        };
        let w = &chars[i..end];
        // The quotes taken off a word wholly in one kind of them, as /^(["'])(.*)\1$/ (no line break inside).
        let strip = w.len() >= 2
            && (w[0] == '"' || w[0] == '\'')
            && w[w.len() - 1] == w[0]
            && !w[1..w.len() - 1].iter().any(|&x| js::is_line_terminator(x));
        out.push(if strip { w[1..w.len() - 1].iter().collect() } else { w.iter().collect() });
        i = end;
    }
    out
}

/// What a child is given: its folder, environment (the whole of it), standard input, time limit, a session of its own
/// (detached) and a shell to run it.
#[derive(Default, Clone)]
pub struct Opts {
    pub cwd: Option<String>,
    pub env: Option<Env>,
    pub input: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    pub detached: bool,
    pub shell: bool,
}

const MAX_OUT: usize = 64 << 20;

/// A child's end: its exit code (None when killed by a signal), its outputs, and why it failed to run.
struct Outcome {
    code: Option<i32>,
    out: Vec<u8>,
    err: Vec<u8>,
    overflow: bool,
    timed_out: bool,
    failed: Option<String>,
}

/// Node's message for a program that could not start: "spawn git ENOENT".
fn spawn_error(cmd: &str, e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => format!("spawn {cmd} {}", errno_name(n)),
        None => e.to_string(),
    }
}

/// libuv's name of an error code: errno on Unix, a Win32 error translated as libuv does on Windows.
fn errno_name(n: i32) -> String {
    #[cfg(target_os = "linux")]
    let names: &[(i32, &str)] = &[
        (1, "EPERM"),
        (2, "ENOENT"),
        (3, "ESRCH"),
        (4, "EINTR"),
        (5, "EIO"),
        (6, "ENXIO"),
        (7, "E2BIG"),
        (8, "ENOEXEC"),
        (9, "EBADF"),
        (10, "ECHILD"),
        (11, "EAGAIN"),
        (12, "ENOMEM"),
        (13, "EACCES"),
        (14, "EFAULT"),
        (16, "EBUSY"),
        (17, "EEXIST"),
        (18, "EXDEV"),
        (19, "ENODEV"),
        (20, "ENOTDIR"),
        (21, "EISDIR"),
        (22, "EINVAL"),
        (23, "ENFILE"),
        (24, "EMFILE"),
        (25, "ENOTTY"),
        (26, "ETXTBSY"),
        (27, "EFBIG"),
        (28, "ENOSPC"),
        (30, "EROFS"),
        (32, "EPIPE"),
        (36, "ENAMETOOLONG"),
        (38, "ENOSYS"),
        (40, "ELOOP"),
        (95, "ENOTSUP"),
    ];
    #[cfg(all(unix, not(target_os = "linux")))]
    let names: &[(i32, &str)] = &[
        (1, "EPERM"),
        (2, "ENOENT"),
        (3, "ESRCH"),
        (4, "EINTR"),
        (5, "EIO"),
        (6, "ENXIO"),
        (7, "E2BIG"),
        (8, "ENOEXEC"),
        (9, "EBADF"),
        (10, "ECHILD"),
        (12, "ENOMEM"),
        (13, "EACCES"),
        (14, "EFAULT"),
        (16, "EBUSY"),
        (17, "EEXIST"),
        (18, "EXDEV"),
        (19, "ENODEV"),
        (20, "ENOTDIR"),
        (21, "EISDIR"),
        (22, "EINVAL"),
        (23, "ENFILE"),
        (24, "EMFILE"),
        (25, "ENOTTY"),
        (26, "ETXTBSY"),
        (27, "EFBIG"),
        (28, "ENOSPC"),
        (30, "EROFS"),
        (32, "EPIPE"),
        (35, "EAGAIN"),
        (45, "ENOTSUP"),
        (62, "ELOOP"),
        (63, "ENAMETOOLONG"),
        (78, "ENOSYS"),
    ];
    // uv_translate_sys_error (src/win/error.c), for the errors CreateProcess gives.
    #[cfg(windows)]
    let names: &[(i32, &str)] = &[
        (1, "EISDIR"),
        (2, "ENOENT"),
        (3, "ENOENT"),
        (4, "EMFILE"),
        (5, "EPERM"),
        (8, "ENOMEM"),
        (14, "ENOMEM"),
        (15, "ENOENT"),
        (32, "EBUSY"),
        (50, "ENOTSUP"),
        (80, "EEXIST"),
        (87, "EINVAL"),
        (123, "ENOENT"),
        (126, "ENOENT"),
        (161, "ENOENT"),
        (183, "EEXIST"),
        (193, "EFTYPE"),
        (206, "ENAMETOOLONG"),
        (267, "ENOENT"),
        (740, "EACCES"),
        (998, "EACCES"),
        (1920, "EACCES"),
        (1921, "ELOOP"),
    ];
    match names.iter().find(|(code, _)| *code == n) {
        Some((_, name)) => name.to_string(),
        None if cfg!(windows) => "UNKNOWN".into(),
        None => format!("Unknown system error -{n}"),
    }
}

fn command(cmd: &str, args: &[&str], opts: &Opts) -> Command {
    let mut c = if opts.shell {
        // As Node: the command and its arguments joined by spaces, run by the system's shell.
        let line = std::iter::once(cmd).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
        if WIN {
            #[allow(unused_mut)]
            let mut c = Command::new(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()));
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.raw_arg(format!("/d /s /c \"{line}\""));
            }
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(line);
            c
        }
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    };
    if let Some(dir) = &opts.cwd {
        c.current_dir(dir);
    }
    if let Some(env) = &opts.env {
        c.env_clear();
        c.envs(env.iter().map(|(k, v)| (k, v)));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: windowsHide
    }
    #[cfg(unix)]
    if opts.detached {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe.
        unsafe {
            c.pre_exec(|| {
                crate::sys::setsid();
                Ok(())
            })
        };
    }
    c
}

/// Runs a child to its end, its outputs read as they come (beyond MAX_OUT dropped, or the child killed when
/// `kill_on_overflow`).
fn spawn(cmd: &str, args: &[&str], opts: &Opts, kill_on_overflow: bool) -> Outcome {
    let mut c = command(cmd, args, opts);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            return Outcome {
                code: None,
                out: Vec::new(),
                err: Vec::new(),
                overflow: false,
                timed_out: false,
                failed: Some(spawn_error(cmd, &e)),
            };
        }
    };
    let pid = child.id();
    let overflow = Arc::new(Mutex::new(false));
    let reader = |mut r: Box<dyn Read + Send>| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (b, over) = (buf.clone(), overflow.clone());
        let t = std::thread::spawn(move || {
            let mut chunk = [0u8; 65536];
            let mut size = 0usize;
            loop {
                match r.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        size += n;
                        if size <= MAX_OUT {
                            b.lock().unwrap_or_else(|p| p.into_inner()).extend_from_slice(&chunk[..n]);
                        } else {
                            *over.lock().unwrap_or_else(|p| p.into_inner()) = true;
                        }
                    }
                }
            }
        });
        (buf, t)
    };
    let (out, out_t) = reader(Box::new(child.stdout.take().unwrap()));
    let (err, err_t) = reader(Box::new(child.stderr.take().unwrap()));
    if let Some(mut stdin) = child.stdin.take() {
        let input = opts.input.clone().unwrap_or_default();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let kill = |_child: &mut std::process::Child| {
        #[cfg(unix)]
        if opts.detached {
            crate::sys::kill_group(pid);
            return;
        }
        #[cfg(unix)]
        crate::sys::terminate(pid);
        #[cfg(not(unix))]
        let _ = (pid, _child.kill());
    };
    let take = |b: &Arc<Mutex<Vec<u8>>>| std::mem::take(&mut *b.lock().unwrap_or_else(|p| p.into_inner()));
    let deadline = opts.timeout.map(|t| Instant::now() + t);
    let mut timed_out = false;
    let status = loop {
        if kill_on_overflow && *overflow.lock().unwrap_or_else(|p| p.into_inner()) {
            kill(&mut child);
            let _ = child.wait();
            break None;
        }
        match deadline {
            None if !kill_on_overflow => break child.wait().ok(),
            _ => {}
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            kill(&mut child);
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if timed_out {
        // As Node: the end is reported at once, with what came so far; the child is reaped in the background.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Outcome { code: None, out: take(&out), err: take(&err), overflow: false, timed_out, failed: None };
    }
    // The outputs stay open as long as a grandchild holds them: the deadline still counts until they close (the
    // child, reaped, is not killed: its pid may be another's by now).
    while !(out_t.is_finished() && err_t.is_finished()) {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Outcome {
                code: None,
                out: take(&out),
                err: take(&err),
                overflow: false,
                timed_out: true,
                failed: None,
            };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = out_t.join();
    let _ = err_t.join();
    let over = *overflow.lock().unwrap_or_else(|p| p.into_inner());
    Outcome {
        code: status.and_then(|s| s.code()),
        out: take(&out),
        err: take(&err),
        overflow: over,
        timed_out,
        failed: None,
    }
}

/// Runs a program and gives its stdout, or None on failure (including a missing program), as execFile: an exit code
/// other than 0, a signal, a timeout or more than 64 MiB of output.
pub fn run(cmd: Option<&str>, args: &[&str], opts: &Opts) -> Option<String> {
    let cmd = cmd?;
    let o = spawn(cmd, args, opts, true);
    (o.failed.is_none() && !o.overflow && !o.timed_out && o.code == Some(0))
        .then(|| String::from_utf8_lossy(&o.out).into_owned())
}

/// What `exec` gives: the exit code (None when the program could not start, was killed or timed out), stdout and
/// stderr.
#[derive(Debug, Clone, PartialEq)]
pub struct Exec {
    pub code: Option<i32>,
    pub out: String,
    pub err: String,
}

/// Runs a program: its code, stdout and stderr; input goes to stdin. run() returns stdout only, and git's error
/// messages and stdin (commit message, pathspecs) are needed here. detached (not on Windows): a session without
/// terminal, so that ssh, gpg or a credential helper cannot take over the pane to ask for a password; they fail
/// instead, or use their graphical prompt. Killed with its process group at the timeout.
pub fn exec(cmd: Option<&str>, args: &[&str], opts: &Opts) -> Exec {
    let Some(cmd) = cmd else { return Exec { code: None, out: String::new(), err: "not found".into() } };
    let o = spawn(cmd, args, opts, false);
    let mut err = String::from_utf8_lossy(&o.err).into_owned();
    if let Some(f) = o.failed {
        err.push_str(&f);
    }
    Exec { code: o.code, out: String::from_utf8_lossy(&o.out).into_owned(), err }
}

/// The result of a herdr command's JSON answer, or null.
pub fn result(stdout: Option<&str>) -> Value {
    json::parse(stdout.unwrap_or("null")).ok().and_then(|v| v.get("result").cloned()).unwrap_or(Value::Null)
}

/// herdr's program (HERDR_BIN_PATH, else herdr from PATH), found once.
pub fn herdr() -> Option<&'static str> {
    static HERDR: OnceLock<Option<String>> = OnceLock::new();
    HERDR.get_or_init(|| which(&env_var("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".into()))).as_deref()
}

/// git from PATH, found once.
pub fn git() -> Option<&'static str> {
    static GIT: OnceLock<Option<String>> = OnceLock::new();
    GIT.get_or_init(|| which("git")).as_deref()
}

/// Hooks folder that never exists. On Windows, /dev/null would mean C:\dev\null, a folder any user can create.
fn no_hooks() -> String {
    if WIN {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        paths::join(&[&paths::dirname(&strip_verbatim(exe)), ".no-hooks"])
    } else {
        "/dev/null".into()
    }
}

/// Environment for every git call in a repository that may not be trusted, whose config could run commands:
/// - core.fsmonitor and hooks are disabled: hook files through core.hooksPath, and the hooks the repository defines
///   in its config (hook.<name>.command) one by one;
/// - so are the filters (clean, smudge, process) the repository declares itself, in .git/config, config.worktree or
///   an included file; global ones, such as git-lfs, are kept;
/// - no lazy fetching in partial clones, and a core.worktree set by the repository is replaced by the folder holding
///   it;
/// - submodules have their own config, out of reach of this one: git never recurses into them (switch, restore,
///   fetch, pull, push), and callers pass --ignore-submodules=all.
///
/// Passed as GIT_CONFIG_KEY_n / GIT_CONFIG_VALUE_n, which, unlike -c, does not split a driver name containing '='.
/// GIT_OPTIONAL_LOCKS=0 keeps git status from taking index.lock, which would make a concurrent git commit fail.
pub fn git_env(cwd: &str) -> Env {
    let hooks = no_hooks();
    // log.showSignature would run the repository's gpg.program on every signed commit shown.
    let mut config: Vec<(String, String)> = [
        ("core.fsmonitor", "false"),
        ("core.hooksPath", hooks.as_str()),
        ("log.showSignature", "false"),
        ("submodule.recurse", "false"),
        ("fetch.recurseSubmodules", "false"),
        ("push.recurseSubmodules", "no"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let mut base = process_env();
    env_set(&mut base, "GIT_OPTIONAL_LOCKS", "0");
    let listing = run(
        git(),
        &[
            "config",
            "-z",
            "--includes",
            "--show-scope",
            "--name-only",
            "--get-regexp",
            r"^((filter|hook)\..+|core\.worktree)$",
        ],
        &Opts { cwd: Some(cwd.into()), env: Some(base), ..Default::default() },
    )
    .unwrap_or_default();
    let fields: Vec<&str> = listing.split('\0').collect();
    let (mut drivers, mut hooks_found): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut worktree = false;
    let mut i = 0;
    while i + 1 < fields.len() {
        let (scope, name) = (fields[i], fields[i + 1]);
        i += 2;
        if scope == "system" || scope == "global" {
            continue;
        }
        if name == "core.worktree" {
            worktree = true;
            continue;
        }
        // /^(filter|hook)\.(.+)\.[^.]+$/s
        for (prefix, set) in [("filter.", &mut drivers), ("hook.", &mut hooks_found)] {
            if let Some(rest) = name.strip_prefix(prefix)
                && let Some(dot) = rest.rfind('.')
                && dot > 0
                && dot + 1 < rest.len()
            {
                let d = rest[..dot].to_string();
                if !set.contains(&d) {
                    set.push(d);
                }
            }
        }
    }
    for d in &drivers {
        for k in ["clean", "smudge", "process"] {
            config.push((format!("filter.{d}.{k}"), String::new()));
        }
        config.push((format!("filter.{d}.required"), "false".into()));
    }
    for h in &hooks_found {
        config.push((format!("hook.{h}.enabled"), "false".into()));
    }
    // GIT_NO_LAZY_FETCH: in a partial clone, a missing object would make git fetch it from the promisor remote,
    // running that remote's upload-pack or ssh command, even for git status.
    let mut env = process_env();
    env_set(&mut env, "GIT_OPTIONAL_LOCKS", "0");
    env_set(&mut env, "GIT_NO_LAZY_FETCH", "1");
    let pairs: Vec<(&str, &str)> = config.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    add_config(&mut env, &pairs);
    // core.worktree set by the repository could point anywhere (the home folder): switch, pull or discard would write
    // there. The working tree is kept to the folder holding the repository.
    if worktree {
        let dir = run(
            git(),
            &["rev-parse", "--absolute-git-dir"],
            &Opts { cwd: Some(cwd.into()), env: Some(env.clone()), ..Default::default() },
        )
        .unwrap_or_default();
        let dir = dir.trim();
        let tree = if dir.is_empty() { cwd.to_string() } else { paths::dirname(dir) };
        env_set(&mut env, "GIT_WORK_TREE", &tree);
    }
    env
}

/// Operation under way in a repository, from the files git keeps in its folder: "MERGING", "REBASING"… or "".
/// rebase-apply is also git am's (applying); .git/sequencer alone: a cherry-pick or revert of several commits,
/// between two of them once a commit made by hand ended one.
pub fn git_operation(gitdir: &str) -> &'static str {
    let has = |name: &str| std::fs::metadata(paths::join(&[gitdir, name])).is_ok();
    let next = || read_text(paths::join(&[gitdir, "sequencer", "todo"])).unwrap_or_default();
    // /^\s*revert\s/
    let reverts = |t: &str| {
        t.trim_start_matches(js::is_space).strip_prefix("revert").is_some_and(|r| r.starts_with(js::is_space))
    };
    if has("MERGE_HEAD") {
        "MERGING"
    } else if has("rebase-apply/applying") {
        "APPLYING"
    } else if has("rebase-merge") || has("rebase-apply") {
        "REBASING"
    } else if has("CHERRY_PICK_HEAD") {
        "CHERRY-PICKING"
    } else if has("REVERT_HEAD") {
        "REVERTING"
    } else if has("sequencer") {
        if reverts(&next()) { "REVERTING" } else { "CHERRY-PICKING" }
    } else if has("BISECT_LOG") {
        "BISECTING"
    } else {
        ""
    }
}

/// Dotted versions as numbers: "1.10.0" → [1, 10, 0], a part that is no number 0.
pub fn version(v: &str) -> Vec<f64> {
    v.split('.').map(js::int_or_zero).collect()
}

/// Dotted versions compared number by number: newer("1.10.0", "1.9.2") is true.
pub fn newer(a: &str, b: &str) -> bool {
    let (x, y) = (version(a), version(b));
    let at = |v: &[f64], j: usize| v.get(j).copied().filter(|n| *n != 0.0).unwrap_or(0.0);
    match x.iter().enumerate().position(|(j, n)| *n != at(&y, j)) {
        Some(i) => x[i] > at(&y, i),
        None => false,
    }
}

/// Pane width remembered by the explorer (width file in the state folder), 32 columns by default.
pub const DEFAULT_WIDTH: f64 = 32.0;

pub fn width_file() -> Option<String> {
    state_dir().map(|d| paths::join(&[&d, "width"]))
}

pub fn read_width() -> f64 {
    let Some(text) = width_file().and_then(|f| read_text(f).ok()) else { return DEFAULT_WIDTH };
    match js::parse_int(&text) {
        Some(n) => n.max(10.0),
        None => DEFAULT_WIDTH,
    }
}

/// User settings, shared by the explorer, the preview and the hook (settings.json in the state folder): the
/// defaults, in the JS's order.
pub fn defaults() -> Value {
    crate::obj! {
        "hidden" => true,       // show dot files
        "ignored" => true,      // show git-ignored files, greyed out
        "open" => "preview",    // Enter and double-click on a text file: 'preview' or 'editor'
        "editor" => "",         // external editor command ("zed", "subl -w"); empty: VS Code when found, else the terminal editor
        "singleClick" => false, // the preview follows the selection (click or arrows): Preview follows selection
        "side" => "left",       // explorer docked to the left or the right of the tab
        "autoOpen" => true,     // open the explorer in every new tab that has a single pane
        "compact" => true,      // compact folders: a folder whose only entry is a folder shows on one row with it
        "follow" => true,       // reveal the folder of the terminal next to the explorer when it changes
        "glyphs" => "nerd",     // icon set: 'nerd', 'emoji', 'unicode' or 'ascii', for terminals without a Nerd Font
        "theme" => "auto",      // colours: 'dark', 'light', or 'auto', by the terminal's background (ui.settingTheme)
        "icons" => "",          // explorer icon theme: { id, label, file } of a VS Code one; empty: Nerd Font icons
        "wrap" => false,        // preview: wrap long lines
        "lineNumbers" => true,  // preview: line number gutter
        "changes" => true,      // preview: the lines changed since HEAD beside the file's lines and on its scrollbar
        "images" => "auto",     // preview images: 'auto' (pixels when the terminal has the Kitty graphics protocol, else
                                // half blocks), 'blocks' (half blocks), 'pixels' (pixels without waiting for the answer)
        "reviewPrefix" => "",   // review notes: text typed before them when sent to the agent (A), such as an instruction
        "diffLayout" => "",     // diffs 'inline' or 'split' (side by side), or { narrow, wide } (diff.js); empty: by width
        "updateCheck" => true,  // check GitHub once a day for a newer release
        "scmTree" => false,     // source control: the changes as a folder tree rather than a list
        "scmGraphAll" => true,  // source control history: every branch, or only the current one
        "scmListShare" => Value::Obj(Vec::new()), // source control: the list's share of the width beside the diff, by view
    }
}

pub fn settings_file() -> Option<String> {
    state_dir().map(|d| paths::join(&[&d, "settings.json"]))
}

/// The own enumerable properties a spread (`{ ...v }`) copies: an object's, an array's or a string's by index,
/// nothing for other values.
pub fn spread(v: &Value) -> Vec<(String, Value)> {
    match v {
        Value::Obj(fields) => fields.clone(),
        Value::Arr(items) => items.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone())).collect(),
        Value::Str(s) => s.chars().enumerate().map(|(i, c)| (i.to_string(), Value::Str(c.to_string()))).collect(),
        _ => Vec::new(),
    }
}

/// `{ ...a, ...b }`.
pub fn merge(a: &Value, b: &Value) -> Value {
    let mut fields = spread(a);
    for (k, v) in spread(b) {
        json::insert(&mut fields, k, v);
    }
    Value::Obj(fields)
}

/// The settings: the defaults, with those of the file over them.
pub fn read_settings() -> Value {
    let parsed = settings_file().and_then(|f| read_text(f).ok()).and_then(|t| json::parse(&t).ok());
    match parsed {
        Some(v) => merge(&defaults(), &v),
        None => defaults(),
    }
}

pub fn write_settings(settings: &Value) {
    if let Some(f) = settings_file() {
        write_atomic(&f, json::stringify_pretty(settings, 1));
    }
}

/// What each plugin pane shows, one file per pane in the state folder (panes/), since herdr does not keep it across a
/// restart: the explorer's space folder and tree root, which Set as root may have moved; the preview's file.
pub fn panes_dir() -> Option<String> {
    state_dir().map(|d| paths::join(&[&d, "panes"]))
}

fn pane_name(pane: &str) -> String {
    format!("{}.json", js::replace_chars(pane, |c| js::is_word(c) || c == '.' || c == '-'))
}

pub fn pane_file(pane: &str) -> Option<String> {
    let dir = panes_dir()?;
    (!pane.is_empty()).then(|| paths::join(&[&dir, &pane_name(pane)]))
}

pub fn read_pane_state(pane: &str) -> Value {
    pane_file(pane).and_then(|f| read_text(f).ok()).and_then(|t| json::parse(&t).ok()).unwrap_or(Value::Null)
}

pub fn write_pane_state(pane: &str, data: &Value) {
    if let Some(f) = pane_file(pane) {
        write_atomic(&f, json::stringify(data));
    }
}

/// What a tab keeps in the state folder, one file per tab and kind: <kind>-<tab id>.json. The kinds: the review notes
/// (notes), the preview's requests (actions), the diff scope and the agent's last turn (scope), the files viewed
/// (viewed).
pub const TAB_KINDS: [&str; 7] = ["notes", "preview", "review", "scope", "search", "turn", "viewed"];

fn tab_name(kind: &str, tab: &str) -> String {
    format!("{kind}-{}.json", js::replace_chars(tab, |c| js::is_word(c) || c == '-'))
}

pub fn tab_file(kind: &str, tab: &str) -> Option<String> {
    let dir = state_dir()?;
    (!tab.is_empty()).then(|| paths::join(&[&dir, &tab_name(kind, tab)]))
}

/// The state files left by tabs and panes that no longer exist: names, the files of the state folder; panes, those of
/// its panes folder; tabs and live_panes, the ids that still exist. Returns the paths to remove.
pub fn stale_state(
    dir: &str,
    names: &[String],
    panes: &[String],
    tabs: &[String],
    live_panes: &[String],
) -> Vec<String> {
    let keep_tabs: Vec<String> = TAB_KINDS.iter().flat_map(|k| tabs.iter().map(move |t| tab_name(k, t))).collect();
    // /^(notes|preview|…)-.+[.]json$/
    let kind_file = |n: &str| {
        TAB_KINDS.iter().any(|k| {
            n.strip_prefix(k)
                .and_then(|r| r.strip_prefix('-'))
                .and_then(|r| r.strip_suffix(".json"))
                .is_some_and(|mid| !mid.is_empty() && !mid.chars().any(js::is_line_terminator))
        })
    };
    let keep_panes: Vec<String> = live_panes.iter().map(|p| pane_name(p)).collect();
    let mut out: Vec<String> =
        names.iter().filter(|n| kind_file(n) && !keep_tabs.contains(n)).map(|n| paths::join(&[dir, n])).collect();
    out.extend(panes.iter().filter(|n| !keep_panes.contains(n)).map(|n| paths::join(&[dir, "panes", n])));
    out
}

fn list_dir(d: &str) -> Vec<String> {
    std::fs::read_dir(d)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

fn modified_before(f: &str, limit: SystemTime) -> bool {
    std::fs::metadata(f).and_then(|m| m.modified()).is_ok_and(|t| t < limit)
}

/// Removes the state of the tabs and panes that no longer exist (stale_state); the ids of those that do, from herdr.
/// A file written in the last 10 s stays: a tab opened after herdr gave its list may already have one.
pub fn sweep_state(tabs: &[String], live_panes: &[String]) {
    let Some(dir) = state_dir() else { return };
    let since = SystemTime::now() - Duration::from_secs(10);
    let names = list_dir(&dir);
    let panes = list_dir(&paths::join(&[&dir, "panes"]));
    for f in stale_state(&dir, &names, &panes, tabs, live_panes) {
        if modified_before(&f, since) {
            let _ = std::fs::remove_dir_all(&f).or_else(|_| std::fs::remove_file(&f));
        }
    }
    // The copies of indexes that the agent-turn snapshots keep (scope's kept index), unused for a week.
    let week = SystemTime::now() - Duration::from_secs(7 * 24 * 3600);
    let index = paths::join(&[&dir, "index"]);
    for n in list_dir(&index) {
        let f = paths::join(&[&index, &n]);
        if modified_before(&f, week) {
            let _ = std::fs::remove_file(&f);
        }
    }
}

/// Key bindings, editable by the user: keys.json in the plugin config folder, one object per pane (only the
/// explorer reads its own so far), mapping an action to its keys ({ "explorer": { "newFile": ["a"] } }). An action
/// left out keeps its default keys.
pub fn keys_file() -> Option<String> {
    config_dir().map(|d| paths::join(&[&d, "keys.json"]))
}

/// A pane's actions and their keys, in order.
pub type Bindings = Vec<(String, Vec<String>)>;

/// Key name → action, in the order the keys were first bound (a JS Map).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct KeyMap(pub Vec<(String, String)>);

impl KeyMap {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, a)| a.as_str())
    }

    fn set(&mut self, key: &str, action: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, a)) => *a = action.to_string(),
            None => self.0.push((key.to_string(), action.to_string())),
        }
    }
}

fn bindings_value(defaults: &Bindings) -> Value {
    Value::Obj(
        defaults
            .iter()
            .map(|(a, keys)| (a.clone(), Value::Arr(keys.iter().map(|k| Value::Str(k.clone())).collect())))
            .collect(),
    )
}

/// The value of a key of a parsed JSON value, as `v[key]` reads it (an array's or a string's index too).
fn member(v: &Value, key: &str) -> Value {
    match v {
        Value::Obj(_) => v.get(key).cloned().unwrap_or(Value::Null),
        Value::Arr(items) => key.parse::<usize>().ok().and_then(|i| items.get(i).cloned()).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// Key name → action. A key the user gives to an action is taken from the action that had it by default. A pane's
/// entry that is a string, a number or true throws in the JS (`in` on a primitive): the pane ends.
pub fn read_keys(pane: &str, defaults: &Bindings) -> KeyMap {
    let parsed = keys_file().and_then(|f| read_text(f).ok()).and_then(|t| json::parse(&t).ok());
    let user = parsed.map(|v| member(&v, pane)).filter(Value::truthy).unwrap_or(Value::Obj(Vec::new()));
    if let (Value::Str(_) | Value::Num(_) | Value::Bool(_), Some((first, _))) = (&user, defaults.first()) {
        panic!("Cannot use 'in' operator to search for '{first}' in {}", js::string(&user));
    }
    let entries = spread(&user);
    let has = |a: &str| entries.iter().any(|(k, _)| k == a);
    let mut map = KeyMap::default();
    for (action, keys) in defaults {
        if !has(action) {
            for k in keys {
                map.set(k, action);
            }
        }
    }
    for (action, keys) in &entries {
        if defaults.iter().any(|(a, _)| a == action) {
            let list = match keys {
                Value::Arr(items) => items.clone(),
                other => vec![other.clone()],
            };
            for k in list {
                if let Value::Str(k) = k {
                    map.set(&k, action);
                }
            }
        }
    }
    map
}

/// keys.json parsed, or why not: Err(true) when it is missing (an empty start), Err(false) when unreadable or not
/// JSON (left alone for its owner to fix).
fn read_all_keys(file: &str) -> Result<Value, bool> {
    match read_text(file) {
        Err(e) => Err(e.kind() == std::io::ErrorKind::NotFound),
        Ok(t) => json::parse(&t).map_err(|_| false),
    }
}

/// `all[pane] = value` on what keys.json held: an array takes it as a property; null or another value throws in the
/// JS (strict mode), and the pane ends.
fn assign(all: Value, pane: &str, value: Value) -> Value {
    let mut fields = match all {
        Value::Obj(f) => f,
        Value::Arr(_) => spread(&all),
        other => {
            let kind = match other {
                Value::Num(_) => "number",
                Value::Str(_) => "string",
                _ => "boolean",
            };
            panic!("Cannot create property '{pane}' on {kind} '{}'", js::string(&other))
        }
    };
    json::insert(&mut fields, pane.to_string(), value);
    Value::Obj(fields)
}

/// The file with every action of the pane and its current keys, created or completed so that the user edits a full
/// list; returns its path.
pub fn keys_template(pane: &str, defaults: &Bindings) -> Option<String> {
    let file = keys_file()?;
    let all = match read_all_keys(&file) {
        Ok(v) => v,
        Err(true) => Value::Obj(Vec::new()),
        Err(false) => return Some(file),
    };
    if all.is_null() {
        panic!("Cannot read properties of null (reading '{pane}')");
    }
    let own = member(&all, pane);
    let keys = merge(&bindings_value(defaults), if own.truthy() { &own } else { &Value::Null });
    let present = matches!(&all, Value::Obj(f) if f.iter().any(|(k, _)| k == pane));
    if present && json::stringify(&own) == json::stringify(&keys) {
        return Some(file);
    }
    save_keys(&assign(all, pane, keys));
    Some(file)
}

/// Bindings changed from a pane: each action of changes given its keys, or its default ones (None). An unreadable
/// file is left alone, for its owner to fix. Returns false when nothing could be written.
pub fn write_keys(pane: &str, changes: &[(String, Option<Value>)]) -> bool {
    let Some(file) = keys_file() else { return false };
    let all = match read_all_keys(&file) {
        Ok(v) => v,
        Err(true) => Value::Obj(Vec::new()),
        Err(false) => return false,
    };
    if all.is_null() {
        panic!("Cannot read properties of null (reading '{pane}')");
    }
    let mut own = Value::Obj(spread(&member(&all, pane)));
    for (action, keys) in changes {
        match keys {
            None => {
                own.remove(action);
            }
            Some(k) => own.set(action, k.clone()),
        }
    }
    save_keys(&assign(all, pane, own))
}

/// keys.json written with one action per line, through a temporary file renamed, so that a pane never reads it
/// half-written. A pane's entry that is null throws in the JS (Object.entries), and the pane ends.
fn save_keys(all: &Value) -> bool {
    let Some(file) = keys_file() else { return false };
    let quote = |s: &str| {
        let mut q = String::new();
        json::quote(&mut q, s);
        q
    };
    let lines: Vec<String> = spread(all)
        .iter()
        .map(|(p, actions)| {
            if actions.is_null() {
                panic!("Cannot convert undefined or null to object");
            }
            let body: Vec<String> =
                spread(actions).iter().map(|(a, k)| format!("    {}: {}", quote(a), json::stringify(k))).collect();
            format!("  {}: {{\n{}\n  }}", quote(p), body.join(",\n"))
        })
        .collect();
    write_atomic(&file, format!("{{\n{}\n}}\n", lines.join(",\n")))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str, opts: &Opts) -> Exec {
        exec(Some("/bin/sh"), &["-c", script], opts)
    }

    #[test]
    fn runs_programs() {
        assert_eq!(run(Some("/bin/sh"), &["-c", "printf 'a\\nb'"], &Opts::default()).as_deref(), Some("a\nb"));
        assert_eq!(run(Some("/bin/sh"), &["-c", "echo x; exit 3"], &Opts::default()), None);
        assert_eq!(run(Some("/nonexistent/tool"), &[], &Opts::default()), None);
        assert_eq!(run(None, &[], &Opts::default()), None);
        let slow = Opts { timeout: Some(Duration::from_millis(100)), ..Default::default() };
        let t = Instant::now();
        assert_eq!(run(Some("/bin/sh"), &["-c", "sleep 5"], &slow), None);
        assert!(t.elapsed() < Duration::from_secs(3));
        let cwd = Opts { cwd: Some("/".into()), env: Some(vec![("X".into(), "1".into())]), ..Default::default() };
        assert_eq!(
            run(Some("/bin/sh"), &["-c", "pwd; echo $X; echo ${HOME:-none}"], &cwd).as_deref(),
            Some("/\n1\nnone\n")
        );
    }

    #[test]
    fn execs_programs() {
        let input = Opts { input: Some(b"in put".to_vec()), ..Default::default() };
        assert_eq!(
            sh("cat; echo err >&2; exit 4", &input),
            Exec { code: Some(4), out: "in put".into(), err: "err\n".into() }
        );
        let e = exec(Some("/nonexistent/tool"), &[], &Opts::default());
        assert_eq!((e.code, e.err.as_str()), (None, "spawn /nonexistent/tool ENOENT"));
        assert_eq!(exec(None, &[], &Opts::default()).err, "not found");
        // Killed at the timeout with its group: the sleep started by the shell goes too.
        let slow = Opts { timeout: Some(Duration::from_millis(200)), detached: true, ..Default::default() };
        let t = Instant::now();
        let e = sh("echo started; sleep 5; echo never", &slow);
        assert_eq!((e.code, e.out.as_str()), (None, "started\n"));
        assert!(t.elapsed() < Duration::from_secs(3));
        // Detached: a session of its own, without terminal.
        let alone = Opts { detached: true, ..Default::default() };
        let e = sh("ps -o sid= -p $$; ps -o pid= -p $$", &alone);
        let ids: Vec<&str> = e.out.split_whitespace().collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        let shell = Opts { shell: true, ..Default::default() };
        assert_eq!(exec(Some("echo"), &["a", "$((1+2))"], &shell).out, "a 3\n");
    }

    #[test]
    fn timeout_counts_while_a_grandchild_holds_the_outputs() {
        let slow = Opts { timeout: Some(Duration::from_millis(300)), ..Default::default() };
        let t = Instant::now();
        let e = sh("(sleep 3 &); exit 0", &slow);
        assert_eq!(e.code, None);
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn real_paths_walk_links_as_node() {
        let dir = std::env::temp_dir().join(format!("hx-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        let d = node_realpath(dir.to_str().unwrap()).unwrap();
        std::os::unix::fs::symlink(format!("{d}/a/b"), dir.join("l")).unwrap();
        std::os::unix::fs::symlink("l", dir.join("m")).unwrap();
        std::os::unix::fs::symlink("loop", dir.join("loop")).unwrap();
        assert_eq!(node_realpath(&format!("{d}/m")).unwrap(), format!("{d}/a/b"));
        // `..` is taken off by name first: l/.. is the folder holding l, not a.
        assert_eq!(node_realpath(&format!("{d}/l/..")).unwrap(), d);
        assert!(node_realpath(&format!("{d}/loop")).is_err());
        assert!(node_realpath(&format!("{d}/none")).is_err());
        assert_eq!(real_path(&format!("{d}/m/x/y")), format!("{d}/a/b/x/y"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn errno_names() {
        assert_eq!(errno_name(2), "ENOENT");
        assert_eq!(errno_name(7), "E2BIG");
    }

    #[test]
    fn words_split_like_the_js() {
        assert_eq!(words("code --wait"), ["code", "--wait"]);
        assert_eq!(words("subl -w \"a b\""), ["subl", "-w", "a b"]);
        assert_eq!(words("\"a\nb\" c"), ["\"a\nb\"", "c"]);
        assert_eq!(words("\u{a0}x\u{2003}y"), ["x", "y"]);
    }

    #[test]
    fn git_env_disables_what_the_repository_declares() {
        let Some(git) = git() else { return };
        let dir = std::env::temp_dir().join(format!("hx-gitenv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_str().unwrap();
        let g = |args: &[&str]| {
            let o = Opts { cwd: Some(d.into()), ..Default::default() };
            assert!(run(Some(git), args, &o).is_some(), "git {args:?}");
        };
        g(&["init", "-q"]);
        g(&["config", "filter.lfsx.clean", "evil"]);
        g(&["config", "filter.a=b.smudge", "evil"]);
        g(&["config", "hook.pre.command", "evil"]);
        g(&["config", "core.worktree", "/"]);
        let env = git_env(d);
        let count: usize = env_get(&env, "GIT_CONFIG_COUNT").unwrap().parse().unwrap();
        let pairs: Vec<(String, String)> = (0..count)
            .map(|n| {
                let k = env_get(&env, &format!("GIT_CONFIG_KEY_{n}")).unwrap().to_string();
                (k, env_get(&env, &format!("GIT_CONFIG_VALUE_{n}")).unwrap().to_string())
            })
            .collect();
        let has = |k: &str, v: &str| pairs.iter().any(|(a, b)| a == k && b == v);
        assert!(has("core.hooksPath", "/dev/null"));
        assert!(has("filter.lfsx.clean", "") && has("filter.lfsx.required", "false"));
        assert!(has("filter.a=b.smudge", ""));
        assert!(has("hook.pre.enabled", "false"));
        assert_eq!(env_get(&env, "GIT_NO_LAZY_FETCH"), Some("1"));
        assert_eq!(env_get(&env, "GIT_WORK_TREE"), Some(d));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
