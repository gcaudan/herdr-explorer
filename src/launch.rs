//! The launcher: the first process of an explorer or preview pane stays as a small launcher of the
//! real one, which runs as its child. Restarting on new code (`Screen::restart`), the child exits with code 75 and
//! the launcher starts it again in the same terminal, the same program: the path of this binary resolved once, at the
//! start, before an update renames it. Before each restart, the binary there is made the manifest's version
//! (`ready`): a git pull changes the manifest, and fetch-bin puts the binary of its version.
//!
//! The child is told by `EXPLORER_IPC_FILE`, the file where it writes its messages (`screen::send`), a line of JSON
//! each: `{restart}`, what the next child must show, added to its environment; `{cwd}`, the folder to announce after
//! Set as root. Started with that variable, this binary is the child; started without, it is the launcher and the
//! pane's first process.

use crate::core::exec::{self, Opts, WIN};
use crate::core::screen::RESTART;
use crate::core::{paths, update};
use crate::json::{self, Value};
use crate::sys;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

/// The launcher's exit code, once its last child ended; `None` in the child, which goes on to run the pane.
pub fn launch() -> Option<i32> {
    if let Some(ipc) = std::env::var_os("EXPLORER_IPC_FILE") {
        let launcher = std::env::var("EXPLORER_LAUNCHER").ok().and_then(|p| p.parse().ok());
        let lifeline = std::env::var("EXPLORER_LIFELINE").ok().and_then(|p| p.parse().ok());
        // SAFETY: at the start, before any thread: the processes this one starts are not the launcher's children.
        unsafe {
            std::env::remove_var("EXPLORER_IPC_FILE");
            std::env::remove_var("EXPLORER_LAUNCHER");
            std::env::remove_var("EXPLORER_LIFELINE");
        }
        let _ = crate::core::screen::IPC_FILE.set(ipc);
        watch_parent(launcher, lifeline);
        return None;
    }
    exec::home(None);
    let Ok(exe) = std::env::current_exe() else { return Some(1) };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    Some(launcher(&exe, &args))
}

/// The launcher gone (killed), the child goes too, instead of running on in a terminal that nobody shows. On Unix, the
/// launcher's end is the end of file of its lifeline (EXPLORER_LIFELINE), waited for without waking. On Windows, the
/// launcher's process is waited for, by the id it was given (EXPLORER_LAUNCHER); one that cannot be opened (gone
/// already, or with other rights) is left unwatched rather than ending a pane that may well have its launcher.
// The launcher's id serves on Windows, the lifeline on Unix.
#[allow(unused_variables)]
fn watch_parent(launcher: Option<u32>, lifeline: Option<i32>) {
    #[cfg(windows)]
    if let Some(pid) = launcher {
        std::thread::spawn(move || {
            if sys::wait_process(pid) {
                exec::exit_hooks();
                std::process::exit(0);
            }
        });
    }
    #[cfg(unix)]
    if let Some(fd) = lifeline.filter(|&fd| sys::adopt_lifeline(fd)) {
        std::thread::spawn(move || {
            if sys::wait_lifeline(fd) {
                exec::exit_hooks();
                std::process::exit(0);
            }
        });
    }
}

/// What the launcher waits for, from the threads that wait for it: the child's end, a message written, a signal.
enum Wake {
    Ended(Option<std::process::ExitStatus>),
    Message,
    #[cfg(unix)]
    Signal(sys::Signal),
}

