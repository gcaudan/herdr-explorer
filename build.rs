// Embeds the plugin's version, read from herdr-plugin.toml, as HERDR_EXPLORER_VERSION: the manifest is the only place
// the version is written (release-please bumps it there).
use std::{env, fs, path::Path, process::Command};

fn main() {
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("herdr-plugin.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(&manifest).expect("herdr-plugin.toml is readable");
    // The top-level key, before the first table.
    let version = text
        .lines()
        .take_while(|l| !l.trim_start().starts_with('['))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "version")
        .and_then(|(_, v)| v.split('"').nth(1))
        .expect("herdr-plugin.toml has a version")
        .to_string();
    let digits = version.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    assert!(digits && version.split('.').count() == 3, "herdr-plugin.toml: version {version:?} is not X.Y.Z");
    println!("cargo:rustc-env=HERDR_EXPLORER_VERSION={version}");
    // The commit built, shown in the help with the version (a development build has the release's version): none
    // outside a git checkout. Built again on each commit or checkout, which writes the reflog of HEAD.
    let dir = manifest.parent().unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git").args(args).current_dir(dir).output().ok().filter(|o| o.status.success())?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    let commit = git(&["rev-parse", "--short=7", "HEAD"]);
    if let Some(log) = git(&["rev-parse", "--path-format=absolute", "--git-path", "logs/HEAD"]) {
        println!("cargo:rerun-if-changed={log}");
    }
    let build = match commit {
        Some(c) => format!("v{version} · {c}"),
        None => format!("v{version}"),
    };
    println!("cargo:rustc-env=HERDR_EXPLORER_BUILD={build}");
}
