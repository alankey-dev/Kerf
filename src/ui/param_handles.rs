//! Canvas parameter handles (spec 49). A single selected parametric shape shows small diamond handles
//! for its parameters. The rectangle's four corner handles drag its radius inward; other kinds are
//! edited in the inspector's Shape section. A drag shows the radius beside the handle and commits as one
//! undo step, since the change coalesces while the pointer is held. Handles never show on a
//! multi-selection, and they hide while a drag is in flight.

use egui::{Align2, Painter, Pos2, Vec2};

use crate::design::{Design, Pt, Shape, ShapeKind};
use crate::expr::Typed;
use crate::parametric::{ParamOp, Params};
use crate::units::Units;

use super::canvas::Grid;
use super::fonts;
use super::tokens::*;

/// How near the pointer must come to a handle to grab it.
pub const REACH_PX: f32 = 11.0;
const DIAMOND_PX: f32 = 5.0;

/// Which parameter handle is being pointed at or dragged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamGrip {
    /// A rectangle corner, in the order top-left, top-right, bottom-right, bottom-left.
    Corner(usize),
}

/// The inward diagonal unit vector at each rectangle corner, toward the box centre.
const CORNER_DIRS: [Pt; 4] = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)];
const ROOT2_INV: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Each handle's grip and its position in the shape's local frame, or empty for a shape with no canvas
/// handles.
fn handles(shape: &Shape) -> Vec<(ParamGrip, Pt)> {
    let ShapeKind::Parametric(Params::Rectangle { corners, .. }) = &shape.kind else {
        return Vec::new();
    };
    let (w, h) = (shape.width_in, shape.height_in);
    let limit = 0.5 * w.min(h);
    let box_corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    (0..4)
        .map(|i| {
            let r = corners[i].clamp(0.0, limit);
            let (cx, cy) = box_corners[i];
            let (dx, dy) = CORNER_DIRS[i];
            (ParamGrip::Corner(i), (cx + dx * ROOT2_INV * r, cy + dy * ROOT2_INV * r))
        })
        .collect()
}

/// The handle the pointer is on, for a single parametric selection, else `None`.
pub fn grip_at(design: &Design, selection: &[u64], grid: &Grid, p: Pos2) -> Option<ParamGrip> {
    let shape = single(design, selection)?;
    handles(shape)
        .into_iter()
        .map(|(grip, local)| (grip, grid.to_screen(shape.to_world(local)).distance(p)))
        .filter(|(_, d)| *d <= REACH_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(grip, _)| grip)
}

/// Drags `grip` to `pointer_in`, editing the live shape from `original` so the radius tracks the pointer
/// cleanly however the drag wanders.
pub fn drag(design: &mut Design, original: &Shape, grip: ParamGrip, pointer_in: Pt) {
    let ParamGrip::Corner(i) = grip;
    let (w, h) = (original.width_in, original.height_in);
    let box_corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let (cx, cy) = box_corners[i.min(3)];
    let (dx, dy) = CORNER_DIRS[i.min(3)];
    let (lx, ly) = original.to_local(pointer_in);
    // The radius is how far the pointer lies inward along the corner's diagonal.
    let r = ((lx - cx) * dx * ROOT2_INV + (ly - cy) * dy * ROOT2_INV).max(0.0);
    if let (Some(s), ShapeKind::Parametric(base)) = (design.shape_mut(original.id), &original.kind) {
        let mut params = *base;
        params.apply(ParamOp::Corner { at: i.min(3), value: Typed::To(r) });
        s.kind = ShapeKind::Parametric(params);
    }
}

/// Paints the handles for a single parametric selection. While a corner is being dragged, the diamonds
/// give way to a read-out of the radius beside that corner.
pub fn paint(painter: &Painter, grid: &Grid, design: &Design, selection: &[u64], active: Option<ParamGrip>, dragging: bool, units: Units) {
    let Some(shape) = single(design, selection) else { return };
    if dragging {
        if let Some(ParamGrip::Corner(i)) = active
            && let ShapeKind::Parametric(Params::Rectangle { corners, .. }) = &shape.kind
        {
            let r = corners[i.min(3)].clamp(0.0, 0.5 * shape.width_in.min(shape.height_in));
            if let Some((_, local)) = handles(shape).into_iter().find(|(g, _)| *g == ParamGrip::Corner(i.min(3))) {
                let at = grid.to_screen(shape.to_world(local)) + Vec2::new(10.0, -10.0);
                painter.text(at, Align2::LEFT_CENTER, units.len(r), fonts::regular(12.0), TEXT_PRIMARY);
            }
        }
        return;
    }
    for (_, local) in handles(shape) {
        diamond(painter, grid.to_screen(shape.to_world(local)));
    }
}

fn diamond(painter: &Painter, at: Pos2) {
    let pts = [at + Vec2::new(0.0, -DIAMOND_PX), at + Vec2::new(DIAMOND_PX, 0.0), at + Vec2::new(0.0, DIAMOND_PX), at + Vec2::new(-DIAMOND_PX, 0.0)];
    painter.add(egui::Shape::convex_polygon(pts.to_vec(), BG_PANEL, egui::Stroke::new(1.5_f32, ACCENT_PRIMARY)));
}

fn single<'a>(design: &'a Design, selection: &[u64]) -> Option<&'a Shape> {
    let [id] = selection else { return None };
    design.shapes.iter().find(|s| s.id == *id).filter(|s| matches!(s.kind, ShapeKind::Parametric(_)))
}
