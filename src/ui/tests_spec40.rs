//! Spec 40: long layer and component names stop short of the row's controls, ending in an ellipsis,
//! and layers and components rename in place.

use egui::{Key, Modifiers, Pos2, Rect, epaint::ClippedShape};
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_snapshot, click_at, double_click_at, key, move_to, pick, rect_of, right_click_at, snapshot_harness};
use crate::design::ShapeKind;
use crate::shape_library::BuiltinId;

const LONG: &str = "Birthday card front panel";

/// Where the text `full` was painted this frame, clipped to what shows, and whether it was cut short.
fn painted(harness: &Fixture, full: &str) -> (Rect, bool) {
    fn find(shape: &egui::Shape, clip: Rect, full: &str) -> Option<(Rect, bool)> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == full => Some((t.galley.rect.translate(t.pos.to_vec2()).intersect(clip), t.galley.elided)),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, clip, full)),
            _ => None,
        }
    }
    let shapes = &harness.output().shapes;
    shapes.iter().find_map(|ClippedShape { clip_rect, shape }| find(shape, *clip_rect, full)).unwrap_or_else(|| panic!("{full} is painted"))
}

fn speech_bubble() -> ShapeKind {
    ShapeKind::Builtin(BuiltinId::named("Speech Bubble").unwrap())
}

/// The app over `kinds`, nothing selected, Layers listing them top first.
fn with_layers(kinds: impl IntoIterator<Item = ShapeKind>) -> Fixture {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    for kind in kinds {
        harness.state_mut().design.add_shape(kind);
    }
    harness.run();
    harness
}

/// A point on the name of the row labelled `row`.
fn on_name(harness: &Fixture, row: &str) -> Pos2 {
    let r = rect_of(harness, row);
    egui::pos2(r.left() + 60.0, r.top() + 14.0)
}

fn type_text(harness: &mut Fixture, text: &str) {
    harness.input_mut().events.push(egui::Event::Text(text.into()));
    harness.run();
}

fn name(harness: &Fixture, id: u64) -> String {
    let d = &harness.state().design;
    d.layer_name(d.shapes.iter().find(|s| s.id == id).unwrap())
}

fn away(harness: &mut Fixture) {
    move_to(harness, egui::pos2(600.0, 700.0));
}

fn undo(harness: &mut Fixture) {
    away(harness);
    key(harness, Key::Z, Modifiers::COMMAND);
}

#[test]
fn a_long_layer_name_stops_short_of_the_eye() {
    let harness = with_layers([speech_bubble()]);
    let (name, _) = painted(&harness, "Speech Bubble");
    let eye = rect_of(&harness, "Hide Speech Bubble");
    assert!(name.right() < eye.left(), "the name ends at {} but the eye starts at {}", name.right(), eye.left());
}

/// A shape, a group with an indented member, a container and an instance with its part, each named long.
fn long_names() -> Fixture {
    let mut harness = with_layers([ShapeKind::Square, ShapeKind::Circle, ShapeKind::Heart, ShapeKind::Star, speech_bubble()]);
    let d = &mut harness.state_mut().design;
    let group = d.group(&[1, 2]).unwrap();
    d.link_names.insert(group, format!("{LONG} group"));
    d.shape_mut(1).unwrap().name = Some(format!("{LONG} member"));
    let container = d.contain(&[3]).unwrap();
    d.shape_mut(container).unwrap().name = Some(format!("{LONG} container"));
    d.shape_mut(4).unwrap().name = Some(format!("{LONG} part"));
    d.make_component(&[4]).unwrap();
    d.components[0].name = format!("{LONG} component");
    harness.run();
    harness
}

#[test]
fn every_row_kind_cuts_a_long_name_short_with_an_ellipsis_before_its_first_icon() {
    let mut harness = long_names();
    for what in ["group", "member", "container", "component"] {
        let full = format!("{LONG} {what}");
        let (name, cut) = painted(&harness, &full);
        let eye = rect_of(&harness, &format!("Hide {full}"));
        assert!(cut, "{full} ends in an ellipsis");
        assert!(name.right() < eye.left(), "{full} ends at {} under its eye at {}", name.right(), eye.left());
    }
    let (part, cut) = painted(&harness, &format!("{LONG} part"));
    assert!(cut && part.right() < rect_of(&harness, &format!("Part {LONG} part")).right(), "a part row cuts its name short too");
    let (short, cut) = painted(&harness, "Circle");
    assert!(!cut && short.right() < rect_of(&harness, "Hide Circle").left(), "a name that fits shows whole");
    away(&mut harness);
    assert_snapshot(&mut harness, "40-layers-truncated");
}

