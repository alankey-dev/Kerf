//! Spec 24: open cutters driven through the Make flow against a fake serial link. No test opens a
//! real port: the app's host is swapped for `FakeHost` before anything is made.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, assert_flow_snapshot, pump, snapshot_harness, step_is};
use crate::design::{Design, ShapeKind};
use crate::make_flow::{ConnectPhase, DonePhase, MakeStep, Outcome};
use crate::output::link::{FAKE_PORT, FakeHost, FakeLink};
use crate::output::{DeviceProfile, OutputDevice};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);
const MAX_FRAMES: usize = 2_000;
/// A failure bound only: nothing here waits this long unless the runner has stopped answering.
const HANG_GUARD: Duration = Duration::from_secs(10);

/// The app over `link`, saving dry runs into the fixture's temp directory.
fn open_device_app(link: &FakeLink) -> (Fixture, PathBuf) {
    let mut fixture = snapshot_harness(SIZE);
    let dir = fixture.root.path().join("dry-runs");
    std::fs::create_dir_all(&dir).unwrap();
    fixture.state_mut().host = Box::new(FakeHost { link: link.clone(), dir: dir.clone() });
    (fixture, dir)
}

/// Steps until `done` holds. Between frames it blocks on the runner's next event, so the wait follows
/// the device rather than a clock.
fn settle(fixture: &mut Fixture, what: &str, done: impl Fn(&Kerf) -> bool) {
    for _ in 0..MAX_FRAMES {
        fixture.step();
        if done(fixture.state()) {
            fixture.run_steps(2);
            return;
        }
        let answered = fixture.state().runner.as_ref().is_some_and(|r| r.await_event(HANG_GUARD));
        assert!(answered, "the device went quiet before {what}");
    }
    panic!("{what} not reached in {MAX_FRAMES} frames");
}

/// Blocks until the fake port has received bytes matching `done`.
fn settle_bytes(link: &FakeLink, what: &str, done: impl Fn(&[u8]) -> bool) {
    assert!(link.wait_for(HANG_GUARD, done), "the port never received {what}");
}

fn click(fixture: &mut Fixture, label: &str) {
    fixture.get_all_by_label(label).last().unwrap_or_else(|| panic!("no {label}")).click();
    fixture.run_steps(2);
}

/// A black square and a red one: two colours, so two mats.
fn two_colours(fixture: &mut Fixture) {
    let mut design = Design::default();
    design.add_shape(ShapeKind::Square);
    let red = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(red).unwrap();
    s.fill = egui::Color32::RED;
    s.x_in = 4.0;
    fixture.state_mut().design = design;
    fixture.run();
}

fn add_pen_plotter(fixture: &mut Fixture) {
    click(fixture, "Make");
    click(fixture, "Continue");
    pump(fixture, "machine list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    click(fixture, "Add device");
    assert!(step_is(fixture.state(), MakeStep::ConnectMachine(ConnectPhase::Setup)), "Add device opens the setup screen");
    click(fixture, "GRBL pen plotter");
    choose_fake_port(fixture);
    test_connection(fixture);
}

fn choose_fake_port(fixture: &mut Fixture) {
    click(fixture, "Port");
    fixture.get_by_label_contains(FAKE_PORT).click();
    fixture.run_steps(2);
}

fn test_connection(fixture: &mut Fixture) {
    click(fixture, "Test connection");
    settle(fixture, "the connection test", |a| a.flow.as_ref().is_some_and(|f| matches!(f.test, crate::make_flow::TestState::Passed(_))));
    assert!(fixture.query_by_label_contains("Connected. Grbl").is_some(), "the banner is shown");
}

fn to_go(fixture: &mut Fixture) {
    settle(fixture, "Set", |a| step_is(a, MakeStep::Set));
    const VINYL: &str = "Vinyl  (Fine-Point Blade, Clamp B)";
    if fixture.query_by_label(VINYL).is_none() {
        click(fixture, "Browse All Materials");
    }
    click(fixture, VINYL);
    click(fixture, "Next");
    assert!(step_is(fixture.state(), MakeStep::Load));
    click(fixture, "Continue");
    assert!(step_is(fixture.state(), MakeStep::Go));
}

fn press_go_and_finish(fixture: &mut Fixture) {
    click(fixture, "Go");
    settle(fixture, "the mat done", |a| step_is(a, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Checking }));
    click(fixture, "Looks good");
    click(fixture, "Material removed");
    assert!(step_is(fixture.state(), MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloaded }));
}

