//! An open device's profile, its serial port and the connection test, inside the machine modal.

use std::ops::RangeInclusive;

use egui::{Align, Layout, Ui};

use crate::make_flow::{MakeFlow, TestState};
use crate::output::link::PortInfo;
use crate::output::profile::{MM_PER_IN, Origin};
use crate::output::{DeviceProfile, Protocol, ToolKind};
use crate::units::{FIELD_W, Units, length_field, number_field};

use super::Outbox;
use super::flow_kit::{self as kit, Notice, body_text, divider, heading, section_header};
use super::machine_modal::Devices;

const WIDTH: f32 = 520.0;
const BAUDS: [u32; 6] = [9600, 19_200, 38_400, 57_600, 115_200, 250_000];

pub fn show(ui: &mut Ui, flow: &mut MakeFlow, devices: Devices, out: &mut Outbox) {
    let Some(before) = flow.profile().cloned() else { return };
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 8.0;
    let saved = devices.saved.iter().any(|d| d.id == before.id);
    heading(ui, if saved { "Set up device" } else { "Add device" });
    let mut profile = before.clone();
    let ports = devices.ports.get_or_insert_with(|| devices.host.ports());
    if port_row(ui, &mut profile, ports) {
        *devices.ports = None;
    }
    test_row(ui, flow, out);
    divider(ui);
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if !saved {
            presets(ui, &mut profile);
        }
        fields(ui, &mut profile, devices.units);
    });
    divider(ui);
    if profile != before {
        let reconnect = (&profile.port, profile.baud, profile.protocol, profile.xon_xoff) != (&before.port, before.baud, before.protocol, before.xon_xoff);
        if let Some(p) = flow.profile_mut() {
            *p = profile;
        }
        flow.profile_changed();
        if reconnect {
            flow.test = TestState::Untested;
        }
    }
    kit::actions(ui, |ui| {
        let passed = matches!(flow.test, TestState::Passed(_));
        if kit::primary_button(ui, "Continue", passed).clicked() {
            out.extend(flow.finish_setup());
        }
        if kit::secondary_button(ui, "Back", true).clicked() {
            out.extend(flow.change_machine());
        }
        if saved && kit::secondary_button(ui, "Delete device", true).clicked() {
            tracing::info!(target: "cmd", device = %before.name, "delete open device");
            out.extend(flow.delete_profile());
        }
    });
}

/// The port dropdown with the OS's USB names, and Refresh. Returns whether Refresh was pressed.
fn port_row(ui: &mut Ui, profile: &mut DeviceProfile, ports: &[PortInfo]) -> bool {
    section_header(ui, "Port");
    let labels: Vec<String> = ports.iter().map(port_label).collect();
    let mut options: Vec<(Option<usize>, &str)> = vec![(None, "Choose a port")];
    options.extend(labels.iter().enumerate().map(|(i, l)| (Some(i), l.as_str())));
    let current = profile.port.as_ref().and_then(|name| ports.iter().position(|p| &p.name == name));
    let mut refresh = false;
    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(ui.available_width() - 110.0, 32.0), |ui| {
            if let Some(pick) = kit::dropdown(ui, "Port", current, &options) {
                profile.port = pick.map(|i| ports[i].name.clone());
            }
        });
        refresh = kit::secondary_button(ui, "Refresh", true).clicked();
    });
    if let Some(note) = port_note(profile.port.as_deref(), ports) {
        body_text(ui, note, true);
    }
    refresh
}

/// A port as the OS names it, `COM3`, `/dev/ttyUSB0` or `/dev/cu.usbserial-1`, then what the OS says it
/// is. Every port is listed: none is hidden for its name.
fn port_label(port: &PortInfo) -> String {
    if port.description.is_empty() { port.name.clone() } else { format!("{}  {}", port.name, port.description) }
}

/// The line under the dropdown. A saved port this machine lacks, such as `COM3` in a profile from
/// Windows opened on a Mac, is reported, not treated as an error.
fn port_note(saved: Option<&str>, ports: &[PortInfo]) -> Option<String> {
    match saved {
        Some(name) if !ports.iter().any(|p| p.name == name) => Some(format!("Port not found: {name}. Plug the device in and press Refresh, or choose another port.")),
        _ if ports.is_empty() => Some("No serial ports found. Plug the device in, then press Refresh.".into()),
        _ => None,
    }
}

