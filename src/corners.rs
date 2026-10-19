//! Live corners for a drawn path (spec 55). A [`Corner`] rounds, chamfers or scoops one node of a
//! `ShapeKind::Path` without moving the node, so the stored geometry stays sharp and the radius can be
//! changed or removed later. [`with_corners`] is the pure evaluation the geometry pipeline runs every
//! time the outline is built; the underlying nodes are untouched.
//!
//! A corner applies to an interior Straight or Cusp node, where two edges meet at an angle. Round fits a
//! circular arc tangent to both edges, drawn with the same cubic approximation the parametric shapes use
//! (spec 49). Chamfer cuts a straight line between the tangent points. Inverted scoops a concave arc on
//! the far side of that line. The radius can never grow past half the shorter adjacent edge, so adjacent
//! rounds never overlap; [`effective_radii`] reports the clamped value for the read-out.

use crate::design::{Contour, Pt, Seg, Shape, ShapeKind};
use crate::path_edit::{NodeContour, NodeId};

/// One change the Corner tool or its toolbar makes to a shape's corners, as a single undo step. The
/// nodes come from the tool's selection; `RoundAll` reaches every eligible node of the subject.
#[derive(Clone, Debug, PartialEq)]
pub enum CornerOp {
    /// Set the radius and/or the style of the listed nodes.
    Set { nodes: Vec<NodeId>, radius: Option<f32>, style: Option<CornerStyle> },
    /// Cycle the style of the listed nodes: Round, Chamfer, Inverted.
    Cycle { nodes: Vec<NodeId> },
    /// Round every eligible corner of the subject to `radius_in`.
    RoundAll { radius_in: f32 },
}

/// How a corner replaces its node.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CornerStyle {
    /// A circular arc tangent to both edges.
    #[default]
    Round,
    /// A straight cut between the tangent points.
    Chamfer,
    /// A concave arc scooped out of the corner.
    Inverted,
}

impl CornerStyle {
    pub const ALL: [CornerStyle; 3] = [Self::Round, Self::Chamfer, Self::Inverted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Round => "Round",
            Self::Chamfer => "Chamfer",
            Self::Inverted => "Inverted",
        }
    }

    /// The next style when the widget is Option-clicked.
    pub fn next(self) -> Self {
        match self {
            Self::Round => Self::Chamfer,
            Self::Chamfer => Self::Inverted,
            Self::Inverted => Self::Round,
        }
    }
}

/// One node's corner. `radius_in <= 0` means no corner, so a node that has never been cornered and a node
/// whose radius was typed back to zero read the same.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Corner {
    pub radius_in: f32,
    #[serde(default)]
    pub style: CornerStyle,
}

impl Corner {
    pub fn round(radius_in: f32) -> Self {
        Self { radius_in, style: CornerStyle::Round }
    }

    /// Whether this corner draws anything.
    pub fn is_set(self) -> bool {
        self.radius_in > 0.0
    }
}

/// True when `corners` is all unset, so it need not be stored.
pub fn all_clear(corners: &[Vec<Corner>]) -> bool {
    corners.iter().flatten().all(|c| !c.is_set())
}

const EPS: f32 = 1e-5;
/// Samples along a cubic edge when finding where a corner's tangent point lands on it.
const ARC_SAMPLES: usize = 32;

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

fn mul(a: Pt, k: f32) -> Pt {
    (a.0 * k, a.1 * k)
}

fn len(a: Pt) -> f32 {
    a.0.hypot(a.1)
}

fn unit(a: Pt) -> Option<Pt> {
    let l = len(a);
    (l > EPS).then(|| (a.0 / l, a.1 / l))
}

fn dist(a: Pt, b: Pt) -> f32 {
    len(sub(a, b))
}

/// What a corner at one node resolves to once clamped: the tangent points and the sub-path between them.
struct Fit {
    /// Clamped radius reported to the UI.
    radius: f32,
    /// Tangent point on the incoming edge, toward the previous node.
    t_in: Pt,
    /// Tangent point on the outgoing edge, toward the next node.
    t_out: Pt,
    /// The arc or line from `t_in` to `t_out`, as segments following a pen already sitting at `t_in`.
    body: Vec<Seg>,
}

