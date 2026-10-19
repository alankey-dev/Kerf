//! Optimise layout's side of composition (spec 30): the nesting job a design's cut units make, the
//! fingerprint that says whether a stored layout still fits the design, and the mats a current layout
//! composes. Print Then Cut pages are never nested.

use super::{Mat, MatItem, Unit, units};
use crate::design::Design;
use crate::effects::{Piece, PieceId};
use crate::machine::MatSize;
use crate::nesting::{self, Group, Job, MatArea, NestSettings, NestedLayout, UnitPlacement};

/// Whether the design holds a layout, and whether it was optimised for the design as it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutStatus {
    Absent,
    Current,
    Stale,
}

/// Every cut unit of every copy, by colour, as nesting pieces: each unit's members' world lines, grain
/// locked when any member is.
pub fn job(design: &Design, size: MatSize, copies: u32, settings: NestSettings) -> Job {
    let units = units(design);
    let mut groups: Vec<Group> = Vec::new();
    for (copy, unit) in (0..copies).flat_map(|copy| units.iter().filter(|u| !u.key.0).map(move |u| (copy, u))) {
        let piece = nesting::Piece {
            lines: unit.pieces.iter().filter_map(|p| with_piece(design, p.id, |p| p.lines.clone())).flatten().collect(),
            grain_locked: grain_locked(design, unit),
        };
        let at = match groups.iter().position(|g| g.colour == unit.key.1) {
            Some(i) => i,
            None => {
                groups.push(Group { colour: unit.key.1, units: Vec::new(), pieces: Vec::new() });
                groups.len() - 1
            }
        };
        groups[at].units.push((unit.pieces[0].id, copy));
        groups[at].pieces.push(piece);
    }
    Job { fingerprint: fingerprint_of(design, &units, size, copies), settings, mat: MatArea { width_in: size.width_in, height_in: size.height_in }, groups }
}

/// The layout Optimise layout would store, worked out on this thread.
#[cfg(test)]
pub fn optimise(
    design: &Design,
    size: MatSize,
    copies: u32,
    settings: NestSettings,
    budget: std::time::Duration,
    cancel: &std::sync::atomic::AtomicBool,
) -> NestedLayout {
    job(design, size, copies, settings).run(budget, cancel)
}

pub fn status(design: &Design, size: MatSize, copies: u32) -> LayoutStatus {
    match &design.nesting {
        None => LayoutStatus::Absent,
        Some(layout) if layout.fingerprint == fingerprint(design, size, copies) => LayoutStatus::Current,
        Some(_) => LayoutStatus::Stale,
    }
}

pub fn fingerprint(design: &Design, size: MatSize, copies: u32) -> u64 {
    fingerprint_of(design, &units(design), size, copies)
}

/// FNV-1a over what nesting reads: every cut unit's members, operations, colours, grain locks and
/// world line points, the mat size and the copies. Stable across runs, unlike `DefaultHasher`.
fn fingerprint_of(design: &Design, units: &[Unit], size: MatSize, copies: u32) -> u64 {
    let mut h = Fnv::default();
    h.f32(size.width_in);
    h.f32(size.height_in);
    h.u64(u64::from(copies));
    for unit in units.iter().filter(|u| !u.key.0) {
        h.u64(unit.pieces.len() as u64);
        h.bytes(&unit.key.1.to_array());
        h.u64(u64::from(grain_locked(design, unit)));
        for p in &unit.pieces {
            h.u64(p.id.owner);
            h.bytes(format!("{:?}", p.id.local).as_bytes());
            h.bytes(format!("{:?}", p.style.operation).as_bytes());
            h.bytes(&p.style.fill.to_array());
            with_piece(design, p.id, |piece| {
                for (pts, closed) in &piece.lines {
                    h.u64(pts.len() as u64 * 2 + u64::from(*closed));
                    for &(x, y) in pts {
                        h.f32(x);
                        h.f32(y);
                    }
                }
            });
        }
    }
    h.0
}

#[derive(Clone, Copy)]
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
}

/// The design's layout when it was optimised for the design as it is now.
pub(super) fn current<'a>(design: &'a Design, units: &[Unit], size: MatSize, copies: u32) -> Option<&'a NestedLayout> {
    design.nesting.as_ref().filter(|l| l.fingerprint == fingerprint_of(design, units, size, copies))
}

/// `layout`'s mats of `colour`, each unit turned about its bounds' centre and placed rigidly.
pub(super) fn mats(design: &Design, layout: &NestedLayout, units: &[Unit], colour: egui::Color32) -> Vec<Mat> {
    let mats = layout.mats.iter().filter(|m| m.colour == colour);
    mats.map(|m| {
        let mut mat = Mat { print_then_cut: false, colour, items: Vec::new(), operations: Vec::new(), overflow: m.overflow, page: None };
        for p in &m.placements {
            if let Some(unit) = units.iter().find(|u| !u.key.0 && u.pieces[0].id == p.unit) {
                add_turned(design, &mut mat, unit, p);
            }
        }
        mat
    })
    .collect()
}

/// Each member keeps its place in the unit: turned about the unit's centre `u`, a member whose own
/// centre is `c` lands at `turn(c - u)` from the unit's turned centre, and its item's offset is that
/// less its own turned extent's top-left.
fn add_turned(design: &Design, mat: &mut Mat, unit: &Unit, p: &UnitPlacement) {
    let u = unit.bounds.centre();
    let turn = nesting::turner(p.rotation_deg, u);
    let members: Vec<(PieceId, (f32, f32), crate::snap::Aabb)> =
        unit.pieces.iter().filter_map(|m| with_piece(design, m.id, |piece| (m.id, turn(piece.bounds.centre()), super::turned_extent(piece, p.rotation_deg)))).collect();
    let min = members.iter().fold((f32::MAX, f32::MAX), |(x, y), (_, (cx, cy), r)| (x.min(cx + r.min_x), y.min(cy + r.min_y)));
    for (id, (cx, cy), r) in members {
        let offset_in = (p.x_in + cx + r.min_x - min.0, p.y_in + cy + r.min_y - min.1);
        mat.items.push(MatItem { piece: id, copy: p.copy, offset_in, rotation_deg: p.rotation_deg });
    }
    for m in &unit.pieces {
        if !mat.operations.contains(&m.style.operation) {
            mat.operations.push(m.style.operation);
        }
    }
}

fn grain_locked(design: &Design, unit: &Unit) -> bool {
    unit.pieces.iter().any(|p| design.shapes.iter().any(|s| s.id == p.id.owner && s.grain_lock))
}

fn with_piece<R>(design: &Design, id: PieceId, f: impl FnOnce(&Piece) -> R) -> Option<R> {
    let shape = design.shapes.iter().find(|s| s.id == id.owner)?;
    design.geometry(shape).piece(id).map(f)
}

/// The material a Make run uses: over every cut mat, the lowest placed point measured from the mat's
/// top edge.
pub fn material_length(design: &Design, mats: &[Mat], size: MatSize) -> f32 {
    mats.iter().filter(|m| !m.print_then_cut).map(|m| crate::output::estimate::usage(design, m, size).bounds_in.3).sum()
}

#[cfg(test)]
mod tests;
