//! The canvas side of containers (spec 18): dragging into, out of and within them, and their dashed
//! padding frames.

use egui::{Painter, Stroke};

use crate::containers::inner_box;
use crate::design::{Design, Pt};

use super::canvas::Grid;
use super::tokens::ACCENT_PRIMARY;

/// A Move drag at `pointer`. Inside a laid-out container the dragged unit slides to the slot under the
/// pointer and the layout places it, so this returns true and the caller skips the free move.
pub(super) fn slide(design: &mut Design, selection: &[u64], pointer: Pt) -> bool {
    let roots = design.roots(selection);
    if !design.in_layout(&roots) {
        return false;
    }
    design.slide_in_layout(&roots, pointer);
    true
}

/// A Move drag released at `pointer`: the selection joins the container under it, or leaves its own.
pub(super) fn drop(design: &mut Design, selection: &[u64], pointer: Pt) {
    let roots = design.roots(selection);
    if !design.in_layout(&roots) {
        design.drop_at(&roots, pointer);
    }
}

/// A 1px dashed frame on the padded box of each selected container and of the container a drag at
/// `dragging_to` would drop into.
pub(super) fn paint(painter: &Painter, grid: &Grid, design: &Design, selection: &[u64], dragging_to: Option<Pt>) {
    let roots = design.roots(selection);
    let target = dragging_to.filter(|_| !design.in_layout(&roots)).and_then(|at| design.drop_target(&roots, at));
    let framed = design.shapes.iter().filter(|s| s.container.is_some() && !s.hidden && (selection.contains(&s.id) || target == Some(s.id)));
    for s in framed {
        let crate::snap::Aabb { min_x: x0, min_y: y0, max_x: x1, max_y: y1 } = inner_box(design, s);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)].map(|p| super::canvas::snap_px(grid.to_screen(p)));
        painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(1.0_f32, ACCENT_PRIMARY), 4.0, 3.0));
    }
}
