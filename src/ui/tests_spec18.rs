//! Spec 18: containers driven through the app.

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, click_at, drag_along, pick, right_click_at, snapshot_harness, two_squares_apart};
use crate::design::{Pt, Shape, ShapeKind};

type App<'a> = Harness<'a, Kerf>;

const PALETTE: [egui::Color32; 5] = [
    egui::Color32::from_rgb(0xDD, 0xD6, 0xF3),
    egui::Color32::from_rgb(0x2A, 0x9D, 0x8F),
    egui::Color32::from_rgb(0xE7, 0x6F, 0x51),
    egui::Color32::from_rgb(0xF4, 0xA2, 0x61),
    egui::Color32::from_rgb(0x26, 0x46, 0x53),
];

/// Replaces the design with `shapes`, each a kind at a top-left corner with a side length in inches, and
/// returns the map from inches to screen points. Ids run from 1 in order.
fn place(harness: &mut App<'_>, shapes: &[(ShapeKind, Pt, f32)]) -> impl Fn(Pt) -> Pos2 + use<> {
    let at = two_squares_apart(harness);
    let design = &mut harness.state_mut().design;
    design.shapes.clear();
    for ((kind, (x, y), side), fill) in shapes.iter().zip(PALETTE.iter().cycle()) {
        let id = design.add_shape(kind.clone());
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in, s.fill) = (*x, *y, *side, *side, *fill);
    }
    harness.run();
    at
}

fn shape(harness: &App<'_>, id: u64) -> Shape {
    harness.state().design.shapes.iter().find(|s| s.id == id).cloned().unwrap_or_else(|| panic!("no shape {id}"))
}

fn centre(s: &Shape) -> Pt {
    let (x0, y0, x1, y1) = s.world_bounds();
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

fn close(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
}

/// Drags through `path`, first nudging past the drag threshold so the drag starts where it was pressed.
fn drag(harness: &mut App<'_>, path: &[Pos2]) {
    // Offset every point off the shape centre so the grab clears the pivot crosshair and moves the shape;
    // the net translation is unchanged (spec 52).
    let off = egui::vec2(14.0, 14.0);
    let mut steps = vec![path[0] + off, path[0] + off + egui::vec2(6.0, 0.0)];
    steps.extend(path[1..].iter().map(|&p| p + off));
    drag_along(harness, &steps);
}

fn edit_menu(harness: &mut App<'_>, item: &str) {
    harness.get_by_label("Edit").click();
    harness.run();
    pick(harness, item);
}

fn undo(harness: &mut App<'_>) {
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
}

fn row_left(harness: &App<'_>, label: &str) -> f32 {
    harness.get_by_label(label).bounding_box().unwrap().x0 as f32
}

#[test]
fn a_heart_made_a_container_centres_a_star_dropped_on_it() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = place(&mut harness, &[(ShapeKind::Heart, (1.0, 1.0), 3.0), (ShapeKind::Star, (6.0, 1.0), 0.75)]);
    right_click_at(&mut harness, at((2.5, 2.5)));
    pick(&mut harness, "Make container");
    assert!(shape(&harness, 1).container.is_some(), "the heart is a container");
    assert_eq!(harness.state().selection, vec![1], "and selected");

    drag(&mut harness, &[at((6.375, 1.375)), at((5.0, 2.0)), at((3.0, 2.0)), at((2.0, 2.0))]);
    assert_eq!(shape(&harness, 2).parent, Some(1), "the star dropped into the heart");
    assert_eq!(harness.state().selection, vec![2]);

    harness.get_by_label("Align").click();
    harness.run();
    harness.get_by_label("Align to Container");
    assert!(!harness.get_by_label("Align Centre").is_disabled(), "Container is the default, so one star can align");
    harness.get_by_label("Align Centre").click();
    harness.run();
    assert!(close(centre(&shape(&harness, 2)), centre(&shape(&harness, 1))), "the star sits on the heart's centre");

    harness.state_mut().selection = vec![1];
    harness.run();
    edit_menu(&mut harness, "Remove container");
    let (heart, star) = (shape(&harness, 1), shape(&harness, 2));
    assert!(heart.container.is_none() && star.parent.is_none(), "the role is gone");
    assert_eq!(harness.state().design.shapes.len(), 2, "both shapes stay");
}

