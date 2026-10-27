//! Images in the preview: an image file in pixels with the Kitty graphics protocol, else drawn with
//! half blocks; and the images of rendered Markdown, under their paragraph.

use super::{Chip, Kind, Preview, Row, ext, within};
use crate::codec::{base64, deflate};
use crate::core::ui::{self, Seg, c, fg, seg, text_width};
use crate::core::{actions, exec, paths, worker};
use crate::render::image::{self, Pixels};
use crate::render::markdown;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Where the documentation tells which terminals show images in pixels.
const DOCS: &str = "https://github.com/gcaudan/herdr-explorer/blob/main/docs/usage/preview.md#sharp-images";
/// The id of an image file sent in pixels; those of rendered Markdown follow it.
const ID: u32 = 0x4846;

/// The Kitty graphics protocol. The Images setting chooses how an image shows: 'blocks' with half blocks, 'auto' in
/// pixels when the terminal answers the protocol's query (herdr does, in a terminal that supports it), 'pixels' in
/// pixels without waiting for the answer. In pixels, an image is sent once and placed over blank rows. supported: the
/// terminal answered; probing: the answer is awaited, at most 500 ms, before an image is drawn in 'auto'. cell: the
/// size of a cell in pixels, (width, height).
pub struct Kitty {
    pub supported: bool,
    pub probing: bool,
    pub cell: (i64, i64),
    /// The image file's picture sent, and where it is placed ("row;column").
    sent: Option<Rc<Picture>>,
    shown: Option<String>,
    /// The images of rendered Markdown each have an id of their own from next on (picture), kept in figures once
    /// sent; placed: the placements shown, by "image id,placement id", with where and what part, in their order.
    next: u32,
    figures: Vec<u32>,
    placed: Vec<(String, String)>,
    /// What the exit sends: the images still in the terminal deleted.
    pub exit: Arc<Mutex<String>>,
}

impl Kitty {
    pub fn new() -> Kitty {
        Kitty {
            supported: false,
            probing: true,
            cell: (8, 16),
            sent: None,
            shown: None,
            next: ID + 1,
            figures: Vec::new(),
            placed: Vec::new(),
            exit: Arc::new(Mutex::new(String::new())),
        }
    }
}

/// An image file in pixels: its pixels, the cells it covers, its left margin.
#[derive(Debug)]
pub struct Picture {
    px: Pixels,
    cols: i64,
    rows: i64,
    pad: i64,
}

/// An image of rendered Markdown in pixels: its pixels, its id, the cells it covers, its height in pixels.
#[derive(Debug)]
pub struct FigPic {
    px: Pixels,
    pub(super) id: u32,
    cols: i64,
    rows: i64,
    pixels: i64,
}

impl image::Pic for FigPic {
    fn rows(&self) -> i64 {
        self.rows
    }

    fn pixels(&self) -> i64 {
        self.pixels
    }
}

/// A row of an image of rendered Markdown: the image, the row of it.
#[derive(Debug)]
pub struct Fig {
    pub pic: Rc<FigPic>,
    row: i64,
}

/// An image of rendered Markdown, drawn: its lines (None: not drawn, its name shows) and the size and date of its
/// file.
#[derive(Clone)]
struct Shot {
    lines: Option<Vec<markdown::Line>>,
    sig: String,
}

/// The images a Markdown file shows, decoded once per file, width and way of drawing. stale: those of the Markdown
/// file shown before a reload, taken back when their file kept its size and date (an agent rewriting the file
/// reloads it often), the rest dropped once the rows are built again; another file's are dropped at once.
#[derive(Default)]
pub struct Pictures {
    map: HashMap<String, Shot>,
    stale: HashMap<String, Shot>,
    of: Option<String>,
}

/// Deletes an image (d=I) or its placements (d=i).
fn delete(what: char, id: u32) -> String {
    format!("\x1b_Ga=d,d={what},i={id},q=2\x1b\\")
}

