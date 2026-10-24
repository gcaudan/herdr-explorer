//! The Changes pane, as the JS's src/diff.js: git's diff read (parse), then laid out and shown.

mod parse;

pub use parse::{Diff, File, Head, Hunk, Line, head, parse, strip, unquote};
