//! Recognises a plugged-in USB serial machine from the descriptor the OS already parsed. This table is
//! the single source of which vendor ids Kerf knows; the UI asks `identify` and never matches ids
//! itself. Recognition reads the descriptor only. Nothing here opens a port or touches any protocol.

use super::link::PortInfo;

/// Cricut Inc's USB vendor id, from the live capture in
/// `docs/native-supporting/live-capture-2026-10-18.md`.
const CRICUT_VID: u16 = 0x20D3;

/// USB-serial-bridge vendor ids common on open cutters, each mapped to the preset to start from. FTDI,
/// QinHeng (CH340) and Silicon Labs (CP210x) are the bridges vinyl cutters and GRBL boards ship with.
const OPEN_CUTTER_BRIDGES: [(u16, &str); 3] = [(0x0403, "HPGL vinyl cutter"), (0x1A86, "HPGL vinyl cutter"), (0x10C4, "HPGL vinyl cutter")];

/// A machine Kerf recognised on a USB serial port. `name` is what the row shows: the OS product string
/// where it has one, so a Cricut reads as "Cricut Maker 3", with a plain fallback otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownMachine {
    pub name: String,
    pub kind: MachineKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineKind {
    /// An open cutter Kerf drives, matched to a `DeviceProfile::presets()` entry by name.
    OpenCutter { preset: &'static str },
    /// A Cricut machine Kerf names but cannot drive: its protocol is encrypted and server-authenticated.
    Cricut,
}

/// The machine on `port`, if Kerf recognises its USB vendor id. A Cricut is matched by vendor id alone,
/// since Kerf names it the same way whatever the model; an open cutter by its bridge's vendor id.
pub fn identify(port: &PortInfo) -> Option<KnownMachine> {
    let usb = port.usb.as_ref()?;
    if usb.vid == CRICUT_VID {
        let name = usb.product.clone().unwrap_or_else(|| "Cricut machine".into());
        return Some(KnownMachine { name, kind: MachineKind::Cricut });
    }
    let preset = OPEN_CUTTER_BRIDGES.iter().find(|(vid, _)| *vid == usb.vid).map(|(_, preset)| *preset)?;
    let name = usb.product.clone().unwrap_or_else(|| "Open cutter".into());
    Some(KnownMachine { name, kind: MachineKind::OpenCutter { preset } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::DeviceProfile;
    use crate::output::link::UsbId;

    fn port(vid: u16, product: Option<&str>) -> PortInfo {
        let usb = UsbId { vid, pid: 0x0028, serial: Some("4402324072".into()), product: product.map(Into::into), manufacturer: None };
        PortInfo { name: "/dev/cu.usbmodem44023240721".into(), description: String::new(), usb: Some(usb) }
    }

    #[test]
    fn a_cricut_vendor_id_is_a_cricut_named_from_the_product_string() {
        let machine = identify(&port(0x20D3, Some("Cricut Maker 3"))).expect("the Cricut vendor id is recognised");
        assert_eq!(machine.kind, MachineKind::Cricut);
        assert_eq!(machine.name, "Cricut Maker 3");
    }

    #[test]
    fn a_known_bridge_is_an_open_cutter_whose_preset_exists() {
        let machine = identify(&port(0x0403, Some("FT232R USB UART"))).expect("an FTDI bridge is recognised");
        let MachineKind::OpenCutter { preset } = machine.kind else { panic!("{:?} is not an open cutter", machine.kind) };
        assert!(DeviceProfile::presets().iter().any(|p| p.name == preset), "the preset {preset:?} must exist in DeviceProfile::presets()");
    }

    #[test]
    fn an_unknown_vendor_id_is_not_recognised() {
        assert_eq!(identify(&port(0x1234, Some("Mystery device"))), None);
    }

    #[test]
    fn a_port_with_no_usb_descriptor_is_not_recognised() {
        let plain = PortInfo { name: "/dev/ttyS0".into(), description: String::new(), usb: None };
        assert_eq!(identify(&plain), None);
    }
}