/// The tangent direction and chord length of the edge leaving node `i` toward neighbour `j`. For a line
/// it is the chord itself; for a cubic it is the handle direction, falling back to the chord.
fn edge(nc: &NodeContour, i: usize, j: usize, out: bool) -> Option<(Pt, f32)> {
    let (a, b) = (nc.nodes[i].pt, nc.nodes[j].pt);
    let handle = if out { nc.nodes[i].out } else { nc.nodes[i].inn };
    let dir = handle.and_then(|h| unit(sub(h, a))).or_else(|| unit(sub(b, a)))?;
    Some((dir, dist(a, b)))
}

/// The parameter on a cubic whose point sits `target` along the curve from its start, by arc length.
fn param_at_arc_len(p: [Pt; 4], target: f32) -> f32 {
    let mut acc = 0.0;
    let mut prev = p[0];
    for k in 1..=ARC_SAMPLES {
        let t = k as f32 / ARC_SAMPLES as f32;
        let here = cubic_at(p, t);
        let step = dist(prev, here);
        if acc + step >= target {
            let frac = if step > EPS { (target - acc) / step } else { 0.0 };
            return (k as f32 - 1.0 + frac) / ARC_SAMPLES as f32;
        }
        acc += step;
        prev = here;
    }
    1.0
}

fn cubic_at(p: [Pt; 4], t: f32) -> Pt {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * p[0].0 + b * p[1].0 + c * p[2].0 + d * p[3].0, a * p[0].1 + b * p[1].1 + c * p[2].1 + d * p[3].1)
}

/// The portion of the cubic between parameters `t0` and `t1`, as `[start, c1, c2, end]`.
fn subcurve(p: [Pt; 4], t0: f32, t1: f32) -> [Pt; 4] {
    let right = split(p, t0).1;
    // Re-parameterise t1 into the right part's own range.
    let t = if t1 < 1.0 { (t1 - t0) / (1.0 - t0) } else { 1.0 };
    split(right, t).0
}

fn split(p: [Pt; 4], t: f32) -> ([Pt; 4], [Pt; 4]) {
    let lerp = |a: Pt, b: Pt| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let (p01, p12, p23) = (lerp(p[0], p[1]), lerp(p[1], p[2]), lerp(p[2], p[3]));
    let (p012, p123) = (lerp(p01, p12), lerp(p12, p23));
    let m = lerp(p012, p123);
    ([p[0], p01, p012, m], [m, p123, p23, p[3]])
}

fn angle(c: Pt, p: Pt) -> f32 {
    (p.1 - c.1).atan2(p.0 - c.0).to_degrees()
}

/// The signed minor sweep from `a` to `b` in degrees, in (-180, 180].
fn minor_sweep(a: f32, b: f32) -> f32 {
    (b - a + 540.0).rem_euclid(360.0) - 180.0
}

/// An arc of radius `r` about `c`, from `from` degrees through `sweep`, as cubics of at most 90 degrees,
/// following a pen already at the arc's start.
fn arc_segs(c: Pt, r: f32, from: f32, sweep: f32) -> Vec<Seg> {
    let n = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep / n as f32;
    let k = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
    let at = |deg: f32| {
        let (s, co) = deg.to_radians().sin_cos();
        ((c.0 + r * co, c.1 + r * s), (-r * s, r * co))
    };
    (0..n)
        .map(|i| {
            let ((p0, d0), (p1, d1)) = (at(from + step * i as f32), at(from + step * (i + 1) as f32));
            Seg::Cubic((p0.0 + k * d0.0, p0.1 + k * d0.1), (p1.0 - k * d1.0, p1.1 - k * d1.1), p1)
        })
        .collect()
}

/// Reflects `p` across the line through `a` and `b`.
fn reflect(p: Pt, a: Pt, b: Pt) -> Pt {
    let Some(d) = unit(sub(b, a)) else { return p };
    let v = sub(p, a);
    let along = v.0 * d.0 + v.1 * d.1;
    let foot = add(a, mul(d, along));
    add(foot, sub(foot, p))
}

