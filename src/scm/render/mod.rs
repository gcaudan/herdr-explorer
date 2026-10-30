//! Drawing of the source control pane: the rows of each view, the header, tabs, commit box,
//! status line and key help, the diff or branch comparison beside the list, and the branch picker.
//!
//! This file holds the frame, its header, tabs, commit box, status line and key help; rows.rs the rows of the views,
//! picker.rs the branch picker.

mod picker;
mod rows;

use super::context::{self, DiffView, Row, RowRef, VIEWS, base, dir_of};
use super::parse::{self, Commit, Item, Label};
use super::state;
use super::{Act, Hit, Pane};
use crate::core::exec::{self, WIN};
use crate::core::icons;
use crate::core::ui::{self, Rgb, Seg, bg, c, fg, seg, shade};
use crate::diff::{self, View, layout_of};
use crate::json::Value;
use rows::compare_lines;

const BOLD: &str = "\x1b[1m";

/// The active tab, underlined in blue where the terminal can.
const UNDERLINE: &str = "\x1b[4m\x1b[58;2;55;148;255m";

/// Columns of the graph at most: lanes further right are cut.
const MAX_GRAPH: usize = 24;

const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Columns of the list beside the diff: at least 30, and 40 left to the diff.
const MIN_LIST: i64 = 30;

const MIN_DIFF: i64 = 40;

fn dim() -> String {
    fg(c().dim)
}

fn key_style() -> String {
    format!("{BOLD}{}", fg(c().text))
}

/// The main button of the header and the commit box.
fn primary() -> String {
    format!("{}{}{BOLD}", bg([14, 99, 156]), fg([255, 255, 255]))
}

/// A button or the branch while its menu hangs under it.
fn lit() -> String {
    format!("{}{}{BOLD}", bg(c().blue), fg(c().canvas))
}

fn badge_color(code: char) -> String {
    fg(ui::git(code).unwrap_or(c().text))
}

fn red_d() -> String {
    fg(ui::git('D').unwrap_or(c().red))
}

fn chevron(open: bool) -> &'static str {
    if open { ui::g().open } else { ui::g().closed }
}

/// A pill: its colour on a shade of it, close to the theme's background.
fn pill_rgb(kind: &str) -> Rgb {
    match kind {
        "heads" => c().green,
        "remotes" => c().blue,
        _ => c().yellow,
    }
}

fn pill(name: &str, kind: &str, style: &str) -> Seg {
    let rgb = pill_rgb(kind);
    seg(format!(" {name} "), format!("{}{}{style}", bg(shade(rgb)), fg(rgb)))
}

/// Colours of the graph's lanes, in turn; a branch's pill takes the colour of its lane, on a shade of it.
fn lane_rgb(n: usize) -> Rgb {
    let p = c();
    [p.blue, p.green, p.purple, p.orange, p.yellow, [86, 182, 194], [224, 108, 117]][n % 7]
}

fn lane_pill(name: &str, n: usize, style: &str) -> Seg {
    seg(format!(" {name} "), format!("{}{}{style}", bg(shade(lane_rgb(n))), fg(lane_rgb(n))))
}

pub(super) fn spinner() -> char {
    SPIN[(crate::core::clock::now_ms() / 100.0).floor() as usize % SPIN.len()]
}

fn tw(text: &str) -> i64 {
    ui::text_width(text) as i64
}

fn sw(segs: &[Seg]) -> i64 {
    ui::seg_width(segs) as i64
}

fn spaces(n: i64) -> String {
    " ".repeat(n.max(0) as usize)
}

fn pad(text: &str, w: i64) -> String {
    let t = ui::ellipsis(text, w);
    let n = w - tw(&t);
    format!("{t}{}", spaces(n))
}

/// A count as text (NaN included).
fn num(n: f64) -> String {
    if n.is_nan() { "NaN".into() } else { crate::json::number(n) }
}

/// Text right-aligned on n columns.
fn pad_start(text: &str, n: usize) -> String {
    let len = crate::core::ui::text_width(text);
    format!("{}{text}", " ".repeat(n.saturating_sub(len)))
}

/// Windows paths ignore case, and git gives them with '/'.
fn same_path(a: &str, b: &str) -> bool {
    if WIN { a.to_lowercase() == b.to_lowercase() } else { a == b }
}

fn tilde(p: &str) -> String {
    let h = icons::homedir();
    let abs = crate::core::paths::resolve(&[p]);
    let head = abs.get(..h.len() + 1).unwrap_or("");
    if same_path(&abs, &h) || same_path(head, &format!("{h}{}", crate::core::paths::SEP)) {
        format!("~{}", &abs[h.len()..])
    } else {
        p.to_string()
    }
}

/// A line made of a left part, cut to fit, and a right part kept whole (buttons, counts).
pub(super) fn split(left: &[Seg], right: &[Seg], width: i64, back: Option<Rgb>) -> String {
    let rw = tw(&ui::seg_text(right));
    let room = (width - rw).max(0);
    let mut segs: Vec<Seg> = Vec::new();
    let mut used = 0;
    for (text, style) in left {
        let t = ui::clean(text);
        if used + tw(&t) > room {
            segs.push(seg(ui::ellipsis(&t, room - used), style.clone()));
            used = room;
            break;
        }
        used += tw(&t);
        segs.push(seg(t, style.clone()));
    }
    segs.push(seg(spaces(room - used), ""));
    segs.extend(right.iter().cloned());
    ui::line(&segs, width.max(0) as usize, back)
}

/// A button: its label, what a click does, its style ("" for the keys' own).
type Btn = (String, Act, String);

/// Buttons from column x, registered as clickable.
fn place(hits: &mut Vec<Hit>, list: Vec<Btn>, y: i64, mut x: i64) -> Vec<Seg> {
    let mut segs = Vec::new();
    for (label, act, style) in list {
        let w = tw(&label);
        hits.push(Hit { y, x0: x, x1: x + w + 2, act });
        segs.push(seg(format!(" {label} "), if style.is_empty() { key_style() } else { style }));
        x += w + 2;
    }
    segs
}

