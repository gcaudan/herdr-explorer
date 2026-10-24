//! What the explorer does with a file or a line, as the JS's src/core/actions.js; so far the paths it gives.

use super::paths;

/// A path relative to a folder when inside it, with '/' separators; else the absolute path.
pub fn relative_to(dir: Option<&str>, abs: &str) -> String {
    let mut reference = abs.to_string();
    if let Some(dir) = dir.filter(|d| !d.is_empty()) {
        let rel = paths::relative(dir, abs);
        if !rel.starts_with("..") && !paths::is_absolute(&rel) {
            reference = if rel.is_empty() { ".".into() } else { rel };
        }
    }
    reference.split(paths::SEP).collect::<Vec<_>>().join("/")
}
