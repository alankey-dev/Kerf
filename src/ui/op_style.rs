//! How each operation looks on the canvas and the mat previews. `look` is the table from an operation
//! to a fill and an edge; `paint` turns a look and screen-space outlines into egui shapes. Dashes, waves
//! and tool-mark widths are screen points, so they read the same at every zoom. Every non-Pen stroke is
//! drawn at a fixed screen width; only the pen nib is physical and follows the zoom.

use std::collections::HashMap;
use std::sync::Arc;

use egui::cache::CacheTrait;
use egui::{Color32, Context, Mesh, Painter, Pos2, Shape, Stroke, Vec2, epaint::PathStroke, vec2};

use crate::assets::{AssetId, Texture, Textures, Wrap};
use crate::bridge::Matrix;
use crate::design::{Design, FoilWeight, Operation, POINTS_PER_INCH, Pt, Shape as Layer};
use crate::effects::{Ink, Lines, Paint, Piece, PieceId, Placement, image_corners};
use crate::pens::Pen;

/// Design shows each operation's own style; As made shows only what the machine will produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Design,
    AsMade,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Line {
    Solid,
    Dashed {
        dash: f32,
        gap: f32,
    },
    Wavy {
        amplitude: f32,
        wavelength: f32,
    },
    /// A stroke whose tone sweeps light and dark along the page, like foil catching the light.
    Foil,
    /// A wide dark stroke with a pale centre line, so the mark reads as pressed into the material.
    Groove,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub line: Line,
    pub width: f32,
    pub colour: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Option<Color32>,
    pub edge: Option<Edge>,
    /// A soft drop shadow under the piece: a Print Then Cut piece reads as a printed sticker.
    pub shadow: bool,
}

/// The dash pattern for a score line.
pub const SCORE_DASH: (f32, f32) = (20.0, 20.0);
/// The dash pattern for a basic perforation.
pub const PERF_DASH: (f32, f32) = (15.0, 5.0);
/// Kerf dashes guides so they never pass for a cut line.
pub const GUIDE_DASH: (f32, f32) = (4.0, 4.0);
pub const GUIDE_ALPHA: f32 = 0.6;
/// Kerf draws a wave for Wavy Cut so the edge reads as wavy.
pub const WAVE_AMPLITUDE: f32 = 2.0;
pub const WAVE_LENGTH: f32 = 8.0;
/// The stroke width for a fine deboss.
pub const DEBOSS_W: f32 = 3.0;
const OPEN_PATH_W: f32 = 1.5;
/// Most points one outline is resampled into for a wave or foil line, however far it is zoomed.
const MAX_SAMPLES: f32 = 4000.0;
/// Tool marks with no colour of their own (Score, Perforate, Deboss, Engrave) are drawn black.
const TOOL_INK: Color32 = super::tokens::TEXT_PRIMARY;
const GUIDE: Color32 = super::tokens::TEXT_MUTED;

/// Foil line widths by weight.
pub fn foil_width(weight: FoilWeight) -> f32 {
    match weight {
        FoilWeight::Fine => 1.0,
        FoilWeight::Medium => 2.0,
        FoilWeight::Bold => 3.0,
    }
}

/// `colour` is the layer's fill; `pen` and `px_per_in` matter only for Pen, whose nib is physical.
pub fn look(op: Operation, colour: Color32, pen: Pen, px_per_in: f32, mode: Mode) -> Look {
    let line = |line, width, colour| Look { fill: None, edge: Some(Edge { line, width, colour }), shadow: false };
    let dashed = |(dash, gap): (f32, f32)| Line::Dashed { dash, gap };
    let wavy = Line::Wavy { amplitude: WAVE_AMPLITUDE, wavelength: WAVE_LENGTH };
    match (op, mode) {
        (Operation::BasicCut, Mode::Design) => Look { fill: Some(colour), edge: None, shadow: false },
        (Operation::BasicCut, Mode::AsMade) => line(Line::Solid, 1.0, darken(colour)),
        (Operation::WavyCut, Mode::Design) => Look { fill: Some(colour), edge: Some(Edge { line: wavy, width: 1.0, colour: darken(colour) }), shadow: false },
        (Operation::WavyCut, Mode::AsMade) => line(wavy, 1.0, darken(colour)),
        (Operation::Perforate, _) => line(dashed(PERF_DASH), 1.0, TOOL_INK),
        (Operation::Pen, _) => line(Line::Solid, (pen.nib_in() * px_per_in).max(1.0), colour),
        (Operation::Foil(weight), _) => line(Line::Foil, foil_width(weight), colour),
        (Operation::Score, _) => line(dashed(SCORE_DASH), 1.0, TOOL_INK),
        (Operation::Deboss, _) => line(Line::Groove, DEBOSS_W, TOOL_INK),
        (Operation::Engrave, _) => line(Line::Solid, 1.0, TOOL_INK),
        (Operation::PrintThenCut, mode) => {
            Look { fill: Some(colour), edge: Some(Edge { line: Line::Solid, width: 1.0, colour: darken(colour) }), shadow: mode == Mode::Design }
        }
        (Operation::Guide, Mode::Design) => line(dashed(GUIDE_DASH), 1.0, GUIDE.gamma_multiply(GUIDE_ALPHA)),
        (Operation::Guide, Mode::AsMade) => Look { fill: None, edge: None, shadow: false },
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), a.a())
}

