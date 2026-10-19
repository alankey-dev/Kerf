//! Magnetic snapping. Pure maths in inch space: a [`Candidate`] (the moving thing, as points with
//! roles) meets the best [`Targets`] point or line within the threshold, and [`snap`] returns the
//! correction plus a [`Snapped`] report the canvas draws. The per-drag [`Targets`], the moving
//! [`Candidate`] and the per-shape point cache are built in [`cache`]; the UI supplies the threshold in
//! inches (screen pixels divided by the zoom).

use crate::design::Pt;

pub mod cache;

/// The grid a snap lands on: lines every `pitch_in` from `origin`, on both axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapGrid {
    pub pitch_in: f32,
    pub origin: Pt,
}

impl SnapGrid {
    /// The grid point nearest `p`.
    pub fn nearest(&self, p: Pt) -> Pt {
        (line(p.0, self.origin.0, self.pitch_in), line(p.1, self.origin.1, self.pitch_in))
    }
}

fn line(value: f32, origin: f32, grid_in: f32) -> f32 {
    origin + ((value - origin) / grid_in).round() * grid_in
}

/// An axis-aligned box in world inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Aabb {
    /// What a shape with no outline measures: the identity of `union`.
    pub const EMPTY: Aabb = Aabb { min_x: f32::MAX, min_y: f32::MAX, max_x: f32::MIN, max_y: f32::MIN };

    pub fn union(self, b: Aabb) -> Aabb {
        Aabb { min_x: self.min_x.min(b.min_x), min_y: self.min_y.min(b.min_y), max_x: self.max_x.max(b.max_x), max_y: self.max_y.max(b.max_y) }
    }

    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }

    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    pub fn centre(&self) -> Pt {
        ((self.min_x + self.max_x) / 2.0, (self.min_y + self.max_y) / 2.0)
    }

    fn xs(&self) -> [f32; 3] {
        [self.min_x, (self.min_x + self.max_x) / 2.0, self.max_x]
    }

    fn ys(&self) -> [f32; 3] {
        [self.min_y, (self.min_y + self.max_y) / 2.0, self.max_y]
    }

    /// The four corners, for point snapping.
    fn corners(&self) -> [Pt; 4] {
        [(self.min_x, self.min_y), (self.max_x, self.min_y), (self.max_x, self.max_y), (self.min_x, self.max_y)]
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Aabb {
        Aabb { min_x: self.min_x + dx, min_y: self.min_y + dy, max_x: self.max_x + dx, max_y: self.max_y + dy }
    }

    /// Grown by `margin` on every side, for the viewport test in [`cache`].
    fn expanded(&self, margin: f32) -> Aabb {
        Aabb { min_x: self.min_x - margin, min_y: self.min_y - margin, max_x: self.max_x + margin, max_y: self.max_y + margin }
    }

    fn intersects(&self, b: &Aabb) -> bool {
        self.min_x <= b.max_x && b.min_x <= self.max_x && self.min_y <= b.max_y && b.min_y <= self.max_y
    }
}

/// A snap line to draw while a snap is active. `V` is a world x, `H` a world y.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Guide {
    V(f32),
    H(f32),
}

/// What a snap point or line is, lowest priority to highest. On an equal correction the higher kind
/// wins, so a node beats a midpoint beats an edge beats the grid (spec 47).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Grid,
    Mat,
    Guide,
    Edge,
    Spacing,
    Midpoint,
    Intersection,
    Node,
}

/// A target class the user can switch off from the Snapping popover (spec 47).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapClass {
    Grid,
    ShapeEdges,
    Mat,
    Guides,
    Nodes,
    Intersections,
    Spacing,
}

impl SnapClass {
    pub const ALL: [SnapClass; 7] =
        [SnapClass::Grid, SnapClass::ShapeEdges, SnapClass::Mat, SnapClass::Guides, SnapClass::Nodes, SnapClass::Intersections, SnapClass::Spacing];

