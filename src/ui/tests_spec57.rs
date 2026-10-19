//! Spec 57: Shape Builder and Knife. Dragging across the overlap regions of several shapes unites or
//! deletes them in one undo step, and a drawn Knife line splits the shapes it crosses. The pure planar
//! map and the cut live in `geometry::planar` and `knife`; these tests drive the tools through the app.

use egui::{Modifiers, Pos2, Vec2, pos2, vec2};

use crate::design::{Design, History, Operation, ShapeKind};
use crate::edit::{Edit, Subject};
use crate::ui::canvas::Tool;
use crate::ui::test_harness::{Fixture, assert_snapshot, click_at, move_to, press_at, snapshot_harness};

const SIZE: Vec2 = vec2(1400.0, 900.0);

/// A map from world inches to screen points, calibrated by placing a probe text and reading it back.
fn grid_map(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> Pos2 + use<> {
    fixture.state_mut().tool = Tool::Text;
    let probe = pos2(640.0, 420.0);
    click_at(fixture, probe);
    let text = fixture.state().design.shapes.last().unwrap().clone();
    let px = 72.0 * fixture.state().view.zoom;
    let origin = probe - vec2(text.x_in, text.y_in) * px;
    fixture.state_mut().design.remove_many(&[text.id]);
    fixture.state_mut().tool = Tool::Select;
    fixture.run();
    move |(x, y)| origin + vec2(x, y) * px
}

/// Two circles overlapping side by side at y = 3, both selected, snapping off.
fn two_overlapping_circles(fixture: &mut Fixture) -> Vec<u64> {
    let mut design = Design::default();
    let a = design.add_shape(ShapeKind::Circle);
    let b = design.add_shape(ShapeKind::Circle);
    for (id, x) in [(a, 2.0_f32), (b, 3.0_f32)] {
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (x, 2.0, 2.0, 2.0);
    }
    design.set_snapping(false);
    fixture.state_mut().design = design;
    fixture.state_mut().selection = vec![a, b];
    fixture.state_mut().history = History::default();
    fixture.run();
    vec![a, b]
}

/// A drag, holding `modifiers` for its whole length, so Shift-drags and Option-drags reach the tools.
fn drag_with(fixture: &mut Fixture, path: &[Pos2], modifiers: Modifiers) {
    fixture.input_mut().modifiers = modifiers;
    fixture.input_mut().events.push(egui::Event::PointerMoved(path[0]));
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: path[0], button: egui::PointerButton::Primary, pressed: true, modifiers });
    fixture.run();
    for &p in &path[1..] {
        fixture.input_mut().modifiers = modifiers;
        fixture.input_mut().events.push(egui::Event::PointerMoved(p));
        fixture.run();
    }
    fixture.input_mut().modifiers = modifiers;
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: *path.last().unwrap(), button: egui::PointerButton::Primary, pressed: false, modifiers });
    fixture.run();
    fixture.input_mut().modifiers = Modifiers::NONE;
    fixture.run();
}

/// An Option-click at `pos`.
fn alt_click(fixture: &mut Fixture, pos: Pos2) {
    let modifiers = Modifiers { alt: true, ..Modifiers::NONE };
    fixture.input_mut().modifiers = modifiers;
    fixture.input_mut().events.push(egui::Event::PointerMoved(pos));
    fixture.run();
    for pressed in [true, false] {
        fixture.input_mut().modifiers = modifiers;
        fixture.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers });
        fixture.run();
    }
    fixture.input_mut().modifiers = Modifiers::NONE;
    fixture.run();
}

#[test]
fn shape_builder_dragging_across_the_regions_of_two_circles_makes_one_shape() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    two_overlapping_circles(&mut fixture);
    fixture.state_mut().tool = Tool::ShapeBuilder;
    fixture.run();
    fixture.run();

    // Drag along y = 3 through the left crescent, the lens and the right crescent.
    drag_with(&mut fixture, &[map((2.4, 3.0)), map((3.5, 3.0)), map((4.6, 3.0))], Modifiers::NONE);

    assert_eq!(fixture.state().design.shapes.len(), 1, "one shape results");
    let shape = fixture.state().design.shapes[0].clone();
    assert_eq!(fixture.state().design.geometry(&shape).pieces().len(), 1, "Make sees one piece");

    // One undo restores the two circles.
    let app = fixture.state_mut();
    let mut design = std::mem::take(&mut app.design);
    app.history.undo(&mut design);
    app.design = design;
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 2, "undo restores two circles");
}

#[test]
fn shape_builder_option_click_on_the_lens_leaves_two_crescents() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    two_overlapping_circles(&mut fixture);
    fixture.state_mut().tool = Tool::ShapeBuilder;
    fixture.run();
    fixture.run();

    alt_click(&mut fixture, map((3.5, 3.0)));

    assert_eq!(fixture.state().design.shapes.len(), 2, "two crescents remain");
    assert!(fixture.state().design.shapes.iter().all(|s| !crate::effects::evaluate(s).bounds().is_empty()), "both crescents have area");
}

