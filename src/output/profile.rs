//! A saved description of one open machine: how to reach it, how its axes run, and what moves its head.

use serde::{Deserialize, Serialize};

use crate::material::{Material, PressureAdjust};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Hpgl,
    Grbl,
}

impl Protocol {
    pub const ALL: [Protocol; 2] = [Self::Hpgl, Self::Grbl];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hpgl => "HPGL",
            Self::Grbl => "GRBL G-code",
        }
    }

    /// The dry-run file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Hpgl => "hpgl",
            Self::Grbl => "gcode",
        }
    }
}

/// The corner of the work area where the machine's (0, 0) sits. Its axes point into the work area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Origin {
    pub const ALL: [Origin; 4] = [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight];

    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "top left",
            Self::TopRight => "top right",
            Self::BottomLeft => "bottom left",
            Self::BottomRight => "bottom right",
        }
    }

    /// A point in millimetres from the work area's top-left, y down, measured from this corner instead.
    pub fn place(self, (x, y): (f32, f32), (w, h): (f32, f32)) -> (f32, f32) {
        match self {
            Self::TopLeft => (x, y),
            Self::TopRight => (w - x, y),
            Self::BottomLeft => (x, h - y),
            Self::BottomRight => (w - x, h - y),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToolKind {
    DragKnife {
        offset_mm: f32,
        swivel_threshold_deg: f32,
    },
    /// GRBL commands that lower and raise the pen, and how long the pen takes to settle after each.
    Pen {
        up: String,
        down: String,
        settle_ms: u32,
    },
    /// `S0..=power_max`; dynamic power (`M4`) scales with speed, constant power (`M3`) does not.
    Laser {
        power_max: u32,
        dynamic: bool,
    },
}

impl ToolKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DragKnife { .. } => "Drag knife",
            Self::Pen { .. } => "Pen",
            Self::Laser { .. } => "Laser",
        }
    }

    pub fn load_line(&self) -> &'static str {
        match self {
            Self::DragKnife { .. } => "Load the drag knife",
            Self::Pen { .. } => "Load the pen",
            Self::Laser { .. } => "Focus the laser",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub id: u64,
    pub name: String,
    pub protocol: Protocol,
    pub baud: u32,
    /// Machine units per millimetre: 40 for HPGL (1016 per inch), 1 for GRBL in millimetres.
    pub units_per_mm: f32,
    pub origin: Origin,
    /// Roll cutters feed the material along their own X.
    pub swap_xy: bool,
    /// Width and height; the Make flow composes mats at this size.
    pub work_area_mm: (f32, f32),
    pub tool: ToolKind,
    /// Slowest and fastest, mapped from `Material::cut_speed`.
    pub speed_mm_s: (f32, f32),
    /// Weakest and strongest HPGL `FS`, mapped from `Material::pressure`. `None` leaves the cutter's own.
    pub force: Option<(u32, u32)>,
    /// How far a closed cut runs past its start. 0 disables.
    pub overcut_mm: f32,
    /// The last port that passed a connection test.
    pub port: Option<String>,
    /// Software flow control on the port. Without it, HPGL is paced by an estimated drain time.
    #[serde(default)]
    pub xon_xoff: bool,
    /// HPGL only: sent by the connection test to ask the cutter who it is.
    #[serde(default)]
    pub identify: Option<String>,
    /// HPGL only: sent after `PU;` when a job is cancelled.
    #[serde(default)]
    pub abort: String,
    /// HPGL only: stop for a tool change before Pen strokes and again before the knife.
    #[serde(default)]
    pub pen_change: bool,
}

pub const MM_PER_IN: f32 = 25.4;

/// The material pressure that maps onto the top of a profile's force or power range: the strongest
/// material in `material::MATERIALS`.
pub const PRESSURE_FULL_SCALE: f32 = 1500.0;

/// The share of cut power a laser uses for Pen, Score and Engrave.
pub const LIGHT_POWER: f32 = 0.3;

/// What one mat runs at on one profile, after More or Less.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drive {
    pub speed_mm_s: f32,
    /// HPGL `FS`, when the profile has a force range.
    pub force: Option<u32>,
    /// Laser `S` for cuts, when the tool is a laser. Pen, Score and Engrave use `LIGHT_POWER` of it.
    pub power: Option<u32>,
    /// How many times each cut stroke runs.
    pub passes: u32,
}

impl Drive {
    /// "Speed 146 mm/s, Force 98".
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("Speed {} mm/s", self.speed_mm_s.round())];
        if let Some(f) = self.force {
            parts.push(format!("Force {f}"));
        }
        if let Some(p) = self.power {
            parts.push(format!("Power {p}"));
        }
        if self.passes > 1 {
            parts.push(format!("{} passes", self.passes));
        }
        parts.join(", ")
    }
}

impl DeviceProfile {
    /// Speed is linear in `cut_speed` (0 to 10) over the speed range; force and laser power are linear in
    /// the adjusted pressure over `0..PRESSURE_FULL_SCALE`, clamped to their range.
    pub fn drive(&self, material: &Material, adjust: PressureAdjust) -> Drive {
        let (lo, hi) = self.speed_mm_s;
        let speed_mm_s = lo + (hi - lo) * (material.cut_speed.min(10) as f32) / 10.0;
        let share = (material.adjusted_pressure(adjust) as f32 / PRESSURE_FULL_SCALE).clamp(0.0, 1.0);
        let along = |(lo, hi): (u32, u32)| (lo as f32 + (hi as f32 - lo as f32) * share).round() as u32;
        let power = match self.tool {
            ToolKind::Laser { power_max, .. } => Some(along((0, power_max))),
            _ => None,
        };
        Drive { speed_mm_s, force: self.force.map(along), power, passes: material.multi_pass.max(1) }
    }

