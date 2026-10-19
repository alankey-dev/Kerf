//! Spec 47: snapping to another shape's node, equal spacing, the Shift angle constraints on a move and
//! a Pen segment, and the Snapping popover, all driven through the app.

use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;

use super::canvas::Tool;
use super::test_harness::{Fixture, any_near, assert_snapshot, click_at, double_click_at, frame, key, move_to, press_at, snapshot_harness, two_squares_apart};
use super::tokens::ACCENT_PRIMARY;
use crate::design::{Design, ShapeKind};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// Every world node of a shape's evaluated outline.
fn world_nodes(design: &Design, id: u64) -> Vec<(f32, f32)> {
    let shape = design.shapes.iter().find(|s| s.id == id).unwrap().clone();
    design.geometry(&shape).lines().flat_map(|(pts, _)| pts.clone()).collect()
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// A drag that holds `modifiers` down the whole gesture, through each point in `path`.
fn drag_with(fixture: &mut Fixture, modifiers: Modifiers, path: &[Pos2]) {
    let button = egui::PointerButton::Primary;
    fixture.input_mut().modifiers = modifiers;
    fixture.input_mut().events.push(egui::Event::PointerMoved(path[0]));
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: path[0], button, pressed: true, modifiers });
    fixture.run();
    for &p in &path[1..] {
        fixture.input_mut().modifiers = modifiers;
        fixture.input_mut().events.push(egui::Event::PointerMoved(p));
        fixture.run();
    }
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: *path.last().unwrap(), button, pressed: false, modifiers });
    fixture.run();
    fixture.input_mut().modifiers = Modifiers::NONE;
    fixture.run();
}

/// A click that holds `modifiers` down.
fn click_with(fixture: &mut Fixture, modifiers: Modifiers, pos: Pos2) {
    let button = egui::PointerButton::Primary;
    fixture.input_mut().modifiers = modifiers;
    fixture.input_mut().events.push(egui::Event::PointerMoved(pos));
    fixture.run();
    for pressed in [true, false] {
        fixture.input_mut().modifiers = modifiers;
        fixture.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers });
        fixture.run();
    }
    fixture.input_mut().modifiers = Modifiers::NONE;
    fixture.run();
}

#[test]
fn a_star_corner_dragged_near_a_heart_node_snaps_to_it_with_a_cross() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    {
        let app = fixture.state_mut();
        app.design.shapes.clear();
        let heart = app.design.add_shape(ShapeKind::Heart);
        let star = app.design.add_shape(ShapeKind::Star);
        let place = |s: &mut crate::design::Shape, x: f32, y: f32| (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, 2.5, 2.5);
        place(app.design.shape_mut(heart).unwrap(), 7.0, 3.0);
        place(app.design.shape_mut(star).unwrap(), 2.0, 2.5);
        app.design.set_snapping(true);
        app.selection = vec![star];
    }
    fixture.run();
    let star = fixture.state().design.shapes.iter().find(|s| s.width_in == 2.5 && s.x_in == 2.0).unwrap().id;
    let heart = fixture.state().design.shapes.iter().find(|s| s.id != star).unwrap().id;

    // The two nearest nodes between the shapes are the pair the drag will close.
    let star_nodes = world_nodes(&fixture.state().design, star);
    let heart_nodes = world_nodes(&fixture.state().design, heart);
    let (source, target) = star_nodes.iter().flat_map(|&s| heart_nodes.iter().map(move |&h| (s, h))).min_by(|a, b| dist(a.0, a.1).total_cmp(&dist(b.0, b.1))).unwrap();

    // Drag the star so its node stops about 0.05 in short of the heart node, within the snap threshold.
    let delta = (target.0 - source.0, target.1 - source.1);
    let len = dist(source, target);
    let scale = (len - 0.05) / len;
    let drag = (delta.0 * scale, delta.1 * scale);
    let sq = fixture.state().design.shapes.iter().find(|s| s.id == star).unwrap().clone();
    // Grab off the star's centre so the drag moves it, not its pivot crosshair (spec 52).
    let centre = at((sq.x_in + sq.width_in / 4.0, sq.y_in + sq.height_in / 2.0));
    let px = 72.0 * fixture.state().view.zoom;
    let to = centre + vec2(drag.0, drag.1) * px;
    let path: Vec<Pos2> = (0..=6).map(|i| centre + (to - centre) * (i as f32 / 6.0)).collect();

    move_to(&mut fixture, path[0]);
    press_at(&mut fixture, path[0], true);
    for &p in &path[1..] {
        move_to(&mut fixture, p);
    }
    let image = frame(&mut fixture);
    let meet = at(target);
    assert!(any_near(&image, egui::Rect::from_center_size(meet, egui::Vec2::splat(10.0)), ACCENT_PRIMARY, 24), "a cross is drawn at the meeting point");
    assert_snapshot(&mut fixture, "47-snap-node");
    press_at(&mut fixture, *path.last().unwrap(), false);
    fixture.run();

    let moved = world_nodes(&fixture.state().design, star);
    let nearest = moved.iter().map(|&n| dist(n, target)).fold(f32::MAX, f32::min);
    assert!(nearest < 1e-3, "a star node now sits on the heart node, within {nearest} inches");
}

