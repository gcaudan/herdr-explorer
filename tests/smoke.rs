//! The manifest's commands as herdr runs them (Linux): `bin/herdr-explorer <args>` in a plugin folder of its own,
//! the binary cargo built behind a script that logs each start, marked as a development one (bin/.dev) so that nothing
//! is fetched. Each pane starts in a pty, shows its first screen and quits on its key with code 0; the hook and peek
//! answer and call herdr; the preview, its own launcher, starts again on new code, and the explorer's launcher takes
//! the folder its child announces (EXPLORER_IPC_FILE). The environment is cleaned as for every test that runs a pane:
//! the fake herdr first on PATH, dummy values.
#![cfg(target_os = "linux")]

mod support;

use herdr_explorer::sys::pty;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::vt::Screen;

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A test's folders: the plugin (manifest, bin/), the work folder, herdr's fake, state and config.
struct Setup {
    _tmp: support::TempDir,
    root: PathBuf,
    plugin: PathBuf,
    work: PathBuf,
}

impl Setup {
    fn new(name: &str) -> Setup {
        let tmp = support::TempDir::new(&format!("smoke-{name}"));
        let root = tmp.path.clone();
        let plugin = root.join("plugin");
        for d in ["plugin/bin", "work", "state", "config", "home", "herdr"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::copy(repo().join("herdr-plugin.toml"), plugin.join("herdr-plugin.toml")).unwrap();
        // A copy, not a link: the binary finds its plugin folder (the manifest it watches) above its own bin/.
        let real = plugin.join("bin/herdr-explorer.real");
        std::fs::copy(env!("CARGO_BIN_EXE_herdr-explorer"), &real).unwrap();
        let log = root.join("starts.log");
        let wrapper = plugin.join("bin/herdr-explorer");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] || echo \"$* file=$EXPLORER_FILE view=$EXPLORER_VIEW ipc=${{EXPLORER_IPC_FILE:+set}}\" >>'{}'\nexec '{}' \"$@\"\n",
                log.display(),
                real.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(plugin.join("bin/.dev"), "").unwrap();
        // Setup offered already, but in its own test.
        std::fs::write(root.join("state/setup-offered"), "").unwrap();
        std::fs::write(root.join("state/settings.json"), r#"{"glyphs":"unicode","theme":"dark","updateCheck":false}"#)
            .unwrap();
        std::fs::write(root.join("work/a.txt"), "hello smoke\n").unwrap();
        Setup { _tmp: tmp, plugin, work: root.join("work"), root }
    }

    fn env(&self) -> Vec<(String, String)> {
        let fake = repo().join("tools/fake-herdr/herdr");
        let p = |p: &Path| p.display().to_string();
        vec![
            ("PATH".into(), format!("{}:/usr/local/bin:/usr/bin:/bin", p(fake.parent().unwrap()))),
            ("HOME".into(), p(&self.root.join("home"))),
            ("TERM".into(), "xterm-256color".into()),
            ("LANG".into(), "C.UTF-8".into()),
            ("HERDR_BIN".into(), p(&fake)),
            ("HERDR_BIN_PATH".into(), p(&fake)),
            ("HERDR_PANE_ID".into(), "w1:p2".into()),
            ("HERDR_PLUGIN_ID".into(), "gcaudan.explorer".into()),
            ("HERDR_PLUGIN_ROOT".into(), p(&self.plugin)),
            ("HERDR_PLUGIN_STATE_DIR".into(), p(&self.root.join("state"))),
            ("HERDR_PLUGIN_CONFIG_DIR".into(), p(&self.root.join("config"))),
            ("FAKE_HERDR_DIR".into(), p(&self.root.join("herdr"))),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ]
    }

    /// A command of the manifest: the plugin's binary with these arguments.
    fn cmd(&self, args: &[&str]) -> Vec<OsString> {
        let mut cmd = vec![self.plugin.join("bin/herdr-explorer").into()];
        cmd.extend(args.iter().map(OsString::from));
        cmd
    }

    fn starts(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.root.join("starts.log")).unwrap_or_default();
        text.lines().map(String::from).collect()
    }

