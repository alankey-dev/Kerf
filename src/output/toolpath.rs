//! A mat's geometry as machine strokes, in millimetres on the work area from its top-left, y down.

use crate::bridge;
use crate::composition::Mat;
use crate::design::{Design, Operation, POINTS_PER_INCH};
use crate::geometry;
use crate::make_flow::MatSetup;

use super::profile::{DeviceProfile, MM_PER_IN};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toolpath {
    /// Machine order once `order::order` has run.
    pub strokes: Vec<Stroke>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub op: Operation,
    /// A closed stroke does not repeat its first point at the end.
    pub points: Vec<(f32, f32)>,
    pub closed: bool,
}

/// Every contour of every placed piece on the mat, through `bridge::mat_placements`, flattened to
/// `tol_mm`. Copies and Mirror land where the Prepare preview puts them, and each stroke takes its
/// piece's own operation, as the bridge payload does.
pub fn build(design: &Design, mat: &Mat, setup: &MatSetup, tol_mm: f32) -> Toolpath {
    let tol_in = tol_mm / MM_PER_IN;
    let mm = |(x, y): (f32, f32)| (x * MM_PER_IN / POINTS_PER_INCH, y * MM_PER_IN / POINTS_PER_INCH);
    let mut strokes = Vec::new();
    for p in bridge::mat_placements(design, mat, setup) {
        let piece = p.piece();
        let op = piece.style.operation;
        if op == Operation::Guide || op.art_type().is_none() {
            continue;
        }
        for contour in &piece.contours {
            let points: Vec<_> = geometry::flatten(contour, tol_in).into_iter().map(|(x, y)| mm(p.transform.apply((x * POINTS_PER_INCH, y * POINTS_PER_INCH)))).collect();
            if points.len() >= 2 {
                strokes.push(Stroke { op, points, closed: contour.closed });
            }
        }
    }
    Toolpath { strokes }
}

/// A work-area point in the profile's machine frame and units, unrounded.
pub(super) fn to_machine(p: (f32, f32), profile: &DeviceProfile) -> (f32, f32) {
    let (x, y) = profile.origin.place(p, profile.work_area_mm);
    let (x, y) = if profile.swap_xy { (y, x) } else { (x, y) };
    (x * profile.units_per_mm, y * profile.units_per_mm)
}

/// The stroke's tip path, back to its start when closed.
pub(super) fn traced(stroke: &Stroke) -> impl Iterator<Item = (f32, f32)> + '_ {
    stroke.points.iter().copied().chain(stroke.closed.then(|| stroke.points[0]))
}

pub(super) fn is_cut(op: Operation) -> bool {
    matches!(op, Operation::BasicCut | Operation::WavyCut | Operation::Perforate | Operation::PrintThenCut)
}

