//! test/syntax.test.js: every script of src/ (the shims herdr runs) passes `node --check`: a shim that fails to parse
//! opens no pane. CI runs it on Node 20 and 24 as well (.github/workflows/ci.yml).

use std::path::{Path, PathBuf};
use std::process::Command;

fn node() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(exe)).find(|p| p.is_file())
}

#[test]
fn every_shim_parses() {
    let Some(node) = node() else {
        assert!(std::env::var_os("CI").is_none(), "node not found: the shims must be checked in CI");
        eprintln!("node not found: the shims are not checked");
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut scripts = Vec::new();
    for dir in [root.join("src")] {
        for e in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
            if e.path().extension().is_some_and(|x| x == "js") {
                scripts.push(e.path());
            }
        }
    }
    assert!(scripts.iter().any(|s| s.ends_with("src/shim.js")), "{scripts:?}");
    for s in scripts {
        let out = Command::new(&node).arg("--check").arg(&s).output().unwrap();
        assert!(out.status.success(), "{}: {}", s.display(), String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn every_script_the_manifest_runs_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("herdr-plugin.toml")).unwrap();
    let scripts: Vec<&str> =
        manifest.split("[\"node\", \"").skip(1).map(|rest| &rest[..rest.find('"').unwrap()]).collect();
    assert!(scripts.len() > 10, "{scripts:?}");
    for s in scripts {
        assert!(root.join(s).is_file(), "herdr-plugin.toml runs {s}, which does not exist");
    }
}