/// A darker tone of `c` for edges. Near-black moves towards grey instead, so it stays visible.
pub fn darken(c: Color32) -> Color32 {
    let luma = 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
    if luma < 48.0 { mix(c, Color32::from_gray(0x80), 0.5) } else { mix(c, Color32::BLACK, 0.35) }
}

/// The mat-preview stroke rule. Black on light fills, white on dark.
fn contrast(c: Color32) -> Color32 {
    let (r, g, b) = (f32::from(c.r()), f32::from(c.g()), f32::from(c.b()));
    if (0.299 * r * r + 0.587 * g * g + 0.114 * b * b).sqrt() > 127.5 { Color32::BLACK } else { Color32::WHITE }
}

/// One outline in screen points: its points and whether it closes.
pub type ScreenLine = (Vec<Pos2>, bool);

/// The shapes that draw `lines` (one shape's outlines, holes included) in `look`, back to front.
pub fn paint(ctx: &Context, look: &Look, lines: &[ScreenLine]) -> Vec<Shape> {
    let lines: Lines = lines.iter().map(|(pts, closed)| (pts.iter().map(|p| (p.x, p.y)).collect(), *closed)).collect();
    let view = Matrix::IDENTITY;
    [shadow(ctx, look, &lines, view), fill(ctx, look.fill, &lines, view), edge(look, &lines, view)].concat()
}

/// A placement's `transform`, which works in points, as a map from inches to inches.
pub fn in_inches(transform: Matrix) -> Matrix {
    Matrix::scale(POINTS_PER_INCH, POINTS_PER_INCH).then(transform).then(Matrix::scale(1.0 / POINTS_PER_INCH, 1.0 / POINTS_PER_INCH))
}

fn at(view: Matrix, p: Pt) -> Pos2 {
    let (x, y) = view.apply(p);
    Pos2::new(x, y)
}

/// `shape` on the canvas, through `view` from inches to screen points. Shapes wholly outside the painter's
/// clip are skipped, so a zoomed-in canvas pays only for what it shows.
pub fn paint_shape(painter: &Painter, design: &Design, shape: &Layer, view: Matrix, px_per_in: f32, mode: Mode) {
    let geometry = design.geometry(shape);
    let b = geometry.bounds();
    let corners = [(b.min_x, b.min_y), (b.max_x, b.min_y), (b.max_x, b.max_y), (b.min_x, b.max_y)].map(|p| at(view, p));
    if painter.clip_rect().intersects(egui::Rect::from_points(&corners).expand(8.0)) {
        for piece in geometry.pieces() {
            paint_piece(painter, shape, piece, view, px_per_in, mode, None);
        }
    }
}

/// One piece of a shape on a mat preview, where `view` maps the shape's own frame (inches, before its
/// rotation and flips) onto a mat of colour `mat`.
pub fn paint_placed(painter: &Painter, design: &Design, shape: &Layer, piece: PieceId, view: Matrix, px_per_in: f32, mat: Color32) {
    let local = Layer { x_in: 0.0, y_in: 0.0, rotation_deg: 0.0, flip_x: false, flip_y: false, ..shape.clone() };
    if let Some(piece) = design.geometry(&local).piece(piece) {
        paint_piece(painter, &local, piece, view, px_per_in, Mode::Design, Some(mat));
    }
}

/// A piece in its own style. Print Then Cut artwork fills each part in its own colour inside one shadow
/// and one cut line around the whole outline. On a mat, a fill or line too close to the mat colour gets
/// a black or white edge.
fn paint_piece(painter: &Painter, shape: &Layer, piece: &Piece, view: Matrix, px_per_in: f32, mode: Mode, mat: Option<Color32>) {
    let look_in = |colour| look(piece.style.operation, colour, piece.style.pen, px_per_in, mode);
    let mut whole = look_in(piece.style.fill);
    if let Some(mat) = mat {
        whole = on_mat(whole, mat);
    }
    let ctx = painter.ctx();
    painter.extend(shadow(ctx, &whole, &piece.lines, view));
    match &piece.paint {
        Paint::Print(layers) => {
            for (lines, ink) in layers {
                match ink {
                    Ink::Colour(colour) => painter.extend(fill(ctx, look_in(*colour).fill, lines, view)),
                    Ink::Image { asset, corners, repeat } if whole.fill.is_some() => {
                        painter.extend(image(painter, asset, lines, *corners, *repeat, view, px_per_in));
                    }
                    Ink::Image { .. } => {}
                }
            }
        }
        Paint::Fill => painter.extend(fill(ctx, whole.fill, &piece.lines, view)),
        Paint::Image(asset) if whole.fill.is_some() => {
            let corners = image_corners((shape.width_in, shape.height_in), Placement::of(shape).placer());
            painter.extend(image(painter, asset, &piece.lines, corners, false, view, px_per_in));
        }
        Paint::Image(_) => {}
    }
    painter.extend(edge(&whole, &piece.lines, view));
}

