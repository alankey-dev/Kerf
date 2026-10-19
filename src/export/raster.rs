//! Raster export (spec 54): draw an export [`Sheet`] to pixels with tiny-skia at a chosen resolution,
//! in the same per-operation styles the canvas uses, so a PNG or JPEG matches the Project Preview. A
//! Score line is dashed, a cut shape fills its colour and a Print Then Cut image shows its pixels, all
//! through `op_style`. Images draw from the full-size decode in `pixels`, not the thumbnail copy. Pure
//! given those pixels; the dialog checks the size before it runs this, as a 600 dpi mat is hundreds of
//! megapixels.

use std::io::Cursor;

use image::RgbaImage;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

use crate::assets::Pixels;
use crate::design::{Contour, Pt, Seg};
use crate::pens::Pen;
use crate::ui::op_style::{self, Edge, Line, Mode};

use super::{Layer, Sheet, picture};

/// An export larger than this many megapixels is refused, as Upload refuses large images.
pub const MAX_MEGAPIXELS: f64 = 100.0;
/// The margin a raster export keeps around the design, as SVG uses.
pub const DEFAULT_MARGIN_IN: f32 = 0.25;

/// What fills the pixels behind the design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    /// PNG only: the pixels outside the design stay fully transparent.
    Transparent,
    White,
    /// The artboard colour, with its grid and rulers off.
    Mat,
}

/// Everything a raster export needs beyond the sheets: resolution, what fills the background, the
/// margin, the JPEG quality and whether to draw as the machine makes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterOptions {
    pub dpi: u32,
    pub background: Background,
    pub margin_in: f32,
    /// JPEG quality, 60 to 100; ignored by PNG.
    pub quality: u8,
    /// View's Show as made: cut outlines only, no fills or tool styles.
    pub as_made: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self { dpi: 300, background: Background::Transparent, margin_in: DEFAULT_MARGIN_IN, quality: 90, as_made: false }
    }
}

impl RasterOptions {
    fn mode(self) -> Mode {
        if self.as_made { Mode::AsMade } else { Mode::Design }
    }
}

/// The pixel size `opts` writes a sheet of `size_in` inches at: the design plus a margin each side.
pub fn dimensions(size_in: Pt, opts: &RasterOptions) -> (u32, u32) {
    let px = |len: f32| (((len + 2.0 * opts.margin_in) * opts.dpi as f32).round() as i64).clamp(1, i64::from(u32::MAX)) as u32;
    (px(size_in.0), px(size_in.1))
}

/// How many megapixels a sheet of `size_in` would be at `opts`.
pub fn megapixels(size_in: Pt, opts: &RasterOptions) -> f64 {
    let (w, h) = dimensions(size_in, opts);
    f64::from(w) * f64::from(h) / 1_000_000.0
}

/// A message naming the size when a sheet would exceed [`MAX_MEGAPIXELS`], else `None`.
pub fn oversize(size_in: Pt, opts: &RasterOptions) -> Option<String> {
    let mp = megapixels(size_in, opts);
    (mp > MAX_MEGAPIXELS).then(|| {
        let (w, h) = dimensions(size_in, opts);
        format!("This image would be {w} x {h} px ({mp:.0} megapixels), over the {MAX_MEGAPIXELS:.0} megapixel limit. Lower the resolution, or export one mat at a time.")
    })
}

/// A rough byte estimate for the file list, before anything is rendered. Flat cut art compresses hard;
/// a photo does not, so this is only an order of magnitude.
pub fn estimate_bytes(w: u32, h: u32, jpeg: bool, quality: u8) -> u64 {
    let px = u64::from(w) * u64::from(h);
    if jpeg { (px * u64::from(quality) / 400).max(1024) } else { (px / 4).max(1024) }
}

/// The sheet drawn to pixels at `opts.dpi`, in the canvas styles. [`oversize`] must pass first.
pub fn render(sheet: &Sheet, opts: &RasterOptions, pixels: &Pixels) -> RgbaImage {
    let (w, h) = dimensions(sheet.size_in, opts);
    let mut pixmap = Pixmap::new(w, h).expect("the raster size is checked before render");
    match opts.background {
        Background::Transparent => {}
        Background::White => pixmap.fill(tiny_skia::Color::WHITE),
        Background::Mat => {
            let c = crate::ui::tokens::BG_ARTBOARD;
            pixmap.fill(tiny_skia::Color::from_rgba8(c.r(), c.g(), c.b(), 255));
        }
    }
    let (dpi, margin) = (opts.dpi as f32, opts.margin_in);
    let map = move |(x, y): Pt| ((x + margin) * dpi, (y + margin) * dpi);
    for layer in &sheet.layers {
        draw_layer(&mut pixmap, layer, dpi, opts.mode(), pixels, &map);
    }
    to_image(&pixmap)
}

