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

/// Whether the config binds one of the plugin's actions.
pub fn bound() -> bool {
    let text = config_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    KEYS.iter().any(|(_, action, _)| binds(&text, action))
}

/// Whether a [[keys.command]] block of the config binds the action.
fn binds(text: &str, action: &str) -> bool {
    keys_in(text).iter().any(|(_, a)| a == action)
}

/// A TOML value as a string: a quoted one up to its closing quote (`"prefix+#"` keeps its `#`), else the bare value
/// before a comment.
fn toml_str(v: &str) -> String {
    let v = v.trim();
    let mut chars = v.chars();
    match chars.next() {
        Some('\'') => chars.take_while(|&c| c != '\'').collect(),
        Some('"') => {
            let mut out = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => out.extend(chars.next()),
                    c => out.push(c),
                }
            }
            out
        }
        _ => v.split('#').next().unwrap_or("").trim().to_string(),
    }
}

/// The keys the config binds to each of the plugin's actions ([[keys.command]] blocks): (key, action), in the
/// config's order.
pub fn action_keys() -> Vec<(String, String)> {
    let text = config_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    keys_in(&text)
}

fn keys_in(text: &str) -> Vec<(String, String)> {
    let prefix = format!("{}.", exec::plugin());
    let (mut out, mut block) = (Vec::new(), None::<(String, String)>);
    let mut end = |block: &mut Option<(String, String)>| {
        if let Some((key, command)) = block.take()
            && let Some(action) = command.strip_prefix(&prefix)
            && !key.is_empty()
        {
            out.push((key, action.to_string()));
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            end(&mut block);
            if l.split('#').next().unwrap_or("").replace([' ', '\t'], "") == "[[keys.command]]" {
                block = Some(Default::default());
            }
        } else if let Some((k, v)) = l.split_once('=')
            && let Some(b) = &mut block
        {
            match k.trim() {
                "key" => b.0 = toml_str(v),
                "command" => b.1 = toml_str(v),
                _ => {}
            }
        }
    }
    end(&mut block);
    out
}

/// A binding as config.toml writes it.
fn block(key: &str, action: &str, description: &str) -> String {
    format!(
        "\n[[keys.command]]\nkey = \"{key}\"\ntype = \"plugin_action\"\ncommand = \"{}.{action}\"\ndescription = \"{description}\"\n",
        exec::plugin()
    )
}

/// What the config's text gets: the bindings to append and the lines that tell what was done (where: the config,
/// shown). A key already bound is skipped; a config that binds keys.command inline, or sets keys as an inline table,
/// which a table appended would make invalid, is left alone: Err, why.
fn plan(text: &str, shown: &str) -> Result<(String, Vec<String>), &'static str> {
    let mut table = String::new();
    let mut keys = Vec::new();
    for line in text.lines() {
        let l: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if l.starts_with('[') {
            table = l.trim_matches(['[', ']']).to_string();
        } else if l.starts_with("keys.command=") || (table == "keys" && l.starts_with("command=")) {
            return Err("sets keys.command inline");
        } else if table.is_empty() && l.starts_with("keys=") {
            return Err("sets keys inline");
        } else if let Some(k) = l.strip_prefix("key=").filter(|_| table == "keys" || table.starts_with("keys.")) {
            keys.push(toml_str(k));
        }
    }
    let mut add = String::new();
    let mut done = Vec::new();
    for (key, action, description) in KEYS {
        if binds(text, action) {
            continue;
        }
        // herdr reads keys without regard to case: Prefix+F is prefix+f.
        if keys.iter().any(|k| k.eq_ignore_ascii_case(key)) {
            done.push(format!("key     {key} already bound in {shown}, skipped"));
            continue;
        }
        add += &block(key, action, description);
        done.push(format!("key     {key} in {shown}"));
    }
    Ok((add, done))
}

/// The bindings missing from the config appended (plan): the lines that tell what was done. A config that cannot be
/// read (not UTF-8) or that plan leaves alone is not changed, the lines to copy given in the error.
fn bind(config: &Path) -> Result<Vec<String>, String> {
    let io = |e: std::io::Error| format!("{}: {e}", config.display());
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let text = match std::fs::read_to_string(config) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(manual(config, &format!("cannot be read ({e})"))),
    };
    let (add, done) = plan(&text, &config.display().to_string()).map_err(|why| manual(config, why))?;
    if !add.is_empty() {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(config).map_err(io)?;
        f.write_all(add.as_bytes()).map_err(io)?;
        let opts = Opts { timeout: Some(Duration::from_secs(10)), ..Opts::default() };
        let _ = exec::run(exec::herdr(), &["server", "reload-config"], &opts);
    }
    Ok(done)
}

