//! What the modules of the source control pane share (src/scm/context.js): its folder, settings and constants, the
//! state S, and the small helpers of names. The JS's late-bound drawing and selection are the pane's own methods.

use super::parse::{Branch, Commit, Item, Refs, Remote, Stash, Status, Worktree};
use crate::core::exec;
use crate::core::ui::Field;
use crate::diff::Diff;
use crate::json::Value;
use std::collections::HashSet;
use std::sync::Arc;

/// The pane's folder: EXPLORER_ROOT, else its argument, else the working directory.
pub fn root(args: &[String]) -> String {
    let given = exec::env_var("EXPLORER_ROOT").filter(|v| !v.is_empty()).or_else(|| args.first().cloned());
    crate::core::paths::resolve(&[given.as_deref().unwrap_or("")])
}

/// A review (actions.scm): the tab reviewed. The pane then shares a tab of its own with the preview of the files
/// chosen, and the diff never opens on the right of the list.
pub fn review() -> String {
    exec::env_var("EXPLORER_REVIEW").unwrap_or_default()
}

/// Columns from which the diff opens on the right of the list (never in a review).
pub fn split(review: &str) -> f64 {
    if review.is_empty() { 100.0 } else { f64::INFINITY }
}

/// Characters of diff sent to Claude.
pub const MAX_PROMPT: usize = 60000;
pub const NET_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(120000);
pub const VIEWS: [(&str, &str); 4] =
    [("changes", "Changes"), ("history", "History"), ("stashes", "Stashes"), ("refs", "Refs")];

/// Share of the width taken by the list beside the diff, per view ({ changes: 0.3, history: 0.6 }); a view left out
/// has the default width.
pub fn shares(saved: Option<&Value>) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if let Some(saved) = saved.filter(|s| matches!(s, Value::Obj(_) | Value::Arr(_))) {
        for (id, _) in VIEWS {
            if let Some(Value::Num(n)) = saved.get(id)
                && *n > 0.0
                && *n < 1.0
            {
                out.push((id.to_string(), *n));
            }
        }
    }
    out
}

/// The repository: its folders, its status (status.rs) and the operation under way ("MERGING"…, "" for none).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Repo {
    pub top: String,
    pub gitdir: String,
    pub common: String,
    pub status: Status,
    pub state: &'static str,
}

impl Repo {
    /// A section's files: "conflicts", "staged" or "changes".
    pub fn section(&self, sec: &str) -> &[Item] {
        match sec {
            "conflicts" => &self.status.conflicts,
            "staged" => &self.status.staged,
            _ => &self.status.changes,
        }
    }
}

/// S.repo: null until loaded, false outside a repository.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum RepoState {
    #[default]
    Loading,
    None,
    Some(Box<Repo>),
}

impl RepoState {
    pub fn get(&self) -> Option<&Repo> {
        match self {
            RepoState::Some(r) => Some(r),
            _ => None,
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut Repo> {
        match self {
            RepoState::Some(r) => Some(r),
            _ => None,
        }
    }
}

/// A commit of the History view, with its graph: its cells, its lane's colour and its node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hist {
    pub commit: Commit,
    pub graph: Vec<(char, Option<usize>)>,
    pub lane: usize,
    pub node: char,
}

/// What a row of the Refs view stands for.
#[derive(Debug, Clone, PartialEq)]
pub enum RowRef {
    Branch(Branch),
    Remote(Remote),
    Worktree(Worktree),
}

/// A row of the current view (render.js builds them): its type ("section", "folder", "file", "note", "branch", "tag",
/// "remote", "worktree", "commit", "stash", "action"), key, what it stands for, and whether it is open or current.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub kind: &'static str,
    pub key: String,
    pub r#ref: Option<RowRef>,
    pub current: bool,
    pub open: bool,
}

impl Row {
    pub fn branch(&self) -> Option<&Branch> {
        match &self.r#ref {
            Some(RowRef::Branch(b)) => Some(b),
            _ => None,
        }
    }
}

/// A branch comparison's result: the counts, the commits only on each side, the files changed, git's error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cmp {
    pub ahead: f64,
    pub behind: f64,
    pub mine: Vec<super::parse::Compared>,
    pub theirs: Vec<super::parse::Compared>,
    pub files: Vec<super::parse::Numstat>,
    pub failed: Option<String>,
}

