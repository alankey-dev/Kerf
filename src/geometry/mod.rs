//! Uniform polygon geometry in inch space: flatten any shape to closed rings, rebuild an SVG `d`
//! string, triangulate with holes, and run boolean and offset operations. The boolean engine is
//! `i_overlay` 9 with the non-zero fill rule; offset is its outline offset.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use serde::{Deserialize, Serialize};

use crate::design::{Contour, Pt, Seg};

mod planar;
pub use planar::{Region, planar_map};

/// A closed ring of points in inch space.
pub type Ring = Vec<Pt>;

/// Curve flattening tolerance in inches.
pub const FLATTEN_TOL_IN: f32 = 0.01;
/// Segments in a full turn of a round join.
pub(crate) const ARC_STEPS: usize = 16;

/// Flattens one contour to a polyline, subdividing cubics until flat within `tol`.
pub fn flatten(contour: &Contour, tol: f32) -> Vec<Pt> {
    let mut pts = vec![contour.start];
    for seg in &contour.segs {
        match *seg {
            Seg::Line(p) => pts.push(p),
            Seg::Cubic(c1, c2, end) => {
                let p0 = *pts.last().unwrap();
                flatten_cubic(p0, c1, c2, end, tol, &mut pts);
            }
        }
    }
    if contour.closed && pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

/// Subdivision depth at which a cubic stops splitting: at most 2^16 points per cubic.
const MAX_DEPTH: u32 = 16;
/// Points one contour may reach before the remaining cubics are drawn as chords.
const MAX_POINTS: usize = 1 << 20;

/// Subdivides until the control points lie within `tol` of the chord, the depth or point cap is
/// reached, or the input stops being finite. Total for any input: a non-finite cubic becomes a
/// chord to its end, or nothing when the end itself is not finite.
fn flatten_cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, tol: f32, out: &mut Vec<Pt>) {
    subdivide(p0, p1, p2, p3, tol, out, MAX_DEPTH);
}

fn subdivide(p0: Pt, p1: Pt, p2: Pt, p3: Pt, tol: f32, out: &mut Vec<Pt>, depth: u32) {
    let finite = |p: Pt| p.0.is_finite() && p.1.is_finite();
    if !finite(p3) {
        return;
    }
    let flat = !(finite(p0) && finite(p1) && finite(p2)) || dist_to_line(p1, p0, p3).max(dist_to_line(p2, p0, p3)) <= tol;
    if flat || depth == 0 || out.len() >= MAX_POINTS {
        out.push(p3);
        return;
    }
    let mid = |a: Pt, b: Pt| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (p01, p12, p23) = (mid(p0, p1), mid(p1, p2), mid(p2, p3));
    let (p012, p123) = (mid(p01, p12), mid(p12, p23));
    let m = mid(p012, p123);
    subdivide(p0, p01, p012, m, tol, out, depth - 1);
    subdivide(m, p123, p23, p3, tol, out, depth - 1);
}

fn dist_to_line(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt();
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len
}

/// Signed area of a ring (positive for counter-clockwise in screen coords, y down).
pub fn signed_area(ring: &[Pt]) -> f32 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f32>()
        / 2.0
}

/// Even-odd filled area of a set of rings (outer rings add, holes subtract).
pub fn even_odd_area(rings: &[Ring]) -> f32 {
    rings.iter().map(|r| signed_area(r).abs()).sum::<f32>() - 2.0 * holes_area(rings)
}

fn holes_area(rings: &[Ring]) -> f32 {
    rings.iter().enumerate().filter(|(i, _)| is_hole(*i, rings)).map(|(_, r)| signed_area(r).abs()).sum()
}

/// A ring is a hole when it sits inside an odd number of strictly larger rings.
fn is_hole(i: usize, rings: &[Ring]) -> bool {
    let area_i = signed_area(&rings[i]).abs();
    let depth = rings.iter().enumerate().filter(|(j, o)| *j != i && signed_area(o).abs() > area_i && contains(o, centroid(&rings[i]))).count();
    depth % 2 == 1
}

fn centroid(ring: &[Pt]) -> Pt {
    let n = ring.len().max(1) as f32;
    (ring.iter().map(|p| p.0).sum::<f32>() / n, ring.iter().map(|p| p.1).sum::<f32>() / n)
}

