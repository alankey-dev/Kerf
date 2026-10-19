//! The SVG writer: `width` and `height` in the chosen unit over a viewBox in points, so the file opens at
//! true scale anywhere that honours units. Each layer and group is an Inkscape layer. In Filled style an
//! image layer embeds its pixels as a PNG data URI clipped by its outline (spec 37).

use std::collections::HashSet;

use crate::assets::Pixels;
use crate::design::{Contour, POINTS_PER_INCH, Pt, Seg};
use crate::units::Units;

use super::picture::{Prepared, base64, png, prepare};
use super::{APP_NAME, Layer, Sheet, Style};

/// Filled style strokes lines at 0.01 in; Outlines style draws every contour as a 0.001 in hairline.
const LINE_IN: f32 = 0.01;
const HAIRLINE_IN: f32 = 0.001;

pub fn document(sheet: &Sheet, units: Units, style: Style, pixels: &Pixels) -> String {
    let (w, h) = sheet.size_in;
    let unit = units.file_unit();
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {APP_NAME}: {} -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{}{unit}\" height=\"{}{unit}\" viewBox=\"0 0 {} {}\">\n",
        escape(&sheet.name),
        num(units.file_from_in(w)),
        num(units.file_from_in(h)),
        num(w * POINTS_PER_INCH),
        num(h * POINTS_PER_INCH),
    );
    let mut ids = HashSet::new();
    let mut open: Vec<&str> = Vec::new();
    for layer in &sheet.layers {
        let shared = open.iter().zip(&layer.group).take_while(|(a, b)| **a == b.as_str()).count();
        for _ in shared..open.len() {
            out += "</g>\n";
        }
        open.truncate(shared);
        for g in &layer.group[shared..] {
            out += &open_group(g, &mut ids);
            open.push(g);
        }
        out += &open_group(&layer.name, &mut ids);
        match layer.picture.as_ref().filter(|_| style == Style::Filled).and_then(|p| prepare(p, pixels)) {
            Some(prepared) => out += &image(layer, &prepared, &mut ids),
            None => out += &paths(layer, style),
        }
        out += "</g>\n";
    }
    out += &"</g>\n".repeat(open.len());
    out + "</svg>\n"
}

fn open_group(name: &str, ids: &mut HashSet<String>) -> String {
    let base = slug(name);
    let id = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|id| !ids.contains(id)).expect("an unused id");
    ids.insert(id.clone());
    format!("<g id=\"{id}\" inkscape:label=\"{}\" inkscape:groupmode=\"layer\">\n", escape(name))
}

fn paths(layer: &Layer, style: Style) -> String {
    let colour = layer.hex();
    let stroke = |contours: Vec<&Contour>, width_in: f32| match contours.is_empty() {
        true => String::new(),
        false => format!("<path d=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"{}\"/>\n", d(&contours), num(width_in * POINTS_PER_INCH)),
    };
    let all: Vec<&Contour> = layer.contours.iter().collect();
    match style {
        Style::Outlines => stroke(all, HAIRLINE_IN),
        Style::Filled if layer.draws_lines() => stroke(all, LINE_IN),
        Style::Filled => {
            let (closed, open): (Vec<&Contour>, Vec<&Contour>) = all.into_iter().partition(|c| c.closed);
            let fill = match closed.is_empty() {
                true => String::new(),
                false => format!("<path d=\"{}\" fill=\"{colour}\" fill-rule=\"evenodd\"/>\n", d(&closed)),
            };
            fill + &stroke(open, LINE_IN)
        }
    }
}

/// The pixels at their corners, inside a group clipped by the layer's closed outline. The image is laid out
/// one unit per pixel and the matrix carries that box onto its corners.
fn image(layer: &Layer, prepared: &Prepared, ids: &mut HashSet<String>) -> String {
    let id = (1..).map(|n| format!("clip-{n}")).find(|id| !ids.contains(id)).expect("an unused id");
    ids.insert(id.clone());
    let closed: Vec<&Contour> = layer.contours.iter().filter(|c| c.closed).collect();
    let (w, h) = prepared.image.dimensions();
    let [tl, tr, bl] = prepared.corners.map(|(x, y)| (x * POINTS_PER_INCH, y * POINTS_PER_INCH));
    let matrix = [(tr.0 - tl.0) / w as f32, (tr.1 - tl.1) / w as f32, (bl.0 - tl.0) / h as f32, (bl.1 - tl.1) / h as f32, tl.0, tl.1].map(|v| places(v, 6)).join(" ");
    format!(
        "<clipPath id=\"{id}\"><path d=\"{}\" clip-rule=\"evenodd\"/></clipPath>\n<g clip-path=\"url(#{id})\"><image href=\"data:image/png;base64,{}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" transform=\"matrix({matrix})\"/></g>\n",
        d(&closed),
        base64(&png(&prepared.image)),
    )
}

