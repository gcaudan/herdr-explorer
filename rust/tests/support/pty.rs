//! Screen captures (Linux): a scenario (`rust/tests/fixtures/screens/<pane>/<name>.keys`) runs a pane in a pty of a
//! fixed size, in a folder of its own with the fake herdr, types keys, waits for the output to settle, and gives the
//! final screen (`vt::Screen::text`) and the calls made to herdr. The JS capture is recorded next to the scenario
//! (`.screen`, `.calls`); the Rust binary must give the same.
//!
//! A scenario holds one directive per line, run in order (`#` starts a comment):
//!   size <cols> <rows>          the terminal's size at start (default 80 24)
//!   file <path> <text>          a file of the working folder; \n, \t, \e and \\ escaped in text
//!   dir <path>                  a folder of the working folder
//!   state <path> <text>         a file of the plugin's state folder ($ROOT: the scenario's folder)
//!   config <path> <text>        a file of the plugin's config folder
//!   home <path> <text>          a file of the pane's home folder (VS Code icon themes)
//!   herdr <a> <b> <text>        the fake herdr's answer to `herdr <a> <b> …`
//!   env <NAME>=<value>          a variable for the pane ($ROOT: the scenario's folder)
//!   git <args>                  git run in the working folder (fixed author and dates), before start
//!   args <arg>…                 the pane's arguments
//!   start                       starts the pane, then waits for its output to settle
//!   keys <key>…                 keys typed (names: up, enter, ctrl+c, space…; else the characters), then a settle
//!   text <text>                 text typed as it is (escapes as for file), then a settle
//!   wait <ms>                   a pause
//!   until <ms>                  a pause until ms after the start, then a settle: the pane's timers due by then
//!                               (its width checked at 300, 1000 and 2500 ms) have run, whatever the keys took
//!   resize <cols> <rows>        the terminal resized, then a settle
//!
//! The pane runs with a cleaned environment: no HERDR_* of the session,
//! the fake herdr first in PATH, fixed HOME, TERM and locale, the state and config folders under the scenario's.

use super::vt::Screen;
use herdr_explorer::sys::pty;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn screens_dir() -> PathBuf {
    repo().join("rust/tests/fixtures/screens")
}

/// What a scenario gave.
pub struct Capture {
    pub screen: String,
    pub calls: String,
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('r') => out.push('\r'),
            // \xNN: a character below 0x80 (a NUL in a binary file).
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(char::from(u8::from_str_radix(&hex, 16).unwrap()));
            }
            Some(c) => out.push(c),
            None => out.push('\\'),
        }
    }
    out
}

/// The bytes a key name stands for, as a terminal sends them.
fn key_bytes(name: &str) -> Vec<u8> {
    let seq = match name {
        "up" => "\x1b[A",
        "down" => "\x1b[B",
        "right" => "\x1b[C",
        "left" => "\x1b[D",
        "home" => "\x1b[H",
        "end" => "\x1b[F",
        "pageup" => "\x1b[5~",
        "pagedown" => "\x1b[6~",
        "delete" => "\x1b[3~",
        "enter" => "\r",
        "tab" => "\t",
        "shift+tab" => "\x1b[Z",
        "backspace" => "\x7f",
        "escape" => "\x1b",
        "space" => " ",
        "f1" => "\x1bOP",
        "f2" => "\x1bOQ",
        _ => "",
    };
    if !seq.is_empty() {
        return seq.as_bytes().to_vec();
    }
    if let Some(c) = name.strip_prefix("ctrl+").filter(|c| c.len() == 1) {
        return vec![c.as_bytes()[0] & 0x1f];
    }
    if let Some(c) = name.strip_prefix("alt+").filter(|c| c.chars().count() == 1) {
        return format!("\x1b{c}").into_bytes();
    }
    name.as_bytes().to_vec()
}

struct Running {
    master: OwnedFd,
    child: std::process::Child,
    /// What the pane wrote, and when it last wrote (None: nothing yet).
    output: Arc<Mutex<(Vec<u8>, Option<Instant>)>>,
    /// When the last keys were sent: the pane's answer comes after.
    sent: Instant,
    /// When the pane started.
    started: Instant,
}

impl Drop for Running {
    /// The pane and what it started, as one process group (setsid): never left running, even after a panic.
    fn drop(&mut self) {
        // SAFETY: a signal to the group the child leads.
        unsafe { kill(-(self.child.id() as i32), 9) };
        let _ = self.child.wait();
    }
}

