//! Spec 52, the deferred half: the pivot crosshair drag, the parallelogram frame's Cmd shear, Transform
//! Again after a canvas rotate, and the Step and repeat and Rotate copies popovers with their live
//! preview. The pure maths lives in `repeat`, `transform` and `pivot`; these drive it through the app.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2, Vec2, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::design::{Design, ShapeKind};
use crate::transform::{Frame, Handle};
use crate::ui::canvas::Tool;
use crate::ui::test_harness::{Fixture, assert_snapshot, click_at, double_click_at, right_click_at, snapshot_harness};

const SIZE: Vec2 = vec2(1400.0, 900.0);

/// A map from world inches to screen points, found by dropping a probe at a known point.
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

/// Puts one `size` in square at `(x, y)` on the canvas and selects it.
fn one_square(fixture: &mut Fixture, (x, y): (f32, f32), size: f32) -> u64 {
    let mut design = Design::default();
    design.set_snapping(false);
    let id = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, size, size);
    fixture.state_mut().design = design;
    fixture.state_mut().selection = vec![id];
    fixture.run();
    id
}

fn shape(fixture: &Fixture, id: u64) -> crate::design::Shape {
    fixture.state().design.shapes.iter().find(|s| s.id == id).unwrap().clone()
}

fn frame_of(fixture: &Fixture, id: u64) -> Frame {
    Frame::of(&fixture.state().design, [&shape(fixture, id)]).unwrap()
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

/// Where the rotate handle sits on screen, so a test can grab it.
fn rotate_handle(fixture: &Fixture, map: &impl Fn((f32, f32)) -> Pos2, id: u64) -> Pos2 {
    let arm = super::selection::ROTATE_ARM_PX / (72.0 * fixture.state().view.zoom);
    map(frame_of(fixture, id).rotate_handle_pos(arm))
}

#[test]
fn dragging_the_pivot_to_a_corner_turns_the_shape_about_it() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (3.0, 3.0), 2.0);
    let corner_before = frame_of(&fixture, id).handle_pos(Handle::TopLeft);
    // Drag the centre crosshair out to the top-left corner; it snaps there.
    drag_with(&mut fixture, Modifiers::NONE, &[map((4.0, 4.0)), map((3.2, 3.2)), map((3.05, 3.05))]);
    let pivot = shape(&fixture, id).pivot.expect("the drag set a pivot");
    assert!(pivot.0.abs() < 0.1 && pivot.1.abs() < 0.1, "the pivot snapped to the top-left corner: {pivot:?}");
    // Now turn the shape by its rotate handle; the pivoted corner stays put.
    let handle = rotate_handle(&fixture, &map, id);
    drag_with(&mut fixture, Modifiers::NONE, &[handle, map((5.5, 4.0))]);
    assert!(shape(&fixture, id).rotation_deg > 20.0, "the shape turned");
    let corner_after = frame_of(&fixture, id).handle_pos(Handle::TopLeft);
    let moved = (corner_after.0 - corner_before.0).hypot(corner_after.1 - corner_before.1);
    assert!(moved < 0.15, "the pivoted corner held still while the rest swung: moved {moved} inches");
}

#[test]
fn double_clicking_a_set_pivot_resets_it_to_the_centre() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (3.0, 3.0), 2.0);
    fixture.state_mut().design.shape_mut(id).unwrap().pivot = Some((0.0, 0.0));
    fixture.run();
    // The crosshair now sits at the top-left corner; a double-click there clears the pivot.
    double_click_at(&mut fixture, map((3.0, 3.0)));
    assert_eq!(shape(&fixture, id).pivot, None, "the pivot is back at the centre");
}

#[test]
fn cmd_dragging_a_side_handle_shears_the_shape() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (3.0, 3.0), 2.0);
    let width = shape(&fixture, id).width_in;
    let top = map(frame_of(&fixture, id).handle_pos(Handle::Top));
    drag_with(&mut fixture, Modifiers::COMMAND, &[top, pos2(top.x + 60.0, top.y)]);
    assert!(shape(&fixture, id).skew_deg.0.abs() > 5.0, "the top handle sheared X: {:?}", shape(&fixture, id).skew_deg);
    assert_eq!(shape(&fixture, id).width_in, width, "shearing leaves the width");
}