    pub fn work_area_in(&self) -> (f32, f32) {
        (self.work_area_mm.0 / MM_PER_IN, self.work_area_mm.1 / MM_PER_IN)
    }

    pub fn is_laser(&self) -> bool {
        matches!(self.tool, ToolKind::Laser { .. })
    }

    pub fn presets() -> [DeviceProfile; 3] {
        [Self::hpgl_vinyl_cutter(), Self::grbl_laser(), Self::grbl_pen_plotter()]
    }

    pub fn hpgl_vinyl_cutter() -> Self {
        Self {
            id: 0,
            name: "HPGL vinyl cutter".into(),
            protocol: Protocol::Hpgl,
            baud: 9600,
            units_per_mm: 40.0,
            origin: Origin::BottomRight,
            swap_xy: true,
            work_area_mm: (600.0, 1000.0),
            tool: ToolKind::DragKnife { offset_mm: 0.25, swivel_threshold_deg: 30.0 },
            speed_mm_s: (50.0, 400.0),
            force: Some((20, 400)),
            overcut_mm: 1.0,
            port: None,
            xon_xoff: false,
            identify: Some("OI;".into()),
            abort: String::new(),
            pen_change: false,
        }
    }

    pub fn grbl_laser() -> Self {
        Self {
            id: 0,
            name: "GRBL laser".into(),
            protocol: Protocol::Grbl,
            baud: 115_200,
            units_per_mm: 1.0,
            origin: Origin::BottomLeft,
            swap_xy: false,
            work_area_mm: (400.0, 400.0),
            tool: ToolKind::Laser { power_max: 1000, dynamic: true },
            speed_mm_s: (5.0, 50.0),
            force: None,
            overcut_mm: 0.0,
            port: None,
            xon_xoff: false,
            identify: None,
            abort: String::new(),
            pen_change: false,
        }
    }

    pub fn grbl_pen_plotter() -> Self {
        Self {
            id: 0,
            name: "GRBL pen plotter".into(),
            protocol: Protocol::Grbl,
            baud: 115_200,
            units_per_mm: 1.0,
            origin: Origin::TopLeft,
            swap_xy: false,
            work_area_mm: (300.0, 218.0),
            tool: ToolKind::Pen { up: "M5".into(), down: "M3 S30".into(), settle_ms: 150 },
            speed_mm_s: (10.0, 100.0),
            force: None,
            overcut_mm: 0.0,
            port: None,
            xon_xoff: false,
            identify: None,
            abort: String::new(),
            pen_change: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::MATERIALS;

    fn material(name: &str) -> &'static Material {
        MATERIALS.iter().find(|m| m.name == name).unwrap()
    }

    #[test]
    fn cardstock_speed_maps_linearly_over_the_profile_range() {
        let profile = DeviceProfile { speed_mm_s: (20.0, 200.0), ..DeviceProfile::hpgl_vinyl_cutter() };
        assert_eq!(material("Cardstock").cut_speed, 7);
        assert_eq!(profile.drive(material("Cardstock"), PressureAdjust::Default).speed_mm_s, 146.0);
    }

    #[test]
    fn more_raises_force_by_the_materials_delta_on_the_force_scale() {
        let profile = DeviceProfile { force: Some((0, 1500)), ..DeviceProfile::hpgl_vinyl_cutter() };
        let m = material("Cardstock");
        let force = |adjust| profile.drive(m, adjust).force.unwrap();
        assert_eq!(force(PressureAdjust::Default), 300, "a 0 to 1500 range reads the pressure as is");
        assert_eq!(force(PressureAdjust::More) - force(PressureAdjust::Default), m.delta_adjustment[2] as u32);
        assert_eq!(force(PressureAdjust::Default) - force(PressureAdjust::Less), m.delta_adjustment[2] as u32);
    }

    #[test]
    fn force_and_laser_power_clamp_to_their_range() {
        let chipboard = material("Chipboard");
        let cutter = DeviceProfile { force: Some((20, 400)), ..DeviceProfile::hpgl_vinyl_cutter() };
        assert_eq!(cutter.drive(chipboard, PressureAdjust::More).force, Some(400));
        assert_eq!(DeviceProfile::grbl_laser().drive(chipboard, PressureAdjust::More).power, Some(1000));
        assert_eq!(DeviceProfile::grbl_laser().drive(material("Cardstock"), PressureAdjust::Default).power, Some(200));
        assert_eq!(DeviceProfile::grbl_pen_plotter().drive(material("Cardstock"), PressureAdjust::Default).power, None);
    }

    #[test]
    fn origins_measure_from_their_own_corner() {
        let area = (100.0, 50.0);
        assert_eq!(Origin::TopLeft.place((10.0, 5.0), area), (10.0, 5.0));
        assert_eq!(Origin::TopRight.place((10.0, 5.0), area), (90.0, 5.0));
        assert_eq!(Origin::BottomLeft.place((10.0, 5.0), area), (10.0, 45.0));
        assert_eq!(Origin::BottomRight.place((10.0, 5.0), area), (90.0, 45.0));
    }

    #[test]
    fn a_profile_round_trips_through_json() {
        let profile = DeviceProfile { id: 7, port: Some("/dev/cu.usbserial-1".into()), ..DeviceProfile::grbl_pen_plotter() };
        let json = serde_json::to_string(&profile).unwrap();
        assert_eq!(serde_json::from_str::<DeviceProfile>(&json).unwrap(), profile);
    }
}
