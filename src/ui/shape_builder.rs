//! The Shape Builder tool (spec 57): with two or more shapes selected, it shows the planar map of their
//! evaluated outlines and lets a drag unite regions into one shape or Option-drag delete them. Each
//! release is one `Edit::BuildShape`, so one undo step; the map is rebuilt from the new selection after.

use egui::{Color32, Painter, Pos2, Response, Ui};

use crate::design::{Design, Pt};
use crate::edit::{Edit, Subject};
use crate::geometry::{FLATTEN_TOL_IN, Region, contains, planar_map};

use super::canvas::{Grid, View};

/// The most shapes the tool takes at once; its tooltip says so.
pub const MAX_INPUTS: usize = 8;

/// The planar map built for the current selection, and the gesture in progress.
pub struct State {
    inputs: Vec<u64>,
    regions: Vec<Region>,
    gesture: Option<Gesture>,
}

struct Gesture {
    /// Indices into `regions` the pointer has crossed.
    crossed: Vec<usize>,
    delete: bool,
}

impl State {
    /// Builds the map for `on` when it has two to eight selectable shapes with a non-empty map.
    fn for_selection(design: &Design, on: &[u64]) -> Option<State> {
        let inputs: Vec<u64> = design.drawn().filter(|s| on.contains(&s.id)).map(|s| s.id).collect();
        if inputs.len() < 2 || inputs.len() > MAX_INPUTS {
            return None;
        }
        let outlines: Vec<_> = design.drawn().filter(|s| inputs.contains(&s.id)).map(|s| design.geometry(s).rings(FLATTEN_TOL_IN)).collect();
        let regions = planar_map(&outlines);
        (!regions.is_empty()).then_some(State { inputs, regions, gesture: None })
    }

    /// The region index under `at` (world inches), front-most wins.
    fn region_at(&self, at: Pt) -> Option<usize> {
        self.regions.iter().enumerate().rev().find(|(_, r)| contains(&r.ring, at)).map(|(i, _)| i)
    }
}

/// Handles this frame's canvas input for the Shape Builder tool. Returns true when it used the input.
pub fn input(ui: &Ui, response: &Response, design: &mut Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid) -> bool {
    let on = selection.clone();
    if view.shape_builder.as_ref().map(|s| s.inputs.clone()) != Some(on.clone()) {
        view.shape_builder = State::for_selection(design, &on);
    }
    if view.shape_builder.is_none() {
        return false;
    }
    let alt = ui.input(|i| i.modifiers.alt);

    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let state = view.shape_builder.as_mut().expect("present");
        let crossed = state.region_at(grid.to_inches(p)).into_iter().collect();
        state.gesture = Some(Gesture { crossed, delete: alt });
        return true;
    }
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
    {
        let state = view.shape_builder.as_mut().expect("present");
        if let Some(i) = state.region_at(grid.to_inches(p))
            && let Some(gesture) = state.gesture.as_mut()
            && !gesture.crossed.contains(&i)
        {
            gesture.crossed.push(i);
        }
        return true;
    }
    if response.drag_stopped() {
        commit(design, selection, view);
        return true;
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let state = view.shape_builder.as_mut().expect("present");
        if let Some(i) = state.region_at(grid.to_inches(p)) {
            state.gesture = Some(Gesture { crossed: vec![i], delete: alt });
            commit(design, selection, view);
        }
        return true;
    }
    false
}

/// Runs the gesture as one `Edit::BuildShape`, then lets the next frame rebuild the map.
fn commit(design: &mut Design, selection: &mut Vec<u64>, view: &mut View) {
    let Some(state) = view.shape_builder.as_mut() else { return };
    let Some(gesture) = state.gesture.take() else { return };
    if gesture.crossed.is_empty() {
        return;
    }
    let picks: Vec<Pt> = gesture.crossed.iter().filter_map(|&i| state.regions.get(i)).map(Region::interior).collect();
    _ = crate::edit::run(design, selection, &Edit::BuildShape { picks, delete: gesture.delete }, Subject::Selection);
    view.shape_builder = None;
}

/// Paints the map: a thin outline on every region, and a hatched accent fill on the region under the
/// pointer (or every region the current gesture has crossed).
pub fn paint(painter: &Painter, grid: &Grid, view: &View, hover_in: Option<Pt>) {
    use super::tokens::*;
    let Some(state) = view.shape_builder.as_ref() else { return };
    for region in &state.regions {
        let line: Vec<Pos2> = region.ring.iter().map(|&p| grid.to_screen(p)).chain(region.ring.first().map(|&p| grid.to_screen(p))).collect();
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.0_f32, ACCENT_PRIMARY.gamma_multiply(0.5))));
    }
    let highlit: Vec<usize> = match state.gesture.as_ref() {
        Some(g) if !g.crossed.is_empty() => g.crossed.clone(),
        _ => hover_in.and_then(|p| state.region_at(p)).into_iter().collect(),
    };
    for region in highlit.iter().filter_map(|&i| state.regions.get(i)) {
        hatch(painter, grid, region, ACCENT_PRIMARY);
    }
}

/// A hatched accent fill: the region tinted, with diagonal accent lines across its bounds.
fn hatch(painter: &Painter, grid: &Grid, region: &Region, colour: Color32) {
    let poly: Vec<Pos2> = region.ring.iter().map(|&p| grid.to_screen(p)).collect();
    if poly.len() < 3 {
        return;
    }
    painter.add(egui::epaint::PathShape { points: poly.clone(), closed: true, fill: colour.gamma_multiply(0.18), stroke: egui::Stroke::new(1.5_f32, colour).into() });
    let bounds = egui::Rect::from_points(&poly);
    let clip = painter.clone().with_clip_rect(bounds);
    let mut x = bounds.left() - bounds.height();
    while x < bounds.right() {
        clip.line_segment([egui::pos2(x, bounds.top()), egui::pos2(x + bounds.height(), bounds.bottom())], egui::Stroke::new(1.0_f32, colour.gamma_multiply(0.55)));
        x += 6.0;
    }
}
