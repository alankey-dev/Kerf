//! How long a mat takes and how the head moves, from the exact strokes `mat_toolpath` would send.

use crate::composition::Mat;
use crate::design::{Design, Operation, POINTS_PER_INCH};
use crate::machine::{MAT_PADDING_PT, MatSize};
use crate::make_flow::MatSetup;
use crate::material::{Material, PressureAdjust};
use crate::snap::Aabb;

use super::OutputDevice;
use super::profile::{DeviceProfile, Drive, ToolKind};
use super::toolpath::{Toolpath, traced};

/// One straight move in machine order, in millimetres on the work area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub kind: MoveKind,
    /// Cumulative time at the start of this move.
    pub start_s: f32,
    pub duration_s: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    /// Tool up, between strokes.
    Travel,
    /// The tool lowering onto a stroke's start, in place.
    Settle,
    /// Tool down, along a stroke of this operation.
    Work(Operation),
}

/// What one mat runs at. Passes are already in the toolpath, as `with_passes` repeats cut strokes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub work_mm_s: f32,
    pub travel_mm_s: f32,
    /// Lowering the tool onto each stroke.
    pub tool_change_s: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Estimate {
    pub motions: Vec<Motion>,
    pub total_s: f32,
    pub work_mm: f32,
    pub travel_mm: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    /// Left, top, right, bottom on the mat, in inches.
    pub bounds_in: (f32, f32, f32, f32),
    pub area_in2: f32,
    /// `area_in2` over the padded cutting area.
    pub mat_fraction: f32,
}

/// The simulated machine's nominal work speed for each `Material::cut_speed` step, 0 to 10. The
/// machine publishes no speeds, so these only rank materials and give a plausible order of magnitude.
pub const SIM_WORK_MM_S: [f32; 11] = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0];
pub const SIM_TRAVEL_MM_S: f32 = 200.0;
pub const SIM_TOOL_CHANGE_S: f32 = 0.2;

impl Rates {
    /// The mapped work speed, the profile's fastest speed for travel and a pen's settle time.
    pub fn open(profile: &DeviceProfile, drive: Drive) -> Self {
        let tool_change_s = match profile.tool {
            ToolKind::Pen { settle_ms, .. } => settle_ms as f32 / 1000.0,
            ToolKind::DragKnife { .. } | ToolKind::Laser { .. } => 0.0,
        };
        Self { work_mm_s: drive.speed_mm_s, travel_mm_s: profile.speed_mm_s.1, tool_change_s }
    }

    /// The simulated machine's nominal rates for `material`.
    pub fn simulated(material: &Material) -> Self {
        Self { work_mm_s: SIM_WORK_MM_S[material.cut_speed.min(10) as usize], travel_mm_s: SIM_TRAVEL_MM_S, tool_change_s: SIM_TOOL_CHANGE_S }
    }
}

/// The strokes one mat sends on `device` in `material`, with passes, and the rates they run at.
pub fn mat_run(design: &Design, mat: &Mat, setup: &MatSetup, device: &OutputDevice, material: &Material, adjust: PressureAdjust) -> (Toolpath, Rates) {
    match device {
        OutputDevice::Open(p) => {
            let drive = p.drive(material, adjust);
            (super::mat_toolpath(design, mat, setup, Some(p), drive.passes), Rates::open(p, drive))
        }
        OutputDevice::Simulator => (super::mat_toolpath(design, mat, setup, None, material.multi_pass.max(1)), Rates::simulated(material)),
    }
}

/// Travel from the origin to each stroke, a settle, the stroke's segments, and travel back to the origin.
pub fn estimate(path: &Toolpath, rates: &Rates) -> Estimate {
    let mut est = Estimate::default();
    let push = |est: &mut Estimate, from: (f32, f32), to: (f32, f32), kind: MoveKind| {
        let length = (to.0 - from.0).hypot(to.1 - from.1);
        let duration_s = match kind {
            MoveKind::Travel => {
                est.travel_mm += length;
                length / rates.travel_mm_s
            }
            MoveKind::Settle => rates.tool_change_s,
            MoveKind::Work(_) => {
                est.work_mm += length;
                length / rates.work_mm_s
            }
        };
        est.motions.push(Motion { from, to, kind, start_s: est.total_s, duration_s });
        est.total_s += duration_s;
    };
    let mut head = (0.0, 0.0);
    for stroke in &path.strokes {
        let start = stroke.points[0];
        if start != head {
            push(&mut est, head, start, MoveKind::Travel);
        }
        if rates.tool_change_s > 0.0 {
            push(&mut est, start, start, MoveKind::Settle);
        }
        head = start;
        for p in traced(stroke).skip(1) {
            push(&mut est, head, p, MoveKind::Work(stroke.op));
            head = p;
        }
    }
    if head != (0.0, 0.0) {
        push(&mut est, head, (0.0, 0.0), MoveKind::Travel);
    }
    est
}

