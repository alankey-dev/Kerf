//! Spec 06: the selection frame, size badge and action bar, driven through the app.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, drag_along, move_to, press_at, snapshot_harness, two_squares_apart};

fn selection_union(harness: &Harness<'_, Kerf>) -> (f32, f32, f32, f32) {
    let state = harness.state();
    let chosen = state.design.shapes.iter().filter(|s| state.selection.contains(&s.id)).map(|s| s.aabb());
    let b = chosen.reduce(crate::snap::Aabb::union).unwrap();
    (b.min_x, b.min_y, b.max_x, b.max_y)
}

#[test]
fn dragging_the_multi_frame_corner_grows_every_shape_and_the_badge_follows() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    let before: Vec<(f32, f32)> = harness.state().design.shapes.iter().map(|s| (s.width_in, s.height_in)).collect();
    let (_, _, x1, y1) = selection_union(&harness);
    let corner = at((x1, y1));
    drag_along(&mut harness, &[corner, corner + egui::vec2(4.0, 4.0), at((x1 + 1.0, y1 + 1.0))]);
    let after: Vec<(f32, f32)> = harness.state().design.shapes.iter().map(|s| (s.width_in, s.height_in)).collect();
    assert!(after.iter().zip(&before).all(|(a, b)| a.0 > b.0 && a.1 > b.1), "both grew: {before:?} to {after:?}");
    let (x0, y0, x1, y1) = selection_union(&harness);
    harness.get_by_label(&harness.state().units.size(x1 - x0, y1 - y0));
}

#[test]
fn action_bar_duplicates_and_flips_from_more() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1];
    harness.run();
    harness.get_by_label("Duplicate selection").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 3, "a copy was added");
    let copy = harness.state().selection.clone();
    assert!(!copy.contains(&1), "the copy is selected");

    harness.get_by_label("More actions").click();
    harness.run();
    harness.get_by_label("Flip Horizontal").click();
    harness.run();
    let flipped = harness.state().design.shapes.iter().filter(|s| copy.contains(&s.id)).all(|s| s.flip_x);
    assert!(flipped, "More then Flip Horizontal flips the selection");
    assert!(harness.query_by_label("Flip Horizontal").is_none(), "the menu closes");
}

#[test]
fn the_action_bar_leaves_the_tree_while_dragging() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1];
    harness.run();
    assert!(harness.query_by_label("Duplicate selection").is_some());
    move_to(&mut harness, at((1.5, 1.5)));
    press_at(&mut harness, at((1.5, 1.5)), true);
    move_to(&mut harness, at((1.6, 1.6)));
    move_to(&mut harness, at((2.0, 2.0)));
    assert!(harness.query_by_label("Duplicate selection").is_none(), "hidden mid-drag");
    press_at(&mut harness, at((2.0, 2.0)), false);
    assert!(harness.query_by_label("Duplicate selection").is_some(), "back after release");
}

#[test]
fn near_the_canvas_bottom_the_badge_and_bar_sit_above_the_frame() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    let y_in = (860.0 - at((0.0, 0.0)).y) / 72.0 - harness.state().design.shapes[0].height_in;
    harness.state_mut().design.shapes[0].y_in = y_in;
    harness.state_mut().selection = vec![1];
    harness.run();
    let s = harness.state().design.shapes[0].clone();
    let top = f64::from(at((0.0, y_in)).y);
    let label = harness.state().units.size(s.width_in, s.height_in);
    let badge = harness.get_by_label(&label).bounding_box().unwrap();
    let bar = harness.get_by_label("Delete selection").bounding_box().unwrap();
    assert!(badge.y1 < top && bar.y1 < badge.y0, "bar above badge above the frame top {top}: {bar:?} {badge:?}");
}

#[test]
fn renders_the_selection_chrome() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1];
    assert_snapshot(&mut harness, "06-single");
    harness.state_mut().selection = vec![1, 2];
    assert_snapshot(&mut harness, "06-multi");

    let text = harness.state_mut().design.add_text("Text".into(), crate::text::FontRef::Inter, 1.2, egui::Color32::BLACK, 2.0, 4.0);
    harness.state_mut().selection = vec![text];
    assert_snapshot(&mut harness, "06-text");
    assert!(harness.query_by_label("Detach selection").is_none() && harness.query_by_label("Delete selection").is_some());

    harness.state_mut().design.shapes[0].rotation_deg = 30.0;
    harness.state_mut().selection = vec![1];
    assert_snapshot(&mut harness, "06-rotated");
}