#[test]
fn shift_locks_a_diagonal_move_to_the_45_degree_ray() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    let sq = fixture.state().design.shapes[0].clone();
    fixture.state_mut().selection = vec![sq.id];
    fixture.run();
    let centre = (sq.x_in + sq.width_in / 2.0, sq.y_in + sq.height_in / 2.0);
    // Grab off the centre so the drag moves the shape, not its pivot crosshair (spec 52).
    let off = sq.width_in / 4.0;
    let grab = at((centre.0 - off, centre.1));
    // A move of (1.5, 1.2) in is nearer 45 degrees than either axis, so it locks to the diagonal.
    let to = at((centre.0 - off + 1.5, centre.1 + 1.2));
    let path: Vec<Pos2> = (0..=6).map(|i| grab + (to - grab) * (i as f32 / 6.0)).collect();
    drag_with(&mut fixture, Modifiers::SHIFT, &path);

    let moved = fixture.state().design.shapes.iter().find(|s| s.id == sq.id).unwrap();
    let (dx, dy) = (moved.x_in - sq.x_in, moved.y_in - sq.y_in);
    assert!(dx > 0.1 && (dx - dy).abs() < 1e-2, "the move holds the 45 degree ray: ({dx}, {dy})");
}

#[test]
fn shift_draws_the_ray_while_a_move_is_constrained() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    let sq = fixture.state().design.shapes[0].clone();
    fixture.state_mut().selection = vec![sq.id];
    fixture.run();
    let centre = (sq.x_in + sq.width_in / 2.0, sq.y_in + sq.height_in / 2.0);
    // Grab along the ray, off the pivot crosshair, so the shape moves and the 45 degree ray is unchanged (spec 52).
    let off = sq.width_in / 4.0;
    let grab = at((centre.0 - off, centre.1 - off));
    let to = at((centre.0 - off + 2.0, centre.1 - off + 1.6));
    move_to(&mut fixture, grab);
    fixture.input_mut().modifiers = Modifiers::SHIFT;
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: grab, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::SHIFT });
    fixture.run();
    for i in 1..=6 {
        let p = grab + (to - grab) * (i as f32 / 6.0);
        fixture.input_mut().modifiers = Modifiers::SHIFT;
        fixture.input_mut().events.push(egui::Event::PointerMoved(p));
        fixture.run();
    }
    // The faint ray runs from the start along 45 degrees; a point on it is tinted, not bare artboard.
    let on_ray = at((centre.0 + 0.9, centre.1 + 0.9));
    let image = frame(&mut fixture);
    assert!(any_near(&image, egui::Rect::from_center_size(on_ray, egui::Vec2::splat(6.0)), ACCENT_PRIMARY, 40), "the 45 degree ray is drawn");
    fixture.input_mut().modifiers = Modifiers::NONE;
}

#[test]
fn shift_locks_a_node_drag_to_its_axis() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().design.shapes.clear();
    fixture.state_mut().tool = Tool::Pen;
    fixture.run();
    for p in [(3.0, 5.0), (5.0, 5.0), (7.0, 5.0)] {
        click_at(&mut fixture, at(p));
    }
    key(&mut fixture, Key::Enter, Modifiers::NONE);
    fixture.state_mut().selection.clear();
    fixture.run();

    double_click_at(&mut fixture, at((5.0, 5.0)));
    assert!(fixture.state().view.nodes.is_some(), "the path's nodes are editable");
    let path_id = fixture.state().design.shapes.last().unwrap().id;

    // Shift-drag the middle node mostly upward; it locks to the vertical axis and keeps its x.
    let from = at((5.0, 5.0));
    let to = at((5.3, 6.5));
    let steps: Vec<Pos2> = (0..=6).map(|i| from + (to - from) * (i as f32 / 6.0)).collect();
    drag_with(&mut fixture, Modifiers::SHIFT, &steps);

    let node = world_nodes(&fixture.state().design, path_id)[1];
    assert!((node.0 - 5.0).abs() < 0.02, "the node held its x, at {}", node.0);
    assert!(node.1 > 5.6, "the node moved up its axis, at {}", node.1);
}

#[test]
fn shift_holds_a_pen_segment_horizontal() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().design.shapes.clear();
    fixture.state_mut().tool = Tool::Pen;
    fixture.run();
    click_at(&mut fixture, at((3.0, 4.0)));
    // A shallow second click, Shift held, locks the segment to horizontal.
    click_with(&mut fixture, Modifiers::SHIFT, at((7.0, 4.3)));
    key(&mut fixture, Key::Enter, Modifiers::NONE);

    let path = fixture.state().design.shapes.last().expect("a pen path").clone();
    let nodes: Vec<(f32, f32)> = fixture.state().design.geometry(&path).lines().flat_map(|(pts, _)| pts.clone()).collect();
    assert!(nodes.len() >= 2, "two anchors: {nodes:?}");
    assert!((nodes[0].1 - nodes[1].1).abs() < 1e-3, "the segment is horizontal: {nodes:?}");
}

