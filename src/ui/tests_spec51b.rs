//! Spec 51, the canvas half: the start and centre handles on a path text, node editing the path copy
//! (press A) so the letters reflow, the Text tool placing text on a shape's edge, and the overflow
//! plus. Everything is driven through the app with pointer and key events.

use egui::{Color32, Key, Modifiers, Pos2, vec2};

use super::canvas::Tool;
use super::test_harness::{Fixture, assert_snapshot, click_at, drag_along, key, move_to, snapshot_harness};
use crate::design::{Contour, Design, History, Seg, ShapeKind};
use crate::text::FontRef;
use crate::text_path::{Along, Side, TextPath};

const SIZE: egui::Vec2 = egui::vec2(1000.0, 700.0);

/// A text running round a circle, selected with the Select tool, so its path handles show.
fn text_on_circle(content: &str) -> (Fixture, u64) {
    let mut fx = snapshot_harness(SIZE);
    let mut design = Design::default();
    let text = design.add_text(content.into(), FontRef::Inter, 0.6, Color32::BLACK, 1.5, 2.0);
    let circle = design.add_shape(ShapeKind::Circle);
    if let Some(s) = design.shape_mut(circle) {
        (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 2.0, 4.0, 4.0);
        s.set_operation(crate::design::Operation::Guide);
    }
    design.set_snapping(false);
    design.text_on_path(text, circle);
    fx.state_mut().design = design;
    fx.state_mut().selection = vec![text];
    fx.state_mut().tool = Tool::Select;
    fx.state_mut().history = History::default();
    fx.run();
    (fx, text)
}

fn text_path_of(fx: &Fixture, id: u64) -> TextPath {
    match &fx.state().design.shapes.iter().find(|s| s.id == id).unwrap().kind {
        ShapeKind::Text { path: Some(p), .. } => p.clone(),
        _ => panic!("the text is on a path"),
    }
}

fn outline_points(fx: &Fixture, id: u64) -> Vec<(f32, f32)> {
    let s = fx.state().design.shapes.iter().find(|s| s.id == id).unwrap();
    s.world_polylines().into_iter().flat_map(|(pts, _)| pts).collect()
}

#[test]
fn the_start_handle_slides_the_run_along_the_path() {
    let (mut fx, text) = text_on_circle("Spectrum");
    assert!((text_path_of(&fx, text).offset_in).abs() < 1e-6, "a fresh attach starts unslid");
    let handles = fx.state().view.path_handles.as_ref().expect("a path text shows handles");
    let (start, centre) = (handles.start, handles.centre);

    drag_along(&mut fx.harness, &[start, start.lerp(centre, 0.5)]);
    assert!(text_path_of(&fx, text).offset_in.abs() > 1e-3, "dragging the start handle slid the run");
}

#[test]
fn the_centre_handle_sits_on_the_run_baseline() {
    let (fx, text) = text_on_circle("Spectrum");
    let (handle, baseline) = super::text_path::debug_centre_on_baseline(&fx.state().design, text).expect("a path text");
    // The handle lands on the baseline: a glyph pen sits within half an advance of it, both axes.
    assert!((handle.0 - baseline.0).hypot(handle.1 - baseline.1) < 0.15, "the centre handle sits on the letters: {handle:?} vs {baseline:?}");
}

#[test]
fn the_centre_handle_flips_the_text_below_the_path() {
    let (mut fx, text) = text_on_circle("Spectrum");
    assert_eq!(text_path_of(&fx, text).side, Side::Above, "it starts above the path");
    let centre = fx.state().view.path_handles.as_ref().unwrap().centre;

    // Drag the centre handle down, across the path into the circle, to flip the run to the other side.
    drag_along(&mut fx.harness, &[centre, centre + vec2(0.0, 70.0)]);
    assert_eq!(text_path_of(&fx, text).side, Side::Below, "the text flipped below the path");
}