fn test_row(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    let has_port = flow.profile().is_some_and(|p| p.port.is_some());
    kit::controls(ui, |ui| {
        if kit::secondary_button(ui, "Test connection", has_port && flow.test != TestState::Testing).clicked() {
            tracing::info!(target: "cmd", port = ?flow.profile().and_then(|p| p.port.clone()), "test connection");
            out.extend(flow.test_connection());
        }
        if flow.test == TestState::Testing {
            ui.add(egui::Spinner::new().size(16.0).color(super::tokens::ACCENT_PRIMARY));
            body_text(ui, "Testing...", true);
        }
    });
    match &flow.test {
        TestState::Passed(words) => kit::check_item(ui, true, &format!("Connected. {words}")),
        TestState::Failed(words) => kit::notice(ui, Notice::Warning, &format!("Connection test failed: {words}")),
        TestState::Untested if has_port => {
            body_text(ui, "Test the connection before you continue. The test never moves the machine.", true);
        }
        TestState::Untested | TestState::Testing => {}
    }
}

fn presets(ui: &mut Ui, profile: &mut DeviceProfile) {
    section_header(ui, "Start from");
    let presets = DeviceProfile::presets();
    let current = presets.iter().position(|p| p.name == profile.name);
    let options: Vec<(Option<usize>, &str)> = presets.iter().enumerate().map(|(i, p)| (Some(i), p.name.as_str())).collect();
    if let Some(Some(i)) = kit::segmented(ui, current, &options) {
        *profile = DeviceProfile { id: profile.id, port: profile.port.take(), ..presets[i].clone() };
    }
    divider(ui);
}

fn fields(ui: &mut Ui, p: &mut DeviceProfile, units: Units) {
    section_header(ui, "Device");
    row(ui, "Name", |ui| _ = kit::text_field(ui, &mut p.name, "Name"));
    row(ui, "Protocol", |ui| {
        let options = Protocol::ALL.map(|v| (v, v.label()));
        if let Some(v) = kit::segmented(ui, p.protocol, &options) {
            p.protocol = v;
        }
    });
    row(ui, "Baud", |ui| {
        let options: Vec<(u32, String)> = BAUDS.iter().map(|&b| (b, b.to_string())).collect();
        let options: Vec<(u32, &str)> = options.iter().map(|(b, s)| (*b, s.as_str())).collect();
        if let Some(b) = kit::dropdown(ui, "Baud", p.baud, &options) {
            p.baud = b;
        }
    });
    row(ui, "Units per mm", |ui| number(ui, "Units per mm", &mut p.units_per_mm, 0.001..=1000.0, ""));
    row(ui, "Origin", |ui| {
        let options = Origin::ALL.map(|o| (o, o.label()));
        if let Some(o) = kit::dropdown(ui, "Origin", p.origin, &options) {
            p.origin = o;
        }
    });
    kit::toggle_row(ui, "Swap X and Y", &mut p.swap_xy);
    row(ui, "Work area width", |ui| length(ui, "Work area width", &mut p.work_area_mm.0, units));
    row(ui, "Work area height", |ui| length(ui, "Work area height", &mut p.work_area_mm.1, units));
    divider(ui);
    tool(ui, p, units);
    divider(ui);
    section_header(ui, "Motion");
    row(ui, "Slowest speed", |ui| number(ui, "Slowest speed", &mut p.speed_mm_s.0, 0.1..=2000.0, "mm/s"));
    row(ui, "Fastest speed", |ui| number(ui, "Fastest speed", &mut p.speed_mm_s.1, 0.1..=2000.0, "mm/s"));
    let mut has_force = p.force.is_some();
    if kit::toggle_row(ui, "Set the cutting force", &mut has_force).changed() {
        p.force = has_force.then_some((20, 400));
    }
    if let Some((lo, hi)) = p.force.as_mut() {
        row(ui, "Weakest force", |ui| count(ui, "Weakest force", lo, 0..=10_000, ""));
        row(ui, "Strongest force", |ui| count(ui, "Strongest force", hi, 0..=10_000, ""));
    }
    row(ui, "Overcut", |ui| length(ui, "Overcut", &mut p.overcut_mm, units));
    kit::toggle_row(ui, "XON/XOFF flow control", &mut p.xon_xoff);
    if p.protocol == Protocol::Hpgl {
        divider(ui);
        section_header(ui, "HPGL");
        let mut identify = p.identify.clone().unwrap_or_default();
        row(ui, "Identify query", |ui| _ = kit::text_field(ui, &mut identify, "None"));
        p.identify = Some(identify).filter(|q| !q.trim().is_empty());
        row(ui, "Extra abort", |ui| _ = kit::text_field(ui, &mut p.abort, "None"));
        kit::toggle_row(ui, "Pause to swap tools for Pen", &mut p.pen_change);
    }
}

