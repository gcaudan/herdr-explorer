//! `herdr-explorer setup`: what herdr's plugin index does not do. The key bindings appended to herdr's config.toml
//! (prefix+f, prefix+a, prefix+d), each unless the config already binds its action, then the config reloaded; with
//! Claude Code (~/.claude), the herdr-preview skill linked to the plugin's folder (a junction on Windows, which needs
//! no right). A folder of that name that is not a link is the user's, and left alone. The explorer offers it once
//! when the config binds none of the plugin's actions (`offer`), and points the skill's link at each new plugin
//! folder, as `herdr plugin update` installs into a new one (`refresh_skill`).

use super::exec::{self, Opts, WIN};
#[cfg(windows)]
use super::paths;
use super::{icons, update};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The bindings: key, action, description.
const KEYS: [(&str, &str, &str); 3] = [
    ("prefix+f", "toggle", "file explorer"),
    ("prefix+a", "search", "go to file"),
    ("prefix+d", "scm", "source control"),
];

/// Where herdr reads its config: HERDR_CONFIG_PATH, else %APPDATA%\herdr on Windows, $XDG_CONFIG_HOME/herdr or
/// ~/.config/herdr elsewhere (macOS included).
pub fn config_file() -> Option<PathBuf> {
    if let Some(p) = exec::env_var("HERDR_CONFIG_PATH") {
        return Some(p.into());
    }
    let dir = if WIN {
        PathBuf::from(exec::env_var("APPDATA")?)
    } else {
        exec::env_var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(&icons::homedir()).join(".config"))
    };
    Some(dir.join("herdr").join("config.toml"))
}

/// The config's line that binds an action of the plugin.
fn command_of(action: &str) -> String {
    format!("{}.{action}\"", exec::plugin())
}

/// Whether the config binds one of the plugin's actions.
pub fn bound() -> bool {
    let text = config_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    KEYS.iter().any(|(_, action, _)| text.contains(&command_of(action)))
}

/// The bindings missing from the config appended: the lines that tell what was done.
fn bind(config: &Path) -> std::io::Result<Vec<String>> {
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = std::fs::read_to_string(config).unwrap_or_default();
    let mut add = String::new();
    let mut done = Vec::new();
    for (key, action, description) in KEYS {
        if text.contains(&command_of(action)) {
            continue;
        }
        add += &format!(
            "\n[[keys.command]]\nkey = \"{key}\"\ntype = \"plugin_action\"\ncommand = \"{}.{action}\"\ndescription = \"{description}\"\n",
            exec::plugin()
        );
        done.push(format!("key     {key} in {}", config.display()));
    }
    if !add.is_empty() {
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(config)?.write_all(add.as_bytes())?;
        let opts = Opts { timeout: Some(Duration::from_secs(10)), ..Opts::default() };
        let _ = exec::run(exec::herdr(), &["server", "reload-config"], &opts);
    }
    Ok(done)
}

/// The skill's link in ~/.claude/skills, None without Claude Code.
fn skill_link() -> Option<PathBuf> {
    let claude = Path::new(&icons::homedir()).join(".claude");
    claude.is_dir().then(|| claude.join("skills").join("herdr-preview"))
}

/// Whether the path is a link (a junction on Windows), and where it points.
fn link_target(link: &Path) -> Option<Option<PathBuf>> {
    let meta = std::fs::symlink_metadata(link).ok()?;
    meta.file_type().is_symlink().then(|| std::fs::read_link(link).ok())
}

/// The link made, pointing at target, in place of an older link.
fn make_link(link: &Path, target: &Path) -> std::io::Result<()> {
    if link_target(link).is_some() {
        // A junction or a link to a folder goes with remove_dir on Windows, remove_file elsewhere.
        if WIN { std::fs::remove_dir(link) } else { std::fs::remove_file(link) }?;
    }
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    {
        // mklink /J by cmd's absolute path, never one found in a folder of PATH.
        let root = exec::env_var("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let cmd = paths::join(&[&root, "System32", "cmd.exe"]);
        let (l, t) = (link.to_string_lossy(), target.to_string_lossy());
        let opts = Opts { timeout: Some(Duration::from_secs(10)), ..Opts::default() };
        match exec::run(Some(&cmd), &["/c", "mklink", "/J", &l, &t], &opts) {
            Some(_) => Ok(()),
            None => Err(std::io::Error::other("mklink failed")),
        }
    }
}

/// The skill of the plugin's folder.
fn skill_dir() -> Option<PathBuf> {
    Some(Path::new(update::dir()?).join("skills").join("herdr-preview"))
}

/// The skill linked: a line that tells what was done, None without Claude Code.
fn link_skill() -> Option<String> {
    let (link, target) = (skill_link()?, skill_dir()?);
    let shown = link.display();
    if std::fs::symlink_metadata(&link).is_ok() && link_target(&link).is_none() {
        return Some(format!("skill   {shown} exists and is not a link: left alone"));
    }
    Some(match make_link(&link, &target) {
        Ok(()) => format!("skill   {shown}"),
        Err(e) => format!("skill   {shown} not linked: {e}"),
    })
}

/// The skill's link, made by setup, pointed at this plugin folder's skill when it points elsewhere (a former
/// install's folder). Nothing when there is no link: the user ran no setup, or removed it.
pub fn refresh_skill() {
    let (Some(link), Some(target)) = (skill_link(), skill_dir()) else { return };
    let Some(Some(now)) = link_target(&link) else { return };
    let same = |a: &Path, b: &Path| exec::real_path(&a.to_string_lossy()) == exec::real_path(&b.to_string_lossy());
    if !same(&now, &target) && target.is_dir() {
        let _ = make_link(&link, &target);
    }
}

/// Runs the setup: the bindings, then the skill. The lines that tell what was done, or why it failed.
pub fn setup() -> Result<Vec<String>, String> {
    let config = config_file().ok_or("herdr's config folder not found")?;
    let mut lines = bind(&config).map_err(|e| format!("{}: {e}", config.display()))?;
    if lines.is_empty() {
        lines.push(format!("keys    already bound in {}", config.display()));
    }
    lines.extend(link_skill());
    Ok(lines)
}

/// `herdr-explorer setup`: its exit code.
pub fn run() -> i32 {
    match setup() {
        Ok(lines) => {
            for l in lines {
                println!("  {l}");
            }
            0
        }
        Err(e) => {
            eprintln!("setup: {e}");
            1
        }
    }
}
