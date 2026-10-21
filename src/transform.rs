//! Pure geometry for direct manipulation: where a shape's handles sit, and how a drag on each one
//! changes the shape. Everything is in inch space; the UI converts pointer pixels to inches.

use crate::design::{Design, Pt, Shape};

const MIN_SIZE_IN: f32 = 0.05;

/// Skew is clamped to this many degrees either way, so a shear never folds the shape flat (spec 52).
pub const MAX_SKEW_DEG: f32 = 89.0;

/// A shape may drift up to one artboard (`WORLD_MARGIN` x the mat size) past each edge into the void.
pub const WORLD_MARGIN: f32 = 1.0;

/// Clamps a span of `size` starting at `pos` to the void-extended world around a `board`-sized mat:
/// the near edge reaches `-board`, the far edge reaches `2 * board`. Shared by the drag, nudge and
/// typed-field paths so they cannot drift apart.
pub fn clamp_pos(pos: f32, size: f32, board: f32) -> f32 {
    let margin = board * WORLD_MARGIN;
    pos.clamp(-margin, (board + margin - size).max(-margin))
}

/// The largest width or height a typed size may take: `DEFAULT_CANVAS_SIZE`, the 252 in canvas.
pub const MAX_SIZE_IN: f32 = 252.0;

/// A numeric property the inspector types in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    X,
    Y,
    W,
    H,
    Rotation,
}

/// A shape's current value for `field`, in inches or degrees.
pub fn field_value(shape: &Shape, field: Field) -> f32 {
    match field {
        Field::X => shape.x_in,
        Field::Y => shape.y_in,
        Field::W => shape.width_in,
        Field::H => shape.height_in,
        Field::Rotation => shape.rotation_deg,
    }
}

