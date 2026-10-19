//! Parametric shapes (spec 49). One pure generator, [`outline`], turns a [`Params`] and a size in
//! inches into contours. A preset table, [`from_fixed`], reproduces every fixed shape the Shapes panel
//! used to carry, so a project saved with an old kind upgrades to the parametric preset that draws the
//! same outline. The star, rounded-rectangle, ring and polygon helpers live here, as the one place that
//! builds these outlines.

use crate::design::{Contour, Pt, Seg};

/// A polygon or star keeps between a triangle and this many points.
pub const MIN_SIDES: u32 = 3;
pub const MAX_SIDES: u32 = 64;

/// Which family a parametric shape belongs to. It groups a multi-selection in the inspector and keys the
/// preset table. Each [`Params`] variant names its own kind through [`Params::kind`].
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ParamKind {
    Rectangle,
    Ellipse,
    Polygon,
    Star,
    Arc,
    Ring,
    Line,
    Plus,
}

impl ParamKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
            Self::Arc => "Arc",
            Self::Ring => "Ring",
            Self::Line => "Line",
            Self::Plus => "Plus",
        }
    }
}

/// How an arc closes its ends: a filled sector back through the centre, a straight chord across the
/// mouth, or an open stroke that is cut as a line.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ArcClose {
    Pie,
    Chord,
    Open,
}

/// One editable numeric parameter, named for the inspector field and the canvas handle that drive it.
/// `Radius` is the Rectangle corner radius, `Fillet` a Polygon's, `Point` and `Valley` a Star's outer
/// and inner corner radii.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamField {
    Radius,
    Sides,
    Fillet,
    Inner,
    Point,
    Valley,
    ArcStart,
    ArcSweep,
    ArcInner,
    Thickness,
    Bar,
}

impl ParamField {
    /// Whether the field is a length in inches (so it takes a length expression) rather than a count,
    /// an angle or a fraction.
    pub fn is_length(self) -> bool {
        matches!(self, Self::Radius | Self::Fillet | Self::Point | Self::Valley)
    }

    fn range(self) -> std::ops::RangeInclusive<f32> {
        match self {
            Self::Sides => MIN_SIDES as f32..=MAX_SIDES as f32,
            Self::Inner | Self::ArcInner | Self::Thickness | Self::Bar => 0.0..=1.0,
            Self::ArcStart | Self::ArcSweep => 0.0..=360.0,
            Self::Radius | Self::Fillet | Self::Point | Self::Valley => 0.0..=f32::MAX,
        }
    }
}

/// A change to one parametric shape's fields, the subject of `Edit::Params`. `Set` drives a numeric
/// field; the rest are the toggles and the arc's closure.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ParamOp {
    Set {
        field: ParamField,
        value: crate::expr::Typed,
    },
    /// One rectangle corner by index, from a handle drag; sets all four when the corners are linked.
    Corner {
        at: usize,
        value: crate::expr::Typed,
    },
    Link(bool),
    ScaleWithShape(bool),
    Close(ArcClose),
}

/// A parametric shape's values, in its own local frame. Corner radii and ring thickness are inches, so a
/// resize keeps them unless the Rectangle's `scale_with_shape` is on; counts, fractions and angles are
/// plain. `turn` orients a polygon or star within its box, point up by default.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Params {
    /// One corner radius per corner, in the order top-left, top-right, bottom-right, bottom-left.
    Rectangle {
        corners: [f32; 4],
        linked: bool,
        scale_with_shape: bool,
    },
    Ellipse,
    Polygon {
        sides: u32,
        corner: f32,
        turn: f32,
    },
    Star {
        points: u32,
        inner: f32,
        corner: f32,
        inner_corner: f32,
        turn: f32,
    },
    Arc {
        start_deg: f32,
        sweep_deg: f32,
        inner: f32,
        close: ArcClose,
    },
    Ring {
        thickness: f32,
    },
    Line,
    Plus {
        bar: f32,
    },
}

impl Params {
    pub fn kind(&self) -> ParamKind {
        match self {
            Self::Rectangle { .. } => ParamKind::Rectangle,
            Self::Ellipse => ParamKind::Ellipse,
            Self::Polygon { .. } => ParamKind::Polygon,
            Self::Star { .. } => ParamKind::Star,
            Self::Arc { .. } => ParamKind::Arc,
            Self::Ring { .. } => ParamKind::Ring,
            Self::Line => ParamKind::Line,
            Self::Plus { .. } => ParamKind::Plus,
        }
    }

