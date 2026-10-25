//! A pane's screen: the alternate screen, hidden cursor, no line wrap, mouse (SGR), focus reports and bracketed
//! paste; raw input decoded into events; the lines changed since the last frame painted; everything restored on
//! exit. With the theme on auto, the terminal's background asked (OSC 11) and the theme following it. `restart` hands
//! the terminal over to the next process (the pane's code updated on disk).

use super::exec;
use super::ui::{self, Event};
use crate::json::Value;
use crate::sys;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The exit code of a pane that asks its launcher to start it again.
pub const RESTART: i32 = 75;

/// The terminal given over to the next process by restart(): nothing more is painted nor restored.
static HANDED: AtomicBool = AtomicBool::new(false);

/// What the screen reports: mouse on (motion: drags too), focus reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub mouse: bool,
    pub motion: bool,
    pub focus: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { mouse: true, motion: false, focus: true }
    }
}

/// The sequences that set the screen's modes, and those that put them back.
pub fn modes(o: Options) -> (String, String) {
    let mouse_on = if o.mouse {
        format!("{}\x1b[?1006h", if o.motion { "\x1b[?1002h" } else { "\x1b[?1000h" })
    } else {
        String::new()
    };
    let on = format!("\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h{mouse_on}{}", if o.focus { "\x1b[?1004h" } else { "" });
    let off = format!(
        "{}{}\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l",
        if o.focus { "\x1b[?1004l" } else { "" },
        if o.mouse { "\x1b[?1006l\x1b[?1002l\x1b[?1000l" } else { "" }
    );
    (on, off)
}

/// What a pane gets from its screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A key, a mouse event, a focus report, a paste, or a terminal's answer other than its background.
    Ui(Event),
    /// The terminal's size changed: the next frame is painted whole.
    Resize,
    /// The theme followed the terminal's background (theme on auto): the pane works out its styles again and
    /// paints (the JS calls its theme handler, else its resize one).
    Theme,
    /// A `sys::Waker` was called.
    Wake,
}

// --- input split between reads

/// The input as it comes, read by read, cut into what can be decoded now. A long paste arrives in several reads:
/// held until its end marker, so that its text never reads as keys. So does a terminal's answer (OSC, APC) cut
/// between reads, for 100 ms at most: alt+] or alt+_ alone then reads as keys. A character cut between two reads
/// waits for its other bytes, as Node's UTF-8 decoding of stdin does.
#[derive(Debug, Default)]
pub struct Splitter {
    pending: String,
    carry: Vec<u8>,
    flush: Option<Instant>,
}

/// How long an answer cut between reads is held.
const HOLD: Duration = Duration::from_millis(100);

impl Splitter {
    /// The text of a read to decode now (maybe none).
    pub fn feed(&mut self, bytes: &[u8], now: Instant) -> String {
        self.flush = None;
        let mut data = std::mem::take(&mut self.pending) + &self.utf8(bytes);
        if let Some(start) = data.rfind("\x1b[200~")
            && !data[start..].contains("\x1b[201~")
        {
            self.pending = data.split_off(start);
        }
        if self.pending.is_empty()
            && let Some(reply) = unended(&data)
        {
            self.pending = data.split_off(reply);
            self.flush = Some(now + HOLD);
        }
        data
    }

    /// When the answer held must be decoded as it is.
    pub fn due(&self) -> Option<Instant> {
        self.flush
    }

    /// The answer held, once its time is up.
    pub fn tick(&mut self, now: Instant) -> Option<String> {
        if self.flush.is_some_and(|t| now >= t) {
            self.flush = None;
            return Some(std::mem::take(&mut self.pending));
        }
        None
    }

    /// The bytes as text, a character cut at the end kept for the next read; invalid bytes as U+FFFD.
    fn utf8(&mut self, bytes: &[u8]) -> String {
        self.carry.extend_from_slice(bytes);
        let tail = self.carry.split_off(self.carry.len() - incomplete(&self.carry));
        let text = String::from_utf8_lossy(&self.carry).into_owned();
        self.carry = tail;
        text
    }
}

/// The length of a character's start at the end of buf, its other bytes still to come.
fn incomplete(buf: &[u8]) -> usize {
    for k in 1..=buf.len().min(3) {
        let b = buf[buf.len() - k];
        if b & 0xc0 != 0x80 {
            let need = match b {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => 1,
            };
            return if need > k { k } else { 0 };
        }
    }
    0
}

/// Where a terminal's answer (OSC, APC) starts that its end has not reached; only after the last paste, whose text
/// may hold an ESC ] of its own.
fn unended(data: &str) -> Option<usize> {
    let pasted = data.rfind("\x1b[201~");
    let start = data.rfind("\x1b]").max(data.rfind("\x1b_"))?;
    let ended = data[start + 2..].contains('\x07') || data[start + 2..].contains("\x1b\\");
    (pasted.is_none_or(|p| start > p) && !ended).then_some(start)
}

// --- painting