    /// Which switch governs a snap of this `kind`.
    pub fn of(kind: Kind) -> SnapClass {
        match kind {
            Kind::Grid => SnapClass::Grid,
            Kind::Edge => SnapClass::ShapeEdges,
            Kind::Mat => SnapClass::Mat,
            Kind::Guide => SnapClass::Guides,
            Kind::Node | Kind::Midpoint => SnapClass::Nodes,
            Kind::Intersection => SnapClass::Intersections,
            Kind::Spacing => SnapClass::Spacing,
        }
    }

    /// The popover row's and palette command's name.
    pub fn label(self) -> &'static str {
        match self {
            SnapClass::Grid => "Grid",
            SnapClass::ShapeEdges => "Shape edges and centres",
            SnapClass::Mat => "Mat",
            SnapClass::Guides => "Guides",
            SnapClass::Nodes => "Nodes and midpoints",
            SnapClass::Intersections => "Intersections",
            SnapClass::Spacing => "Equal spacing",
        }
    }
}

/// What the magnet snaps to, and the angle step Shift locks to. Lives on `Design::snap`, so a project
/// remembers it; `#[serde(default)]` loads an older project with every class on and a 45 degree step.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapOptions {
    pub grid: bool,
    pub shape_edges: bool,
    pub mat: bool,
    pub guides: bool,
    pub nodes: bool,
    pub intersections: bool,
    pub spacing: bool,
    /// Degrees between the directions Shift locks a move or a Pen segment to: 15, 30, 45 or 90.
    pub angle_step_deg: f32,
}

impl Default for SnapOptions {
    fn default() -> Self {
        SnapOptions { grid: true, shape_edges: true, mat: true, guides: true, nodes: true, intersections: true, spacing: true, angle_step_deg: 45.0 }
    }
}

impl SnapOptions {
    pub fn enabled(&self, class: SnapClass) -> bool {
        match class {
            SnapClass::Grid => self.grid,
            SnapClass::ShapeEdges => self.shape_edges,
            SnapClass::Mat => self.mat,
            SnapClass::Guides => self.guides,
            SnapClass::Nodes => self.nodes,
            SnapClass::Intersections => self.intersections,
            SnapClass::Spacing => self.spacing,
        }
    }

    pub fn set(&mut self, class: SnapClass, on: bool) {
        match class {
            SnapClass::Grid => self.grid = on,
            SnapClass::ShapeEdges => self.shape_edges = on,
            SnapClass::Mat => self.mat = on,
            SnapClass::Guides => self.guides = on,
            SnapClass::Nodes => self.nodes = on,
            SnapClass::Intersections => self.intersections = on,
            SnapClass::Spacing => self.spacing = on,
        }
    }

    /// Whether a snap of this `kind` is allowed through.
    pub fn allows(&self, kind: Kind) -> bool {
        self.enabled(SnapClass::of(kind))
    }

    /// Whether the magnet reads on: any class is on.
    pub fn any(&self) -> bool {
        SnapClass::ALL.iter().any(|&c| self.enabled(c))
    }

    /// The magnet's click: every class on, or every class off. The angle step is left alone.
    pub fn set_all(&mut self, on: bool) {
        for class in SnapClass::ALL {
            self.set(class, on);
        }
    }

    /// Whether this equals the default (every class on, a 45 degree step), so a project need not write it.
    pub fn is_default(&self) -> bool {
        *self == SnapOptions::default()
    }
}

/// The moving thing, as points with roles. Frame edges and the centre carry [`Kind::Edge`] and drive
/// line snapping; a path's or text's nodes carry [`Kind::Node`] and drive point snapping, so a rotated
/// rectangle's corner can meet another shape's corner (spec 47).
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    pub points: Vec<(Pt, Kind)>,
    /// The frame's bounds, for equal spacing.
    pub frame: Option<Aabb>,
}

