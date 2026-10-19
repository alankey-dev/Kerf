//! The canvas grid (spec 46): its pitch, subdivisions, origin and visibility, the snap pitch it gives,
//! and which of its lines and ruler ticks show at a zoom. Pure: the canvas paints what this returns.

use serde::{Deserialize, Serialize};

use crate::design::{Design, POINTS_PER_INCH, Pt};
use crate::snap::SnapGrid;
use crate::units::Units;

/// Lines closer than this on screen are not drawn.
pub const MIN_GAP_PX: f32 = 4.0;
/// Ruler labels closer than this skip every other major line, then every fourth, and so on.
const LABEL_GAP_PX: f32 = 36.0;
pub const MAX_SUBDIVISIONS: u32 = 16;
const MIN_MAJOR_IN: f32 = 0.01;
const MAX_MAJOR_IN: f32 = 48.0;

/// Which grid lines the canvas draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridVisibility {
    Off,
    Major,
    #[default]
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridSettings {
    /// The pitch between major lines, in inches.
    pub major_in: f32,
    /// Lines per major pitch, 1 to [`MAX_SUBDIVISIONS`]; 1 draws major lines only.
    pub subdivisions: u32,
    /// Where the lines start, in inches from the artboard's top-left.
    #[serde(default)]
    pub origin_in: Pt,
    #[serde(default)]
    pub visible: GridVisibility,
}

/// A one-click grid: its name and the settings it fills in.
pub struct Preset {
    pub name: &'static str,
    pub major_in: f32,
    pub subdivisions: u32,
}

const MM: f32 = 1.0 / 25.4;

/// The Grid popover's presets. Each keeps a major line every inch or centimetre and subdivides it down
/// to the named pitch, so the named pitch is also the snap pitch.
pub const PRESETS: [Preset; 7] = [
    Preset { name: "1/8 in", major_in: 1.0, subdivisions: 8 },
    Preset { name: "1/4 in", major_in: 1.0, subdivisions: 4 },
    Preset { name: "1/2 in", major_in: 1.0, subdivisions: 2 },
    Preset { name: "1 in", major_in: 1.0, subdivisions: 1 },
    Preset { name: "1 mm", major_in: 10.0 * MM, subdivisions: 10 },
    Preset { name: "5 mm", major_in: 10.0 * MM, subdivisions: 2 },
    Preset { name: "1 cm", major_in: 10.0 * MM, subdivisions: 1 },
];

/// One grid line or ruler tick along an axis: where it falls in inches from the artboard edge, which
/// major pitch from the origin it sits in, and its place within it (0 on a major line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at_in: f32,
    pub major: i64,
    pub sub: u32,
}

/// What shows at one zoom: the major pitch after thinning and how many parts each is split into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub major_in: f32,
    pub per_major: u32,
}

/// `_getFullMultiplierForZoomLevel`: the major pitch doubles at each halving of the zoom.
pub fn multiplier(zoom: f32) -> f32 {
    match zoom {
        z if z < 1.0 / 8.0 => 16.0,
        z if z < 1.0 / 4.0 => 8.0,
        z if z < 1.0 / 2.0 => 4.0,
        z if z < 1.0 => 2.0,
        _ => 1.0,
    }
}

impl GridSettings {
    /// The grid a project draws until it is changed: 1 in in eighths, or 10 cm in centimetres.
    pub fn for_units(units: Units) -> Self {
        let (major_in, subdivisions) = match units {
            Units::Imperial => (1.0, 8),
            Units::Metric => (10.0 / Units::CM_PER_IN, 10),
        };
        Self { major_in, subdivisions, origin_in: (0.0, 0.0), visible: GridVisibility::Full }
    }

    /// The same grid with every number in range, whatever a file or a typed entry held.
    pub fn sane(self) -> Self {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        let major_in = if self.major_in.is_finite() { self.major_in.clamp(MIN_MAJOR_IN, MAX_MAJOR_IN) } else { 1.0 };
        Self { major_in, subdivisions: self.subdivisions.clamp(1, MAX_SUBDIVISIONS), origin_in: (finite(self.origin_in.0), finite(self.origin_in.1)), ..self }
    }

