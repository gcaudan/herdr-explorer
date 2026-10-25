//! Keys and mouse of the source control pane (src/scm/input.js); the selection, which the diff beside the list
//! follows, is the pane's (pane.rs).

use super::context::{PickItem, Row, VIEWS};
use super::pane::{Act, Pane, Yes};
use super::state;
use crate::core::actions;
use crate::core::ui::{self, BarHit, Confirm, Mouse};
use std::time::{Duration, Instant};

impl Pane {
    // --- branch picker and names typed in place

    fn picker_key(&mut self, k: &str) {
        let Some(p) = self.s.picker.as_mut() else { return };
        let b = usize::try_from(p.sel).ok().and_then(|i| p.items.get(i)).cloned();
        let body = p.r#box.4;
        let move_to = |p: &mut super::context::Picker, i: i64| {
            p.sel = i.min(p.items.len() as i64 - 1).max(0);
        };
        let sel = p.sel;
        if k == "up" || (!p.typing && k == "k") {
            move_to(p, sel - 1);
            return self.draw();
        }
        if k == "down" || (!p.typing && k == "j") {
            move_to(p, sel + 1);
            return self.draw();
        }
        if k == "pageup" {
            move_to(p, sel - body);
            return self.draw();
        }
        if k == "pagedown" {
            move_to(p, sel + body);
            return self.draw();
        }
        if k == "enter" {
            return self.pick_branch(b);
        }
        if k == "escape" {
            if !p.field.value.is_empty() || p.typing {
                p.field.set("");
                p.typing = false;
                state::filter_picker(p);
                return self.draw();
            }
            self.s.picker = None;
            return self.draw();
        }
        if !p.typing {
            if k == "q" {
                self.s.picker = None;
                return self.draw();
            }
            if k == "n" && p.mode.is_none() {
                return self.new_branch("");
            }
            if k == "D" && p.mode.is_none() {
                if let Some(PickItem::Branch(b)) = b {
                    self.delete_branch(b.branch);
                }
                return;
            }
            if k == "/" {
                p.typing = true;
                return self.draw();
            }
        }
        if p.field.key(k) {
            p.typing = true;
            if !p.field.value.is_empty() && p.mode.is_none() {
                p.sel = 1;
            }
            state::filter_picker(p);
            if p.items.len() < 2 {
                p.sel = 0;
            }
            self.draw();
        }
    }

    fn inline_key(&mut self, k: &str) {
        if k == "escape" || k == "ctrl+c" {
            self.inline = None;
            return self.draw();
        }
        if k == "enter" || k == "alt+enter" || k == "ctrl+o" {
            let Some(it) = self.inline.take() else { return };
            self.draw();
            return self.inline_done(it.done, it.field.value, k != "enter");
        }
        if let Some(it) = self.inline.as_mut() {
            it.field.key(k);
        }
        self.draw();
    }

    // --- diffs

