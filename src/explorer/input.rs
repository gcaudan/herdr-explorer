//! The explorer's keyboard and mouse: a question or a prompt first, then the menu, the
//! settings, the key help, the filter, and the tree's actions by their keys.

use super::{Act, Explorer, Filter, later};
use crate::core::actions;
use crate::core::paths;
use crate::core::ui::{self, BarHit, Choice, Event, Field, Mouse};
use crate::explorer::tree::{holds, parent_of};
use crate::json::Value;
use std::time::{Duration, Instant};

/// While filtering, typed characters go to the filter; arrows and Enter act on the list.
const FILTER_EDIT: [&str; 4] = ["backspace", "delete", "ctrl+u", "ctrl+d"];

/// Preview follows selection, from the keyboard: once the cursor stops (a key held down sends a burst), and only in a
/// preview already open, so that moving never splits the tab.
const MOVES: [&str; 10] =
    ["up", "down", "pageUp", "pageDown", "halfPageUp", "halfPageDown", "top", "bottom", "expand", "collapse"];

/// The actions that need the entry on disk, refused on a deleted one.
const ON_DISK: [&str; 10] = [
    "rename",
    "delete",
    "edit",
    "openInEditor",
    "openInApp",
    "preview",
    "copyPath",
    "newFile",
    "newFolder",
    "searchInFiles",
];

impl Explorer {
    /// An event of the screen.
    pub(super) fn event(&mut self, e: Event) {
        match e {
            Event::Key(k) => self.key(&k),
            Event::Mouse(m) => self.mouse(&m),
            Event::Focus(f) => {
                // A button released out of the pane may never be reported: a held scrollbar stops with the focus.
                if !f {
                    self.scrollbar.stop();
                }
                self.state.focused = f;
                self.render();
            }
            Event::Paste { text, .. } => {
                if let Some(p) = &mut self.state.prompt {
                    p.field.insert(&text);
                } else if let Some(f) = self.state.filter.as_mut().filter(|f| f.active) {
                    f.field.insert(&text);
                    self.build_rows();
                }
                self.render();
            }
            Event::Reply(text) => {
                if ui::kitty_ok(&text) {
                    self.graphics = Some(true);
                    self.render();
                }
            }
        }
    }

    // --- action menu

    /// Escape always closes the menu, whatever the bindings; the key of an item runs it.
    fn menu_key(&mut self, k: &str) {
        let action = self.keys.get(k).map(String::from);
        let h = self.size().1 as i64;
        let Some(m) = &mut self.state.menu else { return };
        if k == "escape" {
            self.state.menu = None;
        } else if let Some(a @ ("up" | "down")) = action.as_deref() {
            // Moving and running go through ui::Menu, as in the other menus, from the keys bound here.
            m.key(a, h);
        } else if action.as_deref() == Some("open") {
            if let Some(Choice::Run { run, .. }) = m.key("enter", h) {
                return self.run_action(move |ex| (run.run)(ex));
            }
        } else {
            // The key shown beside an item runs it (Settings… on ,); the others close the menu.
            let item = action.as_deref().and_then(|a| m.items.iter().flatten().find(|it| it.run.action == Some(a)));
            if let Some(it) = item {
                let run = it.run.run.clone();
                return self.run_action(move |ex| run(ex));
            }
            // expand (right, l) does nothing: an arrow never runs an action.
            if matches!(action.as_deref(), Some("close" | "collapse" | "menu" | "settings")) {
                self.state.menu = None;
            }
        }
        self.render();
    }

    // --- keyboard and mouse

    /// A question with several answers (ui::ask_key; escape: its escape answer, else none).
    fn choose(&mut self, answer: Option<String>) {
        let Some(answer) = answer.filter(|a| a != "move") else { return self.render() };
        let Some(choice) = self.state.choice.take() else { return };
        match choice.on.into_iter().find(|(k, _)| *k == answer) {
            Some((_, run)) => self.run_action(move |ex| run(ex)),
            None => self.render(),
        }
    }

    /// A question (ui::confirm_key) or a prompt.
    fn prompt_key(&mut self, k: &str) {
        if let Some(choice) = &mut self.state.choice {
            let answer = ui::ask_key(k, &mut choice.ask);
            return self.choose(answer);
        }
        if let Some((q, _)) = &mut self.state.confirm {
            let answer = ui::confirm_key(k, q);
            if answer == "move" {
                return self.render();
            }
            let (_, yes) = self.state.confirm.take().unwrap();
            if answer == "yes" {
                return self.run_action(move |ex| yes(ex));
            }
            return self.render();
        }
        let Some(p) = &mut self.state.prompt else { return };
        if k == "escape" {
            self.state.prompt = None;
        } else if k == "enter" {
            let p = self.state.prompt.take().unwrap();
            let v = p.field.value.clone();
            let blank = v.trim_matches(|c| crate::unicode::is_space(c) || crate::core::regex::is_line_terminator(c));
            if !blank.is_empty() || p.empty {
                self.run_action(move |ex| (p.done)(ex, v));
            }
        } else {
            p.field.key(k);
        }
        self.render();
    }