/// The pixels of an image sent under an id, as zlib-compressed RGBA in 4096-byte chunks.
fn transmit(px: &Pixels, id: u32) -> String {
    let data = base64::encode(&deflate::deflate_zlib(&px.data));
    let mut out = String::new();
    for i in (0..data.len()).step_by(4096) {
        let more = u8::from(i + 4096 < data.len());
        let chunk = &data[i..data.len().min(i + 4096)];
        if i > 0 {
            out += &format!("\x1b_Gm={more};{chunk}\x1b\\");
        } else {
            out += &format!("\x1b_Ga=t,f=32,o=z,s={},v={},i={id},q=2,m={more};{chunk}\x1b\\", px.width, px.height);
        }
    }
    out
}

fn graphics(text: &str) {
    let _ = crate::sys::write_stdout(text.as_bytes());
}

/// `url.replace(/[?#].*$/, '')`: from the first ? or # after which the line ends with the text.
fn strip_query(url: &str) -> &str {
    let line_end = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    for (i, ch) in url.char_indices() {
        if (ch == '?' || ch == '#') && !url[i + 1..].contains(line_end) {
            return &url[..i];
        }
    }
    url
}

/// Its %XX escapes decoded as UTF-8; None for a % not followed by two hex digits, or bytes that are not UTF-8.
fn decode_uri_component(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok())?;
            if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What picture needs of the pane.
struct Ask<'a> {
    file: &'a str,
    root: &'a str,
    pixels: bool,
    waiting: bool,
}

/// An image of rendered Markdown: local files only (never a URL), relative to the Markdown file, at most 60 columns
/// and 20 rows. In pixels (blank lines that the image is placed over, see place_figures; nothing until the
/// terminal's answer in 'auto') or with half blocks.
fn picture(url: &str, width: usize, a: &Ask, kitty: &mut Kitty, pics: &mut Pictures) -> Option<Vec<markdown::Line>> {
    let scheme = {
        let b = url.as_bytes();
        b.first().is_some_and(u8::is_ascii_alphabetic)
            && url[1..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || "_+.-".contains(ch)))
                .is_some_and(|i| url[1 + i..].starts_with(':'))
    };
    if scheme || url.starts_with("//") {
        return None;
    }
    let rel = strip_query(url);
    let rel = decode_uri_component(rel).unwrap_or_else(|| rel.to_string());
    if rel.is_empty() || !image::FORMATS.contains(&ext(&rel).as_str()) || ext(&rel) == "svg" {
        return None;
    }
    // Checked again once decoded, and kept in the project (or the Markdown file's folder): %5C%5Chost%5Cshare would
    // otherwise reach a network share, which on Windows sends the user's credentials.
    let dir = paths::dirname(a.file);
    let abs = paths::resolve(&[&dir, &rel]);
    let base = if !a.root.is_empty() && within(a.file, a.root) { a.root.to_string() } else { dir };
    if !within(&abs, &base) || a.waiting {
        return None;
    }
    let (cw, ch) = kitty.cell;
    let key = format!("{abs}\0{width}{}", if a.pixels { format!("\0{cw}×{ch}") } else { String::new() });
    if let Some(shot) = pics.map.get(&key) {
        return shot.lines.clone();
    }
    let stat = if exec::remote_link(&abs, &base) { None } else { std::fs::metadata(&abs).ok() };
    let sig = stat.as_ref().map_or(String::new(), |s| {
        let ms = s.modified().map_or(0.0, crate::core::clock::ms);
        format!("{}:{}", crate::json::number(ms), s.len())
    });
    if let Some(old) = pics.stale.get(&key).filter(|o| o.sig == sig) {
        let lines = old.lines.clone();
        pics.map.insert(key, old.clone());
        return lines;
    }
    let mut lines = None;
    if stat.is_some_and(|s| s.is_file() && s.len() <= image::MAX_BYTES as u64)
        && let Ok(buf) = std::fs::read(&abs)
    {
        if a.pixels {
            let room = (width as i64 - 2).clamp(1, 60);
            if let Ok(px) = image::pixels(&buf, (room * cw) as f64, (20 * ch) as f64) {
                let cols = room.min((px.width as f64 / cw as f64).ceil() as i64);
                let rows = (px.height as f64 / ch as f64).ceil() as i64;
                let pixels = px.height as i64;
                let pic = Rc::new(FigPic { px, id: kitty.next, cols, rows, pixels });
                kitty.next += 1;
                lines = Some(
                    (0..rows)
                        .map(|row| markdown::Line {
                            segs: vec![seg(" ", ""), seg(" ".repeat(cols.max(0) as usize), "")],
                            anchor: None,
                            figure: Some(Rc::new(Fig { pic: pic.clone(), row })),
                        })
                        .collect(),
                );
            }
        } else if let Ok(res) = image::render(&buf, (width as f64 - 2.0).min(60.0), 20.0, None) {
            lines = Some(
                res.lines
                    .into_iter()
                    .map(|segs| markdown::Line {
                        segs: [vec![seg(" ", "")], segs].concat(),
                        ..markdown::Line::default()
                    })
                    .collect(),
            );
        }
    }
    pics.map.insert(key, Shot { lines: lines.clone(), sig });
    lines
}