/// Whether `(px, py)` lies inside `ring`, by the even-odd crossing rule.
pub fn contains(ring: &[Pt], (px, py): Pt) -> bool {
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let ((xi, yi), (xj, yj)) = (ring[i], ring[j]);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Triangulates rings with holes via earcut, one outer-plus-holes group at a time, returning the
/// combined vertices and triangle index triples.
pub fn triangulate(rings: &[Ring]) -> (Vec<Pt>, Vec<[usize; 3]>) {
    let (sources, tris) = triangulate_rings(rings);
    (sources.into_iter().map(|(r, k)| rings[r][k]).collect(), tris)
}

/// [`triangulate`], naming each vertex by its ring and its index in that ring.
pub fn triangulate_rings(rings: &[Ring]) -> (Vec<(usize, usize)>, Vec<[usize; 3]>) {
    let mut sources: Vec<(usize, usize)> = Vec::new();
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for (i, outer) in rings.iter().enumerate().filter(|(i, _)| !is_hole(*i, rings)) {
        let holes_of = rings.iter().enumerate().filter(|(j, _)| *j != i && is_hole(*j, rings) && contains(outer, centroid(rings[*j].as_slice()))).map(|(j, _)| j);
        let base = sources.len();
        let mut verts: Vec<f32> = Vec::new();
        let mut hole_idx: Vec<usize> = Vec::new();
        for r in std::iter::once(i).chain(holes_of) {
            if r != i {
                hole_idx.push(verts.len() / 2);
            }
            for (k, p) in rings[r].iter().enumerate() {
                verts.extend([p.0, p.1]);
                sources.push((r, k));
            }
        }
        let idx = earcutr::earcut(&verts, &hole_idx, 2).unwrap_or_default();
        tris.extend(idx.chunks(3).filter(|c| c.len() == 3).map(|c| [base + c[0], base + c[1], base + c[2]]));
    }
    (sources, tris)
}

fn to_io(rings: &[Ring]) -> Vec<Vec<[f64; 2]>> {
    rings.iter().map(|r| r.iter().map(|&(x, y)| [x as f64, y as f64]).collect()).collect()
}

fn from_io(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Ring> {
    shapes.into_iter().flatten().map(|c| c.into_iter().map(|p| (p[0] as f32, p[1] as f32)).collect()).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boolean {
    Weld,
    Subtract,
    Intersect,
    Exclude,
}

/// Boolean combination of per-shape ring groups, back-most group first (Subtract keeps it as the base).
pub fn boolean(groups: &[Vec<Ring>], op: Boolean) -> Vec<Ring> {
    if groups.is_empty() {
        return Vec::new();
    }
    let rule = match op {
        Boolean::Weld => OverlayRule::Union,
        Boolean::Subtract => OverlayRule::Difference,
        Boolean::Intersect => OverlayRule::Intersect,
        Boolean::Exclude => OverlayRule::Xor,
    };
    let mut acc = to_io(&groups[0]);
    for group in &groups[1..] {
        let clip = to_io(group);
        let result = acc.overlay(&clip, rule, FillRule::NonZero);
        acc = result.into_iter().flatten().collect();
    }
    from_io(acc.into_iter().map(|c| vec![c]).collect())
}

/// The three pieces of a Slice: `a` minus `b`, `b` minus `a`, and their overlap. Any may be empty.
pub fn slice(a: &[Ring], b: &[Ring]) -> [Vec<Ring>; 3] {
    let (a, b) = (a.to_vec(), b.to_vec());
    [boolean(&[a.clone(), b.clone()], Boolean::Subtract), boolean(&[b.clone(), a.clone()], Boolean::Subtract), boolean(&[a, b], Boolean::Intersect)]
}

/// How an offset turns a corner.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Join {
    Round,
    /// A sharp point, cut off once it reaches `limit` times the distance from the corner.
    Miter {
        limit: f32,
    },
    Bevel,
}

pub const DEFAULT_MITER_LIMIT: f32 = 4.0;

impl Join {
    fn line_join(self) -> LineJoin<f64> {
        match self {
            Join::Round => LineJoin::Round(std::f64::consts::TAU / ARC_STEPS as f64),
            // A miter reaches 1 / sin(angle / 2) times the distance, so a limit is a minimum interior angle.
            Join::Miter { limit } => LineJoin::Miter(2.0 * (1.0 / f64::from(limit.max(1.0))).asin()),
            Join::Bevel => LineJoin::Bevel,
        }
    }
}

/// An offset's rings, and how many separate parts of the input an inset consumed entirely.
#[derive(Debug, PartialEq)]
pub struct Offsetted {
    pub rings: Vec<Ring>,
    pub dropped: usize,
}

impl Offsetted {
    /// Whether the inset consumed every part, leaving nothing.
    pub fn all_dropped(&self) -> bool {
        self.rings.is_empty() && self.dropped > 0
    }
}

/// Offset outward (positive) or inward (negative), turning corners with `join`.
pub fn offset(rings: &[Ring], distance: f32, join: Join) -> Vec<Ring> {
    offset_regions(rings, distance, join).rings
}

/// [`offset`], counting the parts an inset consumes. The rings are welded first, since the outline
/// offset needs valid regions; each region is then an outer ring with its holes. Insetting cannot merge
/// regions, so each is inset alone and one that vanishes is counted.
pub fn offset_regions(rings: &[Ring], distance: f32, join: Join) -> Offsetted {
    if distance == 0.0 || rings.is_empty() {
        return Offsetted { rings: rings.to_vec(), dropped: 0 };
    }
    let regions = to_io(rings).simplify_shape(FillRule::NonZero);
    let style = OutlineStyle::new(f64::from(distance)).line_join(join.line_join());
    if distance > 0.0 {
        return Offsetted { rings: from_io(regions.outline(&style)), dropped: 0 };
    }
    let mut out = Offsetted { rings: Vec::new(), dropped: 0 };
    for region in &regions {
        let inset = from_io(region.outline(&style));
        out.dropped += usize::from(inset.is_empty());
        out.rings.extend(inset);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, s: f32) -> Ring {
        vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)]
    }

    fn cubic(span: f32) -> Contour {
        Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((0.0, span), (span, span), (span, 0.0))], closed: false }
    }

    #[test]
    fn flattening_is_bounded_for_huge_and_non_finite_cubics() {
        for span in [1e4, 1e6, 1e8, 1e10, 1e30, f32::MAX] {
            let pts = flatten(&cubic(span), FLATTEN_TOL_IN);
            assert!(pts.len() <= (1 << MAX_DEPTH) + 1, "{span}: {} points", pts.len());
            assert_eq!(pts.last(), Some(&(span, 0.0)), "{span} still ends at its end point");
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let c =
                Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((bad, 1.0), (2.0, bad), (3.0, 0.0)), Seg::Cubic((1.0, 1.0), (2.0, 1.0), (bad, bad))], closed: false };
            assert_eq!(flatten(&c, FLATTEN_TOL_IN), [(0.0, 0.0), (3.0, 0.0)], "{bad}: a chord, and a non-finite end is skipped");
        }
    }

    #[test]
    fn a_contour_of_many_huge_cubics_stops_at_the_point_cap() {
        let c = Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((0.0, 1e9), (1e9, 1e9), (1e9, 0.0)); 64], closed: false };
        let pts = flatten(&c, FLATTEN_TOL_IN);
        assert!(pts.len() <= MAX_POINTS + 64 * 2, "{} points", pts.len());
    }

    fn bbox(rings: &[Ring]) -> (f32, f32, f32, f32) {
        let pts = rings.iter().flatten();
        pts.fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
    }

    #[test]
    fn weld_merges_two_overlapping_squares() {
        let a = vec![square(0.0, 0.0, 2.0)];
        let b = vec![square(1.0, 0.0, 2.0)];
        let out = boolean(&[a.clone(), b.clone()], Boolean::Weld);
        let (x0, y0, x1, y1) = bbox(&out);
        assert!((x1 - x0 - 3.0).abs() < 1e-3 && (y1 - y0 - 2.0).abs() < 1e-3, "bbox {} {}", x1 - x0, y1 - y0);
        assert!(even_odd_area(&out) < 8.0 && even_odd_area(&out) > 5.0, "area {}", even_odd_area(&out));
    }

    #[test]
    fn subtract_removes_the_overlap() {
        let base = vec![square(0.0, 0.0, 2.0)];
        let top = vec![square(1.0, 0.0, 2.0)];
        let out = boolean(&[base, top], Boolean::Subtract);
        assert!((even_odd_area(&out) - 2.0).abs() < 1e-2, "area {}", even_odd_area(&out));
    }

    #[test]
    fn intersect_keeps_only_the_overlap() {
        let a = vec![square(0.0, 0.0, 2.0)];
        let b = vec![square(1.0, 0.0, 2.0)];
        let out = boolean(&[a, b], Boolean::Intersect);
        let (x0, _, x1, _) = bbox(&out);
        assert!((x1 - x0 - 1.0).abs() < 1e-3, "width {}", x1 - x0);
        assert!((even_odd_area(&out) - 2.0).abs() < 1e-2);
    }

    #[test]
    fn exclude_leaves_two_pieces() {
        let a = vec![square(0.0, 0.0, 2.0)];
        let b = vec![square(1.0, 0.0, 2.0)];
        let out = boolean(&[a, b], Boolean::Exclude);
        assert!(out.len() >= 2, "pieces {}", out.len());
        assert!((even_odd_area(&out) - 4.0).abs() < 1e-2, "area {}", even_odd_area(&out));
    }

    #[test]
    fn slicing_overlapping_squares_gives_three_pieces_covering_the_union() {
        let (a, b) = (vec![square(0.0, 0.0, 2.0)], vec![square(1.0, 1.0, 2.0)]);
        let pieces = slice(&a, &b);
        assert!(pieces.iter().all(|p| !p.is_empty()), "three non-empty pieces");
        let sum: f32 = pieces.iter().map(|p| even_odd_area(p)).sum();
        let union = even_odd_area(&boolean(&[a, b], Boolean::Weld));
        assert!((sum - union).abs() < 1e-3, "pieces {sum} vs union {union}");
        assert!((even_odd_area(&pieces[2]) - 1.0).abs() < 1e-3, "overlap is the 1 x 1 corner");
    }

    #[test]
    fn slicing_disjoint_squares_leaves_no_overlap() {
        let pieces = slice(&[square(0.0, 0.0, 1.0)], &[square(3.0, 0.0, 1.0)]);
        assert!(pieces[2].is_empty(), "no overlap piece");
        assert!((even_odd_area(&pieces[0]) - 1.0).abs() < 1e-3 && (even_odd_area(&pieces[1]) - 1.0).abs() < 1e-3);
    }

    fn size(rings: &[Ring]) -> (f32, f32) {
        let (x0, y0, x1, y1) = bbox(rings);
        (x1 - x0, y1 - y0)
    }

    #[test]
    fn inset_of_a_tenth_on_a_two_inch_square_is_a_square_of_one_point_eight() {
        for join in [Join::Round, Join::Miter { limit: DEFAULT_MITER_LIMIT }, Join::Bevel] {
            let out = offset(&[square(0.0, 0.0, 2.0)], -0.1, join);
            let (w, h) = size(&out);
            assert!((w - 1.8).abs() < 1e-3 && (h - 1.8).abs() < 1e-3, "{join:?}: {w} x {h}");
            assert!((even_odd_area(&out) - 1.8 * 1.8).abs() < 1e-3, "{join:?}: area {}", even_odd_area(&out));
        }
    }

    #[test]
    fn outset_corners_order_miter_round_bevel_by_area() {
        let s = vec![square(0.0, 0.0, 2.0)];
        let area = |join| even_odd_area(&offset(&s, 0.25, join));
        let (miter, round, bevel) = (area(Join::Miter { limit: DEFAULT_MITER_LIMIT }), area(Join::Round), area(Join::Bevel));
        assert!((miter - 2.5 * 2.5).abs() < 1e-3, "a miter keeps the square's corners: {miter}");
        assert!(miter > round + 0.01 && round > bevel + 0.01, "miter {miter} round {round} bevel {bevel}");
        assert!((size(&offset(&s, 0.25, Join::Round)).0 - 2.5).abs() < 1e-3);
    }

    #[test]
    fn a_miter_past_its_limit_is_cut_off() {
        let spike = vec![vec![(0.0, 0.0), (4.0, 0.5), (0.0, 1.0)]];
        let long = size(&offset(&spike, 0.1, Join::Miter { limit: 20.0 })).0;
        let short = size(&offset(&spike, 0.1, Join::Miter { limit: 1.5 })).0;
        assert!(long > short + 0.1, "limit 20 reaches {long}, limit 1.5 stops at {short}");
    }

    #[test]
    fn an_inset_drops_and_counts_the_parts_it_consumes() {
        let thin_bar = vec![(3.0, 0.0), (3.1, 0.0), (3.1, 2.0), (3.0, 2.0)];
        let out = offset_regions(&[square(0.0, 0.0, 2.0), thin_bar], -0.1, Join::Round);
        assert_eq!(out.dropped, 1, "the 0.1 in bar is gone");
        assert!((size(&out.rings).0 - 1.8).abs() < 1e-3, "the square remains");
        let none = offset_regions(&[square(0.0, 0.0, 2.0)], -1.5, Join::Round);
        assert!(none.rings.is_empty() && none.dropped == 1 && none.all_dropped());
        assert!(!out.all_dropped());
    }

    #[test]
    fn a_ring_with_a_hole_insets_both_ways_and_outset_merges_neighbours() {
        let hole: Ring = square(1.0, 1.0, 2.0).into_iter().rev().collect();
        let frame = offset(&[square(0.0, 0.0, 4.0), hole], -0.25, Join::Miter { limit: DEFAULT_MITER_LIMIT });
        assert!((even_odd_area(&frame) - (3.5 * 3.5 - 2.5 * 2.5)).abs() < 1e-3, "area {}", even_odd_area(&frame));
        let merged = offset(&[square(0.0, 0.0, 1.0), square(1.2, 0.0, 1.0)], 0.25, Join::Miter { limit: DEFAULT_MITER_LIMIT });
        assert_eq!(merged.len(), 1, "two squares 0.2 apart grow into one");
    }

    #[test]
    fn overlapping_input_rings_are_welded_before_offsetting() {
        let out = offset(&[square(0.0, 0.0, 2.0), square(1.0, 0.0, 2.0)], -0.1, Join::Miter { limit: DEFAULT_MITER_LIMIT });
        assert_eq!(out.len(), 1);
        assert!((even_odd_area(&out) - 2.8 * 1.8).abs() < 1e-3, "area {}", even_odd_area(&out));
    }

    #[test]
    fn even_odd_area_subtracts_a_hole() {
        let outer = square(0.0, 0.0, 4.0);
        let hole = square(1.0, 1.0, 2.0);
        assert!((even_odd_area(&[outer, hole]) - 12.0).abs() < 1e-3);
    }
}

