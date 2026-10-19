# 24 Open cutters

**Wave 5. Depends on 09, 16, 18. Size XL.** Requested directly by the user: cut on real machines without a proprietary cloud key.

## Problem

The Make flow can only drive `SimulatedMachine`. Every step speaks the simulator's bridge protocol: `MakeFlow` in `src/make_flow.rs` returns `PbCommonBridge` messages, `Kerf::send_all` in `src/ui/mod.rs` pushes them through `Box<dyn Transport>`, and `bridge::build_mat_path_data` turns each composed `Mat` into the simulator's path data. A real cloud-locked machine cannot be driven this way. Its command session is AES-encrypted with a master key that only the vendor's server hands out (`docs/real-device-feasibility.md` sections 2 and 3), so Kerf will not go there.

Plenty of open machines need no key at all. Vinyl cutters take HPGL over a USB serial port. GRBL boards drive diode lasers and pen plotters from G-code. Kerf already composes mats (`composition::compose`) and lays them out (`bridge::mat_placements`). Nothing turns that layout into motion for those machines.

## Goal

The Make flow targets an **output device**. The simulated cutter stays exactly as it is. Next to it the user can add open devices: an HPGL vinyl cutter or a GRBL G-code laser or pen plotter, each described by a saved profile and reached over a serial port. The same Prepare, Set, Load, Go, Cutting and Done screens drive them. A dry run writes the exact bytes to a file instead of the port.

Nothing in this spec touches a cutting machine's own protocol, its encryption or its key material.

## Data shape

New module tree `src/output/`. The architect owns the final signatures; this is the shape to start from.

```rust
// src/output/profile.rs
pub enum Protocol { Hpgl, Grbl }

pub struct DeviceProfile {
    pub id: u64,
    pub name: String,                     // "Vinyl cutter", "Desk laser"
    pub protocol: Protocol,
    pub baud: u32,                         // 9600 for most HPGL cutters, 115200 for GRBL
    pub units_per_mm: f32,                 // 40.0 for HPGL (1016 per inch), 1.0 for GRBL in mm
    pub origin: Origin,                    // TopLeft, TopRight, BottomLeft, BottomRight on the work area
    pub swap_xy: bool,                     // roll cutters feed along their own X
    pub work_area_mm: (f32, f32),          // composes mats at this size
    pub tool: ToolKind,                    // what moves the head
    pub speed_mm_s: (f32, f32),            // min and max, mapped from Material::cut_speed
    pub force: Option<(u32, u32)>,         // min and max HPGL FS, mapped from Material::pressure
    pub overcut_mm: f32,                   // 0 disables
    pub port: Option<String>,              // the last port that passed a connection test
}

pub enum ToolKind {
    DragKnife { offset_mm: f32, swivel_threshold_deg: f32 },
    Pen { up: String, down: String, settle_ms: u32 },          // GRBL: "M5" / "M3 S1000", or "G0 Z5" / "G1 Z0 F600"
    Laser { power_max: u32, dynamic: bool },                    // S0..power_max, M4 when dynamic, M3 otherwise
}

// src/output/toolpath.rs
pub struct Toolpath { pub strokes: Vec<Stroke> }                // machine order, in mm on the work area
pub struct Stroke { pub op: Operation, pub points: Vec<(f32, f32)>, pub closed: bool }

pub fn build(design: &Design, mat: &Mat, setup: &MatSetup, tol_mm: f32) -> Toolpath;  // via bridge::mat_placements
pub fn order(path: Toolpath) -> Toolpath;                                              // inside-out, then nearest neighbour
pub fn compensate(path: &Toolpath, tool: &ToolKind, overcut_mm: f32) -> Toolpath;     // blade offset and overcut

// src/output/hpgl.rs and src/output/gcode.rs
pub fn emit(path: &Toolpath, profile: &DeviceProfile, material: &Material) -> Job;

pub struct Job { pub lines: Vec<String>, pub stop: Vec<u8>, pub pause: Option<Vec<u8>>, pub resume: Option<Vec<u8>> }
```

`build` reads geometry through `bridge::mat_placements`, so copies and Mirror land exactly where the Prepare preview and the simulator payload put them. Coordinates convert from points to millimetres once, then the profile's origin, axis swap and `units_per_mm` apply in `emit`. Nothing downstream of `build` knows about inches or points.