impl Candidate {
    /// A moving frame: its corners and centre as edge points, for line snapping.
    fn frame_points(frame: Aabb) -> impl Iterator<Item = (Pt, Kind)> {
        frame.corners().into_iter().chain([frame.centre()]).map(|p| (p, Kind::Edge))
    }
}

/// Everything in reach to snap to, built once per drag. Point targets are the nodes, segment midpoints,
/// intersections and the mat's corners and centre; line targets are shape edges and centres, ruler
/// guides and the mat's edges; the grid is kept whole, and `others` are the other shapes' frames for
/// equal spacing.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    pub points: Vec<(Pt, Kind)>,
    pub v_lines: Vec<(f32, Kind)>,
    pub h_lines: Vec<(f32, Kind)>,
    pub grid: Option<SnapGrid>,
    pub others: Vec<Aabb>,
    pub threshold: f32,
}

/// What met what, for the canvas to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// An edge, guide or grid snap: today's full-height line.
    Line(Guide),
    /// A point snap: a small cross where `candidate` met `target`.
    Cross { at: Pt, candidate: Kind, target: Kind },
    /// Equal spacing: the two equalised gaps, drawn as tick marks between `a` and `b`.
    Gap { a: Pt, b: Pt },
    /// The faint ray Shift locks a move to, from the start point to the constrained pointer.
    Ray { from: Pt, to: Pt },
}

/// The correction to add to a move, and what the canvas should draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapped {
    pub dx: f32,
    pub dy: f32,
    pub marks: Vec<Mark>,
}

/// One candidate correction, before the best is chosen.
struct Move {
    dx: f32,
    dy: f32,
    cost: f32,
    kind: Kind,
    mark: Mark,
}

/// The best snap on one axis: the correction, its cost, the target class and the line it met.
struct AxisSnap {
    d: f32,
    cost: f32,
    kind: Kind,
    at: f32,
}

/// One whole correction under consideration, with the marks it would draw.
struct Solution {
    cost: f32,
    kind: Kind,
    dx: f32,
    dy: f32,
    marks: Vec<Mark>,
}

/// The best correction within the threshold, preferring the smaller correction and, on a tie, the
/// higher [`Kind`]. Edge points snap each axis independently onto a line or the grid (so a move can meet
/// a guide on one axis while already aligned on the other); a node snaps both axes at once onto a point
/// target and wins when it is at least as near. `options` gates each class.
pub fn snap(candidate: &Candidate, targets: &Targets, options: &SnapOptions) -> Snapped {
    let threshold = targets.threshold;
    let grid = options.grid.then_some(targets.grid).flatten();
    let (mut x, mut y): (Option<AxisSnap>, Option<AxisSnap>) = (None, None);
    let mut point: Option<Move> = None;
    for &(p, role) in &candidate.points {
        if role == Kind::Edge {
            consider_axis(&mut x, p.0, &targets.v_lines, grid.map(|g| line(p.0, g.origin.0, g.pitch_in)), options, threshold);
            consider_axis(&mut y, p.1, &targets.h_lines, grid.map(|g| line(p.1, g.origin.1, g.pitch_in)), options, threshold);
        } else {
            for &(t, kind) in &targets.points {
                if !options.allows(kind) {
                    continue;
                }
                let (dx, dy) = (t.0 - p.0, t.1 - p.1);
                let cost = dx.hypot(dy);
                if cost <= threshold && point.as_ref().is_none_or(|b| better(cost, kind, b.cost, b.kind)) {
                    point = Some(Move { dx, dy, cost, kind, mark: Mark::Cross { at: t, candidate: Kind::Node, target: kind } });
                }
            }
        }
    }

    let mut solutions: Vec<Solution> = Vec::new();
    if x.is_some() || y.is_some() {
        let dx = x.as_ref().map_or(0.0, |a| a.d);
        let dy = y.as_ref().map_or(0.0, |a| a.d);
        let kind = x.iter().chain(y.iter()).map(|a| a.kind).max().unwrap();
        let marks = x.iter().map(|a| Mark::Line(Guide::V(a.at))).chain(y.iter().map(|a| Mark::Line(Guide::H(a.at)))).collect();
        solutions.push(Solution { cost: dx.hypot(dy), kind, dx, dy, marks });
    }
    if let Some(m) = point {
        solutions.push(Solution { cost: m.cost, kind: m.kind, dx: m.dx, dy: m.dy, marks: vec![m.mark] });
    }
    if options.spacing
        && let Some(frame) = candidate.frame
    {
        for m in spacing_moves(frame, &targets.others, threshold) {
            solutions.push(Solution { cost: m.cost, kind: m.kind, dx: m.dx, dy: m.dy, marks: vec![m.mark] });
        }
    }
    match solutions.into_iter().reduce(|a, b| if better(b.cost, b.kind, a.cost, a.kind) { b } else { a }) {
        Some(s) => Snapped { dx: s.dx, dy: s.dy, marks: s.marks },
        None => Snapped::default(),
    }
}

