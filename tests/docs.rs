//! test/docs.test.js: the relative links of the Markdown docs, files and #anchors, point at something that exists.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// GitHub's heading anchors: lower case, punctuation other than - and _ dropped, spaces as dashes.
fn slug(heading: &str) -> String {
    let h = heading.trim_start_matches('#');
    let h = h.strip_prefix(' ').unwrap_or(h);
    h.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn is_heading(line: &str) -> bool {
    let rest = line.trim_start_matches('#');
    rest.len() < line.len() && rest.starts_with(' ')
}

fn anchors(file: &Path) -> HashSet<String> {
    std::fs::read_to_string(file).unwrap_or_default().split('\n').filter(|l| is_heading(l)).map(slug).collect()
}

/// The targets of `](…)` links, as the JS's /\]\(([^)\s]+)\)/g.
fn links(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        let end = after.find(|c: char| c == ')' || c.is_whitespace()).unwrap_or(after.len());
        if end > 0 && after[end..].starts_with(')') {
            out.push(&after[..end]);
        }
        rest = &rest[i + 2..];
    }
    out
}

fn scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(s, _)| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase()))
}

#[test]
fn the_links_of_the_docs_point_at_files_and_anchors_that_exist() {
    let root = root();
    let mut docs: Vec<PathBuf> = ["README.md", "SECURITY.md", "CONTRIBUTING.md"].iter().map(|d| root.join(d)).collect();
    let mut more: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    more.sort();
    docs.extend(more);
    let mut broken = Vec::new();
    for doc in &docs {
        let text = std::fs::read_to_string(doc).unwrap();
        for url in links(&text) {
            if scheme(url) {
                continue;
            }
            let mut parts = url.split('#');
            let (p, anchor) = (parts.next().unwrap_or(""), parts.next());
            let target = if p.is_empty() { doc.clone() } else { doc.parent().unwrap().join(p) };
            let name = doc.strip_prefix(&root).unwrap().display();
            if !target.exists() {
                broken.push(format!("{name}: {url} (missing file)"));
            } else if let Some(a) = anchor.filter(|a| !a.is_empty())
                && !anchors(&target).contains(a)
            {
                broken.push(format!("{name}: {url} (missing anchor)"));
            }
        }
    }
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

#[test]
fn anchors_follow_github() {
    assert_eq!(slug("## Pane restart"), "pane-restart");
    assert_eq!(slug("### Network (fetch, pull, push)"), "network-fetch-pull-push");
    assert_eq!(slug("# `bin/.dev` and dev_build"), "bindev-and-dev_build");
    assert!(!is_heading("#!/bin/sh") && is_heading("## A"));
    assert_eq!(links("[a](b.md#c) [x](http://y) [z](d e)"), ["b.md#c", "http://y"]);
}
