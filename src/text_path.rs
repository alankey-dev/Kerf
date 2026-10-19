//! Lays a single line of text along an arbitrary path, the general form of the circle bend in
//! `text::curve`. The text keeps its own copy of the guide path (`TextPath::contours`, in the text's
//! local inch frame), so it stays whole when the shape it was taken from is deleted.
//!
//! [`place`] turns each glyph's straight cell onto the path: its baseline centre sits at the glyph's
//! advance midpoint measured as arc length, turned to the path tangent there. This is exactly the turn
//! `text::curve` applies, with the circle's analytic point and tangent replaced by a sampler over the
//! path's own line and cubic segments, so a circle path reproduces the old curve layout.

use crate::design::{Contour, Pt, Seg};
use crate::text::{GlyphOutline, TextLayout};

/// Which side of the path the text sits on. `Above` keeps it upright on the outside of a bend; `Below`
/// mirrors it across the path to the other side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Side {
    #[default]
    Above,
    Below,
}

/// Where the run sits along an open path. `Start` begins at the path start; `Centre` centres the run
/// between start and end; `End` ends the last glyph at the path end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Along {
    #[default]
    Start,
    Centre,
    End,
}

/// A text's own copy of the path it follows. The contours are in the text's local inch frame, sized by
/// `view_box`; `offset_in` slides the run's start along the path.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextPath {
    pub contours: Vec<Contour>,
    pub view_box: Pt,
    #[serde(default)]
    pub offset_in: f32,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub align: Along,
}

/// How finely each cubic is sampled when building the arc-length table. A whole circle built from eight
/// cubic arcs is then within about 1e-6 in of true per sample, well under the layout tolerances.
const CUBIC_SAMPLES: usize = 32;

impl TextPath {
    /// The circle `text::curve` bends onto, as a path: a positive diameter puts the text on top, a
    /// negative one underneath, matching the signed `curve_in`. The run centres on the circle, as the
    /// Curve panel has always placed it.
    pub fn circle(diameter_in: f32) -> TextPath {
        let radius = diameter_in.abs() / 2.0;
        let side = diameter_in.signum();
        // to(theta) matches text::curve: the run centre sits at the top (theta 0), the circle centre is
        // at (0, side*radius), and theta runs from -pi (the far point) back round to pi, so arc length
        // grows with theta and the top lands at half the circumference.
        let to = |theta: f32| (radius * theta.sin(), side * radius * (1.0 - theta.cos()));
        let arcs = 8;
        let step = std::f32::consts::TAU / arcs as f32;
        // A cubic arc of angle `step` approximates the circle with handles of length k*radius.
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let start = to(-std::f32::consts::PI);
        let mut segs = Vec::with_capacity(arcs);
        for i in 0..arcs {
            let a = -std::f32::consts::PI + i as f32 * step;
            let b = a + step;
            let (pa, pb) = (to(a), to(b));
            // The unit tangents at the ends, in the direction of travel, scaled to the handle length.
            let ta = (radius * a.cos(), side * radius * a.sin());
            let tb = (radius * b.cos(), side * radius * b.sin());
            let norm = |(x, y): Pt| {
                let len = x.hypot(y);
                if len > 1e-9 { (x / len, y / len) } else { (0.0, 0.0) }
            };
            let (ua, ub) = (norm(ta), norm(tb));
            let h = k * radius;
            let c1 = (pa.0 + ua.0 * h, pa.1 + ua.1 * h);
            let c2 = (pb.0 - ub.0 * h, pb.1 - ub.1 * h);
            segs.push(Seg::Cubic(c1, c2, pb));
        }
        TextPath {
            contours: vec![Contour { start, segs, closed: true }],
            view_box: (2.0 * radius, 2.0 * radius),
            offset_in: 0.0,
            side: Side::Above,
            align: Along::Centre,
        }
    }

    /// An arc-length view of this path, for the canvas handles and node reflow. A query gives a point
    /// and tangent at an arc length, or the nearest arc length to a point with which side it fell on.
    pub fn trace(&self) -> Trace {
        let sampler = Sampler::new(&self.contours);
        Trace { total: sampler.total, sampler }
    }
}

/// A read-only arc-length view of a [`TextPath`]'s contours. All points are in the path's own frame,
/// before [`place`]'s `shift`.
pub struct Trace {
    sampler: Sampler,
    pub total: f32,
}

impl Trace {
    /// The point and tangent angle at arc length `s`.
    pub fn point_at(&self, s: f32) -> (Pt, f32) {
        let (p, a, _) = self.sampler.at(s);
        (p, a)
    }

