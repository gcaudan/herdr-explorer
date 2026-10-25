//! The source control pane (src/scm.js and src/scm/): parse (git's output turned into data), context (state S,
//! settings, what the modules share), state (git and the loaders), pane (the pane, its loop and the selection), ops
//! (what it does to the repository), render (the frame), input (keys and mouse).

pub mod context;
mod input;
mod ops;
pub mod pane;
pub mod parse;
mod render;
pub mod state;