#[test]
fn equal_spacing_snaps_a_third_shape_into_the_row_with_gap_ticks() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    let mover = {
        let app = fixture.state_mut();
        app.design.shapes.clear();
        let a = app.design.add_shape(ShapeKind::Square);
        let b = app.design.add_shape(ShapeKind::Square);
        let c = app.design.add_shape(ShapeKind::Square);
        let set = |s: &mut crate::design::Shape, x: f32| (s.x_in, s.y_in, s.width_in, s.height_in) = (x, 4.0, 2.0, 2.0);
        set(app.design.shape_mut(a).unwrap(), 1.0);
        set(app.design.shape_mut(b).unwrap(), 9.0);
        set(app.design.shape_mut(c).unwrap(), 4.5);
        app.design.set_snapping(true);
        // Only equal spacing may explain the snap, not the grid.
        app.design.snap.grid = false;
        app.selection = vec![c];
        c
    };
    fixture.run();
    // C sits between A (right edge 3) and B (left edge 9); centred at x = 5 the gaps are equal at 2 in.
    // Dragged to x = 4.95 the gaps are 1.95 and 2.05, so spacing pulls it the last 0.05 in.
    // Grab off C's centre so the drag moves it, not its pivot crosshair (spec 52).
    let centre = at((5.0, 5.0));
    let to = at((5.45, 5.0));
    let path: Vec<Pos2> = (0..=5).map(|i| centre + (to - centre) * (i as f32 / 5.0)).collect();
    move_to(&mut fixture, path[0]);
    press_at(&mut fixture, path[0], true);
    for &p in &path[1..] {
        move_to(&mut fixture, p);
    }
    assert_snapshot(&mut fixture, "47-snap-spacing");
    press_at(&mut fixture, *path.last().unwrap(), false);
    fixture.run();
    let c = fixture.state().design.shapes.iter().find(|s| s.id == mover).unwrap();
    assert!((c.x_in - 5.0).abs() < 1e-3, "C snaps to the equal 2 in gap at x = 5, at {}", c.x_in);
}

#[test]
fn the_snapping_popover_lists_the_classes_and_the_angle_step() {
    let mut fixture = snapshot_harness(SIZE);
    let _ = two_squares_apart(&mut fixture);
    fixture.state_mut().design.set_snapping(true);
    fixture.run();
    fixture.get_by_label("Snapping settings").click();
    fixture.run();
    assert!(fixture.query_by_label("Nodes and midpoints").is_some(), "the popover lists the node switch");
    assert_snapshot(&mut fixture, "47-snap-panel");
}

#[test]
fn turning_off_guides_in_the_popover_stops_a_drag_snapping_to_a_guide() {
    use crate::edit::{Edit, Subject, run};
    use crate::guides::{Axis, GuideEdit};
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().design.set_snapping(true);
    {
        let app = fixture.state_mut();
        run(&mut app.doc.design, &mut app.doc.selection, &Edit::Guide(GuideEdit::Add { axis: Axis::Horizontal, at_in: 3.06 }), Subject::Selection).unwrap();
    }
    fixture.run();

    fixture.get_by_label("Snapping settings").click();
    fixture.run();
    fixture.get_by_label("Guides").click();
    fixture.run();
    assert!(!fixture.state().design.snap.guides, "the Guides switch is off");
    key(&mut fixture, Key::Escape, Modifiers::NONE);

    // Drag the first square's top towards the guide at 3.06; with Guides off nothing pulls it there.
    let sq = fixture.state().design.shapes[0].clone();
    fixture.state_mut().design.shape_mut(sq.id).unwrap().x_in = 0.5;
    fixture.state_mut().design.snap.grid = false;
    fixture.run();
    let centre = (sq.x_in.min(0.5) + sq.width_in / 2.0, sq.y_in + sq.height_in / 2.0);
    let grab = at(centre);
    let to = at((centre.0, 3.07 + sq.height_in / 2.0));
    let path: Vec<Pos2> = (0..=6).map(|i| grab + (to - grab) * (i as f32 / 6.0)).collect();
    move_to(&mut fixture, path[0]);
    press_at(&mut fixture, path[0], true);
    for &p in &path[1..] {
        move_to(&mut fixture, p);
    }
    press_at(&mut fixture, *path.last().unwrap(), false);
    fixture.run();
    let top = fixture.state().design.shapes[0].y_in;
    assert!((top - 3.06).abs() > 0.01, "the top did not snap to the guide, at {top}");
}
