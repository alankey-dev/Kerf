//! Spec 13: right-click context menus and selecting from the Layers panel, driven through the app.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, click_at, pick, right_click_at, save_snapshot, snapshot_harness, two_squares_apart};
use crate::design::ShapeKind;

/// A point on the `nth` row labelled `label`, left of its buttons.
fn row_at(harness: &Harness<'_, Kerf>, label: &str, nth: usize) -> egui::Pos2 {
    let r = harness.get_all_by_label(label).nth(nth).unwrap().bounding_box().unwrap();
    egui::pos2(r.x0 as f32 + 60.0, ((r.y0 + r.y1) / 2.0) as f32)
}

fn click_holding(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2, modifiers: egui::Modifiers) {
    harness.input_mut().modifiers = modifiers;
    click_at(harness, pos);
    harness.input_mut().modifiers = egui::Modifiers::NONE;
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort();
    ids
}

/// Three lone shapes and a Group of two, listed top first: Group (Heart, Star), Triangle, Circle, Square.
fn five_shapes(harness: &mut Harness<'_, Kerf>) {
    for k in [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Triangle, ShapeKind::Star, ShapeKind::Heart] {
        harness.state_mut().design.add_shape(k);
    }
    harness.state_mut().design.group(&[4, 5]);
    harness.state_mut().selection.clear();
    harness.run();
}

#[test]
fn layers_rows_select_like_the_canvas() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    five_shapes(&mut harness);

    let triangle = row_at(&harness, "Layer Triangle", 0);
    click_at(&mut harness, triangle);
    assert_eq!(harness.state().selection, vec![3], "a row selects its shape");

    let square = row_at(&harness, "Layer Square", 0);
    click_holding(&mut harness, square, egui::Modifiers::SHIFT);
    assert_eq!(sorted(harness.state().selection.clone()), vec![1, 2, 3], "Shift selects the range from the anchor");

    let circle = row_at(&harness, "Layer Circle", 0);
    click_holding(&mut harness, circle, egui::Modifiers::COMMAND);
    assert_eq!(sorted(harness.state().selection.clone()), vec![1, 3], "Cmd removes one row");
    click_holding(&mut harness, circle, egui::Modifiers::COMMAND);
    assert_snapshot(&mut harness, "13-layers-multi");

    let group = row_at(&harness, "Layer Group", 0);
    click_at(&mut harness, group);
    assert_eq!(sorted(harness.state().selection.clone()), vec![4, 5], "a parent row selects the whole Group");
    let star = row_at(&harness, "Layer Star", 0);
    click_at(&mut harness, star);
    assert_eq!(harness.state().selection, vec![4], "a member row selects that member");
}

#[test]
fn a_rows_eye_hides_without_changing_the_selection() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    five_shapes(&mut harness);
    harness.state_mut().selection = vec![3];
    harness.run();
    harness.get_by_label("Hide Circle").click();
    harness.run();
    assert!(harness.state().design.shapes.iter().find(|s| s.id == 2).unwrap().hidden, "the eye hides the circle");
    assert_eq!(harness.state().selection, vec![3], "the selection is untouched");
}

#[test]
fn right_click_selects_a_shape_and_duplicate_undoes_in_one_step() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    right_click_at(&mut harness, at((2.0, 2.0)));
    assert_eq!(harness.state().selection, vec![1], "right-click selects the shape under it");
    assert!(harness.query_by_label("Move Forward").is_none(), "submenus start closed");
    pick(&mut harness, "Arrange");
    assert!(harness.query_by_label("Move Forward").is_some(), "Arrange opens its submenu");
    save_snapshot(&mut harness, "13-context-shape.png");

    pick(&mut harness, "Duplicate");
    assert_eq!(harness.state().design.shapes.len(), 3, "Duplicate adds one shape");
    assert!(harness.query_by_label("Move Forward").is_none(), "picking an item closes the menu");
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 2, "one Undo removes it");
}

#[test]
fn right_click_keeps_a_multiple_selection_and_groups_it() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    right_click_at(&mut harness, at((5.0, 2.0)));
    assert_eq!(sorted(harness.state().selection.clone()), vec![1, 2], "a selected shape keeps the selection");
    pick(&mut harness, "Group");
    let links: Vec<_> = harness.state().design.shapes.iter().map(|s| s.link.map(|l| (l.id, l.kind))).collect();
    assert!(links[0].is_some_and(|(_, k)| k == crate::links::LinkKind::Group) && links[0] == links[1], "both share one Group");
}

#[test]
fn right_click_on_a_layers_row_deletes_that_object() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    five_shapes(&mut harness);
    harness.state_mut().selection = vec![1];
    harness.run();
    let circle = row_at(&harness, "Layer Circle", 0);
    right_click_at(&mut harness, circle);
    assert_eq!(harness.state().selection, vec![2], "the row's object is selected first");
    pick(&mut harness, "Delete");
    assert!(harness.state().design.shapes.iter().all(|s| s.id != 2), "Delete removes the circle");
    assert_eq!(harness.state().design.shapes.len(), 4);
}

#[test]
fn right_click_on_empty_canvas_offers_select_all() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    right_click_at(&mut harness, at((8.0, 8.0)));
    assert!(harness.query_by_label("Duplicate").is_none(), "no shape items on empty canvas");
    pick(&mut harness, "Select All");
    assert_eq!(sorted(harness.state().selection.clone()), vec![1, 2], "Select All selects every shape");
}

#[test]
fn escape_and_a_click_elsewhere_close_the_menu() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    right_click_at(&mut harness, at((8.0, 8.0)));
    assert!(harness.query_by_label("Deselect").is_some());
    harness.press_key(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Deselect").is_none(), "Escape closes it");

    right_click_at(&mut harness, at((8.0, 8.0)));
    click_at(&mut harness, at((10.0, 1.0)));
    assert!(harness.query_by_label("Deselect").is_none(), "a click elsewhere closes it");
}

#[test]
fn selecting_on_the_canvas_scrolls_its_row_into_view() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 700.0));
    harness.state_mut().design.add_shape(ShapeKind::Square);
    for _ in 0..30 {
        harness.state_mut().design.add_shape(ShapeKind::Circle);
    }
    harness.state_mut().selection.clear();
    harness.run();
    let visible = |h: &Harness<'_, Kerf>| h.query_by_label("Layer Square").and_then(|n| n.bounding_box()).is_some_and(|r| r.y1 <= 700.0);
    assert!(!visible(&harness), "the bottom row starts out of view");
    harness.state_mut().selection = vec![1];
    harness.run();
    assert!(visible(&harness), "a selection made elsewhere scrolls its row into view");
}
