use egui::Key;
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_snapshot, snapshot_harness};
use crate::design::{Operation, ShapeKind};

const SIZE: egui::Vec2 = egui::vec2(1280.0, 820.0);

fn selected_heart() -> (Fixture, u64) {
    let mut fixture = snapshot_harness(SIZE);
    let id = fixture.state_mut().doc.design.add_shape(ShapeKind::Heart);
    let shape = fixture.state_mut().doc.design.shape_mut(id).unwrap();
    (shape.x_in, shape.y_in) = (7.0, 4.0);
    fixture.state_mut().doc.selection = vec![id];
    fixture.run();
    (fixture, id)
}

fn copied_text(fixture: &Fixture) -> String {
    fixture
        .output()
        .platform_output
        .commands
        .iter()
        .rev()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("clipboard text output")
}

fn paste_text(fixture: &mut Fixture, text: String) {
    fixture.input_mut().events.push(egui::Event::Paste(text));
    fixture.run();
}

#[test]
fn copy_and_paste_adds_an_offset_copy_and_undo_removes_it() {
    let (mut fixture, id) = selected_heart();
    let before = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == id).unwrap().clone();
    let depth = fixture.state().doc.history.depth();

    fixture.input_mut().events.push(egui::Event::Copy);
    fixture.run();
    let copied = copied_text(&fixture);
    assert!(copied.contains("kerf-clipboard"), "the system clipboard gets native metadata in an SVG");
    assert_eq!(fixture.state().doc.design.shapes.len(), 1, "Copy does not change the design");

    paste_text(&mut fixture, copied);
    assert_eq!(fixture.state().doc.design.shapes.len(), 2);
    let pasted_id = fixture.state().doc.selection[0];
    let pasted = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == pasted_id).unwrap();
    assert_eq!((pasted.x_in, pasted.y_in), (before.x_in + 0.25, before.y_in + 0.25));
    assert_eq!(pasted.kind, before.kind);
    assert_eq!(fixture.state().doc.history.depth(), depth + 1, "paste is one undo step");

    fixture.get_by_label("Edit").click();
    fixture.run();
    for label in ["Cut", "Copy", "Paste", "Paste in Place", "Paste Style"] {
        fixture.get_by_label(label);
    }
    assert_snapshot(&mut fixture, "44-paste");
    fixture.press_key(Key::Escape);
    fixture.run();
    fixture.press_key_modifiers(egui::Modifiers::COMMAND, Key::Z);
    fixture.run();
    assert_eq!(fixture.state().doc.design.shapes.len(), 1, "one Undo removes the pasted copy");
}

#[test]
fn cut_and_paste_in_place_restore_the_original_position_in_one_step_each() {
    let (mut fixture, id) = selected_heart();
    let before = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == id).unwrap().clone();
    let before_depth = fixture.state().doc.history.depth();

    fixture.input_mut().events.push(egui::Event::Cut);
    fixture.step();
    assert!(fixture.state().doc.design.shapes.is_empty(), "Cut removes the selected shape");
    let copied = copied_text(&fixture);
    assert_eq!(fixture.state().doc.history.depth(), before_depth + 1);

    fixture.input_mut().modifiers = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    fixture.input_mut().events.push(egui::Event::Paste(copied));
    fixture.run();
    let pasted_id = fixture.state().doc.selection[0];
    let pasted = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == pasted_id).unwrap();
    assert_eq!((pasted.x_in, pasted.y_in), (before.x_in, before.y_in));
    assert_eq!(fixture.state().doc.history.depth(), before_depth + 2);

    fixture.press_key_modifiers(egui::Modifiers::COMMAND, Key::Z);
    fixture.run();
    assert!(fixture.state().doc.design.shapes.is_empty(), "one Undo removes the restored shape");
}

#[test]
fn cut_skips_locked_layers_and_reports_why() {
    let mut fixture = snapshot_harness(SIZE);
    let locked = fixture.state_mut().doc.design.add_shape(ShapeKind::Square);
    let removable = fixture.state_mut().doc.design.add_shape(ShapeKind::Heart);
    fixture.state_mut().doc.design.shape_mut(locked).unwrap().locked = true;
    fixture.state_mut().doc.selection = vec![locked, removable];
    fixture.run();

    fixture.input_mut().events.push(egui::Event::Cut);
    fixture.step();
    assert_eq!(fixture.state().doc.design.shapes.iter().map(|shape| shape.id).collect::<Vec<_>>(), [locked]);
    assert_eq!(fixture.state().doc.view.notice.as_ref().unwrap().message(), "Locked layers stay");
    let copied = copied_text(&fixture);

    paste_text(&mut fixture, copied);
    assert_eq!(fixture.state().doc.design.shapes.len(), 2);
    assert!(fixture.state().doc.design.shapes.iter().any(|shape| matches!(shape.kind, ShapeKind::Heart)));
    assert!(fixture.state().doc.design.shapes.iter().any(|shape| shape.id == locked && shape.locked));
}

