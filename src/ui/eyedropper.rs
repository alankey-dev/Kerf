//! The eyedropper tool (spec 56): sampling a colour and style from the object under a point, and the
//! cursor that shows what is under the pointer. The tool itself lives in `canvas`; this module is the
//! pure sampling and the glyph.

use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, vec2};
use image::RgbaImage;

use crate::assets::Pixels;
use crate::clipboard::PasteStyle;
use crate::design::{Design, Operation, Pt, Shape, ShapeKind};
use crate::{effects, patterns};

use super::tokens::ACCENT_PRIMARY;

/// What the eyedropper lifted from one object. `style` is the clicked shape's operation, fill, pen and
/// pattern; `colour` is the colour actually under the point, which for an image or a pattern is the
/// sampled pixel, and otherwise the pen's colour for a Pen shape or the shape's fill.
pub struct Lift {
    pub style: PasteStyle,
    pub colour: Color32,
}

/// The tool's carried state: what was lifted, and the decoded pixels of the images under the canvas so a
/// colour can be read from a photo or a pattern.
#[derive(Default)]
pub struct State {
    pub lifted: Option<Lift>,
    pub pixels: Pixels,
}

/// The lift at `world`, or `None` when no shape sits there.
pub fn sample(design: &Design, pixels: &Pixels, world: Pt) -> Option<Lift> {
    let id = design.pick_at(world.0, world.1)?;
    let shape = design.shapes.iter().find(|s| s.id == id)?;
    let style = PasteStyle::from_shape(shape);
    let colour = sampled_pixel(pixels, shape, world).unwrap_or_else(|| shape_colour(shape));
    Some(Lift { style, colour })
}

/// The colour of a shape that carries no decoded pixels under the point.
fn shape_colour(shape: &Shape) -> Color32 {
    if shape.operation == Operation::Pen { shape.pen.rgb() } else { shape.fill }
}

/// The pixel under `world` for an image or a patterned shape, or `None` when the shape has no image, its
/// pixels are not loaded, or the point falls outside the image.
fn sampled_pixel(pixels: &Pixels, shape: &Shape, world: Pt) -> Option<Color32> {
    let local = effects::Placement::of(shape).to_local(world);
    match &shape.kind {
        ShapeKind::Bitmap { asset, .. } => {
            let img = pixels.get(asset)?;
            pixel(img, local.0 / shape.width_in, local.1 / shape.height_in)
        }
        _ => {
            let fill = shape.pattern.as_ref()?;
            let img = pixels.get(&fill.asset)?;
            let placed = patterns::placement(fill, (shape.width_in, shape.height_in));
            let (mut u, mut v) = patterns::uv(placed.corners, local)?;
            if placed.repeat {
                (u, v) = (u.rem_euclid(1.0), v.rem_euclid(1.0));
            }
            pixel(img, u, v)
        }
    }
}

/// The opaque colour at `(u, v)` in the unit square of `img`, or `None` when the point is outside it.
fn pixel(img: &RgbaImage, u: f32, v: f32) -> Option<Color32> {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return None;
    }
    let (w, h) = img.dimensions();
    let x = ((u * w as f32) as u32).min(w.saturating_sub(1));
    let y = ((v * h as f32) as u32).min(h.saturating_sub(1));
    let [r, g, b, _] = img.get_pixel(x, y).0;
    Some(Color32::from_rgb(r, g, b))
}

const CHIP: f32 = 18.0;

/// The dropper glyph and a swatch of the colour under the pointer. In colour-only mode the chip takes an
/// accent ring; in style mode it carries a second chip behind it, so the two modes read apart.
pub fn paint_cursor(painter: &Painter, pointer: Pos2, colour: Color32, colour_only: bool) {
    let ink = Color32::from_gray(40);
    let tip = pointer;
    let top = pointer + vec2(13.0, -13.0);
    painter.line_segment([tip, top], Stroke::new(2.0_f32, ink));
    painter.circle(top, 3.5, colour, Stroke::new(1.5_f32, ink));

    let chip = Rect::from_min_size(pointer + vec2(16.0, 4.0), vec2(CHIP, CHIP));
    if !colour_only {
        painter.rect_filled(chip.translate(vec2(4.0, 4.0)), 3.0, colour.gamma_multiply(0.5));
    }
    painter.rect_filled(chip, 3.0, colour);
    let border = if colour_only { Stroke::new(2.0_f32, ACCENT_PRIMARY) } else { Stroke::new(1.0_f32, ink) };
    painter.rect_stroke(chip, 3.0, border, StrokeKind::Outside);
}
