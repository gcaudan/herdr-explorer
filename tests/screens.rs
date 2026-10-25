//! Screen captures of the panes (Linux): each scenario of rust/tests/fixtures/screens/<pane>/ runs the pane in a
//! pty (support/pty.rs), started directly and through its shim (`node src/<entry>.js`), and compares its screen and
//! herdr calls with the capture recorded from the JS of 0.8.0 before the switch to Rust. `HX_ONLY=<prefix>` keeps the
//! scenarios whose name starts with it.
#![cfg(target_os = "linux")]

mod support;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use support::pty;

/// The panes, by id, and their shims.
const PANES: [(&str, &str); 6] = [
    ("tree", "explorer.js"),
    ("preview", "preview.js"),
    ("search", "search.js"),
    ("scm", "scm.js"),
    ("diff", "diff.js"),
    ("edit", "edit.js"),
];

fn node() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("node")).find(|p| p.is_file())
}

fn scenarios(pane: &str) -> Vec<PathBuf> {
    let mut list: Vec<_> = std::fs::read_dir(pty::screens_dir().join(pane))
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "keys")).collect()
        })
        .unwrap_or_default();
    // HX_ONLY=<prefix>: the scenarios whose name starts with it, while working on them.
    if let Some(only) = std::env::var_os("HX_ONLY") {
        let only = only.to_string_lossy().into_owned();
        list.retain(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().starts_with(&only)));
    }
    list.sort();
    list
}

/// The scenario's folder: the same for both sides and every run, as a path shown on screen may be cut to the pane's
/// width (a cut path would keep what differs); runs take turns on it (pty::lock).
fn root(pane: &str, scenario: &Path) -> PathBuf {
    let name = scenario.file_stem().unwrap().to_str().unwrap();
    PathBuf::from(format!("/tmp/hxs/{pane}-{name}"))
}

fn compare(scenario: &Path, got: &pty::Capture) -> Option<String> {
    let want = |ext| std::fs::read_to_string(scenario.with_extension(ext)).unwrap_or_default();
    let (screen, calls) = (want("screen"), want("calls"));
    let mut diff = String::new();
    if got.screen != screen {
        diff += &format!("--- want screen\n{screen}--- got screen\n{}", got.screen);
    }
    if got.calls != calls {
        diff += &format!("--- want calls\n{calls}--- got calls\n{}", got.calls);
    }
    (!diff.is_empty()).then(|| format!("{}\n{diff}", scenario.display()))
}

/// A plugin folder for the shims, made once: the repository's shims and manifest, and in bin/ a link to the
/// binary cargo built, marked as a development one (bin/.dev), so that no shim fetches anything. The binary follows
/// the link: its plugin folder is HERDR_PLUGIN_ROOT, the repository, as when it runs directly.
fn plugin() -> &'static Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("hxs-plugin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        for e in std::fs::read_dir(pty::repo().join("src")).unwrap().filter_map(Result::ok) {
            if e.path().extension().is_none_or(|x| x != "js") {
                continue;
            }
            std::fs::copy(e.path(), dir.join("src").join(e.file_name())).unwrap();
        }
        std::fs::copy(pty::repo().join("herdr-plugin.toml"), dir.join("herdr-plugin.toml")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_herdr-explorer"), dir.join("bin/herdr-explorer")).unwrap();
        std::fs::write(dir.join("bin/.dev"), "").unwrap();
        dir
    })
}

/// The panes started by their shim give the JS captures: the shim adds nothing to the screen nor to herdr's calls.
fn shim(pane: &str) {
    let Some(node) = node() else {
        assert!(std::env::var_os("CI").is_none(), "node not found: the shims' captures must be checked in CI");
        eprintln!("node not found: the captures of {pane} through its shim are not checked");
        return;
    };
    let script = PANES.iter().find(|(p, _)| *p == pane).unwrap().1;
    let cmd: Vec<OsString> = vec![node.into(), plugin().join("src").join(script).into()];
    let failures: Vec<_> =
        scenarios(pane).iter().filter_map(|s| compare(s, &pty::run(s, &cmd, &root(pane, s)))).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The Rust panes give the JS captures.
fn rust(pane: &str) {
    let cmd: Vec<OsString> = vec![env!("CARGO_BIN_EXE_herdr-explorer").into(), "pane".into(), pane.into()];
    let failures: Vec<_> =
        scenarios(pane).iter().filter_map(|s| compare(s, &pty::run(s, &cmd, &root(pane, s)))).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn emulator_plays_moves_styles_and_wide_characters() {
    let mut s = support::vt::Screen::new(10, 3);
    s.feed("\x1b[?1049h\x1b[2;3H\x1b[1;38;5;12mab\x1b[0m界x\x1b[3;1H\x1b[48;2;1;2;3m \x1b[K".as_bytes());
    assert_eq!(s.text(), "size 10x3\n\n  ⟪fg=12 b⟫ab⟪⟫界x\n⟪bg=#010203⟫          ⟪⟫\n");
    // No wrap (?7l): the last column is written over.
    s.feed(b"\x1b[0m\x1b[?7l\x1b[1;9Habcd");
    assert!(s.text().lines().nth(1).unwrap().ends_with("ad"));
    // A charset designation is never shown; colours in the colon form are one group each.
    let mut s = support::vt::Screen::new(10, 1);
    s.feed(b"\x1b(Bx\x1b[1;38:2::1:2:3;48:5:4mA\x1b[0;38:2:5:6:7mB");
    assert_eq!(s.text(), "size 10x1\nx⟪fg=#010203 bg=4 b⟫A⟪fg=#050607⟫B⟪⟫\n");
}

/// The panes ported (ready), then those still to port, with the case that ports them (pending).
macro_rules! panes {
    (ready { $($r:ident: $rp:literal;)* } pending { $($name:ident: $pane:literal, $case:literal;)* }) => {
        mod shim_captures {
            $(#[test] fn $r() { super::shim($rp) })*
            $(#[test] #[ignore = $case] fn $name() { super::shim($pane) })*
        }
        mod rust_captures {
            $(#[test] fn $r() { super::rust($rp) })*
            $(#[test] #[ignore = $case] fn $name() { super::rust($pane) })*
        }
    };
}

panes! {
    ready {
        tree: "tree";
        preview: "preview";
        search: "search";
        diff: "diff";
        scm: "scm";
        edit: "edit";
    }
    pending {
    }
}
