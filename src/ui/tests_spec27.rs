//! Spec 27: Print Then Cut. Two stickers print on one page, saved as a PDF from the
//! Print step, then the page and the cut square each run Set, Load and Go.

use egui::Color32;
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_flow_snapshot, device_is, pump, snapshot_harness, step_is};
use crate::design::{Design, Operation, ShapeKind};
use crate::device::DeviceState;
use crate::make_flow::{ConnectPhase, DonePhase, MakeStep, Outcome};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// A red heart sticker and a blue star sticker, then a black cut square.
fn stickers_and_a_square(fixture: &mut Fixture) -> tempfile::TempDir {
    let mut d = Design::default();
    for (kind, fill, x) in [(ShapeKind::Heart, Color32::RED, 1.0), (ShapeKind::Star, Color32::BLUE, 4.0)] {
        let id = d.add_shape(kind);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.fill) = (x, 1.0, fill);
        d.expanded_sticker(&[id], 0.125, crate::effects::Corners::Round).expect("makes a sticker");
    }
    let square = d.add_shape(ShapeKind::Square);
    let s = d.shape_mut(square).unwrap();
    (s.x_in, s.y_in, s.fill, s.operation) = (7.0, 1.0, Color32::BLACK, Operation::BasicCut);
    let out = tempfile::tempdir().unwrap();
    let app = fixture.state_mut();
    app.design = d;
    app.export_dir = Some(out.path().to_owned());
    fixture.run();
    out
}

fn mats(fixture: &Fixture) -> Vec<(bool, usize)> {
    fixture.state().flow.as_ref().unwrap().mats.iter().map(|m| (m.page.is_some(), m.items.len())).collect()
}

/// Set, Load, Go and Done for the current mat, ending unloaded.
fn make_the_mat(fixture: &mut Fixture) {
    pump(fixture, "waiting on material", |a| step_is(a, MakeStep::Set) && device_is(a, DeviceState::WaitingOnMaterial));
    fixture.get_by_label("Next").click();
    pump(fixture, "waiting on load", |a| device_is(a, DeviceState::WaitingOnLoad));
    fixture.get_by_label("Load/Unload").click();
    pump(fixture, "Go step", |a| step_is(a, MakeStep::Go));
    fixture.get_by_label("Go").click();
    pump(fixture, "cut complete", |a| step_is(a, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Checking }));
    fixture.get_by_label("Looks good").click();
    let unloading = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloading };
    pump(fixture, "unloading", |a| step_is(a, unloading) && device_is(a, DeviceState::WaitingOnUnload));
    fixture.get_by_label("Unload mat").click();
    pump(fixture, "unloaded", |a| step_is(a, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloaded }));
}

#[test]
fn an_image_larger_than_the_print_area_blocks_continue_and_shows_the_warning() {
    let mut fixture = snapshot_harness(SIZE);
    let mut d = Design::default();
    let id = d.add_shape(ShapeKind::Square);
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in, s.fill) = (0.5, 0.5, 10.0, 10.0, Color32::RED);
    d.expanded_sticker(&[id], 0.125, crate::effects::Corners::Round).expect("makes a sticker");
    fixture.state_mut().design = d;
    fixture.run();
    fixture.get_by_label("Make").click();
    fixture.run_steps(4);
    assert!(step_is(fixture.state(), MakeStep::Prepare));
    fixture.get_by_label_contains("Image is too large for");
    assert!(fixture.get_by_label("Continue").is_disabled(), "Continue is blocked");
    fixture.get_by_label("Continue").click();
    fixture.run_steps(2);
    assert!(step_is(fixture.state(), MakeStep::Prepare), "a click on the blocked button goes nowhere");
}

#[test]
fn two_stickers_print_on_one_page_then_the_cut_square_runs_without_print() {
    let mut fixture = snapshot_harness(SIZE);
    let out = stickers_and_a_square(&mut fixture);
    fixture.get_by_label("Make").click();
    fixture.run_steps(4);
    assert_eq!(mats(&fixture), [(true, 2), (false, 1)], "both stickers share the white page, whatever their colour");
    assert_eq!(fixture.get_all_by_label("Paper Size").count(), 2, "the Paper Size header and its dropdown");
    assert_flow_snapshot(&mut fixture, "27-prepare-print-page");

    fixture.get_by_label("Continue").click();
    fixture.run_steps(2);
    assert!(step_is(fixture.state(), MakeStep::Print), "the page opens on Print");
    fixture.get_by_label("Add Bleed");
    assert_flow_snapshot(&mut fixture, "27-print-step");

    fixture.get_by_label("Save print sheet (PDF)").click();
    fixture.run_steps(2);
    let pdf = std::fs::read(out.path().join("Untitled-1 - Print 1.pdf")).expect("the print sheet is written");
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(pdf.contains("/MediaBox [0 0 612 792]"), "a Letter page");
    assert!(step_is(fixture.state(), MakeStep::Print), "saving stays on Print");

    fixture.get_by_label("Next").click();
    pump(&mut fixture, "device list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    fixture.get_all_by_label("Connect with USB").next().unwrap().click();
    pump(&mut fixture, "connected", |a| step_is(a, MakeStep::ConnectMachine(ConnectPhase::Connected)));
    fixture.get_all_by_label("Continue").last().unwrap().click();
    pump(&mut fixture, "Set step", |a| step_is(a, MakeStep::Set));
    fixture.get_by_label("Browse All Materials").click();
    fixture.step();
    fixture.get_by_label("Cardstock  (Fine-Point Blade, Clamp B)").click();
    make_the_mat(&mut fixture);

    fixture.get_by_label("Next Mat").click();
    fixture.step();
    assert!(step_is(fixture.state(), MakeStep::Set), "the cut mat goes straight to Set, with no Print step");
    assert_eq!(fixture.state().flow.as_ref().unwrap().current, 1);
    make_the_mat(&mut fixture);
    fixture.get_by_label("Back to canvas");
}
