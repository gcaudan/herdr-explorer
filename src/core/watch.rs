//! A file watched: changed once its times, size, mode or identity differ from the last look, or it appears or goes.
//! Its folder is watched (`sys::Watcher`, one per folder for the whole process, shared by the files watched there):
//! a change of the file's name there wakes the pane's loop (`wake_with`), which then calls `poll`. A folder's watch
//! lost (the system tells: the folder removed or moved away) is dropped from the folders shared, and made again by the
//! first of its files to look; every 30 s, on a grid that all the watches share, the folder's identity is looked at
//! too, for a loss the system did not tell. Without a watch (the folder missing, or an unsupported system), the file's status
//! is polled every interval, the watch tried again each time. The pane's loop calls `poll`, and `due` to time it.

use crate::sys;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime};

/// The look at a watched file's folder, in case its watch went with the folder.
const CHECK: Duration = Duration::from_secs(30);

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

/// What tells a folder apart from one made again at its path: its inode (its creation time on Windows).
type Ident = (u64, u64, Option<SystemTime>);

fn ident(dir: &str) -> Option<Ident> {
    let m = std::fs::metadata(dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((m.ino(), m.dev(), None))
    }
    #[cfg(not(unix))]
    Some((0, 0, m.created().ok()))
}

/// The names watched in a folder, each with the flag it sets, gone with its Watch.
type Names = Arc<Mutex<Vec<(String, Weak<AtomicBool>)>>>;

/// A folder's watch, shared by the files watched there: the folder it watches (its identity then), and whether it was
/// found gone.
struct Shared {
    watcher: Mutex<sys::Watcher>,
    names: Names,
    ident: Option<Ident>,
    dead: AtomicBool,
}

impl Shared {
    /// Whether it still watches the folder at its path: the system watches it still, and the folder there is the
    /// same; marked dead otherwise, for every file watched there.
    fn live(&self, now: &Option<Ident>) -> bool {
        let alive = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).alive();
        if !alive || *now != self.ident {
            self.dead.store(true, Ordering::SeqCst);
        }
        !self.dead.load(Ordering::SeqCst)
    }
}

/// The folders watched, by path: a watch goes with its last file.
static FOLDERS: Mutex<Option<HashMap<String, Weak<Shared>>>> = Mutex::new(None);

/// A file's part in its folder's watch: its flag, and the watch kept alive.
type Seen = (Arc<AtomicBool>, Arc<Shared>);

pub struct Watch {
    file: String,
    interval: Duration,
    last: Option<Stat>,
    next: Instant,
    /// Set by the folder's watch when the file's name changes there, with that watch; None while the file is polled.
    seen: Option<Seen>,
    /// The next look at the folder (CHECK).
    check: Instant,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch").field("file", &self.file).field("watched", &self.seen.is_some()).finish()
    }
}

fn dir_of(file: &str) -> Option<String> {
    let dir = std::path::Path::new(file).parent().filter(|d| !d.as_os_str().is_empty())?;
    Some(dir.to_string_lossy().into_owned())
}

/// The file's folder watched for its name, None when it cannot be: the folder's shared watch while it is live, else a
/// new one in its place. A lost track (None for a name) counts as a change.
fn watch(file: &str) -> Option<Seen> {
    let name = std::path::Path::new(file).file_name()?.to_string_lossy().into_owned();
    let dir = dir_of(file)?;
    let now = ident(&dir);
    let mut folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner());
    let folders = folders.get_or_insert_with(HashMap::new);
    folders.retain(|_, w| w.strong_count() > 0);
    let shared = match folders.get(&dir).and_then(Weak::upgrade).filter(|s| s.live(&now)) {
        Some(s) => s,
        None => {
            let names: Names = Arc::default();
            let told = names.clone();
            let watcher = sys::Watcher::new(&dir, false, move |changed| {
                let mut names = told.lock().unwrap_or_else(|e| e.into_inner());
                names.retain(|(_, f)| f.strong_count() > 0);
                let mut hit = false;
                for (n, flag) in names.iter() {
                    if changed.as_deref().is_none_or(|c| c == n)
                        && let Some(flag) = flag.upgrade()
                    {
                        flag.store(true, Ordering::SeqCst);
                        hit = true;
                    }
                }
                if hit && let Some(w) = WAKER.get() {
                    w.wake();
                }
            })
            .ok()?;
            let s = Arc::new(Shared { watcher: Mutex::new(watcher), names, ident: now, dead: AtomicBool::new(false) });
            folders.insert(dir, Arc::downgrade(&s));
            s
        }
    };
    let flag = Arc::new(AtomicBool::new(false));
    shared.names.lock().unwrap_or_else(|e| e.into_inner()).push((name, Arc::downgrade(&flag)));
    Some((flag, shared))
}

