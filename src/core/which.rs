//! Finding programs in the absolute folders of PATH.

use super::paths;

/// Absolute folders of PATH (relative entries would depend on the working directory).
pub fn path_dirs() -> Vec<String> {
    std::env::var_os("PATH")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
        .split(paths::DELIMITER)
        .map(|d| if d.len() >= 2 && d.starts_with('"') && d.ends_with('"') { &d[1..d.len() - 1] } else { d })
        .filter(|d| paths::is_absolute(d))
        .map(String::from)
        .collect()
}

/// Absolute path of a program found in the absolute folders of PATH, or None. On Windows, a bare name is looked up in
/// the child's working directory before PATH; elsewhere, an empty or '.' entry of PATH means that folder too: a git
/// planted in an untrusted repository would run. Resolved first, on every system.
pub fn which(name: &str) -> Option<String> {
    if paths::is_absolute(name) {
        return Some(name.to_string());
    }
    let file = if cfg!(windows) && paths::extname(name).is_empty() { format!("{name}.exe") } else { name.to_string() };
    for dir in path_dirs() {
        let full = paths::join(&[&dir, &file]);
        if std::fs::metadata(&full).is_ok_and(|m| m.is_file()) {
            #[cfg(unix)]
            if !crate::sys::executable(&full) {
                continue;
            }
            return Some(full);
        }
    }
    None
}
