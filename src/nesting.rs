//! Nesting (spec 30): packs pieces onto mats as tightly as their real outlines allow, by raster
//! bottom-left fill. Each piece in each allowed rotation is rasterised once, dilated by half the
//! spacing, onto a grid of `u64` rows; placements are whole cells of that grid, and the exact offsets
//! and rotations come back in inches, so cut paths are never rasterised. Pure: no design, no UI.

mod raster;
pub mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::design::{POINTS_PER_INCH, Pt};
use crate::effects::{Lines, PieceId};
use crate::machine::MAT_PADDING_PT;
use crate::snap::Aabb;
use raster::{Board, Cells, Footprint};

/// Greedy passes the improvement loop may run per piece, so a large enough budget always stops at the
/// same place and a design nests the same way every time.
const ROUNDS_PER_PIECE: usize = 8;
const SEED: u64 = 0x2545_f491_4f6c_dd1d;

/// What nesting moves and turns as one: a composition unit's world lines, in inches.
#[derive(Clone, Debug)]
pub struct Piece {
    pub lines: Lines,
    /// Any member's grain is locked: it may only turn 0 or 180 degrees.
    pub grain_locked: bool,
}

/// The step a Free rotation turns by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreeStep {
    Five,
    Ten,
    Fifteen,
    FortyFive,
}

impl FreeStep {
    pub const ALL: [FreeStep; 4] = [FreeStep::Five, FreeStep::Ten, FreeStep::Fifteen, FreeStep::FortyFive];

    pub fn degrees(self) -> u16 {
        match self {
            FreeStep::Five => 5,
            FreeStep::Ten => 10,
            FreeStep::Fifteen => 15,
            FreeStep::FortyFive => 45,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    None,
    #[default]
    Quarter,
    Free(FreeStep),
}

impl Rotation {
    /// The turns a piece may take, in degrees. Grain-locked pieces keep to 0 and 180.
    pub fn angles(self, grain_locked: bool) -> Vec<f32> {
        let step = match self {
            Rotation::None => return vec![0.0],
            Rotation::Quarter => 90,
            Rotation::Free(step) => step.degrees(),
        };
        (0..360u16).step_by(usize::from(step)).filter(|d| !grain_locked || d % 180 == 0).map(f32::from).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NestSettings {
    pub rotation: Rotation,
    /// The minimum gap between pieces, in inches.
    pub spacing_in: f32,
    /// Small pieces may go in the holes of larger ones.
    pub use_holes: bool,
}

impl Default for NestSettings {
    fn default() -> Self {
        NestSettings { rotation: Rotation::Quarter, spacing_in: 0.125, use_holes: true }
    }
}

/// The full mat or work area. Nesting keeps the mat's padding clear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatArea {
    pub width_in: f32,
    pub height_in: f32,
}

/// `pieces[piece]` turned by `rotation_deg` about its bounds' centre, with the turned bounds' top-left
/// at `(x_in, y_in)` on the mat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub piece: usize,
    pub x_in: f32,
    pub y_in: f32,
    pub rotation_deg: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub placed: Vec<Placed>,
    /// The lowest placed point, measured from the mat's top edge.
    pub used_length_in: f32,
    /// One piece too large for an empty mat in any rotation, on a mat of its own.
    pub overflow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NestResult {
    pub sheets: Vec<Sheet>,
}

/// Packs `pieces` onto as few mats as it can. The greedy pass always completes; the improvement loop
/// then runs until `budget`, `cancel` or its fixed round count, and the best packing so far comes back.
pub fn nest(pieces: &[Piece], mat: MatArea, settings: &NestSettings, budget: Duration, cancel: &AtomicBool) -> NestResult {
    let started = Instant::now();
    nest_until(pieces, mat, settings, |_| cancel.load(Ordering::Relaxed) || started.elapsed() >= budget)
}

/// [`nest`] with the stop decided by `stop(round)`, asked before each improvement round, so a test can
/// end the loop after exactly `n` rounds without a clock.
fn nest_until(pieces: &[Piece], mat: MatArea, settings: &NestSettings, mut stop: impl FnMut(usize) -> bool) -> NestResult {
    let pad = MAT_PADDING_PT / POINTS_PER_INCH;
    let spacing = settings.spacing_in.max(0.0);
    let size = (spacing / 2.0).clamp(0.02, 0.05);
    let cells = Cells { size, halo: (spacing + size) / 2.0 };
    let footprints: Vec<Vec<Footprint>> =
        pieces.iter().map(|p| settings.rotation.angles(p.grain_locked).into_iter().map(|deg| Footprint::of(p, deg, cells, settings.use_holes)).collect()).collect();
    let packer = Packer { footprints: &footprints, empty: Board::new((mat.width_in - 2.0 * pad, mat.height_in - 2.0 * pad), cells), pad, cell: size };

    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(footprints[i][0].cells));
    let mut best = packer.pack(&order);
    let mut rng = XorShift(SEED);
    for round in 0..ROUNDS_PER_PIECE * pieces.len() {
        if pieces.len() < 2 || stop(round) {
            break;
        }
        let i = rng.below(pieces.len() - 1);
        let mut candidate = order.clone();
        candidate.swap(i, i + 1);
        let packed = packer.pack(&candidate);
        if packed.score() < best.score() {
            (best, order) = (packed, candidate);
        }
    }
    best
}

struct Packer<'a> {
    footprints: &'a [Vec<Footprint>],
    empty: Board,
    pad: f32,
    cell: f32,
}

struct Open {
    /// `None` on an overflow mat, which takes nothing more.
    board: Option<Board>,
    sheet: Sheet,
}

impl Packer<'_> {
    /// Each piece in `order` onto the earliest mat it fits, in its best rotation there.
    fn pack(&self, order: &[usize]) -> NestResult {
        let mut open: Vec<Open> = Vec::new();
        for &i in order {
            let fits_empty = self.footprints[i].iter().any(|fp| self.empty.holds(fp));
            if !fits_empty {
                let placed = Placed { piece: i, x_in: self.pad, y_in: self.pad, rotation_deg: 0.0 };
                let used = self.pad + self.footprints[i][0].extent.height();
                open.push(Open { board: None, sheet: Sheet { placed: vec![placed], used_length_in: used, overflow: true } });
                continue;
            }
            let spot = open.iter().enumerate().find_map(|(n, o)| o.board.as_ref().and_then(|b| self.best_spot(b, i)).map(|s| (n, s)));
            let (n, (r, x, y)) = match spot {
                Some(found) => found,
                None => {
                    open.push(Open { board: Some(self.empty.clone()), sheet: Sheet { placed: Vec::new(), used_length_in: 0.0, overflow: false } });
                    (open.len() - 1, self.best_spot(&self.empty, i).expect("fits an empty mat"))
                }
            };
            let fp = &self.footprints[i][r];
            let o = &mut open[n];
            o.board.as_mut().expect("a mat with room").mark(fp, x, y);
            let (x_in, y_in) = self.at(x, y);
            o.sheet.placed.push(Placed { piece: i, x_in, y_in, rotation_deg: fp.deg });
            o.sheet.used_length_in = o.sheet.used_length_in.max(y_in + fp.extent.height());
        }
        NestResult { sheets: open.into_iter().map(|o| o.sheet).collect() }
    }