    /// The arc length of the point on the path nearest `q`, and a signed measure of the side `q` lies on
    /// (the forward tangent crossed into `q - p`), so a drag across the path flips side at zero.
    pub fn project(&self, q: Pt) -> (f32, f32) {
        const STEPS: usize = 256;
        let dist2 = |s: f32| {
            let (p, _, _) = self.sampler.at(s);
            (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)
        };
        let coarse = (0..=STEPS).map(|i| i as f32 / STEPS as f32 * self.total).min_by(|&a, &b| dist2(a).total_cmp(&dist2(b))).unwrap_or(0.0);
        let span = self.total / STEPS as f32;
        let (mut lo, mut hi) = (coarse - span, coarse + span);
        for _ in 0..40 {
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if dist2(m1) < dist2(m2) { hi = m2 } else { lo = m1 }
        }
        let s = ((lo + hi) / 2.0).clamp(0.0, self.total);
        let (p, a, _) = self.sampler.at(s);
        let (sin, cos) = a.sin_cos();
        // The forward tangent crossed into (q - p): positive turns left, negative right.
        let side = cos * (q.1 - p.1) - sin * (q.0 - p.0);
        (s, side)
    }
}

/// How a laid-out path text sits on its path, in the path's own frame, so the canvas can draw the
/// guide line and place the start and centre handles over the letters. `shift` is the translation
/// [`place`] applied to the layout, `base` the arc length the run starts at and `run_width` the
/// straight run's advance width; the centre handle sits at `base + run_width / 2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathFrame {
    pub shift: Pt,
    pub base: f32,
    pub run_width: f32,
}

/// One straight glyph cell: the character it belongs to, its pen x on the baseline and its advance.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub char_index: usize,
    pub x: f32,
    pub advance: f32,
}

/// Lays `straight`'s glyphs along `path`. `width` is the straight line's advance width, `ascender` the
/// baseline's y in the straight box and `line_h` the cell height. The result's box is refitted to the
/// turned glyphs, and `overflow` is set when the run runs off the end of an open path.
pub fn place(straight: TextLayout, path: &TextPath, cells: &[Cell], ascender: f32, line_h: f32, width: f32) -> TextLayout {
    let sampler = Sampler::new(&path.contours);
    let base = run_start(sampler.total, width, path.align, path.offset_in);
    let flip = path.side == Side::Below;
    // Each cell's turn onto the path and the corners of its turned cell, for the box.
    let mut overflow = false;
    let placed: Vec<(usize, Turn, [Pt; 4])> = cells
        .iter()
        .map(|cell| {
            let mid = cell.x + cell.advance / 2.0;
            let (point, angle, off) = sampler.at(base + mid);
            overflow |= off;
            let turn = Turn { angle, flip, from: (mid, ascender), to: point };
            let corners = [(cell.x, 0.0), (cell.x + cell.advance, 0.0), (cell.x, line_h), (cell.x + cell.advance, line_h)].map(|p| turn.apply(p, ascender));
            (cell.char_index, turn, corners)
        })
        .collect();
    let corners = placed.iter().flat_map(|c| c.2);
    let (min, max) = corners.fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), p| ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1))));
    let glyphs = straight
        .glyphs
        .into_iter()
        .map(|g| {
            let turn = placed.iter().find(|c| c.0 == g.char_index).expect("every glyph has a cell").1;
            let place = |p: Pt| {
                let q = turn.apply(p, ascender);
                (q.0 - min.0, q.1 - min.1)
            };
            let contours = g.contours.iter().map(|c| map_contour(c, &place)).collect();
            let angle = if flip { -turn.angle } else { turn.angle };
            GlyphOutline { contours, pen: place(g.pen), angle, ..g }
        })
        .collect();
    let size = ((max.0 - min.0).max(0.01), (max.1 - min.1).max(0.01));
    let frame = PathFrame { shift: min, base, run_width: width };
    TextLayout { glyphs, lines: straight.lines, size, overflow, path: Some(frame) }
}

/// The arc length the run starts at. `Start` begins at the path start, `Centre` centres the run between
/// start and end, `End` ends it at the path end; `offset_in` then slides that start along the path.
pub fn run_start(total: f32, width: f32, along: Along, offset: f32) -> f32 {
    let base = match along {
        Along::Start => 0.0,
        Along::Centre => (total - width) / 2.0,
        Along::End => total - width,
    };
    base + offset
}