/// Resolves node `i`'s corner, or `None` when it is an endpoint, nearly straight, or has no radius.
fn resolve(nc: &NodeContour, i: usize, corner: Corner) -> Option<Fit> {
    if !corner.is_set() {
        return None;
    }
    let n = nc.nodes.len();
    let (prev, next) = if nc.closed {
        ((i + n - 1) % n, (i + 1) % n)
    } else if i == 0 || i + 1 == n {
        return None;
    } else {
        (i - 1, i + 1)
    };
    let v = nc.nodes[i].pt;
    let (u_dir, edge_in) = edge(nc, i, prev, false)?;
    let (w_dir, edge_out) = edge(nc, i, next, true)?;
    let cos = (u_dir.0 * w_dir.0 + u_dir.1 * w_dir.1).clamp(-1.0, 1.0);
    if cos > 1.0 - EPS {
        // The edges are collinear, so there is no corner to cut.
        return None;
    }
    let half = cos.acos() / 2.0;
    let radius = corner.radius_in.min(0.5 * edge_in.min(edge_out)).max(0.0);
    if radius <= EPS {
        return None;
    }
    let along = radius / half.tan();
    let t_in = along_edge(nc, i, prev, false, u_dir, along);
    let t_out = along_edge(nc, i, next, true, w_dir, along);
    let body = match corner.style {
        CornerStyle::Chamfer => vec![Seg::Line(t_out)],
        CornerStyle::Round | CornerStyle::Inverted => {
            let bisector = unit(add(u_dir, w_dir)).unwrap_or(u_dir);
            let centre = add(v, mul(bisector, radius / half.sin()));
            let (from, to) = (angle(centre, t_in), angle(centre, t_out));
            match corner.style {
                CornerStyle::Round => arc_segs(centre, radius, from, minor_sweep(from, to)),
                _ => {
                    // Scoop: the round arc mirrored across the chord, so it bulges away from the vertex.
                    let c_inv = reflect(centre, t_in, t_out);
                    let (from, to) = (angle(c_inv, t_in), angle(c_inv, t_out));
                    arc_segs(c_inv, radius, from, minor_sweep(from, to))
                }
            }
        }
    };
    Some(Fit { radius, t_in, t_out, body })
}

/// The tangent point at distance `along` from node `i` toward neighbour `j`. On a line it sits on the
/// straight edge; on a cubic it sits on the curve, so the trimmed segment and the corner meet.
fn along_edge(nc: &NodeContour, i: usize, j: usize, out: bool, dir: Pt, along: f32) -> Pt {
    let v = nc.nodes[i].pt;
    match cubic_between(nc, i, j, out) {
        Some(p) => cubic_at(p, param_at_arc_len(p, along)),
        None => add(v, mul(dir, along)),
    }
}

/// The cubic control points of the edge from node `i` toward `j`, in that direction, or `None` for a line.
fn cubic_between(nc: &NodeContour, i: usize, j: usize, out: bool) -> Option<[Pt; 4]> {
    let (a, b) = (&nc.nodes[i], &nc.nodes[j]);
    if out {
        match (a.out, b.inn) {
            (None, None) => None,
            (c1, c2) => Some([a.pt, c1.unwrap_or(a.pt), c2.unwrap_or(b.pt), b.pt]),
        }
    } else {
        // Facing back along the edge from `i` to `j`, so the pen walks j -> i.
        match (b.out, a.inn) {
            (None, None) => None,
            (c1, c2) => Some([b.pt, c1.unwrap_or(b.pt), c2.unwrap_or(a.pt), a.pt]),
        }
    }
}

/// The clamped radius each node's corner draws, zero where it draws nothing, one per node.
pub fn effective_radii(contour: &Contour, corners: &[Corner]) -> Vec<f32> {
    let nc = NodeContour::from_contour(contour, None);
    (0..nc.nodes.len()).map(|i| corners.get(i).copied().and_then(|c| resolve(&nc, i, c)).map_or(0.0, |f| f.radius)).collect()
}