/// The union of the mat's items at their packed offsets, against the padded cutting area.
pub fn usage(design: &Design, mat: &Mat, size: MatSize) -> Usage {
    let placed = mat.items.iter().filter_map(|item| {
        let shape = design.shapes.iter().find(|s| s.id == item.piece.owner)?;
        let b = crate::composition::turned_extent(design.geometry(shape).piece(item.piece)?, item.rotation_deg);
        let (x, y) = item.offset_in;
        Some(Aabb { min_x: x, min_y: y, max_x: x + b.width(), max_y: y + b.height() })
    });
    let b = placed.fold(Aabb::EMPTY, Aabb::union);
    if b.is_empty() {
        return Usage { bounds_in: (0.0, 0.0, 0.0, 0.0), area_in2: 0.0, mat_fraction: 0.0 };
    }
    let pad = 2.0 * MAT_PADDING_PT / POINTS_PER_INCH;
    let area_in2 = b.width() * b.height();
    Usage { bounds_in: (b.min_x, b.min_y, b.max_x, b.max_y), area_in2, mat_fraction: area_in2 / ((size.width_in - pad) * (size.height_in - pad)) }
}

/// The head's position at `t_s`, clamped to the run, and the index of the motion under way.
pub fn position_at(est: &Estimate, t_s: f32) -> ((f32, f32), usize) {
    if est.motions.is_empty() {
        return ((0.0, 0.0), 0);
    }
    let t = t_s.clamp(0.0, est.total_s);
    let i = est.motions.partition_point(|m| m.start_s <= t).saturating_sub(1);
    let m = &est.motions[i];
    if m.duration_s <= 0.0 {
        return (m.to, i);
    }
    let f = ((t - m.start_s) / m.duration_s).min(1.0);
    ((m.from.0 + (m.to.0 - m.from.0) * f, m.from.1 + (m.to.1 - m.from.1) * f), i)
}