    /// Keys of a focused comparison: the selection moves over the commits, Enter shows the selected one. Below SPLIT
    /// columns, the tab's preview follows the selected commit, as it follows a History row.
    fn compare_key(&mut self, k: &str, page: i64) -> bool {
        let h = self.size().1;
        let wide = self.wide();
        let Some(d) = self.s.diff.as_mut() else { return false };
        let at: Vec<i64> =
            d.cmp_rows.iter().enumerate().filter(|(_, r)| r.commit.is_some()).map(|(i, _)| i as i64).collect();
        if at.is_empty() {
            return false;
        }
        let pos = at.iter().position(|&i| i == d.sel).map_or(0, |p| p as i64);
        let target = match k {
            "up" | "k" => Some(if d.sel < 0 { 0 } else { pos - 1 }),
            "down" | "j" => Some(if d.sel < 0 { 0 } else { pos + 1 }),
            "pageup" => Some(pos - page),
            "pagedown" => Some(pos + page),
            "home" | "g" => Some(0),
            "end" | "G" => Some(at.len() as i64 - 1),
            _ => None,
        };
        if let Some(i) = target {
            d.sel = at[i.min(at.len() as i64 - 1).max(0) as usize];
            let body = h - 3;
            if d.sel < d.top + 1 {
                d.top = (d.sel - 1).max(0);
            }
            if d.sel >= d.top + body {
                d.top = d.sel - body + 1;
            }
            let row = d.cmp_rows[d.sel as usize].commit.as_ref().map(Pane::commit_row);
            self.draw();
            if !wide && let Some(row) = row {
                self.compare_at = Some((Instant::now() + Duration::from_millis(120), row));
            }
            return true;
        }
        if k == "enter" && d.sel >= 0 {
            let Some(c) = d.cmp_rows.get(d.sel as usize).and_then(|r| r.commit.clone()) else { return false };
            if !wide {
                self.compare_at = None;
                self.preview_row(Some(&Pane::commit_row(&c)), true, true);
                return true;
            }
            let back = self.s.diff.take();
            let mut args: Vec<String> = vec!["show".into()];
            args.extend(crate::diff::DIFF_OPTS.iter().map(|a| a.to_string()));
            args.extend(
                ["--stat", "-p", "--diff-merges=first-parent", "--format=fuller", "--decorate=short", c.hash.as_str()]
                    .iter()
                    .map(|a| a.to_string()),
            );
            self.show_diff(&format!("commit:{}", c.hash), &format!("{} {}", c.short, c.subject), Some(args), None, "");
            if let Some(d) = self.s.diff.as_mut() {
                d.back = back.map(Box::new);
                d.focus = true;
            }
            return true;
        }
        false
    }

    fn diff_key(&mut self, k: &str) {
        let page = self.size().1 - 4;
        let wide = self.wide();
        let Some(d) = self.s.diff.as_mut() else { return };
        // A commit shown from a comparison: q and Escape go back to the comparison.
        if d.back.is_some() && (k == "q" || k == "escape") {
            let mut back = *d.back.take().unwrap();
            back.focus = true;
            self.s.diff = Some(back);
            return self.draw();
        }
        if d.compare && self.compare_key(k, page) {
            return;
        }
        let Some(d) = self.s.diff.as_mut() else { return };
        if matches!(k, "q" | "escape" | "tab" | "shift+tab") {
            // Beside the list, the diff stays: the list takes the keys back.
            if wide {
                d.focus = false;
                return self.draw();
            }
            if k == "q" || k == "escape" {
                return self.close_diff();
            }
        }
        let half = (page + 1).div_euclid(2);
        let scroll = match k {
            "up" | "k" => Some(-1),
            "down" | "j" | "enter" => Some(1),
            "pageup" => Some(-page),
            "pagedown" | "space" => Some(page),
            "ctrl+u" => Some(-half),
            "ctrl+d" => Some(half),
            _ => None,
        };
        if let Some(n) = scroll {
            d.top += n;
            return self.draw();
        }
        match k {
            "home" | "g" => {
                d.top = 0;
                self.draw();
            }
            // drawn at the end: the render keeps the top in range
            "end" | "G" => {
                d.top = i64::MAX / 4;
                self.draw();
            }
            "v" => self.switch_layout(),
            "w" => self.toggle_wrap(),
            "," => self.open_diff_view(0),
            // Without wrap, the text 8 columns sideways, up to the longest line.
            "left" | "h" => {
                if !self.wrap && d.left > 0 {
                    d.left = d.left.saturating_sub(8);
                    self.draw();
                }
            }
            "right" | "l" => {
                let widest = d.parsed.as_ref().map_or(0, |p| crate::diff::widest_of(p));
                if !self.wrap && d.left + 8 < widest {
                    d.left += 8;
                    self.draw();
                }
            }
            "n" | "N" => {
                let top = d.top;
                let hunks = d.rows.iter().enumerate().filter(|(_, r)| r.hunk).map(|(j, _)| j as i64);
                let i = if k == "n" { hunks.into_iter().find(|&j| j > top) } else { hunks.rev().find(|&j| j < top) };
                if let Some(i) = i {
                    d.top = i;
                    self.draw();
                }
            }
            _ => {}
        }
    }

