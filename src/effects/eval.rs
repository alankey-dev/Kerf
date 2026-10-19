//! Evaluation: a shape's source and effect stack in, styled pieces out. Pure; `cache.rs` memoises it.
//!
//! Evaluation runs in two levels. [`evaluate_local`] works in the shape's local frame and depends only on
//! what the shape is (its cache key); [`place`] then puts those pieces on the canvas, so
//! moving, turning or flipping a shape never re-runs outline, text layout, warp or offset.

use egui::Color32;

use super::model::{Direction, Effect, EffectId, Offset, Sticker, Style};
use crate::design::{Contour, Operation, Pt, Seg, Shape, ShapeKind};
use crate::geometry::{Boolean, FLATTEN_TOL_IN, Ring};
use crate::snap::Aabb;

/// World polylines with their closed flags.
pub type Lines = Vec<(Vec<Pt>, bool)>;

/// Names a piece across evaluations: the shape that produced it and which of its pieces it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PieceId {
    pub owner: u64,
    pub local: Local,
}

impl PieceId {
    /// What the bridge appends to the shape's path and image ids: nothing for the body, so a plain shape's
    /// payload is unchanged, `-e{id}` for an effect's piece and `-s{n}` for a Slice piece.
    pub fn suffix(&self) -> String {
        match self.local {
            Local::Body => String::new(),
            Local::Effect(id) => format!("-e{id}"),
            Local::Slice(n) => format!("-s{n}"),
            Local::Part { part, inner } => format!("-p{part}{}", inner.suffix()),
        }
    }
}

impl PartLocal {
    fn suffix(self) -> String {
        match self {
            PartLocal::Body => String::new(),
            PartLocal::Effect(id) => format!("-e{id}"),
            PartLocal::Slice(n) => format!("-s{n}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Local {
    /// The shape's own outline, after its leading Warp and contour hiding.
    Body,
    /// The piece an Offset or Sticker entry made.
    Effect(EffectId),
    /// A sliced boolean group's pieces: back minus front, front minus back, then their overlap.
    Slice(u8),
    /// A piece an instance draws: what its part `part` (an id `components::part_id` derived) made. A
    /// nested instance's pieces keep the innermost part, whose id is unique to that nesting.
    Part { part: u64, inner: PartLocal },
}

/// Which of a part's pieces a [`Local::Part`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PartLocal {
    Body,
    Effect(EffectId),
    Slice(u8),
}

impl Local {
    /// This piece as part `part` of an instance made it; a nested instance's piece already names its part.
    pub(super) fn of_part(self, part: u64) -> Local {
        let inner = match self {
            Local::Part { .. } => return self,
            Local::Body => PartLocal::Body,
            Local::Effect(id) => PartLocal::Effect(id),
            Local::Slice(n) => PartLocal::Slice(n),
        };
        Local::Part { part, inner }
    }
}

/// What the canvas fills inside a piece's cut line.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// The piece's own fill.
    Fill,
    /// Print Then Cut artwork (Sticker, Flattened): world lines with their own inks, back to front. In a
    /// `LocalPiece` the lines are still in the local frame.
    Print(Vec<(Lines, Ink)>),
    /// An image (Bitmap): its texture, mapped onto the shape's box, shows through the cut line.
    Image(crate::assets::AssetId),
}

/// What fills one layer of Print Then Cut artwork.
#[derive(Clone, Debug, PartialEq)]
pub enum Ink {
    Colour(Color32),
    /// An image's pixels, whose top-left, top-right and bottom-left corners land at `corners`: a flattened
    /// image part, or a pattern fill. With `repeat` the image tiles the plane past its corners.
    Image {
        asset: crate::assets::AssetId,
        corners: [Pt; 3],
        repeat: bool,
    },
}

impl Ink {
    /// The ink with its image corners moved by `f`. Exact for an affine `f`; a Warp bends only the corners.
    pub fn mapped(&self, f: impl Fn(Pt) -> Pt) -> Ink {
        match self {
            Ink::Colour(c) => Ink::Colour(*c),
            Ink::Image { asset, corners, repeat } => Ink::Image { asset: asset.clone(), corners: corners.map(f), repeat: *repeat },
        }
    }
}

/// Where a `w` by `h` box's top-left, top-right and bottom-left corners land through `place`: an image's
/// corners from its shape's local frame.
pub fn image_corners((w, h): Pt, place: impl Fn(Pt) -> Pt) -> [Pt; 3] {
    [(0.0, 0.0), (w, 0.0), (0.0, h)].map(place)
}

/// Where a shape's local frame sits on the canvas: flip, skew, then rotate about the centre. The skew
/// shear `[[1, tan x], [tan y, 1]]` sits between scale and rotation, so an outline needs no change
/// downstream: `geometry`, Combine, Offset, Make and export all read it through this one matrix (spec 52).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub centre: Pt,
    pub half: Pt,
    pub rotation_deg: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    /// Skew in degrees along the local X and Y axes.
    pub skew_deg: (f32, f32),
}

