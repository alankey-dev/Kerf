//! Spec 10: the text toolbar, Format Text, wrapped-text reflow and toolbar overflow, driven through the app.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::test_harness::{REFERENCE_CSS, assert_snapshot, click_at, drag_along, snapshot_harness};
use super::{Kerf, canvas};
use crate::design::{Shape, ShapeKind};

/// Places a text with the Text tool at `at`, snapping off, so its top-left sits under the pointer.
fn place_text(harness: &mut Harness<'_, Kerf>, at: egui::Pos2) -> Shape {
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(harness, at);
    harness.state().design.shapes.last().unwrap().clone()
}

fn text_shape(harness: &Harness<'_, Kerf>) -> Shape {
    harness.state().design.shapes.iter().find(|s| matches!(s.kind, ShapeKind::Text { .. })).unwrap().clone()
}

fn type_into(harness: &mut Harness<'_, Kerf>, label: &str, value: &str) {
    harness.get_by_label(label).focus();
    harness.run();
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_label(label).type_text(value);
    harness.run();
}

fn commit_into(harness: &mut Harness<'_, Kerf>, label: &str, value: &str) {
    type_into(harness, label, value);
    harness.press_key(egui::Key::Enter);
    harness.run();
}

#[test]
fn text_toolbar_sets_size_in_points_and_style() {
    let mut harness = snapshot_harness(REFERENCE_CSS);
    place_text(&mut harness, egui::pos2(700.0, 400.0));
    assert!(harness.query_by_label("Text content").is_some(), "placing a text opens its editor");
    commit_into(&mut harness, "Font size", "36");
    let ShapeKind::Text { size_in, .. } = text_shape(&harness).kind else { unreachable!() };
    assert!((size_in - 0.5).abs() < 1e-5, "36 pt is half an inch: {size_in}");

    let before = text_shape(&harness).contours();
    harness.get_by_label("Font style").click();
    harness.run();
    harness.get_by_label("Bold").click();
    harness.run();
    let after = text_shape(&harness);
    assert!(matches!(after.kind, ShapeKind::Text { style: crate::text::FontStyle::Bold, .. }));
    assert_ne!(after.contours(), before, "bold outlines differ");

    harness.get_by_label("Font").click();
    harness.run();
    assert!(harness.query_by_label("Space Grotesk").is_some(), "the font menu lists each font");
    assert_snapshot(&mut harness, "10-text-toolbar");
}

#[test]
fn letter_space_from_format_widens_the_box() {
    let mut harness = snapshot_harness(REFERENCE_CSS);
    let placed = place_text(&mut harness, egui::pos2(700.0, 400.0));
    harness.get_by_label("Format").click();
    harness.run();
    commit_into(&mut harness, "Letter Space", "200");
    let widened = text_shape(&harness);
    assert!(widened.width_in > placed.width_in + 0.2, "box widened: {} -> {}", placed.width_in, widened.width_in);
    assert!(harness.query_by_label("Wrap in box").is_some(), "Format stays open after an edit");
}

#[test]
fn wrapped_text_reflows_when_its_side_handle_moves() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let probe = egui::pos2(500.0, 400.0);
    place_text(&mut harness, probe);
    type_into(&mut harness, "Text content", "Hello there");
    commit_into(&mut harness, "Font size", "36");
    harness.get_by_label("Format").click();
    harness.run();
    harness.get_by_label("Wrap in box").click();
    harness.run();
    harness.get_by_label("Format").click();
    harness.run();
    let before = text_shape(&harness);
    assert!(matches!(before.kind, ShapeKind::Text { wrap_in: Some(_), .. }), "wrapping is on");

    let px = 72.0 * harness.state().view.zoom;
    let right = probe + egui::vec2(before.width_in, before.height_in / 2.0) * px;
    let target = right - egui::vec2(before.width_in * 0.4 * px, 0.0);
    // egui starts a drag a few points after the press, and the handle must still be under the pointer then.
    let path: Vec<egui::Pos2> = [0.0, 2.0, 4.0, 6.0, 8.0].iter().map(|&dx| right - egui::vec2(dx, 0.0)).chain([target]).collect();
    drag_along(&mut harness, &path);
    let after = text_shape(&harness);
    assert!(after.width_in < before.width_in * 0.7, "the box narrowed: {} -> {}", before.width_in, after.width_in);
    assert!(after.height_in > before.height_in * 1.8, "a second line: {} -> {}", before.height_in, after.height_in);
}

#[test]
fn a_narrow_window_moves_tools_into_more() {
    let mut harness = snapshot_harness(egui::vec2(900.0, 700.0));
    place_text(&mut harness, egui::pos2(300.0, 400.0));
    assert!(harness.query_by_label("Contour").is_none(), "Contour is hidden");
    harness.get_by_label_contains(" More").click();
    harness.run();
    harness.get_by_label("Contour").click();
    harness.run();
    assert!(harness.state().contour_open, "the More row opens the Contour panel");

    harness.get_by_label_contains(" More").click();
    harness.run();
    harness.get_by_label("Flip").click();
    harness.run();
    harness.get_by_label("Flip Horizontal").click();
    harness.run();
    assert!(text_shape(&harness).flip_x, "a hidden tool's own popover opens from More");
}

#[test]
fn the_floating_text_card_is_gone() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    place_text(&mut harness, egui::pos2(500.0, 400.0));
    assert!(harness.query_by_label("Type here").is_none());
    assert!(harness.query_by_label("Text size").is_none());
}