    // --- views and keys

    fn field_key(&mut self, k: &str) {
        match k {
            "enter" => return self.commit(),
            "escape" | "tab" => {
                self.s.editing = false;
                return self.draw();
            }
            "ctrl+g" => return self.generate(),
            "alt+a" => return self.toggle_amend(),
            "ctrl+c" => self.quit(),
            _ => {}
        }
        if self.s.generating {
            return;
        }
        self.s.field.key(k);
        self.draw();
    }

    pub(super) fn key(&mut self, k: &str) {
        let h = self.size().1;
        if let Some((q, _)) = self.confirm.as_mut() {
            let answer = ui::confirm_key(k, q);
            let yes = if answer != "move" { self.confirm.take().map(|(_, y)| y) } else { None };
            self.draw();
            if answer == "yes"
                && let Some(yes) = yes
            {
                self.yes(yes);
            }
            return;
        }
        if self.inline.is_some() {
            return self.inline_key(k);
        }
        if let Some(menu) = self.menu.as_mut() {
            let r = menu.key(k, h);
            return self.menu_choice(r);
        }
        if self.s.picker.is_some() {
            return self.picker_key(k);
        }
        if self.s.editing && self.repo().is_some() {
            return self.field_key(k);
        }
        if k == "ctrl+c" {
            self.quit();
        }
        // The key help (ui::help_key): scrolls, finds with /, closes with esc, q, ? or f1.
        if let Some(help) = self.help.as_mut() {
            if !ui::help_key(k, help) {
                self.help = None;
            }
            return self.draw();
        }
        if (k == "?" || k == "f1") && self.repo().is_some() {
            self.help = Some(ui::HelpState::default());
            return self.draw();
        }
        if k == "z" {
            let pane = self.pane.clone();
            self.herdr.send(Box::new(move || {
                actions::zoom(&pane);
                Box::new(|_: &mut Pane| {})
            }));
            return;
        }
        if self.s.diff.as_ref().is_some_and(|d| d.focus) || (self.s.diff.is_some() && !self.wide()) {
            return self.diff_key(k);
        }
        if self.repo().is_none() {
            if k == "q" || k == "escape" {
                self.quit();
            }
            if k == "i" && matches!(self.s.repo, super::context::RepoState::None) && !self.s.no_git {
                let q = Confirm {
                    where_: Some("git init".into()),
                    tone: "go".into(),
                    title: "Create a repository?".into(),
                    name: self.root.clone(),
                    action: "Create".into(),
                    ..Confirm::default()
                };
                self.confirm(q, Yes::Init);
            }
            if k == "r" {
                self.schedule();
            }
            return;
        }
        let len = self.frame.rows.len() as i64;
        let row = self.frame.rows.get(self.s.cursor as usize).cloned();
        let page = (self.frame.list_h - 1).max(1);
        let cursor = self.s.cursor;
        let half = (page + 1).div_euclid(2);
        match k {
            "q" | "escape" => self.quit(),
            "up" | "k" => self.select(cursor - 1),
            "down" | "j" => self.select(cursor + 1),
            "pageup" => self.select(cursor - page),
            "pagedown" => self.select(cursor + page),
            "home" | "g" => self.select(0),
            "end" | "G" => self.select(len - 1),
            "left" | "h" => {
                if let Some(row) = row.as_ref().filter(|r| r.foldable() && r.open) {
                    return self.toggle(row, Some(false));
                }
                let depth = row.as_ref().map_or(0, |r| r.depth);
                let up = (0..cursor).rev().find(|&i| {
                    let r = &self.frame.rows[i as usize];
                    r.foldable() && r.depth < depth
                });
                if let Some(i) = up {
                    self.select(i);
                }
            }
            "right" | "l" => {
                if let Some(row) = row.as_ref().filter(|r| r.foldable() && !r.open) {
                    return self.toggle(row, Some(true));
                }
            }
            "tab" => {
                if let Some(d) = self.s.diff.as_mut() {
                    d.focus = true;
                }
            }
            "enter" => {
                let wide = self.wide();
                // Beside the list, the diff already shows the selection: Enter goes into it.
                if let Some(row) =
                    row.as_ref().filter(|r| wide && state::diffable(&self.s, Some(r)) && r.kind != "branch")
                {
                    if self.s.diff.as_ref().is_none_or(|d| d.key != row.key) {
                        self.open_row(Some(row), false);
                    }
                    if let Some(d) = self.s.diff.as_mut().filter(|d| d.key == row.key) {
                        d.focus = true;
                        d.auto = false;
                    }
                } else {
                    if !wide && self.previewable(row.as_ref()) {
                        return self.preview_row(row.as_ref(), true, true);
                    }
                    return self.open_row(row.as_ref(), false);
                }
            }
            "1" | "2" | "3" | "4" => {
                let i = k.parse::<usize>().unwrap_or(1) - 1;
                return self.set_view(VIEWS[i].0);
            }
            "r" => {
                self.flash("Refreshed", false);
                return self.schedule();
            }
            "f" => return self.fetch(),
            "P" => return self.push(),
            "L" => return self.pull(),
            "ctrl+u" => self.select(cursor - half),
            "ctrl+d" => self.select(cursor + half),
            "ctrl+p" => return self.search("files"),
            "ctrl+f" => return self.search("content"),
            "b" => return self.open_picker(None, None),
            "m" | "menu" | "shift+f10" => return self.open_menu(false),
            "T" => return self.new_tag(None),
            "<" | ">" => {
                if self.wide() {
                    self.resize_list(self.frame.list_w + if k == "<" { -4 } else { 4 }, true);
                } else if !self.review.is_empty() {
                    self.keep_share(if k == "<" { -0.04 } else { 0.04 });
                }
            }
            "shift+left" | "shift+right" => {
                if self.wide() {
                    self.resize_list(self.frame.list_w + if k == "shift+left" { -4 } else { 4 }, true);
                }
            }
            "c" => {
                if self.s.view != "changes" {
                    self.set_view("changes");
                }
                self.s.editing = true;
            }
            "ctrl+g" => {
                if self.s.view != "changes" {
                    self.set_view("changes");
                }
                return self.generate();
            }
            "alt+a" => return self.toggle_amend(),
            _ => return self.view_key(k, row),
        }
        self.draw();
    }