/// An image's texture clipped to the closed `lines` (world inches), its top-left, top-right and
/// bottom-left corners at `corners`, tiling past them when it repeats. While the texture loads only the
/// cut line shows; a missing asset is a grey hatched box labelled "Missing image".
fn image(painter: &Painter, asset: &AssetId, lines: &Lines, corners: [Pt; 3], repeat: bool, view: Matrix, px_per_in: f32) -> Vec<Shape> {
    let Some(textures) = Textures::of(painter.ctx()) else { return Vec::new() };
    let [tl, tr, bl] = corners;
    let (across, down) = ((tr.0 - tl.0, tr.1 - tl.1), (bl.0 - tl.0, bl.1 - tl.1));
    let (w, h) = (across.0.hypot(across.1), down.0.hypot(down.1));
    match textures.get(asset, if repeat { Wrap::Repeat } else { Wrap::Clamp }) {
        Texture::Loading => Vec::new(),
        Texture::Ready(texture) => {
            if crate::patterns::uv(corners, tl).is_none() {
                return Vec::new();
            }
            let fan = tessellation(painter.ctx(), lines);
            let mut mesh = Mesh { indices: fan.triangles.clone(), vertices: Vec::with_capacity(fan.points.len()), texture_id: texture.id() };
            mesh.vertices.extend(fan.points.iter().map(|&p| {
                let (u, v) = crate::patterns::uv(corners, p).unwrap_or_default();
                egui::epaint::Vertex { pos: at(view, p), uv: Pos2::new(u, v), color: Color32::WHITE }
            }));
            vec![Shape::mesh(mesh)]
        }
        Texture::Missing if repeat => fill(painter.ctx(), Some(MISSING_FILL), lines, view),
        Texture::Missing => {
            let place = |(x, y): Pt| (tl.0 + across.0 * x / w + down.0 * y / h, tl.1 + across.1 * x / w + down.1 * y / h);
            missing(painter, &place, view, (w, h), px_per_in)
        }
    }
}

const MISSING_FILL: Color32 = Color32::from_gray(0xd8);
const MISSING_HATCH: Color32 = Color32::from_gray(0x9a);
/// Hatch spacing in screen points.
const HATCH_STEP: f32 = 10.0;

/// The image's box in grey with diagonal hatching and its name, `place`d into inches and drawn through
/// `view`.
fn missing(painter: &Painter, place: &dyn Fn(Pt) -> Pt, view: Matrix, (w, h): Pt, px_per_in: f32) -> Vec<Shape> {
    let local_to_screen = |p| at(view, place(p));
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(place).to_vec();
    let mut out = fill(painter.ctx(), Some(MISSING_FILL), &vec![(corners, true)], view);
    let step = HATCH_STEP / px_per_in.max(1e-3);
    let mut c = -h + step / 2.0;
    while c < w {
        // The line x = y + c across the box.
        let (y0, y1) = ((-c).max(0.0), h.min(w - c));
        if y1 > y0 {
            out.push(Shape::line_segment([local_to_screen((y0 + c, y0)), local_to_screen((y1 + c, y1))], Stroke::new(1.0_f32, MISSING_HATCH)));
        }
        c += step;
    }
    let font = egui::FontId::proportional(13.0);
    let label = painter.layout_no_wrap("Missing image".into(), font, super::tokens::TEXT_PRIMARY);
    let at = local_to_screen((w / 2.0, h / 2.0)) - label.size() / 2.0;
    out.push(Shape::rect_filled(egui::Rect::from_min_size(at, label.size()).expand(4.0), 4.0, Color32::WHITE.gamma_multiply(0.85)));
    out.push(Shape::galley(at, label, super::tokens::TEXT_PRIMARY));
    out
}