    fn filter_key(&mut self, k: &str) {
        if k == "escape" {
            return self.close_filter();
        }
        // Bound keys that are not characters (arrows, Enter, ctrl+…) act on the list; characters and the field's
        // editing keys go to the filter (ctrl+u and ctrl+d move half a page in the tree only).
        if self.keys.get(k).is_some() && !ui::printable(k) && !FILTER_EDIT.contains(&k) {
            return self.list_key(k);
        }
        let Some(f) = &mut self.state.filter else { return };
        if k == "tab" {
            f.active = false;
            return self.render();
        }
        if f.field.key(k) {
            self.state.selected = None;
            if f.field.value.is_empty() {
                let field = std::mem::take(&mut f.field);
                self.state.filter = Some(Filter { field, active: true });
            }
            self.build_rows();
            self.state.top = 0;
            self.reveal();
            self.render();
        }
    }

    fn close_filter(&mut self) {
        let sel = self.state.selected.clone();
        self.state.filter = None;
        match sel {
            Some(sel) => self.reveal_rel(&sel),
            None => self.build_rows(),
        }
        self.render();
    }

    fn key(&mut self, k: &str) {
        let s = &self.state;
        if s.choice.is_some() || s.confirm.is_some() || s.prompt.is_some() {
            return self.prompt_key(k);
        }
        if s.menu.is_some() {
            return self.menu_key(k);
        }
        if self.state.settings.is_some() {
            return self.settings_key(k);
        }
        if let Some(help) = &mut self.state.help {
            if !ui::help_key(k, help) {
                self.state.help = None;
            }
            return self.render();
        }
        let close = k == "escape" || self.keys.get(k) == Some("close");
        match &self.state.filter {
            Some(f) if f.active => return self.filter_key(k),
            Some(_) if close => return self.close_filter(),
            _ => {}
        }
        if self.state.changed && close {
            return self.show_changed(false);
        }
        self.list_key(k);
    }

    fn list_key(&mut self, k: &str) {
        let action = self.keys.get(k).unwrap_or("").to_string();
        self.list_action(&action);
    }

    /// Preview follows selection: the selected file previewed once the cursor stops for 150 ms.
    fn following(&mut self) {
        self.following_at = None;
        if !self.settings.get("singleClick").is_some_and(Value::truthy) {
            return;
        }
        self.following_at = Some(Instant::now() + Duration::from_millis(150));
    }

    pub(super) fn follow_selection(&mut self) {
        let Some(r) = self.current().filter(|r| !r.dir) else { return };
        let abs = self.abs_of(&r.rel);
        if !actions::can_preview(&abs) {
            return;
        }
        let (root, pane) = (self.root.clone(), self.pane.clone());
        let view = if self.state.changed { "diff" } else { "file" };
        let shown = actions::Shown { view: view.into(), ..Default::default() };
        self.herdr.send(Box::new(move || {
            let text = actions::preview(&abs, &root, &pane, 0.0, false, false, &shown);
            Box::new(move |ex: &mut Explorer| ex.flash(text))
        }));
    }

