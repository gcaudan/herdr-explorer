//! A file watched: changed once its times, size, mode or identity differ from the last look, or it appears or goes.
//! Its folder is watched (`sys::Watcher`): a change of the file's name there wakes
//! the pane's loop (`wake_with`), which then calls `poll`; nothing is looked at in between. Without a watch (the
//! folder missing, or an unsupported system), the file's status is polled every interval, the watch tried again each
//! time. The pane's loop calls `poll`, and `due` to time it.

use crate::sys;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// What wakes the pane's loop when a watched file changes: its screen's waker, given once it is open.
static WAKER: OnceLock<sys::Waker> = OnceLock::new();

/// The watches wake the loop with waker from now on.
pub fn wake_with(waker: sys::Waker) {
    let _ = WAKER.set(waker);
}

/// What a file's status says, compared from one look to the next (libuv's statbuf_eq); None when it is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stat {
    modified: Option<SystemTime>,
    changed: (i64, i64),
    size: u64,
    mode: u32,
    ino: u64,
    dev: u64,
}

fn stat(file: &str) -> Option<Stat> {
    let m = std::fs::metadata(file).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(Stat {
            modified: m.modified().ok(),
            changed: (m.ctime(), m.ctime_nsec()),
            size: m.len(),
            mode: m.mode(),
            ino: m.ino(),
            dev: m.dev(),
        })
    }
    #[cfg(not(unix))]
    {
        let created = m.created().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok());
        Some(Stat {
            modified: m.modified().ok(),
            changed: created.map_or((0, 0), |d| (d.as_secs() as i64, d.subsec_nanos() as i64)),
            size: m.len(),
            mode: m.permissions().readonly() as u32,
            ino: 0,
            dev: 0,
        })
    }
}

pub struct Watch {
    file: String,
    interval: Duration,
    last: Option<Stat>,
    next: Instant,
    /// Set by the folder's watch when the file's name changes there, with that watch; None while the file is polled.
    seen: Option<(Arc<AtomicBool>, sys::Watcher)>,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch").field("file", &self.file).field("watched", &self.seen.is_some()).finish()
    }
}

/// The file's folder watched for its name, None when it cannot be. A lost track (None for a name) counts as a change.
fn watch(file: &str) -> Option<(Arc<AtomicBool>, sys::Watcher)> {
    let path = std::path::Path::new(file);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty())?;
    let flag = Arc::new(AtomicBool::new(false));
    let set = flag.clone();
    let watcher = sys::Watcher::new(&dir.to_string_lossy(), false, move |changed| {
        if changed.is_none_or(|n| n == name) {
            set.store(true, Ordering::SeqCst);
            if let Some(w) = WAKER.get() {
                w.wake();
            }
        }
    })
    .ok()?;
    Some((flag, watcher))
}

impl Watch {
    pub fn new(file: &str, interval: Duration) -> Watch {
        // The folder watched before the first look: a change between the two is seen.
        let seen = watch(file);
        Watch { file: file.to_string(), interval, last: stat(file), next: Instant::now() + interval, seen }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    /// When the next look is due: now once the folder's watch saw the file change, never before; the next poll
    /// without a watch.
    pub fn due(&self) -> Option<Instant> {
        match &self.seen {
            Some((flag, _)) => flag.load(Ordering::SeqCst).then(Instant::now),
            None => Some(self.next),
        }
    }

    /// Looks at the file when its time has come: true when it changed since the last look.
    pub fn poll(&mut self) -> bool {
        match &self.seen {
            Some((flag, _)) => {
                if !flag.swap(false, Ordering::SeqCst) {
                    return false;
                }
            }
            None => {
                let now = Instant::now();
                if now < self.next {
                    return false;
                }
                self.next = now + self.interval;
                self.seen = watch(&self.file);
            }
        }
        let s = stat(&self.file);
        let changed = s != self.last;
        self.last = s;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_changes() {
        let dir = std::env::temp_dir().join(format!("hx-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("f");
        let mut w = Watch::new(f.to_str().unwrap(), Duration::ZERO);
        // The folder's watch tells from its own thread: waited for, up to 2 s.
        let saw = |w: &mut Watch| {
            (0..200).any(|_| {
                std::thread::sleep(Duration::from_millis(10));
                w.poll()
            })
        };
        assert!(!w.poll());
        std::fs::write(&f, "a").unwrap();
        assert!(saw(&mut w));
        assert!(!w.poll());
        std::fs::write(&f, "bb").unwrap();
        assert!(saw(&mut w));
        std::fs::remove_file(&f).unwrap();
        assert!(saw(&mut w));
        // Another file of the folder: no look due.
        std::fs::write(dir.join("other"), "x").unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(w.due().is_none_or(|_| !w.poll()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