/// Picks `(cost, kind)` over `(other_cost, other_kind)`: the smaller correction, or on a tie the higher
/// kind.
fn better(cost: f32, kind: Kind, other_cost: f32, other_kind: Kind) -> bool {
    cost < other_cost || (cost == other_cost && kind > other_kind)
}

/// Keeps the nearer line (or grid) for one axis, breaking a tie by the higher kind.
fn consider_axis(best: &mut Option<AxisSnap>, from: f32, lines: &[(f32, Kind)], grid: Option<f32>, options: &SnapOptions, threshold: f32) {
    let mut take = |at: f32, kind: Kind| {
        let d = at - from;
        if d.abs() <= threshold && best.as_ref().is_none_or(|b| better(d.abs(), kind, b.cost, b.kind)) {
            *best = Some(AxisSnap { d, cost: d.abs(), kind, at });
        }
    };
    for &(at, kind) in lines {
        if options.allows(kind) {
            take(at, kind);
        }
    }
    if let Some(at) = grid {
        take(at, Kind::Grid);
    }
}

/// When the moving frame sits between two other frames on an axis, the correction that equalises the
/// two gaps. Affinity calls this a spacing snap.
fn spacing_moves(frame: Aabb, others: &[Aabb], threshold: f32) -> Vec<Move> {
    let mut out = Vec::new();
    equal_gap_axis(frame, others, threshold, Axis2::X, &mut out);
    equal_gap_axis(frame, others, threshold, Axis2::Y, &mut out);
    out
}

#[derive(Clone, Copy)]
enum Axis2 {
    X,
    Y,
}