// ---------------------------------------------------------------------------
// Freehand path fitting for the Pencil tool.

/// Ramer-Douglas-Peucker simplification. Drops points closer than `epsilon` inches to the chord.
pub fn rdp(points: &[Pt], epsilon: f32) -> Vec<Pt> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let (idx, dmax) = points[1..points.len() - 1].iter().enumerate().map(|(i, p)| (i + 1, dist_to_line(*p, a, b))).max_by(|x, y| x.1.total_cmp(&y.1)).unwrap();
    if dmax > epsilon {
        let mut left = rdp(&points[..=idx], epsilon);
        let right = rdp(&points[idx..], epsilon);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![a, b]
    }
}

/// Fits a smooth open cubic path through the points using the Catmull-Rom to Bezier conversion.
pub fn smooth_open_path(points: &[Pt]) -> Contour {
    let n = points.len();
    if n < 2 {
        return Contour { start: points.first().copied().unwrap_or((0.0, 0.0)), segs: Vec::new(), closed: false };
    }
    let mut segs = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let p0 = points[i.saturating_sub(1)];
        let (p1, p2) = (points[i], points[i + 1]);
        let p3 = points[(i + 2).min(n - 1)];
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        segs.push(Seg::Cubic(c1, c2, p2));
    }
    Contour { start: points[0], segs, closed: false }
}