/// The mat rule. Whatever would vanish into the mat is edged in black or white.
fn on_mat(mut look: Look, mat: Color32) -> Look {
    let near = |c: Color32| [c.r().abs_diff(mat.r()), c.g().abs_diff(mat.g()), c.b().abs_diff(mat.b())].into_iter().max() < Some(32);
    if look.fill.is_some_and(near) {
        look.edge = Some(Edge { line: look.edge.map_or(Line::Solid, |e| e.line), width: 1.0, colour: contrast(mat) });
    } else if let Some(e) = look.edge.as_mut().filter(|e| look.fill.is_none() && near(e.colour)) {
        e.colour = contrast(mat);
    }
    look
}

/// Two soft layers, each grown by a few screen points and dropped 2 points down.
fn shadow(ctx: &Context, look: &Look, lines: &Lines, view: Matrix) -> Vec<Shape> {
    if !look.shadow {
        return Vec::new();
    }
    let fan = tessellation(ctx, lines);
    let px_per_unit = (view.a * view.d - view.b * view.c).abs().sqrt().max(1e-6);
    [(3.0, 0.05), (1.5, 0.08)].into_iter().map(|(grow, alpha)| fan.mesh(view, Color32::BLACK.gamma_multiply(alpha), grow / px_per_unit, vec2(0.0, 2.0))).collect()
}

/// Closed rings fill as one mesh; open paths have no inside, so they draw as a line in the fill colour.
fn fill(ctx: &Context, fill: Option<Color32>, lines: &Lines, view: Matrix) -> Vec<Shape> {
    let Some(c) = fill else { return Vec::new() };
    let open = lines.iter().filter(|(_, closed)| !closed).map(|(pts, _)| Shape::line(pts.iter().map(|&p| at(view, p)).collect(), Stroke::new(OPEN_PATH_W, c)));
    std::iter::once(tessellation(ctx, lines).mesh(view, c, 0.0, Vec2::ZERO)).chain(open).collect()
}

fn edge(look: &Look, lines: &Lines, view: Matrix) -> Vec<Shape> {
    let Some(e) = look.edge else { return Vec::new() };
    lines.iter().flat_map(|(pts, closed)| stroke(&pts.iter().map(|&p| at(view, p)).collect::<Vec<_>>(), *closed, e)).collect()
}

fn stroke(pts: &[Pos2], closed: bool, e: Edge) -> Vec<Shape> {
    let path = |pts: Vec<Pos2>, stroke: Stroke| if closed { Shape::closed_line(pts, stroke) } else { Shape::line(pts, stroke) };
    let solid = Stroke::new(e.width, e.colour);
    match e.line {
        Line::Solid => vec![path(pts.to_vec(), solid)],
        Line::Dashed { dash, gap } => dashes(pts, closed, dash, gap).into_iter().map(|d| Shape::line(d, solid)).collect(),
        Line::Wavy { amplitude, wavelength } => vec![path(wave(pts, closed, amplitude, wavelength), solid)],
        Line::Groove => vec![path(pts.to_vec(), solid), path(pts.to_vec(), Stroke::new(1.0_f32, mix(e.colour, Color32::WHITE, 0.6)))],
        Line::Foil => {
            let colour = e.colour;
            let sheen = PathStroke::new_uv(e.width, move |_, p: Pos2| foil_tone(colour, p));
            let pts: Vec<Pos2> = resample(pts, closed, 3.0).into_iter().map(|(p, _, _)| p).collect();
            let path = if closed { egui::epaint::PathShape::closed_line(pts, sheen) } else { egui::epaint::PathShape::line(pts, sheen) };
            vec![Shape::Path(path)]
        }
    }
}

/// Bands of light and shade running diagonally across the page, so a foil line glints as it turns.
fn foil_tone(colour: Color32, p: Pos2) -> Color32 {
    let phase = ((p.x + p.y) / 18.0 * std::f32::consts::TAU).sin();
    if phase > 0.0 { mix(colour, Color32::WHITE, 0.75 * phase) } else { mix(colour, Color32::BLACK, -0.35 * phase) }
}

/// The closed rings of some lines as one even-odd fan of triangles, so holes read through, in the lines'
/// own units (inches for a piece). Each point carries its mitre: the step that moves it one unit
/// outwards, which grows a shadow without triangulating again.
struct Tessellation {
    points: Vec<Pt>,
    mitres: Vec<Vec2>,
    triangles: Vec<u32>,
}

impl Tessellation {
    fn of(lines: &Lines) -> Self {
        #[cfg(test)]
        TESSELLATIONS.with(|n| n.set(n.get() + 1));
        let (rings, mitres): (Vec<Vec<Pt>>, Vec<Vec<Vec2>>) = lines.iter().filter(|(pts, closed)| *closed && pts.len() > 2).map(|(pts, _)| mitred(pts)).unzip();
        let (sources, tris) = crate::geometry::triangulate_rings(&rings);
        Self {
            points: sources.iter().map(|&(r, k)| rings[r][k]).collect(),
            mitres: sources.iter().map(|&(r, k)| mitres[r][k]).collect(),
            triangles: tris.into_iter().flatten().map(|i| i as u32).collect(),
        }
    }