impl Placement {
    pub fn of(s: &Shape) -> Placement {
        Placement {
            centre: s.centre_in(),
            half: (s.width_in / 2.0, s.height_in / 2.0),
            rotation_deg: s.rotation_deg,
            flip_x: s.flip_x,
            flip_y: s.flip_y,
            skew_deg: s.skew_deg,
        }
    }

    /// The shear's `(tan x, tan y)`.
    fn shear(self) -> (f32, f32) {
        (self.skew_deg.0.to_radians().tan(), self.skew_deg.1.to_radians().tan())
    }

    /// Local to world.
    pub fn placer(&self) -> impl Fn(Pt) -> Pt + use<> {
        let (cx, cy) = self.centre;
        let (sin, cos) = self.rotation_deg.to_radians().sin_cos();
        let (hw, hh) = self.half;
        let (fx, fy) = (self.flip_x, self.flip_y);
        let (tx, ty) = self.shear();
        move |(x, y): Pt| {
            let (mut lx, mut ly) = (x - hw, y - hh);
            if fx {
                lx = -lx;
            }
            if fy {
                ly = -ly;
            }
            let (sx, sy) = (lx + ly * tx, ly + lx * ty);
            (cx + sx * cos - sy * sin, cy + sx * sin + sy * cos)
        }
    }

    /// World to local, the inverse of [`Self::placer`].
    pub fn to_local(self, (x, y): Pt) -> Pt {
        let (cx, cy) = self.centre;
        let (sin, cos) = self.rotation_deg.to_radians().sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        let (sx, sy) = (dx * cos + dy * sin, -dx * sin + dy * cos);
        let (tx, ty) = self.shear();
        let det = 1.0 - tx * ty;
        let (mut lx, mut ly) = if det.abs() < 1e-6 { (sx, sy) } else { ((sx - sy * tx) / det, (sy - sx * ty) / det) };
        if self.flip_x {
            lx = -lx;
        }
        if self.flip_y {
            ly = -ly;
        }
        (lx + self.half.0, ly + self.half.1)
    }
}

/// A piece in its shape's local frame, before placement.
#[derive(Debug, PartialEq)]
pub(crate) struct LocalPiece {
    /// The shape that made it, when that is not the shape being placed: a Compound's child.
    pub(super) owner: Option<u64>,
    pub(super) local: Local,
    pub(super) style: Style,
    pub(super) contours: Vec<Contour>,
    pub(super) paint: Paint,
}

/// Everything about a shape that does not depend on where it sits.
#[derive(Debug, PartialEq)]
pub(crate) struct LocalPieces {
    pub(super) pieces: Vec<LocalPiece>,
    /// Text is grabbed anywhere inside its box.
    pub(super) grab: bool,
}

/// One cuttable, styled piece of an evaluated shape.
#[derive(Debug, PartialEq)]
pub struct Piece {
    pub id: PieceId,
    pub style: Style,
    /// In the placement's local frame, beziers kept: what Make sends and export writes.
    pub contours: Vec<Contour>,
    /// World, flattened: what the canvas paints and hit-tests.
    pub lines: Lines,
    pub paint: Paint,
    /// World bounds of `lines`.
    pub bounds: Aabb,
}

/// A placed shape: its pieces back to front.
#[derive(Debug, PartialEq)]
pub struct Geometry {
    pieces: Vec<Piece>,
    placement: Placement,
    grab: bool,
}