    /// The rotation index and cell with the smallest resulting used length, then the smallest x.
    fn best_spot(&self, board: &Board, piece: usize) -> Option<(usize, usize, usize)> {
        let mut best: Option<(usize, usize, usize, f32)> = None;
        for (r, fp) in self.footprints[piece].iter().enumerate() {
            let bottom = |y: usize| self.at(0, y).1 + fp.extent.height();
            let beaten = |y: usize| best.is_some_and(|(_, _, _, b)| bottom(y) > b);
            let Some((x, y)) = board.first_fit(fp, beaten) else { continue };
            let better = match best {
                None => true,
                Some((_, bx, _, b)) => bottom(y) < b || (bottom(y) == b && x < bx),
            };
            if better {
                best = Some((r, x, y, bottom(y)));
            }
        }
        best.map(|(r, x, y, _)| (r, x, y))
    }

    /// A footprint's top-left cell to its piece's turned bounds' top-left, in mat inches.
    fn at(&self, x: usize, y: usize) -> Pt {
        (self.pad + x as f32 * self.cell, self.pad + y as f32 * self.cell)
    }
}

impl NestResult {
    /// Fewer mats, then a shorter last mat, then less material overall.
    fn score(&self) -> (usize, Ordered, Ordered) {
        let last = self.sheets.last().map_or(0.0, |s| s.used_length_in);
        (self.sheets.len(), Ordered(last), Ordered(self.sheets.iter().map(|s| s.used_length_in).sum()))
    }
}

#[derive(PartialEq, PartialOrd)]
struct Ordered(f32);

