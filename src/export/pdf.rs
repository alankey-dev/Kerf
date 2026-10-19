//! The PDF writer: vector paths, one page per sheet, each layer an optional content group named like the
//! SVG layer, so Acrobat and Illustrator list the operations as layers. `page` is public so the print
//! sheet (spec 27) can reuse it. In Filled style an image layer draws its pixels (spec 37) as a
//! Flate-compressed RGB image XObject with an alpha SMask, clipped to the layer's outline.

use image::RgbaImage;
use pdf_writer::{Array, Content, Filter, Name, Pdf, Rect, Ref, TextStr};

use crate::assets::Pixels;
use crate::design::{Contour, POINTS_PER_INCH, Pt, Seg};

use super::picture::{Prepared, prepare};
use super::{APP_NAME, Layer, Sheet, Style};

const LINE_IN: f32 = 0.01;
const HAIRLINE_IN: f32 = 0.001;

/// A PDF being built page by page. The layer panel lists each page as a labelled group holding its
/// layers, nested by the layers' group paths.
pub struct PdfBuilder {
    pdf: Pdf,
    next: i32,
    page_tree: Ref,
    pages: Vec<Ref>,
    ocgs: Vec<Ref>,
    order: Vec<Order>,
}

enum Order {
    Layer(Ref),
    Group(String, Vec<Order>),
}

impl PdfBuilder {
    pub fn new() -> Self {
        Self { pdf: Pdf::new(), next: 2, page_tree: Ref::new(1), pages: Vec::new(), ocgs: Vec::new(), order: Vec::new() }
    }

    fn alloc(&mut self) -> Ref {
        self.next += 1;
        Ref::new(self.next - 1)
    }

    pub fn finish(mut self) -> Vec<u8> {
        let (catalog, info) = (self.alloc(), self.alloc());
        self.pdf.pages(self.page_tree).kids(self.pages.iter().copied()).count(self.pages.len() as i32);
        {
            let mut cat = self.pdf.catalog(catalog);
            cat.pages(self.page_tree);
            let mut props = cat.insert(Name(b"OCProperties")).dict();
            props.insert(Name(b"OCGs")).array().items(self.ocgs.iter().copied());
            let mut config = props.insert(Name(b"D")).dict();
            write_order(&mut config.insert(Name(b"Order")).array(), &self.order);
        }
        self.pdf.document_info(info).producer(TextStr(APP_NAME)).creator(TextStr(APP_NAME));
        self.pdf.finish()
    }
}

fn write_order(array: &mut Array, items: &[Order]) {
    for item in items {
        match item {
            Order::Layer(ocg) => _ = array.item(*ocg),
            Order::Group(label, kids) => {
                let mut group = array.push().array();
                group.item(TextStr(label));
                write_order(&mut group, kids);
            }
        }
    }
}

/// Adds `sheet` as a page the size of the sheet, drawing images from `pixels`.
pub fn page(b: &mut PdfBuilder, sheet: &Sheet, style: Style, pixels: &Pixels) {
    let height_pt = sheet.size_in.1 * POINTS_PER_INCH;
    let (page_id, content_id) = (b.alloc(), b.alloc());
    let mut content = Content::new();
    let mut properties = Vec::new();
    let mut images = Vec::new();
    let mut tree = Vec::new();
    for (i, layer) in sheet.layers.iter().enumerate() {
        let ocg = b.alloc();
        b.pdf.indirect(ocg).dict().pair(Name(b"Type"), Name(b"OCG")).pair(Name(b"Name"), TextStr(&layer.name));
        b.ocgs.push(ocg);
        let key = format!("oc{i}");
        content.begin_marked_content_with_properties(Name(b"OC")).properties_named(Name(key.as_bytes()));
        let picture = layer.picture.as_ref().filter(|_| style == Style::Filled).and_then(|p| prepare(p, pixels));
        match picture {
            Some(prepared) => {
                let name = format!("im{}", images.len());
                draw_image(&mut content, layer, &prepared, &name, height_pt);
                images.push((name, image_xobject(b, &prepared.image)));
            }
            None => draw(&mut content, layer, style, height_pt),
        }
        content.end_marked_content();
        properties.push((key, ocg));
        nest(&mut tree, &layer.group, ocg);
    }
    b.pdf.stream(content_id, &content.finish());
    {
        let mut page = b.pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, sheet.size_in.0 * POINTS_PER_INCH, height_pt)).parent(b.page_tree).contents(content_id);
        let mut resources = page.resources();
        {
            let mut dict = resources.insert(Name(b"Properties")).dict();
            for (key, ocg) in &properties {
                dict.pair(Name(key.as_bytes()), *ocg);
            }
        }
        if !images.is_empty() {
            let mut dict = resources.x_objects();
            for (name, id) in &images {
                dict.pair(Name(name.as_bytes()), *id);
            }
        }
    }
    b.pages.push(page_id);
    b.order.push(Order::Group(sheet.name.clone(), tree));
}

/// Files `ocg` under its group path. Layers of one group are adjacent, so a group is always the last entry.
fn nest(tree: &mut Vec<Order>, path: &[String], ocg: Ref) {
    let Some((first, rest)) = path.split_first() else {
        return tree.push(Order::Layer(ocg));
    };
    if !matches!(tree.last(), Some(Order::Group(label, _)) if label == first) {
        tree.push(Order::Group(first.clone(), Vec::new()));
    }
    if let Some(Order::Group(_, kids)) = tree.last_mut() {
        nest(kids, rest, ocg);
    }
}