    /// The pitch a shape snaps to: one subdivision.
    pub fn snap_in(&self) -> f32 {
        self.major_in / self.subdivisions as f32
    }

    pub fn snap(&self) -> SnapGrid {
        SnapGrid { pitch_in: self.snap_in(), origin: self.origin_in }
    }

    /// Fills in a preset's pitch, keeping the origin and what shows.
    pub fn with_preset(self, preset: &Preset) -> Self {
        Self { major_in: preset.major_in, subdivisions: preset.subdivisions, ..self }
    }

    /// The pitch shown at `zoom`: the major pitch doubled as the zoom halves, its subdivisions dropped
    /// once they would fall closer than [`MIN_GAP_PX`]. `None` once the major lines would too.
    pub fn drawn(&self, zoom: f32) -> Option<Drawn> {
        let px = |inches: f32| inches * POINTS_PER_INCH * zoom;
        let major_in = self.major_in * multiplier(zoom);
        if px(major_in) < MIN_GAP_PX {
            return None;
        }
        let subdivided = self.subdivisions > 1 && px(major_in / self.subdivisions as f32) >= MIN_GAP_PX;
        Some(Drawn { major_in, per_major: if subdivided { self.subdivisions } else { 1 } })
    }

    /// The canvas lines at `zoom` along one axis whose `origin` is this axis's [`GridSettings::origin_in`],
    /// within `range` inches from the artboard edge: subdivisions only under Full, none while Off.
    pub fn lines(&self, zoom: f32, origin: f32, range: (f32, f32)) -> Vec<Mark> {
        let Some(drawn) = self.drawn(zoom) else { return Vec::new() };
        let drawn = match self.visible {
            GridVisibility::Off => return Vec::new(),
            GridVisibility::Major => Drawn { per_major: 1, ..drawn },
            GridVisibility::Full => drawn,
        };
        marks(drawn, origin, range)
    }

    /// What the rulers tick at `zoom`: the grid's pitch while it shows, else its major pitch doubled until
    /// the ticks have room, so a ruler always measures.
    pub fn ruler(&self, zoom: f32) -> Drawn {
        self.drawn(zoom).unwrap_or_else(|| {
            let mut major_in = self.major_in * multiplier(zoom);
            while major_in * POINTS_PER_INCH * zoom < MIN_GAP_PX {
                major_in *= 2.0;
            }
            Drawn { major_in, per_major: 1 }
        })
    }

    /// Whether the grid is meant to show but every line would fall closer than [`MIN_GAP_PX`].
    pub fn hidden_at(&self, zoom: f32) -> bool {
        self.visible != GridVisibility::Off && self.drawn(zoom).is_none()
    }
}

/// Every mark of `drawn` from `origin` that falls within `range`, both ends included.
pub fn marks(drawn: Drawn, origin: f32, (lo, hi): (f32, f32)) -> Vec<Mark> {
    let step = drawn.major_in / drawn.per_major as f32;
    let first = ((lo - origin) / step - 1e-4).ceil() as i64;
    let last = ((hi - origin) / step + 1e-4).floor() as i64;
    let per_major = i64::from(drawn.per_major);
    (first..=last).map(|k| Mark { at_in: origin + k as f32 * step, major: k.div_euclid(per_major), sub: k.rem_euclid(per_major) as u32 }).collect()
}

/// A ruler tick's start as a fraction of the ruler height: the major tick runs the full height, the half
/// way one is longer when the count is even, and with a count divisible by four so are the quarters.
pub fn tick(sub: u32, per_major: u32) -> f32 {
    let quarters = per_major.is_multiple_of(4);
    match sub {
        0 => 0.0,
        s if per_major.is_multiple_of(2) && s * 2 == per_major => 0.6,
        s if quarters && s.is_multiple_of(per_major / 4) => 0.712,
        _ if quarters => 0.825,
        _ => 0.8,
    }
}

/// How many major lines apart the ruler labels sit, so they never crowd: 1, 2, 4 and so on.
pub fn label_every(major_in: f32, zoom: f32) -> u32 {
    let px = major_in * POINTS_PER_INCH * zoom;
    let mut every = 1;
    while px * (every as f32) < LABEL_GAP_PX && every < 1 << 16 {
        every *= 2;
    }
    every
}