#[test]
fn wrapping_three_shapes_and_centring_them_undoes_in_two_steps() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = place(&mut harness, &[(ShapeKind::Square, (6.0, 1.5), 1.0), (ShapeKind::Circle, (8.0, 2.0), 1.0), (ShapeKind::Triangle, (9.5, 3.0), 1.0)]);
    harness.state_mut().selection = vec![1, 2, 3];
    harness.run();
    edit_menu(&mut harness, "Wrap in container");
    let [c] = harness.state().selection[..] else { panic!("the container is selected") };
    assert!([1, 2, 3].iter().all(|&id| shape(&harness, id).parent == Some(c)), "each is a child");
    let wrapped = harness.state().design.clone();

    harness.get_by_label("Align").click();
    harness.run();
    assert_snapshot(&mut harness, "18-container");
    harness.get_by_label("Align Centre").click();
    harness.run();
    let middle = centre(&shape(&harness, c));
    assert!([1, 2, 3].iter().all(|&id| close(centre(&shape(&harness, id)), middle)), "all three centre in the container");

    undo(&mut harness);
    assert!(harness.state().design == wrapped, "one Undo reverts the align");
    undo(&mut harness);
    assert!(harness.state().design.shapes.iter().all(|s| s.container.is_none() && s.parent.is_none()), "a second removes the container");
    assert_eq!(harness.state().design.shapes.len(), 3);
}

#[test]
fn dragging_a_shape_onto_a_container_and_off_again() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = place(&mut harness, &[(ShapeKind::Square, (1.0, 1.0), 3.0), (ShapeKind::Circle, (6.0, 1.0), 1.0)]);
    harness.state_mut().design.contain(&[1]);
    harness.run();

    drag(&mut harness, &[at((6.5, 1.5)), at((4.0, 2.0)), at((2.5, 2.5))]);
    assert_eq!(shape(&harness, 2).parent, Some(1), "dropped in");
    assert!(row_left(&harness, "Layer Circle") > row_left(&harness, "Layer Square") + 10.0, "Layers indents the child");

    drag(&mut harness, &[at((2.5, 2.5)), at((5.0, 4.0)), at((8.0, 6.0))]);
    assert_eq!(shape(&harness, 2).parent, None, "dragged out");
    assert!((row_left(&harness, "Layer Circle") - row_left(&harness, "Layer Square")).abs() < 1.0, "back at the top level");
}

#[test]
fn cmd_held_while_dragging_keeps_the_shape_out() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = place(&mut harness, &[(ShapeKind::Square, (1.0, 1.0), 3.0), (ShapeKind::Circle, (6.0, 1.0), 1.0)]);
    harness.state_mut().design.contain(&[1]);
    harness.state_mut().selection = vec![2];
    harness.run();
    harness.input_mut().modifiers = egui::Modifiers::COMMAND;
    drag(&mut harness, &[at((6.5, 1.5)), at((4.0, 2.0)), at((2.5, 2.5))]);
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    harness.run();
    assert_eq!(shape(&harness, 2).parent, None);
}

#[test]
fn a_row_layout_places_a_new_child_after_the_others() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let small = |x: f32| (ShapeKind::Circle, (x, 1.5), 0.75);
    let at = place(&mut harness, &[(ShapeKind::Square, (1.0, 1.0), 4.0), small(1.5), small(3.0), small(2.0), (ShapeKind::Triangle, (8.0, 6.0), 0.75)]);
    let design = &mut harness.state_mut().design;
    let c = design.contain(&[1]).unwrap();
    design.reparent(&[2, 3, 4], Some(c));
    harness.state_mut().selection = vec![c];
    harness.run();

    harness.get_by_label("Layout Row").click();
    harness.run();
    let xs: Vec<f32> = [2, 3, 4].iter().map(|&id| shape(&harness, id).x_in).collect();
    assert!(xs.windows(2).all(|w| w[1] > w[0] + 0.75), "the children run left to right in z-order: {xs:?}");

    drag(&mut harness, &[at((8.375, 6.5)), at((5.0, 3.0)), at((2.0, 1.6))]);
    assert_eq!(shape(&harness, 5).parent, Some(c), "the triangle joined");
    let last = shape(&harness, 4);
    let new = shape(&harness, 5);
    assert!(new.x_in > last.x_in + last.width_in, "placed after the others: {} after {}", new.x_in, last.x_in);
    assert!((new.y_in - last.y_in).abs() < 1e-3, "on the same row");

    harness.state_mut().selection = vec![c];
    harness.run();
    assert_snapshot(&mut harness, "18-layout-row");
}

#[test]
fn a_marquee_selects_a_guide_container_with_children() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = place(&mut harness, &[(ShapeKind::Square, (2.0, 2.0), 1.0), (ShapeKind::Circle, (4.0, 2.0), 1.0)]);
    let c = harness.state_mut().design.contain(&[1, 2]).unwrap();
    harness.state_mut().selection.clear();
    harness.run();
    drag(&mut harness, &[at((0.5, 0.5)), at((3.0, 3.0)), at((6.0, 4.0))]);
    assert!(harness.state().selection.contains(&c), "the Guide container is picked up: {:?}", harness.state().selection);
    click_at(&mut harness, at((10.0, 10.0)));
}
