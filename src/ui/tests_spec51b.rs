//! Spec 51, the canvas half: the start and centre handles on a path text, node editing the path copy
//! (press A) so the letters reflow, the Text tool placing text on a shape's edge, and the overflow
//! plus. Everything is driven through the app with pointer and key events.

use egui::{Color32, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;

use super::canvas::Tool;
use super::test_harness::{Fixture, assert_snapshot, click_at, drag_along, key, move_to, pick, right_click_at, snapshot_harness};
use crate::design::{Contour, Design, History, Seg, ShapeKind};
use crate::edit::{Edit, TextPathEdit};
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

/// A text and a heart apart on a fresh canvas, only the text selected, with the Select tool.
fn lone_text_and_heart() -> (Fixture, u64, u64) {
    // Wide enough that the edit toolbar keeps Effects out of its More menu beside the text controls.
    let mut fx = snapshot_harness(egui::vec2(1400.0, 900.0));
    let mut design = Design::default();
    let text = design.add_text("Spectrum".into(), FontRef::Inter, 0.6, Color32::BLACK, 1.0, 1.0);
    let heart = design.add_shape(ShapeKind::Heart);
    if let Some(s) = design.shape_mut(heart) {
        (s.x_in, s.y_in, s.width_in, s.height_in) = (4.0, 2.0, 3.5, 3.5);
        s.set_operation(crate::design::Operation::Guide);
    }
    design.set_snapping(false);
    fx.state_mut().design = design;
    fx.state_mut().selection = vec![text];
    fx.state_mut().tool = Tool::Select;
    fx.state_mut().history = History::default();
    fx.run();
    (fx, text, heart)
}

/// The screen point at `at_in` inches, via the artboard mapping the canvas draws with.
fn screen_of(fx: &Fixture, at_in: (f32, f32)) -> Pos2 {
    let view = &fx.state().view;
    let art = view.artboard_rect(view.nav.viewport.expect("the canvas has reported its viewport"));
    art.min + vec2(at_in.0, at_in.1) * 72.0 * view.zoom
}

fn on_path(fx: &Fixture, id: u64) -> bool {
    matches!(fx.state().design.shapes.iter().find(|s| s.id == id).map(|s| &s.kind), Some(ShapeKind::Text { path: Some(_), .. }))
}

#[test]
fn effects_text_on_path_over_a_lone_text_waits_for_a_click_on_the_shape() {
    let (mut fx, text, heart) = lone_text_and_heart();
    let before = fx.state().design.clone();

    fx.harness.get_by_label("Effects").click();
    fx.harness.run();
    pick(&mut fx.harness, "Text on path");
    assert_eq!(fx.state().view.path_pick, Some(text), "the menu starts a pick instead of editing");
    assert!(fx.state().design == before, "nothing changes until a shape is clicked");

    let at = screen_of(&fx, (5.75, 3.75));
    click_at(&mut fx.harness, at);
    assert!(on_path(&fx, text), "clicking the heart put the text on it");
    assert_eq!(fx.state().selection, vec![text], "the text is left selected");
    assert!(fx.state().view.path_pick.is_none(), "the pick is over");
    assert!(fx.state().design.shapes.iter().any(|s| s.id == heart), "the heart is kept");

    key(&mut fx.harness, Key::Z, Modifiers::COMMAND);
    assert!(fx.state().design == before, "one undo reverses the whole attach");
}

#[test]
fn the_context_menu_starts_the_same_pick() {
    let (mut fx, text, _) = lone_text_and_heart();
    let at = screen_of(&fx, (1.5, 1.3));
    right_click_at(&mut fx.harness, at);
    pick(&mut fx.harness, "Text on path");
    assert_eq!(fx.state().view.path_pick, Some(text));

    let at = screen_of(&fx, (5.75, 3.75));
    click_at(&mut fx.harness, at);
    assert!(on_path(&fx, text), "the click on the heart finished it");
}

#[test]
fn escape_or_a_click_on_nothing_leaves_the_text_alone() {
    let (mut fx, text, _) = lone_text_and_heart();
    let before = fx.state().design.clone();

    key(&mut fx.harness, Key::T, Modifiers::COMMAND.plus(Modifiers::ALT));
    assert_eq!(fx.state().view.path_pick, Some(text), "the shortcut starts the pick too");
    key(&mut fx.harness, Key::Escape, Modifiers::NONE);
    assert!(fx.state().view.path_pick.is_none(), "Escape cancels");
    assert!(fx.state().design == before);

    key(&mut fx.harness, Key::T, Modifiers::COMMAND.plus(Modifiers::ALT));
    let at = screen_of(&fx, (2.0, 5.0));
    click_at(&mut fx.harness, at);
    assert!(fx.state().view.path_pick.is_none(), "a click on empty canvas cancels");
    assert!(fx.state().design == before, "and attaches nothing");
}

#[test]
fn a_text_and_a_shape_selected_together_still_attach_at_once() {
    let (mut fx, text, heart) = lone_text_and_heart();
    fx.state_mut().selection = vec![text, heart];
    fx.run();

    fx.harness.get_by_label("Effects").click();
    fx.harness.run();
    pick(&mut fx.harness, "Text on path");
    assert!(fx.state().view.path_pick.is_none(), "no pick when the shape is already chosen");
    assert!(on_path(&fx, text), "the text is on the path in one step");
}

#[test]
fn the_disabled_tooltip_names_what_is_missing() {
    let (fx, _, heart) = lone_text_and_heart();
    let blocked = fx.state().design.check(&Edit::TextPath(TextPathEdit::Attach), &[heart]).expect_err("a shape alone cannot go on a path");
    assert_eq!(blocked, crate::edit::Blocked::NeedTextAndPath);
    assert!(blocked.reason().contains("Select a text"), "{}", blocked.reason());
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