    /// The params after a resize whose width grew by `fx`. Corner radii follow the box only when the
    /// Rectangle asks, tracking the width as Affinity's Scale with object does; otherwise they keep their
    /// inch value.
    #[must_use]
    pub fn resized(self, fx: f32) -> Params {
        match self {
            Params::Rectangle { corners, linked, scale_with_shape: true } => Params::Rectangle { corners: corners.map(|r| r * fx), linked, scale_with_shape: true },
            other => other,
        }
    }

    /// The numeric fields this shape shows in the inspector, in order.
    pub fn fields(&self) -> &'static [ParamField] {
        match self {
            Params::Rectangle { .. } => &[ParamField::Radius],
            Params::Ellipse | Params::Line => &[],
            Params::Polygon { .. } => &[ParamField::Sides, ParamField::Fillet],
            Params::Star { .. } => &[ParamField::Sides, ParamField::Inner, ParamField::Point, ParamField::Valley],
            Params::Arc { .. } => &[ParamField::ArcStart, ParamField::ArcSweep, ParamField::ArcInner],
            Params::Ring { .. } => &[ParamField::Thickness],
            Params::Plus { .. } => &[ParamField::Bar],
        }
    }

    /// The field's current value, in its display sense: a count, inches, degrees, or a 0..1 fraction.
    pub fn get(&self, field: ParamField) -> Option<f32> {
        Some(match (self, field) {
            (Params::Rectangle { corners, .. }, ParamField::Radius) => corners[0],
            (Params::Polygon { sides, .. }, ParamField::Sides) => *sides as f32,
            (Params::Polygon { corner, .. }, ParamField::Fillet) => *corner,
            (Params::Star { points, .. }, ParamField::Sides) => *points as f32,
            (Params::Star { inner, .. }, ParamField::Inner) => *inner,
            (Params::Star { corner, .. }, ParamField::Point) => *corner,
            (Params::Star { inner_corner, .. }, ParamField::Valley) => *inner_corner,
            (Params::Arc { start_deg, .. }, ParamField::ArcStart) => *start_deg,
            (Params::Arc { sweep_deg, .. }, ParamField::ArcSweep) => *sweep_deg,
            (Params::Arc { inner, .. }, ParamField::ArcInner) => *inner,
            (Params::Ring { thickness }, ParamField::Thickness) => *thickness,
            (Params::Plus { bar }, ParamField::Bar) => *bar,
            _ => return None,
        })
    }

    /// Whether `op` targets a parameter this shape has, so the edit can reach it (regardless of whether
    /// the value would change).
    pub fn accepts(&self, op: ParamOp) -> bool {
        match op {
            ParamOp::Set { field, .. } => self.get(field).is_some(),
            ParamOp::Corner { .. } | ParamOp::Link(_) | ParamOp::ScaleWithShape(_) => matches!(self, Params::Rectangle { .. }),
            ParamOp::Close(_) => matches!(self, Params::Arc { .. }),
        }
    }

    /// Applies `op`, returning whether anything changed. A count rounds to the nearest whole; a length,
    /// angle or fraction clamps to the field's range.
    pub fn apply(&mut self, op: ParamOp) -> bool {
        match op {
            ParamOp::Set { field, value } => self.set(field, value),
            ParamOp::Corner { at, value } => match self {
                Params::Rectangle { corners, linked, .. } => {
                    let to = value.apply(corners[at.min(3)]).max(0.0);
                    if *linked {
                        let changed = *corners != [to; 4];
                        *corners = [to; 4];
                        changed
                    } else {
                        let changed = corners[at.min(3)] != to;
                        corners[at.min(3)] = to;
                        changed
                    }
                }
                _ => false,
            },
            ParamOp::Link(on) => match self {
                Params::Rectangle { linked, corners, .. } if *linked != on => {
                    *linked = on;
                    // Linking again levels the corners to the first, so they move together from then on.
                    if on {
                        *corners = [corners[0]; 4];
                    }
                    true
                }
                _ => false,
            },
            ParamOp::ScaleWithShape(on) => matches!(self, Params::Rectangle { scale_with_shape, .. } if *scale_with_shape != on)
                .then(|| {
                    if let Params::Rectangle { scale_with_shape, .. } = self {
                        *scale_with_shape = on;
                    }
                })
                .is_some(),
            ParamOp::Close(close) => matches!(self, Params::Arc { close: c, .. } if *c != close)
                .then(|| {
                    if let Params::Arc { close: c, .. } = self {
                        *c = close;
                    }
                })
                .is_some(),
        }
    }

    fn set(&mut self, field: ParamField, value: crate::expr::Typed) -> bool {
        let range = field.range();
        match (self, field) {
            (Params::Rectangle { corners, .. }, ParamField::Radius) => {
                let to = value.apply(corners[0]).clamp(*range.start(), *range.end());
                let changed = *corners != [to; 4];
                *corners = [to; 4];
                changed
            }
            (Params::Polygon { sides, .. }, ParamField::Sides) | (Params::Star { points: sides, .. }, ParamField::Sides) => {
                let to = value.apply(*sides as f32).round().clamp(*range.start(), *range.end()) as u32;
                let changed = *sides != to;
                *sides = to;
                changed
            }
            (Params::Polygon { corner: v, .. }, ParamField::Fillet)
            | (Params::Star { inner: v, .. }, ParamField::Inner)
            | (Params::Star { corner: v, .. }, ParamField::Point)
            | (Params::Star { inner_corner: v, .. }, ParamField::Valley)
            | (Params::Arc { start_deg: v, .. }, ParamField::ArcStart)
            | (Params::Arc { sweep_deg: v, .. }, ParamField::ArcSweep)
            | (Params::Arc { inner: v, .. }, ParamField::ArcInner)
            | (Params::Ring { thickness: v }, ParamField::Thickness)
            | (Params::Plus { bar: v }, ParamField::Bar) => value.set(v, range),
            _ => false,
        }
    }
}