## Rules

1. **Output device.** `MakeFlow` gains `device: OutputDevice`, where `enum OutputDevice { Simulator, Open(DeviceProfile) }`. `Simulator` keeps today's behaviour and today's bridge messages byte for byte. The flow's outbox becomes `Vec<Effect>` with `enum Effect { Bridge(PbCommonBridge), Device(DeviceCommand) }`. `Kerf::send_all` sends `Bridge` to the existing `Transport` and `Device` to the open-device runner. Inbound runner events reach `MakeFlow::on_device_event`, the counterpart of `on_message`.
2. **Runner thread.** `src/output/runner.rs` owns the serial port on its own thread, like `SimulatedMachine` owns the fake machine. The UI and the runner share no state. Two `mpsc` channels carry `DeviceCommand { Test, Start(Job), Pause, Resume, Cancel }` one way and `DeviceEvent { TestPassed(String), TestFailed(String), Progress { sent, total }, Paused, Resumed, Done, Failed(String) }` the other. The UI never blocks on the port.
3. **Serial link.** `trait SerialLink { fn write(&mut self, bytes: &[u8]) -> io::Result<()>; fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>>; }`. Three implementations: `PortLink` over the `serialport` crate, `FakeLink` for tests (records every byte, scripted replies), and `FileLink` for the dry run. The runner only sees the trait.
4. **Flow control.** GRBL streams one line at a time and waits for `ok`. `error:N` or `ALARM:N` stops the job and reports the code with GRBL's meaning. HPGL streams in chunks of at most 256 bytes. If the profile enables XON/XOFF, the port honours it. Otherwise the runner paces chunks by an estimated drain time from the move lengths and the profile speed.
5. **Port picker and connection test.** The machine modal (`src/ui/machine_modal.rs`) lists the simulated cutter first, then each saved profile, then **Add device**. Choosing a profile opens `src/ui/device_setup.rs`. It shows a port dropdown from `serialport::available_ports()` with USB vendor and product names where the OS reports them, a refresh button and **Test connection**. A GRBL test opens the port, sends a bare newline and waits up to 2 s for the `Grbl` banner, or sends `$I` if no banner arrives. An HPGL test opens the port and, when the profile has an identify query (`OI;` by default), waits up to 2 s for a reply. A cutter that never answers passes with the note "Port opened; this cutter does not report an identity". A test never sends a motion command.
6. **Profiles in settings.** `Settings` in `src/settings.rs` gains `#[serde(default)] devices: Vec<DeviceProfile>` and `last_device: Option<u64>`. That drops `Copy` from `Settings`; callers clone. `src/ui/device_setup.rs` edits every profile field, with lengths shown and typed through `src/units.rs`. Presets: "HPGL vinyl cutter" (9600 baud, 40 units/mm, drag knife 0.25 mm, overcut 1 mm), "GRBL laser" (115200 baud, laser 1000), "GRBL pen plotter" (115200 baud, servo pen `M3 S30` down, `M5` up).
7. **Composition at the device's size.** For an open device, `compose` runs at `MatSize { imperial_name: "Work area", metric_name: "Work area", width_in, height_in, preferred: true, matless: true }` from the profile's work area. Prepare hides Material Load Type for open devices. Copies and Mirror stay.
8. **Material mapping.** Set lists the same materials. `Material::cut_speed` runs up to 10. Speed is `min + (max - min) * cut_speed / 10` over the profile's `speed_mm_s` range. `Material::pressure` maps linearly onto `force` when the profile has one, clamped. Laser power uses the same pressure mapping onto `0..power_max`. `multi_pass` repeats the cut strokes of the mat, not the draw strokes. The Set step shows the derived values ("Speed 120 mm/s, Force 85"), and More and Less move them by the material's `delta_adjustment` as they do for the simulator.
9. **Operations per device.** HPGL draws every operation with the knife, except Pen, which pauses for a tool change only when the profile says so. GRBL laser maps Pen, Score and Engrave to 30 % power and cuts at full mapped power. GRBL pen plotter draws every operation with the pen. Print Then Cut mats are skipped for open devices with the notice "Print Then Cut needs a registration step this device does not have" (spec 27 tracks the open question).
10. **Ordering.** `order` runs in this sequence, all in `src/output/order.rs`:
    - Group strokes by plot order (`composition::plot_order`, made `pub(crate)`), so draws finish before cuts.
    - Inside-out. Build a containment tree over closed strokes with `geometry` point-in-polygon on each ring's first point. A stroke is cut only after every stroke it contains. Letter counters go before letters, and letters go before the card they sit on.
    - Nearest neighbour. Among the strokes whose children are done, take the one with the closest entry point to the current head position, starting from the origin. For a closed stroke, rotate its ring to start at the vertex closest to the head. Open strokes may be reversed.
