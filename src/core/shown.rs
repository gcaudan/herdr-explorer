//! The tab herdr shows, as its last tab.focused event told the hook: shown-tab in the state folder, the session's
//! socket (HERDR_SOCKET_PATH) and the tab's id, a line each. The explorers of the other tabs leave Follow asleep;
//! the file of another session, or none yet, wakes them all.

use super::exec::{env_var, read_text, state_dir, write_atomic};
use super::paths;

pub fn file() -> Option<String> {
    Some(paths::join(&[&state_dir()?, "shown-tab"]))
}

fn session() -> String {
    env_var("HERDR_SOCKET_PATH").unwrap_or_default()
}

fn line(tab: &str) -> String {
    format!("{}\n{tab}\n", session())
}

/// The tab shown from now on; written only when it changes, so that the explorers wake for a change only.
pub fn set(tab: &str) {
    let Some(f) = file() else { return };
    let text = line(tab);
    if !tab.is_empty() && read_text(&f).ok().as_deref() != Some(text.as_str()) {
        write_atomic(&f, text);
    }
}

/// The tab shown in this session, when the hook told it.
pub fn get() -> Option<String> {
    let text = read_text(file()?).ok()?;
    let (session, tab) = text.strip_suffix('\n')?.split_once('\n')?;
    (session == self::session() && !tab.is_empty()).then(|| tab.to_string())
}
