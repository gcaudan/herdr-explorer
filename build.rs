// Embeds the plugin's version, read from herdr-plugin.toml, as HERDR_EXPLORER_VERSION: the manifest is the only place
// the version is written (release-please bumps it there).
use std::{env, fs, path::Path};

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
}
