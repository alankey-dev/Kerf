//! The Measure tool, the read-out beside the pointer while a drag moves or resizes, and the red distance
//! lines Option-hover draws. None of it touches the design or its history.

use egui::{Color32, Event, Galley, Key, KeyboardShortcut, Modifiers, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, vec2};
use std::sync::Arc;

use crate::design::{Design, Pt};
use crate::measure::{DistanceLine, Measurement, constrain_45, distance_lines};
use crate::snap::{self, Aabb};
use crate::units::Units;

use super::canvas::Grid;
use super::fonts;
use super::tokens::*;

const SNAP_PX: f32 = 8.0;
const TICK_PX: f32 = 5.0;
const PILL_PAD: Vec2 = vec2(8.0, 4.0);
const READOUT_OFFSET: Vec2 = vec2(16.0, 18.0);
const DISTANCE_RED: Color32 = Color32::from_rgb(0xf2, 0x48, 0x22);

#[derive(Default)]
pub struct State {
    pub measurement: Option<Measurement>,
    /// The lines Option-hover drew this frame.
    pub distances: Vec<DistanceLine>,
}

/// Lays a new measurement with each drag, snapped while snapping is on; Shift holds it to 45 degree steps.
pub fn input(ui: &Ui, response: &Response, design: &Design, grid: &Grid, snapping: bool, artboard_in: Pt, measurement: &mut Option<Measurement>) {
    let snap = |p: Pos2| {
        let at = grid.to_inches(p);
        if !snapping {
            return at;
        }
        // The pointer snaps as a node, so it meets a corner, node, midpoint, centre or the mat whole,
        // rather than sliding along one axis as an edge would.
        let mat = Aabb { min_x: 0.0, min_y: 0.0, max_x: artboard_in.0, max_y: artboard_in.1 };
        let threshold = SNAP_PX / grid.scale;
        let targets = snap::cache::targets(design, &[], &[], None, mat, mat, threshold, &design.snap, &snap::cache::PointCache::default());
        let candidate = snap::Candidate { points: vec![(at, snap::Kind::Node)], frame: None };
        let s = snap::snap(&candidate, &targets, &design.snap);
        (at.0 + s.dx, at.1 + s.dy)
    };
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let from = snap(p);
        *measurement = Some(Measurement { from, to: from });
    }
    if response.dragged()
        && let (Some(m), Some(p)) = (measurement.as_mut(), response.interact_pointer_pos())
    {
        let to = snap(p);
        m.to = if ui.input(|i| i.modifiers.shift) { constrain_45(m.from, to) } else { to };
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        *measurement = None;
    }
}

/// Copies the measurement's label.
pub const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);

/// Paints the measurement and its label, and copies the label on [`COPY`].
pub fn show(ui: &mut Ui, painter: &Painter, grid: &Grid, measurement: &Measurement, units: Units) {
    let label = measurement.label(units);
    let copy = !ui.ctx().wants_keyboard_input() && ui.input_mut(|i| i.events.iter().any(|e| matches!(e, Event::Copy)) || i.consume_shortcut(&COPY));
    if copy {
        ui.ctx().copy_text(label.clone());
    }
    let (a, b) = (grid.to_screen(measurement.from), grid.to_screen(measurement.to));
    ruler(painter, a, b, Stroke::new(1.5_f32, ACCENT_PRIMARY));
    for end in [a, b] {
        painter.circle(end, 3.0, BG_PANEL, Stroke::new(1.5_f32, ACCENT_PRIMARY));
    }
    let galley = painter.layout_no_wrap(label.clone(), fonts::semibold(12.0), ON_PRIMARY);
    let at = a + (b - a) / 2.0 - galley.size() / 2.0 - PILL_PAD;
    labelled(ui, pill(painter, at, galley, ACCENT_PRIMARY), "measurement", label);
}

/// Paints `text` beside the pointer while a move or resize is under way.
pub fn readout(ui: &mut Ui, painter: &Painter, text: String, pointer: Pos2) {
    let galley = painter.layout_no_wrap(text.clone(), fonts::semibold(12.0), ON_PRIMARY);
    labelled(ui, pill(painter, pointer + READOUT_OFFSET, galley, TEXT_PRIMARY), "drag-readout", text);
}

/// With Option held, a selection and the pointer over another shape, paints the distance lines between
/// them and returns what it drew.
pub fn distances(ui: &mut Ui, painter: &Painter, grid: &Grid, design: &Design, selection: &[u64], units: Units) -> Vec<DistanceLine> {
    let (alt, hover) = ui.input(|i| (i.modifiers.alt, i.pointer.hover_pos()));
    let Some(pointer) = hover.filter(|&p| alt && painter.clip_rect().contains(p)).map(|p| grid.to_inches(p)) else { return Vec::new() };
    let Some(other) = design.hit_test(pointer.0, pointer.1).filter(|id| !selection.contains(id)).and_then(|id| design.shapes.iter().find(|s| s.id == id)) else {
        return Vec::new();
    };
    let chosen = design.shapes.iter().filter(|s| selection.contains(&s.id)).map(|s| design.geometry(s).bounds()).fold(Aabb::EMPTY, Aabb::union);
    if chosen.is_empty() {
        return Vec::new();
    }
    let lines = distance_lines(chosen, design.geometry(other).bounds());
    for (i, line) in lines.iter().enumerate() {
        let (a, b) = (grid.to_screen(line.from), grid.to_screen(line.to));
        ruler(painter, a, b, Stroke::new(1.0_f32, DISTANCE_RED));
        let text = units.len(line.length);
        let galley = painter.layout_no_wrap(text.clone(), fonts::semibold(11.0), ON_PRIMARY);
        let at = a + (b - a) / 2.0 - galley.size() / 2.0 - PILL_PAD;
        labelled(ui, pill(painter, at, galley, DISTANCE_RED), ("distance", i), text);
    }
    lines
}

/// A line with a short tick across each end.
fn ruler(painter: &Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    painter.line_segment([a, b], stroke);
    let across = (b - a).normalized().rot90() * TICK_PX;
    if across.is_finite() {
        for end in [a, b] {
            painter.line_segment([end - across, end + across], stroke);
        }
    }
}

fn pill(painter: &Painter, at: Pos2, galley: Arc<Galley>, fill: Color32) -> Rect {
    let rect = Rect::from_min_size(at, galley.size() + 2.0 * PILL_PAD);
    painter.rect_filled(rect, rect.height() / 2.0, fill);
    painter.galley(rect.min + PILL_PAD, galley, ON_PRIMARY);
    rect
}

fn labelled(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash, text: String) {
    ui.interact(rect, ui.id().with(id), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &text));
}
