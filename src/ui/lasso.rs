//! The freehand lasso (spec 50): an Option-drag on empty canvas draws a polygon that selects every
//! shape with an outline node inside it. This module owns the gesture (start, grow, close, cancel) and
//! the drawing; `select::lasso_hits` owns which shapes the closed polygon picks.

use egui::{Key, Painter, Pos2, Response, Stroke, Ui, pos2};

use crate::design::{Design, Pt};

use super::canvas::{Grid, View};
use super::tokens::ACCENT_PRIMARY;

/// A lasso shorter than this across is treated as a click, so a stray Option-click does not clear the
/// selection by drawing a one-pixel polygon.
const MIN_PX: f32 = 8.0;

/// A lasso in progress, carried across frames on the [`View`].
pub struct Lasso {
    /// The freehand path in inches; the polygon closes implicitly on the first point.
    pub(super) points: Vec<Pt>,
    /// Shift was held at the start, so the lasso adds to the selection instead of replacing it.
    additive: bool,
    /// The selection before the lasso began, restored on cancel and the base of an additive lasso.
    before: Vec<u64>,
}

/// Drives the lasso for one frame. Returns true when it handled the gesture, so the canvas leaves its
/// normal click, marquee and drag handling alone.
pub fn input(ui: &Ui, response: &Response, design: &Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid, shift: bool) -> bool {
    if view.lasso.is_some() && ui.input(|i| i.key_pressed(Key::Escape)) {
        if let Some(l) = view.lasso.take() {
            *selection = l.before;
        }
        return true;
    }
    if view.lasso.is_none() {
        let alt = ui.input(|i| i.modifiers.alt);
        if alt
            && response.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && is_empty_canvas(design, selection, grid, p)
        {
            let start = grid.to_inches(p);
            view.lasso = Some(Lasso { points: vec![start], additive: shift, before: selection.clone() });
            return true;
        }
        return false;
    }
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(l) = view.lasso.as_mut()
    {
        l.points.push(grid.to_inches(p));
        let hits = resolve(l, design);
        *selection = hits;
    }
    if response.drag_stopped()
        && let Some(l) = view.lasso.take()
    {
        *selection = if long_enough(&l, grid) {
            resolve(&l, design)
        } else if l.additive {
            l.before
        } else {
            Vec::new()
        };
    }
    true
}

/// The selection a lasso yields: the shapes it covers, added to the starting selection when additive.
fn resolve(l: &Lasso, design: &Design) -> Vec<u64> {
    let hits = crate::select::lasso_hits(design, &l.points);
    if !l.additive {
        return hits;
    }
    let mut out = l.before.clone();
    for id in hits {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// A lasso only starts where a marquee would: no handle and no shape under the press.
fn is_empty_canvas(design: &Design, selection: &[u64], grid: &Grid, p: Pos2) -> bool {
    let (x, y) = grid.to_inches(p);
    super::selection::grip_at(design, selection, grid, p).is_none() && design.pick_at(x, y).is_none()
}

/// Whether the gesture spans at least [`MIN_PX`] across on screen.
fn long_enough(l: &Lasso, grid: &Grid) -> bool {
    let pts: Vec<Pos2> = l.points.iter().map(|&p| grid.to_screen(p)).collect();
    let (mut min, mut max) = (pts[0], pts[0]);
    for p in &pts {
        min = pos2(min.x.min(p.x), min.y.min(p.y));
        max = pos2(max.x.max(p.x), max.y.max(p.y));
    }
    (max - min).length() >= MIN_PX
}

/// The lasso mid-drag: a faint accent fill under a 1 px dashed accent outline.
pub fn paint(painter: &Painter, grid: &Grid, view: &View) {
    let Some(l) = &view.lasso else { return };
    if l.points.len() < 2 {
        return;
    }
    let screen: Vec<Pos2> = l.points.iter().map(|&p| grid.to_screen(p)).collect();
    painter.add(egui::Shape::convex_polygon(screen.clone(), ACCENT_PRIMARY.gamma_multiply(0.08), Stroke::NONE));
    let mut closed = screen;
    closed.push(closed[0]);
    painter.add(egui::Shape::dashed_line(&closed, Stroke::new(1.0_f32, ACCENT_PRIMARY), 4.0, 4.0));
}
