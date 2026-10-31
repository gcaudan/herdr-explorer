//! Screen captures of the panes (Linux): each scenario of tests/fixtures/screens/<pane>/ runs the pane in a
//! pty (support/pty.rs), started as the manifest runs it (`herdr-explorer pane <id>`), and compares its screen and
//! herdr calls with the capture recorded next to it, recorded again with `HX_RECORD=1` for a change of behaviour its
//! commit explains. `HX_ONLY=<prefix>` keeps the scenarios whose name
//! starts with it.
#![cfg(target_os = "linux")]

mod support;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use support::pty;

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

/// The scenario's folder: the same for every run, as a path shown on screen may be cut to the pane's
/// width (a cut path would keep what differs); runs take turns on it (pty::lock).
fn root(pane: &str, scenario: &Path) -> PathBuf {
    let name = scenario.file_stem().unwrap().to_str().unwrap();
    PathBuf::from(format!("/tmp/hxs/{pane}-{name}"))
}

/// The screen with the build of the key help (version and commit, new with each) as `<build>`, the spaces before it as
/// one.
fn unbuilt(screen: &str) -> String {
    let build = herdr_explorer::core::update::BUILD;
    let mut out = String::new();
    for l in screen.split_inclusive('\n') {
        let Some(i) = l.find(build) else {
            out += l;
            continue;
        };
        let (head, tail) = (&l[..i], &l[i + build.len()..]);
        // Its style, then the spaces of the segment before.
        let at = head.rfind('⟪').filter(|&s| head[s..].ends_with('⟫')).unwrap_or(head.len());
        out += &format!("{} {}<build>{tail}", head[..at].trim_end_matches(' '), &head[at..]);
    }
    out
}

fn compare(scenario: &Path, got: &pty::Capture) -> Option<String> {
    let got = &pty::Capture { screen: unbuilt(&got.screen), calls: got.calls.clone() };
    if std::env::var_os("HX_RECORD").is_some() {
        std::fs::write(scenario.with_extension("screen"), &got.screen).unwrap();
        std::fs::write(scenario.with_extension("calls"), &got.calls).unwrap();
        return None;
    }
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

/// The panes give their captures.
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

/// The panes checked (ready), then those ignored, with the reason (pending).
macro_rules! panes {
    (ready { $($r:ident: $rp:literal;)* } pending { $($name:ident: $pane:literal, $case:literal;)* }) => {
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