/// Applies one field edit, in inches or degrees, to a shape. W and H move together when the shape's
/// aspect is locked. X and Y clamp through the same void-extended rule as dragging, over a `board`-sized mat.
pub fn set_field(shape: &mut Shape, field: Field, value: f32, board: (f32, f32)) {
    let keep_ratio = shape.lock_aspect;
    let size = |v: f32| v.clamp(0.001, MAX_SIZE_IN);
    let ratio = shape.height_in / shape.width_in.max(1e-4);
    match field {
        Field::X => shape.x_in = clamp_pos(value, shape.width_in, board.0),
        Field::Y => shape.y_in = clamp_pos(value, shape.height_in, board.1),
        Field::W => {
            let before = shape.width_in.max(1e-4);
            shape.width_in = size(value);
            if keep_ratio {
                shape.height_in = size(shape.width_in * ratio);
            }
            crate::parametric::rescale(&mut shape.kind, shape.width_in / before);
        }
        Field::H => {
            let before = shape.width_in.max(1e-4);
            shape.height_in = size(value);
            if keep_ratio {
                shape.width_in = size(shape.height_in / ratio.max(1e-4));
            }
            crate::parametric::rescale(&mut shape.kind, shape.width_in / before);
        }
        Field::Rotation => shape.rotation_deg = value.rem_euclid(360.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    pub const RESIZE: [Handle; 8] = [Self::TopLeft, Self::Top, Self::TopRight, Self::Right, Self::BottomRight, Self::Bottom, Self::BottomLeft, Self::Left];

    /// Which edges the handle drives, each `-1`, `0` or `+1` along the shape's local axes.
    fn sign(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (-1.0, -1.0),
            Self::Top => (0.0, -1.0),
            Self::TopRight => (1.0, -1.0),
            Self::Right => (1.0, 0.0),
            Self::BottomRight => (1.0, 1.0),
            Self::Bottom => (0.0, 1.0),
            Self::BottomLeft => (-1.0, 1.0),
            Self::Left => (-1.0, 0.0),
        }
    }
}

fn rotate(deg: f32, (x, y): Pt) -> Pt {
    let (sin, cos) = deg.to_radians().sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// The selection frame in inches. One visible shape gives its rotated box; several give the axis-aligned
/// union of their world bounds with rotation 0. Hidden shapes are skipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub centre: Pt,
    pub width_in: f32,
    pub height_in: f32,
    pub rotation_deg: f32,
    /// Skew in degrees along the frame's own X and Y axes, so the frame leans as a parallelogram (spec
    /// 52). A multi-selection's union frame carries none.
    pub skew_deg: (f32, f32),
    /// Mirror along the frame's own X and Y axes, so a flipped shape's frame places its corners the way
    /// its geometry sits.
    pub flip: (bool, bool),
}

impl Frame {
    /// One shape's own box, or the world bounds of several, as `design` evaluates them.
    pub fn of<'a>(design: &Design, shapes: impl IntoIterator<Item = &'a Shape>) -> Option<Frame> {
        let visible: Vec<&Shape> = shapes.into_iter().filter(|s| !s.hidden).collect();
        if let [s] = visible.as_slice() {
            return Some(Frame {
                centre: s.centre_in(),
                width_in: s.width_in,
                height_in: s.height_in,
                rotation_deg: s.rotation_deg,
                skew_deg: s.skew_deg,
                flip: (s.flip_x, s.flip_y),
            });
        }
        let b = visible.iter().map(|s| design.geometry(s).bounds()).reduce(crate::snap::Aabb::union)?;
        Some(Frame { centre: b.centre(), width_in: b.width(), height_in: b.height(), rotation_deg: 0.0, skew_deg: (0.0, 0.0), flip: (false, false) })
    }

    /// The shear's `(tan x, tan y)`, matching `effects::eval::Placement` so the frame leans the same way
    /// the geometry does.
    fn shear(&self) -> (f32, f32) {
        (self.skew_deg.0.to_radians().tan(), self.skew_deg.1.to_radians().tan())
    }

    /// A point given relative to the centre along the frame's own axes, in world inches: flip, then
    /// shear, then rotate, as the placement matrix does (spec 52).
    fn place(&self, local: Pt) -> Pt {
        let (tx, ty) = self.shear();
        let (mut lx, mut ly) = local;
        if self.flip.0 {
            lx = -lx;
        }
        if self.flip.1 {
            ly = -ly;
        }
        let (ox, oy) = rotate(self.rotation_deg, (lx + ly * tx, ly + lx * ty));
        (self.centre.0 + ox, self.centre.1 + oy)
    }

    /// The inverse of `place`: unrotate, unshear, unflip.
    fn local(&self, (x, y): Pt) -> Pt {
        let (tx, ty) = self.shear();
        let (dx, dy) = rotate(-self.rotation_deg, (x - self.centre.0, y - self.centre.1));
        let det = 1.0 - tx * ty;
        let (mut lx, mut ly) = if det.abs() < 1e-6 { (dx, dy) } else { ((dx - dy * tx) / det, (dy - dx * ty) / det) };
        if self.flip.0 {
            lx = -lx;
        }
        if self.flip.1 {
            ly = -ly;
        }
        (lx, ly)
    }

    pub fn handle_pos(&self, h: Handle) -> Pt {
        let (sx, sy) = h.sign();
        self.place((sx * self.width_in / 2.0, sy * self.height_in / 2.0))
    }

    /// The rotate handle, `arm_in` inches above the top edge.
    pub fn rotate_handle_pos(&self, arm_in: f32) -> Pt {
        self.place((0.0, -self.height_in / 2.0 - arm_in))
    }

    /// The frame after dragging `h` to `pointer`. The opposite corner or edge stays put.
    pub fn resized(&self, h: Handle, pointer: Pt, keep_aspect: bool) -> Frame {
        let (sx, sy) = h.sign();
        let (w, h) = (self.width_in, self.height_in);
        let anchor = (-sx * w / 2.0, -sy * h / 2.0);
        let p = self.local(pointer);
        let rel = (p.0 - anchor.0, p.1 - anchor.1);
        let mut new_w = if sx == 0.0 { w } else { (sx * rel.0).max(MIN_SIZE_IN) };
        let mut new_h = if sy == 0.0 { h } else { (sy * rel.1).max(MIN_SIZE_IN) };
        if keep_aspect {
            let scale = match (sx != 0.0, sy != 0.0) {
                (true, true) => (new_w / w).max(new_h / h),
                (true, false) => new_w / w,
                _ => new_h / h,
            };
            new_w = w * scale;
            new_h = h * scale;
        }
        let centre = self.place((anchor.0 + sx * new_w / 2.0, anchor.1 + sy * new_h / 2.0));
        Frame { centre, width_in: new_w, height_in: new_h, ..*self }
    }

    /// Absolute rotation in degrees for the rotate handle dragged to `pointer`, in 15 deg steps when `snap`.
    pub fn rotation_to(&self, pointer: Pt, snap: bool) -> f32 {
        let (cx, cy) = self.centre;
        let deg = ((pointer.1 - cy).atan2(pointer.0 - cx).to_degrees() + 90.0).rem_euclid(360.0);
        if snap { (deg / 15.0).round() * 15.0 % 360.0 } else { deg }
    }

    /// The frame turned by `delta_deg` about a world `pivot`: its rotation gains the angle and its centre
    /// swings round the pivot, so the pivot stays fixed. A pivot at the centre is a plain spin.
    pub fn rotated_about(&self, pivot: Pt, delta_deg: f32) -> Frame {
        let (ox, oy) = rotate(delta_deg, (self.centre.0 - pivot.0, self.centre.1 - pivot.1));
        Frame { centre: (pivot.0 + ox, pivot.1 + oy), rotation_deg: self.rotation_deg + delta_deg, ..*self }
    }

    /// The frame one more step beyond `self`, repeating the move, rotation, resize, skew and flip that
    /// carried `from` to `self`. Reapplying the same affine preserves an off-centre pivot, so a rotation
    /// about a corner steps round that corner (spec 52, Transform Again and Power Duplicate).
    pub fn stepped_from(&self, from: &Frame) -> Frame {
        let ratio = |to: f32, from: f32| if from.abs() > f32::EPSILON { to / from } else { 1.0 };
        let (sx, sy) = (ratio(self.width_in, from.width_in), ratio(self.height_in, from.height_in));
        // The linear part of from -> self: into from's axes, scale, then out along self's axes.
        let local = rotate(-from.rotation_deg, (self.centre.0 - from.centre.0, self.centre.1 - from.centre.1));
        let swung = rotate(self.rotation_deg, (local.0 * sx, local.1 * sy));
        // Skew adds its step again; flip is its own inverse, so the toggle repeats.
        let dflip = (self.flip.0 ^ from.flip.0, self.flip.1 ^ from.flip.1);
        Frame {
            centre: (self.centre.0 + swung.0, self.centre.1 + swung.1),
            width_in: self.width_in * sx,
            height_in: self.height_in * sy,
            rotation_deg: self.rotation_deg + (self.rotation_deg - from.rotation_deg),
            skew_deg: clamp_skew((self.skew_deg.0 + (self.skew_deg.0 - from.skew_deg.0), self.skew_deg.1 + (self.skew_deg.1 - from.skew_deg.1))),
            flip: (self.flip.0 ^ dflip.0, self.flip.1 ^ dflip.1),
        }
    }

    /// Where a shape's stored `pivot` (local, `None` meaning the centre) sits in world inches.
    pub fn pivot_world(&self, pivot: Option<Pt>) -> Pt {
        match pivot {
            None => self.centre,
            Some((px, py)) => self.place((px - self.width_in / 2.0, py - self.height_in / 2.0)),
        }
    }

    /// A world point as a shape's stored `pivot`, the inverse of [`Self::pivot_world`] (spec 52).
    pub fn local_pivot(&self, world: Pt) -> Pt {
        let (lx, ly) = self.local(world);
        (lx + self.width_in / 2.0, ly + self.height_in / 2.0)
    }
}