    /// The fan in `colour` through `view`, every point first moved `grow` units outwards, then shifted by
    /// `shift` screen points.
    fn mesh(&self, view: Matrix, colour: Color32, grow: f32, shift: Vec2) -> Shape {
        let vertices = self
            .points
            .iter()
            .zip(&self.mitres)
            .map(|(&(x, y), m)| egui::epaint::Vertex { pos: at(view, (x + m.x * grow, y + m.y * grow)) + shift, uv: egui::epaint::WHITE_UV, color: colour })
            .collect();
        Shape::mesh(Mesh { indices: self.triangles.clone(), vertices, ..Mesh::default() })
    }
}

/// Triangulating is the costly part of drawing a filled outline, so it happens once per outline in its
/// own units and every frame only moves the points onto the screen: zooming and panning reuse it.
fn tessellation(ctx: &Context, lines: &Lines) -> Arc<Tessellation> {
    ctx.memory_mut(|m| m.caches.cache::<Tessellations>().get(lines))
}

/// Points kept across every cached fan before the least recently drawn are dropped, about 14 MB. The
/// heavy benchmark design's canvas and mats hold under 40,000, so zooming in and back out finds every fan
/// still there.
const KEEP_POINTS: usize = 500_000;

/// Fans by a hash of the lines they fill, each with the pass that last drew it.
#[derive(Default)]
struct Tessellations {
    pass: u64,
    fans: HashMap<u64, (u64, Arc<Tessellation>)>,
}

impl Tessellations {
    fn get(&mut self, lines: &Lines) -> Arc<Tessellation> {
        let (used, fan) = self.fans.entry(egui::util::hash(LinesKey(lines))).or_insert_with(|| (0, Arc::new(Tessellation::of(lines))));
        *used = self.pass;
        Arc::clone(fan)
    }
}

impl CacheTrait for Tessellations {
    /// Past the budget, drops the fans drawn longest ago, never one the pass just ending drew. Fans left
    /// behind by a drag or a deleted shape go this way.
    fn update(&mut self) {
        let mut total: usize = self.fans.values().map(|(_, fan)| fan.points.len()).sum();
        if total > KEEP_POINTS {
            let mut stale: Vec<(u64, u64, usize)> =
                self.fans.iter().filter(|(_, (used, _))| *used < self.pass).map(|(&key, (used, fan))| (*used, key, fan.points.len())).collect();
            stale.sort_unstable();
            for (_, key, points) in stale {
                if total <= KEEP_POINTS {
                    break;
                }
                self.fans.remove(&key);
                total -= points;
            }
        }
        self.pass += 1;
    }