/// `contour` with every set corner drawn live. The nodes are read exactly as [`crate::path_edit`] counts
/// them, so `corners` lines up with a path's stored `node_kinds`. Nodes with no corner are untouched.
pub fn with_corners(contour: &Contour, corners: &[Corner]) -> Contour {
    let nc = NodeContour::from_contour(contour, None);
    let n = nc.nodes.len();
    if n < 2 || corners.iter().all(|c| !c.is_set()) {
        return contour.clone();
    }
    let fits: Vec<Option<Fit>> = (0..n).map(|i| corners.get(i).copied().and_then(|c| resolve(&nc, i, c))).collect();
    let seg_count = if nc.closed { n } else { n - 1 };

    let start = match &fits[0] {
        Some(f) if nc.closed => f.t_out,
        _ => nc.nodes[0].pt,
    };
    let mut segs = Vec::new();
    for s in 0..seg_count {
        let (a, b) = (s, (s + 1) % n);
        let trimmed = trim_segment(&nc, a, b, &fits[a], &fits[b]);
        segs.extend(trimmed);
        if let Some(f) = &fits[b] {
            segs.extend(f.body.clone());
        }
    }
    Contour { start, segs, closed: nc.closed }
}

/// The segment from node `a` to node `b`, trimmed to start at `a`'s outgoing tangent point and end at
/// `b`'s incoming tangent point when either is cornered.
fn trim_segment(nc: &NodeContour, a: usize, b: usize, fa: &Option<Fit>, fb: &Option<Fit>) -> Vec<Seg> {
    let end = fb.as_ref().map_or(nc.nodes[b].pt, |f| f.t_in);
    match cubic_between(nc, a, b, true) {
        None => vec![Seg::Line(end)],
        Some(p) => {
            let edge_len = dist(nc.nodes[a].pt, nc.nodes[b].pt).max(EPS);
            let t0 = fa.as_ref().map_or(0.0, |f| param_at_arc_len(p, dist(nc.nodes[a].pt, f.t_out)).min(1.0)).max(0.0);
            let t1 = fb.as_ref().map_or(1.0, |_| param_at_arc_len(p, (edge_len - dist(nc.nodes[b].pt, end)).max(0.0)));
            let t1 = t1.max(t0);
            let [_, c1, c2, e] = subcurve(p, t0, t1);
            vec![Seg::Cubic(c1, c2, e)]
        }
    }
}