#[test]
fn hovering_a_cut_short_name_shows_it_whole_and_its_icons_still_click() {
    let mut harness = long_names();
    let full = format!("{LONG} member");
    let at = on_name(&harness, &format!("Layer {full}"));
    move_to(&mut harness, at);
    for _ in 0..8 {
        harness.step();
    }
    harness.run();
    assert!(harness.query_by_label(&full).is_some(), "the tooltip shows the whole name");
    harness.get_by_label(&format!("Hide {full}")).click();
    harness.run();
    assert!(harness.state().design.shapes.iter().find(|s| s.id == 1).unwrap().hidden, "the eye beside a long name still hides it");
}

#[test]
fn double_clicking_a_name_renames_it_in_one_undo_step_and_leaves_the_selection_be() {
    let mut harness = with_layers([ShapeKind::Octagon, ShapeKind::Square]);
    harness.state_mut().selection = vec![2];
    harness.run();
    let at = on_name(&harness, "Layer Octagon");
    double_click_at(&mut harness, at);
    assert!(harness.query_by_label("Layer name").is_some(), "the name opens for typing");
    type_text(&mut harness, "Speech Bubble");
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(name(&harness, 1), "Speech Bubble", "typing replaced the whole name");
    assert!(harness.query_by_label("Layer name").is_none(), "Enter closes the field");
    assert!(harness.query_by_label("Layer Speech Bubble").is_some());
    assert_eq!(harness.state().selection, vec![1], "the double-click's first click picked the row");
    undo(&mut harness);
    assert_eq!(name(&harness, 1), "Octagon", "one Undo gives the label back");
}

#[test]
fn f2_renames_the_selected_layer_escape_cancels_and_a_blank_name_restores_the_label() {
    let mut harness = with_layers([ShapeKind::Octagon]);
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::F2, Modifiers::NONE);
    type_text(&mut harness, "Badge");
    key(&mut harness, Key::Escape, Modifiers::NONE);
    assert_eq!(name(&harness, 1), "Octagon", "Escape keeps the old name");
    assert_eq!(harness.state().selection, vec![1], "and the selection");

    key(&mut harness, Key::F2, Modifiers::NONE);
    type_text(&mut harness, "Badge");
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(name(&harness, 1), "Badge");
    assert_eq!(harness.state().design.shapes[0].name.as_deref(), Some("Badge"));
    assert!(harness.query_by_label("Layer name").is_none(), "the Enter that commits does not open the field again");

    key(&mut harness, Key::Enter, Modifiers::NONE);
    key(&mut harness, Key::Backspace, Modifiers::NONE);
    away(&mut harness);
    click_at(&mut harness, egui::pos2(1280.0, 700.0));
    assert!(harness.query_by_label("Layer name").is_none(), "a click away commits");
    assert_eq!(harness.state().design.shapes[0].name, None, "a blank name clears back to the label");
    assert_eq!(name(&harness, 1), "Octagon");
}

#[test]
fn rename_in_the_context_menu_names_a_whole_group() {
    let mut harness = with_layers([ShapeKind::Square, ShapeKind::Circle]);
    let group = harness.state_mut().design.group(&[1, 2]).unwrap();
    harness.run();
    let at = on_name(&harness, "Layer Group");
    right_click_at(&mut harness, at);
    pick(&mut harness, "Rename");
    type_text(&mut harness, "Badge");
    let at = on_name(&harness, "Layer Group");
    move_to(&mut harness, at);
    assert_snapshot(&mut harness, "40-rename");
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(harness.state().design.link_names.get(&group).map(String::as_str), Some("Badge"));
    assert!(harness.query_by_label("Layer Badge").is_some(), "the parent row shows the name");
    assert_eq!(harness.state().selection.len(), 2, "the menu acted on the whole group");
}

#[test]
fn enter_renames_one_selected_layer_but_not_two() {
    let mut harness = with_layers([ShapeKind::Octagon, ShapeKind::Square]);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.query_by_label("Layer name").is_none(), "two layers do not rename");
    harness.state_mut().selection = vec![2];
    harness.run();
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.query_by_label("Layer name").is_some(), "one layer does");
}