/// The diff open beside or over the list: a file's, a commit's, a stash's (parsed), or a branch comparison
/// (compare). item: the file it follows; tag: "staged", "untracked", "conflict" or ""; back: the comparison a commit
/// was opened from; auto: opened by the selection itself.
#[derive(Debug, Clone, Default)]
pub struct DiffView {
    pub key: String,
    pub title: String,
    pub item: Option<Item>,
    pub tag: &'static str,
    pub parsed: Option<Arc<Diff>>,
    pub top: i64,
    pub left: usize,
    pub focus: bool,
    pub cache: String,
    pub auto: bool,
    pub back: Option<Box<DiffView>>,
    pub compare: bool,
    pub base: String,
    pub sel: i64,
    pub cmp: Option<Cmp>,
}

/// A branch of the picker: a local one (current: checked out) or a remote one.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub branch: Branch,
    pub current: bool,
}

/// An entry of the picker: "New branch…" (the switch picker only) or a branch.
#[derive(Debug, Clone, PartialEq)]
pub enum PickItem {
    Create(String),
    Branch(Pick),
}

/// The branch picker: mode "merge" or "rebase" picks the branch to merge or rebase onto, else it switches. anchor:
/// the right edge (column from 0, excluded) and top line of the menu that opened it; else under the header's branch.
#[derive(Debug, Clone, Default)]
pub struct Picker {
    pub mode: Option<&'static str>,
    pub anchor: Option<(i64, i64)>,
    pub field: Field,
    pub typing: bool,
    pub sel: i64,
    pub top: i64,
    pub all: Vec<Pick>,
    pub items: Vec<PickItem>,
}

/// The state S of the pane. The JS's confirm, inline, menu, lastClick and pulse are the pane's (render and input).
#[derive(Debug, Clone, Default)]
pub struct State {
    pub view: &'static str,
    pub repo: RepoState,
    /// git is not installed.
    pub no_git: bool,
    pub tree: bool,
    pub collapsed: HashSet<String>,
    pub cursor: i64,
    pub top: i64,
    pub sel_key: Option<String>,
    pub field: Field,
    pub editing: bool,
    pub amend: bool,
    pub generating: bool,
    pub diff: Option<DiffView>,
    pub picker: Option<Picker>,
    /// The status line's message, and whether it is an error.
    pub flash: Option<(String, bool)>,
    pub busy: Option<String>,
    pub help: bool,
    /// fetch, pull or push running: its header button spins.
    pub net_op: Option<String>,
    /// Share of the width taken by the list beside the diff, per view, set by dragging the separator or with < and >.
    pub list_share: Vec<(String, f64)>,
    pub resizing: bool,
    /// Refs: the branch the others are compared with, None for the current one.
    pub compare_base: Option<Branch>,
    /// History: every branch and remote branch, or the current one.
    pub all_branches: bool,
    /// History: the commits of one file only (its absolute path), from the explorer's or the preview's File history.
    pub hist_file: Option<String>,
    /// The names the file had, from git log --follow, its current one first.
    pub hist_names: Vec<String>,
    pub hist_loaded: bool,
    pub history: Vec<Hist>,
    pub stashes: Vec<Stash>,
    pub refs: Option<Refs>,
}

impl State {
    /// The state at startup, from the settings and the variables the explorer set.
    pub fn new(settings: &Value) -> State {
        let hist_file = exec::env_var("EXPLORER_HISTORY").filter(|v| !v.is_empty());
        State {
            view: if hist_file.is_some() { "history" } else { "changes" },
            tree: settings.get("scmTree") == Some(&Value::Bool(true)),
            list_share: shares(settings.get("scmListShare")),
            all_branches: settings.get("scmGraphAll") != Some(&Value::Bool(false)),
            hist_file,
            ..State::default()
        }
    }

    pub fn repo(&self) -> Option<&Repo> {
        self.repo.get()
    }
}

/// A path's last part.
pub fn base(p: &str) -> &str {
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// A path's folder, "" for none.
pub fn dir_of(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

pub fn files(n: usize) -> String {
    format!("{n} file{}", if n > 1 { "s" } else { "" })
}
