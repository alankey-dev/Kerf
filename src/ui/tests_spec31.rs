//! Spec 31: Make component, Detach instance, the Components panel, editing a master in isolation and
//! overriding parts, driven through the real app.

use std::f32::consts::FRAC_1_SQRT_2;

use egui::Modifiers;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{
    Fixture, any_near, assert_snapshot, bare_frame, click_at, double_click_at, drag_along, key, move_to, pick, rect_of, right_click_at, snapshot_harness,
    two_squares_apart,
};
use crate::design::{Design, ShapeKind};

type App<'a> = Harness<'a, Kerf>;

const CMD_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

/// The app over two squares and a circle, all selected, nothing to undo; and a map from inches to screen.
fn three_selected() -> (Fixture, impl Fn((f32, f32)) -> egui::Pos2) {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = two_squares_apart(&mut harness);
    let state = harness.state_mut();
    let circle = state.design.add_shape(ShapeKind::Circle);
    (state.design.shape_mut(circle).unwrap().x_in, state.design.shape_mut(circle).unwrap().y_in) = (1.0, 4.0);
    state.selection = state.design.shapes.iter().map(|s| s.id).collect();
    harness.run();
    (harness, to_screen)
}

fn instance(harness: &App<'_>) -> u64 {
    let [id] = harness.state().selection[..] else { panic!("one object selected: {:?}", harness.state().selection) };
    assert!(harness.state().design.shapes.iter().any(|s| s.id == id && s.instance_of().is_some()), "the selection is an instance");
    id
}

fn plain(design: &Design) -> Vec<ShapeKind> {
    design.shapes.iter().map(|s| s.kind.clone()).collect()
}

#[test]
fn make_component_by_shortcut_makes_one_instance_and_one_undo_restores_the_three_shapes() {
    let (mut harness, _) = three_selected();
    let before = harness.state().design.clone();
    key(&mut harness, egui::Key::K, CMD_ALT);
    instance(&harness);
    let design = &harness.state().design;
    assert_eq!(design.shapes.len(), 1);
    assert_eq!(design.components.len(), 1);
    assert_eq!(design.components[0].shapes.len(), 3);
    assert_eq!(harness.get_by_label("Layer Component 1").toggled(), Some(egui::accesskit::Toggled::True), "Layers lists the instance, selected");
    assert_eq!(harness.get_all_by_label("Part Square").count(), 2, "with its parts under it");
    assert!(harness.query_by_label("Part Circle").is_some());
    assert!(harness.query_by_label("Component").is_some(), "the component badge");
    harness.get_by_label("Components").click();
    harness.run();
    assert_eq!(harness.query_all_by_label_contains("Component tile").count(), 1, "the panel shows one tile");
    assert!(harness.query_by_label("Component tile Component 1").is_some());
    key(&mut harness, egui::Key::Z, Modifiers::COMMAND);
    assert_eq!(plain(&harness.state().design), plain(&before), "one Undo brings the three shapes back");
    assert!(harness.state().design.components.is_empty());
    assert_eq!(harness.query_all_by_label_contains("Component tile").count(), 0, "and takes the tile away");
}

#[test]
fn make_component_from_the_edit_menu() {
    let (mut harness, _) = three_selected();
    harness.get_by_label("Edit").click();
    harness.run();
    pick(&mut harness, "Make component");
    instance(&harness);
    assert_eq!(harness.state().design.shapes.len(), 1);
}

#[test]
fn clicking_a_part_row_selects_its_instance() {
    let (mut harness, _) = three_selected();
    key(&mut harness, egui::Key::K, CMD_ALT);
    let id = instance(&harness);
    harness.state_mut().selection.clear();
    harness.run();
    harness.get_by_label("Part Circle").click();
    harness.run();
    assert_eq!(harness.state().selection, [id]);
}

#[test]
fn detach_instance_leaves_a_group_of_plain_shapes_in_layers() {
    let (mut harness, _) = three_selected();
    key(&mut harness, egui::Key::K, CMD_ALT);
    instance(&harness);
    key(&mut harness, egui::Key::K, CMD_ALT.plus(Modifiers::SHIFT));
    let design = &harness.state().design;
    assert!(design.shapes.iter().all(|s| s.instance_of().is_none()), "no instance left");
    assert_eq!(design.shapes.len(), 3);
    assert_eq!(harness.state().selection.len(), 3, "the plain shapes are held");
    assert!(harness.query_by_label("Layer Group").is_some(), "Layers shows a Group");
    assert_eq!(harness.get_all_by_label("Layer Square").count(), 2);
    assert!(harness.query_by_label("Layer Circle").is_some());
    assert!(harness.query_by_label("Part Square").is_none(), "no part rows left");
}