fn draw_layer(pixmap: &mut Pixmap, layer: &Layer, dpi: f32, mode: Mode, pixels: &Pixels, map: &impl Fn(Pt) -> Pt) {
    let look = op_style::look(layer.operation, layer.colour, Pen::default(), dpi, mode);
    let Some(path) = build_path(&layer.contours, map) else { return };
    if let Some(fill) = look.fill {
        paint_fill(pixmap, &path, fill, layer, pixels, map);
    }
    if let Some(edge) = look.edge {
        stroke_edge(pixmap, &path, edge);
    }
}

/// The layer's area filled with its picture when it has one, else its colour. A missing image falls
/// back to the colour, as the thumbnail does.
fn paint_fill(pixmap: &mut Pixmap, path: &tiny_skia::Path, fill: egui::Color32, layer: &Layer, pixels: &Pixels, map: &impl Fn(Pt) -> Pt) {
    if let Some(prepared) = layer.picture.as_ref().and_then(|p| picture::prepare(p, pixels))
        && let Some(raster) = picture::premultiplied(&prepared.image)
    {
        let paint = Paint { shader: picture::image_shader(&raster, prepared.corners.map(map), false), anti_alias: true, ..Paint::default() };
        pixmap.fill_path(path, &paint, FillRule::EvenOdd, Transform::identity(), None);
        return;
    }
    let [r, g, b, a] = fill.to_srgba_unmultiplied();
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    paint.set_color_rgba8(r, g, b, a);
    pixmap.fill_path(path, &paint, FillRule::EvenOdd, Transform::identity(), None);
}

/// The layer's outline in its edge style. Dashes carry across (Score, Perforate); other tool lines draw
/// solid, since a raster at export resolution does not need the canvas's zoom-fixed waves and sheen.
fn stroke_edge(pixmap: &mut Pixmap, path: &tiny_skia::Path, edge: Edge) {
    let [r, g, b, a] = edge.colour.to_srgba_unmultiplied();
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    paint.set_color_rgba8(r, g, b, a);
    let mut stroke = Stroke { width: edge.width.max(0.5), ..Stroke::default() };
    if let Line::Dashed { dash, gap } = edge.line {
        stroke.dash = StrokeDash::new(vec![dash, gap], 0.0);
    }
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

fn build_path(contours: &[Contour], map: &impl Fn(Pt) -> Pt) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for c in contours {
        let (sx, sy) = map(c.start);
        pb.move_to(sx, sy);
        for seg in &c.segs {
            match *seg {
                Seg::Line(p) => {
                    let (x, y) = map(p);
                    pb.line_to(x, y);
                }
                Seg::Cubic(a, b, p) => {
                    let (ax, ay) = map(a);
                    let (bx, by) = map(b);
                    let (x, y) = map(p);
                    pb.cubic_to(ax, ay, bx, by, x, y);
                }
            }
        }
        if c.closed {
            pb.close();
        }
    }
    pb.finish()
}

/// The premultiplied pixmap turned back into the unmultiplied RGBA every encoder and test reads.
fn to_image(pixmap: &Pixmap) -> RgbaImage {
    RgbaImage::from_fn(pixmap.width(), pixmap.height(), |x, y| {
        let c = pixmap.pixel(x, y).map_or(tiny_skia::ColorU8::from_rgba(0, 0, 0, 0), |p| p.demultiply());
        image::Rgba([c.red(), c.green(), c.blue(), c.alpha()])
    })
}

/// The file bytes: a PNG with its alpha, or a JPEG at `opts.quality` over an opaque background.
pub fn encode(image: &RgbaImage, opts: &RasterOptions, jpeg: bool) -> Vec<u8> {
    if jpeg { encode_jpeg(image, opts.quality) } else { picture::png(image) }
}