#[test]
fn paste_style_changes_operation_and_colour_without_moving_the_target() {
    let mut fixture = snapshot_harness(SIZE);
    let source = fixture.state_mut().doc.design.add_shape(ShapeKind::Square);
    let target = fixture.state_mut().doc.design.add_shape(ShapeKind::Circle);
    let source_shape = fixture.state_mut().doc.design.shape_mut(source).unwrap();
    source_shape.operation = Operation::Score;
    source_shape.fill = egui::Color32::from_rgb(160, 20, 80);
    fixture.state_mut().doc.selection = vec![source];
    fixture.run();
    let before = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == target).unwrap().clone();

    fixture.input_mut().events.push(egui::Event::Copy);
    fixture.run();
    let copied = copied_text(&fixture);
    fixture.state_mut().doc.selection = vec![target];
    fixture.run();
    fixture.input_mut().modifiers = egui::Modifiers::COMMAND.plus(egui::Modifiers::ALT);
    paste_text(&mut fixture, copied);

    let pasted = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == target).unwrap();
    assert_eq!((pasted.operation, pasted.fill), (Operation::Score, egui::Color32::from_rgb(160, 20, 80)));
    assert_eq!((pasted.x_in, pasted.y_in, pasted.width_in, pasted.height_in), (before.x_in, before.y_in, before.width_in, before.height_in));
}

#[test]
fn a_clip_pastes_in_place_into_another_tab_with_the_same_mat() {
    let (mut fixture, id) = selected_heart();
    let before = fixture.state().doc.design.shapes.iter().find(|shape| shape.id == id).unwrap().clone();
    fixture.input_mut().events.push(egui::Event::Copy);
    fixture.run();
    let copied = copied_text(&fixture);

    fixture.press_key_modifiers(egui::Modifiers::COMMAND, Key::T);
    fixture.run();
    paste_text(&mut fixture, copied);
    let pasted = fixture.state().doc.design.shapes.first().unwrap();
    assert_eq!(fixture.state().doc.design.shapes.len(), 1);
    assert_eq!((pasted.x_in, pasted.y_in), (before.x_in, before.y_in));
    assert_eq!(pasted.kind, before.kind);
}

#[test]
fn measure_label_copy_takes_precedence_over_selection_copy() {
    let (mut fixture, _) = selected_heart();
    let measurement = crate::measure::Measurement { from: (1.0, 1.0), to: (2.0, 1.0) };
    let expected = measurement.label(crate::units::Units::Imperial);
    fixture.state_mut().doc.view.measure.measurement = Some(measurement);

    fixture.input_mut().events.push(egui::Event::Copy);
    fixture.run();
    assert_eq!(copied_text(&fixture), expected);
}

#[test]
fn external_svg_pastes_as_a_path_at_true_scale() {
    let mut fixture = snapshot_harness(SIZE);
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96"><path d="M0 0 L96 0 L96 96 Z"/></svg>"#;
    paste_text(&mut fixture, svg.into());

    let [shape] = &fixture.state().doc.design.shapes[..] else { panic!("one path is pasted") };
    assert!(matches!(shape.kind, ShapeKind::Imported { .. }));
    assert_eq!(shape.operation, Operation::BasicCut);
    assert!((shape.width_in - 1.0).abs() < 0.001);
    assert!((shape.height_in - 1.0).abs() < 0.001);
}

#[test]
fn plain_text_pastes_as_text_at_the_view_centre() {
    let mut fixture = snapshot_harness(SIZE);
    let centre = fixture.state().doc.view.centre_in();
    paste_text(&mut fixture, "Cut this label".into());

    let [shape] = &fixture.state().doc.design.shapes[..] else { panic!("one text shape is pasted") };
    let ShapeKind::Text { content, .. } = &shape.kind else { panic!("clipboard text becomes a text object") };
    assert_eq!(content, "Cut this label");
    assert!((shape.x_in + shape.width_in / 2.0 - centre.0).abs() < 0.001);
    assert!((shape.y_in + shape.height_in / 2.0 - centre.1).abs() < 0.001);
}

#[test]
fn a_guide_copies_even_without_an_svg_fallback_path() {
    let mut fixture = snapshot_harness(SIZE);
    let id = fixture.state_mut().doc.design.add_shape(ShapeKind::Square);
    fixture.state_mut().doc.design.shape_mut(id).unwrap().operation = Operation::Guide;
    fixture.state_mut().doc.selection = vec![id];
    fixture.run();

    fixture.input_mut().events.push(egui::Event::Copy);
    fixture.run();
    let copied = copied_text(&fixture);
    assert!(copied.contains("kerf-clipboard"));
    paste_text(&mut fixture, copied);

    assert_eq!(fixture.state().doc.design.shapes.len(), 2);
    assert_eq!(fixture.state().doc.design.shapes[1].operation, Operation::Guide);
}