#[test]
fn the_context_menu_detaches_an_instance() {
    let (mut harness, to_screen) = three_selected();
    key(&mut harness, egui::Key::K, CMD_ALT);
    instance(&harness);
    right_click_at(&mut harness, to_screen((2.0, 2.0)));
    pick(&mut harness, "Detach instance");
    assert!(harness.state().design.shapes.iter().all(|s| s.instance_of().is_none()));
}

#[test]
fn locking_from_the_instance_row_locks_the_instance_and_canvas_clicks_pass_it() {
    let (mut harness, to_screen) = three_selected();
    key(&mut harness, egui::Key::K, CMD_ALT);
    let id = instance(&harness);
    harness.state_mut().selection.clear();
    harness.run();
    click_at(&mut harness, to_screen((2.0, 2.0)));
    assert_eq!(harness.state().selection, [id], "an unlocked instance takes the click");
    let row = rect_of(&harness, "Layer Component 1");
    move_to(&mut harness, row.left_center() + egui::vec2(4.0, 0.0));
    harness.get_by_label("Lock Component 1").click();
    harness.run();
    let inst = harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().clone();
    assert!(inst.locked, "the instance shape itself is locked");
    harness.state_mut().selection.clear();
    harness.run();
    click_at(&mut harness, to_screen((2.0, 2.0)));
    assert!(harness.state().selection.is_empty(), "a click on a locked instance selects nothing");
}

fn type_text(harness: &mut App<'_>, text: &str) {
    harness.input_mut().events.push(egui::Event::Text(text.into()));
    harness.run();
}

fn instances(design: &Design) -> Vec<crate::design::Shape> {
    design.shapes.iter().filter(|s| s.instance_of().is_some()).cloned().collect()
}

/// The colour every piece of `shape` draws in.
fn fills(design: &Design, shape: &crate::design::Shape) -> Vec<egui::Color32> {
    design.geometry(shape).pieces().iter().map(|p| p.style.fill).collect()
}

/// The app with the first of two squares, turned into `first`, made into a component, its instance
/// selected, and the Components tab open.
fn one_component(first: ShapeKind) -> (Fixture, impl Fn((f32, f32)) -> egui::Pos2) {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = two_squares_apart(&mut harness);
    let shape = &mut harness.state_mut().design.shapes[0];
    shape.kind = first;
    harness.state_mut().selection = vec![harness.state().design.shapes[0].id];
    harness.run();
    key(&mut harness, egui::Key::K, CMD_ALT);
    instance(&harness);
    harness.get_by_label("Components").click();
    harness.run();
    (harness, to_screen)
}

