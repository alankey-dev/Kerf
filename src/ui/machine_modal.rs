use egui::{Context, Id, RichText, Ui};

use crate::bridge::DeviceInfo;
use crate::machine::{ConnectionType, MachineFamily};
use crate::make_flow::{ConnectPhase, MakeFlow, MakeStep};
use crate::output::DeviceProfile;
use crate::output::link::{Host, PortInfo};
use crate::output::machines::{MachineKind, identify};
use crate::units::Units;

use super::Outbox;
use super::device_setup;
use super::flow_kit::{self as kit, body_text, heading, section_header};
use super::fonts;
use super::tokens::*;

const CONNECTIONS: [(ConnectionType, &str); 2] = [(ConnectionType::Usb, "Connect with USB"), (ConnectionType::Bluetooth, "Connect with Bluetooth")];
const WIDTH: f32 = 400.0;

/// The open devices the modal lists and the setup screen edits.
pub struct Devices<'a> {
    pub saved: &'a [DeviceProfile],
    pub host: &'a dyn Host,
    /// Scanned when the setup screen first needs it and again on Refresh.
    pub ports: &'a mut Option<Vec<PortInfo>>,
    pub units: Units,
}

pub fn show(ctx: &Context, flow: &mut MakeFlow, devices: Devices, out: &mut Outbox) {
    let MakeStep::ConnectMachine(phase) = flow.step else { return };
    let modal = egui::Modal::new(Id::new("machine-modal")).frame(kit::card_frame()).show(ctx, |ui| {
        kit::themed(ui, |ui| match phase {
            ConnectPhase::Listing => {
                ui.set_width(WIDTH);
                listing(ui, flow, devices, out);
            }
            ConnectPhase::Connecting(_) => {
                ui.set_width(WIDTH);
                waiting(ui, "Connecting...");
            }
            ConnectPhase::Connected => {
                ui.set_width(WIDTH);
                connected(ui, flow);
            }
            ConnectPhase::Setup => device_setup::show(ui, flow, devices, out),
        });
    });
    if modal.should_close() && phase == ConnectPhase::Listing {
        flow.dismiss_machine_modal();
    }
}

fn waiting(ui: &mut Ui, words: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.add(egui::Spinner::new().size(16.0).color(ACCENT_PRIMARY));
        body_text(ui, words, true);
    });
}

fn listing(ui: &mut Ui, flow: &mut MakeFlow, devices: Devices, out: &mut Outbox) {
    let Devices { saved, host, ports, units: _ } = devices;
    heading(ui, "Select Machine");
    ui.add_space(16.0);
    if let Some(gone) = &flow.missing {
        let words = format!("Your {} ({}) was not found. Turn it on and connect it, or choose another machine.", gone.device_type, gone.serial);
        kit::notice(ui, kit::Notice::Warning, &words);
        ui.add_space(16.0);
    }
    section_header(ui, "My Machine");
    if flow.devices.is_empty() {
        waiting(ui, "Looking for machines...");
    }
    let mut chosen = None;
    for family in detected_families(&flow.devices) {
        machine_row(ui, family, &flow.devices, &mut chosen);
    }
    ui.add_space(16.0);
    section_header(ui, "Other Machines");
    let detected = detected_families(&flow.devices);
    for family in MachineFamily::ALL.into_iter().filter(|f| !detected.contains(f)) {
        machine_row(ui, family, &[], &mut chosen);
    }
    ui.add_space(16.0);
    detected_section(ui, flow, saved, host, ports);
    section_header(ui, "Open Devices");
    let mut picked = None;
    for profile in saved {
        if kit::select_row(ui, false, &profile.name, &open_detail(profile)).clicked() {
            picked = Some(profile.clone());
        }
    }
    ui.add_space(4.0);
    if kit::secondary_button(ui, "Add device", true).clicked() {
        let id = crate::settings::next_id(saved);
        picked = Some(DeviceProfile { id, ..DeviceProfile::hpgl_vinyl_cutter() });
    }
    if let Some(profile) = picked {
        tracing::info!(target: "cmd", device = %profile.name, new = !saved.iter().any(|d| d.id == profile.id), "choose open device");
        flow.choose_profile(profile);
    }
    if let Some(device) = chosen {
        out.extend(flow.connect(&device));
    }
}

