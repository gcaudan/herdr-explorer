//! herdr-plugin.toml: every command runs the binary or a script of the plugin; the explorer and the
//! preview through the binary's `pane` subcommand, their own launcher.

use std::path::Path;

/// Each command of the manifest, its spaces left out: `["bin/herdr-explorer","hook","--restore"]`.
fn commands(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .filter_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == "command"))
        .map(|(_, v)| v.chars().filter(|c| !c.is_whitespace()).collect())
        .collect()
}

#[test]
fn every_command_of_the_manifest_runs_the_binary_or_a_script() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("herdr-plugin.toml")).unwrap();
    let all = commands(&manifest);
    assert!(all.len() > 15, "{all:?}");
    for c in &all {
        let first = c.trim_start_matches("[\"").split('"').next().unwrap_or("");
        let script = c.split('"').find(|s| s.starts_with("scripts/"));
        match first {
            "bin/herdr-explorer" => {}
            "sh" | "powershell" => assert!(script.is_some_and(|s| root.join(s).is_file()), "{c}"),
            _ => panic!("herdr-plugin.toml runs {c}: neither the binary nor a script of scripts/"),
        }
    }
}

#[test]
fn the_manifest_runs_the_binary_for_the_explorer_and_the_preview() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("herdr-plugin.toml")).unwrap();
    // Each [[panes]] block's id and command, its spaces left out: the command checked in the block of its own pane.
    let panes: Vec<(String, String)> = manifest
        .split("[[")
        .filter(|b| b.starts_with("panes]]"))
        .map(|b| {
            let value = |key: &str| {
                let line = b.lines().find(|l| l.split('=').next().is_some_and(|k| k.trim() == key)).unwrap_or("");
                line.split_once('=').map_or("", |(_, v)| v).chars().filter(|c| !c.is_whitespace()).collect()
            };
            (value("id"), value("command"))
        })
        .collect();
    // Their own launcher: these panes live long.
    for id in ["tree", "preview"] {
        let command = panes.iter().find(|(i, _)| *i == format!("\"{id}\"")).map(|(_, c)| c.as_str());
        let want = format!("[\"bin/herdr-explorer\",\"pane\",\"{id}\"]");
        assert_eq!(command, Some(want.as_str()), "the {id} pane of herdr-plugin.toml: {panes:?}");
    }
}
