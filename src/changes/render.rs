//! Drawing of the Changes pane: the header and its switches, the rows of the diff with the cursor, selection and
//! notes, the footer (keys, a question, a note being typed), the menus and the key help.

use super::Pane;
use super::state::{Chip, name_of, note_where};
use crate::core::ui::{self, Hints, Seg, c, fg, seg};
use crate::core::{paths, scope};
use crate::diff::{File, Row, View, layout_of, render};

/// A switch of the header: its segments, its width, its choices (id, from, to in columns from 0).
type Switch = (Vec<Seg>, usize, Vec<(&'static str, usize, usize)>);

pub(super) fn dim() -> String {
    fg(c().dim)
}
fn green() -> String {
    fg(c().green)
}
fn red() -> String {
    fg(ui::git('D').unwrap_or(c().red))
}
pub(super) fn cyan() -> String {
    fg(c().blue)
}
pub(super) fn note_style() -> String {
    fg(c().yellow)
}

fn help_groups() -> Vec<ui::Group> {
    let group = |title: &str, pairs: &[(&str, &str)]| -> ui::Group {
        (title.into(), pairs.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect())
    };
    vec![
        group(
            "Move",
            &[
                ("↑↓ j k", "line"),
                ("pgup pgdn", "page"),
                ("space", "page down"),
                ("ctrl+u/d", "half page"),
                ("g G", "top, bottom"),
            ],
        ),
        group(
            "Diff",
            &[
                (",", "the view's options (or ⚙)"),
                ("n N", "next, previous hunk"),
                ("v", "inline, split (side by side)"),
                ("w", "wrap long lines"),
                ("← → h l", "left, right (no wrap)"),
                ("b", "compare with: HEAD, merge base, last agent turn, a commit or range, a branch"),
            ],
        ),
        group(
            "Stage (HEAD)",
            &[
                ("shift+↑↓ J K", "select lines"),
                ("s", "stage the line, hunk (@@ row) or file (name row)"),
                ("u", "unstage it"),
                ("D", "discard it from the working tree"),
            ],
        ),
        group(
            "Review",
            &[
                ("a", "note on the line (hunk, file)"),
                ("tab", "kind of note, while typing"),
                ("A", "send the notes to the agent"),
                ("C", "copy the notes"),
                ("U", "remove the last note"),
                ("y", "copy path:line"),
                ("m", "mark the file viewed: folded until it changes"),
                ("] [", "next, previous file not viewed"),
            ],
        ),
        group(
            "Pane",
            &[
                ("ctrl+p", "go to file"),
                ("ctrl+f", "search in files"),
                ("z", "zoom the pane"),
                ("? f1", "keys"),
                ("q esc", "close"),
            ],
        ),
    ]
}

impl Pane {
    /// The layout's key: left is not in it, the rows cut from another column are made again when drawn (redo).
    pub(super) fn key_of(&self, width: usize, m: &str) -> String {
        format!("{width}:{m}:{}:{}", self.wrap, self.viewed.join("\0"))
    }

    pub(super) fn layout(&mut self, width: usize, m: &str) {
        let key = self.key_of(width, m);
        let Some(parsed) = self.parsed.clone() else { return };
        if key == self.cache {
            return;
        }
        // The cursor stays on the same lines when the layout changes.
        let at = |r: &Row| r.r#ref.is_some() && !r.more;
        let was = usize::try_from(self.cur).ok().and_then(|i| self.rows.get(i));
        let (was, hunk) = (was.and_then(|r| r.r#ref.clone()), was.is_some_and(|r| r.hunk));
        let viewed = &self.viewed;
        let folded = |f: &File| name_of(f).is_some_and(|n| viewed.iter().any(|v| v == n));
        let view = View { wrap: self.wrap, left: 0, viewed: Some(&folded) };
        self.rows = render(&parsed, width, m, false, &view);
        self.cache = key;
        let rows = &self.rows;
        let mut i = match (self.stay, &was) {
            (Some(stay), _) => rows.iter().enumerate().position(|(k, r)| k as i64 >= stay && at(r)),
            (None, Some(was)) => rows.iter().position(|r| {
                at(r)
                    && r.r#ref.as_ref().is_some_and(|x| x.file == was.file && x.from == was.from && x.sec == was.sec)
                    && r.hunk == hunk
            }),
            (None, None) => None,
        };
        if i.is_none() && self.stay.is_some() {
            i = rows.iter().rposition(at);
        }
        self.cur = i.or_else(|| rows.iter().position(|r| r.r#ref.is_some())).map_or(-1, |i| i as i64);
        self.stay = None;
        self.anchor = None;
    }

    pub(super) fn title(&self) -> String {
        if self.file.is_empty() {
            return paths::basename(&self.cwd, None);
        }
        let rel = paths::relative(&self.cwd, &self.file);
        if rel.is_empty() { paths::basename(&self.file, None) } else { rel }
    }

    pub(super) fn draw(&mut self) {
        let (w, h) = self.size();
        let m = layout_of(w);
        let body = h as i64 - 2;
        // The first column holds the cursor's bar, the last the scrollbar when it shows (a narrower layout never has
        // fewer rows): one column narrower then, kept while it shows.
        if self.cache != self.key_of(w.saturating_sub(2), m) || self.rows.len() as i64 <= body {
            self.layout(w.saturating_sub(1), m);
        }
        if self.rows.len() as i64 > body {
            self.layout(w.saturating_sub(2), m);
        }
        let len = self.rows.len() as i64;
        self.top0 = self.top0.min(len - body).max(0);
        let bw = self.scrollbar.set(len, body, self.top0, &[]);
        // Header: the name and the counts, then the scope (a click opens b's menu) and ⚙, which opens the view's
        // options (,), as in the preview's Diff view; the scope left out when they do not fit.
        let mut head = vec![seg(format!(" {}", self.title()), "\x1b[1m")];
        if let Some(d) = &self.parsed {
            head.push(seg(format!(" +{}", d.add), green()));
            head.push(seg(format!(" −{}", d.del), red()));
        }
        let gear_style = fg(if self.menu.is_some() { c().text } else { c().dim });
        let label = scope::label(&self.scope, &self.base_name);
        let (scope_segs, scope_w, scope_hits) = ui::segmented(&[("scope", &label)], "scope");
        let mut switches: Vec<Switch> = vec![
            (scope_segs, scope_w, scope_hits.iter().map(|h| ("scope", h.from, h.to)).collect()),
            (vec![seg(format!(" {} ", ui::g().gear), gear_style)], 3, vec![("menu", 0, 2)]),
        ];
        let wi = w as i64;
        let head_w = ui::seg_width(&head) as i64;
        let width = |s: &[Switch]| s.iter().map(|x| x.1 as i64 + 1).sum::<i64>();
        while !switches.is_empty() && wi - head_w - width(&switches) < 1 {
            switches.remove(0);
        }
        self.chips.clear();
        let mut x = wi - width(&switches) + 1;
        let mut segs = head;
        segs.push(seg(" ".repeat((wi - head_w - width(&switches)).max(0) as usize), ""));
        for (s, sw, hits) in switches {
            segs.extend(s);
            segs.push(seg(" ", ""));
            for (id, from, to) in hits {
                self.chips.push(Chip { id, from: x + from as i64, to: x + to as i64 });
            }
            x += sw as i64 + 1;
        }
        let mut lines = vec![ui::line(&segs, w, Some(c().header))];
        let body_u = body.max(0) as usize;
        if self.parsed.is_none() {
            lines.extend(ui::empty::<&str, &str>(w, body_u, "Loading…", &[]));
        } else if self.rows.is_empty() {
            lines.extend(ui::empty(w, body_u, "No changes", &[("b", "compare with"), ("q", "close")]));
        } else {
            // The cursor's row and the others selected, and the noted ones: a bar in the first column.
            let (from, to) = self.range();
            let end = (self.top0 + body).min(len);
            for i in self.top0..end {
                let mark = if i >= from && i <= to {
                    seg("▌", fg(c().blue))
                } else if self.noted(i as usize) {
                    seg("▎", note_style())
                } else {
                    seg(" ", "")
                };
                let r = &self.rows[i as usize];
                let mut segs = vec![mark];
                match &r.redo {
                    Some(redo) if self.left > 0 => segs.extend(redo(self.left)),
                    _ => segs.extend(r.segs.iter().cloned()),
                }
                let back = if i == self.cur && r.back.is_none() { Some(c().select_blur) } else { r.back };
                lines.push(ui::line(&segs, w.saturating_sub(bw), back) + &self.scrollbar.cell(i - self.top0));
            }
        }
        while lines.len() < h.saturating_sub(1) {
            lines.push(ui::line(&[], w, None));
        }
        lines.truncate(h.saturating_sub(1));
        let foot = self.footer(w);
        lines.push(foot);
        if let Some(menu) = &mut self.menu {
            menu.draw(&mut lines, wi, h as i64);
        }
        if let Some(help) = &mut self.help {
            ui::help(&mut lines, w, "Changes", &help_groups(), help);
        }
        self.screen.paint(&lines);
    }

    pub(super) fn footer(&mut self, w: usize) -> String {
        self.hints = Hints::default();
        let header = Some(c().header);
        let room = |lead: &str| (w as i64 - ui::text_width(lead) as i64).max(1);
        if let Some(f) = &self.prompt {
            let lead = " Commit or range (abc123, main..HEAD): ";
            let mut segs = vec![seg(lead, cyan())];
            segs.extend(f.segments(room(lead), ""));
            return ui::line(&segs, w, header);
        }
        if let Some(q) = &self.confirm {
            let mut segs = vec![seg(format!(" {} ", q.text), fg(c().red))];
            segs.extend(ui::keys(&[("y", "yes"), ("n", "no")], w).segs);
            return ui::line(&segs, w, header);
        }
        if let Some(n) = &self.note {
            let lead = format!(" ✎ {} ", note_where(&n.r#ref));
            let kind = if n.kind.is_empty() { "[tab: kind] ".to_string() } else { format!("[{}] ", n.kind) };
            let mut segs = vec![seg(lead.clone(), note_style()), seg(kind.clone(), dim())];
            segs.extend(n.field.segments(room(&format!("{lead}{kind}")), ""));
            return ui::line(&segs, w, header);
        }
        if let Some(text) = &self.flash {
            return ui::line(&ui::message(text, None), w, header);
        }
        // The keys, and how far down the diff is.
        let body = self.body();
        let len = self.rows.len() as i64;
        let pct = if len > body {
            format!(" {}% ", (100.0 * self.top0 as f64 / (len - body).max(1) as f64 + 0.5).floor())
        } else {
            String::new()
        };
        let mut pairs: Vec<(String, String)> = Vec::new();
        let n = self.tab_notes.len();
        if n > 0 {
            pairs.push(("A".into(), format!("send {n} note{}", if n > 1 { "s" } else { "" })));
        }
        // b and v are left out: the header's switches show them (their keys in the help).
        if self.scope.kind() == "head" {
            pairs.push(("s".into(), "stage".into()));
            pairs.push(("u".into(), "unstage".into()));
        }
        for (k, a) in [("a", "note"), ("n N", "hunks"), ("q", "close"), ("?", "keys")] {
            pairs.push((k.into(), a.into()));
        }
        self.hints = ui::keys(&pairs, w.saturating_sub(pct.len()));
        let mut segs = self.hints.segs.clone();
        let pad = w.saturating_sub(ui::seg_width(&segs));
        segs.push(seg(format!("{pct:>pad$}"), dim()));
        ui::line(&segs, w, header)
    }
}