#[test]
fn pressing_a_edits_the_path_nodes_and_the_letters_reflow() {
    let (mut fx, text) = text_on_circle("Spectrum");
    let before = outline_points(&fx, text);
    key(&mut fx.harness, Key::A, Modifiers::NONE);
    let dots = fx.state().view.path_nodes.as_ref().expect("A enters path-node editing").dots.clone();
    assert!(dots.len() >= 2, "the circle path shows its nodes");

    // Drag one node well off the circle; the path reshapes and the run follows it.
    drag_along(&mut fx.harness, &[dots[0], dots[0] + vec2(0.0, 120.0)]);
    assert!(fx.state().view.path_nodes.is_some(), "still editing nodes after the drag");
    assert_ne!(before, outline_points(&fx, text), "the letters reflowed onto the moved path");
}

#[test]
fn the_text_tool_places_text_on_a_shapes_edge() {
    let mut fx = snapshot_harness(SIZE);
    let mut design = Design::default();
    let circle = design.add_shape(ShapeKind::Circle);
    if let Some(s) = design.shape_mut(circle) {
        (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 2.0, 4.0, 4.0);
    }
    design.set_snapping(false);
    fx.state_mut().design = design;
    fx.state_mut().selection.clear();
    fx.state_mut().tool = Tool::Text;
    fx.state_mut().history = History::default();
    fx.run();

    // The top of the circle on screen, found from the artboard mapping the handles would use.
    let edge = circle_top_screen(&fx, circle);
    move_to(&mut fx.harness, edge);
    click_at(&mut fx.harness, edge);
    for ch in "Hello".chars() {
        fx.harness.input_mut().events.push(egui::Event::Text(ch.to_string()));
        fx.harness.run();
    }
    key(&mut fx.harness, Key::Escape, Modifiers::NONE);

    let placed = fx.state().design.shapes.iter().find(|s| matches!(&s.kind, ShapeKind::Text { path: Some(_), .. }));
    assert!(placed.is_some(), "clicking the circle's edge made a path text");
    assert!(matches!(&placed.unwrap().kind, ShapeKind::Text { content, .. } if content.contains("Hello")), "typing entered the text");
}

/// The screen point at the top of `circle`, via the same grid the canvas draws with.
fn circle_top_screen(fx: &Fixture, circle: u64) -> Pos2 {
    let s = fx.state().design.shapes.iter().find(|x| x.id == circle).unwrap();
    let top_in = (s.x_in + s.width_in / 2.0, s.y_in);
    let view = &fx.state().view;
    let viewport = view.nav.viewport.expect("the canvas has reported its viewport");
    let art = view.artboard_rect(viewport);
    let scale = 72.0 * view.zoom;
    art.min + vec2(top_in.0 * scale, top_in.1 * scale)
}

#[test]
fn handles_and_overflow_snapshots() {
    let (mut fx, _) = text_on_circle("Spectrum");
    assert_snapshot(&mut fx.harness, "51-path-handles");

    // A long run on a short open line overflows; the canvas marks the end with a red plus.
    let (mut fx, _) = text_on_short_line("Spectrumspectrum");
    assert_snapshot(&mut fx.harness, "51-overflow");
}

/// A long run on a short horizontal open line, so it overflows past the line's end.
fn text_on_short_line(content: &str) -> (Fixture, u64) {
    let mut fx = snapshot_harness(SIZE);
    let mut design = Design::default();
    let text = design.add_text(content.into(), FontRef::Inter, 0.5, Color32::BLACK, 3.0, 3.0);
    let before = design.shapes.iter().find(|s| s.id == text).unwrap().kind.clone();
    if let Some(s) = design.shape_mut(text)
        && let ShapeKind::Text { path, .. } = &mut s.kind
    {
        *path = Some(TextPath {
            contours: vec![Contour { start: (0.0, 0.5), segs: vec![Seg::Line((2.0, 0.5))], closed: false }],
            view_box: (2.0, 1.0),
            offset_in: 0.0,
            side: Side::Above,
            align: Along::Start,
        });
    }
    design.refit_text(text, &before);
    design.set_snapping(false);
    fx.state_mut().design = design;
    fx.state_mut().selection = vec![text];
    fx.state_mut().tool = Tool::Select;
    fx.state_mut().history = History::default();
    fx.run();
    (fx, text)
}