fn d(contours: &[&Contour]) -> String {
    let pt = |(x, y): Pt| format!("{} {}", num(x * POINTS_PER_INCH), num(y * POINTS_PER_INCH));
    let mut d = Vec::new();
    for c in contours {
        d.push(format!("M{}", pt(c.start)));
        for seg in &c.segs {
            d.push(match *seg {
                Seg::Line(p) => format!("L{}", pt(p)),
                Seg::Cubic(a, b, p) => format!("C{} {} {}", pt(a), pt(b), pt(p)),
            });
        }
        if c.closed {
            d.push("Z".into());
        }
    }
    d.join(" ")
}

/// Up to four decimals, trailing zeros trimmed.
fn num(v: f32) -> String {
    places(v, 4)
}

/// Up to `n` decimals, trailing zeros trimmed. A per-pixel matrix needs six: its entries can be a few
/// thousandths of a point.
fn places(v: f32, n: usize) -> String {
    let s = format!("{v:.n$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// An XML id: lowercase letters, digits and single hyphens, starting with a letter.
fn slug(name: &str) -> String {
    let mut out = String::from("layer-");
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::assets::Pixels;
    use crate::design::{Contour, Design, Seg, ShapeKind};
    use crate::export::tests::{mat, request};
    use crate::export::{Format, Scope, Style, plan};
    use crate::units::Units;

    /// A 2 in red image cut along a diamond, with its pixels.
    fn diamond_image() -> (Design, Pixels) {
        let img = image::RgbaImage::from_pixel(200, 200, image::Rgba([255, 0, 0, 255]));
        let asset = crate::assets::id_of(&img);
        let diamond = Contour { start: (100.0, 0.0), segs: vec![Seg::Line((200.0, 100.0)), Seg::Line((100.0, 200.0)), Seg::Line((0.0, 100.0))], closed: true };
        let mut design = Design::default();
        design.add_shape(ShapeKind::Bitmap { asset: asset.clone(), px: (200, 200), clip: vec![diamond] });
        (design, Pixels::from([(asset, Arc::new(img))]))
    }

    #[test]
    fn filled_svg_embeds_the_image_clipped_by_its_cut_line_and_outlines_does_not() {
        let (design, pixels) = diamond_image();
        let [file] = &plan(&design, &request(Format::Svg, Scope::Design, Units::Imperial), "p", mat())[..] else { panic!("one file") };
        assert_eq!(file.assets().count(), 1, "Filled draws the image");
        let svg = String::from_utf8(file.bytes(&pixels)).unwrap();
        assert_eq!(svg.matches("<image href=\"data:image/png;base64,").count(), 1, "{svg}");
        assert!(svg.contains("<clipPath id=\"clip-1\"><path d=\"M") && svg.contains("<g clip-path=\"url(#clip-1)\"><image "), "clipped by the outline: {svg}");
        assert!(!svg.contains("fill=\"#"), "the picture replaces the colour fill");

        let outlines = crate::export::Request { style: Style::Outlines, ..request(Format::Svg, Scope::Design, Units::Imperial) };
        let [file] = &plan(&design, &outlines, "p", mat())[..] else { panic!("one file") };
        assert_eq!(file.assets().count(), 0, "Outlines needs no pixels");
        let svg = String::from_utf8(file.bytes(&pixels)).unwrap();
        assert!(!svg.contains("<image") && svg.contains("<path"), "the cut line only: {svg}");
    }

    #[test]
    fn a_pattern_fill_exports_as_its_tiles_over_the_outline_on_the_design_and_on_a_mat() {
        use crate::patterns::{PatternMode, fixture};
        let design = fixture::circle(PatternMode::Tile);
        for scope in [Scope::Design, Scope::PerMat] {
            let files = plan(&design, &request(Format::Svg, scope, Units::Imperial), "p", mat());
            let [file] = &files[..] else { panic!("one file") };
            let [layer] = &file.sheets()[0].layers[..] else { panic!("one layer") };
            let picture = layer.picture.as_ref().expect("the pattern exports as a picture");
            let [tl, tr, bl] = picture.corners;
            assert!(((tr.0 - tl.0) - 2.0).abs() < 0.01 && ((bl.1 - tl.1) - 2.0).abs() < 0.01, "{scope:?}: over the 2 in circle: {:?}", picture.corners);
            let tile = picture.tile.expect("tiled");
            assert!(((tile[1].0 - tile[0].0) - 0.5).abs() < 1e-3, "{scope:?}: 0.5 in tiles: {tile:?}");
            let svg = String::from_utf8(file.bytes(&fixture::pixels())).unwrap();
            assert_eq!(svg.matches("<image href=\"data:image/png;base64,").count(), 1, "{scope:?}");
        }
    }

    #[test]
    fn an_image_whose_pixels_are_missing_exports_its_filled_cut_line() {
        let (design, _) = diamond_image();
        let [file] = &plan(&design, &request(Format::Svg, Scope::Design, Units::Imperial), "p", mat())[..] else { panic!("one file") };
        let svg = String::from_utf8(file.bytes(&Pixels::new())).unwrap();
        assert!(!svg.contains("<image") && svg.contains("fill=\"#"), "{svg}");
    }
}
