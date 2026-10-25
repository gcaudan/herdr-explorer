//! The source control pane (src/scm.js and src/scm/): parse (git's output turned into data), context (state S,
//! settings, what the modules share), state (git and the loaders).

pub mod context;
pub mod parse;
pub mod state;