fn tool(ui: &mut Ui, p: &mut DeviceProfile, units: Units) {
    section_header(ui, "Tool");
    let kinds = [
        (0, ToolKind::DragKnife { offset_mm: 0.25, swivel_threshold_deg: 30.0 }),
        (1, ToolKind::Pen { up: "M5".into(), down: "M3 S30".into(), settle_ms: 150 }),
        (2, ToolKind::Laser { power_max: 1000, dynamic: true }),
    ];
    let current = kinds.iter().position(|(_, k)| std::mem::discriminant(k) == std::mem::discriminant(&p.tool)).unwrap_or(0);
    let options = kinds.each_ref().map(|(i, k)| (*i, k.label()));
    if let Some(i) = kit::segmented(ui, current, &options) {
        p.tool = kinds[i].1.clone();
    }
    match &mut p.tool {
        ToolKind::DragKnife { offset_mm, swivel_threshold_deg } => {
            row(ui, "Blade offset", |ui| length(ui, "Blade offset", offset_mm, units));
            row(ui, "Swivel above", |ui| number(ui, "Swivel above", swivel_threshold_deg, 0.0..=180.0, "°"));
        }
        ToolKind::Pen { up, down, settle_ms } => {
            row(ui, "Pen down", |ui| _ = kit::text_field(ui, down, "M3 S30"));
            row(ui, "Pen up", |ui| _ = kit::text_field(ui, up, "M5"));
            row(ui, "Settle time", |ui| count(ui, "Settle time", settle_ms, 0..=5000, "ms"));
        }
        ToolKind::Laser { power_max, dynamic } => {
            row(ui, "Full power S", |ui| count(ui, "Full power S", power_max, 1..=100_000, ""));
            kit::toggle_row(ui, "Dynamic power (M4)", dynamic);
        }
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        body_text(ui, label, false);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.set_max_width(240.0);
            add(ui);
        });
    });
}

fn number(ui: &mut Ui, label: &str, value: &mut f32, range: RangeInclusive<f32>, suffix: &str) {
    _ = number_field(ui, label, Some(*value), suffix, FIELD_W).is_some_and(|t| t.set(value, range));
}

fn count(ui: &mut Ui, label: &str, value: &mut u32, range: RangeInclusive<u32>, suffix: &str) {
    let mut v = *value as f32;
    if number_field(ui, label, Some(v), suffix, FIELD_W).is_some_and(|t| t.set(&mut v, *range.start() as f32..=*range.end() as f32)) {
        *value = v.round() as u32;
    }
}

/// A length stored in millimetres, shown and typed in the app's units.
fn length(ui: &mut Ui, label: &str, mm: &mut f32, units: Units) {
    let mut inches = *mm / MM_PER_IN;
    if length_field(ui, label, Some(inches), units, FIELD_W).is_some_and(|t| t.set(&mut inches, 0.0..=10_000.0 / MM_PER_IN)) {
        *mm = inches * MM_PER_IN;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, description: &str) -> PortInfo {
        PortInfo { name: name.into(), description: description.into(), usb: None }
    }

    #[test]
    fn ports_are_listed_by_the_name_each_os_gives_them() {
        let ports = [port("COM3", "USB: FT232R (FTDI)"), port("/dev/ttyUSB0", ""), port("/dev/ttyACM0", "USB: Arduino Uno"), port("/dev/cu.usbserial-1", "")];
        let labels: Vec<String> = ports.iter().map(port_label).collect();
        assert_eq!(labels, ["COM3  USB: FT232R (FTDI)", "/dev/ttyUSB0", "/dev/ttyACM0  USB: Arduino Uno", "/dev/cu.usbserial-1"]);
    }

    #[test]
    fn a_saved_port_this_machine_lacks_is_not_found() {
        let linux = [port("/dev/ttyUSB0", ""), port("/dev/ttyACM0", "")];
        assert_eq!(port_note(Some("/dev/ttyACM0"), &linux), None, "a present port needs no note");
        assert_eq!(port_note(None, &linux), None);
        let note = port_note(Some("COM3"), &linux).expect("a Windows port on Linux");
        assert!(note.starts_with("Port not found: COM3."), "{note}");
        let note = port_note(Some("/dev/cu.usbserial-1"), &[]).expect("a Mac port with nothing plugged in");
        assert!(note.starts_with("Port not found: /dev/cu.usbserial-1."), "the saved port is named even with no ports at all: {note}");
        assert_eq!(port_note(None, &[]).as_deref(), Some("No serial ports found. Plug the device in, then press Refresh."));
    }
}
