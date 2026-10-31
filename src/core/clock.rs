//! The time as the state files keep it: milliseconds since the epoch, with the fraction.

use std::time::{SystemTime, UNIX_EPOCH};

/// Now, in milliseconds since the epoch.
pub fn now_ms() -> f64 {
    ms(SystemTime::now())
}

/// A time (a file's mtime) in milliseconds since the epoch, negative before it.
pub fn ms(t: SystemTime) -> f64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64() * 1e3,
        Err(e) => -e.duration().as_secs_f64() * 1e3,
    }
}