#[test]
fn dragging_a_tile_twice_then_recolouring_the_master_in_isolation_recolours_every_instance() {
    let (mut harness, to_screen) = one_component(ShapeKind::Square);
    let tile = rect_of(&harness, "Component tile Component 1").center();
    for at in [(5.0, 6.0), (8.0, 6.0)] {
        drag_along(&mut harness, &[tile, tile + egui::vec2(-12.0, 0.0), tile + egui::vec2(-60.0, 10.0), to_screen(at), to_screen(at)]);
        let placed = instances(&harness.state().design).last().unwrap().clone();
        let centre = (placed.x_in + placed.width_in / 2.0, placed.y_in + placed.height_in / 2.0);
        assert!((centre.0 - at.0).abs() < 0.05 && (centre.1 - at.1).abs() < 0.05, "dropped centred where it was let go: {centre:?}");
        assert_eq!(harness.state().selection, [placed.id], "the new instance is selected");
    }
    assert_eq!(instances(&harness.state().design).len(), 3, "two dragged in beside the first");
    let depth = harness.state().history.depth();
    let opened = instances(&harness.state().design)[1].id;

    double_click_at(&mut harness, to_screen((5.0, 6.0)));
    assert!(harness.state().isolation.is_some(), "double-clicking an instance opens its master");
    assert!(harness.query_by_label("Component: Component 1").is_some(), "the breadcrumb names it");
    assert!(harness.query_by_label("Instance preview 3").is_some(), "the strip previews all three instances");
    assert_eq!(harness.state().history.depth(), depth, "opening a master is not an undo step");

    key(&mut harness, egui::Key::A, Modifiers::COMMAND);
    assert_eq!(harness.state().selection.len(), 1, "Select All in isolation holds the master's one shape");
    harness.get_by_label("Layers").click();
    harness.run();
    key(&mut harness, egui::Key::K, Modifiers::COMMAND);
    type_text(&mut harness, "Set colour to Wine");
    key(&mut harness, egui::Key::Enter, Modifiers::NONE);
    let wine = crate::pens::Pen::named("Wine", Default::default()).rgb();
    assert_eq!(harness.state().history.depth(), depth + 1, "a master edit is one ordinary undo step");
    assert_snapshot(&mut harness, "31-isolation");

    harness.get_by_label("Done").click();
    harness.run();
    assert!(harness.state().isolation.is_none());
    assert_eq!(harness.state().selection, [opened], "Done gives the canvas its selection back");
    let design = &harness.state().design;
    assert_eq!(design.shapes.len(), 4, "the canvas is back: three instances and the other square");
    for inst in instances(design) {
        assert_eq!(fills(design, &inst), [wine], "every instance shows the new colour");
    }
    assert_eq!(harness.state().history.depth(), depth + 1, "Done is not an undo step");

    key(&mut harness, egui::Key::Z, Modifiers::COMMAND);
    let design = &harness.state().design;
    assert!(instances(design).iter().all(|i| fills(design, i) != [wine]), "Undo after Done reverts the master edit");
}

#[test]
fn isolation_closes_when_undo_takes_its_component_away() {
    let (mut harness, _) = one_component(ShapeKind::Square);
    let selected = harness.state().selection.clone();
    let tile = rect_of(&harness, "Component tile Component 1").center();
    right_click_at(&mut harness, tile);
    pick(&mut harness, "Edit master");
    assert!(harness.state().isolation.is_some(), "Edit master from the panel opens it");
    assert!(harness.state().selection.is_empty());
    key(&mut harness, egui::Key::Z, Modifiers::COMMAND);
    assert!(harness.state().isolation.is_none(), "with Make component undone there is no master to edit");
    assert!(harness.query_by_label("Done").is_none());
    assert_eq!(harness.state().design.shapes.len(), 2);
    assert!(!selected.is_empty() && harness.state().selection.is_empty(), "the instance the canvas held went with the Undo");
}

#[test]
fn clicking_a_tile_places_an_instance_and_its_menu_renames_duplicates_and_deletes() {
    let (mut harness, _) = one_component(ShapeKind::Square);
    let tile = rect_of(&harness, "Component tile Component 1").center();
    click_at(&mut harness, tile);
    assert_eq!(instances(&harness.state().design).len(), 2, "a click places another instance");

    right_click_at(&mut harness, tile);
    pick(&mut harness, "Rename");
    for _ in "Component 1".chars() {
        key(&mut harness, egui::Key::Backspace, Modifiers::NONE);
    }
    type_text(&mut harness, "Tag");
    key(&mut harness, egui::Key::Enter, Modifiers::NONE);
    assert_eq!(harness.state().design.components[0].name, "Tag");
    assert_eq!(instances(&harness.state().design).len(), 2, "typing the name deleted nothing");

    let tile = rect_of(&harness, "Component tile Tag").center();
    right_click_at(&mut harness, tile);
    pick(&mut harness, "Duplicate component");
    assert_eq!(harness.state().design.components.len(), 2);
    assert!(harness.query_by_label("Component tile Tag copy").is_some());
    move_to(&mut harness, egui::pos2(1300.0, 750.0));
    assert_snapshot(&mut harness, "31-panel");

    right_click_at(&mut harness, tile);
    pick(&mut harness, "Delete");
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(harness.state().design.components.len(), 2, "Cancel keeps the component");

    right_click_at(&mut harness, tile);
    pick(&mut harness, "Delete");
    harness.get_by_label("Detach 2 instances and delete").click();
    harness.run();
    let design = &harness.state().design;
    assert_eq!(design.components.len(), 1);
    assert!(instances(design).is_empty(), "no dangling instance: both were detached");
    assert_eq!(design.shapes.len(), 3, "two detached squares and the other square");

    let copy = rect_of(&harness, "Component tile Tag copy").center();
    right_click_at(&mut harness, copy);
    pick(&mut harness, "Delete");
    assert!(harness.state().design.components.is_empty(), "a component with no instances goes without asking");
}