/// The sequences that paint the lines changed since shown, each at its row, shown then set to lines; empty when none
/// changed. A whole frame is large, and on Windows a write to the terminal blocks until it is read.
pub fn frame(shown: &mut Vec<String>, lines: &[String]) -> String {
    let mut s = String::new();
    for (i, l) in lines.iter().enumerate() {
        if shown.get(i) != Some(l) {
            s += &format!("\x1b[{};1H{l}", i + 1);
        }
    }
    *shown = lines.to_vec();
    if s.is_empty() { s } else { format!("\x1b[?2026h{s}\x1b[?2026l") }
}

// --- the screen

/// The terminal's size: columns and rows, 80 and 24 when unknown.
pub fn size() -> (usize, usize) {
    let (w, h) = sys::size_or_default();
    (w as usize, h as usize)
}

/// A pane's screen, set up by open() and restored when dropped or by exit().
pub struct Screen {
    events: sys::Events,
    raw: Option<sys::RawMode>,
    off: String,
    shown: Vec<String>,
    split: Splitter,
}

impl Screen {
    /// The screen's modes set, the terminal's background asked when the theme is on auto, stdin in raw mode.
    pub fn open(o: Options) -> io::Result<Screen> {
        let (on, off) = modes(o);
        // What can fail first: the modes once set are always put back.
        let events = sys::Events::new()?;
        let raw = if sys::stdin_is_tty() { Some(sys::RawMode::enable()?) } else { None };
        let screen = Screen { events, raw, off, shown: Vec::new(), split: Splitter::default() };
        let ask = if ui::theme_setting() == "auto" { "\x1b]11;?\x07" } else { "" };
        sys::write_stdout(format!("{on}{ask}").as_bytes())?;
        Ok(screen)
    }

    pub fn waker(&self) -> sys::Waker {
        self.events.waker()
    }

    /// The events seen, waiting up to timeout (forever with None) for the first. Returns early, maybe with none, when
    /// an answer held from a cut read is decoded. An ending signal (SIGTERM, SIGHUP, SIGINT) exits at once, the
    /// screen restored, with code 0.
    pub fn wait(&mut self, timeout: Option<Duration>) -> Vec<Input> {
        let now = Instant::now();
        let held = self.split.due().map(|t| t.saturating_duration_since(now));
        let timeout = match (timeout, held) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let mut out = Vec::new();
        // An answer held past its time is decoded before the next read, as Node runs its timer first.
        self.held(&mut out);
        for e in self.events.wait(timeout) {
            match e {
                sys::Event::Input(bytes) => {
                    self.held(&mut out);
                    let text = self.split.feed(&bytes, Instant::now());
                    self.decode(&text, &mut out);
                }
                sys::Event::Eof => {}
                sys::Event::Resize => {
                    self.shown.clear();
                    out.push(Input::Resize);
                }
                sys::Event::Signal(_) => self.exit(0),
                sys::Event::Wake => out.push(Input::Wake),
            }
        }
        self.held(&mut out);
        out
    }

    /// The answer held from a cut read, decoded as it is once its time is up.
    fn held(&mut self, out: &mut Vec<Input>) {
        if let Some(text) = self.split.tick(Instant::now()) {
            self.decode(&text, out);
        }
    }

    fn decode(&mut self, text: &str, out: &mut Vec<Input>) {
        for e in ui::decode(text) {
            match e {
                Event::Reply(t) => self.reply(t, out),
                e => out.push(Input::Ui(e)),
            }
        }
    }

    /// The terminal's background (theme on auto): kept in the state folder, so that the next panes open in it at
    /// once, and the theme turned to it.
    fn reply(&mut self, t: String, out: &mut Vec<Input>) {
        let seen = if t.starts_with("\x1b]11;") { ui::background_of(&t) } else { None };
        let Some(seen) = seen else {
            out.push(Input::Ui(Event::Reply(t)));
            return;
        };
        if let Some(file) = ui::background_file()
            && seen != ui::seen_background()
        {
            exec::write_atomic(&file, seen);
        }
        if seen == ui::theme_name() || ui::theme_setting() != "auto" {
            return;
        }
        ui::use_theme(seen);
        self.shown.clear();
        out.push(Input::Theme);
    }

    /// The lines changed since the last frame written, each at its row. Nothing once the terminal is handed over.
    pub fn paint(&mut self, lines: &[String]) {
        if HANDED.load(Ordering::Relaxed) {
            return;
        }
        let s = frame(&mut self.shown, lines);
        if !s.is_empty() {
            let _ = sys::write_stdout(s.as_bytes());
        }
    }

    /// The next frame whole: after something else wrote on the screen.
    pub fn again(&mut self) {
        self.shown.clear();
    }

    /// The screen restored, then the process ended with code.
    pub fn exit(&mut self, code: i32) -> ! {
        self.restore();
        std::process::exit(code)
    }

    fn restore(&mut self) {
        if !HANDED.load(Ordering::Relaxed) && !self.off.is_empty() {
            let _ = sys::write_stdout(self.off.as_bytes());
        }
        self.off.clear();
        self.raw = None;
    }