/// A rotation by `angle` (clockwise on the canvas) carrying `from` onto `to`, optionally mirroring the
/// cell across its baseline first so the text sits on the other side of the path.
#[derive(Clone, Copy)]
struct Turn {
    angle: f32,
    flip: bool,
    from: Pt,
    to: Pt,
}

impl Turn {
    fn apply(self, (x, y): Pt, baseline: f32) -> Pt {
        let y = if self.flip { 2.0 * baseline - y } else { y };
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (x - self.from.0, y - self.from.1);
        (self.to.0 + dx * cos - dy * sin, self.to.1 + dx * sin + dy * cos)
    }
}

fn map_contour(c: &Contour, f: &impl Fn(Pt) -> Pt) -> Contour {
    let segs = c
        .segs
        .iter()
        .map(|s| match *s {
            Seg::Line(p) => Seg::Line(f(p)),
            Seg::Cubic(a, b, p) => Seg::Cubic(f(a), f(b), f(p)),
        })
        .collect();
    Contour { start: f(c.start), segs, closed: c.closed }
}

/// An arc-length parameterisation of a path's contours. Each segment keeps its analytic form so a query
/// returns a point exactly on the path and a tangent from the true derivative, with a fine sub-table on
/// cubics to invert arc length to the curve parameter.
struct Sampler {
    segs: Vec<Walk>,
    total: f32,
    closed: bool,
}

enum Walk {
    Line { a: Pt, b: Pt, start: f32, len: f32 },
    Cubic { p: [Pt; 4], start: f32, len: f32, table: Vec<(f32, f32)> },
}

impl Sampler {
    fn new(contours: &[Contour]) -> Sampler {
        let mut segs = Vec::new();
        let mut total = 0.0;
        let mut closed = !contours.is_empty();
        for c in contours {
            closed &= c.closed;
            let mut from = c.start;
            for seg in &c.segs {
                match *seg {
                    Seg::Line(to) => {
                        let len = (to.0 - from.0).hypot(to.1 - from.1);
                        segs.push(Walk::Line { a: from, b: to, start: total, len });
                        total += len;
                        from = to;
                    }
                    Seg::Cubic(c1, c2, to) => {
                        let p = [from, c1, c2, to];
                        let mut table = Vec::with_capacity(CUBIC_SAMPLES + 1);
                        let mut len = 0.0;
                        let mut prev = from;
                        table.push((0.0, 0.0));
                        for i in 1..=CUBIC_SAMPLES {
                            let t = i as f32 / CUBIC_SAMPLES as f32;
                            let pt = cubic_at(p, t);
                            len += (pt.0 - prev.0).hypot(pt.1 - prev.1);
                            table.push((len, t));
                            prev = pt;
                        }
                        segs.push(Walk::Cubic { p, start: total, len, table });
                        total += len;
                        from = to;
                    }
                }
            }
        }
        Sampler { segs, total: total.max(1e-6), closed }
    }

    /// The point and tangent angle at arc length `s`, and whether `s` fell off an open path. A closed
    /// path wraps; an open one extrapolates along its end tangent.
    fn at(&self, s: f32) -> (Pt, f32, bool) {
        if self.closed {
            return self.on_path(s.rem_euclid(self.total));
        }
        if s < 0.0 {
            let (p, a, _) = self.on_path(0.0);
            return (along(p, a, s), a, true);
        }
        if s > self.total {
            let (p, a, _) = self.on_path(self.total);
            return (along(p, a, s - self.total), a, true);
        }
        self.on_path(s)
    }

    fn on_path(&self, s: f32) -> (Pt, f32, bool) {
        let seg = self.segs.iter().find(|w| s <= w.end()).or_else(|| self.segs.last()).expect("a path has at least one segment");
        match seg {
            Walk::Line { a, b, start, len } => {
                let t = if *len > 1e-9 { (s - start) / len } else { 0.0 };
                let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                (p, (b.1 - a.1).atan2(b.0 - a.0), false)
            }
            Walk::Cubic { p, start, table, .. } => {
                let t = invert(table, s - start);
                (cubic_at(*p, t), cubic_angle(*p, t), false)
            }
        }
    }
}

impl Walk {
    fn end(&self) -> f32 {
        match self {
            Walk::Line { start, len, .. } | Walk::Cubic { start, len, .. } => start + len,
        }
    }
}

