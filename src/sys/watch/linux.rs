//! inotify. A recursive watch adds one watch per folder of the tree, and one more for each
//! folder made or moved in later; a folder of the tree that cannot be read fails the whole watch.

use super::Callback;
use std::collections::HashMap;
use std::ffi::{CString, c_char};
use std::io;
use std::os::raw::{c_int, c_void};
use std::thread::JoinHandle;

const IN_MODIFY: u32 = 0x2;
const IN_ATTRIB: u32 = 0x4;
const IN_MOVED_FROM: u32 = 0x40;
const IN_MOVED_TO: u32 = 0x80;
const IN_CREATE: u32 = 0x100;
const IN_DELETE: u32 = 0x200;
const IN_DELETE_SELF: u32 = 0x400;
const IN_MOVE_SELF: u32 = 0x800;
const IN_Q_OVERFLOW: u32 = 0x4000;
const IN_IGNORED: u32 = 0x8000;
const IN_ONLYDIR: u32 = 0x0100_0000;
const IN_ISDIR: u32 = 0x4000_0000;
const IN_NONBLOCK: c_int = 0o4000;
const IN_CLOEXEC: c_int = 0o2000000;
const O_CLOEXEC: c_int = 0o2000000;
const POLLIN: i16 = 1;
const EINTR: i32 = 4;
const ENOENT: i32 = 2;
const ENOTDIR: i32 = 20;
/// What libuv asks inotify for.
const MASK: u32 =
    IN_ATTRIB | IN_CREATE | IN_MODIFY | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF | IN_MOVED_FROM | IN_MOVED_TO;

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn inotify_init1(flags: c_int) -> c_int;
    fn inotify_add_watch(fd: c_int, path: *const c_char, mask: u32) -> c_int;
    fn inotify_rm_watch(fd: c_int, wd: c_int) -> c_int;
    fn pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    fn poll(fds: *mut PollFd, n: std::os::raw::c_ulong, ms: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    fn close(fd: c_int) -> c_int;
}

/// The folders watched: inotify's watch descriptor → the folder's path relative to the root ("" for the root).
struct Tree {
    fd: c_int,
    root: String,
    dirs: HashMap<c_int, String>,
}