/// The next look of the 30 s grid, the same for every watch of the process: one wake for all of them.
fn next_check() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    let n = start.elapsed().as_secs() / CHECK.as_secs() + 1;
    start + CHECK * n as u32
}

impl Watch {
    pub fn new(file: &str, interval: Duration) -> Watch {
        // The folder watched before the first look: a change between the two is seen.
        let seen = watch(file);
        let next = Instant::now() + interval;
        Watch { file: file.to_string(), interval, last: stat(file), next, seen, check: next_check() }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    /// When the next look is due: now once the folder's watch saw the file change, else the look at the folder;
    /// the next poll without a watch.
    pub fn due(&self) -> Option<Instant> {
        match &self.seen {
            Some((flag, _)) => Some(if flag.load(Ordering::SeqCst) { Instant::now() } else { self.check }),
            None => Some(self.next),
        }
    }

    /// Looks at the file when its time has come: true when it changed since the last look.
    pub fn poll(&mut self) -> bool {
        match &self.seen {
            Some((flag, shared)) => {
                let told = flag.swap(false, Ordering::SeqCst);
                let look = !told && Instant::now() >= self.check;
                if look {
                    self.check = next_check();
                }
                // The folder's watch lost (as told, or as its identity says at the look): watched again, the file
                // looked at; else nothing to look at but what the watch told.
                let lost = (told || look) && !shared.live(&dir_of(&self.file).and_then(|d| ident(&d)));
                if lost {
                    self.seen = watch(&self.file);
                } else if !told {
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
        assert!(!w.poll());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_files_of_a_folder_share_its_watch() {
        let dir = std::env::temp_dir().join(format!("hx-watch-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        let mut wa = Watch::new(a.to_str().unwrap(), Duration::ZERO);
        let mut wb = Watch::new(b.to_str().unwrap(), Duration::ZERO);
        let (sa, sb) = (&wa.seen.as_ref().unwrap().1, &wb.seen.as_ref().unwrap().1);
        assert!(Arc::ptr_eq(sa, sb));
        let saw = |w: &mut Watch| {
            (0..200).any(|_| {
                std::thread::sleep(Duration::from_millis(10));
                w.poll()
            })
        };
        std::fs::write(&b, "x").unwrap();
        assert!(saw(&mut wb));
        assert!(!wa.poll());
        // The first gone, the watch serves the second still.
        drop(wa);
        std::fs::write(&b, "yy").unwrap();
        assert!(saw(&mut wb));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_made_again_is_watched_again_once() {
        let dir = std::env::temp_dir().join(format!("hx-watch-again-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, "a").unwrap();
        let mut wa = Watch::new(a.to_str().unwrap(), Duration::ZERO);
        let saw = |w: &mut Watch| {
            (0..200).any(|_| {
                std::thread::sleep(Duration::from_millis(10));
                w.poll()
            })
        };
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(saw(&mut wa));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&a, "again").unwrap();
        assert!(saw(&mut wa));
        // Watched again by the folder's new watch, which a file watched from now on shares.
        let wb = Watch::new(b.to_str().unwrap(), Duration::ZERO);
        let (sa, sb) = (&wa.seen.as_ref().unwrap().1, &wb.seen.as_ref().unwrap().1);
        assert!(Arc::ptr_eq(sa, sb));
        std::fs::write(&a, "and again").unwrap();
        assert!(saw(&mut wa));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
