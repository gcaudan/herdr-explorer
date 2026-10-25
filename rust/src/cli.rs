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

/// Runs the command line and gives the process's exit code.
pub fn run(args: Vec<OsString>) -> ExitCode {
    match parse(args) {
        Ok(Command::Version) => {
            println!("{}", crate::VERSION);
            ExitCode::SUCCESS
        }
        Ok(Command::Hook(args)) => done(crate::hook::run(&strings(args))),
        Ok(Command::Peek(args)) => done(crate::peek::run(strings(args))),
        Ok(cmd @ Command::Pane(..)) => {
            // The pane's first process stays as its launcher; the child runs the pane.
            if let Some(code) = crate::launch::launch() {
                return ExitCode::from(u8::try_from(code).unwrap_or(1));
            }
            match cmd {
                Command::Pane("tree", args) => return done(crate::explorer::run(&strings(args))),
                Command::Pane("preview", args) => return done(crate::preview::run(&strings(args))),
                Command::Pane("search", args) => return done(crate::search::run(&strings(args))),
                Command::Pane("diff", args) => return done(crate::diff::pane::run(&strings(args))),
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