    /// An action of the tree, from its key.
    fn list_action(&mut self, action: &str) {
        let r = self.current().cloned();
        let page = self.body_height() - 1;
        let half = (page as f64 / 2.0).ceil() as i64;
        let dir_of = |r: &Option<crate::explorer::tree::Entry>| match r {
            Some(r) if r.dir => r.rel.clone(),
            Some(r) => parent_of(&r.rel).unwrap_or("").to_string(),
            None => String::new(),
        };
        if r.as_ref().is_some_and(|r| r.gone) && ON_DISK.contains(&action) {
            return self.flash(Some("Deleted: Discard changes in the menu restores it".into()));
        }
        let cursor = self.state.cursor;
        let (root, pane) = (self.root.clone(), self.pane.clone());
        let abs = r.as_ref().map(|r| self.abs_of(&r.rel));
        let not_git = || Some("Not a git repository".to_string());
        match action {
            "up" => self.select(cursor - 1),
            "down" => self.select(cursor + 1),
            "pageUp" => self.select(cursor - page),
            "pageDown" => self.select(cursor + page),
            "halfPageUp" => self.select(cursor - half),
            "halfPageDown" => self.select(cursor + half),
            "help" => return self.open_help(),
            "close" => return self.close(),
            "top" => self.select(0),
            "bottom" => self.select(self.state.rows.len() as i64 - 1),
            "open" => {
                if let Some(r) = r.as_ref().filter(|r| !r.dir) {
                    return self.open_file(r);
                }
                self.toggle(r, None);
            }
            "menu" => return self.open_menu((!self.state.rows.is_empty()).then_some(cursor)),
            "expand" => match &r {
                Some(e) if e.dir && !e.open => self.toggle(r, Some(true)),
                Some(e) if e.dir => self.select(cursor + 1),
                _ => {}
            },
            "collapse" => match &r {
                Some(e) if e.dir && e.open && self.state.filter.is_none() => self.toggle(r, Some(false)),
                Some(e) => {
                    let first = e.from.as_deref().filter(|f| !f.is_empty()).unwrap_or(&e.rel);
                    let up = parent_of(first).unwrap_or("").to_string();
                    if let Some(p) = self.state.rows.iter().position(|x| holds(x, &up)) {
                        self.select(p as i64);
                    }
                }
                None => {}
            },
            "refresh" => return self.schedule(),
            "filter" => {
                let value = self.state.filter.as_ref().map_or(String::new(), |f| f.field.value.clone());
                self.state.filter = Some(Filter { field: Field::new(&value), active: true });
                self.build_rows();
            }
            "goToFile" => return self.run_action(|_| later(move || actions::search(&root, &pane, "files", ""))),
            "searchInFiles" => {
                let dir = match &r {
                    Some(e) if e.dir => self.abs_of(&e.rel),
                    _ => root,
                };
                return self.run_action(|_| later(move || actions::search(&dir, &pane, "content", "")));
            }
            "sourceControl" => {
                let file = abs.unwrap_or_default();
                let rel = r.map(|r| r.rel).unwrap_or_default();
                return self.run_action(|ex| {
                    let Some(repo) = ex.repo_dir(&rel) else { return Act::Now(not_git()) };
                    later(move || actions::scm(&repo, &pane, &file, false))
                });
            }
            "settings" => return self.open_settings(),
            "toggleHidden" | "toggleIgnored" => {
                let (name, what) = if action == "toggleHidden" { ("hidden", "Hidden") } else { ("ignored", "Ignored") };
                let on = !self.settings.get(name).is_some_and(Value::truthy);
                self.set_setting(name, Value::Bool(on));
                return self.flash(Some(format!("{what} files {}", if on { "shown" } else { "hidden" })));
            }
            "newFile" => return self.new_entry(&dir_of(&r), false),
            "newFolder" => return self.new_entry(&dir_of(&r), true),
            "rename" => {
                if let Some(r) = &r {
                    self.rename_entry(r);
                }
                return;
            }
            "delete" => {
                if let Some(r) = &r {
                    self.delete_entry(r);
                }
                return;
            }
            "edit" => {
                if let (Some(abs), false) = (abs, r.as_ref().is_some_and(|r| r.dir)) {
                    self.run_action(|_| later(move || actions::edit(&abs, &pane, None)));
                }
                return;
            }
            "openInEditor" => {
                if let Some(abs) = abs {
                    self.run_action(|_| later(move || actions::editor(&abs, &pane)));
                }
                return;
            }
            "openInApp" => {
                if let Some(abs) = abs {
                    self.run_action(|_| later(move || actions::app(&abs, &pane)));
                }
                return;
            }
            "preview" => {
                if let (Some(abs), false) = (abs, r.as_ref().is_some_and(|r| r.dir)) {
                    let shown = actions::Shown { keep: true, ..Default::default() };
                    self.run_action(|_| later(move || actions::preview(&abs, &root, &pane, 0.0, true, true, &shown)));
                }
                return;
            }
            "copyPath" => {
                if let Some(abs) = abs {
                    self.run_action(|_| later(move || Some(actions::copy(&abs, "Path copied"))));
                }
                return;
            }
            "copyRelativePath" => {
                if let Some(rel) = r.map(|r| r.rel) {
                    self.run_action(|_| later(move || Some(actions::copy(&rel, "Path copied"))));
                }
                return;
            }
            "parentRoot" => return self.set_root(&paths::dirname(&root)),
            "changedOnly" => return self.show_changed(!self.state.changed),
            "viewChanges" => {
                if let Some(r) = r {
                    self.run_action(|ex| ex.view_changes(&r));
                }
                return;
            }
            "nextChange" => return self.next_change(1),
            "previousChange" => return self.next_change(-1),
            "fileHistory" => {
                if let (Some(abs), false) = (abs, r.as_ref().is_none_or(|r| r.dir)) {
                    let rel = r.map(|r| r.rel).unwrap_or_default();
                    self.run_action(|ex| {
                        let Some(repo) = ex.repo_dir(&rel) else { return Act::Now(not_git()) };
                        later(move || actions::scm(&repo, &pane, &abs, true))
                    });
                }
                return;
            }
            "narrower" => return self.widen(-4.0),
            "wider" => return self.widen(4.0),
            _ => return,
        }
        if MOVES.contains(&action) {
            self.following();
        }
        self.reveal();
        self.render();
    }