/// xorshift64: a seeded generator, so the improvement loop is repeatable without a dependency.
struct XorShift(u64);

impl XorShift {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Turns a point by `deg` about `centre` and returns it relative to `centre`, in the sense of
/// `Matrix::rotate` and `effects::Placement::placer` (y down).
pub fn turner(deg: f32, centre: Pt) -> impl Fn(Pt) -> Pt {
    let (sin, cos) = deg.to_radians().sin_cos();
    move |(x, y)| {
        let (dx, dy) = (x - centre.0, y - centre.1);
        (dx * cos - dy * sin, dx * sin + dy * cos)
    }
}

/// The bounds of `lines` turned by `deg` about `centre`, relative to `centre`.
pub fn turned_bounds(lines: &Lines, deg: f32, centre: Pt) -> Aabb {
    let turn = turner(deg, centre);
    lines.iter().flat_map(|(pts, _)| pts).map(|&p| turn(p)).fold(Aabb::EMPTY, |b, (x, y)| Aabb {
        min_x: b.min_x.min(x),
        min_y: b.min_y.min(y),
        max_x: b.max_x.max(x),
        max_y: b.max_y.max(y),
    })
}

/// One colour's units to nest: each `(unit, copy)` is `pieces[i]`.
#[derive(Clone, Debug)]
pub struct Group {
    pub colour: Color32,
    pub units: Vec<(PieceId, u32)>,
    pub pieces: Vec<Piece>,
}

/// Everything one Optimise layout needs, as plain data a worker thread can own.
#[derive(Clone, Debug)]
pub struct Job {
    pub fingerprint: u64,
    pub settings: NestSettings,
    pub mat: MatArea,
    pub groups: Vec<Group>,
}

impl Job {
    /// Nests each colour on its own mats, sharing `budget` between them by piece count.
    pub fn run(&self, budget: Duration, cancel: &AtomicBool) -> NestedLayout {
        let total = self.groups.iter().map(|g| g.pieces.len()).sum::<usize>().max(1);
        let mut mats = Vec::new();
        for group in &self.groups {
            let share = budget.mul_f64(group.pieces.len() as f64 / total as f64);
            let result = nest(&group.pieces, self.mat, &self.settings, share, cancel);
            mats.extend(result.sheets.into_iter().map(|sheet| {
                NestedMat {
                    colour: group.colour,
                    overflow: sheet.overflow,
                    used_length_in: sheet.used_length_in,
                    placements: sheet
                        .placed
                        .iter()
                        .map(|p| {
                            let (unit, copy) = group.units[p.piece];
                            UnitPlacement { unit, copy, x_in: p.x_in, y_in: p.y_in, rotation_deg: p.rotation_deg }
                        })
                        .collect(),
                }
            }));
        }
        NestedLayout { fingerprint: self.fingerprint, settings: self.settings, mats }
    }
}

/// An optimised layout, stored on the design. `compose` uses it while `fingerprint` matches the design.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedLayout {
    pub fingerprint: u64,
    pub settings: NestSettings,
    pub mats: Vec<NestedMat>,
}

impl NestedLayout {
    /// Every number finite and on a mat of sane size: what a loaded file must hold to be used.
    pub fn is_sound(&self) -> bool {
        let fine = |v: f32| v.is_finite() && v.abs() <= 10_000.0;
        let spaced = fine(self.settings.spacing_in) && self.settings.spacing_in >= 0.0;
        spaced && self.mats.iter().all(|m| fine(m.used_length_in) && m.placements.iter().all(|p| fine(p.x_in) && fine(p.y_in) && fine(p.rotation_deg)))
    }
}

/// The command log prints every edit; a layout is summarised rather than listed.
impl std::fmt::Debug for NestedLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let placements: usize = self.mats.iter().map(|m| m.placements.len()).sum();
        write!(f, "NestedLayout {{ fingerprint: {:#x}, mats: {}, placements: {placements} }}", self.fingerprint, self.mats.len())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NestedMat {
    #[serde(with = "crate::design::color_serde")]
    pub colour: Color32,
    pub overflow: bool,
    pub used_length_in: f32,
    pub placements: Vec<UnitPlacement>,
}

/// A unit, named by its first piece, turned and placed as [`Placed`] describes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitPlacement {
    pub unit: PieceId,
    pub copy: u32,
    pub x_in: f32,
    pub y_in: f32,
    pub rotation_deg: f32,
}

#[cfg(test)]
mod tests;
