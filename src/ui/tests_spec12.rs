//! Spec 12: editing text in place, Select Ungrouping and Curve, driven through the app.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::test_harness::{assert_snapshot, click_at, double_click_at, snapshot_harness};
use super::{Kerf, canvas};
use crate::design::ShapeKind;

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);
const PLACE: egui::Pos2 = egui::pos2(600.0, 400.0);
/// Inside the placed text's box, clear of its corner handle.
const INSIDE: egui::Pos2 = egui::pos2(PLACE.x + 40.0, PLACE.y + 40.0);

/// Places "Text" with the Text tool at `PLACE`, then leaves the editor that placing opens.
fn place_text(harness: &mut Harness<'_, Kerf>) -> u64 {
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(harness, PLACE);
    assert!(harness.state().view.text_edit.is_some(), "placing a text opens its editor");
    escape(harness);
    harness.state().selection[0]
}

fn escape(harness: &mut Harness<'_, Kerf>) {
    harness.press_key(egui::Key::Escape);
    harness.run();
}

fn content(harness: &Harness<'_, Kerf>, id: u64) -> String {
    match &harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().kind {
        ShapeKind::Text { content, .. } => content.clone(),
        _ => unreachable!("a text shape"),
    }
}

fn type_text(harness: &mut Harness<'_, Kerf>, text: &str) {
    harness.input_mut().events.push(egui::Event::Text(text.into()));
    harness.run();
}

#[test]
fn double_click_edits_text_in_place_as_one_undo_step() {
    let mut harness = snapshot_harness(SIZE);
    let id = place_text(&mut harness);
    assert!(harness.state().view.text_edit.is_none(), "Escape leaves the editor");
    double_click_at(&mut harness, INSIDE);
    assert!(harness.state().view.text_edit.is_some(), "double-click opens the editor");
    let before = harness.state().design.shapes[0].width_in;
    type_text(&mut harness, "!");
    assert_eq!(content(&harness, id), "Text!");
    assert!(harness.state().design.shapes[0].width_in > before, "the box follows the typing");
    type_text(&mut harness, "!");
    escape(&mut harness);
    assert!(harness.state().view.text_edit.is_none());
    assert_eq!(content(&harness, id), "Text!!");

    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(content(&harness, id), "Text", "one undo restores the words");
}

#[test]
fn the_text_tool_edits_a_text_it_clicks_and_a_click_outside_commits() {
    let mut harness = snapshot_harness(SIZE);
    let id = place_text(&mut harness);
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(&mut harness, INSIDE);
    assert_eq!(harness.state().design.shapes.len(), 1, "no second text");
    assert!(harness.state().view.text_edit.as_ref().is_some_and(|s| s.id == id));
    type_text(&mut harness, "s");
    click_at(&mut harness, egui::pos2(300.0, 700.0));
    assert!(harness.state().view.text_edit.is_none(), "a click outside leaves the editor");
    assert_eq!(content(&harness, id), "Texts");
    assert!(harness.query_by_label("Text").is_none_or(|n| n.role() != Role::MultilineTextInput), "the inspector has no Text field");
}

/// The artboard as rendered with nothing selected, so no selection chrome differs. The panels are
/// left out, as the Layers panel lists the new Group.
fn bare_canvas(harness: &mut Harness<'_, Kerf>) -> image::RgbaImage {
    let selection = std::mem::take(&mut harness.state_mut().selection);
    harness.run();
    let image = image::imageops::crop_imm(&harness.render().expect("wgpu render"), 190, 120, 850, 700).to_image();
    harness.state_mut().selection = selection;
    harness.run();
    image
}

#[test]
fn ungrouping_letters_looks_like_the_source() {
    let mut harness = snapshot_harness(SIZE);
    place_text(&mut harness);
    let before = bare_canvas(&mut harness);
    harness.get_by_label("Format").click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Letters").click();
    harness.run();
    let shapes = &harness.state().design.shapes;
    assert_eq!(shapes.len(), 4, "T, e, x, t");
    assert!(shapes.iter().all(|s| s.link.is_some() && s.link == shapes[0].link), "one Group");
    assert_eq!(harness.state().selection.len(), 4, "the letters are selected");
    harness.get_by_label("Format").click();
    harness.run();

    let after = bare_canvas(&mut harness);
    let differing = before.pixels().zip(after.pixels()).filter(|(a, b)| a.0.iter().zip(b.0).any(|(x, y)| x.abs_diff(y) > 48)).count();
    assert!(differing < 40, "{differing} pixels changed");
}

#[test]
fn curve_from_effects_bends_the_text_and_straighten_restores_it() {
    let mut harness = snapshot_harness(SIZE);
    let id = place_text(&mut harness);
    let straight = harness.state().design.shapes[0].clone();
    harness.get_by_label("Effects").click();
    harness.run();
    harness.get_by_label("Curve").click();
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "Diameter").focus();
    harness.run();
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "Diameter").type_text("3");
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    let curved = harness.state().design.shapes[0].clone();
    assert!(matches!(curved.kind, ShapeKind::Text { curve: Some(c), .. } if (c.diameter_in - 3.0).abs() < 1e-3), "{:?}", curved.kind);
    assert!(curved.height_in > straight.height_in, "the arc is taller than the line");
    assert_snapshot(&mut harness, "12-curve");

    harness.get_by_label("Straighten").click();
    harness.run();
    let ShapeKind::Text { curve, .. } = harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().kind else { unreachable!() };
    assert_eq!(curve, None);
}

#[test]
fn curve_is_offered_only_for_text() {
    let mut harness = snapshot_harness(SIZE);
    let id = harness.state_mut().design.add_shape(ShapeKind::Square);
    harness.state_mut().selection = vec![id];
    harness.run();
    harness.get_by_label("Effects").click();
    harness.run();
    assert!(harness.query_by_label("Warp").is_some());
    assert!(harness.query_by_label("Curve").is_none(), "a square cannot curve");
}