/// The config left alone: why, then the bindings to copy into it.
fn manual(config: &Path, why: &str) -> String {
    let blocks: String = KEYS.iter().map(|(k, a, d)| block(k, a, d)).collect();
    format!("{} {why}: not changed; add these lines to it:\n{blocks}", config.display())
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
    let mut lines = bind(&config)?;
    if lines.is_empty() {
        lines.push(format!("keys    already bound in {}", config.display()));
    }
    lines.extend(link_skill());
    Ok(lines)
}

/// What setup's lines tell in one line, for the explorer's message: the keys added, those skipped.
pub fn summary(lines: &[String]) -> String {
    let key = |l: &String| l.strip_prefix("key     ").and_then(|l| l.split(' ').next()).map(String::from);
    let skipped: Vec<String> = lines.iter().filter(|l| l.ends_with(", skipped")).filter_map(key).collect();
    let added = lines.iter().filter(|l| !l.ends_with(", skipped")).filter_map(key).count();
    match (added, skipped.is_empty()) {
        (0, true) => "Key bindings already in herdr's config".into(),
        (_, true) => "Key bindings added".into(),
        (0, false) => format!("No key binding added: {} already bound", skipped.join(", ")),
        _ => format!("Key bindings added; {} already bound: skipped", skipped.join(", ")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_bound_already_is_skipped() {
        let (add, done) = plan("[[keys.command]]\nkey = \"prefix+f\" # mine\ncommand = \"other\"\n", "c").unwrap();
        assert!(!add.contains("prefix+f") && add.contains("prefix+a") && add.contains("prefix+d"), "{add}");
        assert!(done[0].contains("prefix+f already bound in c, skipped"), "{done:?}");
        assert_eq!(summary(&done), "Key bindings added; prefix+f already bound: skipped");
    }

    #[test]
    fn a_binding_commented_out_binds_nothing() {
        let text = "# [[keys.command]]\n# key = \"prefix+f\"\n  # command = \"gcaudan.explorer.toggle\"\n";
        assert!(!binds(text, "toggle"));
        assert!(plan(text, "c").unwrap().0.contains("prefix+f"));
        let trailing = "[[keys.command]]\nkey = \"x\" # \"gcaudan.explorer.toggle\"\ncommand = \"other\"\n";
        assert!(!binds(trailing, "toggle"));
    }

    #[test]
    fn bindings_are_read_as_toml_strings() {
        let text = "[[keys.command]] # mine\nkey = \"prefix+#\" # hash\ncommand = 'gcaudan.explorer.toggle'\n";
        assert_eq!(keys_in(text), [("prefix+#".to_string(), "toggle".to_string())]);
        assert!(binds(text, "toggle"));
        assert!(!plan(text, "c").unwrap().0.contains("explorer.toggle"));
    }

    #[test]
    fn keys_are_compared_without_case() {
        let text = "[[keys.command]]\nkey = \"Prefix+F\"\n[[keys.command]]\nkey='PREFIX+A'\n[[keys.command]]\nkey=\"prefix+D\"\n";
        let (add, done) = plan(text, "c").unwrap();
        assert!(add.is_empty(), "{add}");
        assert_eq!(summary(&done), "No key binding added: prefix+f, prefix+a, prefix+d already bound");
        assert_eq!(summary(&plan("", "c").unwrap().1), "Key bindings added");
    }

    #[test]
    fn a_key_outside_the_keys_tables_binds_nothing() {
        let (add, _) = plan("[theme]\nkey = \"prefix+f\"\n", "c").unwrap();
        assert!(add.contains("prefix+f"), "{add}");
    }

    #[test]
    fn an_inline_keys_command_is_left_alone() {
        assert!(plan("keys.command = []\n", "c").is_err());
        assert!(plan("[keys]\ncommand = [ ]\n", "c").is_err());
        assert!(plan("keys = { prefix = \"ctrl+b\" }\n", "c").is_err());
        assert!(plan("[theme]\nkeys = 1\n", "c").is_ok());
        let text = format!(
            "[[keys.command]]\nkey = \"prefix+f\"\ncommand = \"{p}.toggle\" # x\n\n[ui]\nkey = \"z\"\n\
             [[keys.command]]\ncommand = \"other.scm\"\nkey = \"prefix+d\"\n[[ keys.command ]]\ncommand = '{p}.scm'\n\
             key = 'ctrl+s'\n",
            p = exec::plugin()
        );
        assert_eq!(keys_in(&text), [("prefix+f".into(), "toggle".into()), ("ctrl+s".into(), "scm".into())]);
        let (add, done) = plan("", "c").unwrap();
        assert_eq!((add.matches("[[keys.command]]").count(), done.len()), (3, 3));
    }
}
