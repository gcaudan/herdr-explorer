//! A folder watched as `fs.watch` watches it: each change in it calls back with the name changed, relative to the
//! folder and with the system's separator, as Node gives `filename` (None when the system lost track, an overflow).
//! `recursive` watches the whole tree. The callback runs on a thread of the watcher's own; the watch ends when the
//! `Watcher` is dropped. Linux: inotify, one watch per folder for a recursive one, as Node does, which fails when a
//! folder of the tree cannot be read. macOS: FSEvents, as libuv. Windows: ReadDirectoryChangesW, as libuv.

use std::io;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// What a change calls: the name changed, relative to the folder watched.
pub type Callback = Box<dyn FnMut(Option<String>) + Send + 'static>;

/// A folder watched; the watch ends when dropped.
pub struct Watcher(#[allow(dead_code)] imp::Watcher);

impl Watcher {
    /// Watches dir (and its whole tree when recursive); fails as `fs.watch` throws: a missing or unreadable folder.
    pub fn new(dir: &str, recursive: bool, on: impl FnMut(Option<String>) + Send + 'static) -> io::Result<Watcher> {
        imp::Watcher::new(dir, recursive, Box::new(on)).map(Watcher)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub struct Watcher;

    impl Watcher {
        pub fn new(_: &str, _: bool, _: super::Callback) -> std::io::Result<Watcher> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }
    }
}
