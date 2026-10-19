//! Builds the per-drag [`Targets`] and the moving [`Candidate`] from the design, the guides and the
//! grid, and caches each shape's snap points (nodes and segment midpoints) while its evaluated geometry
//! does not change. Point targets are gathered only for shapes within the viewport plus the threshold,
//! so a design of thousands of shapes does not pay for the ones off screen (spec 47).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::design::{Design, Pt, Shape};
use crate::effects::Geometry;
use crate::guides::{Axis, RulerGuide};

use super::{Aabb, Candidate, Kind, SnapGrid, SnapOptions, Targets};

/// Each shape's snap points, cached until its geometry changes. The key is the shared geometry `Rc`:
/// the geometry cache hands back the same `Rc` while the shape is untouched and a fresh one after an
/// edit, and holding the old `Rc` here keeps its address from being reused under a stale entry.
#[derive(Default)]
pub struct PointCache {
    by_shape: RefCell<HashMap<u64, Entry>>,
    builds: Cell<usize>,
}

struct Entry {
    geom: Rc<Geometry>,
    points: Rc<Vec<(Pt, Kind)>>,
}

impl PointCache {
    /// `shape`'s nodes and segment midpoints in world inches, rebuilt only when its geometry changes.
    pub fn points(&self, design: &Design, shape: &Shape) -> Rc<Vec<(Pt, Kind)>> {
        let geom = design.geometry(shape);
        if let Some(entry) = self.by_shape.borrow().get(&shape.id)
            && Rc::ptr_eq(&entry.geom, &geom)
        {
            return entry.points.clone();
        }
        let points = Rc::new(node_points(&geom));
        self.builds.set(self.builds.get() + 1);
        self.by_shape.borrow_mut().insert(shape.id, Entry { geom, points: points.clone() });
        points
    }

    /// Drops cached points for shapes no longer in the design, so a long session does not keep dead
    /// geometry alive. Called when a drag begins.
    pub fn retain(&self, live: &std::collections::HashSet<u64>) {
        self.by_shape.borrow_mut().retain(|id, _| live.contains(id));
    }

    /// How many times a shape's points have been built: the cache's miss count, for tests.
    #[cfg(test)]
    pub fn builds(&self) -> usize {
        self.builds.get()
    }
}

/// Every node and segment midpoint of one shape's evaluated outline, in world inches.
fn node_points(geom: &Geometry) -> Vec<(Pt, Kind)> {
    let mut out = Vec::new();
    for (line, _closed) in geom.lines() {
        for &p in line {
            out.push((p, Kind::Node));
        }
        for pair in line.windows(2) {
            out.push((midpoint(pair[0], pair[1]), Kind::Midpoint));
        }
    }
    out
}

