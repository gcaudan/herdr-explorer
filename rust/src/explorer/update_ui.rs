//! The explorer's side of the update (explorer.js): the newer release asked about, installed, skipped; the installer
//! to run again; the header's pill; the restart on the new code.

use super::{Act, Choice, Explorer, Run, later};
use crate::core::actions;
use crate::core::ui::{Ask, Button};
use crate::core::{update, worker};
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

fn button(key: &str, label: &str, tone: &str) -> Button {
    Button { key: key.into(), label: label.into(), tone: tone.into() }
}

impl Explorer {
    /// update.json changed (the daily check, a skip, an update in another tab): what it says, read again.
    pub(super) fn update_changed(&mut self) {
        self.latest = update::available();
        self.skipped = update::skipped();
        self.finish = update::finishing();
        self.render();
    }

    /// The newer release: the versions, its first changes (the release's text, kept by the daily check), and the
    /// answers: update, its notes in the browser, skip it (not for one skipped already), later (focused, as escape).
    /// Installed by herdr: herdr's update command copied instead, since only herdr may move its checkout (herdr not
    /// answering: the update, whose checks refuse such a checkout).
    pub(super) fn ask_update(&mut self) -> Act {
        if self.updating {
            return Act::Now(Some("Update in progress".into()));
        }
        Act::Later(Box::new(|| {
            if update::managed() {
                let text = actions::copy(&update::herdr_update(), "Update command copied");
                return Box::new(move |ex: &mut Explorer| ex.flash(Some(text)));
            }
            Box::new(|ex: &mut Explorer| {
                let text = ex.update_choice();
                ex.flash(text)
            })
        }))
    }

    fn update_choice(&mut self) -> Option<String> {
        let to = self.latest.clone().or_else(|| self.skipped.clone())?;
        let (notes, url) = update::release();
        let mut buttons = vec![button("y", "Update", "go"), button("r", "Notes", "")];
        if self.latest.is_some() {
            buttons.push(button("s", "Skip version", ""));
        }
        buttons.push(button("n", "Later", ""));
        let note = if notes.len() > 3 {
            format!("and {} more (r)", notes.len() - 3)
        } else {
            "The panes restart on the new code.".into()
        };
        let focus = buttons.len() - 1;
        let install: Run = {
            let to = to.clone();
            Rc::new(move |ex: &mut Explorer| ex.install(&to))
        };
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
            on: vec![("y".into(), install), ("r".into(), notes_page), ("s".into(), skip)],
        });
        self.render();
        None
    }

    /// The update, on a thread of its own as it may take minutes: the panes then restart on the new code by
    /// themselves (VersionWatch). Refused: the reason only, since the command would bypass the checks. Failed: the
    /// command copied, to run in the terminal. "Updating…" stays until it ends.
    fn install(&mut self, to: &str) -> Act {
        self.updating = true;
        self.flash_at = None;
        self.state.flash = Some("Updating…".into());
        self.render();
        let to = to.to_string();
        let job: super::Job = Box::new(move || {
            let text = match std::panic::catch_unwind(AssertUnwindSafe(|| update::install(&to))) {
                Err(_) => "Update failed".to_string(),
                Ok(o) => match o.retry.filter(|_| !o.ok) {
                    None => o.message,
                    Some(command) => {
                        format!("{}: {}", o.message, actions::copy(&command, "update command copied"))
                    }
                },
            };
            Box::new(move |ex: &mut Explorer| {
                ex.updating = false;
                ex.state.flash = None;
                ex.flash(Some(text));
            })
        });
        if !worker::spawn(self.tx.clone(), self.waker.clone(), job) {
            self.updating = false;
            self.state.flash = None;
            return Act::Now(Some("Update failed".into()));
        }
        Act::Now(None)
    }

    /// The update the installer must finish: its command shown, then copied to run in a terminal; the ! finish then
    /// goes away in every explorer.
    pub(super) fn finish_update(&mut self) -> Act {
        let copy: Run = Rc::new(|_: &mut Explorer| {
            later(|| {
                let done = actions::copy(&update::installer(), "Installer command copied");
                if done != "Clipboard unavailable" {
                    update::finished();
                }
                Some(done)
            })
        });
        self.state.menu = None;
        self.state.choice = Some(Choice {
            ask: Ask {
                title: format!("Finish the {} update", self.finish.as_deref().unwrap_or("")),
                name: update::installer(),
                note: Some("Run it once in a terminal.".into()),
                buttons: vec![button("c", "Copy command", "go"), button("n", "Later", "")],
                focus: 0,
                ..Ask::default()
            },
            on: vec![("c".into(), copy)],
        });
        self.render();
        Act::Now(None)
    }

    /// What the header's pill does (render.rs pill): finish, update, or the notes of the update just done.
    pub(super) fn pill_action(&mut self) -> Act {
        let check = self.settings.get("updateCheck").is_some_and(crate::json::Value::truthy);
        if self.finish.is_some() {
            self.finish_update()
        } else if self.latest.is_some() && check && !self.updating {
            self.ask_update()
        } else {
            let (_, url) = update::release();
            later(move || Some(actions::web(&url, "the release notes")))
        }
    }

    /// Nothing typed nor chosen, no update running: the explorer may restart on the new code.
    pub(super) fn idle_for_update(&self) -> bool {
        let s = &self.state;
        !self.updating
            && s.prompt.is_none()
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
        self.update_watch = None;
        self.requests = None;
        self.screen.restart(&[("EXPLORER_TREE".into(), self.root.clone())]);
    }
}
