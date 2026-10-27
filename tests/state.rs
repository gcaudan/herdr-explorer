//! The state files that tabs and panes leave behind once closed.

use herdr_explorer::core::exec::stale_state;
use herdr_explorer::core::paths;

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_files_of_closed_tabs_and_panes_go_the_others_stay() {
    let names =
        strings(&["notes-w1_t1.json", "notes-w1_t2.json", "preview-w1_t2.json", "settings.json", "width", "panes"]);
    let panes = strings(&["w1_p1.json", "w1_p7.json"]);
    let mut stale = stale_state("S", &names, &panes, &strings(&["w1:t1"]), &strings(&["w1:p1"]));
    stale.sort();
    assert_eq!(
        stale,
        [
            paths::join(&["S", "notes-w1_t2.json"]),
            paths::join(&["S", "panes", "w1_p7.json"]),
            paths::join(&["S", "preview-w1_t2.json"])
        ]
    );
}

#[test]
fn no_live_tab_every_tab_file_goes_nothing_else() {
    let names = strings(&["notes-a.json", "search.json", "expanded.json"]);
    assert_eq!(stale_state("S", &names, &[], &[], &[]), [paths::join(&["S", "notes-a.json"])]);
}