/// A corner radius can never grow past half the shorter of the two edges that meet at it, so adjacent
/// rounds never overlap.
fn clamp_corner(r: f32, edge_a: f32, edge_b: f32) -> f32 {
    r.clamp(0.0, 0.5 * edge_a.min(edge_b))
}

/// The one generator: `params` drawn to fill a `w` x `h` inch box, origin at the top left.
pub fn outline(params: &Params, (w, h): Pt) -> Vec<Contour> {
    match params {
        Params::Rectangle { corners, .. } => vec![rectangle(w, h, *corners)],
        Params::Ellipse => vec![Pen::new().ellipse_arc((w / 2.0, h / 2.0), (w / 2.0, h / 2.0), -90.0, 270.0).close()],
        Params::Polygon { sides, corner, turn } => vec![polygon(*sides, *turn, *corner, (w, h))],
        Params::Star { points, inner, corner, inner_corner, turn } => vec![star(*points, *inner, *corner, *inner_corner, *turn, (w, h))],
        Params::Arc { start_deg, sweep_deg, inner, close } => arc(*start_deg, *sweep_deg, *inner, *close, (w, h)),
        Params::Ring { thickness } => ring(*thickness, (w, h)),
        Params::Line => vec![Contour { start: (w / 2.0, 0.0), segs: vec![Seg::Line((w / 2.0, h))], closed: false }],
        Params::Plus { bar } => vec![plus(*bar, (w, h))],
    }
}

/// The sharp outline of a parametric shape with its corner radii handed over as live corners (spec 55),
/// so pressing A on a rounded rectangle or filleted polygon keeps the rounding editable instead of baking
/// it into arcs. Shapes with no corner fields (Ellipse, Arc, Ring, Line, Plus) return `None`, so the
/// caller falls back to their baked outline. The corner order matches the sharp contour's nodes.
pub fn to_corner_path(params: &Params, size: Pt) -> Option<(Vec<Contour>, Vec<Vec<crate::corners::Corner>>)> {
    use crate::corners::Corner;
    let round = |r: f32| Corner::round(r.max(0.0));
    match params {
        Params::Rectangle { corners, .. } => {
            let (w, h) = size;
            let sharp = poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
            let [tl, tr, br, bl] = *corners;
            Some((vec![sharp], vec![vec![round(tl), round(tr), round(br), round(bl)]]))
        }
        Params::Polygon { sides, corner, turn } => {
            let n = (*sides).clamp(MIN_SIDES, MAX_SIDES);
            let pts = fit(&regular(n, *turn), size);
            let cs = vec![round(*corner); pts.len()];
            Some((vec![poly(&pts)], vec![cs]))
        }
        Params::Star { points, inner, corner, inner_corner, turn } => {
            let n = (*points).clamp(MIN_SIDES, MAX_SIDES);
            let inner = inner.clamp(0.0, 1.0);
            let unit: Vec<Pt> = (0..n * 2)
                .map(|i| {
                    let r = if i % 2 == 0 { 1.0 } else { inner };
                    polar(turn + 360.0 * i as f32 / (n * 2) as f32, r)
                })
                .collect();
            let pts = fit(&unit, size);
            let cs = (0..pts.len()).map(|i| round(if i % 2 == 0 { *corner } else { *inner_corner })).collect();
            Some((vec![poly(&pts)], vec![cs]))
        }
        _ => None,
    }
}