fn equal_gap_axis(frame: Aabb, others: &[Aabb], threshold: f32, axis: Axis2, out: &mut Vec<Move>) {
    // The frame's near and far edges on this axis, and the test for overlap on the other axis.
    let (lo, hi, o_lo, o_hi): (f32, f32, f32, f32) = match axis {
        Axis2::X => (frame.min_x, frame.max_x, frame.min_y, frame.max_y),
        Axis2::Y => (frame.min_y, frame.max_y, frame.min_x, frame.max_x),
    };
    let edges = |o: &Aabb| -> (f32, f32, f32, f32) {
        match axis {
            Axis2::X => (o.min_x, o.max_x, o.min_y, o.max_y),
            Axis2::Y => (o.min_y, o.max_y, o.min_x, o.max_x),
        }
    };
    let overlaps = |a_lo: f32, a_hi: f32| a_lo <= o_hi && o_lo <= a_hi;
    let before = others
        .iter()
        .filter_map(|o| {
            let (lo2, hi2, c_lo, c_hi) = edges(o);
            (hi2 <= lo + 1e-4 && overlaps(c_lo, c_hi)).then_some((hi2, lo2, c_lo, c_hi))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let after = others
        .iter()
        .filter_map(|o| {
            let (lo2, hi2, c_lo, c_hi) = edges(o);
            (lo2 >= hi - 1e-4 && overlaps(c_lo, c_hi)).then_some((lo2, hi2, c_lo, c_hi))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let (Some((left_edge, _, lc_lo, lc_hi)), Some((right_edge, _, rc_lo, rc_hi))) = (before, after) {
        let gap_before = lo - left_edge;
        let gap_after = right_edge - hi;
        let shift = (gap_after - gap_before) / 2.0;
        // A mover already evenly spaced needs no correction, and must not shadow a real snap at cost 0.
        if shift.abs() > 1e-4 && shift.abs() <= threshold {
            let mid_other = (o_lo + o_hi) / 2.0;
            let (lc, rc) = ((lc_lo + lc_hi) / 2.0, (rc_lo + rc_hi) / 2.0);
            let tick_mid = mid_other.clamp(lc.min(rc), lc.max(rc));
            let (a, b) = match axis {
                Axis2::X => ((left_edge, tick_mid), (lo + shift, tick_mid)),
                Axis2::Y => ((tick_mid, left_edge), (tick_mid, lo + shift)),
            };
            let (dx, dy) = match axis {
                Axis2::X => (shift, 0.0),
                Axis2::Y => (0.0, shift),
            };
            out.push(Move { dx, dy, cost: shift.abs(), kind: Kind::Spacing, mark: Mark::Gap { a, b } });
        }
    }
}

/// Projects `delta` onto the nearest ray at a multiple of `step_deg` from the start point, so Shift
/// locks a move or a Pen segment to horizontal, vertical or a diagonal (spec 47).
pub fn constrain_angle(delta: Pt, step_deg: f32) -> Pt {
    let (dx, dy) = delta;
    if dx == 0.0 && dy == 0.0 || step_deg <= 0.0 {
        return delta;
    }
    let step = step_deg.to_radians();
    let snapped = (dy.atan2(dx) / step).round() * step;
    let (sin, cos) = snapped.sin_cos();
    let along = dx * cos + dy * sin;
    (along * cos, along * sin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aabb(x: f32, y: f32, w: f32, h: f32) -> Aabb {
        Aabb { min_x: x, min_y: y, max_x: x + w, max_y: y + h }
    }

    /// A moving frame as a candidate: its corners and centre as edge points.
    fn frame(a: Aabb) -> Candidate {
        Candidate { points: Candidate::frame_points(a).collect(), frame: Some(a) }
    }

    #[test]
    fn a_rotated_corner_snaps_to_another_nodes_and_the_report_names_both_nodes() {
        // The moving shape's corner sits 0.01 in from a target node on each axis.
        let candidate = Candidate { points: vec![((2.0, 2.0), Kind::Node)], frame: None };
        let targets = Targets { points: vec![((2.01, 1.99), Kind::Node)], threshold: 0.1, ..Targets::default() };
        let s = snap(&candidate, &targets, &SnapOptions::default());
        assert!((2.0 + s.dx - 2.01).abs() < 1e-5 && (2.0 + s.dy - 1.99).abs() < 1e-5, "corner now ({}, {})", 2.0 + s.dx, 2.0 + s.dy);
        assert_eq!(s.marks, vec![Mark::Cross { at: (2.01, 1.99), candidate: Kind::Node, target: Kind::Node }]);
    }

    #[test]
    fn a_node_beats_the_grid_at_an_equal_distance_and_a_nearer_grid_wins() {
        // The candidate carries a frame corner (an edge, which grid snaps) and a node at the same spot,
        // as a real move does. y = 2.3 is clear of any grid line, so only x is in play.
        let grid = Some(SnapGrid { pitch_in: 1.0, origin: (0.0, 0.0) });
        let both = |x: f32| Candidate { points: vec![((x, 2.3), Kind::Edge), ((x, 2.3), Kind::Node)], frame: None };

        // Grid line at 5.0 and a target node at 5.0, both 0.05 in away on x: the node wins the tie.
        let tie = Targets { points: vec![((5.0, 2.3), Kind::Node)], grid, threshold: 0.1, ..Targets::default() };
        let s = snap(&both(5.05), &tie, &SnapOptions::default());
        assert_eq!(s.marks, vec![Mark::Cross { at: (5.0, 2.3), candidate: Kind::Node, target: Kind::Node }], "the node wins the tie: {:?}", s.marks);

        // Now the grid line is 0.02 away and the node 0.08 away: the nearer grid wins.
        let far = Targets { points: vec![((5.1, 2.3), Kind::Node)], grid, threshold: 0.1, ..Targets::default() };
        let s = snap(&both(5.02), &far, &SnapOptions::default());
        assert!(matches!(s.marks.first(), Some(Mark::Line(Guide::V(x))) if (*x - 5.0).abs() < 1e-5), "the nearer grid wins: {:?}", s.marks);
    }

    #[test]
    fn a_shape_between_two_others_snaps_to_equal_spacing() {
        // Others at x [0,1] and [4,5], the mover one inch wide dragged to a 1.02 in gap on the left.
        let others = vec![aabb(0.0, 0.0, 1.0, 2.0), aabb(4.0, 0.0, 1.0, 2.0)];
        let mover = aabb(2.02, 0.5, 1.0, 1.0);
        let targets = Targets { others, threshold: 0.1, ..Targets::default() };
        // Only spacing on, so nothing else can explain the move.
        let opts = SnapOptions { grid: false, shape_edges: false, mat: false, guides: false, nodes: false, intersections: false, spacing: true, angle_step_deg: 45.0 };
        let s = snap(&frame(mover), &targets, &opts);
        assert!((s.dx - (-0.02)).abs() < 1e-5 && s.dy == 0.0, "snaps to the 1 in gap: dx {}", s.dx);
        assert!(matches!(s.marks.first(), Some(Mark::Gap { .. })), "reports equal spacing: {:?}", s.marks);
    }

    #[test]
    fn shift_constrains_a_move_to_the_nearer_ray() {
        // A dominant vertical move locks to vertical; its x drops to zero.
        let v = constrain_angle((0.2, 1.0), 45.0);
        assert!(v.0.abs() < 1e-6 && (v.1 - 1.0).abs() < 1e-6, "{v:?}");
        // A near 45 degree move locks to the diagonal; the components equalise.
        let d = constrain_angle((1.0, 0.9), 45.0);
        assert!((d.0 - d.1).abs() < 1e-6 && d.0 > 0.0, "{d:?}");
        assert!((d.0 - 0.95).abs() < 1e-3, "projected onto the 45 degree ray: {d:?}");
    }

    #[test]
    fn with_nodes_off_a_frame_snaps_by_its_edges_not_its_nodes() {
        // The candidate carries both a frame edge point and a node; a node target sits nearer than the edge.
        let candidate = Candidate { points: vec![((4.96, 1.0), Kind::Edge), ((4.9, 1.0), Kind::Node)], frame: Some(aabb(4.96, 0.0, 1.0, 1.0)) };
        let targets = Targets { points: vec![((4.91, 1.0), Kind::Node)], v_lines: vec![(5.0, Kind::Edge)], threshold: 0.1, ..Targets::default() };
        let opts = SnapOptions { nodes: false, ..SnapOptions::default() };
        let s = snap(&candidate, &targets, &opts);
        assert!((4.96 + s.dx - 5.0).abs() < 1e-5, "the frame edge meets the shape edge: {}", 4.96 + s.dx);
        assert!(matches!(s.marks.first(), Some(Mark::Line(Guide::V(_)))), "an edge line, not a cross: {:?}", s.marks);
    }

    #[test]
    fn the_grid_point_nearest_rounds_on_both_axes() {
        let g = SnapGrid { pitch_in: 0.125, origin: (0.0, 0.0) };
        assert_eq!(g.nearest((1.03, 2.0)), (1.0, 2.0));
    }
}
