//! Spec 56: the swatch picker's hex field, project colours, recent colours and saved swatches, driven
//! through the real app.

use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, snapshot_harness};
use crate::design::{Design, Operation, ShapeKind};
use crate::palettes::project_colours;
use crate::pens::{Pen, PenSize};
use crate::settings::Swatch;

const RED: Color32 = Color32::from_rgb(0xD0, 0x20, 0x2E);
const BLUE: Color32 = Color32::from_rgb(0x1F, 0x5F, 0xBF);
const VIOLET: Color32 = Color32::from_rgb(0x70, 0x48, 0xE8);

/// A row of 1 inch squares in the given fills, snapping off, with nothing selected. Returns their ids.
fn squares(harness: &mut Harness<'_, Kerf>, fills: &[Color32]) -> Vec<u64> {
    let mut design = Design::default();
    let mut ids = Vec::new();
    for (i, &fill) in fills.iter().enumerate() {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (1.0 + 1.5 * i as f32, 1.0, 1.0, 1.0);
        s.fill = fill;
        ids.push(id);
    }
    design.set_snapping(false);
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.run();
    ids
}

/// Clicks the inspector's colour swatch, opening the picker popover.
fn open_picker(harness: &mut Harness<'_, Kerf>) {
    harness.get_by_role_and_label(Role::Button, "Colour").click();
    harness.run();
}

#[test]
fn typing_a_hex_colour_sets_the_shape_and_leads_recent() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let ids = squares(&mut harness, &[RED]);
    harness.state_mut().selection = vec![ids[0]];
    harness.run();

    open_picker(&mut harness);
    harness.get_by_role_and_label(Role::TextInput, "Hex colour").focus();
    harness.run();
    harness.press_key_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "Hex colour").type_text("#7048E8");
    harness.run();
    harness.press_key(Key::Enter);
    harness.run();

    assert_eq!(harness.state().design.shapes[0].fill, VIOLET, "the selected shape is violet");
    assert_eq!(harness.state().recent_colours.iter().next(), Some(VIOLET), "the chosen colour leads Recent");
    harness.get_by_label(format!("Recent {}", crate::palettes::hex_string(VIOLET)).as_str());
}

#[test]
fn clicking_a_project_colour_recolours_without_a_new_mat() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let ids = squares(&mut harness, &[RED, BLUE]);
    harness.state_mut().selection = vec![ids[0]];
    harness.run();

    let before = project_colours(&harness.state().design).len();
    harness.get_by_role_and_label(Role::Button, "Colour").click();
    harness.run();
    harness.get_by_label("Blue, 1 shape").click();
    harness.run();

    assert_eq!(harness.state().design.shapes[0].fill, BLUE, "the red shape takes the project's blue");
    let after = project_colours(&harness.state().design).len();
    assert!(after <= before, "picking a colour the design already uses makes no new mat ({before} -> {after})");
}

#[test]
fn the_picker_shows_its_sections() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let ids = squares(&mut harness, &[RED, BLUE]);
    // A pen layer so Project colours lists a pen apart from the fills.
    let pen = harness.state_mut().design.add_shape(ShapeKind::Square);
    let s = harness.state_mut().design.shape_mut(pen).unwrap();
    (s.x_in, s.y_in) = (4.0, 1.0);
    s.set_operation(Operation::Pen);
    s.set_pen(Pen::named("Green", PenSize::Fine));
    harness.state_mut().swatches =
        vec![Swatch { name: "Navy".into(), colour: Color32::from_rgb(0x1C, 0x2E, 0x6B) }, Swatch { name: "Gold".into(), colour: Color32::from_rgb(0xC9, 0xA2, 0x3A) }];
    harness.state_mut().recent_colours.record(VIOLET);
    harness.state_mut().selection = vec![ids[0]];
    harness.run();

    open_picker(&mut harness);
    assert_snapshot(&mut harness, "56-picker");
}

#[test]
fn replace_colour_swaps_every_red_for_navy_in_one_undo_step() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let ids = squares(&mut harness, &[RED, BLUE, RED, RED]);
    let reds = [ids[0], ids[2], ids[3]];

    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Replace colour...").click();
    harness.run();
    assert_snapshot(&mut harness, "56-replace-colour");

    harness.get_by_label("Red, 3 shapes").click();
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "New colour").focus();
    harness.run();
    harness.press_key_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "New colour").type_text("navy");
    harness.run();
    harness.get_by_label("Replace").click();
    harness.run();

    let navy = Color32::from_rgb(0x00, 0x00, 0x80);
    for id in reds {
        assert_eq!(fill_of(&harness, id), navy, "every red shape is navy");
    }
    assert_eq!(fill_of(&harness, ids[1]), BLUE, "the blue shape is untouched");

    harness.press_key_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    for id in reds {
        assert_eq!(fill_of(&harness, id), RED, "one undo restores every red");
    }
}

fn fill_of(harness: &Harness<'_, Kerf>, id: u64) -> Color32 {
    harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().fill
}