impl Preview {
    /// Rendered Markdown: not while editing, which shows the source.
    pub(super) fn markdown_view(&self) -> bool {
        let s = &self.state;
        !s.diff && s.kind == Kind::Markdown && !s.raw && s.edit.is_none()
    }

    /// Images in pixels (true) or half blocks: p's choice in this pane, else the Images setting.
    pub(super) fn sharp(&self) -> bool {
        let images = self.settings.get("images").and_then(|v| v.as_str()).unwrap_or("");
        self.state.sharp.unwrap_or(images == "pixels" || (images == "auto" && self.kitty.supported))
    }

    /// 'auto' waiting for the terminal's answer: images are not drawn yet.
    pub(super) fn waiting(&self) -> bool {
        let images = self.settings.get("images").and_then(|v| v.as_str()).unwrap_or("");
        self.kitty.probing && self.state.sharp.is_none() && images == "auto"
    }

    /// Why an image is not shown, with how to see it when a tool is missing.
    fn no_preview(&self, why: Option<&str>) -> String {
        let why = why.filter(|w| !w.is_empty()).unwrap_or("No preview");
        match &self.state.missing {
            Some(m) => format!("{why}: {m}"),
            None => why.to_string(),
        }
    }

    /// The image file read: its details in the header; a WebP shown once dwebp has made a PNG of it.
    pub(super) fn load_image(&mut self, file: &str, buf: Vec<u8>, info: String) -> String {
        let buf = Arc::new(buf);
        let s = &mut self.state;
        s.kind = Kind::Image;
        s.buffer = Some(buf.clone());
        s.plain.clear();
        s.source.clear();
        let meta = image::info(&buf);
        let info = match &meta {
            Some(m) => {
                format!("{} {}×{} · {info}", m.format, crate::json::number(m.width), crate::json::number(m.height))
            }
            None => info,
        };
        if meta.is_some_and(|m| m.format == "WebP") {
            s.converting = true;
            let (abs, file) = (paths::resolve(&[file]), file.to_string());
            worker::spawn(
                self.tx.clone(),
                self.waker.clone(),
                Box::new(move || {
                    let png = image::convert(&abs, &buf);
                    Box::new(move |p: &mut Preview| {
                        if p.state.file != file || !p.state.buffer.as_ref().is_some_and(|b| Arc::ptr_eq(b, &buf)) {
                            return;
                        }
                        p.state.converting = false;
                        match png {
                            Some(png) => p.state.buffer = Some(Arc::new(png)),
                            None => p.state.missing = Some("install dwebp (libwebp) to see WebP images".into()),
                        }
                        p.state.rows = None;
                        p.render();
                    })
                }),
            );
        }
        info
    }

