//! Drawing the snap feedback (spec 47): the smart-guide line at an edge, guide or grid snap, a small
//! cross where two points meet, tick marks for equal spacing, and the faint ray Shift locks a move to.

use egui::{Painter, Pos2, Rect, Stroke, vec2};

use crate::snap::{Guide, Mark};

use super::canvas::Grid;
use super::tokens::ACCENT_PRIMARY;

const CROSS_PX: f32 = 5.0;
const TICK_PX: f32 = 4.0;

/// Paints every snap mark over the canvas.
pub fn draw(painter: &Painter, grid: &Grid, page: Rect, marks: &[Mark]) {
    let line = Stroke::new(1.0_f32, ACCENT_PRIMARY.gamma_multiply(0.9));
    let faint = Stroke::new(1.0_f32, ACCENT_PRIMARY.gamma_multiply(0.5));
    for mark in marks {
        match *mark {
            Mark::Line(Guide::V(x)) => {
                painter.vline(grid.to_screen((x, 0.0)).x, page.y_range(), line);
            }
            Mark::Line(Guide::H(y)) => {
                painter.hline(page.x_range(), grid.to_screen((0.0, y)).y, line);
            }
            Mark::Cross { at, .. } => cross(painter, grid.to_screen(at), line),
            Mark::Gap { a, b } => gap(painter, grid.to_screen(a), grid.to_screen(b), line),
            Mark::Ray { from, to } => {
                painter.line_segment([grid.to_screen(from), grid.to_screen(to)], faint);
            }
        }
    }
}

/// A small plus where a candidate point met a target point.
fn cross(painter: &Painter, c: Pos2, stroke: Stroke) {
    painter.line_segment([c - vec2(CROSS_PX, 0.0), c + vec2(CROSS_PX, 0.0)], stroke);
    painter.line_segment([c - vec2(0.0, CROSS_PX), c + vec2(0.0, CROSS_PX)], stroke);
}

/// The equalised gap, a line with a short tick across each end.
fn gap(painter: &Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    painter.line_segment([a, b], stroke);
    let across = (b - a).normalized().rot90() * TICK_PX;
    if across.is_finite() {
        for end in [a, b] {
            painter.line_segment([end - across, end + across], stroke);
        }
    }
}