/// Buttons shown on the right of a line, right_after columns before its end.
fn buttons(hits: &mut Vec<Hit>, list: Vec<Btn>, y: i64, width: i64, right_after: i64) -> Vec<Seg> {
    let w: i64 = list.iter().map(|(l, _, _)| tw(l) + 2).sum();
    place(hits, list, y, width - right_after - w)
}

const RANK: [(char, u8); 6] = [('U', 1), ('A', 2), ('R', 2), ('M', 3), ('D', 4), ('C', 5)];

fn rank_of(code: char) -> Option<u8> {
    RANK.iter().find(|(c, _)| *c == code).map(|(_, r)| *r)
}

/// The change shown for a folder: the strongest of its files'.
fn rank(items: &[Item]) -> char {
    items.iter().fold(
        'M',
        |best, i| {
            if rank_of(i.code).is_some_and(|r| r > rank_of(best).unwrap_or(0)) { i.code } else { best }
        },
    )
}

fn row_buttons(row: &Row) -> Vec<Btn> {
    if !row.has_items() || row.items.is_empty() {
        return Vec::new();
    }
    let items = row.items.clone();
    match row.sec {
        "staged" => vec![("−".into(), Act::Unstage(items), String::new())],
        "conflicts" => vec![("+".into(), Act::Stage(items), String::new())],
        sec => vec![
            ("↶".into(), Act::Discard(items.clone(), sec), String::new()),
            ("+".into(), Act::Stage(items), String::new()),
        ],
    }
}

/// Names typed above the list start with this, by kind (the branch's is in the picker).
fn lead_of(kind: &str) -> Option<&'static str> {
    match kind {
        "stash" => Some("  new        "),
        "tag" => Some("  tag        "),
        "message" => Some("  message    "),
        _ => None,
    }
}

impl Pane {
    // --- rows of the views
    fn select_rgb(&self) -> Rgb {
        if self.s.diff.as_ref().is_some_and(|d| d.focus) { c().select_blur } else { c().select }
    }

    /// A commit's labels, before its subject (parse::labels): a branch at the same commit as its remote one marked ☁,
    /// the current branch in bold; 3 at most, then +N (the commit's preview lists them all). They take room columns
    /// at most, the subject giving way first.
    fn label_segs(&self, c: &Commit, lane: usize, room: i64) -> Vec<Seg> {
        let branch = self.repo().map_or("", |r| if r.status.detached { "" } else { r.status.branch.as_str() });
        let list = parse::labels(&c.refs, c.head, branch);
        let segs_of = |n: usize| {
            let mut out: Vec<Seg> = Vec::new();
            for l in &list[..n] {
                out.push(match l {
                    Label::Tag { name } => pill(name, "tags", ""),
                    Label::Local { name, remote, current } => {
                        let name = if *remote { format!("{name} {}", ui::g().remote) } else { name.clone() };
                        lane_pill(&name, lane, if *current { BOLD } else { "" })
                    }
                    Label::Remote { name } => lane_pill(name, lane, ""),
                });
                out.push(seg(" ", ""));
            }
            if list.len() > n {
                out.push(seg(format!("+{}", list.len() - n), dim()));
                out.push(seg("  ", ""));
            } else {
                out.push(seg(" ", ""));
            }
            out
        };
        let mut n = 3.min(list.len());
        let mut segs = segs_of(n);
        while n > 1 && sw(&segs) > room {
            n -= 1;
            segs = segs_of(n);
        }
        if list.is_empty() || sw(&segs) <= room {
            return if list.is_empty() { Vec::new() } else { segs };
        }
        // A single label still too wide: its name cut.
        let (text, style) = segs[0].clone();
        let over = sw(&segs) - room.max(4);
        let mut out = vec![seg(format!(" {} ", ui::ellipsis(text.trim(), (tw(&text) - 2 - over).max(1))), style)];
        out.extend(segs[1..].iter().cloned());
        out
    }

