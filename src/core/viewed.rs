//! Files marked viewed in the Changes pane of a tab (m), until they change: viewed-<tab id>.json in the state folder,
//! { real path: signature of the content when marked }. A viewed file shows folded, and ] and [ skip it.

use super::exec::{tab_file, write_atomic};
use crate::codec::sha;
use crate::json::{self, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

fn file_of(tab: &str) -> Option<String> {
    tab_file("viewed", tab)
}

/// The tab's marks: an object of file → signature, empty when there is none (or the file holds something else).
pub fn read(tab: &str) -> Value {
    let parsed = file_of(tab).and_then(|f| super::exec::read_text(f).ok()).and_then(|t| json::parse(&t).ok());
    match parsed {
        Some(v @ Value::Obj(_)) => v,
        _ => Value::Obj(Vec::new()),
    }
}

/// A signature known: ("size mtime ctime", signature, when hashed in ms).
type Known = (String, String, f64);

/// The signatures known, by file.
static SIGNS: Mutex<Option<HashMap<String, Known>>> = Mutex::new(None);

/// A signature not trusted for a file changed less than this before it was hashed (ms).
const RACY: f64 = 2000.0;

/// What the file holds now, in short: a hash of its content, "gone" once deleted. Hashed again only when its size or
/// times change, as git does: the Changes pane asks at each reload. Nor trusted for a file changed less than RACY
/// before it was hashed: changed again in the same tick (a coarse clock, as on Windows), its times would not show it.
pub fn sign(file: &str) -> String {
    let mut signs = SIGNS.lock().unwrap_or_else(|p| p.into_inner());
    let signs = signs.get_or_insert_with(HashMap::new);
    let stat = std::fs::metadata(file).map(|m| {
        let time = |t: std::io::Result<std::time::SystemTime>| t.map(crate::core::clock::ms).unwrap_or(0.0);
        let mtime = time(m.modified());
        (format!("{} {} {}", m.len(), json::number(mtime), json::number(time(changed(&m)))), mtime)
    });
    let signed = stat.and_then(|(stat, mtime)| {
        if let Some((known, sign, at)) = signs.get(file)
            && *known == stat
            && at - mtime > RACY
        {
            return Ok(sign.clone());
        }
        let at = crate::core::clock::now_ms();
        let sign = sha::hex(&sha::sha1(&std::fs::read(file)?));
        signs.insert(file.to_string(), (stat, sign.clone(), at));
        Ok(sign)
    });
    signed.unwrap_or_else(|_: std::io::Error| {
        signs.remove(file);
        "gone".into()
    })
}

/// The status change time (ctime); on Windows the last write.
fn changed(m: &std::fs::Metadata) -> std::io::Result<std::time::SystemTime> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let d = Duration::new(m.ctime().max(0) as u64, m.ctime_nsec().max(0) as u32);
        Ok(std::time::UNIX_EPOCH + d)
    }
    #[cfg(not(unix))]
    {
        m.modified()
    }
}

/// Viewed, and unchanged since.
pub fn is(map: &Value, file: &str) -> bool {
    match map.get(file) {
        Some(v) if v.truthy() => v.as_str() == Some(sign(file).as_str()),
        _ => false,
    }
}

/// Marks the file viewed, or no longer; returns whether it is now. Entries of files changed since are dropped.
pub fn toggle(tab: &str, file: &str) -> bool {
    let mut map = read(tab);
    let now = !is(&map, file);
    let keys: Vec<String> = map.as_object().map(|f| f.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    for f in keys {
        if f == file || !is(&map, &f) {
            map.remove(&f);
        }
    }
    if now {
        map.set(file, Value::Str(sign(file)));
    }
    if let Some(path) = file_of(tab) {
        if map.as_object().is_none_or(Vec::is_empty) {
            let _ = std::fs::remove_file(&path);
        } else {
            write_atomic(&path, json::stringify(&map));
        }
    }
    now
}

/// Watches the tab's marks (another pane marked a file); None without a state folder.
pub fn watch(tab: &str) -> Option<super::watch::Watch> {
    file_of(tab).map(|f| super::watch::Watch::new(&f, Duration::from_millis(500)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signature known for the file's size and times: trusted once hashed more than RACY after the file changed,
    /// hashed again when less.
    #[test]
    fn a_signature_is_not_trusted_for_a_file_changed_within_2_s_of_its_hash() {
        let dir = std::env::temp_dir().join(format!("viewed-racy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt").to_string_lossy().into_owned();
        std::fs::write(&file, "one\n").unwrap();
        let real = sign(&file);
        let stat = SIGNS.lock().unwrap().as_ref().unwrap()[&file].0.clone();
        let mtime = std::fs::metadata(&file).unwrap().modified().map(crate::core::clock::ms).unwrap();
        let known = |at: f64| {
            SIGNS.lock().unwrap().as_mut().unwrap().insert(file.clone(), (stat.clone(), "stale".into(), at));
        };
        known(mtime + RACY + 1000.0);
        assert_eq!(sign(&file), "stale");
        known(mtime + RACY - 1000.0);
        assert_eq!(sign(&file), real);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
