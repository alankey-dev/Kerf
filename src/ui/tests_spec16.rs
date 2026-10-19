//! Spec 16: the Make flow wears the canvas identity.

use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, assert_flow_snapshot, device_is, flow_frame, pump, snapshot_harness, step_is, ticking_harness};
use super::tokens::{ACCENT_HOVER, ACCENT_PRIMARY, BG_PANEL};
use crate::device::DeviceState;
use crate::make_flow::{ConnectPhase, DonePhase, MakeStep, Outcome};
use crate::material::PressureAdjust;

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

fn start_make(harness: &mut Harness<'_, Kerf>) {
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Heart").click();
    harness.run();
    harness.get_by_label("Make").click();
    harness.run_steps(4);
}

/// From Prepare through the machine modal to Set, with Cardstock picked and the pressure set to More.
fn to_set(harness: &mut Fixture, snapshots: bool) {
    harness.get_by_label("Continue").click();
    pump(harness, "device list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    if snapshots {
        assert_flow_snapshot(harness, "16-machine");
    }
    harness.get_all_by_label("Connect with USB").next().unwrap().click();
    pump(harness, "connected", |a| step_is(a, MakeStep::ConnectMachine(ConnectPhase::Connected)));
    if snapshots {
        assert!(harness.state().flow.as_ref().is_some_and(|f| f.machine.is_some()), "a machine is connected");
        assert_flow_snapshot(harness, "16-machine-connected");
    }
    harness.get_all_by_label("Continue").last().unwrap().click();
    pump(harness, "Set step", |a| step_is(a, MakeStep::Set) && device_is(a, DeviceState::WaitingOnMaterial));

    harness.get_by_label("Browse All Materials").click();
    harness.step();
    harness.get_by_label("Cardstock  (Fine-Point Blade, Clamp B)").click();
    harness.step();
    harness.get_by_label("More").click();
    harness.run_steps(2);
}