    /// The rows of an image file: in pixels, blank rows that the image covers, the first holding what to place (see
    /// place_image); else half blocks, with a line under the image on how to see it sharp.
    pub(super) fn image_rows(&self, w: usize, h: usize) -> Vec<Row> {
        let s = &self.state;
        if s.converting || self.waiting() {
            return Vec::new();
        }
        let (wi, hi) = (w as i64, h as i64);
        let buf = s.buffer.clone().unwrap_or_default();
        let none = |why: &str| vec![Row::empty(&self.no_preview(Some(why)), &[("o", "open"), ("q", "close")])];
        if self.sharp() {
            let (cw, ch) = self.kitty.cell;
            return match image::pixels(&buf, (wi * cw) as f64, ((hi - 2) * ch) as f64) {
                Ok(px) => {
                    let cols = wi.min((px.width as f64 / cw as f64).ceil() as i64);
                    let count = (hi - 2).min((px.height as f64 / ch as f64).ceil() as i64);
                    let pic = Rc::new(Picture { px, cols, rows: count, pad: ((wi - cols) / 2).max(0) });
                    (0..count.max(0))
                        .map(|k| Row { src: -1, picture: (k == 0).then(|| pic.clone()), ..Row::default() })
                        .collect()
                }
                Err(why) => none(&why),
            };
        }
        let hint = self.image_hint(w);
        match image::render(&buf, w as f64, (hi - 2 - if hint.is_some() { 2 } else { 0 }) as f64, None) {
            Ok(res) => {
                let pad = ((wi - res.width as i64) / 2).max(0) as usize;
                let mut list: Vec<Row> = res
                    .lines
                    .into_iter()
                    .map(|segs| Row {
                        segs: [vec![seg(" ".repeat(pad), "")], segs].concat(),
                        src: -1,
                        ..Row::default()
                    })
                    .collect();
                if let Some(hint) = hint {
                    list.push(Row { src: -1, ..Row::default() });
                    list.push(hint);
                }
                list
            }
            Err(why) => none(&why),
        }
    }

    /// Rendered Markdown, its images drawn by picture; its source when it cannot be rendered.
    pub(super) fn markdown_rows(&mut self, width: usize) -> Vec<Row> {
        let waiting = self.waiting();
        let a = Ask { file: &self.state.file, root: &self.state.root, pixels: self.sharp(), waiting };
        let (kitty, pics) = (&mut self.kitty, &mut self.pictures);
        let mut draw = |url: &str, width: usize| picture(url, width, &a, kitty, pics);
        let opts = markdown::Options { highlight: true, image: Some(&mut draw) };
        let res = markdown::render(&self.state.text, (width as f64 - 1.0).clamp(1.0, 120.0), opts);
        // The images taken back are in pictures now; the others are dropped.
        if !waiting {
            self.pictures.stale.clear();
        }
        match res {
            Some(res) => res
                .lines
                .into_iter()
                .map(|l| Row {
                    segs: [vec![seg(" ", "")], l.segs].concat(),
                    src: -1,
                    pad: 1,
                    figure: l.figure.and_then(|f| f.downcast::<Fig>().ok()),
                    ..Row::default()
                })
                .collect(),
            None => self
                .state
                .source
                .iter()
                .enumerate()
                .map(|(i, segs)| Row { segs: segs.clone(), src: i as i64, first: true, ..Row::default() })
                .collect(),
        }
    }

    /// The pictures of the file shown before a reload kept aside (see Pictures).
    pub(super) fn forget_pictures(&mut self, file: &str) {
        let p = &mut self.pictures;
        // Two reloads in a row, with no drawing between them, keep the older ones.
        if p.of.as_deref() != Some(file) {
            p.stale.clear();
        } else if !p.map.is_empty() {
            p.stale = p.map.clone();
        }
        p.of = Some(file.to_string());
        p.map.clear();
    }

    fn has_pictures(&self) -> bool {
        self.pictures.map.values().any(|p| p.lines.is_some())
    }

    /// The Image or Images item of the view's menu: (label, value), when p does something.
    pub(super) fn sharp_item(&self) -> Option<(&'static str, &'static str)> {
        let value = if self.sharp() { "sharp" } else { "pixelated" };
        if self.state.kind == Kind::Image {
            Some(("Image", value))
        } else if self.markdown_view() && self.has_pictures() {
            Some(("Images", value))
        } else {
            None
        }
    }