fn rectangle(w: f32, h: f32, corners: [f32; 4]) -> Contour {
    let [tl, tr, br, bl] = corners.map(|r| clamp_corner(r, w, h));
    if corners.iter().all(|&r| r <= 0.0) {
        return Pen::new().to((0.0, 0.0)).to((w, 0.0)).to((w, h)).to((0.0, h)).close();
    }
    // Clockwise from the top edge: each corner is a quarter turn of its own radius.
    Pen::new()
        .to((tl, 0.0))
        .to((w - tr, 0.0))
        .arc((w - tr, tr), tr, -90.0, 0.0)
        .to((w, h - br))
        .arc((w - br, h - br), br, 0.0, 90.0)
        .to((bl, h))
        .arc((bl, h - bl), bl, 90.0, 180.0)
        .to((0.0, tl))
        .arc((tl, tl), tl, 180.0, 270.0)
        .close()
}

/// A regular polygon with `sides` corners, oriented by `turn` degrees, filleted by `corner` inches, then
/// stretched to fill the box.
fn polygon(sides: u32, turn: f32, corner: f32, size: Pt) -> Contour {
    let n = sides.clamp(MIN_SIDES, MAX_SIDES);
    let pts = fit(&regular(n, turn), size);
    fillet(&pts, corner)
}

fn star(points: u32, inner: f32, corner: f32, inner_corner: f32, turn: f32, size: Pt) -> Contour {
    let n = points.clamp(MIN_SIDES, MAX_SIDES);
    let inner = inner.clamp(0.0, 1.0);
    let unit: Vec<Pt> = (0..n * 2)
        .map(|i| {
            let r = if i % 2 == 0 { 1.0 } else { inner };
            polar(turn + 360.0 * i as f32 / (n * 2) as f32, r)
        })
        .collect();
    let pts = fit(&unit, size);
    if corner <= 0.0 && inner_corner <= 0.0 {
        return poly(&pts);
    }
    // Fillet outer and inner points by their own radius.
    fillet_each(&pts, |i| if i % 2 == 0 { corner } else { inner_corner })
}

fn ring(thickness: f32, (w, h): Pt) -> Vec<Contour> {
    let t = thickness.clamp(0.0, 1.0);
    let outer = Pen::new().ellipse_arc((w / 2.0, h / 2.0), (w / 2.0, h / 2.0), -90.0, 270.0).close();
    let (rx, ry) = (w / 2.0 * (1.0 - t), h / 2.0 * (1.0 - t));
    let inner = Pen::new().ellipse_arc((w / 2.0, h / 2.0), (rx, ry), 270.0, -90.0).close();
    vec![outer, inner]
}

/// A sector or donut sector spanning `sweep_deg` from `start_deg`, stretched to fill the box. A pie runs
/// back through the centre (or the inner arc for a donut), a chord cuts straight across, and an open arc
/// is the stroke alone. The arc is drawn as cubics, like a circle, so it flattens as smoothly as the
/// shape it reproduces.
fn arc(start_deg: f32, sweep_deg: f32, inner: f32, close: ArcClose, size: Pt) -> Vec<Contour> {
    let from = start_deg;
    let to = start_deg + sweep_deg;
    let inner = inner.clamp(0.0, 1.0);
    let pen = Pen::new().arc((0.0, 0.0), 1.0, from, to);
    let contour = match close {
        ArcClose::Open => pen.open(),
        ArcClose::Chord => pen.close(),
        ArcClose::Pie if inner > 0.0 => pen.arc((0.0, 0.0), inner, to, from).close(),
        ArcClose::Pie => pen.to((0.0, 0.0)).close(),
    };
    vec![fit_contour(contour, size)]
}