fn draw(content: &mut Content, layer: &Layer, style: Style, height_pt: f32) {
    let [r, g, b] = [layer.colour.r(), layer.colour.g(), layer.colour.b()].map(|c| f32::from(c) / 255.0);
    content.set_fill_rgb(r, g, b).set_stroke_rgb(r, g, b);
    let up = |(x, y): Pt| (x * POINTS_PER_INCH, height_pt - y * POINTS_PER_INCH);
    let stroke = |content: &mut Content, contours: &[&Contour], width_in: f32| {
        if contours.is_empty() {
            return;
        }
        content.set_line_width(width_in * POINTS_PER_INCH);
        path(content, contours, up);
        content.stroke();
    };
    let all: Vec<&Contour> = layer.contours.iter().collect();
    match style {
        Style::Outlines => stroke(content, &all, HAIRLINE_IN),
        Style::Filled if layer.draws_lines() => stroke(content, &all, LINE_IN),
        Style::Filled => {
            let (closed, open): (Vec<&Contour>, Vec<&Contour>) = all.into_iter().partition(|c| c.closed);
            if !closed.is_empty() {
                path(content, &closed, up);
                content.fill_even_odd();
            }
            stroke(content, &open, LINE_IN);
        }
    }
}

/// The image clipped to the layer's closed outline. Image space is the unit square with its first row at
/// the top, so the matrix sends (0, 1) to the top-left corner, (1, 1) to the top-right and (0, 0) to the
/// bottom-left.
fn draw_image(content: &mut Content, layer: &Layer, prepared: &Prepared, name: &str, height_pt: f32) {
    let up = |(x, y): Pt| (x * POINTS_PER_INCH, height_pt - y * POINTS_PER_INCH);
    let closed: Vec<&Contour> = layer.contours.iter().filter(|c| c.closed).collect();
    let [tl, tr, bl] = prepared.corners.map(up);
    content.save_state();
    path(content, &closed, up);
    content.clip_even_odd().end_path();
    content.transform([tr.0 - tl.0, tr.1 - tl.1, tl.0 - bl.0, tl.1 - bl.1, bl.0, bl.1]);
    content.x_object(Name(name.as_bytes()));
    content.restore_state();
}

/// Writes `image` as an 8-bit RGB XObject with its alpha as an SMask, both Flate-compressed.
fn image_xobject(b: &mut PdfBuilder, image: &RgbaImage) -> Ref {
    let (id, mask) = (b.alloc(), b.alloc());
    let (w, h) = (image.width() as i32, image.height() as i32);
    let deflate = |bytes: Vec<u8>| miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 6);
    let rgb = deflate(image.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect());
    let alpha = deflate(image.pixels().map(|p| p[3]).collect());
    {
        let mut x = b.pdf.image_xobject(id, &rgb);
        x.filter(Filter::FlateDecode);
        x.width(w).height(h).bits_per_component(8).s_mask(mask);
        x.color_space().device_rgb();
    }
    let mut x = b.pdf.image_xobject(mask, &alpha);
    x.filter(Filter::FlateDecode);
    x.width(w).height(h).bits_per_component(8);
    x.color_space().device_gray();
    id
}

fn path(content: &mut Content, contours: &[&Contour], up: impl Fn(Pt) -> Pt) {
    for c in contours {
        let (x, y) = up(c.start);
        content.move_to(x, y);
        for seg in &c.segs {
            match *seg {
                Seg::Line(p) => {
                    let (x, y) = up(p);
                    content.line_to(x, y);
                }
                Seg::Cubic(a, b, p) => {
                    let ((x1, y1), (x2, y2), (x3, y3)) = (up(a), up(b), up(p));
                    content.cubic_to(x1, y1, x2, y2, x3, y3);
                }
            }
        }
        if c.closed {
            content.close_path();
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use crate::design::{Design, Operation, ShapeKind};
    use crate::export::tests::{add, mat, request};
    use crate::export::{Format, Scope, plan};
    use crate::units::Units;

    #[test]
    fn a_two_mat_export_has_a_mat_sized_page_per_mat_with_named_layers() {
        let mut d = Design::default();
        add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::RED, (1.0, 1.0));
        add(&mut d, ShapeKind::Heart, Operation::Score, Color32::BLUE, (5.0, 1.0));
        let [file] = &plan(&d, &request(Format::Pdf, Scope::Design, Units::Imperial), "p", mat())[..] else { panic!("one file") };
        assert_eq!(file.name, "p.pdf");
        let pdf = String::from_utf8_lossy(&file.bytes(&Default::default())).into_owned();
        let media_box = format!("/MediaBox [0 0 {} {}]", mat().width_in * 72.0, mat().height_in * 72.0);
        assert_eq!(pdf.matches(&media_box).count(), 2, "{media_box} twice in\n{pdf}");
        assert_eq!(pdf.matches("/OC /oc0 BDC").count(), 2, "each page marks its layer as optional content");
        for name in ["(Basic Cut - Square)", "(Score - Heart)", "(Mat 1 - Red - Basic Cut)", "(Mat 2 - Blue - Score)"] {
            assert!(pdf.contains(name), "{name}");
        }
        assert!(pdf.contains("/Producer (Kerf)"), "names its maker");
    }
}