/// `0:42`, `3:15`, `1:02:03`.
pub fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::material::MATERIALS;
    use crate::output::toolpath::{Stroke, tests::*, with_passes};

    fn square_mm(op: Operation, x: f32, y: f32, side: f32) -> Stroke {
        Stroke { op, points: vec![(x, y), (x + side, y), (x + side, y + side), (x, y + side)], closed: true }
    }

    fn rates(tool_change_s: f32) -> Rates {
        Rates { work_mm_s: 10.0, travel_mm_s: 100.0, tool_change_s }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn work_s(est: &Estimate, op: Operation) -> f32 {
        est.motions.iter().filter(|m| m.kind == MoveKind::Work(op)).map(|m| m.duration_s).sum()
    }

    fn material(name: &str) -> &'static Material {
        MATERIALS.iter().find(|m| m.name == name).unwrap()
    }

    #[test]
    fn a_10_mm_square_at_10_10_takes_4_s_of_work_and_two_diagonal_travels() {
        let path = Toolpath { strokes: vec![square_mm(Operation::BasicCut, 10.0, 10.0, 10.0)] };
        let est = estimate(&path, &rates(0.0));
        assert!(close(work_s(&est, Operation::BasicCut), 4.0), "{est:?}");
        assert!(close(est.work_mm, 40.0), "{est:?}");
        let diagonal = 200f32.sqrt();
        assert!(close(est.travel_mm, 2.0 * diagonal), "{est:?}");
        let travel_s: f32 = est.motions.iter().filter(|m| m.kind == MoveKind::Travel).map(|m| m.duration_s).sum();
        assert!(close(travel_s, 0.283), "{travel_s}");
        assert!(close(est.total_s, 4.0 + 2.0 * diagonal / 100.0), "{est:?}");
        let last = est.motions.last().unwrap();
        assert!(close(est.total_s, last.start_s + last.duration_s), "{est:?}");
    }

    #[test]
    fn tool_change_adds_once_per_stroke_and_passes_triple_only_cut_time() {
        let path = Toolpath { strokes: vec![square_mm(Operation::Pen, 10.0, 10.0, 5.0), square_mm(Operation::BasicCut, 30.0, 10.0, 10.0)] };
        let plain = estimate(&path, &rates(0.0));
        let settled = estimate(&path, &rates(0.5));
        assert!(close(settled.total_s - plain.total_s, 2.0 * 0.5), "{plain:?} {settled:?}");
        assert_eq!(settled.motions.iter().filter(|m| m.kind == MoveKind::Settle).count(), 2);

        let tripled = estimate(&with_passes(path, 3), &rates(0.0));
        assert!(close(work_s(&tripled, Operation::BasicCut), 3.0 * work_s(&plain, Operation::BasicCut)), "{tripled:?}");
        assert!(close(work_s(&tripled, Operation::Pen), work_s(&plain, Operation::Pen)), "{tripled:?}");
        assert!(close(tripled.travel_mm, plain.travel_mm), "{tripled:?}");
    }

    #[test]
    fn work_moves_follow_the_machine_order_and_an_attached_pen_draws_before_its_cut() {
        let mut design = Design::default();
        let heart = design.add_shape(ShapeKind::Heart);
        let star = design.add_shape(ShapeKind::Star);
        for (id, (x, y, w, op)) in [(heart, (1.0, 1.0, 3.0, Operation::BasicCut)), (star, (1.75, 1.75, 1.5, Operation::Pen))] {
            let s = design.shape_mut(id).unwrap();
            (s.x_in, s.y_in, s.width_in, s.height_in, s.operation) = (x, y, w, w, op);
        }
        design.attach(&[heart, star]).unwrap();
        let path = super::super::mat_toolpath(&design, &only_mat(&design), &setup(), None, 1);
        let est = estimate(&path, &rates(0.1));

        let expected: Vec<_> = path
            .strokes
            .iter()
            .flat_map(|s| {
                let pts: Vec<_> = crate::output::toolpath::traced(s).collect();
                pts.windows(2).map(|w| (w[0], w[1], MoveKind::Work(s.op))).collect::<Vec<_>>()
            })
            .collect();
        let actual: Vec<_> = est.motions.iter().filter(|m| matches!(m.kind, MoveKind::Work(_))).map(|m| (m.from, m.to, m.kind)).collect();
        assert_eq!(actual, expected);

        let last_pen = est.motions.iter().rposition(|m| m.kind == MoveKind::Work(Operation::Pen)).unwrap();
        let first_cut = est.motions.iter().position(|m| m.kind == MoveKind::Work(Operation::BasicCut)).unwrap();
        assert!(last_pen < first_cut, "pen motion {last_pen} after cut motion {first_cut}");
    }

    #[test]
    fn position_starts_and_ends_at_the_origin_and_halfway_is_the_midpoint() {
        let path = Toolpath { strokes: vec![Stroke { op: Operation::BasicCut, points: vec![(0.0, 0.0), (20.0, 0.0)], closed: false }] };
        let est = estimate(&path, &rates(0.0));
        assert_eq!(position_at(&est, 0.0).0, (0.0, 0.0));
        assert_eq!(position_at(&est, est.total_s).0, (0.0, 0.0));
        assert_eq!(position_at(&est, est.total_s + 5.0).0, (0.0, 0.0));
        let (work_i, work) = est.motions.iter().enumerate().find(|(_, m)| m.kind == MoveKind::Work(Operation::BasicCut)).unwrap();
        let ((x, y), i) = position_at(&est, work.start_s + work.duration_s / 2.0);
        assert!(close(x, 10.0) && close(y, 0.0), "{:?}", (x, y));
        assert_eq!(i, work_i);
    }

    #[test]
    fn an_empty_estimate_sits_at_the_origin() {
        let est = estimate(&Toolpath::default(), &rates(0.5));
        assert_eq!(est, Estimate::default());
        assert_eq!(position_at(&est, 3.0), ((0.0, 0.0), 0));
    }

    #[test]
    fn one_2_by_3_shape_on_a_12_by_12_mat_uses_6_square_inches_of_the_padded_area() {
        let mut design = Design::default();
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.width_in, s.height_in, s.operation) = (2.0, 3.0, Operation::BasicCut);
        let size = setup().mat_size;
        assert_eq!((size.width_in, size.height_in), (12.0, 12.0));
        let mat = only_mat(&design);
        let u = usage(&design, &mat, size);
        assert!(close(u.area_in2, 6.0), "{u:?}");
        assert!(close(u.mat_fraction, 6.0 / (11.5 * 11.5)), "{u:?}");
        let (ox, oy) = mat.items[0].offset_in;
        let (l, t, r, b) = u.bounds_in;
        assert!(close(l, ox) && close(t, oy) && close(r - l, 2.0) && close(b - t, 3.0), "{u:?}");
    }

    #[test]
    fn an_empty_mat_uses_nothing() {
        let mat = Mat { print_then_cut: false, colour: egui::Color32::WHITE, items: Vec::new(), operations: Vec::new(), overflow: false, page: None };
        let u = usage(&Design::default(), &mat, setup().mat_size);
        assert_eq!(u, Usage { bounds_in: (0.0, 0.0, 0.0, 0.0), area_in2: 0.0, mat_fraction: 0.0 });
    }

    #[test]
    fn the_simulator_cuts_cardstock_slower_than_vinyl() {
        let path = Toolpath { strokes: vec![square_mm(Operation::BasicCut, 10.0, 10.0, 10.0)] };
        let cardstock = estimate(&path, &Rates::simulated(material("Cardstock")));
        let vinyl = estimate(&path, &Rates::simulated(material("Vinyl")));
        assert!(cardstock.total_s > vinyl.total_s, "{} vs {}", cardstock.total_s, vinyl.total_s);
    }

    #[test]
    fn clock_rounds_to_the_nearest_second() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(41.6), "0:42");
        assert_eq!(clock(195.0), "3:15");
        assert_eq!(clock(3599.4), "59:59");
        assert_eq!(clock(3599.6), "1:00:00");
        assert_eq!(clock(3723.0), "1:02:03");
    }
}
