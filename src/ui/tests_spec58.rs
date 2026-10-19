//! Spec 58: USB machine detection in the Select Machine modal. Detection reads the USB descriptor the
//! OS already parsed, so no test opens a real port, and none opens a Cricut port at all: `DetectHost`
//! refuses to open anything.

use std::io;
use std::path::PathBuf;

use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_flow_snapshot, pump, snapshot_harness, step_is};
use crate::design::{Design, ShapeKind};
use crate::make_flow::{ConnectPhase, MakeStep};
use crate::output::DeviceProfile;
use crate::output::link::{Host, Opener, PortInfo, UsbId};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// A host that lists exactly the USB ports the test names and refuses to open any of them.
struct DetectHost {
    ports: Vec<PortInfo>,
}

impl Host for DetectHost {
    fn ports(&self) -> Vec<PortInfo> {
        self.ports.clone()
    }

    fn open(&self, _profile: &DeviceProfile) -> Opener {
        Box::new(|| Err(io::Error::new(io::ErrorKind::Unsupported, "a detection test never opens a port")))
    }

    fn save_path(&self, _file_name: &str) -> Option<PathBuf> {
        None
    }
}

fn usb_port(name: &str, vid: u16, pid: u16, product: &str) -> PortInfo {
    let usb = UsbId { vid, pid, serial: Some("SN-0001".into()), product: Some(product.into()), manufacturer: None };
    PortInfo { name: name.into(), description: format!("USB: {product}"), usb: Some(usb) }
}

fn click(fixture: &mut Fixture, label: &str) {
    fixture.get_all_by_label(label).last().unwrap_or_else(|| panic!("no {label}")).click();
    fixture.run_steps(2);
}

/// Opens the Select Machine modal over a host listing `ports`, with one square to make.
fn modal_over(ports: Vec<PortInfo>) -> Fixture {
    let mut fixture = snapshot_harness(SIZE);
    fixture.state_mut().host = Box::new(DetectHost { ports });
    let mut design = Design::default();
    design.add_shape(ShapeKind::Square);
    fixture.state_mut().design = design;
    fixture.run();
    click(&mut fixture, "Make");
    click(&mut fixture, "Continue");
    pump(&mut fixture, "machine list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));
    assert!(step_is(fixture.state(), MakeStep::ConnectMachine(ConnectPhase::Listing)), "the modal is on the machine list");
    fixture
}

#[test]
fn a_cricut_is_named_with_the_honest_notice_and_no_button() {
    let mut fixture = modal_over(vec![usb_port("/dev/cu.usbmodem44023240721", 0x20D3, 0x0028, "Cricut Maker 3")]);
    assert!(fixture.query_by_label_contains("Detected on USB").is_some(), "the detected section is shown");
    // The simulator's own family row also reads "Cricut Maker 3", so the unique notice and the snapshot
    // pin the detected row; here we only confirm the product name is on screen.
    assert!(fixture.get_all_by_label_contains("Cricut Maker 3").next().is_some(), "the row names the machine from its product string");
    assert!(fixture.query_by_label_contains("cannot send cuts to a Cricut machine").is_some(), "the notice states the limit");
    assert!(fixture.query_by_label_contains("Cricut Design Space").is_some(), "the notice points at Design Space");
    assert!(fixture.query_by_label("Set up").is_none(), "a Cricut row has no set up button");
    assert_flow_snapshot(&mut fixture, "58-cricut-detected");
}

#[test]
fn an_open_cutter_sets_up_with_the_preset_and_port_filled_in() {
    let node = "/dev/cu.usbserial-AB01";
    let mut fixture = modal_over(vec![usb_port(node, 0x0403, 0x6001, "FT232R USB UART")]);
    assert!(fixture.query_by_label_contains("FT232R USB UART").is_some(), "the row names the open cutter");
    assert_flow_snapshot(&mut fixture, "58-open-cutter-detected");

    click(&mut fixture, "Set up");
    assert!(step_is(fixture.state(), MakeStep::ConnectMachine(ConnectPhase::Setup)), "Set up opens the device setup screen");
    let profile = fixture.state().flow.as_ref().unwrap().profile().cloned().expect("the setup screen has a profile");
    assert_eq!(profile.name, "HPGL vinyl cutter", "the matching preset is selected");
    assert_eq!(profile.port.as_deref(), Some(node), "the detected node is pre-filled");
}

#[test]
fn an_unrecognised_usb_port_makes_no_detected_row_but_stays_in_the_port_picker() {
    let node = "/dev/cu.usbserial-UNKNOWN";
    let mut fixture = modal_over(vec![usb_port(node, 0x1234, 0x5678, "Mystery widget")]);
    assert!(fixture.query_by_label_contains("Detected on USB").is_none(), "an unknown vendor id promotes no row");

    click(&mut fixture, "Add device");
    assert!(step_is(fixture.state(), MakeStep::ConnectMachine(ConnectPhase::Setup)), "Add device opens device setup");
    click(&mut fixture, "Port");
    assert!(fixture.query_by_label_contains(node).is_some(), "the unrecognised port is still selectable in the picker");
}
