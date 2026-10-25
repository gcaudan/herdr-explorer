//! The crate's layers: an import only goes to the same layer or a lower one, and a pane never imports another pane.
//! Read from the `crate::…` paths of every file of src/, `use` or not; a module of src/ that no layer names fails too.
//!
//! - entries: the binary's commands, which may import anything;
//! - panes: one folder each, never importing one another;
//! - shared: what the panes use (diff/ is the diff library, the Changes pane is changes/);
//! - base: what only needs the standard library.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

const LAYERS: [(&str, &[&str]); 4] = [
    ("entries", &["cli", "hook", "peek", "launch", "edit", "main", "lib"]),
    ("panes", &["explorer", "preview", "search", "scm", "changes"]),
    ("shared", &["core", "render", "diff"]),
    ("base", &["codec", "unicode", "json", "sys"]),
];

fn layer_of(module: &str) -> Option<usize> {
    LAYERS.iter().position(|(_, mods)| mods.contains(&module))
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The top-level module a file of src/ belongs to: `core` for src/core/ui.rs, `cli` for src/cli.rs.
fn module_of(rel: &Path) -> String {
    let first = rel.components().next().unwrap().as_os_str().to_string_lossy();
    first.strip_suffix(".rs").unwrap_or(&first).to_string()
}

fn ident(s: &str) -> &str {
    &s[..s.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(s.len())]
}

/// The first segments after each `crate::` of a line, with what follows them: `crate::{core::ui, json}` gives core
/// and json. Line comments are left out.
fn targets(line: &str) -> Vec<(String, bool)> {
    let code = line.trim_start();
    if code.starts_with("//") {
        return Vec::new();
    }
    let code = code.find(" // ").map_or(code, |i| &code[..i]);
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(i) = rest.find("crate::") {
        let before = rest[..i].chars().next_back();
        rest = &rest[i + "crate::".len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if let Some(group) = rest.strip_prefix('{') {
            let mut depth = 0;
            let mut start = 0;
            for (j, c) in group.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' if depth == 0 => {
                        out.push((ident(group[start..j].trim()).to_string(), false));
                        break;
                    }
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        out.push((ident(group[start..j].trim()).to_string(), false));
                        start = j + 1;
                    }
                    _ => {}
                }
            }
        } else {
            let name = ident(rest);
            out.push((name.to_string(), rest[name.len()..].starts_with('!')));
        }
    }
    out.retain(|(name, _)| !name.is_empty());
    out
}

#[test]
fn the_parser_reads_paths_groups_and_macros() {
    let names = |line| targets(line).into_iter().map(|(n, m)| format!("{n}{}", if m { "!" } else { "" })).collect();
    let got: Vec<String> = names("use crate::{core::{ui, js}, json::Value};");
    assert_eq!(got, ["core", "json"]);
    let got: Vec<String> = names("    let v = crate::obj! { \"a\" => crate::launch::home(None) };");
    assert_eq!(got, ["obj!", "launch"]);
    let got: Vec<String> = names("    // crate::explorer::tree, in a comment");
    assert!(got.is_empty());
    let got: Vec<String> = names("x(); // crate::explorer::tree");
    assert!(got.is_empty());
    let got: Vec<String> = names("println!(\"{}\", crate::VERSION);");
    assert_eq!(got, ["VERSION"]);
}

#[test]
fn imports_never_go_up_a_layer_nor_across_panes() {
    let root = src();
    let mut all = Vec::new();
    files(&root, &mut all);
    all.sort();
    let text: Vec<(PathBuf, String)> = all
        .iter()
        .map(|p| (p.strip_prefix(&root).unwrap().to_path_buf(), std::fs::read_to_string(p).unwrap()))
        .collect();

    // The exported macros live at the crate's root: `crate::obj!` belongs to the module that defines obj!.
    let mut macros = HashMap::new();
    for (rel, text) in &text {
        let mut exported = false;
        for line in text.lines().map(str::trim) {
            if line == "#[macro_export]" {
                exported = true;
            } else if let Some(name) = line.strip_prefix("macro_rules! ").filter(|_| exported) {
                macros.insert(ident(name).to_string(), module_of(rel));
                exported = false;
            }
        }
    }

    let mut errors = Vec::new();
    for (rel, text) in &text {
        let own = module_of(rel);
        let Some(own_layer) = layer_of(&own) else {
            errors.push(format!("src/{}: module {own} is in no layer of tests/layers.rs", rel.display()));
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for (name, is_macro) in targets(line) {
                let target = match is_macro {
                    true => macros.get(&name).cloned().unwrap_or_else(|| "lib".into()),
                    false if layer_of(&name).is_some() => name.clone(),
                    // An item of lib.rs itself, as crate::VERSION.
                    false => "lib".into(),
                };
                let layer = layer_of(&target).unwrap();
                let across = own_layer == 1 && layer == 1 && target != own;
                if layer < own_layer || across {
                    errors.push(format!(
                        "src/{}:{}: {own} ({}) imports crate::{name}{} ({})",
                        rel.display(),
                        i + 1,
                        LAYERS[own_layer].0,
                        if is_macro { "!" } else { "" },
                        LAYERS[layer].0,
                    ));
                }
            }
        }
    }
    assert!(errors.is_empty(), "imports against the layers:\n{}", errors.join("\n"));
}