    /// The line under an image drawn with half blocks: p and the setting when the terminal has the protocol, else
    /// where the documentation tells which terminals have it. Nothing until the answer, nor when p chose half blocks.
    /// Its parts with an action are clickable (image_click).
    fn image_hint(&self, w: usize) -> Option<Row> {
        if self.kitty.probing || self.state.sharp == Some(false) {
            return None;
        }
        let (dim, key) = (fg(c().dim), format!("{}\x1b[1m", fg(c().text)));
        let link = format!("{}\x1b[4m", fg(c().blue));
        let part = |text: &str, style: &String, id: &str| (text.to_string(), style.clone(), id.to_string());
        let choices = if self.kitty.supported {
            vec![
                vec![
                    part("▦ Pixelated  ", &dim, ""),
                    part("p", &key, "p"),
                    part(" sharp", &dim, "p"),
                    part("  ·  ", &dim, ""),
                    part("sharp by default", &link, "default"),
                ],
                vec![part("p", &key, "p"), part(" sharp", &dim, "p")],
            ]
        } else {
            vec![
                vec![
                    part("▦ Pixelated: this terminal shows no image in pixels  ", &dim, ""),
                    part("Sharp images ↗", &link, "docs"),
                ],
                vec![part("▦ Pixelated  ", &dim, ""), part("Sharp images ↗", &link, "docs")],
                vec![part("Sharp images ↗", &link, "docs")],
            ]
        };
        let width = |c: &Vec<(String, String, String)>| c.iter().map(|p| text_width(&p.0) as i64).sum::<i64>();
        let parts = choices.iter().find(|c| width(c) <= w as i64 - 2).unwrap_or(&choices[choices.len() - 1]);
        let pad = ((w as i64 - width(parts)) / 2).max(0);
        // The screen columns of each part, from 1.
        let mut x = pad + 1;
        let mut chips = Vec::new();
        for (text, _, id) in parts {
            let from = x;
            x += text_width(text) as i64;
            if !id.is_empty() {
                chips.push(Chip { id: id.clone(), from, to: x - 1, tab: None });
            }
        }
        let mut segs: Vec<Seg> = vec![seg(" ".repeat(pad as usize), "")];
        segs.extend(parts.iter().map(|(text, style, _)| seg(text.clone(), style.clone())));
        Some(Row { segs, src: -1, hint: Some(chips), ..Row::default() })
    }

    /// A click on a part of the image's hint.
    pub(super) fn image_click(&mut self, id: &str) {
        match id {
            "p" => self.key("p"),
            "docs" => self.act(|| Some(actions::web(DOCS, "the documentation"))),
            "default" => {
                self.settings.set("images", crate::json::Value::Str("auto".into()));
                let mut saved = exec::read_settings();
                saved.set("images", crate::json::Value::Str("auto".into()));
                exec::write_settings(&saved);
                self.state.rows = None;
                self.flash(Some("Images: sharp when the terminal can".into()));
            }
            _ => {}
        }
    }

    /// p: this pane's images in pixels or half blocks, whatever the setting: an image file, or those of rendered
    /// Markdown.
    pub(super) fn toggle_sharp(&mut self) {
        if self.state.kind != Kind::Image && !(self.markdown_view() && self.has_pictures()) {
            return;
        }
        self.state.sharp = Some(!self.sharp());
        self.state.rows = None;
        if self.state.sharp == Some(true) && !self.kitty.supported && !self.kitty.probing {
            self.flash(Some("The terminal did not answer the Kitty graphics query: the image may not show".into()));
        } else {
            self.render();
        }
    }

    /// After each frame: the image of the rows placed, sent first when new; removed when none shows (another file,
    /// the key help over the pane).
    pub(super) fn place_image(&mut self) {
        let s = &self.state;
        let on = !s.diff
            && s.kind == Kind::Image
            && s.edit.is_none()
            && self.sharp()
            && s.help.is_none()
            && s.menu.is_none();
        let pic = s.rows.as_ref().filter(|_| on).and_then(|r| r.list.first()).and_then(|r| r.picture.clone());
        let k = &mut self.kitty;
        let Some(pic) = pic else {
            if k.shown.is_some() {
                graphics(&delete(if s.kind == Kind::Image { 'i' } else { 'I' }, ID));
            }
            k.shown = None;
            if s.kind != Kind::Image {
                k.sent = None;
            }
            return self.exit_text();
        };
        if !k.sent.as_ref().is_some_and(|p| Rc::ptr_eq(p, &pic)) {
            graphics(&(delete('I', ID) + &transmit(&pic.px, ID)));
            k.sent = Some(pic.clone());
            k.shown = None;
        }
        let at = format!("{};{}", 2 - s.top, pic.pad + 1);
        if k.shown.as_deref() != Some(at.as_str()) {
            // The cursor saved and restored around the placement, which C=1 keeps from moving anyway.
            graphics(&format!("\x1b7\x1b[{at}H\x1b_Ga=p,i={ID},p=1,c={},r={},C=1,q=2\x1b\\\x1b8", pic.cols, pic.rows));
            k.shown = Some(at);
        }
        self.exit_text();
    }

