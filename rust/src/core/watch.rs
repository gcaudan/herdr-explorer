//! A file watched by polling its status, as `fs.watchFile` does: changed once its times, size, mode or identity
//! differ from the last look, or it appears or goes. The pane's loop calls `poll` (or `due` to time it).

use std::time::{Duration, Instant, SystemTime};

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

#[derive(Debug)]
pub struct Watch {
    file: String,
    interval: Duration,
    last: Option<Stat>,
    next: Instant,
}

impl Watch {
    pub fn new(file: &str, interval: Duration) -> Watch {
        Watch { file: file.to_string(), interval, last: stat(file), next: Instant::now() + interval }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    /// When the next look is due.
    pub fn due(&self) -> Instant {
        self.next
    }

    /// Looks at the file when its time has come: true when it changed since the last look.
    pub fn poll(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next {
            return false;
        }
        self.next = now + self.interval;
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
        assert!(!w.poll());
        std::fs::write(&f, "a").unwrap();
        assert!(w.poll());
        assert!(!w.poll());
        std::fs::write(&f, "bb").unwrap();
        assert!(w.poll());
        std::fs::remove_file(&f).unwrap();
        assert!(w.poll());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