#[cfg(test)]
mod freehand_tests {
    use super::*;

    #[test]
    fn rdp_collapses_a_near_straight_run_to_two_points() {
        let pts = vec![(0.0, 0.0), (1.0, 0.005), (2.0, -0.004), (3.0, 0.002), (4.0, 0.0)];
        assert_eq!(rdp(&pts, 0.02).len(), 2);
    }

    #[test]
    fn rdp_keeps_a_corner() {
        let pts = vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (2.0, 2.0)];
        let out = rdp(&pts, 0.02);
        assert!(out.contains(&(2.0, 0.0)), "corner kept: {out:?}");
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn smooth_path_yields_a_non_empty_open_contour() {
        let c = smooth_open_path(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]);
        assert!(!c.closed && c.segs.len() == 2);
        let d = crate::design::to_svg_path_d(
            &crate::design::Shape {
                width_in: 2.0,
                fill: egui::Color32::BLACK,
                operation: crate::design::Operation::Pen,
                pen: Default::default(),
                ..crate::design::Shape::new(1, crate::design::ShapeKind::Path { contours: vec![c], view_box: (2.0, 1.0), node_kinds: Vec::new(), corners: Vec::new() })
            }
            .contours(),
        );
        assert!(d.starts_with('M') && d.contains(" C") && !d.ends_with('Z'), "{d}");
    }
}