    fn len(&self) -> usize {
        self.fans.len()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct LinesKey<'a>(&'a Lines);

impl std::hash::Hash for LinesKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (pts, closed) in self.0 {
            (pts.len(), closed).hash(state);
            for &(x, y) in pts {
                (x.to_bits(), y.to_bits()).hash(state);
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    static TESSELLATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many fill meshes this thread has triangulated.
#[cfg(test)]
pub fn tessellations() -> usize {
    TESSELLATIONS.with(std::cell::Cell::get)
}

/// Splits a polyline into dashes of `dash` points separated by `gap` points, measured along the line.
pub fn dashes(pts: &[Pos2], closed: bool, dash: f32, gap: f32) -> Vec<Vec<Pos2>> {
    let mut path = pts.to_vec();
    if closed && let Some(&first) = pts.first() {
        path.push(first);
    }
    let mut out = Vec::new();
    let mut current = vec![];
    let (mut on, mut left) = (true, dash);
    for w in path.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut len = a.distance(b);
        while len > 0.0 {
            let step = left.min(len);
            let next = a + (b - a) * (step / len);
            if on {
                if current.is_empty() {
                    current.push(a);
                }
                current.push(next);
            }
            left -= step;
            len -= step;
            a = next;
            if left <= 1e-4 {
                if on {
                    out.push(std::mem::take(&mut current));
                }
                on = !on;
                left = if on { dash } else { gap };
            }
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// Points along the polyline at most `step` apart, each with its unit normal and distance travelled.
/// A closed ring does not repeat its first point at the end.
fn resample(pts: &[Pos2], closed: bool, step: f32) -> Vec<(Pos2, Vec2, f32)> {
    let mut path = pts.to_vec();
    if closed && let Some(&first) = pts.first() {
        path.push(first);
    }
    let total: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    let step = step.max(total / MAX_SAMPLES);
    let mut out = Vec::new();
    let mut travelled = 0.0;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        if len <= 0.0 {
            continue;
        }
        let normal = vec2(a.y - b.y, b.x - a.x) / len;
        let steps = (len / step).ceil() as usize;
        out.extend((0..steps).map(|i| i as f32 / steps as f32).map(|t| (a + (b - a) * t, normal, travelled + t * len)));
        travelled += len;
    }
    if !closed && let [.., before, last] = path[..] {
        out.push((last, vec2(before.y - last.y, last.x - before.x).normalized(), travelled));
    }
    out
}

/// The polyline pushed sideways by a sine wave of `amplitude`, sampled every point.
pub fn wave(pts: &[Pos2], closed: bool, amplitude: f32, wavelength: f32) -> Vec<Pos2> {
    let samples = resample(pts, closed, 1.0);
    let total: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>() + if closed && pts.len() > 1 { pts[pts.len() - 1].distance(pts[0]) } else { 0.0 };
    // A whole number of waves round a closed ring, so the seam joins up.
    let wavelength = if closed && total > 0.0 { total / (total / wavelength).round().max(1.0) } else { wavelength };
    samples.into_iter().map(|(p, normal, s)| p + normal * amplitude * (s / wavelength * std::f32::consts::TAU).sin()).collect()
}

/// The ring without repeated points, and for each point the step that moves it one unit outwards, the
/// corner mitred and capped at 4 units.
fn mitred(ring: &[Pt]) -> (Vec<Pt>, Vec<Vec2>) {
    let mut ring: Vec<Pos2> = ring.iter().map(|&(x, y)| Pos2::new(x, y)).collect();
    let near = |a: Pos2, b: Pos2| a.distance(b) < 1e-5;
    ring.dedup_by(|b, a| near(*a, *b));
    while ring.len() > 1 && near(ring[0], ring[ring.len() - 1]) {
        ring.pop();
    }
    let n = ring.len();
    let area: f32 = (0..n).map(|i| ring[i].x * ring[(i + 1) % n].y - ring[(i + 1) % n].x * ring[i].y).sum();
    // Outward normal for a ring of either winding (y points down on screen).
    let sign = if area > 0.0 { 1.0 } else { -1.0 };
    let normal = |a: Pos2, b: Pos2| {
        let v = (b - a).normalized();
        vec2(v.y, -v.x) * sign
    };
    let mitres = (0..n)
        .map(|i| {
            if n < 3 {
                return Vec2::ZERO;
            }
            let (prev, p, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            let (n1, n2) = (normal(prev, p), normal(p, next));
            let sum = n1 + n2;
            // A full reversal has no bisector; push along the incoming edge's normal instead.
            if sum.length_sq() < 1e-6 {
                return n1;
            }
            let bisector = sum.normalized();
            bisector / bisector.dot(n1).max(0.25)
        })
        .collect();
    (ring.into_iter().map(|p| (p.x, p.y)).collect(), mitres)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const RED: Color32 = Color32::from_rgb(0xd0, 0x30, 0x30);

    fn square(side: f32) -> Vec<ScreenLine> {
        vec![(vec![pos2(0.0, 0.0), pos2(side, 0.0), pos2(side, side), pos2(0.0, side)], true)]
    }

    fn at(op: Operation, px_per_in: f32) -> Look {
        look(op, RED, Pen::default(), px_per_in, Mode::Design)
    }

    fn has_fill(shapes: &[Shape]) -> bool {
        shapes.iter().any(|s| matches!(s, Shape::Mesh(_)))
    }

    #[test]
    fn score_and_perforate_dash_by_the_same_screen_lengths_at_every_zoom() {
        for zoom in [12.0, 96.0, 960.0] {
            assert_eq!(at(Operation::Score, zoom).edge.unwrap().line, Line::Dashed { dash: SCORE_DASH.0, gap: SCORE_DASH.1 });
            assert_eq!(at(Operation::Perforate, zoom).edge.unwrap().line, Line::Dashed { dash: PERF_DASH.0, gap: PERF_DASH.1 });
        }
    }

    #[test]
    fn dashes_measure_their_lengths_along_the_outline() {
        let d = dashes(&[pos2(0.0, 0.0), pos2(200.0, 0.0)], false, SCORE_DASH.0, SCORE_DASH.1);
        assert_eq!(d.len(), 5);
        for (i, dash) in d.iter().enumerate() {
            assert!((dash[0].x - i as f32 * 40.0).abs() < 1e-3, "dash {i} starts every dash plus gap");
            assert!((dash.last().unwrap().x - dash[0].x - SCORE_DASH.0).abs() < 1e-3, "dash {i} is {} long", SCORE_DASH.0);
        }
        let around = dashes(&square(30.0)[0].0, true, PERF_DASH.0, PERF_DASH.1);
        assert_eq!(around.len(), 6, "120 points of outline in 20 point steps");
        let corner = &around[1];
        assert_eq!(corner.len(), 3, "a dash turns the corner rather than breaking at it");
    }

    #[test]
    fn score_draws_dashes_and_no_fill() {
        let shapes = paint(&Context::default(), &at(Operation::Score, 96.0), &square(50.0));
        assert!(!has_fill(&shapes));
        assert!(shapes.len() > 4, "a closed square breaks into many dashes");
    }

    #[test]
    fn pen_draws_no_fill_and_strokes_at_the_nib_width() {
        let pen = Pen::default();
        let look = look(Operation::Pen, RED, pen, 96.0, Mode::Design);
        assert_eq!(look.fill, None);
        assert!((look.edge.unwrap().width - (pen.nib_in() * 96.0).max(1.0)).abs() < 1e-4);
        assert!(!has_fill(&paint(&Context::default(), &look, &square(50.0))));
    }

    #[test]
    fn guide_draws_no_fill_at_reduced_opacity() {
        let look = at(Operation::Guide, 96.0);
        assert_eq!(look.fill, None);
        let edge = look.edge.unwrap();
        assert!(edge.colour.a() < 255 && edge.colour.a() as f32 <= 255.0 * GUIDE_ALPHA + 1.0, "alpha {}", edge.colour.a());
        assert!(matches!(edge.line, Line::Dashed { .. }));
        assert!(!has_fill(&paint(&Context::default(), &look, &square(50.0))));
    }

    #[test]
    fn a_wavy_edge_strays_from_the_outline_by_the_amplitude() {
        let pts = wave(&[pos2(0.0, 0.0), pos2(80.0, 0.0)], false, WAVE_AMPLITUDE, WAVE_LENGTH);
        let most = pts.iter().map(|p| p.y.abs()).fold(0.0, f32::max);
        assert!((most - WAVE_AMPLITUDE).abs() < 0.05, "peaks at {most}");
        assert!(pts.iter().all(|p| p.y.abs() <= WAVE_AMPLITUDE + 1e-3));
        let Line::Wavy { amplitude, .. } = at(Operation::WavyCut, 400.0).edge.unwrap().line else { panic!("wavy cut has a wavy edge") };
        assert_eq!(amplitude, WAVE_AMPLITUDE, "the same in screen points at any zoom");
    }

    #[test]
    fn a_wavy_ring_closes_on_itself() {
        let ring = wave(&square(37.0)[0].0, true, WAVE_AMPLITUDE, WAVE_LENGTH);
        assert!(ring[0].distance(pos2(0.0, 0.0)) < 1e-3, "starts on the outline, so the seam meets");
    }

    #[test]
    fn as_made_draws_a_basic_cut_as_its_outline_only() {
        let look = look(Operation::BasicCut, RED, Pen::default(), 96.0, Mode::AsMade);
        assert_eq!(look.fill, None);
        assert!(!has_fill(&paint(&Context::default(), &look, &square(50.0))));
        assert!(has_fill(&paint(&Context::default(), &at(Operation::BasicCut, 96.0), &square(50.0))));
    }

    #[test]
    fn as_made_hides_guides() {
        assert!(paint(&Context::default(), &look(Operation::Guide, RED, Pen::default(), 96.0, Mode::AsMade), &square(50.0)).is_empty());
    }

    fn offset_ring(ring: &[Pos2], d: f32) -> Vec<Pos2> {
        let (points, mitres) = mitred(&ring.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>());
        points.into_iter().zip(mitres).map(|((x, y), m)| pos2(x, y) + m * d).collect()
    }

    #[test]
    fn shadows_grow_outwards_whatever_the_winding() {
        let ring = square(50.0)[0].0.clone();
        assert!(offset_ring(&ring, 6.0)[0].distance(pos2(-6.0, -6.0)) < 1e-3);
        let mut reversed = ring;
        reversed.reverse();
        assert!(offset_ring(&reversed, 6.0).iter().all(|p| p.x < 0.0 || p.x > 50.0 || p.y < 0.0 || p.y > 50.0));
    }

    #[test]
    fn print_then_cut_is_a_filled_piece_with_a_cut_line_and_a_shadow_until_made() {
        let design = at(Operation::PrintThenCut, 96.0);
        assert_eq!(design.fill, Some(RED));
        assert!(design.shadow && design.edge.is_some());
        assert!(!look(Operation::PrintThenCut, RED, Pen::default(), 96.0, Mode::AsMade).shadow);
    }

    #[test]
    fn foil_and_deboss_keep_design_space_widths_at_every_zoom() {
        for zoom in [12.0, 960.0] {
            assert_eq!(at(Operation::Foil(FoilWeight::Fine), zoom).edge.unwrap().width, 1.0);
            assert_eq!(at(Operation::Foil(FoilWeight::Bold), zoom).edge.unwrap().width, 3.0);
            assert_eq!(at(Operation::Deboss, zoom).edge.unwrap().width, DEBOSS_W);
        }
    }

    #[test]
    fn the_mat_outline_is_black_on_light_fills_and_white_on_dark() {
        assert_eq!(contrast(Color32::from_gray(0xf0)), Color32::BLACK);
        assert_eq!(contrast(Color32::from_rgb(0x20, 0x20, 0x60)), Color32::WHITE);
    }

    #[test]
    fn whatever_would_vanish_into_the_mat_gets_an_edge() {
        let pale_mat = Color32::from_rgb(0xe8, 0xf0, 0xf7);
        let white_cut = on_mat(look(Operation::BasicCut, Color32::WHITE, Pen::default(), 96.0, Mode::Design), pale_mat);
        assert_eq!(white_cut.edge.map(|e| e.colour), Some(Color32::BLACK), "a white cut on a pale mat");
        let white_pen = on_mat(look(Operation::Pen, Color32::WHITE, Pen::default(), 96.0, Mode::Design), Color32::WHITE);
        assert_eq!(white_pen.edge.map(|e| e.colour), Some(Color32::BLACK), "a white pen on a white mat");
        let red_cut = on_mat(at(Operation::BasicCut, 96.0), pale_mat);
        assert_eq!(red_cut.edge, None, "a contrasting fill needs no edge");
        let wavy = on_mat(look(Operation::WavyCut, pale_mat, Pen::default(), 96.0, Mode::Design), pale_mat);
        assert!(matches!(wavy.edge.unwrap().line, Line::Wavy { .. }), "the edge keeps its style");
    }

    #[test]
    fn a_repeated_point_does_not_spike_the_shadow() {
        let ring = vec![pos2(0.0, 0.0), pos2(50.0, 0.0), pos2(50.0, 0.0), pos2(50.0, 50.0), pos2(0.0, 50.0), pos2(0.0, 0.0)];
        let out = offset_ring(&ring, 3.0);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|p| p.x >= -3.01 && p.x <= 53.01 && p.y >= -3.01 && p.y <= 53.01), "{out:?}");
    }

    #[test]
    fn a_shadow_grows_by_the_same_screen_points_at_every_zoom_and_turn() {
        let inch: Lines = vec![(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], true)];
        let ptc = at(Operation::PrintThenCut, 96.0);
        for view in [Matrix::scale(40.0, 40.0), Matrix::rotate(90.0).then(Matrix::scale(300.0, -300.0)).then(Matrix::translate(500.0, 20.0))] {
            let ctx = Context::default();
            let Shape::Mesh(outer) = &shadow(&ctx, &ptc, &inch, view)[0] else { panic!("a shadow is a mesh") };
            let Shape::Mesh(fill) = &fill(&ctx, Some(RED), &inch, view)[0] else { panic!("a fill is a mesh") };
            let (grown, plain) = (outer.calc_bounds(), fill.calc_bounds());
            assert!((grown.width() - plain.width() - 6.0).abs() < 1e-2, "3 points each side: {grown:?} around {plain:?}");
            assert!((grown.center() - plain.center() - vec2(0.0, 2.0)).length() < 1e-2, "dropped 2 points: {grown:?} around {plain:?}");
        }
    }

    #[test]
    fn past_the_budget_the_fans_drawn_longest_ago_go_first() {
        let mut cache = Tessellations::default();
        let fan = || Arc::new(Tessellation { points: vec![(0.0, 0.0); KEEP_POINTS / 2], mitres: Vec::new(), triangles: Vec::new() });
        for key in 0..3 {
            cache.fans.insert(key, (cache.pass, fan()));
            cache.update();
        }
        cache.fans.insert(3, (cache.pass, fan()));
        cache.fans.insert(4, (cache.pass, fan()));
        cache.fans.insert(5, (cache.pass, fan()));
        cache.update();
        let mut kept: Vec<u64> = cache.fans.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [3, 4, 5], "older fans go, but never one the last pass drew, even past the budget");
    }

    #[test]
    fn every_operation_has_a_look_in_both_modes() {
        use crate::design::FoilWeight::*;
        let ops = [
            Operation::BasicCut,
            Operation::WavyCut,
            Operation::Perforate,
            Operation::Pen,
            Operation::Foil(Fine),
            Operation::Foil(Medium),
            Operation::Foil(Bold),
            Operation::Score,
            Operation::Deboss,
            Operation::Engrave,
            Operation::PrintThenCut,
            Operation::Guide,
        ];
        for op in ops {
            for mode in [Mode::Design, Mode::AsMade] {
                let shapes = paint(&Context::default(), &look(op, RED, Pen::default(), 96.0, mode), &square(50.0));
                assert_eq!(shapes.is_empty(), op == Operation::Guide && mode == Mode::AsMade, "{op:?} {mode:?}");
            }
        }
    }
}
