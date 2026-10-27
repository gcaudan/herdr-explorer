//! Review notes in the preview: taken on the selected lines (a), shown in the text under the last line
//! they cover, sent to the agent (A), copied (C), the last one removed (U).

use super::diff::DiffData;
use super::{Kind, Preview, Row};
use crate::core::ui::{self, Field, Seg, c, fg, seg, text_width};
use crate::core::{exec, notes, paths};
use crate::diff::{self as diffs, Ref};
use crate::json::Value;

/// A note while it is typed: its field, its kind, its lines (0 for the whole file); lines and stale as notes::New
/// takes them.
pub struct Draft {
    field: Field,
    kind: String,
    from: i64,
    to: i64,
    lines: Option<Option<Vec<String>>>,
    stale: bool,
}

/// The colour of the notes in the text.
fn note_style() -> String {
    fg(c().yellow)
}

/// `where.replace(/^.*:/, 'L')`: up to the last colon of the first line.
fn line_label(at: &str) -> String {
    let first = at.find(['\n', '\r', '\u{2028}', '\u{2029}']).map_or(at, |i| &at[..i]);
    match first.rfind(':') {
        Some(i) => format!("L{}", &at[i + 1..]),
        None => at.to_string(),
    }
}

impl Preview {
    /// The review notes shown in the text: each one under the last line it covers, those on the whole file, the stale
    /// ones (their lines gone) and every one where the rows are not the file's lines (rendered Markdown) at the top.
    pub(super) fn with_notes(&self, list: Vec<Row>, width: usize) -> Vec<Row> {
        let s = &self.state;
        if s.notes.is_empty()
            || s.diff
            || s.show.is_some()
            || matches!(s.kind, Kind::Image | Kind::Error)
            || (list.len() == 1 && list[0].empty.is_some())
        {
            return list;
        }
        let lines = !self.markdown_view() && s.kind != Kind::Binary;
        let truthy = |n: &Value, k: &str| n.get(k).is_some_and(Value::truthy);
        let rows_of = |n: &Value| -> Vec<Row> {
            let stale = truthy(n, "stale");
            let file = n.get("file").and_then(Value::as_str).unwrap_or("");
            let at = if (lines && !stale) || !truthy(n, "from") {
                String::new()
            } else {
                format!("{} ", line_label(&notes::where_(n, Some(&paths::dirname(file)), false)))
            };
            let kind =
                n.get("kind").filter(|k| k.truthy()).map_or(String::new(), |k| format!("{}: ", crate::json::text(k)));
            let text = n.get("text").and_then(Value::as_str).unwrap_or("");
            let body = format!("✎ {at}{}{kind}{}", if stale { "stale, " } else { "" }, ui::clean_line(text, 4, None));
            let style = if stale { fg(c().dim) } else { note_style() };
            ui::split(&[seg(body, style)], (width as i64 - 2).max(1) as usize)
                .into_iter()
                .map(|segs| Row { segs: [vec![seg("  ", "")], segs].concat(), src: -1, ..Row::default() })
                .collect()
        };
        let len = s.plain.len() as f64;
        let mut out: Vec<Row> = s
            .notes
            .iter()
            .filter(|n| !lines || !truthy(n, "from") || truthy(n, "stale") || crate::json::num(n.get("to")) > len)
            .flat_map(&rows_of)
            .collect();
        if !lines {
            out.extend(list);
            return out;
        }
        for i in 0..list.len() {
            let (src, last) = (list[i].src, list.get(i + 1).is_none_or(|next| next.src != list[i].src));
            out.push(list[i].clone());
            if src >= 0 && last {
                let end = Value::Num((src + 1) as f64);
                out.extend(
                    s.notes
                        .iter()
                        .filter(|n| truthy(n, "from") && !truthy(n, "stale") && n.get("to") == Some(&end))
                        .flat_map(&rows_of),
                );
            }
        }
        out
    }

    /// Whether a line (0-based) has a note, for its number in the gutter.
    pub(super) fn noted(&self, src: i64) -> bool {
        let line = (src + 1) as f64;
        self.state.notes.iter().any(|n| {
            n.get("from").is_some_and(Value::truthy)
                && !n.get("stale").is_some_and(Value::truthy)
                && line >= crate::json::num(n.get("from"))
                && line <= crate::json::num(n.get("to"))
        })
    }

    /// The style of a noted line's number.
    pub(super) fn noted_style() -> String {
        format!("{}\x1b[1m", note_style())
    }