fn encode_jpeg(image: &RgbaImage, quality: u8) -> Vec<u8> {
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut out), quality)
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .expect("JPEG encoding to memory cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;
    use crate::design::{Design, Operation, ShapeKind};
    use crate::export::{self, Scope};
    use crate::machine::MachineFamily;
    use crate::make_flow::MatSetup;

    fn square(x: f32, y: f32, s: f32) -> Contour {
        Contour { start: (x, y), segs: [(x + s, y), (x + s, y + s), (x, y + s)].map(Seg::Line).to_vec(), closed: true }
    }

    fn layer(operation: Operation, colour: Color32, contours: Vec<Contour>) -> Layer {
        Layer { name: "Layer".into(), group: Vec::new(), operation, colour, contours, picture: None }
    }

    fn sheet(size_in: Pt, layers: Vec<Layer>) -> Sheet {
        Sheet { name: "Sheet".into(), size_in, layers }
    }

    #[test]
    fn a_sheet_renders_the_design_plus_a_margin_each_side_at_the_dpi() {
        let opts = RasterOptions { margin_in: 0.25, dpi: 300, ..RasterOptions::default() };
        assert_eq!(dimensions((2.0, 3.0), &opts), (750, 1050), "2.5 x 3.5 in at 300 dpi");
        assert_eq!(dimensions((2.0, 3.0), &RasterOptions { dpi: 72, ..opts }), (180, 252), "at 72 dpi");
        let image = render(&sheet((2.0, 3.0), Vec::new()), &opts, &Pixels::new());
        assert_eq!(image.dimensions(), (750, 1050));
    }

    #[test]
    fn transparent_leaves_the_margin_clear_and_white_fills_every_pixel() {
        let design = sheet((2.0, 2.0), vec![layer(Operation::BasicCut, Color32::RED, vec![square(0.0, 0.0, 2.0)])]);
        let opts = RasterOptions { margin_in: 0.25, dpi: 100, background: Background::Transparent, ..RasterOptions::default() };
        let image = render(&design, &opts, &Pixels::new());
        assert_eq!(image.get_pixel(5, 5).0[3], 0, "the margin corner is transparent");
        let inside = image.get_pixel(125, 125).0;
        assert_eq!(inside, [255, 0, 0, 255], "the red square is opaque inside");

        let white = render(&design, &RasterOptions { background: Background::White, ..opts }, &Pixels::new());
        assert!(white.pixels().all(|p| p.0[3] == 255), "White leaves no transparent pixel");
    }

    #[test]
    fn show_as_made_draws_a_cut_as_an_outline_with_a_clear_interior() {
        let design = sheet((2.0, 2.0), vec![layer(Operation::BasicCut, Color32::RED, vec![square(0.0, 0.0, 2.0)])]);
        let opts = RasterOptions { margin_in: 0.25, dpi: 100, as_made: true, ..RasterOptions::default() };
        let image = render(&design, &opts, &Pixels::new());
        assert_eq!(image.get_pixel(125, 125).0[3], 0, "the interior is not filled");
        let edge: u32 = (100..=150).map(|y| u32::from(image.get_pixel(25, y).0[3])).sum();
        assert!(edge > 0, "the cut line is drawn down the left edge");
    }

    #[test]
    fn jpeg_quality_trades_size_for_fidelity_at_the_same_dimensions() {
        let design = sheet((2.0, 2.0), vec![layer(Operation::PrintThenCut, Color32::from_rgb(40, 120, 200), vec![square(0.0, 0.0, 2.0)])]);
        let opts = RasterOptions { margin_in: 0.1, dpi: 150, background: Background::White, ..RasterOptions::default() };
        let image = render(&design, &opts, &Pixels::new());
        let low = encode(&image, &RasterOptions { quality: 60, ..opts }, true);
        let high = encode(&image, &RasterOptions { quality: 100, ..opts }, true);
        assert!(low.len() < high.len(), "quality 60 ({}) is smaller than quality 100 ({})", low.len(), high.len());
        let decode = |bytes: &[u8]| image::load_from_memory(bytes).unwrap().into_rgba8().dimensions();
        assert_eq!(decode(&low), decode(&high), "both decode to the same size");
        assert_eq!(decode(&low), image.dimensions());
    }

    #[test]
    fn a_24_inch_mat_at_600_dpi_is_refused_with_its_megapixels() {
        let opts = RasterOptions { margin_in: 0.0, dpi: 600, ..RasterOptions::default() };
        let message = oversize((24.0, 24.0), &opts).expect("207 megapixels is over the limit");
        assert!(message.contains("207 megapixels"), "{message}");
        assert!(oversize((6.0, 6.0), &opts).is_none(), "a 6 in mat at 600 dpi (13 megapixels) is fine");
    }

    #[test]
    fn per_mat_scope_renders_one_image_per_mat_at_the_mat_size() {
        let mut design = Design::default();
        for (kind, fill, x) in [(ShapeKind::Square, Color32::RED, 1.0), (ShapeKind::Heart, Color32::BLUE, 4.0)] {
            let id = design.add_shape(kind);
            let s = design.shape_mut(id).unwrap();
            (s.x_in, s.y_in, s.fill) = (x, 1.0, fill);
        }
        let setup = MatSetup::preferred(MachineFamily::StudioPro);
        let sheets = export::sheets(&design, Scope::PerMat, &setup);
        assert_eq!(sheets.len(), 2, "the two colours pack onto two mats");
        let opts = RasterOptions { margin_in: 0.0, dpi: 72, background: Background::White, ..RasterOptions::default() };
        for s in &sheets {
            let image = render(s, &opts, &Pixels::new());
            assert_eq!(image.dimensions(), dimensions(s.size_in, &opts));
            assert_eq!(s.size_in, (setup.mat_size.width_in, setup.mat_size.height_in), "each image is a whole mat");
        }
    }
}