/// `UPDATE_GOLDEN=1 cargo test spec24` rewrites the file.
fn assert_golden(name: &str, bytes: &[u8]) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/24").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, bytes).unwrap();
    }
    let expected = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}; run UPDATE_GOLDEN=1 cargo test spec24", path.display()));
    assert!(bytes == expected, "{name} differs from the golden file:\n{}", String::from_utf8_lossy(bytes));
}

#[test]
fn a_pen_plotter_makes_two_mats_from_the_golden_gcode() {
    let link = FakeLink::grbl();
    let (mut fixture, _) = open_device_app(&link);
    two_colours(&mut fixture);
    add_pen_plotter(&mut fixture);
    assert_flow_snapshot(&mut fixture, "24-device-setup");
    assert_eq!(link.written(), b"\n", "the test sent only the bare newline that asks for the banner");
    click(&mut fixture, "Continue");
    assert_eq!(fixture.state().flow.as_ref().unwrap().mats.len(), 2);
    assert_eq!(fixture.state().last_device, Some(fixture.state().devices[0].id), "the profile is remembered");

    for mat in 1..=2 {
        to_go(&mut fixture);
        link.clear();
        let job = fixture.state().flow.as_ref().unwrap().job().unwrap();
        press_go_and_finish(&mut fixture);
        assert_eq!(link.written(), job.bytes(), "mat {mat}: the port got exactly the job");
        assert_golden(&format!("two-colour-mat{mat}.gcode"), &link.written());
        if mat == 1 {
            click(&mut fixture, "Next Mat");
        }
    }
    click(&mut fixture, "Back to canvas");
    assert!(fixture.state().flow.is_none());
    assert!(fixture.state().runner.is_none(), "closing the flow lets the device go");
}

#[test]
fn nothing_beyond_the_identity_query_reaches_the_device_before_go() {
    let link = FakeLink::grbl();
    let (mut fixture, _) = open_device_app(&link);
    two_colours(&mut fixture);
    add_pen_plotter(&mut fixture);
    click(&mut fixture, "Continue");
    to_go(&mut fixture);
    fixture.run_steps(5);
    assert_eq!(link.written(), b"\n", "Set, Load and Go so far sent nothing");
    click(&mut fixture, "Go");
    settle_bytes(&link, "motion after Go", |b| b.windows(2).any(|w| w == b"G1"));
}

#[test]
fn save_to_file_writes_the_bytes_the_port_receives() {
    let link = FakeLink::grbl();
    let (mut fixture, dir) = open_device_app(&link);
    two_colours(&mut fixture);
    add_pen_plotter(&mut fixture);
    click(&mut fixture, "Continue");
    to_go(&mut fixture);
    click(&mut fixture, "Save to file");
    let saved = std::fs::read(dir.join("Untitled-1 - Mat 1.gcode")).expect("the dry run wrote the file");
    assert_eq!(link.written(), b"\n", "the dry run sent nothing to the port");
    link.clear();
    press_go_and_finish(&mut fixture);
    assert!(!saved.is_empty());
    assert_eq!(saved, link.written(), "the file holds exactly what the port received");
}

