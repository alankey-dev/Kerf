//! Spec 05: the Effects menu, Sticker and Warp.

use egui::Color32;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, assert_snapshot, launch, snapshot_harness};
use crate::design::ShapeKind;
use crate::warp::{Warp, WarpStyle};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// A selected 2 x 2 square, filled `fill`, in the open part of the mat.
fn square(harness: &mut Harness<'_, Kerf>, fill: Color32) -> u64 {
    let id = harness.state_mut().design.add_shape(ShapeKind::Square);
    let s = harness.state_mut().design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in, s.fill) = (3.0, 3.0, 2.0, 2.0, fill);
    harness.state_mut().selection = vec![id];
    harness.run();
    id
}

fn open(harness: &mut Harness<'_, Kerf>, effect: &str) {
    harness.get_by_label("Effects").click();
    harness.run();
    harness.get_by_label(effect).click();
    harness.run();
}

/// Types into the Bend number field, which shares its name with the visible "Bend" caption.
fn type_bend(harness: &mut Harness<'_, Kerf>, text: &str) {
    harness.get_by_role_and_label(Role::TextInput, "Bend").focus();
    harness.run();
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "Bend").type_text(text);
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
}

fn warp_of(harness: &Harness<'_, Kerf>, id: u64) -> Option<Warp> {
    harness.state().design.shapes.iter().find(|s| s.id == id).and_then(|s| s.effects.warp())
}

#[test]
fn effects_opens_a_menu_of_offset_sticker_and_warp() {
    let mut harness = snapshot_harness(SIZE);
    square(&mut harness, Color32::BLACK);
    harness.get_by_label("Effects").click();
    harness.run();
    for label in ["Offset", "Sticker", "Warp"] {
        harness.get_by_label(label);
    }
    assert!(harness.query_by_label("Offset distance").is_none(), "the menu comes before any form");

    harness.get_by_label("Warp").click();
    harness.run();
    harness.get_by_label("Back").click();
    harness.run();
    harness.get_by_label("Sticker");
}

#[test]
fn sticker_backs_the_square_in_white() {
    let mut harness = snapshot_harness(SIZE);
    let backdrop = harness.state_mut().design.add_shape(ShapeKind::Rectangle);
    let b = harness.state_mut().design.shape_mut(backdrop).unwrap();
    (b.x_in, b.y_in, b.width_in, b.height_in, b.fill) = (2.0, 3.5, 6.0, 1.0, Color32::from_rgb(0x40, 0x60, 0xd0));
    let id = square(&mut harness, Color32::from_rgb(0xe0, 0x40, 0x40));
    harness.state_mut().selection = vec![id];
    harness.run();
    open(&mut harness, "Sticker");
    harness.get_by_label("Sticker border");
    harness.get_by_label("Apply").click();
    harness.run();
    harness.get_by_label("Done").click();
    harness.run();
    let design = &harness.state().design;
    assert_eq!(design.shapes.len(), 2, "the backdrop stays, the square keeps its place under a live sticker");
    let square = design.shapes.last().unwrap();
    let geometry = design.geometry(square);
    let backing = geometry.pieces()[0].bounds;
    assert_eq!((square.id, geometry.pieces().len()), (id, 1), "one Print Then Cut piece");
    let crate::effects::Paint::Print(layers) = &geometry.pieces()[0].paint else { panic!("a sticker prints") };
    assert_eq!(layers[0].1, crate::effects::Ink::Colour(Color32::WHITE), "a white backing");
    assert!((backing.max_x - backing.min_x - 2.25).abs() < 1e-3, "the default border goes round the square: {backing:?}");
    assert_eq!(harness.state().selection, [id], "the square stays selected");
    harness.state_mut().selection.clear();

    let state = &mut harness.state_mut().doc;
    state.selection = vec![id];
    crate::edit::run(&mut state.design, &mut state.selection, &crate::edit::Edit::Expand, crate::edit::Subject::Selection).unwrap();
    harness.run();
    assert!(matches!(harness.state().design.shapes.last().unwrap().kind, ShapeKind::Flattened { .. }), "Expand bakes the sticker as before");
    open(&mut harness, "Warp");
    harness.get_by_label("Unflatten this shape to warp its parts.");
    assert!(harness.query_by_label("Arch").is_none(), "a flattened shape offers no warp styles");
}

#[test]
fn an_arch_survives_a_save_and_reload_and_stays_editable() {
    let mut harness = snapshot_harness(SIZE);
    let id = square(&mut harness, Color32::BLACK);
    open(&mut harness, "Warp");
    harness.get_by_label("Rise").click();
    harness.run();
    assert_eq!(warp_of(&harness, id).map(|w| w.style), Some(WarpStyle::Rise), "a tile applies live");
    harness.get_by_label("Arch").click();
    harness.run();
    type_bend(&mut harness, "50");
    harness.get_by_label("Apply").click();
    harness.run();
    let arch = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };
    assert_eq!(warp_of(&harness, id), Some(arch));
    assert!(harness.query_by_label("Bend").is_none(), "Apply closes the popover");
    assert_snapshot(&mut harness, "05-warp-arch");

    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    let Fixture { harness, root, .. } = harness;
    drop(harness);

    let mut harness = launch(root, SIZE);
    harness.get_by_label("Edit").click();
    harness.run();
    assert_eq!(warp_of(&harness, id), Some(arch), "the warp came back from disk");
    harness.state_mut().selection = vec![id];
    harness.run();
    open(&mut harness, "Warp");
    harness.get_by_label("Remove Warp");
    type_bend(&mut harness, "-30");
    assert_eq!(warp_of(&harness, id), Some(Warp { bend: -0.3, ..arch }), "the bend edits live");
}
