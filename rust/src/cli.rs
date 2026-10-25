//! The subcommands, one per entry of the manifest: `pane <id>` for the panes, `hook` for the events and actions,
//! `peek` for agents and links. Each keeps the arguments and the `EXPLORER_*` variables of the JS entry it replaces.

use std::ffi::OsString;
use std::process::ExitCode;

/// The panes of the manifest, by id, and the JS script each replaces.
pub const PANES: [(&str, &str); 6] = [
    ("tree", "explorer.js"),
    ("preview", "preview.js"),
    ("search", "search.js"),
    ("scm", "scm.js"),
    ("diff", "diff.js"),
    ("edit", "edit.js"),
];

const USAGE: &str = "usage: herdr-explorer pane <tree|preview|search|scm|diff|edit> [args]
       herdr-explorer hook [--restore|--closed|--agent|--focused|--toggle|--search|--scm]
       herdr-explorer peek <file>[:<places>] | --last | --diff ... | --url
       herdr-explorer --version";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Version,
    Pane(&'static str, Vec<OsString>),
    Hook(Vec<OsString>),
    Peek(Vec<OsString>),
}

/// Reads the command line (without the program's name); `Err` holds the message for a wrong use.
pub fn parse(mut args: Vec<OsString>) -> Result<Command, String> {
    if args.is_empty() {
        return Err(USAGE.into());
    }
    let first = args.remove(0);
    match first.to_str() {
        Some("--version" | "-V") => Ok(Command::Version),
        Some("pane") => {
            let id = args.first().and_then(|a| a.to_str()).unwrap_or("");
            let Some(&(id, _)) = PANES.iter().find(|(p, _)| *p == id) else {
                return Err(format!("unknown pane {id:?}\n{USAGE}"));
            };
            args.remove(0);
            Ok(Command::Pane(id, args))
        }
        Some("hook") => Ok(Command::Hook(args)),
        Some("peek") => Ok(Command::Peek(args)),
        _ => Err(USAGE.into()),
    }
}

/// The binaries set aside by fetch-bin, dev-build and the updater (`<name>.old-*` next to this one), removed when
/// they can be: Windows keeps one that still runs, until a later start.
fn remove_old(exe: &std::path::Path) {
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name().and_then(|n| n.to_str())) else { return };
    let prefix = format!("{name}.old-");
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if e.file_name().to_str().is_some_and(|n| n.starts_with(&prefix)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Runs the command line and gives the process's exit code.
pub fn run(args: Vec<OsString>) -> ExitCode {
    let cmd = parse(args);
    if matches!(cmd, Ok(Command::Hook(_) | Command::Peek(_) | Command::Pane(..)))
        && let Ok(exe) = std::env::current_exe()
    {
        remove_old(&exe);
    }
    match cmd {
        Ok(Command::Version) => {
            println!("{}", crate::VERSION);
            ExitCode::SUCCESS
        }
        Ok(Command::Hook(args)) => done(crate::hook::run(&strings(args))),
        Ok(Command::Peek(args)) => done(crate::peek::run(strings(args))),
        Ok(cmd @ Command::Pane(..)) => {
            // The explorer's and the preview's first process stays as their launcher; the child runs the pane. The
            // other panes are short-lived and ran without one in the JS; under a shim (EXPLORER_IPC_FILE), every
            // pane is the child of the shim.
            if (matches!(cmd, Command::Pane("tree" | "preview", _)) || std::env::var_os("EXPLORER_IPC_FILE").is_some())
                && let Some(code) = crate::launch::launch()
            {
                return ExitCode::from(u8::try_from(code).unwrap_or(1));
            }
            match cmd {
                Command::Pane("tree", args) => return done(crate::explorer::run(&strings(args))),
                Command::Pane("preview", args) => return done(crate::preview::run(&strings(args))),
                Command::Pane("search", args) => return done(crate::search::run(&strings(args))),
                Command::Pane("diff", args) => return done(crate::diff::pane::run(&strings(args))),
                Command::Pane("scm", args) => return done(crate::scm::pane::run(&strings(args))),
                Command::Pane("edit", args) => return done(crate::edit::run(&strings(args))),
                _ => {}
            }
            eprintln!("herdr-explorer: not implemented yet: {cmd:?}");
            done(1)
        }
        Err(usage) => {
            eprintln!("{usage}");
            ExitCode::from(2)
        }
    }
}

/// The arguments as the JS read them from process.argv: what is not Unicode replaced.
fn strings(args: Vec<OsString>) -> Vec<String> {
    args.into_iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// The exit code, once the work given to `exec::at_exit` is done (the JS's process.on('exit')).
fn done(code: i32) -> ExitCode {
    crate::core::exec::exit_hooks();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_the_binaries_set_aside_only() {
        let dir = std::env::temp_dir().join(format!("hx-old-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let names = ["herdr-explorer", "herdr-explorer.old-20260101-1", "herdr-explorer.older", "other.old-1"];
        for n in names {
            std::fs::write(dir.join(n), "").unwrap();
        }
        remove_old(&dir.join("herdr-explorer"));
        let left: Vec<bool> = names.iter().map(|n| dir.join(n).exists()).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left, [true, false, true, true]);
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_the_manifest_entries() {
        assert_eq!(parse(args(&["--version"])), Ok(Command::Version));
        assert_eq!(parse(args(&["pane", "tree", "/x"])), Ok(Command::Pane("tree", args(&["/x"]))));
        assert_eq!(parse(args(&["hook", "--closed"])), Ok(Command::Hook(args(&["--closed"]))));
        assert_eq!(parse(args(&["peek", "--last"])), Ok(Command::Peek(args(&["--last"]))));
        assert!(parse(args(&["pane", "nope"])).is_err());
        assert!(parse(args(&[])).is_err());
    }

    #[test]
    fn embeds_the_manifest_version() {
        let toml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../herdr-plugin.toml")).unwrap();
        let line = format!("version = \"{}\"", crate::VERSION);
        assert!(toml.lines().any(|l| l == line), "{line} is not a line of herdr-plugin.toml");
    }
}