/// `contours` with `shape`'s stored corners drawn live, when it is a `Path` that has any. Any other
/// shape, or a path with no corners, is returned unchanged. The outline here is already scaled to the
/// shape's size, so the corners line up with its stored nodes.
pub fn round_outline(shape: &Shape, contours: Vec<Contour>) -> Vec<Contour> {
    let ShapeKind::Path { corners, .. } = &shape.kind else { return contours };
    if all_clear(corners) {
        return contours;
    }
    contours
        .into_iter()
        .enumerate()
        .map(|(i, c)| match corners.get(i) {
            Some(cs) => with_corners(&c, cs),
            None => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(side: f32) -> Contour {
        Contour { start: (0.0, 0.0), segs: vec![Seg::Line((side, 0.0)), Seg::Line((side, side)), Seg::Line((0.0, side))], closed: true }
    }

    fn round(r: f32) -> Vec<Corner> {
        vec![Corner::round(r); 4]
    }

    /// The densely flattened points of a contour.
    fn flat(c: &Contour) -> Vec<Pt> {
        c.flatten()
    }

    fn closest(pts: &[Pt], to: Pt) -> f32 {
        pts.iter().map(|&p| dist(p, to)).fold(f32::MAX, f32::min)
    }

    #[test]
    fn a_round_corner_on_a_right_angle_sits_tangent_inside_the_corner() {
        // The top-left node of a 2 in square is a right angle. A 0.2 in round replaces it with an arc
        // whose midpoint lies 0.2 (sqrt 2 - 1) in along the inward diagonal.
        let only_tl = vec![Corner::round(0.2), Corner::default(), Corner::default(), Corner::default()];
        let rounded = with_corners(&square(2.0), &only_tl);
        let pts = flat(&rounded);
        let r = 0.2_f32;
        let inset = r * (2.0_f32.sqrt() - 1.0);
        let midpoint = (inset / 2.0_f32.sqrt(), inset / 2.0_f32.sqrt());
        assert!(closest(&pts, midpoint) < 0.01, "arc midpoint near {midpoint:?}");
        // The tangent points sit 0.2 in along each edge from the corner.
        assert!(closest(&pts, (0.2, 0.0)) < 0.01, "tangent on the top edge");
        assert!(closest(&pts, (0.0, 0.2)) < 0.01, "tangent on the left edge");
        // The sharp corner itself is gone.
        assert!(closest(&pts, (0.0, 0.0)) > inset - 0.001, "the corner point is cut away");
    }

    #[test]
    fn a_chamfer_is_a_straight_cut_between_the_tangent_points() {
        let only_tl = vec![Corner { radius_in: 0.3, style: CornerStyle::Chamfer }, Corner::default(), Corner::default(), Corner::default()];
        let cut = with_corners(&square(2.0), &only_tl);
        // The corner is one straight line from (0, 0.3) to the start point (0.3, 0): no cubics at all.
        assert_eq!(cut.start, (0.3, 0.0), "the contour starts at the outgoing tangent point");
        assert!(cut.segs.iter().all(|s| matches!(s, Seg::Line(_))), "a chamfer adds no arc: {:?}", cut.segs);
        assert_eq!(cut.segs.last(), Some(&Seg::Line((0.3, 0.0))), "the cut closes straight onto the start");
        assert!(cut.segs.contains(&Seg::Line((0.0, 0.3))), "the incoming edge lands on the other tangent");
        // A round of the same radius bulges toward the corner, so it samples near the arc midpoint.
        let round = with_corners(&square(2.0), &[Corner::round(0.3), Corner::default(), Corner::default(), Corner::default()]);
        let mid = 0.3 * (2.0_f32.sqrt() - 1.0) / 2.0_f32.sqrt();
        assert!(closest(&flat(&round), (mid, mid)) < 1e-2, "round samples the arc; chamfer would not");
        assert!(closest(&flat(&cut), (mid, mid)) > 0.2, "the chamfer has no arc near the corner");
    }

    #[test]
    fn an_inverted_corner_scoops_to_the_far_side_of_the_chord() {
        let tl = Corner { radius_in: 0.3, style: CornerStyle::Inverted };
        let scooped = with_corners(&square(2.0), &[tl, Corner::default(), Corner::default(), Corner::default()]);
        let round = with_corners(&square(2.0), &[Corner::round(0.3), Corner::default(), Corner::default(), Corner::default()]);
        // The chord between the tangent points is the line x + y = 0.3. The round arc bulges toward the
        // corner (x + y < 0.3); the inverted scoop bulges the other way, into the shape (x + y > 0.3).
        let near = |pts: &[Pt]| pts.iter().copied().filter(|&(x, y)| x < 0.35 && y < 0.35).collect::<Vec<_>>();
        let round_near = near(&flat(&round));
        let scoop_near = near(&flat(&scooped));
        assert!(round_near.iter().any(|&(x, y)| x + y < 0.3 - 0.02), "the round arc sits on the corner side of the chord");
        assert!(scoop_near.iter().any(|&(x, y)| x + y > 0.3 + 0.02), "the scoop sits on the far side of the chord");
        assert!(scooped.segs.iter().any(|s| matches!(s, Seg::Cubic(..))), "the scoop is an arc");
    }

    #[test]
    fn a_radius_past_half_the_short_edge_clamps_and_two_corners_share_it() {
        // A 1 x 0.4 in rectangle. The short edges are 0.4 in, so a corner clamps to 0.2 in.
        let rect = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((1.0, 0.0)), Seg::Line((1.0, 0.4)), Seg::Line((0.0, 0.4))], closed: true };
        let eff = effective_radii(&rect, &round(5.0));
        assert!(eff.iter().all(|&r| (r - 0.2).abs() < 1e-4), "every corner clamps to half the 0.4 in edge: {eff:?}");
        // The two corners on a short edge each take 0.2 in, so they meet in the middle and do not cross.
        let rounded = with_corners(&rect, &round(5.0));
        assert!(rounded.flatten().iter().all(|&(x, _)| (0.0..=1.0).contains(&x)), "no corner overshoots the edge");
    }
}
