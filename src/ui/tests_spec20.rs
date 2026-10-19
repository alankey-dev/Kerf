//! Spec 20: dragging Layers rows to reorder and relink, driven through the app.

use egui::{Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, move_to, press_at, snapshot_harness};
use crate::design::ShapeKind;
use crate::links::{Link, LinkKind};

type App<'a> = Harness<'a, Kerf>;

/// A point `dy` below the top of the `nth` row labelled `label`, left of its buttons.
fn row(harness: &App<'_>, label: &str, nth: usize, dy: f32) -> Pos2 {
    let r = harness.get_all_by_label(label).nth(nth).unwrap_or_else(|| panic!("no {label} row")).bounding_box().unwrap();
    pos2(r.x0 as f32 + 60.0, r.y0 as f32 + dy)
}

fn add(harness: &mut App<'_>, kinds: impl IntoIterator<Item = ShapeKind>) {
    for k in kinds {
        harness.state_mut().design.add_shape(k);
    }
    harness.state_mut().selection.clear();
    harness.run();
}

/// Presses at `from` and moves past the drag threshold, so the drag has started.
fn grab(harness: &mut App<'_>, from: Pos2) {
    move_to(harness, from);
    press_at(harness, from, true);
    move_to(harness, from + egui::vec2(0.0, 8.0));
}

fn drag_row(harness: &mut App<'_>, from: Pos2, to: Pos2) {
    grab(harness, from);
    move_to(harness, to);
    press_at(harness, to, false);
}

fn order(harness: &App<'_>) -> Vec<u64> {
    harness.state().design.shapes.iter().map(|s| s.id).collect()
}

fn link(harness: &App<'_>, id: u64) -> Option<Link> {
    harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().link
}

#[test]
fn dragging_the_bottom_row_to_the_top_brings_it_to_the_front_in_one_undo_step() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Triangle]);
    let (square, top) = (row(&harness, "Layer Square", 0, 14.0), row(&harness, "Layer Triangle", 0, 4.0));
    drag_row(&mut harness, square, top);
    assert_eq!(order(&harness), vec![2, 3, 1], "the square draws in front");
    assert_eq!(harness.state().selection, vec![1], "dragging an unselected row selects it");
    assert!(row(&harness, "Layer Square", 0, 0.0).y < row(&harness, "Layer Triangle", 0, 0.0).y, "and lists first");
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(order(&harness), vec![1, 2, 3], "one Undo restores the z-order");
}

#[test]
fn dropping_a_loose_row_on_the_group_row_joins_the_group() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Triangle]);
    harness.state_mut().design.group(&[1, 2]);
    harness.run();
    let (triangle, group) = (row(&harness, "Layer Triangle", 0, 14.0), row(&harness, "Layer Group", 0, 14.0));
    grab(&mut harness, triangle);
    move_to(&mut harness, group);
    assert_snapshot(&mut harness, "20-drop-into");
    press_at(&mut harness, group, false);
    let links: Vec<_> = (1..=3).map(|id| link(&harness, id)).collect();
    assert!(links[0].is_some_and(|l| l.kind == LinkKind::Group) && links.iter().all(|l| *l == links[0]), "all three share the Group link: {links:?}");
}

#[test]
fn dragging_a_member_to_the_top_level_takes_it_out_of_the_link() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Star, ShapeKind::Triangle]);
    harness.state_mut().design.group(&[1, 2, 3]);
    harness.run();
    let (circle, top) = (row(&harness, "Layer Circle", 0, 14.0), row(&harness, "Layer Triangle", 0, 4.0));
    drag_row(&mut harness, circle, top);
    assert_eq!(link(&harness, 2), None, "the circle has left the Group");
    assert!(link(&harness, 1).is_some() && link(&harness, 1) == link(&harness, 3), "the other two stay grouped");
    assert_eq!(order(&harness).last(), Some(&2), "and it is at the top");
}

#[test]
fn escape_cancels_a_drag() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Triangle]);
    harness.state_mut().design.group(&[1, 2]);
    harness.run();
    let before = harness.state().design.clone();
    let (triangle, group) = (row(&harness, "Layer Triangle", 0, 14.0), row(&harness, "Layer Group", 0, 14.0));
    grab(&mut harness, triangle);
    move_to(&mut harness, group);
    harness.press_key(egui::Key::Escape);
    harness.run();
    move_to(&mut harness, group + egui::vec2(0.0, 2.0));
    press_at(&mut harness, group, false);
    assert!(harness.state().design == before, "nothing changes");
    assert!(!harness.state().history.can_undo(), "and nothing is recorded");
}

#[test]
fn releasing_outside_the_list_cancels() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, [ShapeKind::Square, ShapeKind::Circle]);
    let before = harness.state().design.clone();
    let square = row(&harness, "Layer Square", 0, 14.0);
    drag_row(&mut harness, square, pos2(600.0, 400.0));
    assert!(harness.state().design == before);
}

#[test]
fn a_multi_selection_drags_together_with_a_ghost_and_insertion_line() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Triangle, ShapeKind::Star, ShapeKind::Heart]);
    harness.state_mut().selection = vec![1, 3];
    harness.run();
    let (square, under_heart) = (row(&harness, "Layer Square", 0, 14.0), row(&harness, "Layer Heart", 0, 25.0));
    grab(&mut harness, square);
    move_to(&mut harness, under_heart);
    assert_snapshot(&mut harness, "20-drag");
    press_at(&mut harness, under_heart, false);
    assert_eq!(order(&harness), vec![2, 4, 1, 3, 5], "both land under the heart, keeping their order");
}

#[test]
fn dragging_to_the_bottom_edge_scrolls_the_list() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    add(&mut harness, std::iter::repeat_n(ShapeKind::Circle, 30));
    for id in 1..=30 {
        harness.state_mut().design.shape_mut(id).unwrap().name = Some(format!("Row {id}"));
    }
    harness.run();
    // Only the rows in view are built, so the list has scrolled when lower rows have been.
    let lowest = |h: &App<'_>| (1..=30).find(|id| h.query_by_label(&format!("Layer Row {id}")).is_some()).unwrap();
    let start = lowest(&harness);
    let bar = harness.get_by_label("Layers Slice").bounding_box().unwrap().y0 as f32;
    let from = row(&harness, "Layer Row 30", 0, 14.0);
    grab(&mut harness, from);
    let edge = pos2(from.x, bar - 16.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(edge));
    for _ in 0..6 {
        harness.step();
    }
    assert!(lowest(&harness) + 2 <= start, "the list scrolled down: row {} from {start}", lowest(&harness));
}