impl Tree {
    /// Watches the folder rel; with recursive, the folders under it too. Fails at the first folder that cannot be
    /// watched or read; a folder under it gone meanwhile (a build cache) is left out.
    fn add(&mut self, rel: &str, recursive: bool) -> io::Result<()> {
        let full = if rel.is_empty() { self.root.clone() } else { format!("{}/{rel}", self.root) };
        let path = CString::new(full.clone()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mask = if rel.is_empty() { MASK } else { MASK | IN_ONLYDIR };
        // SAFETY: a C string that outlives the call.
        let wd = unsafe { inotify_add_watch(self.fd, path.as_ptr(), mask) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        self.dirs.insert(wd, rel.to_string());
        if !recursive {
            return Ok(());
        }
        let gone = |e: &io::Error| matches!(e.raw_os_error(), Some(ENOENT | ENOTDIR));
        let list = match std::fs::read_dir(&full) {
            Err(e) if !rel.is_empty() && gone(&e) => return Ok(()),
            list => list?,
        };
        for e in list {
            let e = match e {
                Err(e) if gone(&e) => continue,
                e => e?,
            };
            // Links are not followed: a link to a folder above would watch forever.
            let dir = match e.file_type() {
                Err(e) if gone(&e) => continue,
                kind => kind?.is_dir(),
            };
            if dir {
                let name = e.file_name().to_string_lossy().into_owned();
                match self.add(&if rel.is_empty() { name } else { format!("{rel}/{name}") }, true) {
                    Err(e) if gone(&e) => {}
                    done => done?,
                }
            }
        }
        Ok(())
    }

    /// A folder moved out of its place: its watches and those under it go, as its changes no longer belong here (a
    /// folder moved within the tree is watched again under its new name).
    fn forget(&mut self, rel: &str) {
        let under = format!("{rel}/");
        let wds: Vec<c_int> =
            self.dirs.iter().filter(|(_, d)| *d == rel || d.starts_with(&under)).map(|(&wd, _)| wd).collect();
        for wd in wds {
            self.dirs.remove(&wd);
            // SAFETY: a watch of this descriptor; its IN_IGNORED then finds nothing to remove.
            unsafe { inotify_rm_watch(self.fd, wd) };
        }
    }
}

pub struct Watcher {
    stop: c_int,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn new(dir: &str, recursive: bool, on: Callback, lost: super::Lost) -> io::Result<Watcher> {
        // SAFETY: a new descriptor, closed by the thread or below.
        let fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut tree = Tree { fd, root: dir.trim_end_matches('/').to_string(), dirs: HashMap::new() };
        if tree.root.is_empty() {
            tree.root = "/".into();
        }
        let mut pipe = [0; 2];
        // SAFETY: pipe2 writes two descriptors.
        let made = tree.add("", recursive).and_then(|_| match unsafe { pipe2(pipe.as_mut_ptr(), O_CLOEXEC) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        });
        if let Err(e) = made {
            // SAFETY: the descriptor made above, closed once.
            unsafe { close(fd) };
            return Err(e);
        }
        let thread = std::thread::spawn(move || run(tree, recursive, pipe[0], on, lost));
        Ok(Watcher { stop: pipe[1], thread: Some(thread) })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let b = 0u8;
        // SAFETY: a one-byte write to the pipe the thread polls, then its end closed.
        unsafe {
            write(self.stop, &b as *const u8 as *const c_void, 1);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: closed once, after the thread is done.
        unsafe { close(self.stop) };
    }
}

/// The watcher's thread: the changes read from inotify until the pipe says stop. The root's watch ended by inotify
/// (the folder removed, or moved away: inotify follows it elsewhere) is lost.
fn run(mut tree: Tree, recursive: bool, stop: c_int, mut on: Callback, lost: super::Lost) {
    let mut buf = vec![0u8; 65536];
    loop {
        let mut fds =
            [PollFd { fd: tree.fd, events: POLLIN, revents: 0 }, PollFd { fd: stop, events: POLLIN, revents: 0 }];
        // SAFETY: poll reads and writes the two structs.
        if unsafe { poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(EINTR) {
                continue;
            }
            break;
        }
        if fds[1].revents != 0 {
            break;
        }
        // SAFETY: read writes at most buf.len() bytes.
        let got = unsafe { read(tree.fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if got <= 0 {
            continue;
        }
        let mut at = 0;
        while at + 16 <= got as usize {
            let word = |i: usize| u32::from_ne_bytes(buf[at + i..at + i + 4].try_into().unwrap());
            let (wd, mask, len) = (word(0) as c_int, word(4), word(12) as usize);
            let raw = &buf[at + 16..(at + 16 + len).min(got as usize)];
            at += 16 + len;
            if mask & IN_Q_OVERFLOW != 0 {
                on(None);
                continue;
            }
            if mask & IN_IGNORED != 0 {
                if tree.dirs.remove(&wd).is_some_and(|d| d.is_empty()) {
                    lost.store(true, std::sync::atomic::Ordering::SeqCst);
                    on(None);
                }
                continue;
            }
            let Some(dir) = tree.dirs.get(&wd).cloned() else { continue };
            if dir.is_empty() && mask & IN_MOVE_SELF != 0 && !lost.swap(true, std::sync::atomic::Ordering::SeqCst) {
                on(None);
            }
            let name = String::from_utf8_lossy(raw.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
            let rel = match (dir.is_empty(), name.is_empty()) {
                (true, false) => name,
                (false, false) => format!("{dir}/{name}"),
                // The folder itself: the root by its name, as libuv gives it.
                (true, true) => tree.root.rsplit('/').next().unwrap_or("").to_string(),
                (false, true) => dir,
            };
            if recursive && mask & IN_ISDIR != 0 && mask & IN_MOVED_FROM != 0 {
                tree.forget(&rel);
            }
            if recursive && mask & IN_ISDIR != 0 && mask & (IN_CREATE | IN_MOVED_TO) != 0 {
                // A folder made or moved in: watched too, as far as it can be read.
                let _ = tree.add(&rel, true);
            }
            on(Some(rel));
        }
    }
    // SAFETY: the descriptors this thread owns, closed once.
    unsafe {
        close(tree.fd);
        close(stop);
    }
}
