//! Spec 42: the Monogram Maker, driven through the app.

use egui::Modifiers;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, double_click_at, key, move_to, right_click_at, save_snapshot, snapshot_harness, two_squares_apart};
use crate::design::{Pt, Shape, ShapeKind};
use crate::edit::{AppCommand, Blocked};
use crate::monogram::{Adjust, Frame, MonogramSpec, Style};

type App<'a> = Harness<'a, Kerf>;

/// An empty canvas and a map from inches to screen points.
fn empty_canvas() -> (Fixture, impl Fn(Pt) -> egui::Pos2) {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = two_squares_apart(&mut fixture.harness);
    let state = fixture.harness.state_mut();
    state.design = Default::default();
    state.tool = super::canvas::Tool::Select;
    fixture.harness.run();
    (fixture, to_screen)
}

fn group(harness: &App<'_>) -> Shape {
    let groups: Vec<&Shape> = harness.state().design.shapes.iter().filter(|s| s.monogram.is_some()).collect();
    let [g] = groups[..] else { panic!("one monogram group, found {}", groups.len()) };
    g.clone()
}

fn children(harness: &App<'_>, id: u64) -> Vec<Shape> {
    harness.state().design.children(Some(id)).cloned().collect()
}

/// A monogram inserted the way Insert inserts it, with nothing to undo before it.
fn with_monogram(spec: &MonogramSpec) -> (Fixture, impl Fn(Pt) -> egui::Pos2, u64) {
    let (mut fixture, to_screen) = empty_canvas();
    let state = fixture.harness.state_mut();
    let id = state.design.insert_monogram(spec, Some((5.0, 4.0)));
    state.history = Default::default();
    fixture.harness.run();
    (fixture, to_screen, id)
}

/// A point on the frame's band, just inside the group's left edge, on screen.
fn on_the_band(g: &Shape, to_screen: &impl Fn(Pt) -> egui::Pos2) -> egui::Pos2 {
    to_screen(g.to_world((g.monogram.as_ref().unwrap().adjust.thickness_in / 2.0, g.height_in / 2.0)))
}

fn edit_monogram_blocked(harness: &App<'_>) -> Option<Blocked> {
    let state = harness.state();
    AppCommand::EditMonogram.check(&state.design, &state.history, &state.selection).err()
}

#[test]
fn rail_to_insert_makes_one_group_and_one_undo_removes_it() {
    let (mut fixture, _) = empty_canvas();
    let harness = &mut fixture.harness;
    harness.get_by_label("Monogram").click();
    harness.run();
    assert!(harness.state().monogram.is_some(), "the rail opens the panel");
    assert!(!harness.state().shapes_open, "in the Shapes panel's slot");
    harness.get_by_role_and_label(Role::TextInput, "Letters").type_text("ABC");
    harness.run();
    harness.get_by_label("Botanical theme").click();
    harness.run();
    assert_eq!(harness.state().monogram.as_ref().unwrap().spec.theme.as_deref(), Some("Botanical"));
    save_snapshot(harness, "42-panel.png");
    assert!(harness.state().design.shapes.is_empty(), "the preview leaves the design alone");

    harness.get_by_label("Insert").click();
    harness.run();
    let g = group(harness);
    assert_eq!(g.name.as_deref(), Some("Monogram ABC"));
    assert!(matches!(g.kind, ShapeKind::Compound(_)));
    let kids = children(harness, g.id);
    assert_eq!(kids.len(), 4, "a frame and three letters");
    assert!(!matches!(kids[0].kind, ShapeKind::Text { .. }), "the frame at the back");
    assert_eq!(kids[1..].iter().filter(|k| matches!(k.kind, ShapeKind::Text { .. })).count(), 3);
    assert_eq!(harness.state().selection, [g.id], "the monogram is selected");
    let view = harness.state().view.centre_in();
    assert!((g.centre_in().0 - view.0).abs() < 1e-3 && (g.centre_in().1 - view.1).abs() < 1e-3, "at the centre of the view");
    assert!(harness.state().monogram.is_none(), "Insert closes the panel");

    key(harness, egui::Key::Z, Modifiers::COMMAND);
    assert!(harness.state().design.shapes.is_empty(), "one Undo removes it");
}

#[test]
fn insert_waits_for_a_letter() {
    let (mut fixture, _) = empty_canvas();
    let harness = &mut fixture.harness;
    harness.get_by_label("Monogram").click();
    harness.run();
    harness.get_by_label("Insert").click();
    harness.run();
    assert!(harness.state().design.shapes.is_empty());
    assert!(harness.state().monogram.is_some(), "still open");
    assert_eq!(Blocked::NoLetters.reason(), "Type at least one letter.");
}