#[test]
fn shape_builder_hover_snapshot() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    two_overlapping_circles(&mut fixture);
    fixture.state_mut().tool = Tool::ShapeBuilder;
    fixture.run();
    fixture.run();
    move_to(&mut fixture, map((3.5, 3.0)));
    fixture.run();
    assert_snapshot(&mut fixture, "57-shape-builder-hover");
}

#[test]
fn knife_shift_drag_across_a_heart_makes_heart_and_heart_two() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let mut design = Design::default();
    let heart = design.add_shape(ShapeKind::Heart);
    let s = design.shape_mut(heart).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    design.set_snapping(false);
    fixture.state_mut().design = design;
    fixture.state_mut().selection = vec![heart];
    fixture.state_mut().history = History::default();
    fixture.state_mut().tool = Tool::Knife;
    fixture.run();

    // A straight horizontal cut across the middle of the heart.
    drag_with(&mut fixture, &[map((2.5, 4.0)), map((5.5, 4.0))], Modifiers { shift: true, ..Modifiers::NONE });

    let names: Vec<String> = {
        let design = &fixture.state().design;
        design.shapes.iter().map(|s| design.layer_name(s)).collect()
    };
    assert_eq!(fixture.state().design.shapes.len(), 2, "two pieces: {names:?}");
    assert!(names.contains(&"Heart".to_owned()) && names.contains(&"Heart 2".to_owned()), "named Heart and Heart 2: {names:?}");
    let selection = fixture.state().selection.clone();
    assert_eq!(selection.len(), 2, "both pieces selected");
}

#[test]
fn knife_mid_drag_snapshot() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let mut design = Design::default();
    let heart = design.add_shape(ShapeKind::Heart);
    let s = design.shape_mut(heart).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    design.set_snapping(false);
    fixture.state_mut().design = design;
    fixture.state_mut().selection = vec![heart];
    fixture.state_mut().history = History::default();
    fixture.state_mut().tool = Tool::Knife;
    fixture.run();

    // Press and move across the heart, leaving the line mid-drag for the snapshot.
    move_to(&mut fixture, map((2.5, 4.0)));
    press_at(&mut fixture, map((2.5, 4.0)), true);
    move_to(&mut fixture, map((5.5, 4.0)));
    assert_snapshot(&mut fixture, "57-knife");
    // Release so the fixture ends cleanly.
    press_at(&mut fixture, map((5.5, 4.0)), false);
}

// ---------------------------------------------------------------------------
// Design-level acceptance for the cut, driven straight through `Edit`.

#[test]
fn knife_over_a_live_offset_expands_it_and_the_pieces_carry_no_effects() {
    let mut design = Design::default();
    let sq = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(sq).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (2.0, 2.0, 2.0, 2.0);
    let mut selection = vec![sq];
    // Add a live Offset around the square.
    let offset = crate::effects::Offset { distance_in: 0.25, direction: crate::effects::Direction::Outset, join: crate::geometry::Join::Round, style: None };
    run_edit(&mut design, &mut selection, &Edit::AddEffect { effect: crate::effects::Effect::Offset(offset), reach: crate::effects::Reach::EachShape });
    assert!(!design.shapes[0].effects.is_empty(), "the square carries a live Offset");

    // Cut straight down the middle, x = 3.
    run_edit(&mut design, &mut selection, &Edit::Knife { line: vec![(3.0, 0.5), (3.0, 5.5)] });
    assert_eq!(design.shapes.len(), 2, "two pieces");
    assert!(design.shapes.iter().all(|s| s.effects.is_empty()), "the pieces carry no effects");
    assert!(design.shapes.iter().all(|s| matches!(s.operation, Operation::BasicCut)), "the operation is kept");
}

#[test]
fn scissors_click_opens_a_closed_path_at_the_nearest_node() {
    let mut design = Design::default();
    // A closed triangle path.
    let contour = crate::design::Contour { start: (0.0, 0.0), segs: vec![crate::design::Seg::Line((2.0, 0.0)), crate::design::Seg::Line((1.0, 2.0))], closed: true };
    let id = design.add_path(contour, egui::Color32::BLACK, Operation::BasicCut).unwrap();
    let mut selection = vec![id];
    // A click near the (0,0) corner (the shape sits at its own origin after add_path).
    let node = design.shapes[0].to_world((0.0, 0.0));
    run_edit(&mut design, &mut selection, &Edit::Knife { line: vec![node] });
    assert_eq!(design.shapes.len(), 1, "still one path");
    match &design.shapes[0].kind {
        ShapeKind::Path { contours, .. } => assert!(contours.iter().all(|c| !c.closed), "the path is now open"),
        other => panic!("{other:?} is not a path"),
    }
}

fn run_edit(design: &mut Design, selection: &mut Vec<u64>, edit: &Edit) {
    crate::edit::run(design, selection, edit, Subject::Selection).unwrap();
}