/// Stretches a whole contour, control points included, so its flattened bounding box fills the box. The
/// map is affine, so cubics stay cubics.
fn fit_contour(c: Contour, (w, h): Pt) -> Contour {
    let pts = crate::geometry::flatten(&c, 1e-4);
    let (min_x, min_y) = pts.iter().fold((f32::MAX, f32::MAX), |(a, b), &(x, y)| (a.min(x), b.min(y)));
    let (max_x, max_y) = pts.iter().fold((f32::MIN, f32::MIN), |(a, b), &(x, y)| (a.max(x), b.max(y)));
    let sx = if max_x > min_x { w / (max_x - min_x) } else { 1.0 };
    let sy = if max_y > min_y { h / (max_y - min_y) } else { 1.0 };
    let map = |(x, y): Pt| ((x - min_x) * sx, (y - min_y) * sy);
    Contour {
        start: map(c.start),
        segs: c
            .segs
            .iter()
            .map(|s| match *s {
                Seg::Line(p) => Seg::Line(map(p)),
                Seg::Cubic(a, b, p) => Seg::Cubic(map(a), map(b), map(p)),
            })
            .collect(),
        closed: c.closed,
    }
}

fn plus(bar: f32, (w, h): Pt) -> Contour {
    let bx = (w * bar.clamp(0.0, 1.0)) / 2.0;
    let by = (h * bar.clamp(0.0, 1.0)) / 2.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    poly(&[
        (cx - bx, 0.0),
        (cx + bx, 0.0),
        (cx + bx, cy - by),
        (w, cy - by),
        (w, cy + by),
        (cx + bx, cy + by),
        (cx + bx, h),
        (cx - bx, h),
        (cx - bx, cy + by),
        (0.0, cy + by),
        (0.0, cy - by),
        (cx - bx, cy - by),
    ])
}

// ---- geometry helpers --------------------------------------------------------------------------

fn polar(deg: f32, r: f32) -> Pt {
    let (s, k) = deg.to_radians().sin_cos();
    (r * k, r * s)
}

/// The unit-circle vertices of a regular `n`-gon, vertex zero at `turn` degrees.
fn regular(n: u32, turn: f32) -> Vec<Pt> {
    (0..n).map(|i| polar(turn + 360.0 * i as f32 / n as f32, 1.0)).collect()
}

/// Stretches a point set so its bounding box exactly fills `w` x `h`. A zero-extent axis keeps its
/// coordinate, so a flat figure is not divided by zero.
fn fit(pts: &[Pt], (w, h): Pt) -> Vec<Pt> {
    let (min_x, min_y) = pts.iter().fold((f32::MAX, f32::MAX), |(a, b), &(x, y)| (a.min(x), b.min(y)));
    let (max_x, max_y) = pts.iter().fold((f32::MIN, f32::MIN), |(a, b), &(x, y)| (a.max(x), b.max(y)));
    let sx = if max_x > min_x { w / (max_x - min_x) } else { 1.0 };
    let sy = if max_y > min_y { h / (max_y - min_y) } else { 1.0 };
    pts.iter().map(|&(x, y)| ((x - min_x) * sx, (y - min_y) * sy)).collect()
}

fn poly(pts: &[Pt]) -> Contour {
    poly_open(pts, true)
}

fn poly_open(pts: &[Pt], closed: bool) -> Contour {
    let mut pen = Pen::new();
    for &p in pts {
        pen = pen.to(p);
    }
    if closed { pen.close() } else { pen.open() }
}

/// Fillets every corner of a closed polygon to the one radius `r`, in inches.
fn fillet(pts: &[Pt], r: f32) -> Contour {
    if r <= 0.0 {
        return poly(pts);
    }
    fillet_each(pts, |_| r)
}

/// Fillets each corner by a per-corner radius, clamped to half its shorter edge so rounds never overlap.
fn fillet_each(pts: &[Pt], radius: impl Fn(usize) -> f32) -> Contour {
    let n = pts.len();
    let dist = |a: Pt, b: Pt| (a.0 - b.0).hypot(a.1 - b.1);
    let unit = |a: Pt, b: Pt| {
        let d = dist(a, b).max(1e-6);
        ((b.0 - a.0) / d, (b.1 - a.1) / d)
    };
    let mut pen = Pen::new();
    for i in 0..n {
        let p = pts[i];
        let (prev, next) = (pts[(i + n - 1) % n], pts[(i + 1) % n]);
        let r = radius(i).clamp(0.0, 0.5 * dist(p, prev).min(dist(p, next)));
        if r <= 1e-6 {
            pen = pen.to(p);
            continue;
        }
        let (u, v) = (unit(p, prev), unit(p, next));
        let half = (u.0 * v.0 + u.1 * v.1).clamp(-1.0, 1.0).acos() / 2.0;
        let (bx, by) = unit((0.0, 0.0), (u.0 + v.0, u.1 + v.1));
        let (along, out) = (r / half.tan(), r / half.sin());
        let c = (p.0 + bx * out, p.1 + by * out);
        let start = angle(c, (p.0 + u.0 * along, p.1 + u.1 * along));
        let end = (angle(c, (p.0 + v.0 * along, p.1 + v.1 * along)) - start + 540.0).rem_euclid(360.0) - 180.0;
        pen = pen.arc(c, r, start, start + end);
    }
    pen.close()
}

