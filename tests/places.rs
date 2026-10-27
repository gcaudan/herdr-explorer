//! The places of peek's argument, and the links clicked in a terminal.

mod support;

use herdr_explorer::core::paths;
use herdr_explorer::core::places::{Mark, clicked, target};

fn mark(from: u64, to: u64, a: u64, b: u64) -> Mark {
    Mark { from, to, a, b }
}

fn cwd(dir: &support::TempDir) -> String {
    dir.path.to_string_lossy().into_owned()
}

#[test]
fn lines_ranges_and_columns_separated_by_commas() {
    let dir = support::TempDir::new("places");
    let cwd = cwd(&dir);
    assert_eq!(
        target("app.js:12-14,30,42:5-18", &cwd),
        Ok((paths::join(&[&cwd, "app.js"]), vec![mark(12, 14, 0, 0), mark(30, 30, 0, 0), mark(42, 42, 5, 18)]))
    );
}

#[test]
fn a_single_column_is_a_position_the_whole_line() {
    let dir = support::TempDir::new("places");
    assert_eq!(target("app.js:12:5", &cwd(&dir)).unwrap().1, vec![mark(12, 12, 0, 0)]);
}

#[test]
fn no_places_or_a_tail_that_does_not_read_as_places_the_name_whole() {
    let dir = support::TempDir::new("places");
    let cwd = cwd(&dir);
    assert_eq!(target("app.js", &cwd), Ok((paths::join(&[&cwd, "app.js"]), vec![])));
    assert_eq!(target("notes:draft", &cwd), Ok((paths::join(&[&cwd, "notes:draft"]), vec![])));
}

#[cfg(not(windows))]
#[test]
fn a_name_with_a_colon_that_exists_is_taken_whole() {
    let dir = support::TempDir::new("places");
    let cwd = cwd(&dir);
    std::fs::write(dir.path.join("a:12"), "").unwrap();
    assert_eq!(target("a:12", &cwd), Ok((paths::join(&[&cwd, "a:12"]), vec![])));
}

#[test]
fn places_that_cannot_be_are_refused() {
    let dir = support::TempDir::new("places");
    for bad in ["app.js:0", "app.js:14-12", "app.js:12:9-5", "app.js:12:0-5"] {
        let err = target(bad, &cwd(&dir)).expect_err(bad);
        assert!(err.contains("wrong place"), "{bad}: {err}");
    }
}

/// url.pathToFileURL(file).href, for the paths of these tests (a space the only character to escape).
fn file_url(file: &str) -> String {
    if cfg!(windows) { format!("file:///{}", file.replace('\\', "/")) } else { format!("file://{file}") }
        .replace(' ', "%20")
}

#[test]
fn a_clicked_link_a_local_file_url_with_its_line_or_the_text_as_it_is() {
    let dir = support::TempDir::new("places");
    let file = paths::join(&[&cwd(&dir), "a b.js"]);
    let url = file_url(&file);
    assert_eq!(clicked(&url, "me"), Some(file.clone()));
    assert_eq!(clicked(&format!("{url}#L12"), "me"), Some(format!("{file}:12")));
    assert_eq!(clicked(&format!("{url}#L12-L14"), "me"), Some(format!("{file}:12-14")));
    assert_eq!(clicked(&url.replacen("file://", "file://localhost", 1), "me"), Some(file.clone()));
    assert_eq!(clicked("src/app.js:12:5", "me"), Some("src/app.js:12:5".to_string()));
    // Another host (a network share on Windows), another scheme, nothing: refused.
    assert_eq!(clicked("file://server/share/x.js", "me"), None);
    assert_eq!(clicked("https://example.com/x.js", "me"), None);
    assert_eq!(clicked("", "me"), None);
}