/// A point `d` beyond `p` along the tangent at angle `a` (d negative goes back before it).
fn along(p: Pt, a: f32, d: f32) -> Pt {
    let (sin, cos) = a.sin_cos();
    (p.0 + cos * d, p.1 + sin * d)
}

/// The curve parameter at arc length `len` into a cubic, by linear interpolation of its sub-table.
fn invert(table: &[(f32, f32)], len: f32) -> f32 {
    if len <= 0.0 {
        return 0.0;
    }
    for pair in table.windows(2) {
        let (l0, t0) = pair[0];
        let (l1, t1) = pair[1];
        if len <= l1 {
            let f = if l1 > l0 { (len - l0) / (l1 - l0) } else { 0.0 };
            return t0 + (t1 - t0) * f;
        }
    }
    1.0
}

fn cubic_at(p: [Pt; 4], t: f32) -> Pt {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * p[0].0 + b * p[1].0 + c * p[2].0 + d * p[3].0, a * p[0].1 + b * p[1].1 + c * p[2].1 + d * p[3].1)
}

fn cubic_angle(p: [Pt; 4], t: f32) -> f32 {
    let u = 1.0 - t;
    let dx = 3.0 * (u * u * (p[1].0 - p[0].0) + 2.0 * u * t * (p[2].0 - p[1].0) + t * t * (p[3].0 - p[2].0));
    let dy = 3.0 * (u * u * (p[1].1 - p[0].1) + 2.0 * u * t * (p[2].1 - p[1].1) + t * t * (p[3].1 - p[2].1));
    dy.atan2(dx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{GlyphOutline, TextSpec, layout};
    use std::f32::consts::PI;

    /// A circular arc of `n` cubic sub-arcs, radius `r`, from angle `a0` to `a1` about `centre`, as an
    /// open contour. The same construction `TextPath::circle` uses, so a sampled point lies on the true
    /// circle to about 1e-6 in.
    fn arc(centre: Pt, r: f32, a0: f32, a1: f32, n: usize) -> Contour {
        let pt = |a: f32| (centre.0 + r * a.cos(), centre.1 + r * a.sin());
        let step = (a1 - a0) / n as f32;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let mut segs = Vec::new();
        for i in 0..n {
            let a = a0 + i as f32 * step;
            let b = a + step;
            let (pa, pb) = (pt(a), pt(b));
            let (ta, tb) = ((-a.sin(), a.cos()), (-b.sin(), b.cos()));
            let h = k * r;
            segs.push(Seg::Cubic((pa.0 + ta.0 * h, pa.1 + ta.1 * h), (pb.0 - tb.0 * h, pb.1 - tb.1 * h), pb));
        }
        Contour { start: pt(a0), segs, closed: false }
    }

    /// The smallest signed difference between two angles, in -pi..pi.
    fn ang_diff(a: f32, b: f32) -> f32 {
        let mut d = a - b;
        while d > PI {
            d -= 2.0 * PI;
        }
        while d < -PI {
            d += 2.0 * PI;
        }
        d
    }

    /// A synthetic straight layout with one glyph per cell, its pen at the cell's baseline centre and a
    /// single contour point `ink` carried with it, so a placed pen is exactly the baseline centre.
    fn synth(cells: &[Cell], ascender: f32, line_h: f32, ink: Pt) -> TextLayout {
        let glyphs = cells
            .iter()
            .map(|c| GlyphOutline {
                contours: vec![Contour { start: ink, segs: Vec::new(), closed: false }],
                line: 0,
                char_index: c.char_index,
                pen: (c.x + c.advance / 2.0, ascender),
                angle: 0.0,
            })
            .collect();
        let width = cells.last().map_or(0.0, |c| c.x + c.advance);
        TextLayout { glyphs, lines: Vec::new(), size: (width, line_h), overflow: false, path: None }
    }

    fn text_path(contours: Vec<Contour>, side: Side, align: Along, offset_in: f32) -> TextPath {
        TextPath { contours, view_box: (1.0, 1.0), offset_in, side, align }
    }

    #[test]
    fn circle_text_path_matches_the_old_curve_within_a_thou() {
        let curved = layout(&TextSpec { content: "Spectrum", curve_in: 3.0, ..TextSpec::default() }).unwrap();
        let circle = TextPath::circle(3.0);
        let viapath = layout(&TextSpec { content: "Spectrum", path: Some(&circle), ..TextSpec::default() }).unwrap();
        assert_eq!(curved.glyphs.len(), viapath.glyphs.len());
        for (a, b) in curved.glyphs.iter().zip(&viapath.glyphs) {
            assert!((a.pen.0 - b.pen.0).abs() < 1e-3 && (a.pen.1 - b.pen.1).abs() < 1e-3, "pen {:?} vs {:?}", a.pen, b.pen);
            assert!(ang_diff(a.angle, b.angle).abs() < 1e-3, "angle {} vs {}", a.angle, b.angle);
        }
        assert!((curved.size.0 - viapath.size.0).abs() < 1e-3 && (curved.size.1 - viapath.size.1).abs() < 1e-3, "{:?} vs {:?}", curved.size, viapath.size);
        let negative = TextPath::circle(-3.0);
        let under = layout(&TextSpec { content: "Spectrum", path: Some(&negative), ..TextSpec::default() }).unwrap();
        let old_under = layout(&TextSpec { content: "Spectrum", curve_in: -3.0, ..TextSpec::default() }).unwrap();
        for (a, b) in old_under.glyphs.iter().zip(&under.glyphs) {
            assert!((a.pen.0 - b.pen.0).abs() < 1e-3 && (a.pen.1 - b.pen.1).abs() < 1e-3, "under pen {:?} vs {:?}", a.pen, b.pen);
        }
    }

    #[test]
    fn a_straight_line_path_lays_out_as_straight_text() {
        let straight = layout(&TextSpec { content: "Hello", ..TextSpec::default() }).unwrap();
        let baseline = straight.glyphs[0].pen.1;
        let line = Contour { start: (0.0, baseline), segs: vec![Seg::Line((straight.size.0 + 1.0, baseline))], closed: false };
        let path = text_path(vec![line], Side::Above, Along::Start, 0.0);
        let on_line = layout(&TextSpec { content: "Hello", path: Some(&path), ..TextSpec::default() }).unwrap();
        assert!(!on_line.overflow);
        assert_eq!(straight.glyphs.len(), on_line.glyphs.len());
        for (a, b) in straight.glyphs.iter().zip(&on_line.glyphs) {
            assert!((a.pen.0 - b.pen.0).abs() < 1e-4 && (a.pen.1 - b.pen.1).abs() < 1e-4, "pen {:?} vs {:?}", a.pen, b.pen);
            assert!(b.angle.abs() < 1e-5, "straight has no turn: {}", b.angle);
            let (ap, bp): (Vec<Pt>, Vec<Pt>) = (a.contours.iter().flat_map(Contour::flatten).collect(), b.contours.iter().flat_map(Contour::flatten).collect());
            assert_eq!(ap.len(), bp.len());
            assert!(ap.iter().zip(&bp).all(|(p, q)| (p.0 - q.0).abs() < 1e-4 && (p.1 - q.1).abs() < 1e-4), "ink moved");
        }
    }

    #[test]
    fn a_semicircle_puts_every_baseline_centre_on_the_arc_at_the_tangent() {
        let r = 2.0;
        let sampler = Sampler::new(&[arc((0.0, 0.0), r, 0.0, PI, 4)]);
        assert!(!sampler.closed);
        assert!((sampler.total - PI * r).abs() < 1e-3, "half circumference {} vs {}", sampler.total, PI * r);
        for frac in [0.05, 0.25, 0.5, 0.75, 0.95] {
            let (p, angle, off) = sampler.at(frac * sampler.total);
            assert!(!off);
            assert!((p.0.hypot(p.1) - r).abs() < 1e-4, "point {p:?} is {} from the centre, not {r}", p.0.hypot(p.1));
            let expected = p.1.atan2(p.0) + PI / 2.0;
            assert!(ang_diff(angle, expected).abs() < 1e-3, "tangent {angle} vs {expected}");
        }
    }

    #[test]
    fn a_run_longer_than_an_open_path_overflows_past_the_end_on_the_tangent() {
        let line = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((4.0, 0.0))], closed: false };
        let sampler = Sampler::new(&[line]);
        assert!((sampler.total - 4.0).abs() < 1e-6);
        let (p, angle, off) = sampler.at(5.5);
        assert!(off && (p.0 - 5.5).abs() < 1e-4 && p.1.abs() < 1e-4 && angle.abs() < 1e-4, "{p:?} {angle}");
        let (back, _, before) = sampler.at(-1.0);
        assert!(before && (back.0 + 1.0).abs() < 1e-4);
        // place reports the overflow when the run is wider than the path.
        let cells: Vec<Cell> = (0..6).map(|i| Cell { char_index: i, x: i as f32, advance: 1.0 }).collect();
        let path = text_path(vec![Contour { start: (0.0, 0.5), segs: vec![Seg::Line((4.0, 0.5))], closed: false }], Side::Above, Along::Start, 0.0);
        let laid = place(synth(&cells, 0.5, 1.0, (0.0, 0.0)), &path, &cells, 0.5, 1.0, 6.0);
        assert!(laid.overflow, "a six inch run on a four inch path overflows");
    }

    #[test]
    fn flipping_side_mirrors_the_glyphs_across_the_path() {
        let (asc, line_h) = (0.8, 1.0);
        let cells = [Cell { char_index: 0, x: 0.0, advance: 1.0 }];
        let line = Contour { start: (0.0, asc), segs: vec![Seg::Line((5.0, asc))], closed: false };
        let above = text_path(vec![line], Side::Above, Along::Start, 0.0);
        let below = TextPath { side: Side::Below, ..above.clone() };
        let ink_offset = |p: &TextPath| {
            let laid = place(synth(&cells, asc, line_h, (0.5, asc - 0.3)), p, &cells, asc, line_h, 1.0);
            let g = &laid.glyphs[0];
            g.contours[0].start.1 - g.pen.1
        };
        assert!(ink_offset(&above) < -0.2, "above the baseline");
        assert!(ink_offset(&below) > 0.2, "flipped below the baseline");
    }

    #[test]
    fn offset_slides_the_run_and_align_end_reaches_the_path_end() {
        assert_eq!(run_start(10.0, 4.0, Along::Start, 0.0), 0.0);
        assert!((run_start(10.0, 4.0, Along::Start, 1.5) - 1.5).abs() < 1e-6, "offset slides the start by its arc length");
        assert!((run_start(10.0, 4.0, Along::Centre, 0.0) - 3.0).abs() < 1e-6);
        assert!((run_start(10.0, 4.0, Along::End, 0.0) + 4.0 - 10.0).abs() < 1e-6, "the last glyph's trailing edge reaches the path end");
        // place wires run_start in: on a semicircle, End turns the last glyph differently from Start.
        let semicircle = vec![arc((0.0, 0.0), 2.0, 0.0, PI, 4)];
        let cells: Vec<Cell> = (0..3).map(|i| Cell { char_index: i, x: i as f32, advance: 1.0 }).collect();
        let at_start = place(synth(&cells, 0.0, 1.0, (0.0, 0.0)), &text_path(semicircle.clone(), Side::Above, Along::Start, 0.0), &cells, 0.0, 1.0, 3.0);
        let at_end = place(synth(&cells, 0.0, 1.0, (0.0, 0.0)), &text_path(semicircle, Side::Above, Along::End, 0.0), &cells, 0.0, 1.0, 3.0);
        let last = |l: &TextLayout| l.glyphs.last().unwrap().angle;
        assert!((last(&at_start) - last(&at_end)).abs() > 0.1, "End reaches a different part of the arc than Start");
    }

    #[test]
    fn trace_projects_a_point_to_its_nearest_arc_length_and_side() {
        let line = TextPath {
            contours: vec![Contour { start: (0.0, 0.0), segs: vec![Seg::Line((4.0, 0.0))], closed: false }],
            ..text_path(Vec::new(), Side::Above, Along::Start, 0.0)
        };
        let trace = line.trace();
        assert!((trace.total - 4.0).abs() < 1e-4);
        let (p, angle) = trace.point_at(1.5);
        assert!((p.0 - 1.5).abs() < 1e-4 && p.1.abs() < 1e-4 && angle.abs() < 1e-4);
        // Points on opposite sides of the path project to the same foot with opposite signs.
        let (s_up, side_up) = trace.project((2.5, -0.7));
        assert!((s_up - 2.5).abs() < 1e-2 && side_up < 0.0, "{s_up} {side_up}");
        let (_, side_down) = trace.project((2.5, 0.7));
        assert!(side_down > 0.0, "the other side flips the sign: {side_down}");
    }

    #[test]
    fn circle_text_path_wraps_round_the_whole_loop() {
        let circle = TextPath::circle(2.0);
        let sampler = Sampler::new(&circle.contours);
        assert!(sampler.closed);
        let (p, _, off) = sampler.at(sampler.total * 1.5);
        let (half, _, _) = sampler.at(sampler.total * 0.5);
        assert!(!off, "a closed path never overflows");
        assert!((p.0 - half.0).abs() < 1e-3 && (p.1 - half.1).abs() < 1e-3, "one and a half turns lands where half a turn does");
    }
}
