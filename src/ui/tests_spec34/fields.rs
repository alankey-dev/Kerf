//! Spec 34: typed expressions in length fields commit once on Enter and refuse what does not parse.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::design::ShapeKind;
use crate::ui::test_harness::{Fixture, assert_snapshot, key, snapshot_harness};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

fn one_square(fixture: &mut Fixture) {
    let id = fixture.state_mut().design.add_shape(ShapeKind::Square);
    let s = fixture.state_mut().design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (2.0, 2.0, 2.0, 2.0);
    fixture.state_mut().selection = vec![id];
    fixture.run();
}

fn type_into_width(fixture: &mut Fixture, text: &str) {
    fixture.get_by_role_and_label(Role::TextInput, "W").focus();
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::A);
    fixture.run();
    fixture.get_by_role_and_label(Role::TextInput, "W").type_text(text);
    fixture.run();
    fixture.press_key(Key::Enter);
    fixture.run();
}

fn width(fixture: &Fixture) -> f32 {
    fixture.state().design.shapes[0].width_in
}

fn shown_width(fixture: &Fixture) -> String {
    fixture.get_by_role_and_label(Role::TextInput, "W").value().unwrap_or_default()
}

#[test]
fn a_typed_sum_of_units_sets_the_width_in_one_undo_step() {
    let mut fixture = snapshot_harness(SIZE);
    one_square(&mut fixture);
    let depth = fixture.state().history.depth();
    type_into_width(&mut fixture, "2in + 5mm");
    assert!((width(&fixture) - 2.196_85).abs() < 1e-5, "{}", width(&fixture));
    assert_eq!(shown_width(&fixture), "2.197");
    assert_eq!(fixture.state().history.depth(), depth + 1);

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert_eq!(width(&fixture), 2.0);
    assert_eq!(shown_width(&fixture), "2");
}

#[test]
fn a_leading_operator_scales_the_current_width() {
    let mut fixture = snapshot_harness(SIZE);
    one_square(&mut fixture);
    type_into_width(&mut fixture, "*1.5");
    assert_eq!(width(&fixture), 3.0);
}

#[test]
fn invalid_input_keeps_the_old_value_and_says_why() {
    let mut fixture = snapshot_harness(SIZE);
    one_square(&mut fixture);
    let depth = fixture.state().history.depth();
    type_into_width(&mut fixture, "2in +");
    assert_eq!(width(&fixture), 2.0);
    assert_eq!(fixture.state().history.depth(), depth);
    assert_eq!(shown_width(&fixture), "2in +", "the entry stays for the user to fix");
    assert!(fixture.query_by_label("Expected a number at character 6").is_some(), "the reason shows beside the field");
    assert_snapshot(&mut fixture, "34-expression");

    fixture.press_key(Key::Escape);
    fixture.run();
    assert_eq!(width(&fixture), 2.0);
    assert_eq!(shown_width(&fixture), "2");
    assert!(fixture.query_by_label("Expected a number at character 6").is_none(), "Escape clears the error");
}