11. **Blade offset.** For `ToolKind::DragKnife`, `compensate` shifts every point forward along its travel direction by `offset_mm`. At a corner sharper than `swivel_threshold_deg`, it inserts an arc of radius `offset_mm` around the true corner, so the trailing tip turns on the line instead of rounding it. Closed strokes start with a short lead-in so the blade aligns before it enters. It is pure and works on millimetres.
12. **Overcut.** A closed cut stroke continues `overcut_mm` along its own first segments past the start point, so the cut closes cleanly. Draw strokes never overcut.
13. **Emitters.** `hpgl::emit` writes `IN;`, `VS`, `FS` (when the profile has force), then `PU x,y;` and `PD x,y,...;` per stroke, and ends with `PU;` and a park move to the origin. `gcode::emit` writes `G21` and `G90`, then `G0` travel and `G1 ... F` moves. It puts the profile's pen down and up commands around each stroke, waits `G4 P` for `settle_ms`, and uses `M3`/`M4 S` and `M5` for lasers. It ends with tool up, `M5` and `G0` to the origin. Numbers are written to 3 decimal places with no trailing zeros, so golden files are stable.
14. **Safety.**
    - Never send motion before Go. Connecting and testing send only the identity queries in rule 5. The runner accepts `Start(Job)` only from `MakeFlow::press_go`, and only once the step is Go.
    - Pause sends the device's pause bytes. For GRBL that is feed hold `!`, and Resume sends `~`. HPGL has no standard pause, so Pause stops streaming and the Cutting screen says "The cutter finishes the moves it has already received".
    - Cancel stops streaming at once and sends `Job::stop`. For GRBL that is `!`, then `0x18` soft reset, then `M5`. For HPGL it is `PU;` plus the profile's extra abort string, if any.
    - Closing the Make flow or the app while a job runs sends Cancel first.
    - A laser profile shows a notice on the Go step: "Laser: wear eye protection rated for this wavelength and never leave the machine running unattended."
    - Kerf never sends `$H` homing or `$X` alarm unlock on its own.
15. **Dry run.** The Go step has **Save to file** beside Go for open devices. It runs the same `Job` through `FileLink` to a path from a save dialog (`<project> - Mat 1.hpgl` or `.gcode`). The dry run and the port see identical bytes.
16. **Load step.** For open devices the Load step lists the profile's tool ("Load the drag knife", "Load the pen", "Focus the laser") and "Place the material at the origin, top left", with the corner taken from the profile's origin. No Load/Unload button message is sent. The user presses Continue.
17. **Simulator unchanged.** `SimulatedMachine`, `device::reduce`, the `Transport` trait, `bridge.rs` and every existing simulator test stay as they are. `OutputDevice::Simulator` is the default, so the existing kittest route through the machine modal still passes.

## Files

- **Owns.** `src/output/mod.rs`, `src/output/profile.rs`, `src/output/toolpath.rs`, `src/output/order.rs`, `src/output/blade.rs`, `src/output/hpgl.rs`, `src/output/gcode.rs`, `src/output/link.rs`, `src/output/runner.rs` (all new), `src/ui/device_setup.rs` (new), `src/ui/tests_spec24.rs` (new), `tests/golden/24/*.hpgl` and `tests/golden/24/*.gcode` (new).
- **Shared edits.**
  - `Cargo.toml`: add `serialport = "4"`.
  - `src/main.rs`: add `mod output`.
  - `src/make_flow.rs`: add `device: OutputDevice`, `Effect`, `on_device_event`, and the open-device branches of `press_go`, `press_pause` and `cancel`.
  - `src/ui/mod.rs`: make `Outbox` a `Vec<Effect>`, add a runner handle beside `transport`, and add the `send_all` dispatch.
  - `src/ui/machine_modal.rs`: the profile list and Add device.
  - `src/ui/set_load_go.rs`: derived values on Set, the open-device Load lines, Save to file and the laser notice on Go.
  - `src/ui/prepare.rs`: hide Load Type for open devices.
  - `src/settings.rs`: `devices` and `last_device`.
  - `src/composition.rs`: make `plot_order` `pub(crate)`.
  - `README.md`: a new "Open cutters" section and rows in the module table.

