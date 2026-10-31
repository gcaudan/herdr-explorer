//! Markdown as the preview renders it: anchors, footnote lines, inline tags, quotes in lists, ordered
//! lists after a paragraph; and Rust's raw strings in the highlighter.

use herdr_explorer::render::highlight::highlight;
use herdr_explorer::render::markdown::{self, Options};

fn text(md: &str) -> Vec<String> {
    let r = markdown::render(md, 80.0, Options::default()).unwrap();
    r.lines.iter().map(|l| l.segs.iter().map(|s| s.0.as_str()).collect::<String>()).collect()
}

#[test]
fn a_heading_anchor_leaves_out_the_url_shown_after_a_link() {
    let r = markdown::render("## Usage with [Docker](https://docker.com)", 80.0, Options::default()).unwrap();
    assert_eq!(r.anchors.first().map(|a| a.0.as_str()), Some("usage-with-docker"));
}

#[test]
fn a_footnote_definition_is_not_a_link_reference() {
    assert_eq!(text("See[^1]\n\n[^1]: Some note"), ["See[^1]", "", "[^1]: Some note"]);
}

#[test]
fn a_tag_closes_at_its_own_name_only() {
    assert_eq!(text("<s>a <sup>2</sup> b</s>"), ["a 2 b"]);
}

#[test]
fn a_quote_in_a_tight_list_keeps_its_paragraphs_apart() {
    let lines = text("- item\n  > p1\n  >\n  > p2");
    let at = |s: &str| lines.iter().position(|l| l.contains(s)).unwrap();
    assert_eq!(at("p2") - at("p1"), 2);
}

#[test]
fn an_ordered_item_from_other_than_1_does_not_break_a_paragraph() {
    assert_eq!(text("The year was\n2024. A good one."), ["The year was 2024. A good one."]);
}

#[test]
fn a_rust_raw_string_ends_at_its_quote_whatever_the_backslash_before() {
    let rows = highlight("let p = r\"C:\\dir\\\";\nlet x = 1;", Some("rust"));
    let string = &rows[0].iter().find(|s| s.0.starts_with("r\"")).unwrap().1;
    assert!(rows[1].iter().all(|s| s.1 != *string), "{:?}", rows[1]);
}