fn angle(c: Pt, p: Pt) -> f32 {
    (p.1 - c.1).atan2(p.0 - c.0).to_degrees()
}

// ---- a tiny pen that builds a `Contour` out of lines and cubic arcs ----------------------------

struct Pen {
    start: Option<Pt>,
    at: Pt,
    segs: Vec<Seg>,
}

fn near(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
}

impl Pen {
    fn new() -> Self {
        Self { start: None, at: (0.0, 0.0), segs: Vec::new() }
    }

    fn to(mut self, p: Pt) -> Self {
        match self.start {
            None => self.start = Some(p),
            Some(_) if near(self.at, p) => {}
            Some(_) => self.segs.push(Seg::Line(p)),
        }
        self.at = p;
        self
    }

    fn curve(mut self, c1: Pt, c2: Pt, p: Pt) -> Self {
        self.segs.push(Seg::Cubic(c1, c2, p));
        self.at = p;
        self
    }

    /// An elliptical arc in degrees, as cubics of at most 90 degrees each.
    fn ellipse_arc(mut self, c: Pt, (rx, ry): Pt, from: f32, to: f32) -> Self {
        let at = |deg: f32| {
            let (s, k) = deg.to_radians().sin_cos();
            ((c.0 + rx * k, c.1 + ry * s), (-rx * s, ry * k))
        };
        self = self.to(at(from).0);
        let n = ((to - from).abs() / 90.0).ceil().max(1.0) as usize;
        let step = (to - from) / n as f32;
        let k = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
        for i in 0..n {
            let ((p0, d0), (p1, d1)) = (at(from + step * i as f32), at(from + step * (i + 1) as f32));
            self = self.curve((p0.0 + k * d0.0, p0.1 + k * d0.1), (p1.0 - k * d1.0, p1.1 - k * d1.1), p1);
        }
        self
    }

    fn arc(self, c: Pt, r: f32, from: f32, to: f32) -> Self {
        self.ellipse_arc(c, (r, r), from, to)
    }

    fn close(mut self) -> Contour {
        let start = self.start.expect("a contour draws something");
        if let Some(Seg::Line(p) | Seg::Cubic(_, _, p)) = self.segs.last_mut()
            && near(*p, start)
        {
            *p = start;
        }
        Contour { start, segs: self.segs, closed: true }
    }

    fn open(self) -> Contour {
        Contour { start: self.start.expect("a contour draws something"), segs: self.segs, closed: false }
    }
}

// ---- the preset table: every fixed kind reproduced ---------------------------------------------

use crate::design::ShapeKind;

/// The parametric preset that reproduces fixed `kind` at `size`, or `None` for a kind that keeps its own
/// representation (Heart, Equals, the free built-ins, and every non-free kind). Corner radii are read off
/// the clean-room SVGs (72 units per inch) and scaled to `size`.
pub fn from_fixed(kind: &ShapeKind, (w, _h): Pt) -> Option<Params> {
    // A fraction of the native width turned into inches at the current width.
    let by_w = |frac: f32| frac * w;
    Some(match kind {
        ShapeKind::Rectangle | ShapeKind::Square => Params::Rectangle { corners: [0.0; 4], linked: true, scale_with_shape: false },
        ShapeKind::RoundedSquare => {
            let r = by_w(13.0 / 143.9);
            Params::Rectangle { corners: [r; 4], linked: true, scale_with_shape: false }
        }
        ShapeKind::Pill => {
            let r = by_w(60.0 / 180.0);
            Params::Rectangle { corners: [r; 4], linked: true, scale_with_shape: false }
        }
        ShapeKind::Arch => {
            let r = by_w(59.6 / 119.2);
            Params::Rectangle { corners: [0.0, 0.0, r, r], linked: false, scale_with_shape: false }
        }
        ShapeKind::Circle | ShapeKind::Oval => Params::Ellipse,
        ShapeKind::Triangle => Params::Polygon { sides: 3, corner: 0.0, turn: -90.0 },
        ShapeKind::TriangleDown => Params::Polygon { sides: 3, corner: 0.0, turn: 90.0 },
        ShapeKind::Diamond => Params::Polygon { sides: 4, corner: 0.0, turn: -90.0 },
        ShapeKind::Pentagon => Params::Polygon { sides: 5, corner: 0.0, turn: -90.0 },
        ShapeKind::Hexagon => Params::Polygon { sides: 6, corner: 0.0, turn: 0.0 },
        ShapeKind::Octagon => Params::Polygon { sides: 8, corner: 0.0, turn: 22.5 },
        ShapeKind::Star => Params::Star { points: 5, inner: 0.50602, corner: 0.0, inner_corner: 0.0, turn: -90.0 },
        ShapeKind::StarThin => Params::Star { points: 5, inner: 0.39001, corner: 0.0, inner_corner: 0.0, turn: -90.0 },
        ShapeKind::HalfCircle => Params::Arc { start_deg: 90.0, sweep_deg: 180.0, inner: 0.0, close: ArcClose::Chord },
        ShapeKind::Plus => Params::Plus { bar: 25.2 / 144.0 },
        ShapeKind::Line => Params::Line,
        _ => return None,
    })
}

