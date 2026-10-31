//! The images of rendered Markdown in pixels: the part of each one that shows for a
//! scroll position, and the lines that carry them through lists and quotes.

use herdr_explorer::render::image::{Crop, Pic, crop, shown};
use herdr_explorer::render::markdown::{self, Line, Options};
use std::rc::Rc;

fn part(at: i64, from: i64, count: i64, y: i64, h: i64) -> Option<Crop> {
    Some(Crop { at, from, count, y, h })
}

// An image 4 rows high, 64 pixels (16 a row), its first row at row 10 of the text, under a body 5 rows high.
#[test]
fn an_image_wholly_in_view_shows_whole() {
    assert_eq!(crop(10, 4, 64, 8, 10), part(2, 0, 4, 0, 64));
}

#[test]
fn an_image_scrolled_partly_above_the_top_shows_its_lower_rows_at_the_top() {
    assert_eq!(crop(10, 4, 64, 11, 5), part(0, 1, 3, 16, 48));
    assert_eq!(crop(10, 4, 64, 13, 5), part(0, 3, 1, 48, 16));
}

#[test]
fn an_image_going_on_below_the_bottom_shows_its_upper_rows() {
    assert_eq!(crop(10, 4, 64, 7, 5), part(3, 0, 2, 0, 32));
}

#[test]
fn an_image_taller_than_the_body_shows_the_rows_in_view() {
    assert_eq!(crop(10, 4, 64, 11, 2), part(0, 1, 2, 16, 32));
}

#[test]
fn an_image_scrolled_out_shows_nothing() {
    assert_eq!(crop(10, 4, 64, 14, 5), None);
    assert_eq!(crop(10, 4, 64, 5, 5), None);
    assert_eq!(crop(10, 0, 64, 10, 5), None);
}

#[test]
fn the_rows_split_pixels_that_are_not_a_multiple_of_the_cell_evenly() {
    // 3 rows over 40 pixels: 13, 14 and 13 pixels.
    let parts: Vec<(i64, i64)> = (0..3).map(|k| crop(0, 3, 40, k, 1).map(|p| (p.y, p.h)).unwrap()).collect();
    assert_eq!(parts, [(0, 13), (13, 14), (27, 13)]);
}

struct Image {
    id: u32,
    rows: i64,
    pixels: i64,
}

impl Pic for Image {
    fn rows(&self) -> i64 {
        self.rows
    }
    fn pixels(&self) -> i64 {
        self.pixels
    }
}

// Rows as the preview builds them: text (a number of rows), then each image's rows carrying the image and the row.
enum Part<'a> {
    Text(usize),
    Pic(&'a Image),
}

fn rows_of<'a>(parts: &[Part<'a>]) -> Vec<Option<(&'a Image, i64)>> {
    parts
        .iter()
        .flat_map(|p| match p {
            Part::Text(n) => vec![None; *n],
            Part::Pic(pic) => (0..pic.rows).map(|row| Some((*pic, row))).collect(),
        })
        .collect()
}

#[test]
fn several_images_in_view_each_once_with_where_it_starts() {
    let (a, b) = (Image { id: 1, rows: 3, pixels: 48 }, Image { id: 2, rows: 2, pixels: 32 });
    let list = rows_of(&[Part::Text(2), Part::Pic(&a), Part::Text(1), Part::Pic(&b), Part::Text(4)]);
    let seen: Vec<_> =
        shown(&list, 3, 5).iter().map(|f| (f.pic.id, f.start, f.part.at, f.part.from, f.part.count)).collect();
    assert_eq!(seen, [(1, 2, 0, 1, 2), (2, 6, 3, 0, 2)]);
    assert!(shown(&list, 9, 5).is_empty());
}

#[test]
fn two_copies_of_an_image_are_told_apart_by_their_start() {
    let a = Image { id: 1, rows: 2, pixels: 32 };
    let list = rows_of(&[Part::Pic(&a), Part::Text(1), Part::Pic(&a)]);
    assert_eq!(shown(&list, 0, 5).iter().map(|f| f.start).collect::<Vec<_>>(), [0, 3]);
}

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