/// Runs exe with args until it ends with another code than 75, and gives that code (1 when it was killed by a
/// signal, or could not start). The ending signals it gets are passed on to the child. Nothing is polled: a thread
/// waits for the child, the messages' folder is watched, the signals come from their own thread; this one sleeps
/// until one of them wakes it.
pub fn launcher(exe: &Path, args: &[OsString]) -> i32 {
    // Raw mode from one child to the next, never read here: in line mode, a ctrl+c typed while the next one starts
    // would kill the pane on Unix. Each child restores at exit the mode it found; this one when dropped.
    let _raw = if sys::stdin_is_tty() { sys::RawMode::enable().ok() } else { None };
    let (tx, rx) = mpsc::channel();
    #[cfg(unix)]
    if let Ok(mut events) = sys::Events::signals() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            loop {
                for e in events.wait(None) {
                    if let sys::Event::Signal(sig) = e
                        && tx.send(Wake::Signal(sig)).is_err()
                    {
                        return;
                    }
                }
            }
        });
    }
    // In a folder of its own, made new, only the user's on Unix: in a shared temporary folder, another user could
    // write the messages first (an environment for the next child).
    let Some(folder) = private_dir() else { return 1 };
    // Held until the last child ended: its read end, inherited by each child, tells it when this process is gone.
    #[cfg(unix)]
    let lifeline = sys::lifeline().ok();
    #[cfg(unix)]
    let lifeline_fd = lifeline.as_ref().map(|(r, _)| std::os::fd::AsRawFd::as_raw_fd(r).to_string());
    #[cfg(not(unix))]
    let lifeline_fd: Option<String> = None;
    let ipc = folder.join("ipc");
    // Without a watch (an unsupported system), the messages are read once a second, and at each child's end.
    let watcher = {
        let tx = tx.clone();
        sys::Watcher::new(&folder.to_string_lossy(), false, move |_| {
            let _ = tx.send(Wake::Message);
        })
        .ok()
    };
    let tick = if watcher.is_some() { None } else { Some(Duration::from_secs(1)) };
    let mut env: Vec<(String, String)> = Vec::new();
    let mut restart = false;
    let code = loop {
        if restart {
            ready(exe);
        }
        let mut seen = 0;
        let mut tries = 0;
        let started = loop {
            let _ = std::fs::write(&ipc, "");
            let started = Command::new(exe)
                .args(args)
                .envs(env.iter().cloned())
                .env("EXPLORER_IPC_FILE", &ipc)
                .env("EXPLORER_LAUNCHER", std::process::id().to_string())
                .envs(lifeline_fd.iter().map(|fd| ("EXPLORER_LIFELINE", fd)))
                .spawn();
            match started {
                // A restart can fall while the binary is being put (renamed into place, the old one moved away on
                // Windows): tried again a few times rather than closing the pane.
                Err(_) if restart && tries < 5 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_secs(1));
                    ready(exe);
                }
                started => break started,
            }
        };
        let Ok(mut child) = started else { break 1 };
        #[cfg(unix)]
        let pid = child.id();
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Wake::Ended(child.wait().ok()));
            });
        }
        let status = loop {
            let wake = match tick {
                None => rx.recv().ok(),
                Some(t) => match rx.recv_timeout(t) {
                    Err(mpsc::RecvTimeoutError::Timeout) => Some(Wake::Message),
                    r => r.ok(),
                },
            };
            match wake {
                Some(Wake::Ended(status)) => break status,
                Some(Wake::Message) => messages(&ipc, &mut seen, &mut env),
                #[cfg(unix)]
                Some(Wake::Signal(sig)) => sys::send_signal(pid, sig),
                // Never: this function keeps a sender.
                None => break None,
            }
        };
        messages(&ipc, &mut seen, &mut env);
        match status.and_then(|s| s.code()) {
            Some(RESTART) => restart = true,
            Some(code) => break code,
            None => break 1,
        }
    };
    drop(watcher);
    #[cfg(unix)]
    drop(lifeline);
    let _ = std::fs::remove_dir_all(&folder);
    code
}

/// The binary at exe of the manifest's version, else fetched (scripts/fetch-bin): fetch-bin
/// waits for bin/.fetch.lock, held by `herdr plugin update`'s build or another fetch until the binary is put,
/// then fetches it only when still missing. Never over a development binary (bin/.dev). On failure (no network), the
/// binary there starts all the same: a pane on the old code rather than a closed one.
fn ready(exe: &Path) {
    let Some(dir) = update::dir() else { return };
    if std::fs::metadata(paths::join(&[dir, "bin", ".dev"])).is_ok() {
        return;
    }
    let want = update::read_version();
    let opts = |secs| Opts { timeout: Some(Duration::from_secs(secs)), ..Opts::default() };
    let has = exec::run(Some(&exe.to_string_lossy()), &["--version"], &opts(10));
    if want.is_none() || has.as_deref().map(str::trim) == want.as_deref() {
        return;
    }
    // By absolute paths: the shell and PowerShell of the system, never one found in a folder of PATH. Longer than a
    // fetch takes: the wait for another one's lock, then the download.
    if WIN {
        let root = exec::env_var("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let ps = paths::join(&[&root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"]);
        let script = paths::join(&[dir, "scripts", "fetch-bin.ps1"]);
        let args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script, dir];
        let _ = exec::run(Some(&ps), &args, &opts(600));
    } else {
        let script = paths::join(&[dir, "scripts", "fetch-bin.sh"]);
        let _ = exec::run(Some("/bin/sh"), &[&script, dir], &opts(600));
    }
}

/// A new folder in the temporary one, mode 0700 on Unix; None when it cannot be made.
fn private_dir() -> Option<std::path::PathBuf> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    for n in 0..100u32 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!("herdr-explorer-{}-{nanos:x}{n}", std::process::id()));
        // Fails when it exists, whoever made it.
        if builder.create(&dir).is_ok() {
            return Some(dir);
        }
    }
    None
}

/// The child's messages written since seen: what the next child must show, added to env; the folder to announce.
fn messages(ipc: &Path, seen: &mut usize, env: &mut Vec<(String, String)>) {
    let Ok(text) = std::fs::read(ipc) else { return };
    let Some(new) = text.get(*seen..) else { return };
    // Whole lines only: the rest is still being written.
    let Some(end) = new.iter().rposition(|&b| b == b'\n') else { return };
    let lines = String::from_utf8_lossy(&new[..end]).into_owned();
    *seen += end + 1;
    for line in lines.lines() {
        let Ok(m) = json::parse(line) else { continue };
        if let Some(restart) = m.get("restart").filter(|r| r.truthy()) {
            for (k, v) in exec::spread(restart) {
                let v = crate::json::text(&v);
                match env.iter_mut().find(|(name, _)| *name == k) {
                    Some(slot) => slot.1 = v,
                    None => env.push((k, v)),
                }
            }
        }
        if let Some(Value::Str(cwd)) = m.get("cwd").filter(|c| c.truthy()) {
            exec::home(Some(cwd));
        }
    }
}