fn pixel(image: &image::RgbaImage, at: egui::Pos2) -> egui::Color32 {
    let [r, g, b, a] = image.get_pixel(at.x as u32, at.y as u32).0;
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// A point inside the widget labelled `label`, `inset` px in from its left edge and clear of its text.
fn inside(harness: &Harness<'_, Kerf>, label: &str, inset: f64) -> egui::Pos2 {
    let r = harness.get_by_label(label).bounding_box().unwrap();
    egui::pos2((r.x0 + inset) as f32, ((r.y0 + r.y1) / 2.0) as f32)
}

#[test]
fn prepare_footer_pairs_an_accent_continue_with_a_white_cancel() {
    let mut harness = snapshot_harness(SIZE);
    start_make(&mut harness);
    let image = flow_frame(&mut harness);
    assert_eq!(pixel(&image, inside(&harness, "Continue", 8.0)), ACCENT_PRIMARY, "Continue is filled with the accent");
    assert_eq!(pixel(&image, inside(&harness, "Cancel", 8.0)), BG_PANEL, "Cancel is white");
}

#[test]
fn the_selected_pressure_segment_takes_the_accent_tint_not_egui_blue() {
    let mut harness = snapshot_harness(SIZE);
    start_make(&mut harness);
    to_set(&mut harness, false);
    assert_eq!(harness.state().flow.as_ref().map(|f| f.pressure), Some(PressureAdjust::More));
    let image = flow_frame(&mut harness);
    let more = pixel(&image, inside(&harness, "More", 4.0));
    assert_eq!(more, ACCENT_HOVER);
    assert_ne!(more, egui::Visuals::light().selection.bg_fill);
    assert_ne!(more, ACCENT_PRIMARY);
    assert_eq!(pixel(&image, inside(&harness, "Less", 4.0)), BG_PANEL, "an unselected segment stays white");
}

/// Colours come from `tokens` and widgets from `flow_kit`, never egui's defaults.
#[test]
fn flow_screens_paint_only_tokens_and_flow_kit_widgets() {
    const FILES: [(&str, &str); 4] = [
        ("prepare.rs", include_str!("prepare.rs")),
        ("set_load_go.rs", include_str!("set_load_go.rs")),
        ("machine_modal.rs", include_str!("machine_modal.rs")),
        ("flow_kit.rs", include_str!("flow_kit.rs")),
    ];
    const BANNED: [&str; 18] = [
        "Color32::from",
        "Color32::WHITE",
        "Color32::BLACK",
        "Color32::GRAY",
        "Color32::LIGHT",
        "Color32::DARK",
        "0x",
        "Button::new",
        "ComboBox::",
        "ui.button(",
        "ui.radio(",
        "selectable_label",
        "selectable_value",
        "toggle_value",
        "ui.group(",
        "CollapsingHeader::",
        "ProgressBar",
        "ui.collapsing(",
    ];
    let mut found = Vec::new();
    for (name, source) in FILES {
        let code = source.split("#[cfg(test)]").next().unwrap();
        found.extend(BANNED.iter().filter(|b| code.contains(*b)).map(|b| format!("{name}: {b}")));
    }
    assert!(found.is_empty(), "default widgets or raw colours in the flow: {found:?}");
}

#[test]
fn walks_every_make_screen_for_snapshots() {
    let mut harness = ticking_harness(SIZE, Duration::from_millis(15));
    start_make(&mut harness);
    assert!(step_is(harness.state(), MakeStep::Prepare));
    assert_eq!(harness.state().flow.as_ref().map(|f| f.mats.len()), Some(1), "one heart, one mat");
    let image = assert_flow_snapshot(&mut harness, "16-prepare");
    assert_eq!(pixel(&image, inside(&harness, "Continue", 8.0)), ACCENT_PRIMARY);
    harness.get_by_role_and_label(Role::ComboBox, "Material Size").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("12 in x 24 in").is_some(), "the size list is open");
    assert_flow_snapshot(&mut harness, "16-prepare-size");
    harness.get_by_role_and_label(Role::ComboBox, "Material Size").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("12 in x 24 in").is_none(), "the size list closes");
    to_set(&mut harness, true);
    let flow = harness.state().flow.as_ref().unwrap();
    assert_eq!((flow.material.map(|m| m.name), flow.pressure), (Some("Cardstock"), PressureAdjust::More));
    let image = assert_flow_snapshot(&mut harness, "16-set");
    assert_eq!(pixel(&image, inside(&harness, "More", 4.0)), ACCENT_HOVER, "the chosen pressure is tinted");
    harness.get_by_label("Next").click();
    pump(&mut harness, "waiting on load", |a| device_is(a, DeviceState::WaitingOnLoad));
    assert!(step_is(harness.state(), MakeStep::Load));
    assert_flow_snapshot(&mut harness, "16-load");
    harness.get_by_label("Load/Unload").click();
    pump(&mut harness, "Go step", |a| step_is(a, MakeStep::Go) && device_is(a, DeviceState::ReadyToGo));
    assert!(harness.state().flow.as_ref().unwrap().progress.is_none(), "nothing is cut before Go");
    assert_flow_snapshot(&mut harness, "16-go");

    harness.get_by_label("Go").click();
    pump(&mut harness, "mid cut", |a| a.flow.as_ref().and_then(|f| f.progress.as_ref()).is_some_and(|p| p.current_percentage >= 40.0));
    harness.get_by_label("Pause").click();
    pump(&mut harness, "paused", |a| device_is(a, DeviceState::Paused));
    let paused_at = harness.state().flow.as_ref().and_then(|f| f.progress.as_ref()).map(|p| p.current_percentage).unwrap();
    assert!((40.0..100.0).contains(&paused_at), "paused mid cut at {paused_at}%");
    harness.get_by_label("Resume").click();
    let checking = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Checking };
    pump(&mut harness, "cut complete", |a| step_is(a, checking));
    let done_at = harness.state().flow.as_ref().and_then(|f| f.progress.as_ref()).map(|p| p.current_percentage);
    assert_eq!(done_at, Some(100.0), "the cut ran to the end");
    assert_flow_snapshot(&mut harness, "16-done");
    harness.get_by_label("Looks good").click();
    let unloading = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloading };
    pump(&mut harness, "unloading", |a| step_is(a, unloading) && device_is(a, DeviceState::WaitingOnUnload));
    harness.get_by_label("Unload mat").click();
    pump(&mut harness, "unloaded", |a| step_is(a, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloaded }));
    assert!(device_is(harness.state(), DeviceState::Unloaded), "the mat is out");
    assert_flow_snapshot(&mut harness, "16-unloaded");
}