/// A ruler label: the distance from the artboard edge in `units`, without its unit, as today's whole
/// inches and tens of centimetres read ("1", "20"), and finer pitches to two decimals ("0.5").
pub fn label(units: Units, at_in: f32) -> String {
    let s = format!("{:.2}", units.from_in(at_in));
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

impl Design {
    /// The grid this design draws: its own once set, else `untouched`, the default it was first shown with.
    pub fn grid_or(&self, untouched: GridSettings) -> GridSettings {
        self.grid.map_or(untouched, GridSettings::sane)
    }

    /// Whether the magnet reads on: any snap class is on.
    pub fn snapping(&self) -> bool {
        self.snap.any()
    }

    /// The magnet's click: every snap class on or every one off. The angle step is left alone.
    pub fn set_snapping(&mut self, on: bool) {
        self.snap.set_all(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line positions the canvas drew before this spec, in screen points from the artboard edge:
    /// `marks` stepping the hard-coded minor pitch and skipping the edge, majors kept apart.
    fn before(units: Units, zoom: f32, extent_in: f32) -> (Vec<f32>, Vec<f32>) {
        let major_pt = match units {
            Units::Imperial => POINTS_PER_INCH,
            Units::Metric => POINTS_PER_INCH / Units::CM_PER_IN * 10.0,
        } * multiplier(zoom);
        let per_major = match units {
            Units::Imperial => 8,
            Units::Metric => 10,
        };
        let minor_pt = major_pt / per_major as f32;
        let last = (extent_in * POINTS_PER_INCH / minor_pt).floor() as u32;
        let (mut minor, mut major) = (Vec::new(), Vec::new());
        for i in 1..=last {
            let px = i as f32 * minor_pt * zoom;
            if i.is_multiple_of(per_major) { major.push(px) } else { minor.push(px) }
        }
        (minor, major)
    }

    fn after(units: Units, zoom: f32, extent_in: f32) -> (Vec<f32>, Vec<f32>) {
        let lines = GridSettings::for_units(units).lines(zoom, 0.0, (0.0, extent_in));
        let px = |m: &Mark| m.at_in * POINTS_PER_INCH * zoom;
        let kept = lines.iter().filter(|m| m.at_in > 1e-4);
        (kept.clone().filter(|m| m.sub != 0).map(px).collect(), kept.filter(|m| m.sub == 0).map(px).collect())
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn the_default_grid_draws_todays_lines_in_both_unit_systems() {
        for units in [Units::Imperial, Units::Metric] {
            for extent in [12.0, 24.0, 8.5] {
                let (old_minor, old_major) = before(units, 1.0, extent);
                let (minor, major) = after(units, 1.0, extent);
                assert!(close(&old_minor, &minor), "{units:?} minor lines over {extent} inches");
                assert!(close(&old_major, &major), "{units:?} major lines over {extent} inches");
                assert!(!major.is_empty());
            }
        }
        let (old_minor, old_major) = before(Units::Imperial, 0.25, 12.0);
        let (minor, major) = after(Units::Imperial, 0.25, 12.0);
        assert!(close(&old_minor, &minor) && close(&old_major, &major), "and as the zoom thins them");
    }

    #[test]
    fn a_5mm_grid_from_half_an_inch_snaps_an_edge_to_its_line() {
        use crate::snap::{Candidate, Kind, SnapOptions, Targets, snap};
        let grid = GridSettings { major_in: 5.0 * MM, subdivisions: 1, origin_in: (0.5, 0.5), visible: GridVisibility::Full };
        let edge = 0.5 + 5.1 * MM;
        let candidate = Candidate { points: vec![((edge, 3.0), Kind::Edge)], frame: None };
        let targets = Targets { grid: Some(grid.snap()), threshold: 0.1, ..Targets::default() };
        let s = snap(&candidate, &targets, &SnapOptions::default());
        assert!((edge + s.dx - (0.5 + 5.0 * MM)).abs() < 1e-5, "edge now {}", edge + s.dx);
    }

    #[test]
    fn at_10_percent_a_1mm_grid_draws_majors_only_at_least_4px_apart() {
        for subdivisions in [1, 10] {
            let grid = GridSettings { major_in: MM, subdivisions, origin_in: (0.0, 0.0), visible: GridVisibility::Full };
            let lines = grid.lines(0.1, 0.0, (0.0, 12.0));
            assert!(!lines.is_empty() && lines.iter().all(|m| m.sub == 0), "{subdivisions} subdivisions: majors only");
            let gaps = lines.windows(2).map(|w| (w[1].at_in - w[0].at_in) * POINTS_PER_INCH * 0.1);
            assert!(gaps.into_iter().all(|g| g >= MIN_GAP_PX - 1e-3));
        }
        let fine = GridSettings { major_in: MM, subdivisions: 1, ..GridSettings::for_units(Units::Metric) };
        assert!(fine.lines(1.0, 0.0, (0.0, 12.0)).is_empty() && fine.hidden_at(1.0), "1 mm at 100 % is under 4 px");
        assert!(!GridSettings::for_units(Units::Imperial).hidden_at(0.1));
    }

    #[test]
    fn the_origin_shifts_every_line_and_off_draws_none() {
        let grid = GridSettings { origin_in: (0.5, 0.0), ..GridSettings::for_units(Units::Imperial).with_preset(&PRESETS[3]) };
        let at: Vec<f32> = grid.lines(1.0, 0.5, (0.0, 3.0)).iter().map(|m| m.at_in).collect();
        assert_eq!(at, [0.5, 1.5, 2.5]);
        assert!(GridSettings { visible: GridVisibility::Off, ..grid }.lines(1.0, 0.5, (0.0, 3.0)).is_empty());
        let major = GridSettings { visible: GridVisibility::Major, ..GridSettings::for_units(Units::Imperial) };
        assert!(major.lines(1.0, 0.0, (0.0, 3.0)).iter().all(|m| m.sub == 0));
    }

    #[test]
    fn ticks_follow_the_subdivision_count() {
        assert_eq!((0..8).map(|s| tick(s, 8)).collect::<Vec<_>>(), [0.0, 0.825, 0.712, 0.825, 0.6, 0.825, 0.712, 0.825], "eighths as before");
        assert_eq!((0..4).map(|s| tick(s, 4)).collect::<Vec<_>>(), [0.0, 0.712, 0.6, 0.712]);
        assert_eq!((0..3).map(|s| tick(s, 3)).collect::<Vec<_>>(), [0.0, 0.8, 0.8]);
        assert_eq!(tick(5, 10), 0.6, "the half way centimetre tick is longer");
    }

    #[test]
    fn labels_read_as_before_and_thin_when_crowded() {
        assert_eq!(label(Units::Imperial, 3.0), "3");
        assert_eq!(label(Units::Metric, 10.0 / Units::CM_PER_IN * 2.0), "20");
        assert_eq!(label(Units::Metric, 0.5 + 5.0 * MM), "1.77");
        assert_eq!(label_every(1.0, 1.0), 1);
        assert_eq!(label_every(5.0 * MM, 1.0), 4, "14 px apart: every fourth");
    }

    #[test]
    fn a_crafted_grid_is_brought_into_range() {
        let wild = GridSettings { major_in: f32::NAN, subdivisions: 0, origin_in: (f32::INFINITY, 2.0), visible: GridVisibility::Major };
        assert_eq!(wild.sane(), GridSettings { major_in: 1.0, subdivisions: 1, origin_in: (0.0, 2.0), visible: GridVisibility::Major });
        assert_eq!(GridSettings { subdivisions: 99, ..wild }.sane().subdivisions, MAX_SUBDIVISIONS);
    }

    #[test]
    fn a_design_follows_the_units_until_its_grid_is_set() {
        let mut d = Design::default();
        assert_eq!(d.grid_or(GridSettings::for_units(Units::Metric)), GridSettings::for_units(Units::Metric));
        let quarter = GridSettings::for_units(Units::Imperial).with_preset(&PRESETS[1]);
        d.grid = Some(quarter);
        assert_eq!(d.grid_or(GridSettings::for_units(Units::Metric)), quarter, "a set grid keeps its pitch in centimetres");
        assert!(d.snapping());
        d.snap.grid = false;
        assert!(d.snapping(), "objects still snap");
        d.set_snapping(false);
        assert!(!d.snapping());
    }
}