    /// a: a note on the selected lines, else the highlighted one, else the whole file. In the Diff view, the anchor is
    /// the text of the lines in the version shown, not on disk (notes::add).
    pub(super) fn start_note(&mut self) {
        if self.tab.is_empty() {
            return self.flash(Some("Notes need the explorer".into()));
        }
        if self.state.file.is_empty() || self.state.kind == Kind::Error {
            return;
        }
        let at = if self.state.diff {
            self.diff_lines()
        } else {
            self.selected_lines().map(|(a, b)| (a, b, Some(String::new())))
        };
        let parsed = match (&self.state.diff_data, at.as_ref()) {
            (Some(DiffData::Done { parsed, top, .. }), Some(at)) if self.state.diff => Some((parsed.clone(), top, at)),
            _ => None,
        };
        let (mut lines, mut disk) = (None, None);
        if let Some((parsed, top, (from, to, sec))) = parsed {
            let rel = paths::relative(top, &exec::real_path(&self.state.file));
            let file = if paths::SEP == '/' { rel } else { rel.replace(paths::SEP, "/") };
            lines = Some(sec.as_ref().and_then(|sec| {
                diffs::shown_lines(&parsed, &Ref { file: file.clone(), from: *from, to: *to, sec: sec.clone() })
            }));
            // A staged line: placed where it is on disk, which notes follow; stale when changed there since.
            if sec.as_deref() == Some("staged") {
                let r = Ref { file, from: *from, to: *to, sec: "staged".into() };
                disk = Some(diffs::disk_lines(&parsed, &r));
            }
        }
        let (from, to) = match disk.flatten() {
            Some(d) => d,
            None => at.map_or((0, 0), |(a, b, _)| (a, b)),
        };
        let stale = disk.is_some_and(|d| d.is_none());
        self.state.note = Some(Draft { field: Field::new(""), kind: String::new(), from, to, lines, stale });
        self.render();
    }

    /// While a note is typed: Enter saves it, Esc cancels, Tab changes its kind; other keys edit it.
    pub(super) fn note_key(&mut self, k: &str) {
        let Some(n) = self.state.note.as_mut() else { return };
        match k {
            "escape" | "ctrl+c" => {
                self.state.note = None;
                self.render()
            }
            "tab" | "shift+tab" => {
                let len = notes::KINDS.len();
                let i = notes::KINDS.iter().position(|x| *x == n.kind).map_or(-1, |i| i as i64);
                let step = if k == "tab" { 1 } else { len as i64 - 1 };
                n.kind = notes::KINDS[(i + step).rem_euclid(len as i64) as usize].to_string();
                self.render()
            }
            "enter" => {
                let Some(n) = self.state.note.take() else { return };
                if n.field.value.chars().all(crate::unicode::is_space) {
                    return self.render();
                }
                self.state.select = None;
                let note = notes::New {
                    file: self.state.file.clone(),
                    from: n.from.max(0) as u64,
                    to: Some(n.to.max(0) as u64),
                    kind: n.kind,
                    text: n.field.value,
                    lines: n.lines,
                    stale: n.stale,
                };
                let msg = notes::add(&self.tab, note);
                self.keep_tab(self.tabs.current);
                self.load_notes();
                self.flash(Some(msg));
            }
            _ => {
                if n.field.key(k) {
                    self.render();
                }
            }
        }
    }

    /// The footer while a note is typed: its lines, its kind, its field.
    pub(super) fn note_footer(&self, w: usize) -> Option<String> {
        let n = self.state.note.as_ref()?;
        let lines = if n.from == 0 {
            "file".to_string()
        } else if n.to > n.from {
            format!("L{}-{}", n.from, n.to)
        } else {
            format!("L{}", n.from)
        };
        let lead = format!(" ✎ {lines} ");
        let kind = if n.kind.is_empty() { "[tab: kind] ".to_string() } else { format!("[{}] ", n.kind) };
        let room = (w as i64 - text_width(&format!("{lead}{kind}")) as i64).max(1);
        let mut segs: Vec<Seg> = vec![seg(lead, note_style()), seg(kind, fg(c().dim))];
        segs.extend(n.field.segments(room, ""));
        Some(ui::line(&segs, w, Some(c().header)))
    }

    /// A paste into the note typed: whether there was one.
    pub(super) fn note_paste(&mut self, text: &str) -> bool {
        let Some(n) = self.state.note.as_mut() else { return false };
        n.field.insert(text);
        self.render();
        true
    }

    /// The tab's notes on the shown file, read again when they change (taken or sent in another pane) and placed
    /// again when the file does (notes::current, this file only: the pane showing another one places its own).
    pub(super) fn read_notes(&mut self, place: bool) {
        let file = (!self.state.file.is_empty()).then(|| exec::real_path(&self.state.file));
        let all = match &file {
            Some(f) if place => notes::current(&self.tab, Some(f)),
            _ => notes::read(&self.tab),
        };
        self.state.note_count = all.len();
        let file = Value::Str(file.unwrap_or_default());
        self.state.notes = all.into_iter().filter(|n| n.get("file") == Some(&file)).collect();
    }

    pub(super) fn load_notes(&mut self) {
        let was = crate::json::stringify(&Value::Arr(self.state.notes.clone()));
        self.read_notes(false);
        if crate::json::stringify(&Value::Arr(self.state.notes.clone())) != was {
            self.state.rows = None;
        }
        self.render();
    }
}
