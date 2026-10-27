//! The herdr explorer plugin: panes, hook and peek, one binary. `main.rs` reads the command line and calls `cli::run`.
//! herdr runs this binary for every command of the manifest.

pub mod changes;
pub mod cli;
pub mod codec;
pub mod core;
pub mod diff;
pub mod edit;
pub mod explorer;
pub mod hook;
pub mod json;
pub mod launch;
pub mod peek;
pub mod preview;
pub mod render;
pub mod scm;
pub mod search;
pub mod sys;
pub mod unicode;

/// The plugin's version, from herdr-plugin.toml at build time: the one that runs.
pub const VERSION: &str = env!("HERDR_EXPLORER_VERSION");
