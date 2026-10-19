//! Open cutters: turns a composed mat into HPGL or G-code and streams it to a machine over a serial
//! port, on its own thread. Nothing here touches a cutting machine's own protocol.
//!
//! The pipeline per mat: `toolpath::build` (millimetres on the work area) then `order::order`, then
//! `blade::compensate` (overcut and drag-knife offset), then `toolpath::with_passes`, then the
//! protocol's `emit` (origin, axis swap and machine units). `job` runs all of it; `mat_toolpath` runs
//! everything before `emit`, so the cut preview plays exactly the strokes `job` sends.

pub mod blade;
pub mod estimate;
pub mod gcode;
pub mod hpgl;
pub mod link;
pub mod machines;
pub mod order;
pub mod profile;
pub mod runner;
pub mod toolpath;

pub use profile::{DeviceProfile, Drive, Protocol, ToolKind};

use crate::composition::Mat;
use crate::design::Design;
use crate::make_flow::MatSetup;

pub const FLATTEN_TOLERANCE_MM: f32 = 0.05;

/// What the Make flow drives.
#[derive(Clone, Debug, Default, PartialEq)]
#[expect(clippy::large_enum_variant, reason = "one flow holds one, so boxing the profile saves nothing")]
pub enum OutputDevice {
    #[default]
    Simulator,
    Open(DeviceProfile),
}

/// The exact bytes for one mat, as lines, with the realtime bytes the runner may send around them.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub protocol: Protocol,
    /// Each is sent followed by `\n`.
    pub lines: Vec<String>,
    /// Estimated seconds the machine spends on each line, for pacing a port without flow control.
    pub seconds: Vec<f32>,
    /// Sent at once on Cancel, after streaming stops.
    pub stop: Vec<u8>,
    /// Realtime feed hold, when the protocol has one.
    pub pause: Option<Vec<u8>>,
    pub resume: Option<Vec<u8>>,
    /// Points where streaming stops for the user to swap tools, resuming on Resume.
    pub tool_changes: Vec<ToolChange>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolChange {
    /// The index in `lines` streaming stops before.
    pub before_line: usize,
    pub message: String,
}

impl Job {
    /// Every byte the port receives for a job that runs to the end: what the dry run writes.
    pub fn bytes(&self) -> Vec<u8> {
        self.lines.iter().flat_map(|l| l.bytes().chain(std::iter::once(b'\n'))).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeviceCommand {
    /// Sends only the identity queries; never motion.
    Test,
    Start(Job),
    Pause,
    Resume,
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeviceEvent {
    TestPassed(String),
    TestFailed(String),
    Progress {
        sent: usize,
        total: usize,
    },
    Paused,
    Resumed,
    /// Streaming stopped for a tool swap; Resume carries on.
    ToolChange(String),
    Done,
    Failed(String),
}

/// The bytes for one mat on one open device.
pub fn job(design: &Design, mat: &Mat, setup: &MatSetup, profile: &DeviceProfile, drive: Drive) -> Job {
    let path = mat_toolpath(design, mat, setup, Some(profile), drive.passes);
    match profile.protocol {
        Protocol::Hpgl => hpgl::emit(&path, profile, drive),
        Protocol::Grbl => gcode::emit(&path, profile, drive),
    }
}

/// One mat's strokes in machine order, each cut stroke `passes` times. An open device's are compensated
/// for its blade and overcut; the machine compensates its own, so `None` leaves them as drawn.
pub fn mat_toolpath(design: &Design, mat: &Mat, setup: &MatSetup, profile: Option<&DeviceProfile>, passes: u32) -> toolpath::Toolpath {
    let path = order::order(toolpath::build(design, mat, setup, FLATTEN_TOLERANCE_MM));
    let path = match profile {
        Some(p) => blade::compensate(&path, &p.tool, p.overcut_mm),
        None => path,
    };
    toolpath::with_passes(path, passes)
}

/// `12.5`, `3`, `-0.125`: three decimals, no trailing zeros, never `-0`.
pub fn number(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}