/// The honest limit shown on a detected Cricut row. It links to nothing and promises nothing Kerf
/// cannot do: Kerf reads the descriptor to name the machine, never opens its port.
const CRICUT_NOTICE: &str = "Kerf cannot send cuts to a Cricut machine. Its cut protocol is encrypted and authenticated by Cricut's servers. Use Cricut Design Space to cut, and use Kerf to design and export SVG, DXF or PDF.";

/// The "Detected on USB" section: a row per recognised machine, above Open Devices. A recognised open
/// cutter offers Set up, which opens the setup screen on its preset with the detected port filled in. A
/// Cricut machine is named with the honest notice and no button. Drawn only when a machine is detected.
fn detected_section(ui: &mut Ui, flow: &mut MakeFlow, saved: &[DeviceProfile], host: &dyn Host, ports: &mut Option<Vec<PortInfo>>) {
    let ports = ports.get_or_insert_with(|| host.ports());
    let detected: Vec<(&PortInfo, _)> = ports.iter().filter_map(|p| identify(p).map(|m| (p, m))).collect();
    if detected.is_empty() {
        return;
    }
    section_header(ui, "Detected on USB");
    let mut set_up: Option<(&'static str, String)> = None;
    for (port, machine) in &detected {
        kit::hover_block(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new(machine.name.as_str()).font(fonts::semibold(13.0)).color(TEXT_PRIMARY));
            match machine.kind {
                MachineKind::OpenCutter { preset } => kit::controls(ui, |ui| {
                    if kit::secondary_button(ui, "Set up", true).clicked() {
                        set_up = Some((preset, port.name.clone()));
                    }
                }),
                MachineKind::Cricut => kit::notice(ui, kit::Notice::Warning, CRICUT_NOTICE),
            }
        });
    }
    if let Some((preset, port)) = set_up {
        let base = DeviceProfile::presets().into_iter().find(|p| p.name == preset).unwrap_or_else(DeviceProfile::hpgl_vinyl_cutter);
        let id = crate::settings::next_id(saved);
        tracing::info!(target: "cmd", preset, port = %port, "set up detected open cutter");
        flow.choose_profile(DeviceProfile { id, port: Some(port), ..base });
    }
    ui.add_space(16.0);
}

/// "HPGL, Drag knife, /dev/cu.usbserial-1".
fn open_detail(profile: &DeviceProfile) -> String {
    let mut parts = vec![profile.protocol.label(), profile.tool.label()];
    parts.extend(profile.port.as_deref());
    parts.join(", ")
}

fn detected_families(devices: &[DeviceInfo]) -> Vec<MachineFamily> {
    let mut families: Vec<_> = devices.iter().map(|d| d.machine_family).collect();
    families.dedup();
    families
}

fn machine_row(ui: &mut Ui, family: MachineFamily, devices: &[DeviceInfo], chosen: &mut Option<DeviceInfo>) {
    kit::hover_block(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new(family.display_name()).font(fonts::semibold(13.0)).color(TEXT_PRIMARY));
            if let Some(codename) = family.codename() {
                body_text(ui, codename, true);
            }
        });
        kit::controls(ui, |ui| {
            for (connection, label) in CONNECTIONS {
                let device = devices.iter().find(|d| d.machine_family == family && d.connection_type == connection);
                if kit::secondary_button(ui, label, device.is_some()).clicked() {
                    *chosen = device.cloned();
                }
            }
        });
    });
}

fn connected(ui: &mut Ui, flow: &mut MakeFlow) {
    ui.spacing_mut().item_spacing.y = 8.0;
    heading(ui, "Machine connected");
    body_text(ui, "Yay! Your machine is connected and ready to make!", false);
    if let Some(m) = &flow.machine {
        body_text(ui, format!("{} over {}", m.device_type, m.connection_type.wire_name()), true);
    }
    ui.add_space(16.0);
    kit::actions(ui, |ui| {
        if kit::primary_button(ui, "Continue", true).clicked() {
            flow.finish_connecting();
        }
    });
}
