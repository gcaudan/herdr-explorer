//! The launcher (src/launch.js): the first process of an explorer or preview pane stays as a small launcher of the
//! real one, which runs as its child. Restarting on new code (`Screen::restart`), the child exits with code 75 and
//! the launcher starts it again in the same terminal, the same program: the path of this binary resolved once, at the
//! start, before an update renames it.
//!
//! The child is told by `EXPLORER_IPC_FILE`, the file where it writes its messages (`screen::send`), a line of JSON
//! each: `{restart}`, what the next child must show, added to its environment; `{cwd}`, the folder to announce after
//! Set as root. Started with that variable by another launcher (the node shim, or the old launch.js), this binary is
//! the child; started without, it is the launcher and the pane's first process.

use crate::core::exec;
use crate::core::screen::RESTART;
use crate::json::{self, Value};
use crate::sys;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Herdr starts plugin panes in the plugin's folder and reads a pane's folder from its first process: in the space
/// folder, a new space, tab or pane made from this one starts there instead of in the plugin's checkout. Done by the
/// first process of every pane; `None` is EXPLORER_ROOT.
pub fn home(dir: Option<&str>) {
    let dir = dir.map(String::from).or_else(|| exec::env_var("EXPLORER_ROOT"));
    if let Some(dir) = dir.filter(|d| !d.is_empty()) {
        let _ = std::env::set_current_dir(dir);
    }
}

/// The launcher's exit code, once its last child ended; `None` in the child, which goes on to run the pane.
pub fn launch() -> Option<i32> {
    if let Some(ipc) = std::env::var_os("EXPLORER_IPC_FILE") {
        let launcher = std::env::var("EXPLORER_LAUNCHER").ok().and_then(|p| p.parse().ok());
        // SAFETY: at the start, before any thread: the processes this one starts are not the launcher's children
        // (Node keeps its channel to itself as well).
        unsafe {
            std::env::remove_var("EXPLORER_IPC_FILE");
            std::env::remove_var("EXPLORER_LAUNCHER");
        }
        let _ = crate::core::screen::IPC_FILE.set(ipc);
        watch_parent(launcher);
        return None;
    }
    home(None);
    let Ok(exe) = std::env::current_exe() else { return Some(1) };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    Some(launcher(&exe, &args))
}

/// The launcher gone (killed), the child goes too, instead of running on in a terminal that nobody shows. On Unix, the
/// parent's end is seen by the parent's id changing, from the launcher's id it was given (EXPLORER_LAUNCHER), as it
/// may be gone already, else from the parent it has at the start (the node shim). On Windows, the launcher's process
/// is waited for, by the id it was given; one that cannot be opened (gone already, or with other rights) is left
/// unwatched rather than ending a pane that may well have its launcher.
fn watch_parent(launcher: Option<u32>) {
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
    {
        let parent = launcher.unwrap_or_else(sys::getppid);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if sys::getppid() != parent {
                    exec::exit_hooks();
                    std::process::exit(0);
                }
            }
        });
    }
}

/// Runs exe with args until it ends with another code than 75, and gives that code (1 when it was killed by a
/// signal, or could not start). The ending signals it gets are passed on to the child.
pub fn launcher(exe: &Path, args: &[OsString]) -> i32 {
    // Raw mode from one child to the next, never read here: in line mode, a ctrl+c typed while the next one starts
    // would kill the pane on Unix. Each child restores at exit the mode it found; this one when dropped.
    let _raw = if sys::stdin_is_tty() { sys::RawMode::enable().ok() } else { None };
    #[cfg(unix)]
    let mut events = sys::Events::signals().ok();
    // In a folder of its own, made new, only the user's on Unix: in a shared temporary folder, another user could
    // write the messages first (an environment for the next child).
    let Some(folder) = private_dir() else { return 1 };
    let ipc = folder.join("ipc");
    let mut env: Vec<(String, String)> = Vec::new();
    let code = loop {
        let _ = std::fs::write(&ipc, "");
        let mut seen = 0;
        let started = Command::new(exe)
            .args(args)
            .envs(env.iter().cloned())
            .env("EXPLORER_IPC_FILE", &ipc)
            .env("EXPLORER_LAUNCHER", std::process::id().to_string())
            .spawn();
        let Ok(mut child) = started else { break 1 };
        let status = loop {
            messages(&ipc, &mut seen, &mut env);
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(_) => break None,
            }
            #[cfg(unix)]
            if let Some(events) = events.as_mut() {
                for e in events.wait(Some(Duration::from_millis(100))) {
                    if let sys::Event::Signal(sig) = e {
                        sys::send_signal(child.id(), sig);
                    }
                }
                continue;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        messages(&ipc, &mut seen, &mut env);
        match status.and_then(|s| s.code()) {
            Some(RESTART) => continue,
            Some(code) => break code,
            None => break 1,
        }
    };
    let _ = std::fs::remove_dir_all(&folder);
    code
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
                let v = crate::core::js::string(&v);
                match env.iter_mut().find(|(name, _)| *name == k) {
                    Some(slot) => slot.1 = v,
                    None => env.push((k, v)),
                }
            }
        }
        if let Some(Value::Str(cwd)) = m.get("cwd").filter(|c| c.truthy()) {
            home(Some(cwd));
        }
    }
}
