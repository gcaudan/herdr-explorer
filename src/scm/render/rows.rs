//! The rows of each view of the source control pane (changes, tree, refs, history), and the rows of a branch
//! comparison.

use super::{BOLD, MAX_GRAPH, dim, num, pad, pad_start, red_d, split};
use crate::core::ui::{self, Seg, c, fg, seg};
use crate::scm::Pane;
use crate::scm::context::{CmpRow, DiffView, Row, RowRef};
use crate::scm::parse::{self, Item};

impl Pane {
    // --- rows of the views
    fn change_rows(&self) -> Vec<Row> {
        let Some(r) = self.repo() else { return Vec::new() };
        let s = &r.status;
        let mut out = Vec::new();
        for (sec, title) in [("conflicts", "Merge Changes"), ("staged", "Staged Changes"), ("changes", "Changes")] {
            let items = r.section(sec);
            if items.is_empty() && sec != "changes" {
                continue;
            }
            let open = !self.s.collapsed.contains(sec);
            out.push(Row {
                kind: "section",
                key: sec.into(),
                sec,
                title: title.into(),
                count: items.len(),
                items: items.to_vec(),
                open,
                ..Row::default()
            });
            if !open {
                continue;
            }
            if items.is_empty() && s.staged.is_empty() && s.conflicts.is_empty() {
                out.push(Row {
                    kind: "note",
                    key: "none".into(),
                    text: "No changes".into(),
                    depth: 1,
                    ..Row::default()
                });
            } else if self.s.tree {
                self.tree_rows(&mut out, sec, items);
            } else {
                for it in items {
                    out.push(file_row(sec, it, 1));
                }
            }
        }
        out
    }

    /// Folders of a section, single-child chains compacted as in VS Code (src/lib).
    fn tree_rows(&self, out: &mut Vec<Row>, sec: &'static str, items: &[Item]) {
        #[derive(Default)]
        struct Node {
            dirs: Vec<(String, Node)>,
            files: Vec<Item>,
        }
        let mut root = Node::default();
        for it in items {
            let parts: Vec<&str> = it.path.split('/').collect();
            let mut n = &mut root;
            for part in &parts[..parts.len() - 1] {
                let i = match n.dirs.iter().position(|(k, _)| k == part) {
                    Some(i) => i,
                    None => {
                        n.dirs.push((part.to_string(), Node::default()));
                        n.dirs.len() - 1
                    }
                };
                n = &mut n.dirs[i].1;
            }
            n.files.push(it.clone());
        }
        fn all(n: &Node, out: &mut Vec<Item>) {
            for (_, d) in &n.dirs {
                all(d, out);
            }
            out.extend(n.files.iter().cloned());
        }
        fn walk(p: &Pane, out: &mut Vec<Row>, sec: &'static str, n: &Node, prefix: &str, depth: usize) {
            let mut dirs: Vec<&(String, Node)> = n.dirs.iter().collect();
            dirs.sort_by(|a, b| crate::unicode::natural_cmp(&a.0, &b.0));
            for (name, d) in dirs {
                let (mut label, mut dir, mut full) = (name.clone(), d, format!("{prefix}{name}"));
                while dir.dirs.len() == 1 && dir.files.is_empty() {
                    let (k, v) = &dir.dirs[0];
                    label = format!("{label}/{k}");
                    full = format!("{full}/{k}");
                    dir = v;
                }
                let key = format!("{sec}:{full}/");
                let open = !p.s.collapsed.contains(&key);
                let mut items = Vec::new();
                all(dir, &mut items);
                out.push(Row {
                    kind: "folder",
                    key,
                    sec,
                    label,
                    count: items.len(),
                    items,
                    depth,
                    open,
                    ..Row::default()
                });
                if open {
                    walk(p, out, sec, dir, &format!("{full}/"), depth + 1);
                }
            }
            for it in &n.files {
                out.push(file_row(sec, it, depth));
            }
        }
        walk(self, out, sec, &root, "", 1);
    }

