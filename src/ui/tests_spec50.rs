//! Spec 50: Select Same, Select Inverse, the container scope, Tab and the lasso, driven through the app.

use egui::{Color32, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{any_near, assert_snapshot, frame, rect_of, snapshot_harness, two_squares_apart};
use crate::design::{Design, Operation, ShapeKind};

/// A row of 1 inch squares in the given fills, from (1, 1), 1.5 inches apart. Returns their ids.
fn squares(harness: &mut Harness<'_, Kerf>, fills: &[Color32]) -> Vec<u64> {
    harness.state_mut().design.set_snapping(false);
    let mut design = Design::default();
    let mut ids = Vec::new();
    for (i, &fill) in fills.iter().enumerate() {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (1.0 + 1.5 * i as f32, 1.0, 1.0, 1.0);
        s.fill = fill;
        ids.push(id);
    }
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.run();
    ids
}

/// Clicks the match for `label` whose box sits furthest left: a menu row, not the inspector's control of
/// the same name on the right.
fn click_leftmost(harness: &mut Harness<'_, Kerf>, label: &str) {
    let node = harness
        .get_all_by_label(label)
        .min_by(|a, b| {
            let x = |n: &egui_kittest::kittest::Node| n.bounding_box().map_or(f64::MAX, |r| r.x0);
            x(a).total_cmp(&x(b))
        })
        .unwrap_or_else(|| panic!("no {label}"));
    node.click();
    harness.run();
}

#[test]
fn edit_select_same_colour_picks_every_red_and_the_inspector_shows_red() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let ids = squares(&mut harness, &[Color32::RED, Color32::BLUE, Color32::RED, Color32::BLUE, Color32::RED]);
    let reds: Vec<u64> = vec![ids[0], ids[2], ids[4]];
    harness.state_mut().selection = vec![ids[0]];
    harness.run();

    harness.get_by_label("Edit").click();
    harness.run();
    // The submenu opens on hover, not a click.
    let select_same = rect_of(&harness, "Select Same");
    for _ in 0..3 {
        super::test_harness::move_to(&mut harness, select_same.center());
    }
    harness.run();
    assert_snapshot(&mut harness, "50-select-same-menu");

    click_leftmost(&mut harness, "Colour");
    assert_eq!(harness.state().selection, reds, "every red shape is selected");

    // The rightmost "Colour" is the inspector's swatch (the menu's row sits on the left).
    let swatch = harness
        .get_all_by_label("Colour")
        .filter_map(|n| n.bounding_box())
        .max_by(|a, b| a.x0.total_cmp(&b.x0))
        .map(|r| egui::Rect::from_min_max(egui::pos2(r.x0 as f32, r.y0 as f32), egui::pos2(r.x1 as f32, r.y1 as f32)))
        .expect("an inspector colour swatch");
    assert!(any_near(&frame(&mut harness), swatch, Color32::RED, 20), "the inspector's colour swatch shows red");
}

#[test]
fn the_legend_pen_swatch_selects_every_pen_shape_through_select() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let mut design = Design::default();
    for (i, op) in [Operation::BasicCut, Operation::Pen, Operation::Score, Operation::Pen].into_iter().enumerate() {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in) = (1.0 + 1.5 * i as f32, 1.0);
        s.set_operation(op);
    }
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.run();
    let pens: Vec<u64> = harness.state().design.shapes.iter().filter(|s| s.operation == Operation::Pen).map(|s| s.id).collect();
    assert_eq!(pens.len(), 2);

    harness.get_by_label("Pen shapes").click();
    harness.run();
    assert_eq!(harness.state().selection, pens);
}

#[test]
fn tab_moves_the_selection_to_the_next_shape_in_layers() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let ids = squares(&mut harness, &[Color32::RED, Color32::RED, Color32::RED]);
    harness.state_mut().selection = vec![ids[0]];
    harness.run();
    super::test_harness::key(&mut harness, egui::Key::Tab, Modifiers::NONE);
    assert_eq!(harness.state().selection, vec![ids[1]], "Tab walks to the next shape");
}

/// Presses, drags along `path` with Option held, and releases, so an empty-canvas drag draws a lasso.
fn alt_lasso(harness: &mut Harness<'_, Kerf>, path: &[Pos2]) {
    let alt = Modifiers::ALT;
    let button = egui::PointerButton::Primary;
    harness.input_mut().modifiers = alt;
    harness.input_mut().events.push(egui::Event::PointerMoved(path[0]));
    harness.input_mut().events.push(egui::Event::PointerButton { pos: path[0], button, pressed: true, modifiers: alt });
    harness.run();
    for &p in &path[1..] {
        harness.input_mut().modifiers = alt;
        harness.input_mut().events.push(egui::Event::PointerMoved(p));
        harness.run();
    }
    harness.input_mut().modifiers = alt;
    harness.input_mut().events.push(egui::Event::PointerButton { pos: *path.last().unwrap(), button, pressed: false, modifiers: alt });
    harness.run();
    harness.input_mut().modifiers = Modifiers::NONE;
    harness.run();
}

#[test]
fn an_option_drag_lassoes_two_of_three_shapes_on_a_diagonal() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = two_squares_apart(&mut harness);
    let mut design = Design::default();
    let mut ids = Vec::new();
    for d in [1.0_f32, 3.0, 5.0] {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (d, d, 1.0, 1.0);
        ids.push(id);
    }
    design.set_snapping(false);
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.run();

    // A loop around the first two squares (corners inside), leaving the third at (5,5) out.
    let loop_in = [(0.5, 0.5), (4.5, 0.5), (4.5, 4.5), (0.5, 4.5), (0.5, 0.5)];
    let path: Vec<Pos2> = loop_in.iter().map(|&p| to_screen(p)).collect();

    // Mid-drag: press and draw most of the loop without releasing, for the snapshot.
    let alt = Modifiers::ALT;
    let button = egui::PointerButton::Primary;
    harness.input_mut().modifiers = alt;
    harness.input_mut().events.push(egui::Event::PointerMoved(path[0]));
    harness.input_mut().events.push(egui::Event::PointerButton { pos: path[0], button, pressed: true, modifiers: alt });
    harness.run();
    for &p in &path[1..4] {
        harness.input_mut().modifiers = alt;
        harness.input_mut().events.push(egui::Event::PointerMoved(p));
        harness.run();
    }
    assert_snapshot(&mut harness, "50-lasso");
    harness.input_mut().modifiers = alt;
    harness.input_mut().events.push(egui::Event::PointerButton { pos: path[3], button, pressed: false, modifiers: alt });
    harness.run();
    harness.input_mut().modifiers = Modifiers::NONE;
    harness.run();

    assert_eq!(harness.state().selection, vec![ids[0], ids[1]], "the two squares inside the lasso are selected, not the third");
}

#[test]
fn a_whole_fresh_lasso_gesture_selects_the_shape_it_encircles() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = two_squares_apart(&mut harness);
    let mut design = Design::default();
    let id = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (2.0, 2.0, 1.0, 1.0);
    design.set_snapping(false);
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.run();

    let loop_in = [(1.5, 1.5), (3.5, 1.5), (3.5, 3.5), (1.5, 3.5), (1.5, 1.5)];
    let path: Vec<Pos2> = loop_in.iter().map(|&p| to_screen(p)).collect();
    alt_lasso(&mut harness, &path);
    assert_eq!(harness.state().selection, vec![id]);
}
