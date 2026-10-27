//! The explorer's side of the update: the newer release asked about, its command copied, skipped; the header's pill;
//! the restart on the new code.

use super::{Act, Choice, Explorer, Run, later};
use crate::core::actions;
use crate::core::ui::{Ask, Button};
use crate::core::{exec, paths, setup, update};
use std::rc::Rc;

fn button(key: &str, label: &str, tone: &str) -> Button {
    Button { key: key.into(), label: label.into(), tone: tone.into() }
}

impl Explorer {
    /// update.json changed (the daily check, a skip in another tab): what it says, read again.
    pub(super) fn update_changed(&mut self) {
        self.latest = update::available();
        self.skipped = update::skipped();
        self.render();
    }

    /// The newer release: the versions, its first changes (the release's text, kept by the daily check), and the
    /// answers: herdr's update command copied (herdr's index is the only channel), its notes in the browser, skip it
    /// (not for one skipped already), later (focused, as escape).
    pub(super) fn ask_update(&mut self) -> Act {
        let Some(to) = self.latest.clone().or_else(|| self.skipped.clone()) else { return Act::Now(None) };
        let (notes, url) = update::release();
        let mut buttons = vec![button("c", "Copy command", "go"), button("r", "Notes", "")];
        if self.latest.is_some() {
            buttons.push(button("s", "Skip version", ""));
        }
        buttons.push(button("n", "Later", ""));
        let note = if notes.len() > 3 {
            format!("and {} more (r)", notes.len() - 3)
        } else {
            "Copies herdr's update command.".into()
        };
        let focus = buttons.len() - 1;
        let copy: Run = Rc::new(|_: &mut Explorer| {
            later(|| {
                Some(actions::copy(&update::herdr_update(), "Update command copied: run it, then reopen the panes"))
            })
        });
        let notes_page: Run = Rc::new(move |_: &mut Explorer| {
            let url = url.clone();
            later(move || Some(actions::web(&url, "the release notes")))
        });
        let skip: Run = {
            let to = to.clone();
            Rc::new(move |_: &mut Explorer| {
                update::skip(&to);
                Act::Now(Some(format!("{to} skipped: Settings to update anyway")))
            })
        };
        self.state.menu = None;
        self.state.choice = Some(Choice {
            ask: Ask {
                title: "Update herdr-explorer".into(),
                name: format!("{} → {to}", update::current().unwrap_or("?")),
                list: notes.into_iter().take(3).collect(),
                note: Some(note),
                buttons,
                focus,
                ..Ask::default()
            },
            on: vec![("c".into(), copy), ("r".into(), notes_page), ("s".into(), skip)],
        });
        self.render();
        Act::Now(None)
    }

    /// No key binding of the plugin in herdr's config, and setup never offered: offered once, in every explorer
    /// (setup-offered in the state folder), the dialog answered or not.
    pub(super) fn offer_setup(&mut self) {
        let Some(marker) = exec::state_dir().map(|d| paths::join(&[&d, "setup-offered"])) else { return };
        if std::fs::metadata(&marker).is_ok() || setup::bound() {
            return;
        }
        exec::write_atomic(&marker, "");
        let run: Run = Rc::new(|_: &mut Explorer| {
            later(|| {
                Some(match setup::setup() {
                    Ok(_) => "Key bindings added".into(),
                    Err(e) => e,
                })
            })
        });
        self.state.choice = Some(Choice {
            ask: Ask {
                title: "Set up herdr-explorer".into(),
                name: "prefix+f explorer · prefix+a go to file · prefix+d source control".into(),
                note: Some("Adds these keys to herdr's config.toml and links the herdr-preview skill for Claude Code.                             Later: herdr-explorer setup"
                    .into()),
                buttons: vec![button("y", "Set up", "go"), button("n", "Later", "")],
                focus: 0,
                ..Ask::default()
            },
            on: vec![("y".into(), run)],
        });
    }

    /// What the header's pill does (render.rs pill): the update, or the notes of the update just made.
    pub(super) fn pill_action(&mut self) -> Act {
        let check = self.settings.get("updateCheck").is_some_and(crate::json::Value::truthy);
        if self.latest.is_some() && check {
            self.ask_update()
        } else {
            let (_, url) = update::release();
            later(move || Some(actions::web(&url, "the release notes")))
        }
    }

    /// Nothing typed nor chosen: the explorer may restart on the new code.
    pub(super) fn idle_for_update(&self) -> bool {
        let s = &self.state;
        s.prompt.is_none()
            && s.confirm.is_none()
            && s.choice.is_none()
            && s.menu.is_none()
            && s.settings.is_none()
            && s.help.is_none()
            && !s.filter.as_ref().is_some_and(|f| f.active)
    }

    /// Updated plugin: this explorer starts again on the new code, at the same root. This process stops watching and
    /// acting, the new one taking over the pane.
    pub(super) fn restart_on_update(&mut self) {
        self.stopped = true;
        self.save();
        self.write_pane_state();
        self.root_watcher = None;
        self.git_watcher = None;
        self.keys_watch = None;
        self.settings_watch = None;
        self.shown_watch = None;
        self.update_watch = None;
        self.requests = None;
        self.screen.restart(&[("EXPLORER_TREE".into(), self.root.clone())]);
    }
}