    fn view_key(&mut self, k: &str, row: Option<Row>) {
        let kind = row.as_ref().map_or("", |r| r.kind);
        match self.s.view {
            "changes" => {
                let items = row.as_ref().filter(|r| r.has_items()).map(|r| (r.items.clone(), r.sec));
                match k {
                    "s" | "space" => {
                        if let Some((items, sec)) = items {
                            if sec == "staged" {
                                self.flash("Already staged", false);
                            } else {
                                self.stage(items);
                            }
                        }
                    }
                    "u" => {
                        if let Some((items, sec)) = items {
                            if sec == "staged" {
                                self.unstage(items);
                            } else {
                                self.flash("Not staged", false);
                            }
                        }
                    }
                    "a" => self.stage_all(),
                    "A" => self.unstage_all(),
                    "d" => {
                        if let Some((items, sec)) = items {
                            self.discard(items, sec);
                        }
                    }
                    "t" => self.toggle_tree(),
                    "X" => self.abort(),
                    "y" | "Y" | "e" | "E" | "o" => self.file_action(k, row.as_ref()),
                    _ => {}
                }
            }
            "history" => {
                let commit =
                    row.as_ref().filter(|_| kind == "commit").and_then(|r| r.commit.as_ref()).map(|h| h.commit.clone());
                match k {
                    "a" => self.toggle_all_branches(),
                    "x" if self.s.hist_file.is_some() => self.all_files(),
                    "t" if commit.is_some() => self.new_tag(commit),
                    "C" => {
                        if let Some(c) = commit {
                            self.cherry_pick(c);
                        }
                    }
                    _ => {}
                }
            }
            "stashes" => {
                if k == "n" {
                    return self.new_stash();
                }
                if let Some(s) = row.as_ref().filter(|_| kind == "stash").and_then(|r| r.stash.clone()) {
                    match k {
                        "a" => self.stash_action(s, "apply"),
                        "o" => self.stash_action(s, "pop"),
                        "d" => self.stash_action(s, "drop"),
                        _ => {}
                    }
                }
            }
            "refs" => {
                let Some(row) = row.filter(|_| kind == "branch") else { return };
                let Some(b) = row.branch().cloned() else { return };
                match k {
                    "D" => self.delete_branch(b),
                    "=" => {
                        if !state::comparable(&self.s, Some(&row)) {
                            return self.flash(
                                format!("{} is the base of the comparisons: select another branch", b.name),
                                false,
                            );
                        }
                        self.share_compare(&row);
                        self.compare(&row, true);
                    }
                    "B" => self.set_base(&row),
                    "M" => self.integrate(b, "merge"),
                    "R" => self.integrate(b, "rebase"),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// What a click on an area of the frame does.
    fn act(&mut self, act: Act) {
        match act {
            Act::Picker => self.open_picker(None, None),
            Act::Push => self.push(),
            Act::Pull => self.pull(),
            Act::Fetch => self.fetch(),
            Act::Actions => self.open_menu(true),
            Act::View(id) => self.set_view(id),
            Act::List => {
                if self.s.tree {
                    self.toggle_tree();
                }
            }
            Act::Tree => {
                if !self.s.tree {
                    self.toggle_tree();
                }
            }
            Act::AllFiles => self.all_files(),
            Act::Edit => self.s.editing = true,
            Act::Commit => self.commit(),
            Act::Generate => self.generate(),
            Act::Amend => self.toggle_amend(),
            Act::Abort => self.abort(),
            Act::Key(k) => self.key(&k),
            Act::DiffView => self.open_diff_view(0),
            Act::Stage(items) => self.stage(items),
            Act::Unstage(items) => self.unstage(items),
            Act::Discard(items, sec) => self.discard(items, sec),
        }
    }

    fn hit_at(&self, x: i64, y: i64) -> Option<Act> {
        self.frame.hits.iter().find(|h| h.y == y && x >= h.x0 && x < h.x1).map(|h| h.act.clone())
    }

    pub(super) fn mouse(&mut self, m: &Mouse) {
        let (w, h) = self.size();
        let (x, y) = (m.x - 1, m.y - 1);
        let (list_y, list_h, list_w) = (self.frame.list_y, self.frame.list_h, self.frame.list_w);
        let len = self.frame.rows.len() as i64;
        if let Some((q, _)) = &self.confirm {
            let Some(answer) = ui::confirm_mouse(m, q) else { return };
            let yes = self.confirm.take().map(|(_, y)| y);
            self.draw();
            if answer == "yes"
                && let Some(yes) = yes
            {
                self.yes(yes);
            }
            return;
        }
        if let Some(help) = self.help.as_mut() {
            if !ui::help_mouse(m, help) {
                self.help = None;
            }
            return self.draw();
        }
        if let Some(menu) = self.menu.as_mut() {
            let r = menu.mouse(m, h);
            return self.menu_choice(r);
        }
        if m.wheel != 0 {
            if let Some(p) = self.s.picker.as_mut() {
                p.sel = (p.sel + m.wheel).min(p.items.len() as i64 - 1).max(0);
                return self.draw();
            }
            let wide = self.wide();
            if let Some(d) = self.s.diff.as_mut().filter(|_| !wide || x > list_w) {
                d.top += m.wheel * 3;
                return self.draw();
            }
            self.s.top = (self.s.top + m.wheel * 3).min(len - list_h).max(0);
            return self.draw();
        }
        // The separator between the list and the diff: dragged to resize them, saved at the release.
        // A motion without the button means its release was not reported (outside the pane).
        if self.s.resizing {
            if m.drag && m.button == 0 {
                return self.resize_list(x, false);
            }
            self.s.resizing = false;
            return self.resize_list(list_w, true);
        }
        let click = m.press && !m.drag && m.button == 0;
        if self.wide()
            && list_w < w
            && x == list_w
            && y >= 1
            && y < h - 1
            && click
            && self.s.picker.is_none()
            && self.help.is_none()
        {
            self.s.resizing = true;
            return self.draw();
        }
        // The diff's scrollbar in the last column, the list's in the last column of the list.
        let now = Instant::now();
        if self.s.diff.is_some() && self.s.picker.is_none() {
            match self.diff_bar.mouse(m, y - 2, x == w - 1, now) {
                BarHit::Ignored => {}
                BarHit::Took => return,
                BarHit::Scroll(top) => {
                    if let Some(d) = self.s.diff.as_mut() {
                        d.top = top;
                    }
                    return self.draw();
                }
            }
        }
        if list_w != 0 && self.s.picker.is_none() {
            match self.list_bar.mouse(m, y - list_y, x == list_w - 1, now) {
                BarHit::Ignored => {}
                BarHit::Took => return,
                BarHit::Scroll(top) => {
                    self.s.top = top;
                    return self.draw();
                }
            }
        }
        if !click {
            return;
        }
        // A key of the status line acts as that key, also while a name is typed or the branch picker is open.
        if (self.inline.is_some() || self.s.picker.is_some())
            && y == h - 1
            && let Some(act) = self.hit_at(x, y)
        {
            self.act(act);
            return self.draw();
        }
        if self.inline.is_some() {
            self.inline = None;
            return self.draw();
        }
        if let Some(p) = self.s.picker.as_mut() {
            let (bx, by, bw, bh, body) = p.r#box;
            let i = p.top + y - by - 2;
            if x >= bx && x < bx + bw && y >= by && y < by + bh {
                if y == by + 1 {
                    p.typing = true;
                    return self.draw();
                }
                if i >= p.top && i < p.top + body && (i as usize) < p.items.len() {
                    p.sel = i;
                    let b = p.items.get(i as usize).cloned();
                    return self.pick_branch(b);
                }
                return;
            }
            self.s.picker = None;
            return self.draw();
        }
        let hit = self.hit_at(x, y);
        let was_editing = self.s.editing;
        // The message field, its buttons and the hints (which then act on the message) keep it being typed.
        if !(hit.is_some() && (y == 2 || y == 3 || y == h - 1)) {
            self.s.editing = false;
        }
        if let Some(act) = hit {
            self.act(act);
            return self.draw();
        }
        if was_editing {
            self.draw();
        }
        let wide = self.wide();
        if let Some(d) = self.s.diff.as_mut().filter(|_| !wide || x > list_w) {
            d.focus = true;
            return self.draw();
        }
        // the diff side, empty
        if wide && x >= list_w {
            return;
        }
        if y < list_y || y >= list_y + list_h {
            return;
        }
        let i = self.s.top + y - list_y;
        if i >= len {
            return;
        }
        if let Some(d) = self.s.diff.as_mut() {
            d.focus = false;
        }
        let double = self.last_click.is_some_and(|(j, t)| j == i && now.duration_since(t) < Duration::from_millis(400));
        self.last_click = if double { None } else { Some((i, now)) };
        self.select(i);
        let row = self.frame.rows[i as usize].clone();
        if row.kind == "section" || row.kind == "folder" {
            self.toggle(&row, None);
        } else if double || row.kind == "action" {
            self.open_row(Some(&row), false);
        }
        self.draw();
    }
}