    fn ref_rows(&self) -> Vec<Row> {
        let Some(refs) = &self.s.refs else {
            return vec![Row {
                kind: "note",
                key: "loading".into(),
                text: "Loading…".into(),
                depth: 1,
                ..Row::default()
            }];
        };
        let current = self.repo().map_or("", |r| r.status.branch.as_str());
        let mut out = Vec::new();
        let mut empty: Vec<String> = Vec::new();
        // Empty sections are named together on one line at the end.
        let mut section = |key: &str, title: &str, list: Vec<(String, Row)>| {
            if list.is_empty() {
                empty.push(title.to_lowercase());
                return;
            }
            let open = !self.s.collapsed.contains(&format!("refs:{key}"));
            out.push(Row {
                kind: "section",
                key: format!("refs:{key}"),
                title: title.into(),
                count: list.len(),
                open,
                ..Row::default()
            });
            if open {
                for (name, row) in list {
                    out.push(Row { key: format!("{key}:{name}"), depth: 1, ..row });
                }
            }
        };
        let branch = |b: &parse::Branch, kind: &'static str, current: bool| {
            (b.name.clone(), Row { kind, r#ref: Some(RowRef::Branch(b.clone())), current, ..Row::default() })
        };
        section("heads", "Local Branches", refs.heads.iter().map(|b| branch(b, "branch", b.name == current)).collect());
        section("remotes", "Remote Branches", refs.remotes.iter().map(|b| branch(b, "branch", false)).collect());
        section("tags", "Tags", refs.tags.iter().map(|t| branch(t, "tag", false)).collect());
        section(
            "urls",
            "Remotes",
            refs.urls
                .iter()
                .map(|u| {
                    (u.name.clone(), Row { kind: "remote", r#ref: Some(RowRef::Remote(u.clone())), ..Row::default() })
                })
                .collect(),
        );
        section(
            "worktrees",
            "Worktrees",
            refs.worktrees
                .iter()
                .map(|w| {
                    (
                        w.path.clone(),
                        Row { kind: "worktree", r#ref: Some(RowRef::Worktree(w.clone())), ..Row::default() },
                    )
                })
                .collect(),
        );
        if !empty.is_empty() {
            let names = if empty.len() > 1 {
                format!("{} or {}", empty[..empty.len() - 1].join(", "), empty[empty.len() - 1])
            } else {
                empty[0].clone()
            };
            out.push(Row {
                kind: "note",
                key: "empty".into(),
                text: format!("No {names}"),
                depth: 1,
                ..Row::default()
            });
        }
        out
    }

    pub(in crate::scm) fn view_rows(&self) -> Vec<Row> {
        let Some(repo) = self.repo() else { return Vec::new() };
        match self.s.view {
            "changes" => self.change_rows(),
            "history" => {
                let h = &self.s.history;
                if h.is_empty() {
                    let text = if !repo.status.has_head {
                        "No commits yet"
                    } else if self.s.hist_loaded {
                        "No commits for this file"
                    } else {
                        "Loading…"
                    };
                    return vec![Row {
                        kind: "note",
                        key: "none".into(),
                        text: text.into(),
                        depth: 1,
                        ..Row::default()
                    }];
                }
                let author_w = 20.min(h.iter().map(|c| ui::text_width(&c.commit.author)).max().unwrap_or(0));
                let graph_w = MAX_GRAPH.min(h.iter().map(|c| c.graph.len()).max().unwrap_or(0).max(1));
                h.iter()
                    .map(|c| Row {
                        kind: "commit",
                        key: c.commit.hash.clone(),
                        commit: Some(Box::new(c.clone())),
                        author_w,
                        graph_w,
                        ..Row::default()
                    })
                    .collect()
            }
            "stashes" => {
                let mut out = vec![Row {
                    kind: "action",
                    key: "new".into(),
                    text: "+ Stash all changes…  (n)".into(),
                    ..Row::default()
                }];
                out.extend(self.s.stashes.iter().map(|s| Row {
                    kind: "stash",
                    key: s.r#ref.clone(),
                    stash: Some(s.clone()),
                    ..Row::default()
                }));
                out
            }
            "pulls" => {
                let note = |text: &str| Row {
                    kind: "note",
                    key: "none".into(),
                    text: text.into(),
                    depth: 1,
                    ..Row::default()
                };
                let pulls = match &self.s.pulls {
                    None => return vec![note("Loading…")],
                    Some(Err(e)) => return vec![note(e)],
                    Some(Ok(p)) if p.is_empty() && self.s.pulls_loading => return vec![note("Loading…")],
                    Some(Ok(p)) if p.is_empty() => return vec![note("No open pull requests")],
                    Some(Ok(p)) => p,
                };
                // The current branch's first.
                let branch = crate::scm::state::head_name(repo);
                let mut rows: Vec<Row> = pulls
                    .iter()
                    .map(|p| Row {
                        kind: "pull",
                        key: format!("#{}", p.number),
                        current: !branch.is_empty() && p.branch == branch,
                        pull: Some(p.clone()),
                        ..Row::default()
                    })
                    .collect();
                rows.sort_by_key(|r| !r.current);
                rows
            }
            _ => self.ref_rows(),
        }
    }
}

fn file_row(sec: &'static str, it: &Item, depth: usize) -> Row {
    Row {
        kind: "file",
        key: format!("{sec}:{}", it.path),
        sec,
        item: Some(it.clone()),
        items: vec![it.clone()],
        count: 1,
        depth,
        ..Row::default()
    }
}

/// A comparison in the diff area: its title row, then its rows (selected commit highlighted).
pub(super) fn compare_lines(
    d: &mut DiffView,
    bar: &mut ui::Scrollbar,
    resizing: bool,
    width: i64,
    height: i64,
) -> Vec<String> {
    let body = height - 1;
    if !resizing && d.cache != width.to_string() {
        d.cmp_rows = compare_rows(d, width - 1);
        d.cache = width.to_string();
    }
    d.top = d.top.min(d.cmp_rows.len() as i64 - body).max(0);
    let bw = bar.set(d.cmp_rows.len() as i64, body, d.top, &[]) as i64;
    let state: Vec<Seg> = match &d.cmp {
        Some(c) if c.failed.is_none() => {
            let style = if c.ahead != 0.0 || c.behind != 0.0 { fg(self::c().yellow) } else { dim() };
            vec![seg(format!("{} ahead · {} behind ", num(c.ahead), num(c.behind)), style)]
        }
        _ => Vec::new(),
    };
    let title = [seg(format!(" {}", d.title), BOLD), seg("  ↔  ", dim()), seg(&d.base, BOLD)];
    let mut lines = vec![split(&title, &state, width, Some(if d.focus { c().select } else { c().header }))];
    let (w, b) = (width.max(0) as usize, body.max(0) as usize);
    if d.cmp.is_none() {
        lines.extend(ui::empty::<&str, &str>(w, b, "Loading…", &[]));
    }
    let from = (d.top as usize).min(d.cmp_rows.len());
    for (k, r) in d.cmp_rows[from..(from + b).min(d.cmp_rows.len())].iter().enumerate() {
        let sel =
            if d.top + k as i64 == d.sel { Some(if d.focus { c().select } else { c().select_blur }) } else { r.back };
        lines.push(ui::line(&r.segs, (width - bw).max(0) as usize, sel) + &bar.cell(k as i64));
    }
    while (lines.len() as i64) < height {
        lines.push(ui::line(&[], w, None));
    }
    lines.truncate(height.max(0) as usize);
    lines
}

/// The rows of a comparison for a width; the commit rows can be selected (Enter shows them).
fn compare_rows(d: &DiffView, width: i64) -> Vec<CmpRow> {
    let Some(c) = &d.cmp else { return Vec::new() };
    let row = |segs: Vec<Seg>| CmpRow { segs, ..CmpRow::default() };
    if let Some(failed) = &c.failed {
        return vec![row(vec![seg(format!(" {failed}"), fg(self::c().red))])];
    }
    let mut out = Vec::new();
    let section = |out: &mut Vec<CmpRow>, title: String, n: String| {
        out.push(CmpRow {
            segs: vec![seg(format!(" {title}"), BOLD), seg(format!("  {n}"), dim())],
            back: Some(self::c().section),
            commit: None,
        });
    };
    let commits = |out: &mut Vec<CmpRow>, list: &[parse::Compared], total: f64| {
        if list.is_empty() {
            out.push(row(vec![seg("   none", dim())]));
        }
        for x in list {
            let date = format!(" {} ", pad_start(&x.date, 6));
            let room = (width - crate::core::ui::text_width(&date) as i64 - 13).max(1);
            out.push(CmpRow {
                commit: Some(x.clone()),
                segs: vec![
                    seg(if x.same { "   = " } else { "     " }, dim()),
                    seg(format!("{}  ", x.short), fg(self::c().yellow)),
                    seg(pad(&x.subject, room), if x.same { dim() } else { String::new() }),
                    seg(date, dim()),
                ],
                back: None,
            });
        }
        if total > list.len() as f64 {
            out.push(row(vec![seg(format!("   … and {} more", num(total - list.len() as f64)), dim())]));
        }
        out.push(row(Vec::new()));
    };
    section(&mut out, format!("Only on {}", d.title), num(c.ahead));
    commits(&mut out, &c.mine, c.ahead);
    section(&mut out, format!("Only on {}", d.base), num(c.behind));
    commits(&mut out, &c.theirs, c.behind);
    let n = |v: &str| if v == "-" { 0.0 } else { v.trim().parse().unwrap_or(f64::NAN) };
    let add: f64 = c.files.iter().map(|f| n(&f.add)).sum();
    let del: f64 = c.files.iter().map(|f| n(&f.del)).sum();
    section(&mut out, "Files changed".into(), format!("{}  +{} −{}", c.files.len(), num(add), num(del)));
    if c.files.is_empty() {
        out.push(row(vec![seg("   none", dim())]));
    }
    for f in &c.files {
        let binary = f.add == "-";
        let counts = if binary { " binary ".to_string() } else { format!(" +{} −{} ", f.add, f.del) };
        let mut segs = vec![
            seg("   ", ""),
            seg(pad(&f.path, (width - crate::core::ui::text_width(&counts) as i64 - 3).max(1)), ""),
        ];
        if binary {
            segs.push(seg(counts, dim()));
        } else {
            segs.push(seg(format!(" +{}", f.add), fg(self::c().green)));
            segs.push(seg(format!(" −{} ", f.del), red_d()));
        }
        out.push(row(segs));
    }
    out
}