const LINE_HIT_IN: f32 = 0.15;
/// An Offset closer to zero than this adds nothing, rather than a copy of the body cut twice.
const MIN_OFFSET_IN: f32 = 1e-4;

impl Geometry {
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn piece(&self, id: PieceId) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.id == id)
    }

    /// World bounds of every piece; [`Aabb::EMPTY`] when there are none.
    pub fn bounds(&self) -> Aabb {
        self.pieces.iter().fold(Aabb::EMPTY, |acc, p| acc.union(p.bounds))
    }

    /// Every piece's world lines, back to front.
    pub fn lines(&self) -> impl Iterator<Item = &(Vec<Pt>, bool)> {
        self.pieces.iter().flat_map(|p| &p.lines)
    }

    /// Text is grabbed anywhere inside its box; glyph outlines alone leave most of a
    /// word unclickable. Everything hits its closed outlines, or an open line within reach.
    pub fn contains(&self, (px, py): Pt) -> bool {
        let in_box = || {
            let (cx, cy) = self.placement.centre;
            let (sin, cos) = self.placement.rotation_deg.to_radians().sin_cos();
            let (dx, dy) = (px - cx, py - cy);
            let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            lx.abs() <= self.placement.half.0 && ly.abs() <= self.placement.half.1
        };
        (self.grab && in_box())
            || self.lines().any(|(pts, closed)| match closed {
                true => crate::geometry::contains(pts, (px, py)),
                false => pts.windows(2).any(|w| distance_to_segment((px, py), w[0], w[1]) < LINE_HIT_IN),
            })
    }

    /// World closed rings of every piece, curves flattened to `tol`.
    pub fn rings(&self, tol: f32) -> Vec<Ring> {
        self.pieces.iter().flat_map(|p| self.piece_rings(p, tol)).collect()
    }

    /// World closed rings of one piece, curves flattened to `tol`.
    pub fn piece_rings(&self, piece: &Piece, tol: f32) -> Vec<Ring> {
        let place = self.placement.placer();
        piece.contours.iter().filter(|c| c.closed).map(|c| crate::geometry::flatten(c, tol).into_iter().map(&place).collect()).collect()
    }

    /// `piece`'s contours moved to the canvas, beziers kept.
    pub fn world_contours(&self, piece: &Piece) -> Vec<Contour> {
        let place = self.placement.placer();
        piece
            .contours
            .iter()
            .map(|c| Contour {
                start: place(c.start),
                segs: c
                    .segs
                    .iter()
                    .map(|s| match *s {
                        Seg::Line(p) => Seg::Line(place(p)),
                        Seg::Cubic(a, b, p) => Seg::Cubic(place(a), place(b), place(p)),
                    })
                    .collect(),
                closed: c.closed,
            })
            .collect()
    }

    /// Every piece's local contours, back to front.
    #[cfg(test)]
    pub fn contours(&self) -> impl Iterator<Item = &Contour> {
        self.pieces.iter().flat_map(|p| &p.contours)
    }
}

/// The body contours before hiding, through the leading Warp: what the Contour panel lists and indexes,
/// and node editing reads. The nodes stay sharp; live corners are drawn only in [`evaluated_contours`].
pub fn body_contours(shape: &Shape) -> Vec<Contour> {
    through_warp(shape, shape.kind.outline(shape.width_in, shape.height_in))
}

/// The geometry every operation evaluates: the body outline with its live corners (spec 55) drawn, then
/// the leading Warp. Corners are cut before the warp, so it bends the rounded outline, matching how
/// Offset, Combine, Make and export all see one evaluated shape.
fn evaluated_contours(shape: &Shape) -> Vec<Contour> {
    let rounded = crate::corners::round_outline(shape, shape.kind.outline(shape.width_in, shape.height_in));
    through_warp(shape, rounded)
}

fn through_warp(shape: &Shape, base: Vec<Contour>) -> Vec<Contour> {
    match shape.effects.leading_warp() {
        Some(warp) => crate::warp::apply(warp, &base, (shape.width_in, shape.height_in)),
        None => base,
    }
}