/// Scales a parametric shape's size-following fields (a Rectangle's corner radii when Scale with shape
/// is on) by the width factor `fx` of a resize. A no-op for every other kind, so callers need not test.
pub fn rescale(kind: &mut ShapeKind, fx: f32) {
    if let ShapeKind::Parametric(params) = kind {
        *params = params.resized(fx);
    }
}

/// `kind` as a live parametric shape at `size`, when it is a free shape with a preset; otherwise `kind`
/// unchanged. The Shapes panel and `Design::add_shape` route new free shapes through this so they are
/// editable after placing, while Heart, Equals, the built-ins and every other kind keep their own form.
pub fn insert_as(kind: ShapeKind, size: Pt) -> ShapeKind {
    match from_fixed(&kind, size) {
        Some(params) => ShapeKind::Parametric(params),
        None => kind,
    }
}

/// The box a fresh parametric shape of `kind` starts in, matching the free shape it most resembles.
pub fn default_size_in(kind: ParamKind) -> Pt {
    match kind {
        ParamKind::Rectangle => (2.0, 1.6667),
        ParamKind::Ellipse => (2.0, 2.0),
        ParamKind::Polygon => (2.0, 1.7319),
        ParamKind::Star => (2.0, 1.9028),
        ParamKind::Arc => (2.0, 2.0),
        ParamKind::Ring => (2.0, 2.0),
        ParamKind::Line => (0.0, 2.5),
        ParamKind::Plus => (2.0, 2.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    fn flat(contours: &[Contour]) -> Vec<Pt> {
        contours.iter().flat_map(|c| crate::geometry::flatten(c, 1e-4)).collect()
    }

    /// The directed Hausdorff distance from `a` to `b`: the farthest any point of `a` sits from `b`.
    fn directed(a: &[Pt], b: &[Pt]) -> f32 {
        a.iter().map(|p| b.iter().map(|q| (p.0 - q.0).hypot(p.1 - q.1)).fold(f32::MAX, f32::min)).fold(0.0, f32::max)
    }

    fn hausdorff(a: &[Pt], b: &[Pt]) -> f32 {
        directed(a, b).max(directed(b, a))
    }

    #[test]
    fn every_fixed_kind_upgrades_to_a_matching_preset() {
        let fixed = [
            ShapeKind::Line,
            ShapeKind::TriangleDown,
            ShapeKind::Triangle,
            ShapeKind::Diamond,
            ShapeKind::Star,
            ShapeKind::StarThin,
            ShapeKind::Octagon,
            ShapeKind::Pentagon,
            ShapeKind::Hexagon,
            ShapeKind::Plus,
            ShapeKind::Circle,
            ShapeKind::Rectangle,
            ShapeKind::Square,
            ShapeKind::Pill,
            ShapeKind::RoundedSquare,
            ShapeKind::Oval,
            ShapeKind::Arch,
            ShapeKind::HalfCircle,
        ];
        for kind in fixed {
            let (w, h) = kind.default_size_in();
            let params = from_fixed(&kind, (w, h)).unwrap_or_else(|| panic!("{} has no preset", kind.label()));
            let want = flat(&kind.outline(w, h));
            let got = flat(&outline(&params, (w, h)));
            let d = hausdorff(&want, &got);
            assert!(d < 0.01, "{}: Hausdorff {:.4} over tolerance", kind.label(), d);
        }
    }

    #[test]
    fn a_corner_radius_clamps_to_half_the_shorter_edge() {
        let params = Params::Rectangle { corners: [0.2; 4], linked: true, scale_with_shape: false };
        // A 0.2 in radius on a 1 x 0.3 in frame can only round to half the 0.3 in side, so the outline is
        // exactly the one a 0.15 in radius draws.
        let asked = outline(&params, (1.0, 0.3));
        let clamped = outline(&Params::Rectangle { corners: [0.15; 4], linked: true, scale_with_shape: false }, (1.0, 0.3));
        assert_eq!(asked, clamped, "radius clamps to half the shorter side");
        // And a hair over 0.15 would differ, proving the clamp is doing the work.
        let under = outline(&Params::Rectangle { corners: [0.14; 4], linked: true, scale_with_shape: false }, (1.0, 0.3));
        assert_ne!(asked, under);
    }

    #[test]
    fn a_seven_point_star_has_fourteen_alternating_nodes() {
        let params = Params::Star { points: 7, inner: 0.4, corner: 0.0, inner_corner: 0.0, turn: -90.0 };
        let c = &outline(&params, (2.0, 2.0))[0];
        let pts = c.flatten();
        assert_eq!(pts.len(), 14, "a 7 point star draws 14 nodes");
        let centre = (1.0, 1.0);
        let radius = |p: &Pt| (p.0 - centre.0).hypot(p.1 - centre.1);
        let radii: Vec<f32> = pts.iter().map(radius).collect();
        // Each outer point (even index) sits farther out than both of its inner neighbours.
        let n = radii.len();
        for i in (0..n).step_by(2) {
            assert!(radii[i] > radii[(i + 1) % n], "node {i} is an outer point");
            assert!(radii[i] > radii[(i + n - 1) % n], "node {i} is an outer point");
        }
    }

    #[test]
    fn an_arc_closes_as_a_pie_a_chord_or_an_open_line() {
        let size = (2.0, 2.0);
        let pie = &arc(0.0, 270.0, 0.0, ArcClose::Pie, size)[0];
        let chord = &arc(0.0, 270.0, 0.0, ArcClose::Chord, size)[0];
        let open = &arc(0.0, 270.0, 0.0, ArcClose::Open, size)[0];
        let has_centre = |c: &Contour| c.flatten().iter().any(|p| (p.0 - 1.0).hypot(p.1 - 1.0) < 1e-3);
        assert!(has_centre(pie), "a pie runs through the centre");
        assert!(!has_centre(chord), "a chord cuts straight across");
        assert!(!open.closed, "an open arc is not closed");
        assert!(pie.closed && chord.closed);
    }

    #[test]
    fn scale_with_shape_decides_whether_a_radius_follows_the_box() {
        let off = Params::Rectangle { corners: [0.2; 4], linked: true, scale_with_shape: false };
        let on = Params::Rectangle { corners: [0.2; 4], linked: true, scale_with_shape: true };
        assert_eq!(off.resized(2.0), off, "off keeps the radius unchanged");
        match on.resized(2.0) {
            Params::Rectangle { corners, .. } => assert!((corners[0] - 0.4).abs() < 1e-6, "doubling the width doubles the radius"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_rounded_rectangle_converts_to_four_round_corners_matching_its_outline() {
        use crate::corners::{CornerStyle, with_corners};
        let params = Params::Rectangle { corners: [0.3; 4], linked: true, scale_with_shape: false };
        let size = (2.0, 1.5);
        let (contours, corners) = to_corner_path(&params, size).expect("a rectangle converts to a path");
        assert_eq!(corners[0].len(), 4, "one corner per node");
        assert!(corners[0].iter().all(|c| c.style == CornerStyle::Round && (c.radius_in - 0.3).abs() < 1e-6), "four Round corners of the radius: {:?}", corners[0]);
        // The outline the live corners draw matches the baked parametric rectangle within 0.01 in.
        let live = with_corners(&contours[0], &corners[0]);
        let baked = &outline(&params, size)[0];
        let (a, b) = (crate::geometry::flatten(&live, 1e-3), crate::geometry::flatten(baked, 1e-3));
        let one_sided = |p: &[Pt], q: &[Pt]| p.iter().map(|&x| q.iter().map(|&y| (x.0 - y.0).hypot(x.1 - y.1)).fold(f32::MAX, f32::min)).fold(0.0, f32::max);
        assert!(one_sided(&a, &b).max(one_sided(&b, &a)) < 0.01, "live outline matches the baked one");
    }
}