## Acceptance

- [ ] Unit test: `build` for a 2 in square on a 12 x 12 mat gives one closed stroke of 50.8 mm sides at the packed offset, and with Mirror the stroke mirrors across the mat width, matching `bridge::mat_placements`.
- [ ] Unit test: `order` puts the inner ring of the ring shape and each counter of the text "BOA" before the ring or letter that contains it, and puts a card's outline after everything on it.
- [x] Unit test: nearest-neighbour ordering of four squares in a 2 x 2 grid, starting at the origin, visits them in a U order (closed strokes return to their start, rule 10, so the bottom-right corner is nearer than the bottom-left). Travel is under half of canvas order on a shuffled layout.
- [ ] Unit test: a closed ring's start rotates to the vertex nearest the head, and an open stroke reverses when its end is closer.
- [ ] Unit test: `compensate` with a 0.25 mm drag knife adds a 0.25 mm radius arc at each corner of a square and none on a circle flattened to 64 segments (every turn below the threshold).
- [ ] Unit test: a 1 mm overcut extends a closed square 1 mm past its start along the first edge. A Pen stroke is not extended.
- [ ] Unit test: material mapping. Cardstock (`cut_speed` 7, pressure 300) on a profile with speed 20 to 200 mm/s gives 146 mm/s, and More raises force by the material's delta.
- [ ] Golden test: `hpgl::emit` for a 2 in square with overcut 1 mm and a 0.25 mm knife matches `tests/golden/24/square-knife.hpgl`.
- [ ] Golden test: `gcode::emit` for the same square on a pen plotter profile matches `tests/golden/24/square-pen.gcode`. On a laser profile it matches `tests/golden/24/square-laser.gcode`, which uses `M4`, `S` power and ends with `M5`.
- [ ] Golden test: a heart with a pen star attached, on a pen plotter, draws the star before cutting the heart (`tests/golden/24/heart-star.gcode`).
- [ ] Unit test: the GRBL runner on `FakeLink` waits for `ok` before each line, and stops with `Failed("error:20 Unsupported command")` on a scripted error.
- [ ] Unit test: `Cancel` mid-job on GRBL writes `!`, `0x18`, then `M5`, and no motion line after them. `Pause` writes `!` and `Resume` writes `~`.
- [ ] Unit test: Test connection on `FakeLink` records only the identity query bytes, with no `G0`, `G1`, `PU` or `PD`.
- [ ] Unit test: profiles round-trip through `Settings::save` and `Settings::load`, and an old `settings.json` with only `units` loads with no devices.
- [ ] Kittest: add a GRBL pen plotter profile against `FakeLink`, pass the connection test, then Make a two-colour design through Set, Load and Go. The fake link receives the golden G-code for each mat, with Next Mat in between, and the flow reaches Done.
- [ ] Kittest: on the Go step, no bytes beyond the identity query have reached `FakeLink` before Go is pressed.
- [ ] Kittest: Save to file writes the same bytes the fake port receives for the same mat.
- [ ] Kittest: the existing simulator route (Connect with USB through Back to canvas) still passes unchanged.
- [ ] Snapshots `24-device-setup.png` (port dropdown, test passed), `24-machine-modal.png` (simulator plus two profiles) and `24-go-laser.png` (laser notice and Save to file).
- [ ] README describes open cutters, profiles, the port test, ordering, blade offset, dry run and the safety rules.

## Out of scope

- **AxiDraw.** The AxiDraw speaks the EiBotBoard command set (`SM`, `SP`), not G-code, so it needs its own emitter. The `SerialLink` and `Job` shapes take it without change. GRBL-flashed AxiDraw-style plotters work today through the GRBL pen plotter preset. A native EBB emitter is a follow-up spec.
- **Cloud-locked machines.** No real proprietary transport, handshake or path encryption. See `docs/real-device-feasibility.md`.
- **Registration marks** for print-then-cut on open devices. Tracked as an open question in spec 27.