/// `originals` carried from `from` to `to`; see [`Frame::stepped_from`] and the private remap.
pub fn remap_to(originals: &[Shape], from: &Frame, to: &Frame) -> Vec<Shape> {
    remap(originals, from, to)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// `originals` after their frame's `handle` is dragged to `pointer`.
pub fn resize_many(design: &Design, originals: &[Shape], handle: Handle, pointer: Pt, keep_aspect: bool) -> Vec<Shape> {
    let Some(from) = Frame::of(design, originals) else { return originals.to_vec() };
    remap(originals, &from, &from.resized(handle, pointer, keep_aspect))
}

/// `originals` turned by `delta_deg` about their frame's centre.
pub fn rotate_many(design: &Design, originals: &[Shape], delta_deg: f32) -> Vec<Shape> {
    let Some(from) = Frame::of(design, originals) else { return originals.to_vec() };
    remap(originals, &from, &Frame { rotation_deg: from.rotation_deg + delta_deg, ..from })
}

/// `originals` turned by `delta_deg` about a world `pivot`, so a hinge, clock hand or petal swings round
/// a point at an edge or outside the shape (spec 52).
pub fn rotate_many_about(design: &Design, originals: &[Shape], delta_deg: f32, pivot: Pt) -> Vec<Shape> {
    let Some(from) = Frame::of(design, originals) else { return originals.to_vec() };
    remap(originals, &from, &from.rotated_about(pivot, delta_deg))
}

/// The skew in degrees a Cmd-drag of side `handle` to `pointer` gives a shape framed by `frame`, shearing
/// instead of resizing as Affinity's shear handle does (spec 52). A Top or Bottom handle leans the X axis
/// so the dragged edge follows the pointer; a Left or Right handle leans the Y axis. Clamped flat.
pub fn sheared(frame: &Frame, handle: Handle, pointer: Pt) -> (f32, f32) {
    let (dx, dy) = rotate(-frame.rotation_deg, (pointer.0 - frame.centre.0, pointer.1 - frame.centre.1));
    let lean = |offset: f32, span: f32| (offset / (span / 2.0).max(MIN_SIZE_IN)).atan().to_degrees().clamp(-MAX_SKEW_DEG, MAX_SKEW_DEG);
    match handle {
        Handle::Top => (lean(-dx, frame.height_in), frame.skew_deg.1),
        Handle::Bottom => (lean(dx, frame.height_in), frame.skew_deg.1),
        Handle::Left => (frame.skew_deg.0, lean(-dy, frame.width_in)),
        Handle::Right => (frame.skew_deg.0, lean(dy, frame.width_in)),
        _ => frame.skew_deg,
    }
}

/// `originals` mirrored across their frame's centre line along its own axes. A lone shape flips in place.
pub fn flip_many(design: &Design, originals: &[Shape], axis: Axis) -> Vec<Shape> {
    let Some(f) = Frame::of(design, originals) else { return originals.to_vec() };
    originals
        .iter()
        .map(|s| {
            let (lx, ly) = f.local(s.centre_in());
            let mirrored = match axis {
                Axis::Horizontal => (-lx, ly),
                Axis::Vertical => (lx, -ly),
            };
            let (cx, cy) = f.place(mirrored);
            let mut out =
                Shape { x_in: cx - s.width_in / 2.0, y_in: cy - s.height_in / 2.0, rotation_deg: (2.0 * f.rotation_deg - s.rotation_deg).rem_euclid(360.0), ..s.clone() };
            match axis {
                Axis::Horizontal => out.flip_x = !out.flip_x,
                Axis::Vertical => out.flip_y = !out.flip_y,
            }
            out
        })
        .collect()
}

/// Skew held to the app limit, so a repeat step cannot walk it toward `tan(90deg)`.
pub(crate) fn clamp_skew((x, y): Pt) -> Pt {
    (x.clamp(-MAX_SKEW_DEG, MAX_SKEW_DEG), y.clamp(-MAX_SKEW_DEG, MAX_SKEW_DEG))
}

/// Carries each shape from `from` to `to`: its centre keeps its place relative to the frame, its rotation
/// turns with the frame, and its box stretches by the frame's stretch projected onto the shape's own axes.
/// A member at an angle to a non-uniform stretch would need skew to stay exact; this keeps it rectangular.
fn remap(originals: &[Shape], from: &Frame, to: &Frame) -> Vec<Shape> {
    let ratio = |to: f32, from: f32| if from > f32::EPSILON { to / from } else { 1.0 };
    let (sx, sy) = (ratio(to.width_in, from.width_in), ratio(to.height_in, from.height_in));
    // The frame's skew and flip step onto each member, so a repeat carries a shear or a mirror (spec 52).
    let dskew = (to.skew_deg.0 - from.skew_deg.0, to.skew_deg.1 - from.skew_deg.1);
    let dflip = (to.flip.0 ^ from.flip.0, to.flip.1 ^ from.flip.1);
    originals
        .iter()
        .map(|s| {
            let (lx, ly) = from.local(s.centre_in());
            let (cx, cy) = to.place((lx * sx, ly * sy));
            let (sin, cos) = (s.rotation_deg - from.rotation_deg).to_radians().sin_cos();
            let w = (s.width_in * (sx * cos).hypot(sy * sin)).max(MIN_SIZE_IN);
            let h = (s.height_in * (sx * sin).hypot(sy * cos)).max(MIN_SIZE_IN);
            let rotation_deg = (s.rotation_deg + to.rotation_deg - from.rotation_deg).rem_euclid(360.0);
            let mut kind = s.kind.clone();
            crate::parametric::rescale(&mut kind, w / s.width_in.max(1e-4));
            Shape {
                x_in: cx - w / 2.0,
                y_in: cy - h / 2.0,
                width_in: w,
                height_in: h,
                rotation_deg,
                skew_deg: clamp_skew((s.skew_deg.0 + dskew.0, s.skew_deg.1 + dskew.1)),
                flip_x: s.flip_x ^ dflip.0,
                flip_y: s.flip_y ^ dflip.1,
                kind,
                ..s.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Operation, ShapeKind};

    #[test]
    fn clamp_pos_lets_a_shape_into_the_void_but_not_past_one_board() {
        assert_eq!(clamp_pos(-3.0, 1.0, 12.0), -3.0, "off the left edge is allowed");
        assert_eq!(clamp_pos(-100.0, 1.0, 12.0), -12.0, "no further than one board out");
        assert_eq!(clamp_pos(1000.0, 1.0, 12.0), 23.0, "and one board out on the far side");
    }
    use egui::Color32;

    fn square() -> Shape {
        Shape { width_in: 2.0, height_in: 2.0, fill: Color32::BLACK, operation: Operation::BasicCut, pen: Default::default(), ..Shape::new(1, ShapeKind::Square) }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn frame(s: &Shape) -> Frame {
        Frame::of(&Design::default(), [s]).unwrap()
    }

    fn at(x: f32, y: f32) -> Shape {
        Shape { x_in: x, y_in: y, width_in: 1.0, height_in: 1.0, ..square() }
    }

    #[test]
    fn transform_again_cannot_shear_past_the_limit() {
        let sheared = |deg: f32| Frame { skew_deg: (deg, -deg), ..frame(&square()) };
        let (from, to) = (sheared(0.0), sheared(60.0));
        let step = to.stepped_from(&from);
        assert_eq!(step.skew_deg, (MAX_SKEW_DEG, -MAX_SKEW_DEG), "60 + 60 stops at the limit");
        let carried = remap(&[square()], &from, &to);
        assert_eq!(carried[0].skew_deg, (60.0, -60.0));
        let again = remap(&carried, &from, &to);
        assert_eq!(again[0].skew_deg, (MAX_SKEW_DEG, -MAX_SKEW_DEG), "a repeat clamps too");
    }

    #[test]
    fn handles_sit_on_the_box_edges() {
        let f = frame(&square());
        assert_eq!(f.handle_pos(Handle::TopLeft), (0.0, 0.0));
        assert_eq!(f.handle_pos(Handle::BottomRight), (2.0, 2.0));
        assert_eq!(f.handle_pos(Handle::Right), (2.0, 1.0));
        assert_eq!(f.rotate_handle_pos(0.5), (1.0, -0.5));
    }

    #[test]
    fn corner_resize_keeps_the_opposite_corner_fixed() {
        let [b] = resize_many(&Design::default(), &[square()], Handle::BottomRight, (3.0, 3.0), false).try_into().unwrap();
        assert!(close(b.x_in, 0.0) && close(b.y_in, 0.0));
        assert!(close(b.width_in, 3.0) && close(b.height_in, 3.0));
    }

    #[test]
    fn edge_resize_moves_one_axis_only() {
        let [b] = resize_many(&Design::default(), &[square()], Handle::Right, (3.0, 1.5), false).try_into().unwrap();
        assert!(close(b.width_in, 3.0) && close(b.height_in, 2.0));
        assert!(close(b.x_in, 0.0) && close(b.y_in, 0.0));
    }

    #[test]
    fn shift_keeps_aspect_from_the_dominant_axis() {
        let f = frame(&square()).resized(Handle::BottomRight, (4.0, 3.0), true);
        assert!(close(f.width_in, 4.0) && close(f.height_in, 4.0));
    }

    #[test]
    fn a_side_handle_keeping_aspect_scales_both_about_the_opposite_edge() {
        let f = Frame { width_in: 2.0, height_in: 1.0, ..frame(&square()) };
        let grown = f.resized(Handle::Right, (4.0, 0.5), true);
        assert!(close(grown.width_in, 4.0) && close(grown.height_in, 2.0), "{grown:?}");
        assert!(close(grown.handle_pos(Handle::Left).0, f.handle_pos(Handle::Left).0) && close(grown.centre.1, f.centre.1));
        let free = f.resized(Handle::Right, (4.0, 0.5), false);
        assert!(close(free.height_in, 1.0));
    }

    #[test]
    fn resize_clamps_to_a_minimum() {
        let [b] = resize_many(&Design::default(), &[square()], Handle::BottomRight, (-1.0, -1.0), false).try_into().unwrap();
        assert!(b.width_in >= MIN_SIZE_IN && b.height_in >= MIN_SIZE_IN);
    }

    #[test]
    fn rotation_reads_from_the_pointer_and_snaps() {
        let f = frame(&square());
        assert!(close(f.rotation_to((1.0, -1.0), false), 0.0));
        assert!(close(f.rotation_to((3.0, 1.0), false), 90.0));
        assert!(close(f.rotation_to((1.2, -1.0), true), 0.0));
    }

    #[test]
    fn a_rotated_corner_resize_still_pins_its_anchor() {
        let s = Shape { rotation_deg: 90.0, ..square() };
        let anchor = frame(&s).handle_pos(Handle::TopLeft);
        let [moved] = resize_many(&Design::default(), std::slice::from_ref(&s), Handle::BottomRight, (5.0, 5.0), false).try_into().unwrap();
        let anchor_after = frame(&moved).handle_pos(Handle::TopLeft);
        assert!(close(anchor.0, anchor_after.0) && close(anchor.1, anchor_after.1));
        assert!(close(moved.rotation_deg, 90.0));
    }

    #[test]
    fn several_shapes_frame_their_union_unrotated() {
        let f = Frame::of(&Design::default(), &[at(0.0, 0.0), at(3.0, 1.0), Shape { hidden: true, ..at(9.0, 9.0) }]).unwrap();
        assert!(close(f.width_in, 4.0) && close(f.height_in, 2.0) && close(f.rotation_deg, 0.0));
        assert!(close(f.centre.0, 2.0) && close(f.centre.1, 1.0));
        assert_eq!(Frame::of(&Design::default(), &[Shape { hidden: true, ..square() }]), None);
    }

    #[test]
    fn resize_many_scales_the_layout_about_the_anchor() {
        let out = resize_many(&Design::default(), &[at(0.0, 0.0), at(3.0, 1.0)], Handle::BottomRight, (8.0, 4.0), false);
        assert!(close(out[0].x_in, 0.0) && close(out[0].y_in, 0.0));
        assert!(close(out[0].width_in, 2.0) && close(out[0].height_in, 2.0));
        assert!(close(out[1].x_in, 6.0) && close(out[1].y_in, 2.0));
        assert!(close(out[1].width_in, 2.0) && close(out[1].height_in, 2.0));
        assert_eq!((out[0].id, out[1].id), (1, 1));
    }

    #[test]
    fn rotate_many_by_90_twice_equals_180() {
        let shapes = [at(0.0, 0.0), Shape { width_in: 2.0, rotation_deg: 30.0, ..at(3.0, 1.0) }];
        let twice = rotate_many(&Design::default(), &rotate_many(&Design::default(), &shapes, 90.0), 90.0);
        let once = rotate_many(&Design::default(), &shapes, 180.0);
        for (a, b) in twice.iter().zip(&once) {
            let (ca, cb) = (a.centre_in(), b.centre_in());
            assert!(close(ca.0, cb.0) && close(ca.1, cb.1), "{ca:?} vs {cb:?}");
            assert!(close(a.width_in, b.width_in) && close(a.height_in, b.height_in));
            assert!(close(a.rotation_deg.rem_euclid(360.0), b.rotation_deg.rem_euclid(360.0)));
        }
        let (c, f) = (once[1].centre_in(), Frame::of(&Design::default(), &shapes).unwrap().centre);
        assert!(close(c.0, 2.0 * f.0 - 4.0) && close(c.1, 2.0 * f.1 - 1.5), "turned about the frame centre: {c:?}");
        assert!(close(once[1].rotation_deg, 210.0));
    }

    #[test]
    fn a_pivot_at_a_corner_turns_the_opposite_corner_about_it() {
        let s = square();
        let f = frame(&s);
        let pivot = f.handle_pos(Handle::TopLeft);
        let before = f.handle_pos(Handle::BottomRight);
        let after = f.rotated_about(pivot, 90.0).handle_pos(Handle::BottomRight);
        let expect = rotate(90.0, (before.0 - pivot.0, before.1 - pivot.1));
        assert!(close(after.0, pivot.0 + expect.0) && close(after.1, pivot.1 + expect.1), "{after:?} vs rotate-about-point");
        let [moved] = rotate_many_about(&Design::default(), std::slice::from_ref(&s), 90.0, pivot).try_into().unwrap();
        let world = frame(&moved).handle_pos(Handle::BottomRight);
        assert!(close(world.0, after.0) && close(world.1, after.1), "the shape's matrix equals rotate-about-point");
    }

    #[test]
    fn stepping_a_frame_repeats_a_move_at_an_even_pitch() {
        let from = Frame { centre: (0.0, 0.0), width_in: 1.0, height_in: 1.0, rotation_deg: 0.0, skew_deg: (0.0, 0.0), flip: (false, false) };
        let now = Frame { centre: (1.0, 0.0), ..from };
        let next = now.stepped_from(&from);
        assert!(close(next.centre.0, 2.0) && close(next.centre.1, 0.0), "one pitch on: {:?}", next.centre);
    }

    #[test]
    fn stepping_a_frame_repeats_a_rotation_about_the_same_pivot() {
        let pivot = (0.0, 0.0);
        let from = Frame { centre: (1.0, 0.0), width_in: 1.0, height_in: 1.0, rotation_deg: 0.0, skew_deg: (0.0, 0.0), flip: (false, false) };
        let now = from.rotated_about(pivot, 30.0);
        let next = now.stepped_from(&from);
        let expect = from.rotated_about(pivot, 60.0);
        assert!(close(next.centre.0, expect.centre.0) && close(next.centre.1, expect.centre.1), "{:?} vs {:?}", next.centre, expect.centre);
        assert!(close(next.rotation_deg, 60.0));
    }

    #[test]
    fn flip_many_mirrors_positions_across_the_frame() {
        let out = flip_many(&Design::default(), &[at(0.0, 0.0), Shape { rotation_deg: 90.0, ..at(3.0, 1.0) }], Axis::Horizontal);
        assert!(close(out[0].x_in, 3.0) && close(out[1].x_in, 0.0));
        assert!(close(out[1].y_in, 1.0));
        assert!(out.iter().all(|s| s.flip_x && !s.flip_y));
        assert!(close(out[1].rotation_deg.rem_euclid(360.0), 270.0));
        let [one] = flip_many(&Design::default(), &[Shape { rotation_deg: 30.0, ..square() }], Axis::Vertical).try_into().unwrap();
        assert!(one.flip_y && close(one.rotation_deg, 30.0) && close(one.x_in, 0.0), "a lone shape flips in place about its own axes");
    }

    #[test]
    fn a_skewed_frame_leans_as_a_parallelogram() {
        // A 30 degree Skew X leans the top edge by height * tan 30, with the width unchanged.
        let s = Shape { skew_deg: (30.0, 0.0), ..square() };
        let f = frame(&s);
        let (tl, tr, bl) = (f.handle_pos(Handle::TopLeft), f.handle_pos(Handle::TopRight), f.handle_pos(Handle::BottomLeft));
        assert!(close(tr.0 - tl.0, 2.0) && close(tr.1 - tl.1, 0.0), "the top edge keeps its width: {tl:?} {tr:?}");
        let lean = 2.0 * 30.0_f32.to_radians().tan();
        assert!(close((tl.0 - bl.0).abs(), lean), "the top edge is offset by height * tan 30: {}", tl.0 - bl.0);
    }

    #[test]
    fn cmd_dragging_the_top_handle_shears_x_so_the_edge_follows_the_pointer() {
        let f = frame(&square());
        assert_eq!(sheared(&f, Handle::Top, f.handle_pos(Handle::Top)), (0.0, 0.0), "no shear at rest");
        // Dragging the top edge one inch right leans the top the same way, by atan(1 / halfheight).
        let (sx, sy) = sheared(&f, Handle::Top, (f.handle_pos(Handle::Top).0 + 1.0, f.handle_pos(Handle::Top).1));
        assert!(close(sx, -(1.0_f32 / 1.0).atan().to_degrees()) && close(sy, 0.0), "{sx} {sy}");
        // A Left handle leans Y instead of X, and a corner never shears.
        let left = sheared(&f, Handle::Left, (1.0, 3.0));
        assert!(close(left.0, 0.0) && left.1.abs() > 1.0, "the Left handle leans Y only: {left:?}");
        assert_eq!(sheared(&f, Handle::TopLeft, (9.0, 9.0)), (0.0, 0.0));
    }

    #[test]
    fn stepping_a_frame_repeats_a_skew_and_a_flip() {
        let from = Frame { centre: (0.0, 0.0), width_in: 1.0, height_in: 1.0, rotation_deg: 0.0, skew_deg: (0.0, 0.0), flip: (false, false) };
        // The step shears 10 degrees and flips X once; the next copy shears to 20 and flips back.
        let now = Frame { centre: (1.0, 0.0), skew_deg: (10.0, 0.0), flip: (true, false), ..from };
        let next = now.stepped_from(&from);
        assert!(close(next.skew_deg.0, 20.0), "skew steps on: {}", next.skew_deg.0);
        assert!(!next.flip.0, "the flip toggles back");
        // And the shapes carry it: remapping a square from now to next sets its skew and clears its flip.
        let s = Shape { skew_deg: (10.0, 0.0), flip_x: true, ..square() };
        let [out] = remap(std::slice::from_ref(&s), &now, &next).try_into().unwrap();
        assert!(close(out.skew_deg.0, 20.0) && !out.flip_x, "{:?} {}", out.skew_deg, out.flip_x);
    }
}
