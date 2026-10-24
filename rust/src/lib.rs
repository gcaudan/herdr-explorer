//! The herdr explorer plugin: panes, hook and peek, one binary. `main.rs` reads the command line and calls `cli::run`.

pub mod cli;
pub mod codec;
pub mod json;
pub mod sys;
pub mod unicode;

/// The plugin's version, from herdr-plugin.toml at build time: the one that runs.
pub const VERSION: &str = env!("HERDR_EXPLORER_VERSION");
