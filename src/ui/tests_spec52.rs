//! Spec 52: skew, pivot and the repeat engine. The inspector gains Skew X and Skew Y fields, the
//! context menu's Repeat submenu makes Step and repeat and Rotate copies copies, and Cmd+D repeats the
//! last duplicate's step (Power Duplicate). The live-preview popovers and the pivot crosshair drag are
//! not yet wired; their behaviour is covered by the pure unit tests in `repeat`, `transform` and
//! `containers`.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2, Vec2, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::design::{Design, ShapeKind};
use crate::ui::canvas::Tool;
use crate::ui::test_harness::{Fixture, assert_snapshot, right_click_at, snapshot_harness};

const SIZE: Vec2 = vec2(1400.0, 900.0);

/// A map from world inches to screen points, found by dropping a probe at a known point.
fn grid_map(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> Pos2 + use<> {
    fixture.state_mut().tool = Tool::Text;
    let probe = pos2(640.0, 420.0);
    crate::ui::test_harness::click_at(fixture, probe);
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

fn shape(fixture: &Fixture, id: u64) -> &crate::design::Shape {
    fixture.state().design.shapes.iter().find(|s| s.id == id).unwrap()
}

#[test]
fn typing_skew_x_leans_the_shape_and_leaves_its_width() {
    let mut fixture = snapshot_harness(SIZE);
    let id = one_square(&mut fixture, (3.0, 3.0), 3.0);
    let width = shape(&fixture, id).width_in;
    set_field(&mut fixture, "Skew X", "20");
    assert_eq!(shape(&fixture, id).skew_deg.0, 20.0, "Skew X took the typed angle");
    assert_eq!(shape(&fixture, id).width_in, width, "W is unchanged by the skew");
    assert_snapshot(&mut fixture, "52-skew");
}

#[test]
fn the_repeat_menu_opens_the_popover_and_apply_steps_copies_grouped_with_the_source() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let id = one_square(&mut fixture, (3.0, 3.0), 1.0);
    right_click_at(&mut fixture, map((3.5, 3.5)));
    fixture.get_all_by_label("Step and repeat...").last().unwrap().click();
    fixture.run();
    // The popover opens with its preview; Apply makes the copies as plain shapes.
    fixture.get_by_label("Apply").click();
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 4, "the source and three copies");
    let link = shape(&fixture, id).link;
    assert!(link.is_some() && fixture.state().design.shapes.iter().all(|s| s.link == link), "grouped with the source");
}

#[test]
fn rotate_copies_popover_rings_the_selection_and_one_undo_removes_them() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    one_square(&mut fixture, (4.0, 4.0), 1.0);
    right_click_at(&mut fixture, map((4.5, 4.5)));
    fixture.get_all_by_label("Rotate copies...").last().unwrap().click();
    fixture.run();
    fixture.get_by_label("Apply").click();
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 8, "eight petals in the ring");
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::Z);
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 1, "one undo removes the whole ring");
}

#[test]
fn cmd_d_after_a_move_repeats_the_offset() {
    let mut fixture = snapshot_harness(SIZE);
    let id = one_square(&mut fixture, (1.0, 3.0), 1.0);
    // Duplicate, then move the copy one inch right of the source.
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::D);
    fixture.run();
    let copy = fixture.state().selection[0];
    fixture.state_mut().design.shape_mut(copy).unwrap().x_in = 2.0;
    fixture.run();
    // Two more presses lay a row at an even pitch.
    for _ in 0..2 {
        fixture.press_key_modifiers(Modifiers::COMMAND, Key::D);
        fixture.run();
    }
    let mut xs: Vec<f32> = fixture.state().design.shapes.iter().map(|s| s.x_in).collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs.len(), 4);
    for (i, x) in xs.iter().enumerate() {
        assert!((x - (1.0 + i as f32)).abs() < 1e-3, "shape {i} at x = {x}, expected {}", 1.0 + i as f32);
    }
    let _ = id;
}