    /// A command run without a terminal, as herdr runs hooks and actions: code, stdout, stderr.
    fn run(&self, cmd: &[OsString], extra: &[(&str, &str)]) -> (Option<i32>, String, String) {
        let out = Command::new(&cmd[0])
            .args(&cmd[1..])
            .current_dir(&self.work)
            .env_clear()
            .envs(self.env())
            .envs(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let s = |b: &[u8]| String::from_utf8_lossy(b).replace(&self.root.display().to_string(), "$ROOT");
        (out.status.code(), s(&out.stdout), s(&out.stderr))
    }

    fn start(&self, cmd: &[OsString], extra: &[(&str, &str)]) -> Pane {
        let (master, slave) = pty::open_pair().unwrap();
        pty::set_size(master.as_raw_fd(), 80, 24).unwrap();
        let io = |fd: &OwnedFd| Stdio::from(fd.try_clone().unwrap());
        let mut c = Command::new(&cmd[0]);
        c.args(&cmd[1..])
            .current_dir(&self.work)
            .env_clear()
            .envs(self.env())
            .envs(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())))
            .stdin(io(&slave))
            .stdout(io(&slave))
            .stderr(io(&slave));
        // SAFETY: only async-signal-safe calls between fork and exec: a new session whose terminal is the pty.
        unsafe {
            c.pre_exec(|| {
                if pty::setsid() < 0 || pty::ioctl(0, pty::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = c.spawn().unwrap();
        drop(slave);
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = output.clone();
        let mut reader = std::fs::File::from(master.try_clone().unwrap());
        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Pane { master, child, output, reaped: false }
    }
}

struct Pane {
    master: OwnedFd,
    child: Child,
    output: Arc<Mutex<Vec<u8>>>,
    /// The pane's first process reaped: its id, and its group's, may be another process's by now.
    reaped: bool,
}

impl Drop for Pane {
    /// The pane's first process and what it started, as one process group (setsid): never left running, even after a panic.
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        // SAFETY: a signal to the group the child leads, not reaped yet: its id is still its own.
        unsafe { kill(-(self.child.id() as i32), 9) };
        let _ = self.child.wait();
    }
}

impl Pane {
    fn screen(&self) -> String {
        self.screen_after(0)
    }

    /// The screen drawn by the output from `mark` on (a process that draws it whole, such as one restarted).
    fn screen_after(&self, mark: usize) -> String {
        let mut s = Screen::new(80, 24);
        s.feed(&self.output.lock().unwrap()[mark..]);
        s.text()
    }

    /// Waits until the screen holds `text` (10 s at most).
    fn until(&self, text: &str) -> String {
        self.until_after(0, text)
    }

    fn until_after(&self, mark: usize, text: &str) -> String {
        let start = Instant::now();
        loop {
            let screen = self.screen_after(mark);
            if screen.contains(text) {
                return screen;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "{text:?} never shown:\n{screen}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the pane wrote nothing for 300 ms, and gives the length of its output then.
    fn quiet(&self) -> usize {
        let mut len = self.output.lock().unwrap().len();
        loop {
            std::thread::sleep(Duration::from_millis(300));
            let now = self.output.lock().unwrap().len();
            if now == len {
                return len;
            }
            len = now;
        }
    }

    fn send(&self, bytes: &[u8]) {
        std::fs::File::from(self.master.try_clone().unwrap()).write_all(bytes).unwrap();
    }

    /// The pane's exit code, once its first process ended (10 s at most).
    fn code(&mut self) -> Option<i32> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.reaped = true;
                return status.code();
            }
            assert!(start.elapsed() < Duration::from_secs(10), "still running:\n{}", self.screen());
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The pane's close asked of herdr, then its terminal hung up as herdr does: the launcher and its child end.
    fn closed_by_herdr(&mut self, s: &Setup) {
        wait_for("the pane's close never asked of herdr", || {
            std::fs::read_to_string(s.root.join("herdr/calls.log")).unwrap_or_default().contains("pane\tclose\tw1:p2\n")
        });
        // SAFETY: SIGHUP (1) to the group the pane's first process leads, as the kernel sends it when the terminal goes.
        unsafe { kill(-(self.child.id() as i32), 1) };
        self.code();
    }

    /// No process left in the pane's session (setsid: its first process's id): the child ended with its launcher.
    fn none_left(&self) {
        let sid = self.child.id().to_string();
        wait_for("a process of the pane is left", || {
            std::fs::read_dir("/proc").unwrap().filter_map(Result::ok).all(|e| {
                let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
                // pid (comm) state ppid pgrp session …: the fields after the command's closing parenthesis.
                let fields: Vec<&str> = stat.rsplit_once(')').map_or("", |(_, r)| r).split_whitespace().collect();
                fields.get(3) != Some(&sid.as_str()) || fields.first() == Some(&"Z")
            })
        });
    }

    fn cwd(&self) -> PathBuf {
        std::fs::read_link(format!("/proc/{}/cwd", self.child.id())).unwrap()
    }
}

/// A pane's variables, added to the cleaned environment.
type Vars<'a> = &'a [(&'a str, &'a str)];

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < Duration::from_secs(20), "{what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn each_pane_starts_as_the_manifest_runs_it_and_quits_with_code_0() {
    let s = Setup::new("panes");
    let file = s.work.join("a.txt").display().to_string();
    let root = s.work.display().to_string();
    // The pane, what it needs, what its first screen shows, its quit key (the explorer's closes its pane).
    let panes: [(&str, Vars, &str, &[u8]); 5] = [
        ("tree", &[("EXPLORER_ROOT", &root)], "a.txt", b"q"),
        ("preview", &[("EXPLORER_FILE", &file), ("EXPLORER_ROOT", &root)], "hello smoke", b"q"),
        ("search", &[("EXPLORER_ROOT", &root)], "a.txt", b"\x1b"),
        ("diff", &[("EXPLORER_FILE", &file), ("EXPLORER_ROOT", &root)], "a.txt", b"q"),
        ("scm", &[("EXPLORER_ROOT", &root)], "Not a git repository", b"q"),
    ];
    for (id, extra, shown, quit) in panes {
        let mut p = s.start(&s.cmd(&["pane", id]), extra);
        p.until(shown);
        std::thread::sleep(Duration::from_millis(500));
        p.send(quit);
        if id == "tree" {
            // The explorer asks herdr to close its pane, which hangs its terminal up.
            p.closed_by_herdr(&s);
        } else {
            assert_eq!(p.code(), Some(0), "{id}:\n{}", p.screen());
        }
        p.none_left();
    }
    // The editor pane: the editor's code, the editor run in the file's folder (no root), not the pane's.
    std::fs::create_dir_all(s.work.join("sub")).unwrap();
    std::fs::write(s.work.join("sub/b.txt"), "b\n").unwrap();
    let file = s.work.join("sub/b.txt").display().to_string();
    let editor = s.root.join("editor.sh");
    std::fs::write(
        &editor,
        "#!/bin/sh\npwd > \"$(dirname \"$0\")/editor.out\"; echo \"$1\" >> \"$(dirname \"$0\")/editor.out\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut p = s.start(&s.cmd(&["pane", "edit"]), &[("EXPLORER_FILE", &file), ("EDITOR", editor.to_str().unwrap())]);
    assert_eq!(p.code(), Some(0), "edit:\n{}", p.screen());
    let out = std::fs::read_to_string(s.root.join("editor.out")).unwrap();
    assert_eq!(out, format!("{root}/sub\n{file}\n"));
    // Each pane started once by the manifest's command, with no IPC file: the explorer's and the preview's launchers
    // start their child by the binary's own path (bin/herdr-explorer.real here), which the wrapper does not log.
    let starts = s.starts();
    assert_eq!(starts.len(), 6, "{starts:?}");
    assert!(starts.iter().all(|l| l.starts_with("pane ") && l.ends_with("ipc=")), "{starts:?}");
}

#[test]
fn the_hook_and_peek_answer_and_call_herdr() {
    let s = Setup::new("hook");
    // A tab of two panes, p1 a terminal in the work folder and p5 the preview; herdr's answers by their first two
    // words.
    let pane = |id: &str, label: &str| {
        format!(
            r#"{{"pane_id":"{id}","label":{label},"tab_id":"w1:t1","workspace_id":"w1","cwd":"{}"}}"#,
            s.work.display()
        )
    };
    let answers = [
        ("pane-list", format!(r#"{{"panes":[{},{}]}}"#, pane("w1:p1", "null"), pane("w1:p5", r#""Preview""#))),
        ("pane-get", format!(r#"{{"pane":{}}}"#, pane("w1:p1", "null"))),
        ("workspace-list", r#"{"workspaces":[{"workspace_id":"w1","focused":true,"active_tab_id":"w1:t1"}]}"#.into()),
        (
            "pane-layout",
            r#"{"layout":{"area":{"width":200},"panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":200}}]}}"#
                .into(),
        ),
        ("plugin-pane", r#"{"plugin_pane":{"pane":{"pane_id":"w1:p9"}}}"#.into()),
    ];
    for (name, result) in answers {
        std::fs::write(s.root.join("herdr").join(name), format!("{{\"result\":{result}}}\n")).unwrap();
    }
    let ctx = r#"{"workspace_id":"w1","tab_id":"w1:t1","workspace_cwd":"/space"}"#;
    let extra = [("HERDR_PLUGIN_CONTEXT_JSON", ctx), ("HERDR_PANE_ID", "w1:p1")];
    let calls = || {
        let log = s.root.join("herdr/calls.log");
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        text
    };
    // As the manifest runs them: code, output, and the herdr calls made.
    let run = |args: &[&str]| {
        let _ = calls();
        (s.run(&s.cmd(args), &extra), calls())
    };
    let ((code, _, err), log) = run(&["hook", "--toggle"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(log.contains("plugin\tpane\topen\t--plugin\tgcaudan.explorer\t--entrypoint\ttree"), "{log}");
    let ((_, _, _), log) = run(&["hook", "--closed"]);
    assert!(log.contains("workspace\tlist"), "{log}");
    let ((code, out, err), _) = run(&["peek", "a.txt:1"]);
    assert_eq!((code, out.as_str()), (Some(0), "Preview: a.txt:1\n"), "{err}");
    let ((code, _, err), _) = run(&["peek", "--nope"]);
    assert_ne!(code, Some(0), "{err}");
}

#[test]
fn the_binary_started_by_the_manifest_starts_the_preview_again_on_new_code() {
    let s = Setup::new("restart-bin");
    let file = s.work.join("a.txt").display().to_string();
    let root = s.work.display().to_string();
    // As the manifest runs it: the binary is its own launcher (no EXPLORER_IPC_FILE).
    let mut p = s.start(&s.cmd(&["pane", "preview"]), &[("EXPLORER_FILE", &file), ("EXPLORER_ROOT", &root)]);
    p.until("hello smoke");
    assert_eq!(s.starts(), [format!("pane preview file={file} view= ipc=")]);
    let mark = p.quiet();
    let toml = s.plugin.join("herdr-plugin.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, text.replacen("version = \"", "version = \"9.9.9-", 1)).unwrap();
    // The launcher starts its own path again (bin/herdr-explorer.real here), which the wrapper does not log: the new
    // screen, drawn in the same terminal, tells the restart.
    p.until_after(mark, "hello smoke");
    p.send(b"q");
    assert_eq!(p.code(), Some(0), "{}", p.screen());
    p.none_left();
}

#[test]
fn the_binary_started_by_the_manifest_fetches_the_new_binary_before_it_restarts() {
    let s = Setup::new("restart-fetch");
    // No development binary: the launcher makes the binary the manifest's version before the restart, by fetch-bin,
    // here a fake that logs its run.
    std::fs::remove_file(s.plugin.join("bin/.dev")).unwrap();
    std::fs::create_dir_all(s.plugin.join("scripts")).unwrap();
    let log = s.root.join("starts.log");
    std::fs::write(s.plugin.join("scripts/fetch-bin.sh"), format!("echo \"fetch-bin $1\" >>'{}'\n", log.display()))
        .unwrap();
    let file = s.work.join("a.txt").display().to_string();
    let root = s.work.display().to_string();
    let mut p = s.start(&s.cmd(&["pane", "preview"]), &[("EXPLORER_FILE", &file), ("EXPLORER_ROOT", &root)]);
    p.until("hello smoke");
    let mark = p.quiet();
    let toml = s.plugin.join("herdr-plugin.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, text.replacen("version = \"", "version = \"9.9.9-", 1)).unwrap();
    p.until_after(mark, "hello smoke");
    assert_eq!(
        s.starts(),
        [format!("pane preview file={file} view= ipc="), format!("fetch-bin {}", s.plugin.display())]
    );
    p.send(b"q");
    assert_eq!(p.code(), Some(0), "{}", p.screen());
    p.none_left();
}

#[test]
fn the_explorers_launcher_takes_the_folder_its_child_announces() {
    let s = Setup::new("cwd");
    std::fs::create_dir_all(s.work.join("sub/deep")).unwrap();
    let root = s.work.join("sub").display().to_string();
    let mut p = s.start(&s.cmd(&["pane", "tree"]), &[("EXPLORER_ROOT", &root)]);
    p.until("deep");
    assert_eq!(p.cwd(), s.work.join("sub"));
    // backspace: the parent folder as the root, announced to the launcher, the pane's first process.
    std::thread::sleep(Duration::from_millis(500));
    p.send(b"\x7f");
    wait_for("the launcher never moved to the new root", || p.cwd() == s.work);
    p.send(b"q");
    p.closed_by_herdr(&s);
    p.none_left();
}

#[test]
fn the_explorer_outside_a_repository_marks_the_changes_of_those_in_its_folders() {
    let s = Setup::new("repos");
    let git = |dir: &Path, args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .env_clear()
            .envs(s.env())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    // one: a committed file changed since; two: a file never added; plain: no repository.
    for name in ["one", "two", "plain"] {
        std::fs::create_dir_all(s.work.join(name)).unwrap();
        std::fs::write(s.work.join(name).join("f.txt"), "1\n").unwrap();
    }
    git(&s.work.join("one"), &["init", "-q"]);
    git(&s.work.join("one"), &["add", "."]);
    git(&s.work.join("one"), &["commit", "-qm", "f"]);
    std::fs::write(s.work.join("one/f.txt"), "2\n").unwrap();
    git(&s.work.join("two"), &["init", "-q"]);
    // The badge of a folder holding changes, at the end of its row once the colors are left out (a file's icon can be
    // one too).
    let badged = |l: &str| {
        let text: String = l.split('⟪').map(|p| p.split_once('⟫').map_or(p, |(_, t)| t)).collect();
        text.trim_end().ends_with('●')
    };
    let marked = |screen: &str| screen.lines().filter(|l| badged(l)).count();
    let root = s.work.display().to_string();
    let mut p = s.start(&s.cmd(&["pane", "tree"]), &[("EXPLORER_ROOT", &root)]);
    p.until("plain");
    wait_for("the repositories' folders never marked", || marked(&p.screen()) == 2);
    let screen = p.screen();
    assert!(!screen.lines().any(|l| l.contains("plain") && badged(l)), "{screen}");
    // A repository made in a folder afterwards: found, its changes marked.
    std::fs::create_dir_all(s.work.join("three")).unwrap();
    std::fs::write(s.work.join("three/f.txt"), "1\n").unwrap();
    git(&s.work.join("three"), &["init", "-q"]);
    wait_for("the new repository never marked", || marked(&p.screen()) == 3);
    // A folder whose repository is removed: no longer one, its marks gone.
    std::fs::remove_dir_all(s.work.join("one/.git")).unwrap();
    wait_for("the removed repository still marked", || marked(&p.screen()) == 2);
    p.send(b"q");
    p.closed_by_herdr(&s);
    p.none_left();
}

#[test]
fn the_explorer_offers_setup_once_which_adds_the_key_bindings() {
    let s = Setup::new("setup");
    std::fs::remove_file(s.root.join("state/setup-offered")).unwrap();
    let config = s.root.join("home/.config/herdr/config.toml");
    let bound = |n: usize| {
        let text = std::fs::read_to_string(&config).unwrap_or_default();
        text.matches("[[keys.command]]").count() == n && text.contains("command = \"gcaudan.explorer.toggle\"")
    };
    let root = s.work.display().to_string();
    let mut p = s.start(&s.cmd(&["pane", "tree"]), &[("EXPLORER_ROOT", &root)]);
    p.until("Set up herdr-explorer");
    std::thread::sleep(Duration::from_millis(300));
    p.send(b"y");
    wait_for("setup never wrote the key bindings", || bound(3));
    p.send(b"q");
    p.closed_by_herdr(&s);
    p.none_left();
    // Offered once: the next explorer shows its tree only.
    assert!(s.root.join("state/setup-offered").exists());
    // Run again from a terminal: nothing added twice.
    let (code, out, err) = s.run(&s.cmd(&["setup"]), &[]);
    assert_eq!(code, Some(0), "{err}");
    assert!(out.contains("already bound"), "{out}");
    assert!(bound(3));
}