pub(crate) fn evaluate_local(shape: &Shape) -> LocalPieces {
    let hidden = |i: usize| shape.contours.active().get(i).copied().unwrap_or(false);
    let contours: Vec<Contour> = evaluated_contours(shape).into_iter().enumerate().filter(|(i, _)| !hidden(*i)).map(|(_, c)| c).collect();
    let paint = match (&shape.kind, &shape.pattern) {
        (ShapeKind::Flattened { parts, view_box }, _) => Paint::Print(print_layers(parts, *view_box, (shape.width_in, shape.height_in))),
        (ShapeKind::Bitmap { asset, .. }, _) => Paint::Image(asset.clone()),
        (_, Some(fill)) if shape.takes_pattern() => pattern_paint(fill, &contours, (shape.width_in, shape.height_in)),
        _ => Paint::Fill,
    };
    let style = Style::of(shape);
    let mut pieces = if contours.is_empty() { Vec::new() } else { vec![LocalPiece { owner: None, local: Local::Body, style, contours, paint }] };
    run_stack(&mut pieces, shape, shape.effects.after_body());
    LocalPieces { pieces, grab: matches!(shape.kind, ShapeKind::Text { .. }) }
}

/// A pattern fill as print artwork. A tile paints the whole outline; an image drawn once paints only where
/// it overlaps the outline, so every consumer finds its pixels inside their own corners.
fn pattern_paint(fill: &crate::patterns::PatternFill, contours: &[Contour], size: Pt) -> Paint {
    let placed = crate::patterns::placement(fill, size);
    let lines = match placed.repeat {
        true => lines_of(contours),
        false => {
            let [tl, tr, bl] = placed.corners;
            let image = vec![tl, tr, (tr.0 + bl.0 - tl.0, tr.1 + bl.1 - tl.1), bl];
            let outline = contours.iter().filter(|c| c.closed).map(|c| crate::geometry::flatten(c, FLATTEN_TOL_IN)).collect();
            crate::geometry::boolean(&[outline, vec![image]], Boolean::Intersect).into_iter().filter(|r| r.len() >= 3).map(|r| (r, true)).collect()
        }
    };
    Paint::Print(vec![(lines, Ink::Image { asset: fill.asset.clone(), corners: placed.corners, repeat: placed.repeat })])
}

/// Which parts an inset is too thin for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TooThin {
    /// Some parts vanish; the rest are inset.
    Some,
    /// Every part vanishes, so the entry adds nothing.
    All,
}

/// What Offset `o` drops of `input`, the pieces evaluated before it.
pub(super) fn too_thin(input: &[LocalPiece], o: Offset) -> Option<TooThin> {
    if o.direction == Direction::Outset || o.distance_in < MIN_OFFSET_IN {
        return None;
    }
    let out = crate::geometry::offset_regions(&welded(input), o.signed_in(), o.join);
    match out.dropped {
        0 => None,
        _ if out.all_dropped() => Some(TooThin::All),
        _ => Some(TooThin::Some),
    }
}

/// Runs `entries` of `shape`'s stack over `pieces`, which start in the shape's box.
pub(super) fn run_stack<'a>(pieces: &mut Vec<LocalPiece>, shape: &Shape, entries: impl Iterator<Item = &'a super::model::Entry>) {
    let style = Style::of(shape);
    let mut frame = Aabb { min_x: 0.0, min_y: 0.0, max_x: shape.width_in, max_y: shape.height_in };
    for entry in entries {
        let local = Local::Effect(entry.id);
        match entry.effect {
            Effect::Warp(warp) => bend(pieces, warp, frame),
            Effect::Offset(o) => {
                if let Some(piece) = offset_piece(pieces, o, local, style) {
                    frame = frame.union(local_bounds(&piece.contours));
                    // An inset lies inside what it insets, so behind it could never be seen or picked.
                    match o.direction {
                        Direction::Outset => pieces.insert(0, piece),
                        Direction::Inset => pieces.push(piece),
                    }
                }
            }
            Effect::Sticker(s) => {
                if let Some(piece) = sticker_piece(pieces, s, local, style) {
                    frame = frame.union(local_bounds(&piece.contours));
                    *pieces = vec![piece];
                }
            }
        }
    }
}

