//! The folder watcher (`fs.watch`): the names changed, relative to the folder, the whole tree when recursive, the
//! folders made later included, and the end of the watch when dropped. Linux only here (inotify): the other systems
//! are checked by clippy on their targets.
#![cfg(target_os = "linux")]

mod support;

use herdr_explorer::sys::Watcher;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

fn watch(dir: &std::path::Path, recursive: bool) -> std::io::Result<(Watcher, Receiver<Option<String>>)> {
    let (tx, rx) = channel();
    let w = Watcher::new(&dir.to_string_lossy(), recursive, move |name| {
        let _ = tx.send(name);
    })?;
    Ok((w, rx))
}

/// Waits for name among the changes, up to 5 s.
fn seen(rx: &Receiver<Option<String>>, name: &str) -> bool {
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while let Some(left) = end.checked_duration_since(std::time::Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(Some(n)) if n == name => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn a_folder_gives_the_names_changed_in_it() {
    let dir = support::TempDir::new("watch");
    std::fs::create_dir(dir.path.join("sub")).unwrap();
    let (_w, rx) = watch(&dir.path, false).unwrap();
    std::fs::write(dir.path.join("a.txt"), "x").unwrap();
    assert!(seen(&rx, "a.txt"));
    // Not recursive: a change in a folder under it shows only as that folder's own.
    std::fs::write(dir.path.join("sub/b.txt"), "x").unwrap();
    std::fs::remove_file(dir.path.join("a.txt")).unwrap();
    let mut names = Vec::new();
    while let Ok(n) = rx.recv_timeout(Duration::from_millis(300)) {
        names.push(n);
    }
    assert!(names.contains(&Some("a.txt".into())));
    assert!(!names.contains(&Some("sub/b.txt".into())));
}

#[test]
fn a_recursive_watch_gives_the_whole_tree_folders_made_later_included() {
    let dir = support::TempDir::new("watch");
    std::fs::create_dir_all(dir.path.join("a/b")).unwrap();
    let (_w, rx) = watch(&dir.path, true).unwrap();
    std::fs::write(dir.path.join("a/b/deep.txt"), "x").unwrap();
    assert!(seen(&rx, "a/b/deep.txt"));
    std::fs::create_dir(dir.path.join("new")).unwrap();
    assert!(seen(&rx, "new"));
    std::fs::write(dir.path.join("new/f.txt"), "x").unwrap();
    assert!(seen(&rx, "new/f.txt"));
}

#[test]
fn dropped_the_watch_ends() {
    let dir = support::TempDir::new("watch");
    let (w, rx) = watch(&dir.path, true).unwrap();
    drop(w);
    std::fs::write(dir.path.join("a.txt"), "x").unwrap();
    // The callback (and its sender) went with the thread.
    assert!(matches!(rx.recv_timeout(Duration::from_secs(2)), Err(std::sync::mpsc::RecvTimeoutError::Disconnected)));
}

#[test]
fn a_missing_folder_or_an_unreadable_one_in_the_tree_fails() {
    use std::os::unix::fs::PermissionsExt;
    let dir = support::TempDir::new("watch");
    assert!(watch(&dir.path.join("missing"), false).is_err());
    let locked = dir.path.join("locked");
    std::fs::create_dir_all(locked.join("inner")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads it anyway.
    if std::fs::read_dir(&locked).is_err() {
        assert!(watch(&dir.path, true).is_err(), "recursive: the tree cannot be read");
        assert!(watch(&dir.path, false).is_ok(), "the top level only");
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_folder_moved_out_of_the_tree_is_no_longer_watched() {
    let dir = support::TempDir::new("watch");
    let out = support::TempDir::new("watch-out");
    std::fs::create_dir_all(dir.path.join("build/sub")).unwrap();
    let (_w, rx) = watch(&dir.path, true).unwrap();
    std::fs::rename(dir.path.join("build"), out.path.join("build")).unwrap();
    assert!(seen(&rx, "build"));
    std::fs::write(out.path.join("build/sub/f"), "x").unwrap();
    std::fs::write(dir.path.join("here"), "x").unwrap();
    let mut names = Vec::new();
    while let Ok(n) = rx.recv_timeout(Duration::from_millis(300)) {
        names.push(n);
    }
    assert!(names.contains(&Some("here".into())));
    assert!(!names.iter().flatten().any(|n| n.starts_with("build/")), "{names:?}");
}