#[test]
fn the_machine_modal_lists_the_simulator_then_each_profile() {
    let (mut fixture, _) = open_device_app(&FakeLink::grbl());
    let cutter = DeviceProfile { id: 1, port: Some(FAKE_PORT.into()), name: "Vinyl cutter".into(), ..DeviceProfile::hpgl_vinyl_cutter() };
    let laser = DeviceProfile { id: 2, name: "Desk laser".into(), ..DeviceProfile::grbl_laser() };
    fixture.state_mut().devices = vec![cutter, laser];
    two_colours(&mut fixture);
    click(&mut fixture, "Make");
    click(&mut fixture, "Continue");
    pump(&mut fixture, "device list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    assert!(fixture.query_by_label_contains("Vinyl cutter  HPGL").is_some());
    assert!(fixture.query_by_label_contains("Desk laser  GRBL").is_some());
    assert_flow_snapshot(&mut fixture, "24-machine-modal");
}

#[test]
fn a_laser_shows_its_safety_notice_and_save_to_file_on_go() {
    let link = FakeLink::grbl();
    let (mut fixture, _) = open_device_app(&link);
    let laser = DeviceProfile { id: 1, name: "Desk laser".into(), port: Some(FAKE_PORT.into()), ..DeviceProfile::grbl_laser() };
    (fixture.state_mut().devices, fixture.state_mut().last_device) = (vec![laser], Some(1));
    two_colours(&mut fixture);
    click(&mut fixture, "Make");
    click(&mut fixture, "Continue");
    assert!(step_is(fixture.state(), MakeStep::ConnectMachine(ConnectPhase::Setup)), "the remembered device opens its setup to test the port");
    test_connection(&mut fixture);
    click(&mut fixture, "Continue");
    to_go(&mut fixture);
    assert!(fixture.query_by_label_contains("wear eye protection").is_some());
    assert!(fixture.query_by_label("Save to file").is_some());
    assert_flow_snapshot(&mut fixture, "24-go-laser");
}

#[test]
fn a_profile_saved_with_another_platforms_port_opens_with_port_not_found() {
    let (mut fixture, _) = open_device_app(&FakeLink::grbl());
    let windows = DeviceProfile { id: 1, name: "Shop plotter".into(), port: Some("COM3".into()), ..DeviceProfile::grbl_pen_plotter() };
    let saved: DeviceProfile = serde_json::from_str(&serde_json::to_string(&windows).unwrap()).expect("a profile saved on Windows loads anywhere");
    (fixture.state_mut().devices, fixture.state_mut().last_device) = (vec![saved], Some(1));
    two_colours(&mut fixture);
    click(&mut fixture, "Make");
    click(&mut fixture, "Continue");
    assert!(step_is(fixture.state(), MakeStep::ConnectMachine(ConnectPhase::Setup)), "the remembered device opens its setup");
    assert!(fixture.query_by_label_contains("Port not found: COM3").is_some(), "the missing port is named");
    assert_eq!(fixture.state().flow.as_ref().and_then(|f| f.profile()).and_then(|p| p.port.as_deref()), Some("COM3"), "and kept until another is chosen");
    choose_fake_port(&mut fixture);
    assert!(fixture.query_by_label_contains("Port not found").is_none(), "choosing a port here clears it");
}

/// A pen plotter job stuck on its first line: the fake board gives the banner but never says `ok`.
fn stuck_mid_job() -> (Fixture, FakeLink) {
    let link = FakeLink::new(crate::output::link::Answer::Silent);
    link.reply(crate::output::link::GRBL_BANNER);
    let (mut fixture, _) = open_device_app(&link);
    two_colours(&mut fixture);
    add_pen_plotter(&mut fixture);
    click(&mut fixture, "Continue");
    to_go(&mut fixture);
    click(&mut fixture, "Go");
    settle_bytes(&link, "the first job line", |b| b.len() > 1);
    (fixture, link)
}

fn assert_stopped(link: &FakeLink) {
    settle_bytes(link, "the stop bytes", |b| b.windows(2).any(|w| w == b"!\x18") && b.ends_with(b"M5\n"));
    let text = link.written_text();
    let after_stop = &text[text.find("!\u{18}").unwrap()..];
    assert!(!after_stop.contains("G1") && !after_stop.contains("G0"), "no motion after the stop: {after_stop:?}");
}

#[test]
fn cancel_mid_job_stops_the_device_with_no_motion_after() {
    let (mut fixture, link) = stuck_mid_job();
    click(&mut fixture, "Cancel");
    assert!(step_is(fixture.state(), MakeStep::Done { outcome: Outcome::Cancelled, phase: DonePhase::Unloading }));
    assert_stopped(&link);
}

#[test]
fn closing_the_app_mid_job_cancels_the_device_first() {
    let (mut fixture, link) = stuck_mid_job();
    fixture.state_mut().close_device();
    assert!(fixture.state().runner.is_none());
    assert_stopped(&link);
}

#[test]
fn make_from_a_home_card_starts_on_the_remembered_device() {
    let link = FakeLink::grbl();
    let (mut fixture, _) = open_device_app(&link);
    let plotter = DeviceProfile { id: 1, name: "Desk plotter".into(), port: Some(FAKE_PORT.into()), ..DeviceProfile::grbl_pen_plotter() };
    (fixture.state_mut().devices, fixture.state_mut().last_device) = (vec![plotter], Some(1));
    two_colours(&mut fixture);
    assert!(fixture.state_mut().save_to_disk(), "the design saves as a project");
    let id = fixture.state().project.as_ref().unwrap().id.clone();
    fixture.state_mut().flow = None;
    fixture.state_mut().open_and_make(&id);
    let flow = fixture.state().flow.as_ref().expect("Home > Make opens the flow");
    assert!(matches!(flow.device, OutputDevice::Open(ref p) if p.id == 1), "Home > Make uses the remembered device, as the canvas Make does");
}