    fn row_line(&mut self, row: &Row, width: i64, y: i64, selected: bool) -> String {
        // rows just staged or unstaged
        let back = if selected {
            Some(self.select_rgb())
        } else if self.pulse.as_ref().is_some_and(|p| p.contains(&row.key)) {
            Some(c().pulse)
        } else {
            None
        };
        let indent = " ".repeat(row.depth * 2);
        let act = if selected && self.s.view == "changes" { row_buttons(row) } else { Vec::new() };
        let hits = &mut self.frame.hits;
        match row.kind {
            "section" => {
                let mut right = buttons(hits, act, y, width, 1);
                right.push(seg(" ", ""));
                let left = [
                    seg(format!(" {} ", chevron(row.open)), dim()),
                    seg(row.title.to_uppercase(), BOLD),
                    seg(format!("  {}", row.count), dim()),
                ];
                split(&left, &right, width, back)
            }
            "folder" => {
                // A chain of folders shown as one ("src/lib"): the icon of the last.
                let last = row.label.rsplit('/').next().unwrap_or("");
                let (mark, rgb) = icons::icon(self.theme.as_ref(), last, true, row.open);
                let mut right = buttons(hits, act, y, width, 3);
                right.push(seg(" ● ", badge_color(rank(&row.items))));
                let left =
                    [seg(format!("{indent}{} ", chevron(row.open)), dim()), seg(mark, fg(rgb)), seg(&row.label, "")];
                split(&left, &right, width, back)
            }
            "file" => {
                let it = row.item.as_ref().expect("a file row has its item");
                let color = format!("{}{}", badge_color(it.code), if it.code == 'D' { "\x1b[9m" } else { "" });
                let (mark, rgb) = icons::icon(self.theme.as_ref(), base(&it.path), false, false);
                let orig = it.orig.as_deref().filter(|o| !o.is_empty());
                let name = match orig {
                    Some(o) if !self.s.tree => format!("{o} → {}", it.path),
                    _ => base(&it.path).to_string(),
                };
                let dir = match (self.s.tree, orig) {
                    (true, Some(o)) => format!(" ← {}", base(o)),
                    (true, None) | (false, Some(_)) => String::new(),
                    (false, None) => dir_of(&it.path).to_string(),
                };
                let letter = if it.code == 'C' { '!' } else { it.code };
                let mut right = buttons(hits, act, y, width, 3);
                right.push(seg(format!(" {letter} "), badge_color(it.code)));
                let left = [
                    seg(if self.s.tree { format!("{indent}  ") } else { format!("{indent} ") }, ""),
                    seg(mark, fg(rgb)),
                    seg(name, color),
                    seg(if dir.is_empty() { String::new() } else { format!("  {dir}") }, dim()),
                    seg(it.label.map_or(String::new(), |l| format!("  {l}")), dim()),
                ];
                split(&left, &right, width, back)
            }
            "commit" => {
                // Subject first, so that subjects line up; author, age and hash in columns on the right.
                let h = row.commit.as_ref().expect("a commit row has its commit");
                let c = &h.commit;
                let mut right = Vec::new();
                if row.author_w > 0 && width >= 90 {
                    right.push(seg(format!(" {} ", pad(&c.author, row.author_w as i64)), dim()));
                }
                right.push(seg(format!("{}  ", pad_start(&c.date, 6)), dim()));
                right.push(seg(format!("{} ", c.short), fg(self::c().yellow)));
                // The graph, cut at MAX_GRAPH columns and padded so that subjects line up; the commit checked out
                // in bold.
                let mut cells: Vec<Seg> = h
                    .graph
                    .iter()
                    .take(row.graph_w)
                    .map(|&(ch, n)| {
                        if ch == '●' {
                            seg(
                                h.node.to_string(),
                                format!("{}{}", fg(lane_rgb(n.unwrap_or(0))), if c.head { BOLD } else { "" }),
                            )
                        } else {
                            seg(ch.to_string(), n.map_or(String::new(), |n| fg(lane_rgb(n))))
                        }
                    })
                    .collect();
                while cells.len() < row.graph_w {
                    cells.push(seg(" ", ""));
                }
                let room = width - tw(&ui::seg_text(&right)) - row.graph_w as i64 - 3;
                let mut left = vec![seg(" ", "")];
                left.extend(cells);
                left.push(seg("  ", ""));
                left.extend(self.label_segs(c, h.lane, room.div_euclid(2)));
                left.push(seg(&c.subject, if c.head { BOLD } else { "" }));
                split(&left, &right, width, back)
            }
            "stash" => {
                let s = row.stash.as_ref().expect("a stash row has its stash");
                split(
                    &[seg(format!(" {}  ", s.r#ref), fg(c().yellow)), seg(&s.subject, "")],
                    &[seg(format!(" {} ", s.date), dim())],
                    width,
                    back,
                )
            }
            "pull" => {
                let p = row.pull.as_ref().expect("a pull request row has its pull request");
                let (mark, rgb) = match p.checks {
                    "pass" => ("✓", c().green),
                    "fail" => ("✗", c().red),
                    "pending" => ("●", c().yellow),
                    _ => (" ", c().text),
                };
                let mut left = vec![seg(format!(" #{}  ", p.number), fg(c().yellow))];
                if p.draft {
                    left.push(seg("draft ", dim()));
                }
                left.push(seg(&p.title, if row.current { BOLD } else { "" }));
                let review = crate::core::gh::review_text(&p.review);
                let mut right = Vec::new();
                if !review.is_empty() {
                    right.push(seg(format!(" {review} ·"), dim()));
                }
                right.push(seg(format!(" {} ", p.author), dim()));
                right.push(seg(format!("{mark} "), fg(rgb)));
                split(&left, &right, width, back)
            }
            "branch" => {
                let b = row.branch().expect("a branch row has its branch");
                if row.current {
                    let track = if b.upstream.is_empty() {
                        "not published".to_string()
                    } else if b.track.is_empty() {
                        b.upstream.clone()
                    } else {
                        format!("{} {}", b.upstream, b.track)
                    };
                    return split(
                        &[
                            seg(format!("{indent} "), ""),
                            pill(&b.name, "heads", BOLD),
                            seg(format!("  current · {track}"), dim()),
                            seg(format!("  {}", b.subject), dim()),
                        ],
                        &[seg(format!(" {} ", b.date), dim())],
                        width,
                        back,
                    );
                }
                let track = if b.upstream.is_empty() {
                    String::new()
                } else if b.track.is_empty() {
                    format!(" {}", b.upstream)
                } else {
                    format!(" {} {}", b.upstream, b.track)
                };
                split(
                    &[
                        seg(format!("{indent}  "), ""),
                        seg(&b.name, ""),
                        seg(if track.is_empty() { String::new() } else { format!("  {track}") }, dim()),
                        seg(format!("  {}", b.subject), dim()),
                    ],
                    &[seg(format!(" {} ", b.date), dim())],
                    width,
                    back,
                )
            }
            "tag" => {
                let t = row.branch().expect("a tag row has its tag");
                split(
                    &[
                        seg(format!("{indent}  "), ""),
                        seg(&t.name, fg(c().yellow)),
                        seg(format!("  {}", t.subject), dim()),
                    ],
                    &[seg(format!(" {} ", t.date), dim())],
                    width,
                    back,
                )
            }
            "remote" => {
                let Some(RowRef::Remote(u)) = &row.r#ref else { return String::new() };
                split(
                    &[seg(format!("{indent}  "), ""), seg(&u.name, BOLD), seg(format!("  {}", u.url), dim())],
                    &[],
                    width,
                    back,
                )
            }
            "worktree" => {
                let Some(RowRef::Worktree(w)) = &row.r#ref else { return String::new() };
                let top = self.repo().map_or("", |r| r.top.as_str());
                let here = same_path(&crate::core::paths::resolve(&[&w.path]), top);
                split(
                    &[
                        seg(format!("{indent}  "), ""),
                        seg(crate::core::paths::basename(&w.path, None), ""),
                        seg(if here { "  this folder" } else { "" }, dim()),
                        seg(format!("  {}", w.branch), fg(c().blue)),
                        seg(format!("  {}", tilde(&w.path)), dim()),
                    ],
                    &[],
                    width,
                    back,
                )
            }
            "action" => split(&[seg(format!(" {}", row.text), fg(c().blue))], &[], width, back),
            _ => split(&[seg(format!("{indent}{}", row.text), dim())], &[], width, back),
        }
    }

    /// The header's main button: what the branch needs (publish, pull, push), none when it is up to date.
    fn sync_button(&self, short: bool) -> Option<Btn> {
        let r = &self.repo()?.status;
        let spin = |op: &str, label: String| {
            if self.s.net_op.as_deref() == Some(op) {
                format!("{}{}", spinner(), label.chars().skip(1).collect::<String>())
            } else {
                label
            }
        };
        if r.detached || !r.has_head {
            return None;
        }
        let set = |n: f64| n != 0.0 && !n.is_nan();
        if r.upstream.is_empty() {
            let label = if short { "↑ Publish" } else { "↑ Publish branch" };
            return Some((spin("push", label.into()), Act::Push, primary()));
        }
        if set(r.behind) {
            let label = if set(r.ahead) {
                format!("⇅ ↓{} ↑{}", num(r.behind), num(r.ahead))
            } else {
                format!("↓ Pull {}", num(r.behind))
            };
            return Some((spin("pull", label), Act::Pull, primary()));
        }
        if set(r.ahead) {
            return Some((spin("push", format!("↑ Push {}", num(r.ahead))), Act::Push, primary()));
        }
        None
    }

    /// Header: repository, branch (opens the picker), its state against the upstream, and the buttons.
    fn header(&mut self, w: i64) -> String {
        let short = w < 80;
        let top = self.repo().map_or_else(|| self.root.clone(), |r| r.top.clone());
        let mut left = vec![seg(format!(" {} ", crate::core::paths::basename(&top, None)), BOLD)];
        let mut btns: Vec<Btn> = Vec::new();
        if let Some(r) = self.repo().map(|r| r.status.clone()) {
            let name = if r.detached {
                format!("detached {}", r.oid.chars().take(7).collect::<String>())
            } else {
                r.branch.clone()
            };
            let x0 = tw(&left[0].0) + 1;
            let label = format!(" {} {name} ", ui::g().branch);
            self.frame.hits.push(Hit { y: 0, x0, x1: x0 + tw(&label), act: Act::Picker });
            self.frame.pill_x = x0;
            // Lit while its picker hangs under it.
            let open = self.s.picker.as_ref().is_some_and(|p| p.mode.is_none());
            left.push(seg(" ", ""));
            left.push(seg(label, if open { lit() } else { format!("{}{}", bg(shade(c().blue)), fg(c().blue)) }));
            let set = |n: f64| n != 0.0 && !n.is_nan();
            if !r.upstream.is_empty() {
                left.push(if set(r.ahead) || set(r.behind) {
                    seg(format!("  ↑{} ↓{}", num(r.ahead), num(r.behind)), fg(c().yellow))
                } else {
                    seg("  up to date", dim())
                });
            } else if !r.has_head {
                left.push(seg("  no commits yet", dim()));
            } else if !r.detached {
                left.push(seg("  not published", dim()));
            }
            let state = self.repo().map_or("", |r| r.state);
            if !state.is_empty() {
                left.push(seg(format!("  {state}"), format!("{}{BOLD}", fg(c().orange))));
            }
            btns.extend(self.sync_button(short));
            let fetch = if self.s.net_op.as_deref() == Some("fetch") { spinner().to_string() } else { "⇣".into() };
            btns.push((if short { fetch.clone() } else { format!("{fetch} Fetch") }, Act::Fetch, String::new()));
            // The repository's menu (m), lit while it hangs under it.
            let menu_lit = self.menu.is_some() && self.menu_header;
            btns.push((
                if short { "▾" } else { "Actions ▾" }.into(),
                Act::Actions,
                if menu_lit { lit() } else { key_style() },
            ));
        }
        let right = buttons(&mut self.frame.hits, btns, 0, w, 1);
        split(&left, &right, w, Some(c().header))
    }

    fn tabs(&mut self, w: i64) -> String {
        let mut segs = Vec::new();
        let mut x = 0;
        let r = self.repo().map(|r| r.status.clone()).unwrap_or_default();
        for (id, label) in VIEWS {
            let n = match id {
                "changes" => r.conflicts.len() + r.staged.len() + r.changes.len(),
                "stashes" => self.s.stashes.len(),
                _ => 0,
            };
            let count = if n > 0 { format!(" {n}") } else { String::new() };
            let width = tw(&format!("{label}{count}")) + 2;
            // A tab cut off by the list's width takes no click past it.
            if x < w {
                self.frame.hits.push(Hit { y: 1, x0: x, x1: (x + width).min(w), act: Act::View(id) });
            }
            segs.push(seg(" ", ""));
            segs.push(seg(label, if id == self.s.view { format!("{BOLD}{}{UNDERLINE}", fg(c().text)) } else { dim() }));
            segs.push(seg(count, dim()));
            // None after the last, which then fits a list as narrow as its labels.
            if id != VIEWS[VIEWS.len() - 1].0 {
                segs.push(seg("  ", ""));
            }
            x += width + 1;
        }
        // List · Tree, the active one filled (ui::segmented's look); a click on the other switches.
        let right = if self.s.view == "changes" {
            let list = vec![
                ("List".into(), Act::List, ui::switch_style(!self.s.tree)),
                ("Tree".into(), Act::Tree, ui::switch_style(self.s.tree)),
            ];
            buttons(&mut self.frame.hits, list, 1, w, 0)
        } else if self.s.view == "history"
            && let Some(file) = self.s.hist_file.clone()
        {
            let name = format!("{}{}", state::hist_name(&self.top(), &file), state::renamed(&self.s.hist_names));
            let label = format!("{} ✕", ui::ellipsis(&name, (w - x - 8).max(8)));
            buttons(&mut self.frame.hits, vec![(label, Act::AllFiles, fg(c().blue))], 1, w, 0)
        } else {
            Vec::new()
        };
        split(&segs, &right, w, None)
    }

    /// What the commit button does: commit what is staged (or everything), amend, continue or conclude an operation.
    fn commit_label(&self, short: bool) -> String {
        let Some(r) = self.repo() else { return "✓ Commit".into() };
        let o = state::ongoing(&self.s);
        if self.s.amend {
            return "✓ Amend".into();
        }
        if state::continues(&self.s) {
            return if short { "✓ Continue".into() } else { format!("✓ Continue {}", o.map_or("", |o| o.name)) };
        }
        if let Some(o) = o {
            return if short { "✓ Commit".into() } else { format!("✓ Commit {}", o.name) };
        }
        if !r.status.staged.is_empty() {
            return if short { "✓ Commit".into() } else { format!("✓ Commit {} staged", r.status.staged.len()) };
        }
        if !r.status.changes.is_empty() {
            return if short { "✓ Commit all".into() } else { format!("✓ Commit all {}", r.status.changes.len()) };
        }
        "✓ Commit".into()
    }

    /// Commit box, on two lines: the message field, then the commit button, ✧ (message written by Claude), the amend
    /// toggle and, during a merge, rebase, cherry-pick or revert, Abort.
    fn commit_rows(&mut self, w: i64) -> Vec<String> {
        let o = state::ongoing(&self.s);
        let fw = (w - 2).max(4);
        self.frame.hits.push(Hit { y: 2, x0: 0, x1: w, act: Act::Edit });
        let boxed = bg(c().panel);
        let cut = |text: &str| ui::ellipsis(text, fw - 1);
        let field: Vec<Seg> = if self.s.generating {
            vec![seg(cut("✧ generating…"), format!("{}{boxed}", fg(c().purple)))]
        } else if self.s.editing {
            self.s.field.segments(fw - 1, &format!("{boxed}{}", fg(c().text)))
        } else if self.s.field.value.is_empty() {
            let rebasing = self.repo().is_some_and(|r| r.state == "REBASING");
            let text = if self.s.amend {
                "Message (empty: keep the last one)"
            } else if o.is_some() && rebasing {
                "Rebase in progress: continue, or X to abort"
            } else if o.is_some() {
                "Message (empty: the one git prepared)"
            } else {
                "Message  (c)"
            };
            vec![seg(cut(text), format!("{}{boxed}", dim()))]
        } else {
            vec![seg(cut(&self.s.field.value), format!("{}{boxed}", fg(c().text)))]
        };
        let used = sw(&field);
        let mut segs = vec![seg(" ", ""), seg(" ", boxed.clone())];
        segs.extend(field);
        segs.push(seg(spaces(fw - 1 - used), boxed));
        let field_line = ui::line(&segs, w as usize, None);
        let list = |short: bool| -> Vec<Btn> {
            let mut l: Vec<Btn> = vec![
                (self.commit_label(short), Act::Commit, primary()),
                (if short { "✧" } else { "✧ Claude" }.into(), Act::Generate, fg(c().purple)),
                (
                    if self.s.amend { "☑ Amend" } else { "☐ Amend" }.into(),
                    Act::Amend,
                    if self.s.amend { fg(c().text) } else { dim() },
                ),
            ];
            if o.is_some() {
                l.push(("✕ Abort".into(), Act::Abort, fg(c().red)));
            }
            l
        };
        let fits = |l: &[Btn]| l.iter().map(|(t, _, _)| tw(t) + 2).sum::<i64>() < w;
        let long = list(false);
        let btns = if fits(&long) { long } else { list(true) };
        let mut segs = vec![seg(" ", "")];
        segs.extend(place(&mut self.frame.hits, btns, 3, 1));
        vec![field_line, ui::line(&segs, w as usize, None)]
    }

    fn status(&mut self, w: i64, h: i64) -> String {
        let header = Some(c().header);
        if let Some(busy) = &self.s.busy {
            return ui::line(&[seg(format!(" {} {busy}…", spinner()), fg(c().blue))], w as usize, header);
        }
        if self.s.generating {
            return ui::line(&[seg(" ✧ generating…", fg(c().purple))], w as usize, header);
        }
        if let Some((text, error)) = &self.s.flash {
            return ui::line(&ui::message(text, error.then_some(ui::Kind::Error)), w as usize, header);
        }
        // ? opens the full list wherever a key is not typed into a field.
        let right = if self.s.picker.is_some()
            || self.inline.is_some()
            || self.menu.is_some()
            || self.s.editing
            || self.repo().is_none()
        {
            Vec::new()
        } else {
            vec![seg("?", key_style()), seg(if w >= 60 { " all keys " } else { " " }, dim())]
        };
        let rw = sw(&right);
        let hints = ui::keys(&self.hints(), (w - rw - 2).max(0) as usize);
        // A click on a hint does what its key does.
        let y = h - 1;
        for z in &hints.zones {
            self.frame.hits.push(Hit { y, x0: z.x0 as i64, x1: z.x1 as i64, act: Act::Key(z.key.clone()) });
        }
        if rw > 0 {
            self.frame.hits.push(Hit { y, x0: w - rw, x1: w, act: Act::Key("?".into()) });
        }
        let mut segs = hints.segs.clone();
        segs.push(seg(spaces(w - sw(&hints.segs) - rw), ""));
        segs.extend(right);
        ui::line(&segs, w as usize, header)
    }

    /// The keys of the moment, most useful first: as many as fit show in the status line.
    fn hints(&self) -> Vec<(String, String)> {
        let l = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect::<Vec<_>>();
        if let Some(it) = &self.inline {
            return it.keys.clone();
        }
        if self.menu.is_some() {
            return l(&[("↑↓", "move"), ("⏎", "run"), ("esc", "close")]);
        }
        if let Some(p) = &self.s.picker {
            let esc = if p.typing { "clear" } else { "close" };
            if let Some(mode) = p.mode {
                return l(&[("↑↓", "move"), ("⏎", mode), ("/", "filter"), ("esc", esc)]);
            }
            return if p.typing {
                l(&[("↑↓", "move"), ("⏎", "switch"), ("esc", "clear")])
            } else {
                l(&[("⏎", "switch"), ("n", "new branch"), ("D", "delete"), ("/", "filter"), ("esc", "close")])
            };
        }
        let commit_key = if state::continues(&self.s) { "continue" } else { "commit" };
        if self.s.editing {
            return l(&[("⏎", commit_key), ("esc", "leave"), ("ctrl+g", "✧ Claude"), ("alt+a", "amend")]);
        }
        let wide = self.wide();
        if let Some(d) = &self.s.diff
            && (d.focus || !wide)
        {
            if d.compare {
                return l(&[("↑↓", "select"), ("⏎", "show commit"), ("tab", "list"), ("q", "back")]);
            }
            if d.back.is_some() {
                return l(&[("↑↓", "scroll"), ("n N", "hunks"), ("q", "back to the comparison")]);
            }
            return l(&[("↑↓", "scroll"), ("n N", "hunks"), ("tab", "list"), ("q", "back")]);
        }
        if self.repo().is_none() {
            return if self.s.no_git { l(&[("q", "quit")]) } else { l(&[("i", "git init"), ("q", "quit")]) };
        }
        let common =
            l(&[("b", "branch"), ("f", "fetch"), ("P", "push"), ("L", "pull"), ("m", "actions"), ("q", "quit")]);
        let mut out = match self.s.view {
            "changes" => {
                let mut v = l(&[("space", "stage"), ("u", "unstage"), ("d", "discard"), ("c", commit_key)]);
                if state::ongoing(&self.s).is_some() {
                    v.extend(l(&[("X", "abort")]));
                }
                v.extend(l(&[("⏎", if wide { "focus diff" } else { "diff" }), ("ctrl+g", "✧ Claude")]));
                v
            }
            "history" => {
                let mut v = l(&[("⏎", if wide { "focus commit" } else { "show commit" })]);
                if self.s.hist_file.is_some() {
                    v.extend(l(&[("x", "every file")]));
                }
                v.extend(l(&[
                    ("a", if self.s.all_branches { "current branch" } else { "all branches" }),
                    ("t", "tag"),
                    ("C", "cherry-pick"),
                ]));
                v
            }
            "stashes" => l(&[("n", "stash"), ("a", "apply"), ("o", "pop"), ("d", "drop"), ("⏎", "show")]),
            "pulls" => l(&[("⏎", "show"), ("o", "open in browser"), ("C", "check out"), ("r", "refresh")]),
            _ => l(&[
                ("⏎", "checkout"),
                ("M", "merge"),
                ("R", "rebase onto"),
                ("=", "compare"),
                ("B", "compare base"),
                ("D", "delete branch"),
            ]),
        };
        out.extend(common);
        out
    }

    /// Every key, for the help overlay (?): the view's own first.
    fn help_groups(&self) -> Vec<ui::Group> {
        let group = |title: &str, pairs: &[(&str, &str)]| -> ui::Group {
            (title.into(), pairs.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect())
        };
        let mut g = Vec::new();
        match self.s.view {
            "changes" => {
                g.push(group(
                    "Files",
                    &[
                        ("space s", "stage"),
                        ("u", "unstage"),
                        ("a A", "stage, unstage all"),
                        ("d", "discard"),
                        ("t", "list or tree"),
                        ("y", "copy path"),
                        ("Y", "copy relative path"),
                        ("e", "open in the editor"),
                        ("E", "edit in the terminal"),
                        ("o", "open in its app"),
                    ],
                ));
                g.push(group(
                    "Commit",
                    &[
                        ("c", "type the message"),
                        ("⏎", "commit, when typing"),
                        ("ctrl+g", "✧ write with Claude"),
                        ("alt+a", "amend"),
                        ("X", "abort the operation"),
                    ],
                ));
            }
            "history" => g.push(group(
                "History",
                &[
                    ("⏎", "show the commit"),
                    ("a", "all branches or current"),
                    ("x", "every file, after a file history"),
                    ("t", "tag the commit"),
                    ("C", "cherry-pick the commit"),
                    ("m", "more: reset --hard to the commit"),
                ],
            )),
            "stashes" => g.push(group(
                "Stashes",
                &[("n", "stash all changes"), ("a", "apply"), ("o", "pop"), ("d", "drop"), ("⏎", "show")],
            )),
            "pulls" => g.push(group(
                "Pull requests",
                &[
                    ("⏎", "show the pull request"),
                    ("o", "open it in the browser"),
                    ("C", "check it out (gh pr checkout)"),
                    ("r", "refresh the list"),
                ],
            )),
            _ => {
                g.push(group(
                    "Refs",
                    &[
                        ("⏎", "check out"),
                        ("M", "merge into the current branch"),
                        ("R", "rebase the current branch onto it"),
                        ("=", "compare with the base, in the diffs of the tab too"),
                        ("B", "set as compare base"),
                        ("D", "delete the branch"),
                    ],
                ));
                g.push(group("Comparison", &[("↑↓", "select a commit"), ("⏎", "show the commit"), ("q", "back")]));
            }
        }
        g.push(group(
            "Repository",
            &[
                ("f", "fetch"),
                ("L", "pull"),
                ("P", "push or publish"),
                ("b", "branches"),
                ("T", "tag HEAD"),
                ("m", "more: merge, rebase, force push, undo commit, reset --hard"),
                ("r", "refresh"),
            ],
        ));
        g.push(group(
            "Move",
            &[
                ("↑↓ j k", "move"),
                ("← → h l", "fold, unfold"),
                ("pgup pgdn", "page"),
                ("ctrl+u/d", "half page"),
                ("g G", "first, last"),
                ("1-5", "switch view"),
            ],
        ));
        g.push(group(
            "Diff",
            &[
                ("tab", "focus the diff"),
                (",", "the view's options (or ⚙)"),
                ("n N", "next, previous hunk"),
                ("v", "inline, split (side by side)"),
                ("w", "wrap long lines"),
                ("← → h l", "left, right (no wrap)"),
                ("< >", "narrower, wider list"),
                ("shift+← →", "move the separator"),
                ("q", "back to the list"),
            ],
        ));
        g.push(group(
            "Pane",
            &[
                ("ctrl+p", "go to file"),
                ("ctrl+f", "search in files"),
                ("z", "zoom the pane"),
                ("? f1", "keys"),
                ("q esc", "close"),
            ],
        ));
        g
    }

    /// The diff area: a title row and the rows of diff.rs, rendered for its width and cached. Long lines wrap with
    /// the Wrap lines setting (w, shared with the preview and the Changes pane), else they show from column d.left
    /// (← →), the rows cut there made again when drawn (redo).
    fn diff_lines(&mut self, width: i64, height: i64) -> Vec<String> {
        let (wrap, resizing) = (self.wrap, self.s.resizing);
        let menu_view = self.menu.is_some() && self.menu_view;
        let d = self.s.diff.as_mut().expect("a diff to draw");
        if d.compare {
            return compare_lines(d, &mut self.diff_bar, resizing, width, height);
        }
        let mode = layout_of(width.max(0) as usize);
        let key_of = |w: i64| format!("{w}:{mode}:{wrap}");
        // While the separator is dragged, the rows laid out for the old width are cut or padded: laying out a long
        // diff again at each column would make the separator lag behind the mouse. The release lays it out.
        let layout = |d: &mut DiffView, w: i64| {
            if resizing && !d.rows.is_empty() {
                return;
            }
            if let Some(parsed) = &d.parsed
                && d.cache != key_of(w)
            {
                d.rows =
                    diff::render(parsed, w.max(0) as usize, mode, d.item.is_some(), &View { wrap, ..View::default() });
                d.cache = key_of(w);
            }
        };
        let body = height - 1;
        if wrap {
            d.left = 0;
        }
        // One column narrower when the scrollbar shows (a narrower layout never has fewer rows): kept while it does.
        if d.cache != key_of(width - 1) || d.rows.len() as i64 <= body {
            layout(d, width);
        }
        if d.rows.len() as i64 > body {
            layout(d, width - 1);
        }
        d.top = d.top.min(d.rows.len() as i64 - body).max(0);
        let bw = self.diff_bar.set(d.rows.len() as i64, body, d.top, &[]) as i64;
        let mut title = vec![
            seg(format!(" {}", d.title), BOLD),
            seg(if d.tag.is_empty() { String::new() } else { format!("  {}", d.tag) }, dim()),
        ];
        if let Some(p) = &d.parsed {
            title.push(seg(format!("  +{}", p.add), fg(c().green)));
            title.push(seg(format!(" −{}", p.del), red_d()));
        }
        // ⚙, the view's options (,), as in the Changes pane and the preview's Diff view (clicks: switch_hits).
        let right =
            vec![seg(format!(" {} ", ui::g().gear), fg(if menu_view { c().text } else { c().dim })), seg(" ", "")];
        d.switch_at = Some(width - sw(&right));
        d.width = width.max(0) as usize;
        let mut lines = vec![split(&title, &right, width, Some(if d.focus { c().select } else { c().header }))];
        let (w, b) = (width.max(0) as usize, body.max(0) as usize);
        if d.parsed.is_none() {
            lines.extend(ui::empty::<&str, &str>(w, b, "Loading…", &[]));
        } else if d.rows.is_empty() {
            lines.extend(ui::empty::<&str, &str>(w, b, "No changes", &[]));
        }
        let from = (d.top as usize).min(d.rows.len());
        for (k, r) in d.rows[from..(from + b).min(d.rows.len())].iter().enumerate() {
            let segs = match &r.redo {
                Some(redo) if d.left > 0 => redo(d.left),
                _ => r.segs.clone(),
            };
            lines.push(ui::line(&segs, (width - bw).max(0) as usize, r.back) + &self.diff_bar.cell(k as i64));
        }
        while (lines.len() as i64) < height {
            lines.push(ui::line(&[], w, None));
        }
        lines.truncate(height.max(0) as usize);
        lines
    }

    /// The diff's ⚙ in its title row, clickable; x: the diff area's first column.
    fn switch_hits(&mut self, x: i64) {
        let Some(d) = &self.s.diff else { return };
        let Some(at) = d.switch_at.filter(|_| !d.compare) else { return };
        self.frame.hits.push(Hit { y: 1, x0: x + at, x1: x + at + 3, act: Act::DiffView });
    }

    /// The diff area with nothing to show: what to select for it.
    fn empty_diff(&self, width: i64, height: i64) -> Vec<String> {
        let rows = &self.frame.rows;
        let text = match self.s.view {
            "changes" if rows.iter().any(|r| r.kind == "file") => "Select a file to see its changes".to_string(),
            "changes" => "No changes".into(),
            "history" => "Select a commit to see it".into(),
            "stashes" if !self.s.stashes.is_empty() => "Select a stash to see it".into(),
            "stashes" => "No stashes".into(),
            "pulls" if rows.iter().any(|r| r.kind == "pull") => "Select a pull request to see it".into(),
            "pulls" => "No pull request".into(),
            _ if rows.get(self.s.cursor as usize).is_some_and(|r| r.kind == "branch") => {
                format!("{} is the base: select another branch to compare with it", state::base_name(&self.s))
            }
            _ => format!("Select a branch to compare it with {}", state::base_name(&self.s)),
        };
        let mut out = vec![ui::line(&[], width.max(0) as usize, Some(c().header))];
        out.extend(ui::empty::<&str, &str>(width.max(0) as usize, (height - 1).max(0) as usize, &text, &[]));
        out
    }

    fn list_width(&self, w: i64) -> i64 {
        let share = self.s.list_share.iter().find(|(v, _)| v == self.s.view).map(|(_, s)| *s);
        let lw = match share {
            None => 34.max(70.min((w as f64 * 0.38).round() as i64)),
            Some(s) => (w as f64 * s).round() as i64,
        };
        MIN_LIST.max((w - 1 - MIN_DIFF).min(lw))
    }

    /// Sets the list's width in columns and keeps it as a share of the width; saved unless a drag is still going on.
    pub(super) fn resize_list(&mut self, cols: i64, save: bool) {
        let w = self.size().0;
        let share = (MIN_LIST.max((w - 1 - MIN_DIFF).min(cols)) as f64 / w as f64 * 1000.0).round() / 1000.0;
        let view = self.s.view;
        match self.s.list_share.iter_mut().find(|(v, _)| v == view) {
            Some(e) => e.1 = share,
            None => self.s.list_share.push((view.to_string(), share)),
        }
        if save {
            let mut settings = exec::read_settings();
            let mut saved = Value::Obj(Vec::new());
            for (id, s) in context::shares(settings.get("scmListShare")) {
                saved.set(&id, Value::Num(s));
            }
            saved.set(view, Value::Num(share));
            settings.set("scmListShare", saved);
            exec::write_settings(&settings);
        }
        self.draw();
    }

    pub(super) fn draw(&mut self) {
        let (w, h) = self.size();
        self.frame.hits.clear();
        let mut lines = vec![self.header(w)];
        let (wu, body) = (w as usize, (h - 2).max(0) as usize);
        if self.repo().is_none() {
            lines.extend(match self.s.repo {
                context::RepoState::Loading => ui::empty::<&str, &str>(wu, body, "Loading…", &[]),
                _ if self.s.no_git => ui::empty::<&str, &str>(wu, body, "git not found in PATH", &[]),
                _ => ui::empty(wu, body, "Not a git repository", &[("i", "git init here")]),
            });
        } else if self.s.diff.is_some() && !self.wide() {
            lines.extend(self.diff_lines(w, h - 2));
            self.switch_hits(0);
            self.frame.list_w = 0;
        } else {
            // From SPLIT columns, the diff of the selection always shows on the right.
            let lw = if self.wide() { self.list_width(w) } else { w };
            let mut left = vec![self.tabs(lw)];
            if self.s.view == "changes" {
                left.extend(self.commit_rows(lw));
                if h >= 20 {
                    left.push(ui::line(&[], lw as usize, None));
                }
            }
            self.frame.list_y = 1 + left.len() as i64;
            self.frame.list_h = (h - 1 - self.frame.list_y).max(1);
            self.frame.list_w = lw;
            let rows = self.view_rows();
            self.frame.rows = rows.clone();
            let (list_y, list_h) = (self.frame.list_y, self.frame.list_h);
            self.sync_cursor();
            self.s.top = self.s.top.min(rows.len() as i64 - list_h).max(0);
            let bw = self.list_bar.set(rows.len() as i64, list_h, self.s.top, &[]) as i64;
            // A list with nothing in it but a note: the sections' titles, then the note centred in the rest
            // (ui::empty).
            let notes: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].kind == "note").collect();
            let lone =
                (notes.len() == 1 && rows.iter().all(|r| r.kind == "note" || r.kind == "section")).then(|| notes[0]);
            let top = self.s.top;
            for i in top..(rows.len() as i64).min(top + list_h) {
                if Some(i as usize) == lone {
                    continue;
                }
                let line = self.row_line(&rows[i as usize], lw - bw, list_y + i - top, i == self.s.cursor);
                left.push(line + &self.list_bar.cell(i - top));
            }
            if let Some(n) = lone {
                let text = &rows[n].text;
                let pairs: &[(&str, &str)] =
                    if text == "No changes" { &[("ctrl+p", "go to file"), ("2", "history")] } else { &[] };
                left.extend(ui::empty(lw as usize, (h - 2 - left.len() as i64).max(1) as usize, text, pairs));
            }
            // The stash or the tag being named: a row above the list.
            if let Some(lead) = self.inline.as_ref().and_then(|it| lead_of(it.kind)) {
                let row = self.inline_row(lw, lead, None);
                left.insert(((list_y - 1).max(0) as usize).min(left.len()), row);
                left.truncate((h - 2).max(0) as usize);
            }
            while (left.len() as i64) < h - 2 {
                left.push(ui::line(&[], lw as usize, None));
            }
            if lw < w {
                // The diff stays while there is room for it: no close button.
                let right = if self.s.diff.is_some() {
                    self.diff_lines(w - lw - 1, h - 2)
                } else {
                    self.empty_diff(w - lw - 1, h - 2)
                };
                if self.s.diff.is_some() {
                    self.switch_hits(lw + 1);
                }
                let bar = format!("{}│\x1b[0m", if self.s.resizing { fg(c().blue) } else { dim() });
                for (i, l) in left.into_iter().enumerate() {
                    lines.push(format!("{l}{bar}{}", right.get(i).map_or("", String::as_str)));
                }
            } else {
                lines.extend(left);
            }
        }
        if self.s.picker.is_some() {
            self.draw_picker(&mut lines, w, h);
        }
        let last = (h - 1).max(0) as usize;
        while lines.len() < last {
            lines.push(ui::line(&[], wu, None));
        }
        lines.truncate(last);
        lines.push(self.status(w, h));
        if let Some(m) = self.menu.as_mut() {
            m.draw(&mut lines, w, h);
        }
        if let Some((q, _)) = self.confirm.as_mut() {
            ui::confirm(&mut lines, w, h, q);
        }
        if self.help.is_some() {
            let title = VIEWS.iter().find(|v| v.0 == self.s.view).map_or("", |v| v.1);
            let groups = self.help_groups();
            if let Some(state) = self.help.as_mut() {
                ui::help(&mut lines, wu, title, &groups, state);
            }
        }
        self.screen.paint(&lines);
    }

    /// The selection follows its row (same file, commit or branch) when the rows change.
    fn sync_cursor(&mut self) {
        let rows = &self.frame.rows;
        if let Some(key) = &self.s.sel_key
            && rows.get(self.s.cursor as usize).is_none_or(|r| &r.key != key)
            && let Some(i) = rows.iter().position(|r| &r.key == key)
        {
            self.s.cursor = i as i64;
        }
        self.s.cursor = self.s.cursor.min(rows.len() as i64 - 1).max(0);
        self.s.sel_key = rows.get(self.s.cursor as usize).map(|r| r.key.clone());
    }
}

/// The diff's options (⚙ or , in the diff), as in the preview's Diff view, kept for later panes: the layout switched.
pub(super) fn switch_layout(width: usize) {
    diff::save_mode(width, if layout_of(width) == "split" { "inline" } else { "split" });
}