#[test]
fn transform_again_repeats_a_canvas_rotation() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (3.0, 3.0), 2.0);
    // Rotate the square by its handle, releasing to end the gesture.
    let handle = rotate_handle(&fixture, &map, id);
    drag_with(&mut fixture, Modifiers::NONE, &[handle, map((5.0, 2.0))]);
    let once = shape(&fixture, id).rotation_deg;
    assert!(once > 10.0, "the gesture turned the square: {once}");
    // Cmd+Shift+D turns it again by the same step.
    fixture.press_key_modifiers(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::D);
    fixture.run();
    let twice = shape(&fixture, id).rotation_deg;
    assert!((twice - 2.0 * once).abs() < 1.0, "Transform Again repeated the rotation: {once} then {twice}");
    // And Cmd+D stays a plain Duplicate, not a power duplicate.
    let before = fixture.state().design.shapes.len();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::D);
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), before + 1, "a plain Duplicate added one shape");
}

fn set_field(fixture: &mut Fixture, label: &str, text: &str) {
    fixture.get_by_role_and_label(Role::TextInput, label).focus();
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::A);
    fixture.run();
    fixture.get_by_role_and_label(Role::TextInput, label).type_text(text);
    fixture.run();
    fixture.press_key(Key::Enter);
    fixture.run();
}

/// Right-clicks the selection and opens one of the Repeat popovers by its menu label.
fn open_popover(fixture: &mut Fixture, map: &impl Fn((f32, f32)) -> Pos2, centre: (f32, f32), label: &str) {
    right_click_at(fixture, map(centre));
    fixture.get_all_by_label(label).last().unwrap().click();
    fixture.run();
}

#[test]
fn step_and_repeat_previews_then_applies_three_copies_at_one_inch_pitch() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (2.0, 3.0), 1.0);
    open_popover(&mut fixture, &map, (2.5, 3.5), "Step and repeat...");
    // The defaults are three copies an inch to the right; the faint preview shows them before Apply.
    assert_snapshot(&mut fixture, "52-step-repeat");
    fixture.get_by_label("Apply").click();
    fixture.run();
    let xs: Vec<f32> = {
        let mut v: Vec<f32> = fixture.state().design.shapes.iter().map(|s| s.x_in).collect();
        v.sort_by(f32::total_cmp);
        v
    };
    assert_eq!(xs.len(), 4, "the source and three copies");
    for (i, x) in xs.iter().enumerate() {
        assert!((x - (2.0 + i as f32)).abs() < 1e-3, "copy {i} at x = {x}");
    }
    let link = shape(&fixture, id).link;
    assert!(link.is_some() && fixture.state().design.shapes.iter().all(|s| s.link == link), "grouped with the source");
}

#[test]
fn rotate_copies_previews_the_ring() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    one_square(&mut fixture, (5.0, 5.0), 1.0);
    open_popover(&mut fixture, &map, (5.5, 5.5), "Rotate copies...");
    assert_snapshot(&mut fixture, "52-rotate-copies");
    fixture.get_by_label("Apply").click();
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 8, "eight in the ring");
}

#[test]
fn editing_offset_x_reshapes_the_step_preview() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (2.0, 3.0), 1.0);
    open_popover(&mut fixture, &map, (2.5, 3.5), "Step and repeat...");
    set_field(&mut fixture, "Offset X", "2in");
    fixture.get_by_label("Apply").click();
    fixture.run();
    let mut xs: Vec<f32> = fixture.state().design.shapes.iter().map(|s| s.x_in).collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs, vec![2.0, 4.0, 6.0, 8.0], "the copies step by the typed two inches");
    let _ = id;
}

#[test]
fn escape_closes_the_popover_and_makes_nothing() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    one_square(&mut fixture, (2.0, 3.0), 1.0);
    open_popover(&mut fixture, &map, (2.5, 3.5), "Step and repeat...");
    fixture.press_key(Key::Escape);
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 1, "Escape leaves the design untouched");
}

#[test]
fn the_set_pivot_crosshair_shows_at_the_corner() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (3.0, 3.0), 2.0);
    drag_with(&mut fixture, Modifiers::NONE, &[map((4.0, 4.0)), map((3.2, 3.2)), map((3.05, 3.05))]);
    assert!(shape(&fixture, id).pivot.is_some(), "the drag set a pivot");
    assert_snapshot(&mut fixture, "52-pivot");
}