fn midpoint(a: Pt, b: Pt) -> Pt {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

/// The ids of the shapes whose points are within reach of `viewport`, so only they contribute point
/// targets. `exclude` drops the shapes that are moving.
pub fn point_shapes(design: &Design, exclude: &[u64], viewport: Aabb, threshold: f32) -> Vec<u64> {
    let reach = viewport.expanded(threshold);
    design
        .drawn()
        .filter(|s| !s.hidden && !exclude.contains(&s.id))
        .filter(|s| {
            let b = design.geometry(s).bounds();
            !b.is_empty() && b.intersects(&reach)
        })
        .map(|s| s.id)
        .collect()
}

/// Everything in reach to snap to, gathered once per drag. `exclude` is the moving selection; `mat` is
/// the artboard box; `viewport` and `threshold` cull the point targets.
#[allow(clippy::too_many_arguments)]
pub fn targets(
    design: &Design,
    exclude: &[u64],
    guides: &[RulerGuide],
    grid: Option<SnapGrid>,
    mat: Aabb,
    viewport: Aabb,
    threshold: f32,
    options: &SnapOptions,
    cache: &PointCache,
) -> Targets {
    let mut t = Targets { grid: options.grid.then_some(grid).flatten(), threshold, ..Targets::default() };

    let others: Vec<(u64, Aabb)> =
        design.drawn().filter(|s| !s.hidden && !exclude.contains(&s.id)).map(|s| (s.id, design.geometry(s).bounds())).filter(|(_, b)| !b.is_empty()).collect();

    if options.shape_edges {
        for (_, b) in &others {
            for x in b.xs() {
                t.v_lines.push((x, Kind::Edge));
            }
            for y in b.ys() {
                t.h_lines.push((y, Kind::Edge));
            }
            t.points.push((b.centre(), Kind::Edge));
        }
    }
    if options.spacing {
        t.others = others.iter().map(|(_, b)| *b).collect();
    }
    if options.guides {
        for g in guides {
            match g.axis {
                Axis::Vertical => t.v_lines.push((g.at_in, Kind::Guide)),
                Axis::Horizontal => t.h_lines.push((g.at_in, Kind::Guide)),
            }
        }
    }
    if options.mat {
        let c = mat.centre();
        for p in mat.corners() {
            t.points.push((p, Kind::Mat));
        }
        t.points.push((c, Kind::Mat));
        t.v_lines.extend([(mat.min_x, Kind::Mat), (c.0, Kind::Mat), (mat.max_x, Kind::Mat)]);
        t.h_lines.extend([(mat.min_y, Kind::Mat), (c.1, Kind::Mat), (mat.max_y, Kind::Mat)]);
    }

    if options.nodes || options.intersections {
        let ids = point_shapes(design, exclude, viewport, threshold);
        let in_view: Vec<&Shape> = design.drawn().filter(|s| ids.contains(&s.id)).collect();
        if options.nodes {
            for &s in &in_view {
                t.points.extend(cache.points(design, s).iter().copied());
            }
        }
        if options.intersections {
            t.points.extend(intersections(design, &in_view).into_iter().map(|p| (p, Kind::Intersection)));
        }
    }
    t
}

/// Targets for a single dragged point, a resize handle or a ruler guide: lines, grid and the mat only,
/// with no nodes, intersections or spacing. The viewport is unused since no points are gathered.
pub fn line_targets(design: &Design, exclude: &[u64], guides: &[RulerGuide], grid: Option<SnapGrid>, mat: Aabb, threshold: f32, options: &SnapOptions) -> Targets {
    let lines_only = SnapOptions { nodes: false, intersections: false, spacing: false, ..*options };
    targets(design, exclude, guides, grid, mat, mat, threshold, &lines_only, &PointCache::default())
}

/// The moving selection as a [`Candidate`]: its frame's corners and centre, plus every path or text
/// node in it when the Nodes switch is on, all shifted by `delta`.
pub fn move_candidate(design: &Design, originals: &[(u64, Shape)], delta: Pt, options: &SnapOptions, cache: &PointCache) -> Candidate {
    let frame = originals.iter().map(|(_, s)| design.geometry(s).bounds()).filter(|b| !b.is_empty()).reduce(Aabb::union).map(|b| b.translate(delta.0, delta.1));
    let mut points: Vec<(Pt, Kind)> = frame.map(|f| super::Candidate::frame_points(f).collect()).unwrap_or_default();
    if options.nodes {
        for (_, s) in originals {
            points.extend(cache.points(design, s).iter().filter(|(_, k)| *k == Kind::Node).map(|&(p, k)| ((p.0 + delta.0, p.1 + delta.1), k)));
        }
    }
    Candidate { points, frame }
}

/// Straight segment intersections between different in-view shapes, the points where their outlines
/// cross. Curved pieces flatten to short segments, so this also catches near-straight crossings.
fn intersections(design: &Design, shapes: &[&Shape]) -> Vec<Pt> {
    let segments: Vec<Vec<(Pt, Pt)>> = shapes.iter().map(|&s| shape_segments(design, s)).collect();
    let mut out = Vec::new();
    for i in 0..segments.len() {
        for j in (i + 1)..segments.len() {
            for &a in &segments[i] {
                for &b in &segments[j] {
                    if let Some(p) = segment_cross(a, b) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

fn shape_segments(design: &Design, shape: &Shape) -> Vec<(Pt, Pt)> {
    let geom = design.geometry(shape);
    let mut segs = Vec::new();
    for (line, closed) in geom.lines() {
        for pair in line.windows(2) {
            segs.push((pair[0], pair[1]));
        }
        if *closed && let (Some(&first), Some(&last)) = (line.first(), line.last()) {
            segs.push((last, first));
        }
    }
    segs
}

/// Where two segments cross, or `None`. Standard parametric solve; parallels never report a crossing.
fn segment_cross((p1, p2): (Pt, Pt), (p3, p4): (Pt, Pt)) -> Option<Pt> {
    let (r_x, r_y) = (p2.0 - p1.0, p2.1 - p1.1);
    let (s_x, s_y) = (p4.0 - p3.0, p4.1 - p3.1);
    let denom = r_x * s_y - r_y * s_x;
    if denom.abs() < 1e-9 {
        return None;
    }
    let (q_x, q_y) = (p3.0 - p1.0, p3.1 - p1.1);
    let t = (q_x * s_y - q_y * s_x) / denom;
    let u = (q_x * r_y - q_y * r_x) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((p1.0 + t * r_x, p1.1 + t * r_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    fn square_at(design: &mut Design, x: f32, y: f32) -> u64 {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, 1.0, 1.0);
        id
    }

    fn viewport(x0: f32, y0: f32, x1: f32, y1: f32) -> Aabb {
        Aabb { min_x: x0, min_y: y0, max_x: x1, max_y: y1 }
    }

    #[test]
    fn point_targets_come_only_from_shapes_within_the_viewport() {
        let mut design = Design::default();
        for i in 0..2000 {
            let (col, rowi) = ((i % 50) as f32, (i / 50) as f32);
            square_at(&mut design, col * 2.0, rowi * 2.0);
        }
        // A viewport over a 7 by 6 inch window near the origin: it reaches about 20 of the 2 inch cells.
        let view = viewport(0.0, 0.0, 7.0, 6.0);
        let seen = point_shapes(&design, &[], view, 0.1);
        assert!(!seen.is_empty(), "some shapes are in view");
        assert!(seen.len() <= 20, "only the in-view shapes contribute points, not all 2000: {}", seen.len());
    }

    #[test]
    fn a_moving_shapes_descendants_are_not_snap_targets() {
        // A container holding a child; dragging the container must not snap to the child it refits around.
        let mut design = Design::default();
        let parent = square_at(&mut design, 1.0, 1.0);
        let child = square_at(&mut design, 5.0, 1.0);
        design.shape_mut(child).unwrap().parent = Some(parent);
        // The exclusion canvas builds: the moving shape plus its descendants.
        let exclude = design.with_descendants(&[parent]);
        let mat = viewport(0.0, 0.0, 12.0, 12.0);
        let view = viewport(0.0, 0.0, 12.0, 12.0);
        let targets = targets(&design, &exclude, &[], None, mat, view, 0.1, &SnapOptions::default(), &PointCache::default());
        assert!(!targets.v_lines.iter().any(|(x, _)| (*x - 5.0).abs() < 1e-3), "the child's left edge is not a target");
        assert!(targets.others.is_empty(), "the child is not offered for equal spacing either");
    }

    #[test]
    fn the_point_cache_is_reused_until_a_node_edit_rebuilds_it() {
        let mut design = Design::default();
        let id = square_at(&mut design, 1.0, 1.0);
        let cache = PointCache::default();
        let shape = design.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let first = cache.points(&design, &shape);
        assert_eq!(cache.builds(), 1);
        // A second call on the unchanged shape hits the cache.
        let again = cache.points(&design, &shape);
        assert_eq!(cache.builds(), 1, "reused while unchanged");
        assert!(Rc::ptr_eq(&first, &again));
        // Editing the shape changes its geometry, so the next call rebuilds.
        design.shape_mut(id).unwrap().width_in = 2.0;
        let edited = design.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let _ = cache.points(&design, &edited);
        assert_eq!(cache.builds(), 2, "rebuilt after the edit");
    }
}