    /// Runs the pane again, on the code updated on disk, in the same terminal: the pane keeps its place, width and
    /// id. This process hands the terminal over and exits; its launcher starts the new one. env: what the new process
    /// must show, added to this one's environment, sent to the launcher through the file EXPLORER_IPC_FILE names.
    pub fn restart(&mut self, env: &[(String, String)]) {
        if HANDED.swap(true, Ordering::Relaxed) {
            return;
        }
        let _ = sys::write_stdout(self.off.as_bytes());
        self.off.clear();
        self.raw = None;
        send(&Value::Obj(vec![(
            "restart".into(),
            Value::Obj(env.iter().map(|(k, v)| (k.clone(), Value::Str(v.clone()))).collect()),
        )]));
        std::process::exit(RESTART)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        self.restore();
    }
}

/// A message to the launcher (the JS's process.send): a line of JSON appended to the file EXPLORER_IPC_FILE names;
/// dropped without one.
pub fn send(message: &Value) {
    use std::io::Write;
    let Some(file) = std::env::var_os("EXPLORER_IPC_FILE") else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(file) {
        let _ = f.write_all(format!("{}\n", crate::json::stringify(message)).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(s: &mut Splitter, text: &str, now: Instant) -> String {
        s.feed(text.as_bytes(), now)
    }

    #[test]
    fn the_modes_and_their_reverse() {
        let (on, off) = modes(Options::default());
        assert_eq!(on, "\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1004h");
        assert_eq!(off, "\x1b[?1004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l");
        let (on, off) = modes(Options { mouse: true, motion: true, focus: false });
        assert_eq!(on, "\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h\x1b[?1002h\x1b[?1006h");
        assert_eq!(off, "\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l");
        let (on, off) = modes(Options { mouse: false, motion: false, focus: false });
        assert_eq!(
            (on.as_str(), off.as_str()),
            ("\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h", "\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l")
        );
    }

    #[test]
    fn a_paste_is_held_until_its_end() {
        let (mut s, now) = (Splitter::default(), Instant::now());
        assert_eq!(feed(&mut s, "a\x1b[200~one", now), "a");
        assert_eq!(feed(&mut s, " two", now), "");
        assert_eq!(feed(&mut s, " three\x1b[201~b", now), "\x1b[200~one two three\x1b[201~b");
        assert_eq!(s.due(), None);
    }

    #[test]
    fn an_answer_cut_between_reads_is_held_100_ms() {
        let (mut s, now) = (Splitter::default(), Instant::now());
        assert_eq!(feed(&mut s, "x\x1b]11;rgb:ffff", now), "x");
        assert_eq!(s.due(), Some(now + HOLD));
        assert_eq!(s.tick(now + Duration::from_millis(99)), None);
        // Its end comes: one answer.
        assert_eq!(feed(&mut s, "/ffff/ffff\x07", now), "\x1b]11;rgb:ffff/ffff/ffff\x07");
        assert_eq!(s.due(), None);
        // It never comes: alt+_ reads as keys after 100 ms.
        assert_eq!(feed(&mut s, "\x1b_", now), "");
        assert_eq!(s.tick(now + HOLD), Some("\x1b_".to_string()));
        assert_eq!((s.due(), s.tick(now + HOLD)), (None, None));
        // Ended by ST, or inside a paste: decoded at once.
        assert_eq!(feed(&mut s, "\x1b_Gi=31;OK\x1b\\", now), "\x1b_Gi=31;OK\x1b\\");
        assert_eq!(feed(&mut s, "\x1b[200~\x1b]x\x1b[201~", now), "\x1b[200~\x1b]x\x1b[201~");
        assert_eq!(s.due(), None);
    }

    #[test]
    fn a_character_cut_between_reads_waits_for_its_bytes() {
        let (mut s, now) = (Splitter::default(), Instant::now());
        let e = "é中😀".as_bytes();
        assert_eq!(s.feed(&e[..1], now), "");
        assert_eq!(s.feed(&e[1..4], now), "é");
        assert_eq!(s.feed(&e[4..7], now), "中");
        assert_eq!(s.feed(&e[7..], now), "😀");
        assert_eq!(s.feed(&[0xff, b'a'], now), "\u{fffd}a");
    }

    #[test]
    fn only_the_lines_changed_are_painted() {
        let mut shown = Vec::new();
        let lines = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(frame(&mut shown, &lines(&["a", "b"])), "\x1b[?2026h\x1b[1;1Ha\x1b[2;1Hb\x1b[?2026l");
        assert_eq!(frame(&mut shown, &lines(&["a", "b"])), "");
        assert_eq!(frame(&mut shown, &lines(&["a", "c", "d"])), "\x1b[?2026h\x1b[2;1Hc\x1b[3;1Hd\x1b[?2026l");
        shown.clear();
        assert_eq!(frame(&mut shown, &lines(&["a"])), "\x1b[?2026h\x1b[1;1Ha\x1b[?2026l");
    }
}