/// Each cut stroke repeated `passes` times in place; draw strokes run once.
pub fn with_passes(path: Toolpath, passes: u32) -> Toolpath {
    let strokes = path
        .strokes
        .into_iter()
        .flat_map(|s| {
            let n = if is_cut(s.op) { passes.max(1) } else { 1 };
            std::iter::repeat_n(s, n as usize)
        })
        .collect();
    Toolpath { strokes }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::machine::MachineFamily;

    pub fn setup() -> MatSetup {
        MatSetup::preferred(MachineFamily::StudioPro)
    }

    /// One 2 in square cut, alone on its mat.
    pub fn two_inch_square() -> Design {
        let mut design = Design::default();
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.width_in, s.height_in, s.operation) = (2.0, 2.0, Operation::BasicCut);
        design
    }

    pub fn only_mat(design: &Design) -> Mat {
        let mut mats = crate::composition::compose(design, setup().mat_size, 1);
        assert_eq!(mats.len(), 1, "{mats:?}");
        mats.remove(0)
    }

    /// The bytes for `design`'s only mat on `profile`, composed at its work area, in Vinyl at default
    /// pressure.
    pub fn job_for(design: &Design, profile: &DeviceProfile) -> crate::output::Job {
        let (width_in, height_in) = profile.work_area_in();
        let mat_size = crate::machine::MatSize { imperial_name: "Work area", metric_name: "Work area", width_in, height_in, preferred: true, matless: true };
        let setup = MatSetup { mat_size, ..setup() };
        let mats = crate::composition::compose(design, mat_size, 1);
        assert_eq!(mats.len(), 1, "{mats:?}");
        let vinyl = crate::material::MATERIALS.iter().find(|m| m.name == "Vinyl").unwrap();
        let drive = profile.drive(vinyl, crate::material::PressureAdjust::Default);
        crate::output::job(design, &mats[0], &setup, profile, drive)
    }

    /// Compares `job`'s bytes with `tests/golden/24/{name}` exactly. `UPDATE_GOLDEN=1` rewrites it.
    pub fn assert_golden(name: &str, job: &crate::output::Job) {
        let text = String::from_utf8(job.bytes()).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/24").join(name);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}; run UPDATE_GOLDEN=1 cargo test golden", path.display()));
        assert!(expected == text, "{name} differs from tests/golden/24/{name}; run UPDATE_GOLDEN=1 cargo test golden and review the diff");
    }

    fn bounds(points: &[(f32, f32)]) -> (f32, f32, f32, f32) {
        points.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn a_two_inch_square_is_one_closed_stroke_with_50_8_mm_sides_at_the_packed_offset() {
        let design = two_inch_square();
        let mat = only_mat(&design);
        let path = build(&design, &mat, &setup(), 0.05);
        assert_eq!(path.strokes.len(), 1, "{path:?}");
        let s = &path.strokes[0];
        assert!(s.closed);
        assert_eq!(s.op, Operation::BasicCut);
        let (x0, y0, x1, y1) = bounds(&s.points);
        let (ox, oy) = mat.items[0].offset_in;
        assert!(close(x0, ox * 25.4) && close(y0, oy * 25.4), "{:?} vs offset {:?}", (x0, y0), (ox, oy));
        assert!(close(x1 - x0, 50.8) && close(y1 - y0, 50.8), "{:?}", (x1 - x0, y1 - y0));
    }

    #[test]
    fn mirror_reflects_the_stroke_across_the_mat_width_like_the_placements() {
        let design = two_inch_square();
        let mat = only_mat(&design);
        let plain = build(&design, &mat, &setup(), 0.05);
        let mirrored_setup = MatSetup { mirror: true, ..setup() };
        let mirrored = build(&design, &mat, &mirrored_setup, 0.05);
        let width_mm = setup().mat_size.width_in * 25.4;
        for (&(x, y), &(mx, my)) in plain.strokes[0].points.iter().zip(&mirrored.strokes[0].points) {
            assert!(close(mx, width_mm - x) && close(my, y), "{:?} vs {:?}", (x, y), (mx, my));
        }
        let placement = &bridge::mat_placements(&design, &mat, &mirrored_setup)[0];
        let corner = placement.transform.apply((0.0, 0.0));
        let corner_mm = (corner.0 * 25.4 / 72.0, corner.1 * 25.4 / 72.0);
        assert!(mirrored.strokes[0].points.iter().any(|&(x, y)| close(x, corner_mm.0) && close(y, corner_mm.1)));
    }

    #[test]
    fn passes_repeat_cut_strokes_in_place_and_draw_strokes_once() {
        let stroke = |op| Stroke { op, points: vec![(0.0, 0.0), (1.0, 0.0)], closed: false };
        let path = Toolpath { strokes: vec![stroke(Operation::Pen), stroke(Operation::BasicCut), stroke(Operation::Score)] };
        let ops: Vec<_> = with_passes(path, 3).strokes.iter().map(|s| s.op).collect();
        assert_eq!(ops, [Operation::Pen, Operation::BasicCut, Operation::BasicCut, Operation::BasicCut, Operation::Score]);
    }
}