#[test]
fn overriding_a_part_to_pen_then_reset_overrides_returns_it_to_basic_cut() {
    let (mut harness, to_screen) = one_component(ShapeKind::Circle);
    let id = instance(&harness);
    let part = |harness: &App<'_>| {
        let design = &harness.state().design;
        crate::components::expand(design, design.shapes.iter().find(|s| s.id == id).unwrap()).unwrap()[0].clone()
    };
    assert_eq!(part(&harness).operation, crate::design::Operation::BasicCut);
    harness.get_by_label("Part 1 operation").click();
    harness.run();
    pick(&mut harness, "Pen");
    assert_eq!(part(&harness).operation, crate::design::Operation::Pen, "the part is overridden to Pen");
    assert!(harness.query_by_label("Part 1 overridden").is_some(), "with a dot");
    let frame = assert_snapshot(&mut harness, "31-overrides");
    let on_the_circle = egui::Rect::from_center_size(to_screen((2.0 + FRAC_1_SQRT_2, 2.0 - FRAC_1_SQRT_2)), egui::Vec2::splat(6.0));
    assert!(any_near(&frame, on_the_circle, egui::Color32::BLACK, 90), "the overridden part's pen outline is painted");
    let inside = egui::Rect::from_center_size(to_screen((2.4, 1.6)), egui::Vec2::splat(6.0));
    assert!(!any_near(&frame, inside, egui::Color32::BLACK, 90), "as an outline, not a fill");
    assert!(!any_near(&bare_frame(&mut harness), on_the_circle, egui::Color32::BLACK, 90), "and only the outline is dark there");
    let master = &harness.state().design.components[0].shapes[0];
    assert_eq!(master.operation, crate::design::Operation::BasicCut, "the master is untouched");

    harness.get_by_label("Reset overrides").click();
    harness.run();
    assert_eq!(part(&harness).operation, crate::design::Operation::BasicCut, "Reset overrides returns it to Basic Cut");
    assert!(harness.query_by_label("Part 1 overridden").is_none());
}

#[test]
fn reset_this_part_from_a_part_rows_menu() {
    let (mut harness, _) = one_component(ShapeKind::Square);
    harness.get_by_label("Part 1 operation").click();
    harness.run();
    pick(&mut harness, "Pen");
    let row = rect_of(&harness, "Part 1 Square");
    right_click_at(&mut harness, row.center());
    pick(&mut harness, "Reset this part");
    let design = &harness.state().design;
    assert!(instances(design).iter().all(|i| matches!(&i.kind, ShapeKind::Instance { overrides, .. } if overrides.is_empty())));
}

#[test]
fn a_bundled_pattern_picked_on_a_part_overrides_that_part_only() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    _ = two_squares_apart(&mut harness);
    let first = harness.state().design.shapes[0].id;
    harness.state_mut().design.shape_mut(first).unwrap().set_operation(crate::design::Operation::PrintThenCut);
    harness.state_mut().selection = vec![first];
    harness.run();
    key(&mut harness, egui::Key::K, CMD_ALT);
    let id = instance(&harness);
    harness.get_by_label("Part 1 colour").click();
    harness.run();
    harness.get_by_label("Pattern").click();
    harness.run();
    harness.get_by_label("Stripes, Berry").click();
    harness.run();
    let stripes = crate::assets::id_of(&crate::patterns::generate(crate::patterns::Bundled::Stripes, &crate::patterns::COLOURWAYS[0]));
    let design = &harness.state().design;
    let part = crate::components::expand(design, design.shapes.iter().find(|s| s.id == id).unwrap()).unwrap().remove(0);
    assert_eq!(part.pattern.map(|p| p.asset), Some(stripes), "the part prints the stripes on this instance");
    assert!(design.components[0].shapes[0].pattern.is_none(), "the master keeps its flat colour");
    assert!(design.shapes.iter().find(|s| s.id == id).unwrap().pattern.is_none(), "the instance itself takes no pattern");
    assert!(harness.query_by_label("Part 1 overridden").is_some());
}
