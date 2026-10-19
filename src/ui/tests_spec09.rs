//! Spec 09: mats composed by colour, driven through the real app.

use egui::Color32;
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_snapshot, device_is, pump, snapshot_harness, step_is};
use crate::design::{Design, Operation, ShapeKind};
use crate::device::DeviceState;
use crate::make_flow::{DonePhase, MakeStep, Outcome};
use crate::pens::{Pen, PenSize};

const RED: Color32 = Color32::from_rgb(0xd0, 0x20, 0x20);

fn square(design: &mut Design, fill: Color32, op: Operation, at: (f32, f32)) -> u64 {
    let id = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(id).unwrap();
    s.fill = fill;
    s.set_operation(op);
    (s.x_in, s.y_in) = at;
    id
}

/// Set, Load, Go, Looks good and Unload for the current mat, ending on the Unloaded screen. Returns the
/// path data sent for it.
fn cut_current_mat(harness: &mut Fixture) -> String {
    pump(harness, "waiting on material", |a| step_is(a, MakeStep::Set) && device_is(a, DeviceState::WaitingOnMaterial));
    harness.get_by_label("Next").click();
    pump(harness, "waiting on load", |a| device_is(a, DeviceState::WaitingOnLoad));
    let paths = harness.state().last_sent.as_ref().map(|l| l.json.clone()).unwrap_or_default();
    assert!(paths.contains("riMATCUTSetPathData"), "{paths}");
    harness.get_by_label("Load/Unload").click();
    pump(harness, "Go step", |a| step_is(a, MakeStep::Go));
    harness.get_by_label("Go").click();
    pump(harness, "cut complete", |a| step_is(a, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Checking }));
    harness.get_by_label("Looks good").click();
    let unloading = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloading };
    pump(harness, "unloading", |a| step_is(a, unloading) && device_is(a, DeviceState::WaitingOnUnload));
    harness.get_by_label("Unload mat").click();
    pump(harness, "unloaded", |a| step_is(a, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloaded }));
    paths
}

#[test]
fn make_cuts_a_two_colour_design_as_two_loads() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    square(&mut harness.state_mut().design, Color32::BLACK, Operation::BasicCut, (1.0, 1.0));
    square(&mut harness.state_mut().design, RED, Operation::BasicCut, (6.0, 6.0));
    harness.run();
    harness.get_by_label("Make").click();
    harness.run_steps(4);
    assert_eq!(harness.state().flow.as_ref().unwrap().mats.len(), 2);
    harness.get_by_label("Continue").click();
    pump(&mut harness, "device list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    harness.get_all_by_label("Connect with USB").next().expect("My Machine USB button").click();
    pump(&mut harness, "connected", |a| step_is(a, MakeStep::ConnectMachine(crate::make_flow::ConnectPhase::Connected)));
    harness.get_all_by_label("Continue").last().expect("modal Continue").click();
    pump(&mut harness, "Set step", |a| step_is(a, MakeStep::Set));
    harness.get_by_label("Mat 1 of 2");
    harness.get_by_label("Browse All Materials").click();
    harness.step();
    harness.get_by_label("Cardstock  (Fine-Point Blade, Clamp B)").click();

    let image = |id: u64| format!("\"imageId\": \"{id}\"");
    let [black, red] = [0, 1].map(|n| harness.state().design.shapes[n].id);
    let first = cut_current_mat(&mut harness);
    assert!(first.contains(&image(black)) && !first.contains(&image(red)), "the first load sends only the black square: {first}");
    assert!(harness.query_by_label("Back to canvas").is_none(), "a mat is still to cut");
    harness.get_by_label("Next Mat").click();
    pump(&mut harness, "second mat", |a| a.flow.as_ref().is_some_and(|f| f.current == 1));
    harness.get_by_label("Mat 2 of 2");

    let second = cut_current_mat(&mut harness);
    assert!(second.contains(&image(red)) && !second.contains(&image(black)), "the second load sends only the red square: {second}");
    assert!(harness.query_by_label("Next Mat").is_none());
    harness.get_by_label("Back to canvas").click();
    harness.step();
    assert!(harness.state().flow.is_none(), "the flow finishes after the last mat");
}

#[test]
fn preview_shows_a_coloured_mat_with_a_joined_operation_label() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let design = &mut harness.state_mut().design;
    let cut = square(design, Color32::BLACK, Operation::BasicCut, (1.0, 1.0));
    let score = design.add_shape(ShapeKind::Line);
    let s = design.shape_mut(score).unwrap();
    (s.x_in, s.y_in, s.operation) = (1.5, 2.0, Operation::Score);
    let star = design.add_shape(ShapeKind::Star);
    let s = design.shape_mut(star).unwrap();
    s.set_operation(Operation::Pen);
    s.set_pen(Pen::named("Wine", PenSize::Fine));
    (s.x_in, s.y_in, s.width_in, s.height_in) = (1.5, 1.2, 1.0, 1.0);
    design.attach(&[cut, score, star]);
    square(design, RED, Operation::BasicCut, (6.0, 6.0));
    square(design, Color32::WHITE, Operation::WavyCut, (8.0, 1.0));
    harness.run();

    harness.get_by_label("Preview").click();
    harness.run();
    for line in ["Load 1", "Score, Pen, Basic Cut", "Load 2", "Load 3", "Wavy Cut"] {
        harness.get_by_label(line);
    }
    assert_snapshot(&mut harness, "09-preview");

    harness.get_by_label("Mat Details").click();
    harness.run();
    assert!(harness.query_by_label("Load 1").is_none(), "the header collapses the cards");
    harness.get_by_label("Mat Details").click();
    harness.run();
    harness.get_by_label("Load 1");
}
