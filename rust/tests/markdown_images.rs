//! test/markdown-images.test.js: the images of rendered Markdown in pixels: the part of each one that shows for a
//! scroll position (render/image, ported with the Kitty output), and the lines that carry them through lists and
//! quotes.

use herdr_explorer::render::markdown::{self, Line, Options};
use std::rc::Rc;

#[test]
#[ignore = "case 9"]
fn an_image_wholly_in_view_shows_whole() {}

#[test]
#[ignore = "case 9"]
fn an_image_scrolled_partly_above_the_top_shows_its_lower_rows_at_the_top() {}

#[test]
#[ignore = "case 9"]
fn an_image_going_on_below_the_bottom_shows_its_upper_rows() {}

#[test]
#[ignore = "case 9"]
fn an_image_taller_than_the_body_shows_the_rows_in_view() {}

#[test]
#[ignore = "case 9"]
fn an_image_scrolled_out_shows_nothing() {}

#[test]
#[ignore = "case 9"]
fn the_rows_split_pixels_that_are_not_a_multiple_of_the_cell_evenly() {}

#[test]
#[ignore = "case 9"]
fn several_images_in_view_each_once_with_where_it_starts() {}

#[test]
#[ignore = "case 9"]
fn two_copies_of_an_image_are_told_apart_by_their_start() {}

#[test]
fn an_image_line_keeps_its_figure_in_a_list_and_a_quote() {
    // Rows as the preview builds them: text, then each image's rows carrying a figure (here, its row).
    let mut image = |_: &str, _: usize| {
        Some(
            (0..2usize)
                .map(|row| Line {
                    segs: vec![(" ".into(), String::new()), ("    ".into(), String::new())],
                    anchor: None,
                    figure: Some(Rc::new(row)),
                })
                .collect(),
        )
    };
    for text in ["- item ![x](a.png)", "> quote ![x](a.png)"] {
        let lines = markdown::render(text, 40.0, Options { highlight: true, image: Some(&mut image) }).unwrap().lines;
        let figures: Vec<&Line> = lines.iter().filter(|l| l.figure.is_some()).collect();
        let rows: Vec<usize> =
            figures.iter().map(|l| *l.figure.as_ref().unwrap().downcast_ref::<usize>().unwrap()).collect();
        assert_eq!(rows, [0, 1], "{text}");
        // The blank cells of the image end the line, after the prefix.
        assert!(figures.iter().all(|l| l.segs.last().unwrap().0 == "    "), "{text}");
    }
}
