//! The terminal and the process, as Node gave them to the JS: raw mode (libuv's), the terminal's size, input read
//! without blocking, resizes and signals, all waited for in one place (`Events::wait`), which other threads can wake
//! (`Waker`). Hand-written FFI: `unix.rs` (Linux and macOS, termios declared per target) and `windows.rs` (console).

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(unix)]
pub use unix::{
    SIGHUP, SIGINT, SIGTERM, executable, getppid, getuid, hostname, kill_group, send_signal, setsid, terminate,
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;
#[cfg(windows)]
pub use windows::{file_id, hostname, links};

#[cfg(target_os = "linux")]
pub use unix::pty;

mod watch;
pub use watch::Watcher;

use std::io;
use std::time::Duration;

/// A signal that ends a pane: the JS listens to SIGTERM, SIGHUP and SIGINT (on Windows, the console's close and
/// ctrl+c / ctrl+break events, as libuv maps them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Hup,
    Int,
}

/// What `Events::wait` saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Bytes read from the terminal, as they came (a sequence may be cut between two reads).
    Input(Vec<u8>),
    /// The input ended (the terminal closed): reported once, then no longer read.
    Eof,
    /// The terminal's size changed.
    Resize,
    Signal(Signal),
    /// A `Waker` was called.
    Wake,
}

/// A file's identity, its device and inode as lstat gives them (the link itself, not followed), or None when it
/// cannot be read. On Windows, the volume's serial number and the file's index, as libuv's stat.
#[cfg(unix)]
pub fn file_id(path: &str) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Whether stdin is a terminal (`process.stdin.isTTY`).
pub fn stdin_is_tty() -> bool {
    imp::stdin_is_tty()
}

/// Whether stdout is a terminal (`process.stdout.isTTY`).
pub fn stdout_is_tty() -> bool {
    imp::stdout_is_tty()
}

/// The terminal's size, columns and rows, from stdout; `None` when it is not a terminal (the JS then takes 80x24).
pub fn size() -> Option<(u16, u16)> {
    imp::size()
}

/// The JS's `ui.size()`: the terminal's size, 80 columns and 24 rows when unknown.
pub fn size_or_default() -> (u16, u16) {
    match size() {
        Some((w, h)) => (if w == 0 { 80 } else { w }, if h == 0 { 24 } else { h }),
        None => (80, 24),
    }
}

/// The terminal in raw mode, as `process.stdin.setRawMode(true)` sets it; the mode it found is restored when dropped
/// (Node restores it at exit).
pub struct RawMode(imp::Saved);

impl RawMode {
    /// Raw mode on stdin; fails when it is not a terminal.
    pub fn enable() -> io::Result<RawMode> {
        imp::raw_mode().map(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        imp::restore(&self.0);
    }
}

/// Wakes `Events::wait` from another thread: a child process done, a file changed.
#[derive(Clone)]
pub struct Waker(imp::Waker);

impl Waker {
    pub fn wake(&self) {
        self.0.wake();
    }
}

/// The input, resizes, signals and wakes of the process, waited for together. One per process: signals are caught
/// from the first `Events::new` on.
pub struct Events(imp::Events);

impl Events {
    /// Starts catching resizes and the ending signals, and reading stdin.
    pub fn new() -> io::Result<Events> {
        imp::Events::new().map(Events)
    }

    /// Catches the ending signals and resizes only, stdin left alone (the launcher's: its child reads the terminal).
    #[cfg(unix)]
    pub fn signals() -> io::Result<Events> {
        imp::Events::on(-1).map(Events)
    }

    /// The events seen, waiting up to `timeout` (forever with `None`) for the first; an empty list once it elapsed.
    pub fn wait(&mut self, timeout: Option<Duration>) -> Vec<Event> {
        self.0.wait(timeout)
    }

    pub fn waker(&self) -> Waker {
        Waker(self.0.waker())
    }
}

/// Writes all of `bytes` to stdout, unbuffered: a frame goes out whole, at once.
pub fn write_stdout(bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut out = io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()
}
