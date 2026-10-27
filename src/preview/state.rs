//! The preview's state: the marks peek asks for, the tabs' views, the rows drawn, the selection and find, the
//! header's chips and the view's options, and State, what the pane shows and is doing.

use super::{diff, edit, media, review};
use crate::core::scope::{self, Scope};
use crate::core::tabs::Tabs;
use crate::core::ui::{self, Field, HelpState, Menu, Seg};
use crate::json::Value;
use std::rc::Rc;
use std::sync::Arc;

/// A part highlighted by the caller (peek): 1-based lines from to to, columns a to b included (0: whole lines).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub from: i64,
    pub to: i64,
    pub a: i64,
    pub b: i64,
}

/// Marks from a request or the environment, kept only when well formed: at most 200, in order.
pub(super) fn clean_marks(list: Option<&Value>) -> Vec<Mark> {
    let Some(list) = list.and_then(Value::as_array) else { return Vec::new() };
    // Number.isInteger(v) && v >= 0 ? v : 0
    let int = |v: Option<&Value>| match v {
        Some(Value::Num(n)) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => *n as i64,
        _ => 0,
    };
    list.iter()
        .take(200)
        .filter(|m| m.truthy())
        .map(|m| Mark { from: int(m.get("from")), to: int(m.get("to")), a: int(m.get("a")), b: int(m.get("b")) })
        .filter(|m| m.from >= 1 && m.to >= m.from && (m.a == 0 || m.b >= m.a))
        .collect()
}

/// What the file shows as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Markdown,
    Image,
    Binary,
    Error,
}

/// What a tab not shown keeps of its view, put away by stash and taken back by unstash. None:
/// a tab never shown, which starts from the defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabView {
    pub(super) root: String,
    pub(super) line: f64,
    pub(super) top: i64,
    pub(super) left: i64,
    pub(super) diff: bool,
    pub(super) marks: Vec<Mark>,
    pub(super) mark: usize,
    pub(super) raw: bool,
    /// A tab being edited keeps its text, cursor and history while another tab shows (never saved for a restart).
    pub(super) edit: Option<edit::Shared>,
}

pub(super) type PaneTabs = Tabs<Option<TabView>>;

/// A display row: its segments, the file line it shows (src, -1 for none), whether it is that line's first row, where
/// it starts in the line (off, in bytes, for the find's matches), or an empty area's message and keys in its place.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub segs: Vec<Seg>,
    pub src: i64,
    pub first: bool,
    pub off: usize,
    pub empty: Option<(String, Vec<(String, String)>)>,
    pub pad: usize,
    pub back: Option<ui::Rgb>,
    /// A row of the Diff view: what it shows, for n, s, u, D and the references.
    pub diff: Option<crate::diff::Row>,
    /// An image file in pixels: the first of its blank rows holds what to place (see place_image).
    pub picture: Option<Rc<media::Picture>>,
    /// The line under an image drawn with half blocks: its clickable parts.
    pub hint: Option<Vec<Chip>>,
    /// A row of an image of rendered Markdown in pixels (see place_figures).
    pub figure: Option<Rc<media::Fig>>,
}

impl Row {
    pub(super) fn empty(text: &str, pairs: &[(&str, &str)]) -> Row {
        let pairs = pairs.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect();
        Row { src: -1, empty: Some((text.to_string(), pairs)), ..Row::default() }
    }
}

/// The display rows for a width and height, built again when either changes.
pub struct Rows {
    pub list: Vec<Row>,
    pub(super) width: usize,
    pub(super) height: usize,
    /// The ids of the images of rendered Markdown in pixels that the rows hold, for place_figures.
    pub(super) figures: Vec<u32>,
}

/// A position: a display row and the byte index of a character in its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub(super) row: usize,
    pub(super) at: usize,
}

/// The selection made with the mouse: both ends included, as in a terminal. fixed: a word or a line (double or triple
/// click), never dragged; done: copied, shown until the next click or Escape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Select {
    pub(super) from: Pos,
    pub(super) to: Pos,
    pub(super) moved: bool,
    pub(super) fixed: bool,
    pub(super) done: bool,
}

pub struct Find {
    pub(super) field: Field,
    pub(super) active: bool,
}

/// A clickable part of the header, in columns from 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub(super) id: String,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) tab: Option<usize>,
}

/// An option of the view's menu, or an item of the scope menus (b).
#[derive(Clone, Debug, PartialEq)]
pub enum Opt {
    Wrap,
    LineNumbers,
    Changes,
    Edit,
    Markdown,
    Sharp,
    Layout,
    Compare,
    Scope(scope::Pick),
}

pub struct State {
    pub(super) file: String,
    pub(super) root: String,
    pub(super) line: f64,
    pub(super) kind: Kind,
    /// Header details: size, lines.
    pub(super) info: String,
    pub(super) text: String,
    /// Highlighted source lines, and the same lines as plain text, for the find and the copy.
    pub(super) source: Vec<Vec<Seg>>,
    pub(super) plain: Vec<String>,
    /// Image bytes.
    pub(super) buffer: Option<Arc<Vec<u8>>>,
    /// A WebP waiting for its PNG (dwebp), and what to install when the tool is missing.
    pub(super) converting: bool,
    pub(super) missing: Option<String>,
    /// p: images in pixels (true) or half blocks (false) in this pane; None: as the Images setting.
    pub(super) sharp: Option<bool>,
    /// Markdown shown as source.
    pub(super) raw: bool,
    pub(super) rows: Option<Rc<Rows>>,
    pub(super) top: i64,
    pub(super) left: i64,
    pub(super) focused: bool,
    pub(super) find: Option<Find>,
    /// The find's matches: (line, start, end), byte offsets of the line.
    pub(super) matches: Vec<(usize, usize, usize)>,
    pub(super) matched: i64,
    pub(super) select: Option<Select>,
    pub(super) flash: Option<String>,
    pub(super) chips: Vec<Chip>,
    pub(super) marks: Vec<Mark>,
    /// The mark n and N went to last.
    pub(super) mark: usize,
    /// The Diff view, rather than the file.
    pub(super) diff: bool,
    /// The Diff view's changes; None until asked for, or once another file shows.
    pub(super) diff_data: Option<diff::DiffData>,
    /// What the Diff view compares with: the tab's scope (core::scope, b).
    pub(super) scope: Scope,
    /// The field of a commit or range typed for the scope (b, Commit or range…).
    pub(super) rev_prompt: Option<Field>,
    /// The hunk n or N went to in the Diff view, which s, u and D take without a selection.
    pub(super) hunk: Option<(usize, usize)>,
    /// The scroll of a tab shown again in the Diff view, for once its changes are loaded.
    pub(super) want_top: Option<i64>,
    /// A commit, stash or tag shown over the tabs (source control's review), see enter_show.
    pub(super) show: Option<diff::Show>,
    /// The file's lines changed since HEAD; None outside git.
    pub(super) changes: Option<diff::Changes>,
    pub(super) menu: Option<Menu<Opt>>,
    pub(super) help: Option<HelpState>,
    /// The footer's keys, for clicks: none while the find or a message takes the line.
    pub(super) hints: Option<ui::Hints>,
    /// The edit mode (i, see start_edit), shared with its tab.
    pub(super) edit: Option<edit::Shared>,
    /// A question in a card (ui::ask).
    pub(super) ask: Option<edit::Question>,
    /// The tab's review notes on this file (core::notes), its notes on every file, and the note typed.
    pub(super) notes: Vec<Value>,
    pub(super) note_count: usize,
    pub(super) note: Option<review::Draft>,
}