    fn mouse(&mut self, m: &Mouse) {
        let (w, h) = self.size();
        let h = h as i64;
        // A question: a click on a button answers, out of the card escapes; for confirm, any other click cancels.
        if let Some(choice) = &self.state.choice {
            let answer = ui::ask_mouse(m, &choice.ask);
            return self.choose(answer);
        }
        if let Some((q, _)) = &self.state.confirm {
            let Some(answer) = ui::confirm_mouse(m, q) else { return };
            let (_, yes) = self.state.confirm.take().unwrap();
            if answer == "yes" {
                return self.run_action(move |ex| yes(ex));
            }
            return self.render();
        }
        if m.wheel != 0
            && let Some(menu) = &mut self.state.menu
        {
            menu.move_by(m.wheel, h);
            return self.render();
        }
        if self.state.settings.is_some() {
            return self.settings_mouse(m);
        }
        if let Some(help) = &mut self.state.help {
            if !ui::help_mouse(m, help) {
                self.state.help = None;
            }
            return self.render();
        }
        if m.wheel != 0 {
            let max = (self.state.rows.len() as i64 - self.body_height()).max(0);
            self.state.top = (self.state.top + m.wheel * 3).min(max).max(0);
            return self.render();
        }
        match self.scrollbar.mouse(m, m.y - 2, m.x == w as i64 && self.state.menu.is_none(), Instant::now()) {
            BarHit::Ignored => {}
            BarHit::Took => return,
            BarHit::Scroll(top) => {
                self.state.top = top;
                return self.render();
            }
        }
        if !m.press || m.drag || (m.button != 0 && m.button != 2) {
            return;
        }
        let (btn, x, y) = (m.button, m.x, m.y);
        if self.state.prompt.is_some() {
            self.state.prompt = None;
            return self.render();
        }
        if let Some(menu) = &mut self.state.menu {
            // Click in the menu: action; elsewhere: close the menu (a right-click reopens it on the new row).
            let hit = if btn == 0 { menu.mouse(m, h) } else { Some(Choice::Close) };
            match hit {
                Some(Choice::Run { run, .. }) => return self.run_action(move |ex| (run.run)(ex)),
                Some(Choice::Close) => {}
                None => return,
            }
            self.state.menu = None;
            if btn == 0 {
                return self.render();
            }
        }
        if y == 1 {
            if btn != 0 {
                return self.render();
            }
            // Each icon and the cell after it: a Nerd Font glyph often spills over the next cell.
            if x >= w as i64 - 2 {
                return self.open_settings();
            }
            if x >= w as i64 - 2 - self.pill_width(w) as i64 && self.show_update() {
                return self.run_action(|ex| ex.pill_action());
            }
            return self.render();
        }
        if y > self.body_height() + 1 {
            if let Some(f) = &mut self.state.filter {
                f.active = true;
                self.render();
            }
            return;
        }
        let i = self.state.top + y - 2;
        if i >= self.state.rows.len() as i64 {
            return if btn == 2 { self.open_menu(None) } else { self.render() };
        }
        if btn == 2 {
            return self.open_menu(Some(i));
        }
        if let Some(f) = &mut self.state.filter {
            f.active = false;
        }
        let Some(r) = usize::try_from(i).ok().and_then(|i| self.state.rows.get(i)).cloned() else { return };
        let now = Instant::now();
        let double = self.state.last_click.is_some_and(|(at, t)| at == i && now - t < Duration::from_millis(400));
        self.state.last_click = if double { None } else { Some((i, now)) };
        self.select(i);
        if r.dir {
            self.toggle(Some(r), None);
        } else if double {
            return self.open_file(&r);
        } else if self.settings.get("singleClick").is_some_and(Value::truthy)
            && actions::can_preview(&self.abs_of(&r.rel))
        {
            let (abs, root, pane) = (self.abs_of(&r.rel), self.root.clone(), self.pane.clone());
            let view = if self.state.changed { "diff" } else { "file" };
            let shown = actions::Shown { view: view.into(), ..Default::default() };
            return self.run_action(|_| later(move || actions::preview(&abs, &root, &pane, 0.0, false, true, &shown)));
        }
        self.render();
    }
}