#[test]
fn double_click_then_update_to_circle_keeps_the_group_and_one_undo_restores_it() {
    let spec = MonogramSpec { letters: "ABC".into(), ..MonogramSpec::default() };
    let (mut fixture, to_screen, id) = with_monogram(&spec);
    let harness = &mut fixture.harness;
    let before = harness.state().design.clone();
    harness.state_mut().design.shape_mut(id).unwrap().rotation_deg = 15.0;
    harness.state_mut().design.settle(Some(&before));
    harness.state_mut().history = Default::default();
    harness.run();
    let was = group(harness);
    let old_children: Vec<ShapeKind> = children(harness, id).into_iter().map(|s| s.kind).collect();

    double_click_at(harness, on_the_band(&was, &to_screen));
    let panel = harness.state().monogram.as_ref().expect("double-click opens the panel");
    assert_eq!(panel.editing, Some(id));
    assert_eq!(panel.spec, spec);
    harness.get_by_label("Circle style").click();
    harness.run();
    harness.get_by_label("Update").click();
    harness.run();

    let now = group(harness);
    assert_eq!(now.id, id, "the same group");
    assert_eq!(now.rotation_deg, 15.0);
    assert!((now.centre_in().0 - was.centre_in().0).abs() < 1e-3 && (now.centre_in().1 - was.centre_in().1).abs() < 1e-3, "in the same place");
    assert_eq!(now.monogram.as_ref().unwrap().style, Style::Circle);
    assert_eq!(children(harness, id).len(), 3, "frame, arc and centre letter");
    assert!(harness.state().monogram.is_none(), "Update closes the panel");

    key(harness, egui::Key::Z, Modifiers::COMMAND);
    assert_eq!(group(harness).monogram.as_ref().unwrap().style, Style::ClassicThree);
    assert_eq!(children(harness, id).into_iter().map(|s| s.kind).collect::<Vec<_>>(), old_children, "one Undo restores the old children");
}

#[test]
fn edit_monogram_is_in_the_context_menu_until_the_monogram_is_ungrouped() {
    let spec = MonogramSpec { letters: "ABC".into(), ..MonogramSpec::default() };
    let (mut fixture, to_screen, id) = with_monogram(&spec);
    let harness = &mut fixture.harness;
    let g = group(harness);
    right_click_at(harness, on_the_band(&g, &to_screen));
    assert_eq!(harness.state().selection, [id]);
    assert!(!harness.get_all_by_label("Edit monogram").last().unwrap().is_disabled(), "enabled on a monogram");
    harness.get_all_by_label("Edit monogram").last().unwrap().click();
    harness.run();
    assert_eq!(harness.state().monogram.as_ref().and_then(|p| p.editing), Some(id), "the menu opens the panel on it");
    harness.get_by_label("Close Monogram").click();
    harness.run();

    key(harness, egui::Key::G, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    assert!(harness.state().design.shapes.iter().all(|s| s.monogram.is_none() && s.parent.is_none()), "Ungroup frees the frame and letters");
    assert_eq!(harness.state().selection.len(), 4, "and selects them");
    assert_eq!(edit_monogram_blocked(harness), Some(Blocked::NotMonogram));
    right_click_at(harness, to_screen(harness.state().design.shapes[0].centre_in()));
    let row = harness.get_all_by_label("Edit monogram").last().unwrap();
    assert!(row.is_disabled(), "disabled once ungrouped");
}

#[test]
fn a_child_edited_by_hand_blocks_edit_monogram_with_its_reason() {
    let spec = MonogramSpec { letters: "ABC".into(), ..MonogramSpec::default() };
    let (mut fixture, _, id) = with_monogram(&spec);
    let harness = &mut fixture.harness;
    harness.state_mut().selection = vec![id];
    assert_eq!(edit_monogram_blocked(harness), None);
    let letter = children(harness, id)[2].id;
    let before = harness.state().design.clone();
    harness.state_mut().design.shape_mut(letter).unwrap().y_in += 0.4;
    harness.state_mut().design.settle(Some(&before));
    harness.run();
    assert_eq!(edit_monogram_blocked(harness), Some(Blocked::MonogramEdited));
    assert_eq!(Blocked::MonogramEdited.reason(), "This group was edited by hand.");
}

#[test]
fn every_style_in_the_circle_frame() {
    let (mut fixture, _) = empty_canvas();
    let harness = &mut fixture.harness;
    let adjust = Adjust { centre_in: 1.15, ..Adjust::default() };
    for (i, style) in Style::ALL.into_iter().enumerate() {
        let spec = MonogramSpec { letters: "KRF".into(), style, frame: Frame::Circle, adjust, ..MonogramSpec::default() };
        let at = (2.2 + (i % 3) as f32 * 3.8, 2.8 + (i / 3) as f32 * 5.0);
        harness.state_mut().design.insert_monogram(&spec, Some(at));
    }
    harness.state_mut().selection.clear();
    move_to(harness, egui::pos2(700.0, 880.0));
    save_snapshot(harness, "42-styles.png");
    assert_eq!(harness.state().design.shapes.iter().filter(|s| s.monogram.is_some()).count(), 6);
}
