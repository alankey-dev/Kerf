//! The Knife tool (spec 57): drag a freehand line, or Shift-drag a straight one, across shapes to cut
//! them; a click with no drag is the Scissors, opening or snipping the path under it at a node. Both go
//! through `Edit::Knife`, so each is one undo step. Nothing selected means the line cuts everything it
//! crosses; a selection limits it to those shapes.

use egui::{Pos2, Response, Ui};

use crate::design::{Design, Pt};
use crate::edit::{Edit, Subject};
use crate::geometry::FLATTEN_TOL_IN;

use super::canvas::{Grid, View};

/// How near a Scissors click snaps to a node, in screen pixels.
const SNAP_PX: f32 = 8.0;
/// How near a Scissors click must fall to a path to cut it, in screen pixels.
const REACH_PX: f32 = 10.0;

/// The line being drawn across the canvas, in world inches.
pub struct Drag {
    pub points: Vec<Pt>,
    straight: bool,
}

/// Handles this frame's canvas input for the Knife tool. Returns true when it used the input.
pub fn input(ui: &Ui, response: &Response, design: &mut Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid) -> bool {
    let shift = ui.input(|i| i.modifiers.shift);
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        view.knife = Some(Drag { points: vec![grid.to_inches(p)], straight: shift });
        return true;
    }
    if response.dragged()
        && let (Some(drag), Some(p)) = (view.knife.as_mut(), response.interact_pointer_pos())
    {
        let at = grid.to_inches(p);
        if drag.straight {
            drag.points.truncate(1);
            drag.points.push(at);
        } else {
            drag.points.push(at);
        }
        return true;
    }
    if response.drag_stopped() {
        if let Some(drag) = view.knife.take() {
            cut(design, selection, &drag.points);
        }
        return true;
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        scissors(design, selection, grid, p);
        return true;
    }
    false
}

/// Runs the drag as one `Edit::Knife` over the shapes it crosses, leaving the pieces selected.
fn cut(design: &mut Design, selection: &mut Vec<u64>, line: &[Pt]) {
    if line.len() < 2 {
        return;
    }
    let candidates: Vec<u64> = if selection.is_empty() { design.drawn().map(|s| s.id).collect() } else { selection.clone() };
    let targets: Vec<u64> = candidates.into_iter().filter(|&id| crosses(design, id, line)).collect();
    if targets.is_empty() {
        return;
    }
    *selection = targets;
    _ = crate::edit::run(design, selection, &Edit::Knife { line: line.to_vec() }, Subject::Selection);
}

/// Runs a Scissors click: the nearest path edge within reach is opened or snipped at the click, snapped
/// to a node when one is near.
fn scissors(design: &mut Design, selection: &mut Vec<u64>, grid: &Grid, at: Pos2) {
    let reach_in = REACH_PX / grid.scale;
    let candidates: Vec<u64> = if selection.is_empty() { design.drawn().map(|s| s.id).collect() } else { selection.clone() };
    let world = grid.to_inches(at);
    let nearest = candidates
        .into_iter()
        .filter_map(|id| design.shapes.iter().find(|s| s.id == id).map(|s| (id, edge_distance(design, s, world))))
        .filter(|(_, d)| *d <= reach_in)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((id, _)) = nearest else { return };
    let shape = design.shapes.iter().find(|s| s.id == id).expect("just found");
    let point = snap_to_node(design, shape, grid, at).unwrap_or(world);
    *selection = vec![id];
    _ = crate::edit::run(design, selection, &Edit::Knife { line: vec![point] }, Subject::Selection);
}

/// Whether `line` crosses `id`'s evaluated boundary: any line segment meets any ring or open-contour edge.
fn crosses(design: &Design, id: u64, line: &[Pt]) -> bool {
    let Some(shape) = design.shapes.iter().find(|s| s.id == id) else { return false };
    let geom = design.geometry(shape);
    let rings = geom.rings(FLATTEN_TOL_IN);
    let boundaries =
        rings.iter().map(|r| (r.clone(), true)).chain(geom.pieces().iter().flat_map(|p| geom.world_contours(p)).filter(|c| !c.closed).map(|c| (c.flatten(), false)));
    for (pts, closed) in boundaries {
        let mut edge = pts.clone();
        if closed && pts.len() > 1 {
            edge.push(pts[0]);
        }
        for e in edge.windows(2) {
            for l in line.windows(2) {
                if segments_cross(e[0], e[1], l[0], l[1]) {
                    return true;
                }
            }
        }
    }
    false
}

/// The shortest distance from `at` to `shape`'s evaluated boundary, in inches.
fn edge_distance(design: &Design, shape: &crate::design::Shape, at: Pt) -> f32 {
    let geom = design.geometry(shape);
    let rings = geom.rings(FLATTEN_TOL_IN);
    let closed = rings.iter().map(|r| crate::knife::distance_to(r, true, at));
    let open =
        geom.pieces().iter().flat_map(|p| geom.world_contours(p)).filter(|c| !c.closed).map(|c| crate::knife::distance_to(&c.flatten(), false, at)).collect::<Vec<_>>();
    closed.chain(open).fold(f32::INFINITY, f32::min)
}

/// The nearest evaluated node of `shape` to the click, in world inches, when within [`SNAP_PX`].
fn snap_to_node(design: &Design, shape: &crate::design::Shape, grid: &Grid, at: Pos2) -> Option<Pt> {
    let geom = design.geometry(shape);
    let nodes = geom.rings(FLATTEN_TOL_IN).into_iter().flatten().chain(geom.pieces().iter().flat_map(|p| geom.world_contours(p)).flat_map(|c| c.flatten()));
    nodes.map(|p| (p, grid.to_screen(p).distance(at))).filter(|(_, d)| *d <= SNAP_PX).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(p, _)| p)
}

/// Whether segments `a0a1` and `b0b1` cross strictly (not merely touching at an endpoint of the ring).
fn segments_cross(a0: Pt, a1: Pt, b0: Pt, b1: Pt) -> bool {
    let d = |p: Pt, q: Pt, r: Pt| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (d1, d2, d3, d4) = (d(b0, b1, a0), d(b0, b1, a1), d(a0, a1, b0), d(a0, a1, b1));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// Paints the knife line in progress.
pub fn paint(painter: &egui::Painter, grid: &Grid, view: &View) {
    use super::tokens::*;
    let Some(drag) = view.knife.as_ref() else { return };
    let line: Vec<Pos2> = drag.points.iter().map(|&p| grid.to_screen(p)).collect();
    if line.len() >= 2 {
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5_f32, ACCENT_PRIMARY)));
    }
}
