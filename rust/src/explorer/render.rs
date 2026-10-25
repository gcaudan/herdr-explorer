//! Drawing of the explorer's tree view (src/explorer/render.js): the header, the rows with their git decorations, the
//! name being typed, the filter's and the changed files' lines, and the action menu, then a question over them.

use super::{Explorer, Inline};
use crate::core::icons;
use crate::core::js;
use crate::core::paths;
use crate::core::ui::{self, Seg, c, fg, g, line, seg, text_width};
use crate::explorer::tree::{self, Entry};

fn chevron(open: bool) -> &'static str {
    if open { g().open } else { g().closed }
}

/// A row shown: one of the tree's (by index), the one being renamed made a field, or a new one being named.
enum Shown {
    Row(usize),
    Edit(usize),
    New { depth: usize, dir: bool },
}

/// `fg(GIT[status])`.
fn git_fg(status: char) -> String {
    ui::git(status).map(fg).unwrap_or_default()
}

/// `n` items of a kind: "1 match", "2 matches".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Explorer {
    /// The tree, or a page in its place (case 11d), and a question over it.
    pub(super) fn render(&mut self) {
        let (w, h) = self.size();
        let mut lines = self.tree_view(w, h);
        if self.state.help.is_some() {
            let groups = self.help_groups();
            if let Some(help) = &mut self.state.help {
                ui::help(&mut lines, w, "Explorer", &groups, help);
            }
        }
        if let Some((q, _)) = &mut self.state.confirm {
            ui::confirm(&mut lines, w as i64, h as i64, q);
        }
        if let Some(choice) = &mut self.state.choice {
            ui::ask(&mut lines, w as i64, h as i64, &mut choice.ask);
        }
        self.screen.paint(&lines);
    }

    pub(super) fn body_height(&self) -> i64 {
        let s = &self.state;
        self.size().1 as i64 - 1 - i64::from(s.filter.is_some() || s.prompt.is_some() || s.changed)
    }

    /// The tree, with the name being typed in it (Rename, New file), then at the bottom the prompt, its keys or the
    /// filter.
    fn tree_view(&mut self, w: usize, h: usize) -> Vec<String> {
        let mut lines = vec![self.header(w)];
        let list_h = self.body_height();
        let shown = self.shown_rows();
        if let Some(edit) = shown.iter().position(|e| !matches!(e, Shown::Row(_))) {
            let edit = edit as i64;
            if edit < self.state.top {
                self.state.top = edit;
            }
            if edit >= self.state.top + list_h {
                self.state.top = edit - list_h + 1;
            }
        }
        let n = shown.len() as i64;
        if self.state.top > n - list_h {
            self.state.top = (n - list_h).max(0);
        }
        let bw = self.scrollbar.set(n, list_h, self.state.top, &[]);
        let width = w.saturating_sub(bw);
        let top = self.state.top;
        for i in top..n.min(top + list_h) {
            let l = match &shown[i as usize] {
                Shown::Row(k) => {
                    let selected = *k as i64 == self.state.cursor;
                    self.row(&self.state.rows[*k], width, selected)
                }
                Shown::Edit(k) => self.edit_row(Some(&self.state.rows[*k]), 0, false, width),
                Shown::New { depth, dir } => self.edit_row(None, *depth, *dir, width),
            };
            lines.push(l + &self.scrollbar.cell(i - top));
        }
        let lh = list_h.max(0) as usize;
        if shown.is_empty() {
            let first = |a: &str| ui::key_label(&self.keys_of(a).into_iter().next().unwrap_or_default());
            let empty = if self.state.filter.is_some() {
                ui::empty(w, lh, "No match", &[("esc", "clear")])
            } else if self.state.changed {
                let text = if self.tree.git.top.is_some() { "No changes" } else { "Not a git repository" };
                ui::empty(w, lh, text, &[("esc", "all files")])
            } else {
                let pairs: Vec<(String, &str)> = [(first("newFile"), "new file"), (first("newFolder"), "folder")]
                    .into_iter()
                    .filter(|(k, _)| !k.is_empty())
                    .collect();
                ui::empty(w, lh, "Empty folder", &pairs)
            };
            lines.extend(empty);
        }
        while (lines.len() as i64) < list_h + 1 {
            lines.push(line(&[], w, None));
        }
        if let Some(p) = &self.state.prompt {
            lines.push(if p.inline.is_some() {
                line(&self.prompt_keys(w), w, Some(c().header))
            } else {
                self.prompt_line(w)
            });
        } else if self.state.filter.is_some() {
            lines.push(self.filter_line(w));
        } else if self.state.changed {
            lines.push(self.changed_line(w));
        }
        if let Some(menu) = &mut self.state.menu {
            menu.draw(&mut lines, w as i64, h as i64);
        }
        while lines.len() < h {
            lines.push(line(&[], w, None));
        }
        lines.truncate(h);
        lines
    }

    /// The rows shown: the tree's, the one being renamed made a field, or a new one at the top of the folder it goes
    /// to.
    fn shown_rows(&self) -> Vec<Shown> {
        let rows = &self.state.rows;
        let mut shown: Vec<Shown> = (0..rows.len()).map(Shown::Row).collect();
        match self.state.prompt.as_ref().and_then(|p| p.inline.as_ref()) {
            None => {}
            Some(Inline::Rename { rel }) => {
                if let Some(i) = rows.iter().position(|r| r.rel == *rel) {
                    shown[i] = Shown::Edit(i);
                }
            }
            Some(Inline::New { dir, folder }) => {
                let i = if dir.is_empty() { None } else { rows.iter().position(|r| r.rel == *dir) };
                let depth = i.map_or(0, |i| rows[i].depth + 1);
                shown.insert(i.map_or(0, |i| i + 1), Shown::New { depth, dir: *folder });
            }
        }
        shown
    }

    /// A row being named, its icon following the name typed.
    fn edit_row(&self, r: Option<&Entry>, depth: usize, dir: bool, w: usize) -> String {
        let Some(p) = &self.state.prompt else { return String::new() };
        let f = &p.field;
        let (dir, open, depth) = r.map_or((dir, false, depth), |r| (r.dir, r.open, r.depth));
        let name = if f.value.is_empty() { r.map_or("", |r| r.name.as_str()) } else { f.value.as_str() };
        let icon = icons::icon(self.theme.as_ref(), name, dir, open);
        let lead = format!("{}{} ", " ".repeat(1 + depth * 2), if dir { chevron(open) } else { " " });
        let room = (w as i64 - text_width(&lead) as i64 - text_width(&icon.0) as i64 - 1).max(1);
        let mut segs = vec![seg(lead, fg(c().dim)), seg(icon.0, fg(icon.1))];
        segs.extend(f.segments(room, ""));
        line(&segs, w, Some(c().field))
    }

    /// The keys of a name typed in the tree, and where a new entry goes.
    fn prompt_keys(&self, w: usize) -> Vec<Seg> {
        let what = match self.state.prompt.as_ref().and_then(|p| p.inline.as_ref()) {
            Some(Inline::New { dir, .. }) => {
                format!("create in {}", paths::basename(if dir.is_empty() { &self.root } else { dir }, None))
            }
            _ => "rename".into(),
        };
        ui::keys(&[("⏎", what.as_str()), ("esc", "cancel")], w).segs
    }

    /// A prompt at the bottom.
    fn prompt_line(&self, w: usize) -> String {
        let Some(p) = &self.state.prompt else { return String::new() };
        let label = format!(" {}: ", p.label);
        let room = w as i64 - text_width(&label) as i64;
        let mut segs = vec![seg(label, fg(c().dim))];
        segs.extend(p.field.segments(room, ""));
        line(&segs, w, Some(c().header))
    }

    /// Whether the header shows the pill: ! finish when the installer must finish an update, else ↑ and the newer
    /// release (not a skipped one), else for a while after an update its notes (what's new).
    pub(super) fn show_update(&self) -> bool {
        self.finish.is_some() || self.newer().is_some() || self.notice.is_some()
    }

    /// The newer release the pill shows: with the update check on, and no update running.
    fn newer(&self) -> Option<&str> {
        let check = self.settings.get("updateCheck").is_some_and(crate::json::Value::truthy);
        self.latest.as_deref().filter(|_| check && !self.updating)
    }

    /// The header's pill; the mark alone in a narrow pane.
    fn pill(&self, w: usize) -> Option<Seg> {
        if !self.show_update() {
            return None;
        }
        let (mark, text, rgb) = match (&self.finish, self.newer()) {
            (Some(_), _) => ("!", "finish", c().yellow),
            (None, Some(v)) => ("↑", v, c().green),
            _ => ("✓", "what's new", c().blue),
        };
        let label = if w >= 24 { format!(" {mark} {text} ") } else { format!(" {mark} ") };
        Some(seg(label, format!("\x1b[1m{}{}", ui::bg(ui::shade(rgb)), fg(rgb))))
    }

    /// The pill's columns and the space after it, 0 without it.
    pub(super) fn pill_width(&self, w: usize) -> usize {
        self.pill(w).map_or(0, |p| text_width(&p.0) + 1)
    }

    /// Header: space name (or an action message, cut to the room left), then a pill and the settings gear.
    fn header(&self, w: usize) -> String {
        let mut icons = Vec::new();
        if let Some(p) = self.pill(w) {
            icons.extend([p, seg(" ", "")]);
        }
        icons.push(seg(format!("{} ", g().gear), fg(c().dim)));
        let room = w as i64 - 3 - self.pill_width(w) as i64;
        if let Some(flash) = &self.state.flash {
            let mut msg = ui::slice(&ui::message(flash, None), 0, room.max(0) as usize);
            let pad = (room - ui::seg_width(&msg) as i64 + 1).max(0) as usize;
            msg.push(seg(" ".repeat(pad), ""));
            msg.extend(icons);
            return line(&msg, w, None);
        }
        let base = paths::basename(&self.root, None).to_uppercase();
        let name = if base.is_empty() { format!(" {}", self.root) } else { format!(" {base}") };
        let (name, nw) = ui::fit(&name, room.max(0) as usize, None);
        let style = if self.root == self.home { "\x1b[1m" } else { "\x1b[1;3m" };
        let mut segs = vec![seg(name, style), seg(" ".repeat((room - nw as i64 + 1).max(0) as usize), "")];
        segs.extend(icons);
        line(&segs, w, None)
    }

    /// The filter's line: while typed, the field and the count on the right; once kept (Tab), the filter in blue,
    /// the count, and how to clear it.
    fn filter_line(&self, w: usize) -> String {
        let Some(f) = &self.state.filter else { return String::new() };
        let n = self.state.rows.iter().filter(|r| r.hit.is_some()).count();
        let found = if f.field.value.is_empty() { String::new() } else { count(n, "match", "matches") };
        let lead = seg(format!(" {} ", g().search), fg(c().dim));
        if f.active {
            let field = f.field.segments((w as i64 - 5 - found.len() as i64).max(1), "");
            let pad = (w as i64 - 4 - ui::seg_width(&field) as i64).max(0) as usize;
            let mut segs = vec![lead];
            segs.extend(field);
            segs.push(seg(format!("{found:>pad$}"), fg(c().dim)));
            return line(&segs, w, Some(c().header));
        }
        let shown = if found.is_empty() { String::new() } else { format!("  {found}") };
        let left = vec![lead, seg(f.field.value.clone(), fg(c().blue)), seg(shown, fg(c().dim))];
        self.with_hint(left, "clear", w)
    }

    /// A bottom line: what it shows on the left, and on the right how to leave it (esc) when there is room.
    fn with_hint(&self, mut left: Vec<Seg>, leave: &str, w: usize) -> String {
        let hint = ui::keys(&[("esc", leave)], usize::MAX).segs;
        let room = w as i64 - ui::seg_width(&left) as i64 - ui::seg_width(&hint) as i64 - 1;
        if room >= 0 {
            left.push(seg(" ".repeat(room as usize), ""));
            left.extend(hint);
        }
        line(&left, w, Some(c().header))
    }

    /// The line of the changed files only view: how many, and how to leave it.
    fn changed_line(&self, w: usize) -> String {
        let n = self.state.rows.iter().filter(|r| !r.dir).count();
        let left = vec![
            seg(" ● ", fg(c().blue)),
            seg("Changes", fg(c().blue)),
            seg(format!("  {}", count(n, "file", "files")), fg(c().dim)),
        ];
        self.with_hint(left, "all files", w)
    }

    fn row(&self, r: &Entry, w: usize, selected: bool) -> String {
        let deco = self.tree.decoration(&r.rel, r.dir);
        // A deleted entry no longer on disk: struck through.
        let color = format!("{}{}", deco.map(git_fg).unwrap_or_default(), if r.gone { "\x1b[9m" } else { "" });
        let icon = icons::icon(self.theme.as_ref(), &r.name, r.dir, r.open);
        let indent = " ".repeat(1 + r.depth * 2);
        let mark = if r.dir { chevron(r.open) } else { " " };
        let badge = match deco {
            None | Some('!') => String::new(),
            Some(_) if r.dir => "●".into(),
            Some('C') => "!".into(),
            Some(d) => d.to_string(),
        };
        let badge_w = if badge.is_empty() { 0 } else { 2 };
        // The chevron, a space and the icon.
        let lead = (indent.len() + 2 + text_width(&icon.0)) as i64;
        let room = w as i64 - lead - badge_w - 1;
        let mut name = format!("{}{}", r.chain.as_deref().unwrap_or(""), r.name);
        if text_width(&name) as i64 > room {
            name = format!("{}…", ui::fit(&name, (room - 1).max(1) as usize, None).0);
        }
        let pad = (w as i64 - lead - text_width(&name) as i64 - badge_w - 1).max(0) as usize;
        // The part matching the filter in bold blue, as in Go to file; a folder shown only for what it holds, dimmed.
        let filtering = self.state.filter.as_ref().is_some_and(|f| !f.field.value.is_empty());
        // The JS's offsets, in UTF-16 units of r.name, applied to the name shown (its chain and its … included).
        let units = |at: usize| r.name.get(..at).map(|s| s.encode_utf16().count());
        let hit = r.hit.and_then(|(a, b)| {
            let (a, b) = (units(a)?, units(b)?);
            if b > js::utf16_len(&name) {
                return None;
            }
            Some((tree::unit_at(&name, a)?, tree::unit_at(&name, b)?))
        });
        let parts = match hit {
            Some((a, b)) => vec![
                seg(&name[..a], color.clone()),
                seg(&name[a..b], format!("\x1b[1m{}", fg(c().hit))),
                seg(&name[b..], color.clone()),
            ],
            _ => vec![seg(name.clone(), if filtering && r.hit.is_none() { fg(c().dim) } else { color.clone() })],
        };
        let icon_rgb = if deco == Some('!') { ui::git('!').unwrap_or(icon.1) } else { icon.1 };
        let mut segs = vec![seg(format!("{indent}{mark} "), fg(c().dim)), seg(icon.0, fg(icon_rgb))];
        segs.extend(parts);
        let tail = if badge.is_empty() { String::new() } else { format!(" {badge}") };
        segs.push(seg(format!("{}{tail}", " ".repeat(pad)), color));
        let back = selected.then(|| if self.state.focused { c().select } else { c().select_blur });
        line(&segs, w, back)
    }
}