    /// After each frame, the images of rendered Markdown in pixels (picture): each one in view placed over its blank
    /// rows, cut to the rows that show (image::shown), sent first when new. A placement is removed once its image
    /// scrolls out, and all are while something covers the pane (the key help, the menu) or the view changes; an
    /// image no longer in the rows (another file, the source, the Diff view, another width) is deleted. Two copies of
    /// an image share its id, each placed under the row where it starts (placement id).
    pub(super) fn place_figures(&mut self) {
        let s = &self.state;
        let on = self.markdown_view() && self.sharp() && s.help.is_none() && s.menu.is_none();
        let rows = s.rows.clone();
        let empty = Vec::new();
        let list = rows.as_ref().map_or(&empty, |r| &r.list);
        let figs: Vec<Option<(&FigPic, i64)>> =
            list.iter().map(|r| r.figure.as_ref().map(|f| (&*f.pic, f.row))).collect();
        let want: Vec<(String, image::Placed<FigPic>)> = if on {
            image::shown(&figs, s.top, self.size().1 as i64 - 2)
                .into_iter()
                .map(|f| (format!("{},{}", f.pic.id, f.start + 1), f))
                .collect()
        } else {
            Vec::new()
        };
        let k = &mut self.kitty;
        let mut out = String::new();
        k.placed.retain(|(key, _)| {
            if want.iter().any(|(w, _)| w == key) {
                return true;
            }
            let (i, p) = key.split_once(',').unwrap_or((key, ""));
            out += &format!("\x1b_Ga=d,d=i,i={i},p={p},q=2\x1b\\");
            false
        });
        let held = rows.as_ref().map_or(&[][..], |r| &r.figures[..]);
        k.figures.retain(|id| {
            if held.contains(id) {
                return true;
            }
            out += &delete('I', *id);
            false
        });
        for (key, f) in &want {
            let pic = f.pic;
            if !k.figures.contains(&pic.id) {
                out += &transmit(&pic.px, pic.id);
                k.figures.push(pic.id);
            }
            // The image's left column: the row ends with its blank cells.
            let row = &list[(f.start + f.part.from) as usize];
            let x = ui::seg_width(&row.segs) as i64 - pic.cols + 1;
            let at = format!("{};{x}", 2 + f.part.at);
            let part = format!("{at};{};{};{}", f.part.y, f.part.h, f.part.count);
            match k.placed.iter_mut().find(|(p, _)| p == key) {
                Some((_, was)) if *was == part => continue,
                Some((_, was)) => *was = part,
                None => k.placed.push((key.clone(), part)),
            }
            out += &format!(
                "\x1b7\x1b[{at}H\x1b_Ga=p,i={},p={},x=0,y={},w={},h={},c={},r={},C=1,q=2\x1b\\\x1b8",
                pic.id,
                f.start + 1,
                f.part.y,
                pic.px.width,
                f.part.h,
                pic.cols,
                f.part.count
            );
        }
        if !out.is_empty() {
            graphics(&out);
        }
        self.exit_text();
    }

    /// What the exit sends: the image file's picture and every image of rendered Markdown deleted.
    fn exit_text(&self) {
        let k = &self.kitty;
        let mut text = if k.sent.is_some() { delete('I', ID) } else { String::new() };
        text.extend(k.figures.iter().map(|id| delete('I', *id)));
        *k.exit.lock().unwrap_or_else(|e| e.into_inner()) = text;
    }
}
