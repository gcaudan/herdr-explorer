//! The diff library: git's changes collected (collect), read (parse), then laid out (render). The
//! Changes pane (changes/), the preview's Diff view and source control show them.

mod collect;
mod parse;
mod render;

pub use collect::{
    Collected, DIFF_OPTS, MAX_UNTRACKED, changes_of, collect, default_branch, line_changes, pull_of, show, show_args,
    view_items,
};
pub use parse::{Diff, File, Head, Hunk, Line, head, parse, strip, unquote};
pub use render::{
    MAX_ROWS, Redo, Ref, Row, SPLIT_MIN, View, default_mode, disk_lines, layout_of, render, save_mode, saved_mode,
    shown_lines, widest_of,
};
