//! Spec 34: lock aspect, lock position and lock layer, driven through the inspector, Layers and the canvas.

use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::edit::{Edit, Subject, run};
use crate::ui::test_harness::{Fixture, assert_snapshot, click_at, drag_along, key, move_to, snapshot_harness, two_squares_apart};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

fn select(fixture: &mut Fixture, ids: &[u64]) {
    fixture.state_mut().selection = ids.to_vec();
    fixture.run();
}

fn type_width(fixture: &mut Fixture, text: &str) {
    fixture.get_by_role_and_label(Role::TextInput, "W").focus();
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::A);
    fixture.run();
    fixture.get_by_role_and_label(Role::TextInput, "W").type_text(text);
    fixture.run();
    fixture.press_key(Key::Enter);
    fixture.run();
}

fn size(fixture: &Fixture, i: usize) -> (f32, f32) {
    let s = &fixture.state().design.shapes[i];
    (s.width_in, s.height_in)
}

#[test]
fn lock_aspect_makes_a_typed_width_scale_the_height_and_each_step_undoes_once() {
    let mut fixture = snapshot_harness(SIZE);
    _ = two_squares_apart(&mut fixture);
    fixture.state_mut().design.shapes[0].height_in = fixture.state().design.shapes[0].width_in / 2.0;
    select(&mut fixture, &[1]);
    let before = size(&fixture, 0);
    let depth = fixture.state().history.depth();

    fixture.get_by_label("Lock aspect ratio").click();
    fixture.run();
    assert!(fixture.state().design.shapes[0].lock_aspect);
    assert_eq!(fixture.state().history.depth(), depth + 1, "the toggle is one undo step");

    type_width(&mut fixture, "*3");
    let (w, h) = size(&fixture, 0);
    assert!((w - before.0 * 3.0).abs() < 1e-4 && (h - before.1 * 3.0).abs() < 1e-4, "{before:?} became {:?}", (w, h));

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert_eq!(size(&fixture, 0), before, "one Undo restores both");
    assert!(fixture.state().design.shapes[0].lock_aspect, "and leaves the lock on");
    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(!fixture.state().design.shapes[0].lock_aspect, "the next Undo turns it off");
}

#[test]
fn a_side_handle_scales_both_while_locked_and_shift_frees_it() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().design.shapes[0].lock_aspect = true;
    select(&mut fixture, &[1]);
    let s = fixture.state().design.shapes[0].clone();
    let right = (s.x_in + s.width_in, s.y_in + s.height_in / 2.0);

    drag_along(&mut fixture, &[at(right), at((right.0 + 0.25, right.1)), at((right.0 + 0.5, right.1))]);
    let (w, h) = size(&fixture, 0);
    assert!(w > s.width_in && (w / h - s.width_in / s.height_in).abs() < 1e-3, "{w} x {h} keeps the proportions");

    let s = fixture.state().design.shapes[0].clone();
    let right = (s.x_in + s.width_in, s.y_in + s.height_in / 2.0);
    fixture.input_mut().modifiers = Modifiers::SHIFT;
    drag_along(&mut fixture, &[at(right), at((right.0 + 0.25, right.1)), at((right.0 + 0.5, right.1))]);
    fixture.input_mut().modifiers = Modifiers::NONE;
    let (w, h) = size(&fixture, 0);
    assert!(w > s.width_in && (h - s.height_in).abs() < 1e-4, "Shift stretches only the width: {w} x {h}");
}

#[test]
fn a_locked_layer_is_skipped_by_canvas_clicks_marquee_and_select_all_but_not_by_layers() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    let right = fixture.state().design.shapes[1].clone();
    let centre = right.centre_in();

    let row = fixture.get_all_by_label("Layer Square").next().and_then(|n| n.bounding_box()).expect("the top row");
    move_to(&mut fixture, egui::pos2(((row.x0 + row.x1) / 2.0) as f32, ((row.y0 + row.y1) / 2.0) as f32));
    fixture.get_by_label("Lock Square").click();
    fixture.run();
    assert!(fixture.state().design.shapes[1].locked, "the top row is the right square");
    assert!(!fixture.state().design.shapes[0].locked);

    click_at(&mut fixture, at(centre));
    assert!(fixture.state().selection.is_empty(), "a click on it selects nothing");
    fixture.query_by_label("Unlock Square").expect("the lock shows while the pointer is away from the row");

    drag_along(&mut fixture, &[at((0.5, 0.5)), at((3.0, 2.0)), at((right.x_in + right.width_in + 0.5, 3.0))]);
    assert_eq!(fixture.state().selection, [1], "the marquee skips it");

    key(&mut fixture, Key::A, Modifiers::COMMAND);
    assert_eq!(fixture.state().selection, [1], "Select All skips it");

    fixture.get_all_by_label("Layer Square").next().expect("the top row").click();
    fixture.run();
    assert_eq!(fixture.state().selection, [2], "its Layers row still selects it");
}

#[test]
fn a_position_locked_shape_shows_no_handles_and_ignores_nudges_and_drags_but_takes_a_colour() {
    let mut fixture = snapshot_harness(SIZE);
    let at = two_squares_apart(&mut fixture);
    select(&mut fixture, &[1]);
    let depth = fixture.state().history.depth();
    fixture.get_by_label("Lock position").click();
    fixture.run();
    assert!(fixture.state().design.shapes[0].lock_position);
    assert_eq!(fixture.state().history.depth(), depth + 1, "the toggle is one undo step");
    let locked = fixture.state().design.shapes[0].clone();
    let inside = locked.centre_in();
    let corner = (locked.x_in + locked.width_in - 0.02, locked.y_in + locked.height_in - 0.02);

    move_to(&mut fixture, egui::pos2(700.0, 700.0));
    assert_snapshot(&mut fixture, "34-locks");
    assert!(fixture.get_by_role_and_label(Role::TextInput, "X").is_disabled(), "its position fields are disabled");
    assert!(!fixture.get_by_label("Lock position").is_disabled(), "but the lock is not");

    key(&mut fixture, Key::ArrowRight, Modifiers::NONE);
    drag_along(&mut fixture, &[at(inside), at((inside.0 + 0.5, inside.1)), at((inside.0 + 1.0, inside.1 + 0.5))]);
    drag_along(&mut fixture, &[at(corner), at((corner.0 + 0.5, corner.1)), at((corner.0 + 1.0, corner.1 + 1.0))]);
    let after = &fixture.state().design.shapes[0];
    assert_eq!((after.x_in, after.y_in, after.width_in, after.height_in), (locked.x_in, locked.y_in, locked.width_in, locked.height_in));
    assert_eq!(fixture.state().selection, [1], "it is still selected");

    let app = &mut fixture.state_mut().doc;
    run(&mut app.design, &mut app.selection, &Edit::Colour(Color32::RED), Subject::Selection).unwrap();
    fixture.run();
    assert_eq!(fixture.state().design.shapes[0].fill, Color32::RED, "it can still be recoloured");
}
