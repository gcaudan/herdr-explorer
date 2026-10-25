//! The search's state: the mode and the fields typed in, the options, the file list and Go to file's results,
//! Search in files' groups, the rows and what a click on the top lines does.

use super::find::Group;
use crate::core::regex::Regex;
use crate::core::ui::{Field, HelpState, Hints, Seg};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Files,
    Content,
}

/// The field typed in, in Search in files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Query,
    Include,
    Exclude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Opt {
    Case,
    Word,
    Regex,
    Hidden,
}

/// What a click on a part of the top lines does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    Mode(Mode),
    Toggle(Opt),
    Focus(Focus),
}

/// A clickable area of the last frame: its screen row (from 0), first and last columns (from 1).
pub(super) struct Zone {
    pub(super) y: i64,
    pub(super) x1: i64,
    pub(super) x2: i64,
    pub(super) act: Act,
}

/// A field, and whether its text was restored from the last search: shown selected, typing replaces it.
pub(super) struct SField {
    pub(super) field: Field,
    pub(super) restored: bool,
}

impl SField {
    pub(super) fn new(value: &str) -> SField {
        SField { field: Field::new(value), restored: !value.is_empty() }
    }
}

pub(super) const FILES: usize = 0;
pub(super) const CONTENT: usize = 1;
pub(super) const INCLUDE: usize = 2;
pub(super) const EXCLUDE: usize = 3;

/// The file list, sorted, with each path in UTF-16 units, in lower case, and where its name starts.
pub(super) struct Files {
    pub(super) list: Arc<Vec<String>>,
    pub(super) units: Vec<Vec<u16>>,
    pub(super) lower: Vec<Vec<u16>>,
    pub(super) base: Vec<usize>,
}

/// A Go to file result: the file's index, its score, and the matched positions once drawn.
pub(super) struct Scored {
    pub(super) i: usize,
    pub(super) score: i64,
    pub(super) pos: Option<Vec<usize>>,
}

/// A row of the results: a Go to file result; or a file of Search in files, then its matching lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    File(usize),
    Head(usize),
    Line(usize, usize),
}

/// A file read for the preview on the right: its lines and their coloured segments, or why it cannot show.
pub(super) enum Shown {
    Text { lines: Vec<String>, segs: Vec<Vec<Seg>> },
    Error(String),
}

/// The file and line of a row.
pub(super) struct Target {
    pub(super) rel: String,
    pub(super) line: f64,
}

pub(super) struct State {
    pub(super) mode: Mode,
    pub(super) fields: [SField; 4],
    /// Search in files: query, include or exclude.
    pub(super) focus: Focus,
    pub(super) case: bool,
    pub(super) word: bool,
    pub(super) regex: bool,
    pub(super) hidden: bool,
    pub(super) files: Option<Files>,
    pub(super) capped: bool,
    pub(super) files_gen: u64,
    // Go to file
    pub(super) results: Vec<Scored>,
    pub(super) line: f64,
    /// The last query in lower case and the files it matched, to score only those when the query grows.
    pub(super) prev: Option<(Vec<u16>, Vec<usize>)>,
    // Search in files
    pub(super) groups: Vec<Group>,
    pub(super) total: usize,
    pub(super) searching: bool,
    pub(super) error: Option<String>,
    pub(super) re: Option<Regex>,
    pub(super) search_key: Option<String>,
    pub(super) touched: bool,
    pub(super) cursor: i64,
    pub(super) top: i64,
    pub(super) flash: Option<String>,
    pub(super) last_click: Option<(i64, Instant)>,
    pub(super) help: Option<HelpState>,
    pub(super) hints: Hints,
}
