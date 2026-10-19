//! Spec 35: a machine that dies or goes missing shows an error in the flow instead of waiting forever.

use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{machine_harness, pump, step_is};
use crate::device::{DeviceState, SimulatedMachine, fake_devices};
use crate::make_flow::{ConnectPhase, MakeStep};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// A heart on the canvas, then Make and Continue to the machine modal.
fn start_make(harness: &mut Harness<'_, Kerf>) {
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Heart").click();
    harness.run();
    harness.get_by_label("Make").click();
    harness.run_steps(4);
    harness.get_by_label("Continue").click();
}

#[test]
fn killing_the_simulator_mid_flow_shows_an_error_with_a_way_back() {
    let (machine, kill) = SimulatedMachine::stepped_killable(Duration::from_millis(1));
    let mut harness = machine_harness(SIZE, machine);
    start_make(&mut harness);
    pump(&mut harness, "device list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    harness.get_all_by_label("Connect with USB").next().unwrap().click();
    pump(&mut harness, "connected", |a| step_is(a, MakeStep::ConnectMachine(ConnectPhase::Connected)));
    harness.get_all_by_label("Continue").last().unwrap().click();
    pump(&mut harness, "Set step", |a| a.flow.as_ref().is_some_and(|f| f.device_state == DeviceState::WaitingOnMaterial));

    kill();
    pump(&mut harness, "the lost machine", |a| step_is(a, MakeStep::Lost));
    assert!(harness.query_by_label("Machine disconnected").is_some(), "the flow names what went wrong");
    assert!(harness.query_by_label_contains("stopped responding").is_some());

    harness.get_by_label("Back to canvas").click();
    harness.run_steps(3);
    assert!(harness.state().flow.is_none(), "Back to canvas closes the flow");
}

#[test]
fn a_remembered_machine_missing_from_the_list_reopens_the_machine_picker_with_a_notice() {
    let mut harness = machine_harness(SIZE, SimulatedMachine::stepped(Duration::from_millis(1)));
    let gone = crate::bridge::DeviceInfo { serial: "GONE-0001".into(), ..fake_devices().remove(0) };
    harness.state_mut().machine = Some(gone);
    start_make(&mut harness);
    pump(&mut harness, "machine picker", |a| step_is(a, MakeStep::ConnectMachine(ConnectPhase::Listing)) && a.flow.as_ref().is_some_and(|f| f.missing.is_some()));
    assert!(harness.query_by_label_contains("GONE-0001) was not found").is_some(), "the notice names the missing machine");
    assert!(harness.query_all_by_label("Connect with USB").next().is_some(), "the machines that answered can be chosen");
}

#[test]
fn twenty_project_copies_spill_onto_further_mats_in_prepare() {
    let mut harness = machine_harness(SIZE, SimulatedMachine::stepped(Duration::from_millis(1)));
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    let s = &mut harness.state_mut().design.shapes[0];
    (s.width_in, s.height_in) = (3.0, 3.0);
    harness.get_by_label("Make").click();
    harness.run_steps(4);
    harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Project copies").focus();
    harness.run_steps(2);
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Project copies").type_text("20");
    harness.run_steps(2);
    harness.press_key(egui::Key::Enter);
    harness.run_steps(3);
    let flow = harness.state().flow.as_ref().unwrap();
    assert_eq!(flow.setup.copies, 20);
    assert_eq!(flow.mats.iter().map(|m| m.items.len()).collect::<Vec<_>>(), [9, 9, 2]);
    assert!(harness.query_all_by_label_contains("Load 3").next().is_some(), "Prepare lists the third mat");
}
