//! render::image: what the fixtures of tests/modules.rs cannot hold: where the Rust parts from the JS on purpose,
//! and the theme's background.

// A little-endian TIFF of one grey row, 2 pixels wide unless width (a rational) says otherwise, rows per strip given.
fn grey_tiff(width: Option<(u32, u32)>, rows_per_strip: Option<u32>) -> Vec<u8> {
    let mut entries: Vec<(u16, u16, u32, u32)> =
        vec![(257, 3, 1, 1), (258, 3, 1, 8), (262, 3, 1, 1), (273, 4, 1, 0), (279, 4, 1, 2)];
    entries.insert(0, if width.is_some() { (256, 5, 1, 0) } else { (256, 3, 1, 2) });
    if let Some(r) = rows_per_strip {
        entries.push((278, 4, 1, r));
    }
    let dir_end = 8 + 2 + entries.len() * 12 + 4;
    let (rational, pixels) = (dir_end, dir_end + 8);
    let mut b = b"II*\0\x08\0\0\0".to_vec();
    b.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, n, value) in entries {
        let value = match tag {
            256 if kind == 5 => rational as u32,
            273 => pixels as u32,
            _ => value,
        };
        b.extend(tag.to_le_bytes());
        b.extend(kind.to_le_bytes());
        b.extend(n.to_le_bytes());
        b.extend(value.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    let (num, den) = width.unwrap_or((0, 0));
    b.extend(num.to_le_bytes());
    b.extend(den.to_le_bytes());
    b.extend([0x40, 0xc0]);
    b
}

#[test]
fn a_tiff_with_sizes_that_are_not_whole_shows_nothing() {
    use herdr_explorer::render::image::decode;
    let img = decode(&grey_tiff(None, None)).unwrap();
    assert_eq!((img.width, img.height, img.data), (2, 1, vec![0x40, 0x40, 0x40, 255, 0xc0, 0xc0, 0xc0, 255]));
    assert_eq!(decode(&grey_tiff(Some((4, 2)), None)).map(|i| i.width), Some(2));
    // The JS makes an image of these anyway, with NaN or fractional lengths and indices.
    assert!(decode(&grey_tiff(Some((5, 2)), None)).is_none());
    assert!(decode(&grey_tiff(None, Some(0))).is_some());
}

#[test]
fn half_blocks_go_over_the_theme_background_by_default() {
    use herdr_explorer::core::ui;
    use herdr_explorer::render::image::render;
    let tiff = grey_tiff(None, None);
    let canvas = ui::c().canvas;
    assert_eq!(render(&tiff, 10.0, 4.0, None), render(&tiff, 10.0, 4.0, Some(canvas)));
    let lines = render(&tiff, 2.0, 1.0, Some([1, 2, 3])).unwrap().lines;
    // Two cells wide, one row of pixels: the lower half of each cell is the background.
    assert!(lines[0].iter().all(|(_, style)| style.ends_with("\x1b[48;2;1;2;3m")));
}