impl Running {
    fn send(&mut self, bytes: &[u8]) {
        let mut f = std::fs::File::from(self.master.try_clone().unwrap());
        f.write_all(bytes).unwrap();
        self.sent = Instant::now();
    }

    /// Waits for the pane's first output, then until nothing was written for 400 ms since it last wrote and since
    /// the last keys were sent (10 s at most).
    fn settle(&self) {
        let start = Instant::now();
        let quiet = Duration::from_millis(400);
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if start.elapsed() > Duration::from_secs(10) {
                return;
            }
            let Some(last) = self.output.lock().unwrap().1 else { continue };
            if last.elapsed() > quiet && self.sent.elapsed() > quiet {
                return;
            }
        }
    }
}

/// Holds a scenario's folder for one run at a time, across test threads and processes (flock).
pub struct Lock(#[allow(dead_code)] std::fs::File);

pub fn lock(root: &Path) -> Lock {
    unsafe extern "C" {
        fn flock(fd: i32, op: i32) -> i32;
    }
    std::fs::create_dir_all(root.parent().unwrap()).unwrap();
    let f = std::fs::File::create(root.with_extension("lock")).unwrap();
    // SAFETY: LOCK_EX (2) on a descriptor this function owns; released when the file is closed.
    assert_eq!(unsafe { flock(f.as_raw_fd(), 2) }, 0);
    Lock(f)
}

/// Runs a scenario with `command` (the pane's program and its first arguments, the scenario's `args` after them) in
/// the folder `root`, emptied first.
pub fn run(scenario: &Path, command: &[OsString], root: &Path) -> Capture {
    let _lock = lock(root);
    let _ = std::fs::remove_dir_all(root);
    let work = root.join("work");
    let fake = root.join("herdr");
    for d in ["work", "state", "config", "home", "herdr"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let fake_bin = repo().join("tools/fake-herdr/herdr");
    let text = std::fs::read_to_string(scenario).unwrap_or_else(|e| panic!("{}: {e}", scenario.display()));
    let rootstr = root.to_str().unwrap();
    let (mut cols, mut rows) = (80u16, 24u16);
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), format!("{}:/usr/local/bin:/usr/bin:/bin", fake_bin.parent().unwrap().display())),
        ("HOME".into(), root.join("home").display().to_string()),
        ("TERM".into(), "xterm-256color".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("TZ".into(), "UTC".into()),
        ("HERDR_BIN".into(), fake_bin.display().to_string()),
        ("HERDR_BIN_PATH".into(), fake_bin.display().to_string()),
        ("HERDR_PANE_ID".into(), "w1:p2".into()),
        ("HERDR_PLUGIN_ID".into(), "gcaudan.explorer".into()),
        ("HERDR_PLUGIN_ROOT".into(), repo().display().to_string()),
        ("HERDR_PLUGIN_STATE_DIR".into(), root.join("state").display().to_string()),
        ("HERDR_PLUGIN_CONFIG_DIR".into(), root.join("config").display().to_string()),
        ("FAKE_HERDR_DIR".into(), fake.display().to_string()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
    ];
    let mut args: Vec<String> = Vec::new();
    let mut running: Option<Running> = None;
    let mut screen: Option<Screen> = None;
    let write = |dir: &Path, rest: &str| {
        let (path, content) = rest.split_once(' ').unwrap_or((rest, ""));
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, unescape(content)).unwrap();
    };
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let need = |r: &Option<Running>| assert!(r.is_some(), "{}: {word} before start", scenario.display());
        match word {
            "size" => {
                let mut it = rest.split(' ').map(|n| n.parse::<u16>().unwrap());
                (cols, rows) = (it.next().unwrap(), it.next().unwrap());
            }
            "file" => write(&work, rest),
            "dir" => std::fs::create_dir_all(work.join(rest)).unwrap(),
            "state" => write(&root.join("state"), &rest.replace("$ROOT", rootstr)),
            "config" => write(&root.join("config"), rest),
            "home" => write(&root.join("home"), rest),
            "herdr" => {
                let mut it = rest.splitn(3, ' ');
                let (a, b, answer) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap_or(""));
                std::fs::write(fake.join(format!("{a}-{b}")), unescape(answer).replace("$ROOT", rootstr)).unwrap();
            }
            "env" => {
                let (k, v) = rest.split_once('=').unwrap();
                env.push((k.into(), v.replace("$ROOT", rootstr)));
            }
            "git" => {
                let ok = Command::new("git")
                    .args(rest.split(' '))
                    .current_dir(&work)
                    .env_clear()
                    .envs(env.iter().cloned())
                    .envs([
                        ("GIT_AUTHOR_NAME", "Ada"),
                        ("GIT_AUTHOR_EMAIL", "ada@example.com"),
                        ("GIT_COMMITTER_NAME", "Ada"),
                        ("GIT_COMMITTER_EMAIL", "ada@example.com"),
                        ("GIT_AUTHOR_DATE", "2026-01-02T03:04:05Z"),
                        ("GIT_COMMITTER_DATE", "2026-01-02T03:04:05Z"),
                    ])
                    .stdout(Stdio::null())
                    .status()
                    .unwrap()
                    .success();
                assert!(ok, "{}: git {rest}", scenario.display());
            }
            "args" => args = rest.split(' ').map(|a| a.replace("$ROOT", rootstr)).collect(),
            "start" => {
                running = Some(start(command, &args, &env, &work, cols, rows));
                screen = Some(Screen::new(cols as usize, rows as usize));
                running.as_ref().unwrap().settle();
            }
            "keys" => {
                need(&running);
                let r = running.as_mut().unwrap();
                for k in rest.split(' ') {
                    r.send(&key_bytes(k));
                    std::thread::sleep(Duration::from_millis(30));
                }
                r.settle();
            }
            "text" => {
                need(&running);
                let r = running.as_mut().unwrap();
                r.send(unescape(rest).as_bytes());
                r.settle();
            }
            "wait" => std::thread::sleep(Duration::from_millis(rest.parse().unwrap())),
            "until" => {
                need(&running);
                let r = running.as_ref().unwrap();
                let at = r.started + Duration::from_millis(rest.parse().unwrap());
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                r.settle();
            }
            "resize" => {
                need(&running);
                let r = running.as_ref().unwrap();
                let mut it = rest.split(' ').map(|n| n.parse::<u16>().unwrap());
                let (c, w) = (it.next().unwrap(), it.next().unwrap());
                // The output so far is played at the old size, the rest at the new one.
                let s = screen.as_mut().unwrap();
                let mut out = r.output.lock().unwrap();
                s.feed(&std::mem::take(&mut out.0));
                s.resize(c as usize, w as usize);
                drop(out);
                pty::set_size(r.master.as_raw_fd(), c, w).unwrap();
                running.as_mut().unwrap().sent = Instant::now();
                running.as_ref().unwrap().settle();
            }
            other => panic!("{}: unknown directive {other}", scenario.display()),
        }
    }
    let r = running.unwrap_or_else(|| panic!("{}: no start", scenario.display()));
    let mut s = screen.unwrap();
    s.feed(&std::mem::take(&mut r.output.lock().unwrap().0));
    drop(r);
    let calls = std::fs::read_to_string(fake.join("calls.log")).unwrap_or_default();
    Capture { screen: s.text().replace(rootstr, "$ROOT"), calls: calls.replace(rootstr, "$ROOT") }
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

fn start(command: &[OsString], args: &[String], env: &[(String, String)], cwd: &Path, cols: u16, rows: u16) -> Running {
    let (master, slave) = pty::open_pair().unwrap();
    pty::set_size(master.as_raw_fd(), cols, rows).unwrap();
    let io = |fd: &OwnedFd| Stdio::from(fd.try_clone().unwrap());
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..])
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env.iter().cloned())
        .stdin(io(&slave))
        .stdout(io(&slave))
        .stderr(io(&slave));
    // SAFETY: only async-signal-safe calls between fork and exec: a new session whose controlling terminal is the pty.
    unsafe {
        cmd.pre_exec(|| {
            if pty::setsid() < 0 || pty::ioctl(0, pty::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn().unwrap_or_else(|e| panic!("{command:?}: {e}"));
    drop(slave);
    let output = Arc::new(Mutex::new((Vec::new(), None)));
    let sink = output.clone();
    let mut reader = std::fs::File::from(master.try_clone().unwrap());
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut o = sink.lock().unwrap();
            o.0.extend_from_slice(&buf[..n]);
            o.1 = Some(Instant::now());
        }
    });
    Running { master, child, output, sent: Instant::now(), started: Instant::now() }
}
