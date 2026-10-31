//! The lanes of the history graph: a branch drawn beside its parent's lane, joined back where it
//! started, and the second parent of a merge opening a lane.

use herdr_explorer::render::graph::{Row, layout};

fn commits(list: &[(&'static str, &[&'static str])]) -> Vec<(&'static str, Vec<&'static str>)> {
    list.iter().map(|&(h, ps)| (h, ps.to_vec())).collect()
}

fn draw(list: &[(&'static str, &[&'static str])]) -> Vec<String> {
    layout(&commits(list), None)
        .iter()
        .map(|r: &Row| r.cells.iter().map(|c| c.0).collect::<String>().trim_end().to_string())
        .collect()
}

#[test]
fn a_branch_beside_main_joined_where_it_started() {
    assert_eq!(
        draw(&[("f2", &["f1"]), ("m2", &["m1"]), ("f1", &["m1"]), ("m1", &["m0"]), ("m0", &[])]),
        ["●", "│ ●", "● │", "●─╯", "●"]
    );
}

#[test]
fn a_merge_opens_a_lane_for_its_second_parent() {
    assert_eq!(draw(&[("M", &["a", "b"]), ("b", &["a"]), ("a", &[])]), ["●─╮", "│ ●", "●─╯"]);
}

#[test]
fn a_lane_keeps_its_colour() {
    let rows = layout(&commits(&[("b", &["a"]), ("c", &["a"]), ("a", &[])]), None);
    assert_eq!(rows[0].color, rows[2].color);
    assert_ne!(rows[1].color, rows[0].color);
}

#[test]
fn a_tip_has_no_line_above_the_first_commit_none_below() {
    let rows = layout(&commits(&[("b", &["a"]), ("a", &[])]), None);
    assert_eq!(rows.iter().map(|r| (r.above, r.below)).collect::<Vec<_>>(), [(false, true), (true, false)]);
}

#[test]
fn the_lane_of_a_branch_straight_ahead_of_the_default_branch_changes_colour_where_it_leaves_it() {
    let rows = layout(&commits(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]), Some("b"));
    assert_ne!(rows[0].color, rows[1].color);
    assert_eq!(rows[1].color, rows[2].color);
    // A lane joining there: its colour goes on.
    let fork = layout(&commits(&[("d", &["b"]), ("m", &["b"]), ("b", &["a"]), ("a", &[])]), Some("b"));
    assert_eq!(fork[2].color, fork[1].color);
}

#[test]
fn a_merge_does_not_open_its_second_parent_on_a_lane_ending_there() {
    assert_eq!(
        draw(&[("z", &["c"]), ("x", &["c"]), ("c", &["p1", "p2"]), ("p2", &["p1"]), ("p1", &[])]),
        ["●", "│ ●", "●─╯─╮", "│   ●", "●───╯"]
    );
}
