# 58 USB machine detection

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec58.rs` pass.

**Wave 14. Depends on 24. Size M.** Kerf lists serial ports by their OS description but
recognises nothing, so a maker who plugs in a cutter has to know which `/dev/cu.*` node is
theirs and fill in a profile by hand. This spec gives Kerf plug-in recognition at the
transport layer. Kerf identifies a connected machine from its USB vendor and product id and
acts on what it can honestly do with it.

This is the interoperability layer documented in `docs/real-device-feasibility.md`. It is detection and connection only. It
is deliberately **not** an attempt to drive a Cricut machine: that machine's cut protocol is
AES-encrypted with a key Cricut issues to its own authenticated client, so Kerf names a
Cricut machine and explains the boundary rather than pretending to drive it. Kerf already
drives open cutters that speak cleartext HPGL or GRBL over the same kind of USB CDC node
(spec 24); this spec makes those one click to set up and makes the honest limit visible.

## Behaviour

When the Select Machine modal is open (`src/ui/machine_modal.rs`), Kerf scans serial ports
through the existing `Host` and matches each against a small table of known machines keyed
by USB vendor and product id. A matched port becomes a **Detected** row under a new
"Detected on USB" section above "Open Devices":

- **A recognised open cutter** (a plotter or vinyl cutter Kerf has a preset for, matched by
  its USB-serial-bridge vendor id such as FTDI, CH340 or CP210x, or by an exact known
  machine). The row names the machine and offers **Set up**, which opens the existing device
  setup screen with the matching preset chosen and `port` pre-filled to the detected node.
  No new driving code: this is the spec 24 path with the port and preset filled in.
- **A Cricut machine** (USB vendor id `0x20D3`, "Cricut Inc"). The row names the machine (for
  example "Cricut Maker 3") and shows an inline notice, not a connect button: Kerf cannot
  send cuts to a Cricut machine because its protocol is encrypted and authenticated by
  Cricut's servers; use Cricut Design Space to cut, and use Kerf to design and to export SVG,
  DXF or PDF (spec 25). The notice links to nothing and promises nothing Kerf cannot do.

An unrecognised USB serial port keeps its current behaviour: it is available in the device
setup port picker, just not promoted to a Detected row.

## Data shape

- Extend `PortInfo` (`src/output/link.rs`) with the USB identity the `serialport` crate
  already exposes: `usb: Option<UsbId>` where `UsbId { vid: u16, pid: u16, serial:
  Option<String>, product: Option<String>, manufacturer: Option<String> }`. `SystemHost::ports`
  fills it from `serialport::SerialPortType::UsbPort`; other port types leave it `None`. This
  is observed data from the OS, so it is a plain struct, parsed at the `SystemHost` boundary.
- A new `src/output/machines.rs` holding `KnownMachine { name: &'static str, kind:
  MachineKind }` and `MachineKind { OpenCutter { preset: &'static str }, Cricut }`, plus
  `fn identify(port: &PortInfo) -> Option<KnownMachine>`. `OpenCutter.preset` names a
  `DeviceProfile::presets()` entry so the setup screen can select it. The table is the single
  source of recognition; the UI never matches ids itself.
- No new `Design` or `Shape` fields, so **no `FORMAT_VERSION` bump** and no project-format
  change. `DeviceProfile` is already persisted (spec 24); this spec does not change it.

## Files

- Owns: `src/output/machines.rs` (new), `src/ui/tests_spec58.rs` (new).
- Small edits: `src/output/link.rs` (the `PortInfo` field and the `SystemHost::ports` fill),
  `src/output/mod.rs` (`pub mod machines;` and any re-export), `src/ui/machine_modal.rs` (the
  Detected section), `src/ui/device_setup.rs` only if the preset-selection entry point needs a
  way to open with a named preset chosen. Keep each shared-file edit small and in one place.

## Acceptance

1. `PortInfo` carries USB vid, pid and serial when the OS reports a USB serial port, and
   `SystemHost::ports` fills it. A unit test over a constructed `serialport::SerialPortType`
   is not required, but `identify` has unit tests: a Cricut vid `0x20D3` returns a `Cricut`
   machine named from the product string; a known open-cutter bridge vid returns an
   `OpenCutter` with a preset that exists in `DeviceProfile::presets()`; an unknown vid returns
   `None`.
2. In the Select Machine modal, a fake `Host` returning a Maker-like `PortInfo` (vid `0x20D3`,
   product "Cricut Maker 3") shows a "Detected on USB" row naming "Cricut Maker 3" and an
   inline notice saying Kerf cannot cut to it and to use Design Space, with no connect or set
   up button on that row. Pinned by a committed snapshot; look at the PNG.
3. A fake `Host` returning a recognised open-cutter `PortInfo` shows a Detected row with a
   **Set up** button that opens device setup with the matching preset selected and the port
   pre-filled. A kittest drives the click and asserts the resulting profile's `port` and
   preset name.
4. An unrecognised USB port produces no Detected row and is still selectable in the device
   setup port picker (unchanged behaviour), asserted by a kittest or left to the existing
   spec 24 tests if they already cover it.
5. `docs/guide.md` gains a short paragraph under the Make or device section describing plug-in
   detection and the Cricut boundary. `README.md` Features gains one line.
6. `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test
   --locked` are clean.

## Out of scope

- Any attempt to open, handshake with, or send bytes to a Cricut machine. Detection reads the
  USB descriptor the OS already parsed; it does not open the Cricut port.
- Bluetooth detection. The Maker also exposes a Bluetooth node, but matching there needs a
  name table, not a USB descriptor; leave it to a later spec.
- Auto-connecting. Detection promotes a row; the maker still chooses Set up and Continue.