/// The weld of every piece's closed rings, each piece its own operand so rings of opposite winding in
/// different pieces cannot cancel. One piece is passed through as it is.
pub(super) fn welded<'a>(pieces: impl IntoIterator<Item = &'a LocalPiece>) -> Vec<Ring> {
    let mut groups: Vec<Vec<Ring>> =
        pieces.into_iter().map(|p| p.contours.iter().filter(|c| c.closed).map(|c| crate::geometry::flatten(c, FLATTEN_TOL_IN)).collect()).collect();
    groups.retain(|g: &Vec<Ring>| !g.is_empty());
    match groups.len() {
        0 => Vec::new(),
        1 => groups.pop().unwrap_or_default(),
        _ => crate::geometry::boolean(&groups, Boolean::Weld),
    }
}

pub(super) fn ring_contours(rings: Vec<Ring>) -> Vec<Contour> {
    rings.into_iter().filter(|r| r.len() >= 3).map(|r| Contour { start: r[0], segs: r[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true }).collect()
}

fn offset_piece(pieces: &[LocalPiece], o: Offset, local: Local, style: Style) -> Option<LocalPiece> {
    if o.distance_in < MIN_OFFSET_IN {
        return None;
    }
    let input = welded(pieces);
    let contours = ring_contours(crate::geometry::offset(&input, o.signed_in(), o.join));
    (!contours.is_empty()).then(|| LocalPiece { owner: None, local, style: o.style.unwrap_or(style), contours, paint: Paint::Fill })
}

fn sticker_piece(pieces: &[LocalPiece], s: Sticker, local: Local, style: Style) -> Option<LocalPiece> {
    let backing = ring_contours(crate::geometry::offset(&welded(pieces), s.border_in, s.corners.join()));
    if backing.is_empty() {
        return None;
    }
    let mut print = vec![(lines_of(&backing), Ink::Colour(Color32::WHITE))];
    for p in pieces {
        match &p.paint {
            Paint::Print(layers) => print.extend(layers.iter().cloned()),
            Paint::Fill | Paint::Image(_) => print.push((lines_of(&p.contours), Ink::Colour(p.style.fill))),
        }
    }
    Some(LocalPiece { owner: None, local, style: Style { operation: Operation::PrintThenCut, ..style }, contours: backing, paint: Paint::Print(print) })
}

/// A Warp after an Offset or Sticker: every piece, artwork included, bent together inside `frame`.
fn bend(pieces: &mut [LocalPiece], warp: crate::warp::Warp, frame: Aabb) {
    let all: Vec<Contour> = pieces.iter().flat_map(|p| p.contours.iter().cloned()).collect();
    let size = (frame.max_x - frame.min_x, frame.max_y - frame.min_y);
    let bend = crate::warp::Bend::fit(warp, &all, (frame.min_x, frame.min_y), size);
    for p in pieces {
        p.contours = bend.contours(&p.contours);
        if let Paint::Print(layers) = &mut p.paint {
            for (lines, ink) in layers {
                for (pts, _) in lines {
                    pts.iter_mut().for_each(|q| *q = bend.point(*q));
                }
                *ink = ink.mapped(|q| bend.point(q));
            }
        }
    }
}

pub(super) fn lines_of(contours: &[Contour]) -> Lines {
    contours.iter().map(|c| (c.flatten(), c.closed)).collect()
}

fn local_bounds(contours: &[Contour]) -> Aabb {
    contours.iter().flat_map(Contour::flatten).fold(Aabb::EMPTY, |b, (x, y)| Aabb {
        min_x: b.min_x.min(x),
        min_y: b.min_y.min(y),
        max_x: b.max_x.max(x),
        max_y: b.max_y.max(y),
    })
}

/// Each visible part of a Flattened shape in its own fill, scaled from `view_box` into a `size` box.
fn print_layers(parts: &[Shape], view_box: Pt, size: Pt) -> Vec<(Lines, Ink)> {
    let (sx, sy) = crate::flatten::scale(view_box, size);
    let scaled = |(x, y): Pt| (x * sx, y * sy);
    let scale = |lines: Lines| lines.into_iter().map(|(pts, closed)| (pts.into_iter().map(scaled).collect(), closed)).collect();
    parts.iter().filter(|p| !p.hidden).flat_map(crate::flatten::paint_layers).map(|(lines, ink)| (scale(lines), ink.mapped(scaled))).collect()
}

pub(crate) fn place(local: &LocalPieces, owner: u64, placement: Placement) -> Geometry {
    let place = placement.placer();
    let lines = |contours: &[Contour]| -> Lines { contours.iter().map(|c| (c.flatten().into_iter().map(&place).collect(), c.closed)).collect() };
    let placed = |layers: &[(Lines, Ink)]| {
        layers.iter().map(|(l, ink)| (l.iter().map(|(pts, closed)| (pts.iter().map(|&p| place(p)).collect(), *closed)).collect(), ink.mapped(&place))).collect()
    };
    let pieces = local
        .pieces
        .iter()
        .map(|p| {
            let lines = lines(&p.contours);
            let bounds = lines.iter().flat_map(|(pts, _)| pts).fold(Aabb::EMPTY, |b, &(x, y)| Aabb {
                min_x: b.min_x.min(x),
                min_y: b.min_y.min(y),
                max_x: b.max_x.max(x),
                max_y: b.max_y.max(y),
            });
            let paint = match &p.paint {
                Paint::Print(layers) => Paint::Print(placed(layers)),
                other => other.clone(),
            };
            Piece { id: PieceId { owner: p.owner.unwrap_or(owner), local: p.local }, style: p.style, contours: p.contours.clone(), lines, paint, bounds }
        })
        .collect();
    let geometry = Geometry { pieces, placement, grab: local.grab };
    debug_assert!(distinct_ids(&geometry.pieces), "a shape's pieces must have distinct ids: {:?}", geometry.pieces.iter().map(|p| p.id).collect::<Vec<_>>());
    geometry
}

/// Whether no two of `pieces` share an id, which hit-testing, selection and the bridge's path ids rely on.
fn distinct_ids(pieces: &[Piece]) -> bool {
    pieces.iter().enumerate().all(|(i, p)| pieces[..i].iter().all(|q| q.id != p.id))
}

/// `shape` evaluated and placed, uncached: for shapes outside a design, such as Flattened parts.
pub fn evaluate(shape: &Shape) -> Geometry {
    place(&evaluate_local(shape), shape.id, Placement::of(shape))
}

fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

#[cfg(test)]
mod skew_tests {
    use crate::design::{Combine, Design, ShapeKind};
    use crate::geometry::{Boolean, boolean, even_odd_area};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// A 30 degree Skew X leans a square into a parallelogram whose top edge slides by height * tan 30,
    /// and whose area is unchanged because a shear preserves area.
    #[test]
    fn skew_x_leans_the_square_without_changing_its_area() {
        let mut d = Design::default();
        let id = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(id).unwrap();
        (s.width_in, s.height_in, s.skew_deg) = (2.0, 2.0, (30.0, 0.0));
        let shape = d.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let rings = d.geometry(&shape).rings(0.001);
        assert_eq!(rings.len(), 1, "one outer ring");
        let ring = &rings[0];
        assert!(close(even_odd_area(&rings), 4.0), "area stays 2 x 2: {}", even_odd_area(&rings));
        let top = ring.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        let bottom = ring.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert!(close((bottom.0 - top.0).abs(), 2.0 * 30.0_f32.to_radians().tan()), "top edge offset by height * tan 30");
    }

    /// Combine reads the skewed outline through the placement matrix, so a Weld of two skewed squares
    /// equals the boolean union of their skewed outlines.
    #[test]
    fn combine_welds_the_skewed_outlines() {
        let mut d = Design::default();
        let a = d.add_shape(ShapeKind::Square);
        let b = d.add_shape(ShapeKind::Square);
        for id in [a, b] {
            let s = d.shape_mut(id).unwrap();
            (s.width_in, s.height_in, s.skew_deg) = (2.0, 2.0, (25.0, 0.0));
        }
        d.shape_mut(b).unwrap().x_in = 1.0;
        let get = |d: &Design, id: u64| d.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let rings_a = d.geometry(&get(&d, a)).rings(0.001);
        let rings_b = d.geometry(&get(&d, b)).rings(0.001);
        let reference = even_odd_area(&boolean(&[rings_a, rings_b], Boolean::Weld));
        let made = d.combine(&[a, b], Combine::Weld);
        let welded = d.shapes.iter().find(|s| s.id == made[0]).unwrap().clone();
        let area = even_odd_area(&d.geometry(&welded).rings(0.001));
        assert!(close(area, reference), "welded area {area} vs union of skewed outlines {reference}");
    }
}