#[test]
fn the_palette_lists_rename() {
    let mut harness = with_layers([ShapeKind::Octagon]);
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::K, Modifiers::COMMAND);
    type_text(&mut harness, "Rename");
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.query_by_label("Layer name").is_some(), "the palette's Rename opens the field");
}

#[test]
fn a_component_renames_by_double_clicking_its_name_and_a_long_one_fits_its_tile() {
    let mut harness = with_layers([ShapeKind::Square]);
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::K, Modifiers::COMMAND.plus(Modifiers::ALT));
    harness.get_by_label("Components").click();
    harness.run();
    let tile = rect_of(&harness, "Component tile Component 1");
    let caption = egui::pos2(tile.center().x, tile.top() + 92.0);
    let instances = |h: &Fixture| h.state().design.shapes.iter().filter(|s| s.instance_of().is_some()).count();
    double_click_at(&mut harness, caption);
    assert_eq!(instances(&harness), 1, "a click on the name places nothing");
    assert!(harness.query_by_label("Component name").is_some(), "the name opens for typing");
    type_text(&mut harness, LONG);
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(harness.state().design.components[0].name, LONG);
    let tile = rect_of(&harness, &format!("Component tile {LONG}"));
    let (name, cut) = painted(&harness, LONG);
    assert!(cut && name.left() >= tile.left() && name.right() <= tile.right(), "{name:?} fits {tile:?} with an ellipsis");
    undo(&mut harness);
    assert_eq!(harness.state().design.components[0].name, "Component 1", "one Undo");
}

#[test]
fn clicking_another_row_commits_the_name_and_selects_that_row() {
    let mut harness = with_layers([ShapeKind::Octagon, ShapeKind::Square]);
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::F2, Modifiers::NONE);
    type_text(&mut harness, "Badge");
    let at = on_name(&harness, "Layer Square");
    click_at(&mut harness, at);
    assert_eq!(name(&harness, 1), "Badge", "the click away commits");
    assert_eq!(harness.state().selection, vec![2], "and selects the row it landed on");
    assert!(harness.query_by_label("Layer name").is_none());
}

#[test]
fn f2_renames_a_master_shape_while_its_component_is_open() {
    let mut harness = with_layers([ShapeKind::Square]);
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::K, Modifiers::COMMAND.plus(Modifiers::ALT));
    harness.get_by_label("Components").click();
    harness.run();
    let tile = rect_of(&harness, "Component tile Component 1").center();
    right_click_at(&mut harness, tile);
    pick(&mut harness, "Edit master");
    harness.get_by_label("Layers").click();
    harness.run();
    let at = on_name(&harness, "Layer Square");
    click_at(&mut harness, at);
    key(&mut harness, Key::F2, Modifiers::NONE);
    type_text(&mut harness, "Badge");
    key(&mut harness, Key::Enter, Modifiers::NONE);
    harness.get_by_label("Done").click();
    harness.run();
    assert_eq!(harness.state().design.components[0].shapes[0].name.as_deref(), Some("Badge"), "the master's shape took the name");
}

#[test]
fn a_rename_left_open_behind_the_components_tab_commits_and_does_not_reopen() {
    let mut harness = with_layers([ShapeKind::Octagon]);
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::F2, Modifiers::NONE);
    type_text(&mut harness, "Badge");
    harness.get_by_label("Components").click();
    harness.run();
    assert_eq!(name(&harness, 1), "Badge", "leaving for Components commits what was typed");
    harness.get_by_label("Layers").click();
    harness.run();
    assert!(harness.query_by_label("Layer name").is_none(), "the field does not come back");
}

#[test]
fn f2_on_a_member_of_a_collapsed_group_leaves_nothing_open() {
    let mut harness = with_layers([ShapeKind::Square, ShapeKind::Circle]);
    harness.state_mut().design.group(&[1, 2]);
    harness.run();
    harness.get_by_label("Collapse Group").click();
    harness.run();
    harness.state_mut().selection = vec![1];
    harness.run();
    key(&mut harness, Key::F2, Modifiers::NONE);
    harness.get_by_label("Expand Group").click();
    harness.run();
    assert!(harness.query_by_label("Layer name").is_none(), "no field reopens over the member later");
    assert_eq!(name(&harness, 1), "Square");
}
