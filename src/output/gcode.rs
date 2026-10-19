//! GRBL G-code for lasers and pen plotters.

use super::profile::{DeviceProfile, Drive, LIGHT_POWER, ToolKind};
use super::toolpath::{Toolpath, to_machine, traced};
use super::{Job, number};
use crate::design::Operation;

pub fn emit(path: &Toolpath, profile: &DeviceProfile, drive: Drive) -> Job {
    let mut out = Lines::default();
    out.push("G21", 0.0);
    out.push("G90", 0.0);
    let feed = number(drive.speed_mm_s * 60.0);
    let mm_per_unit = 1.0 / profile.units_per_mm;
    let travel = |from: (f32, f32), to: (f32, f32)| (to.0 - from.0).hypot(to.1 - from.1) * mm_per_unit / drive.speed_mm_s;
    let xy = |(x, y): (f32, f32)| format!("X{} Y{}", number(x), number(y));

    let mut head = to_machine((0.0, 0.0), profile);
    for stroke in &path.strokes {
        let points: Vec<_> = traced(stroke).map(|p| to_machine(p, profile)).collect();
        out.push(format!("G0 {}", xy(points[0])), travel(head, points[0]));
        match &profile.tool {
            ToolKind::Pen { down, settle_ms, .. } => {
                out.push(down, 0.0);
                out.push(format!("G4 P{}", number(*settle_ms as f32 / 1000.0)), *settle_ms as f32 / 1000.0);
            }
            ToolKind::Laser { dynamic, .. } => {
                let full = drive.power.unwrap_or(0);
                let light = matches!(stroke.op, Operation::Pen | Operation::Score | Operation::Engrave);
                let power = if light { (full as f32 * LIGHT_POWER).round() as u32 } else { full };
                out.push(format!("{} S{power}", if *dynamic { "M4" } else { "M3" }), 0.0);
            }
            ToolKind::DragKnife { .. } => {}
        }
        for w in points.windows(2) {
            out.push(format!("G1 {} F{feed}", xy(w[1])), travel(w[0], w[1]));
        }
        match &profile.tool {
            ToolKind::Pen { up, settle_ms, .. } => {
                out.push(up, 0.0);
                out.push(format!("G4 P{}", number(*settle_ms as f32 / 1000.0)), *settle_ms as f32 / 1000.0);
            }
            ToolKind::Laser { .. } => out.push("M5", 0.0),
            ToolKind::DragKnife { .. } => {}
        }
        head = *points.last().unwrap();
    }
    if let ToolKind::Pen { up, .. } = &profile.tool {
        out.push(up, 0.0);
    }
    out.push("M5", 0.0);
    out.push("G0 X0 Y0", travel(head, (0.0, 0.0)));

    let stop = [b"!".as_slice(), &[0x18], b"M5\n"].concat();
    Job { protocol: profile.protocol, lines: out.lines, seconds: out.seconds, stop, pause: Some(b"!".to_vec()), resume: Some(b"~".to_vec()), tool_changes: Vec::new() }
}

#[derive(Default)]
struct Lines {
    lines: Vec<String>,
    seconds: Vec<f32>,
}

impl Lines {
    fn push(&mut self, line: impl Into<String>, seconds: f32) {
        self.lines.push(line.into());
        self.seconds.push(seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Design, ShapeKind};
    use crate::output::toolpath::{Stroke, tests::*};

    #[test]
    fn golden_square_pen() {
        assert_golden("square-pen.gcode", &job_for(&two_inch_square(), &DeviceProfile::grbl_pen_plotter()));
    }

    #[test]
    fn golden_square_laser() {
        let job = job_for(&two_inch_square(), &DeviceProfile::grbl_laser());
        assert!(job.lines.iter().any(|l| l.starts_with("M4 S")));
        assert_eq!(job.lines[job.lines.len() - 2], "M5");
        assert_golden("square-laser.gcode", &job);
    }

    #[test]
    fn golden_heart_star() {
        let mut design = Design::default();
        let heart = design.add_shape(ShapeKind::Heart);
        let star = design.add_shape(ShapeKind::Star);
        for (id, (x, y, w, op)) in [(heart, (1.0, 1.0, 3.0, Operation::BasicCut)), (star, (1.75, 1.75, 1.5, Operation::Pen))] {
            let s = design.shape_mut(id).unwrap();
            (s.x_in, s.y_in, s.width_in, s.height_in, s.operation) = (x, y, w, w, op);
        }
        design.attach(&[heart, star]).unwrap();
        let job = job_for(&design, &DeviceProfile::grbl_pen_plotter());
        let path = crate::output::order::order(crate::output::toolpath::build(&design, &only_mat(&design), &setup(), 0.05));
        assert_eq!(path.strokes.iter().map(|s| s.op).collect::<Vec<_>>(), [Operation::Pen, Operation::BasicCut], "the star draws before the heart cuts");
        assert_golden("heart-star.gcode", &job);
    }

    fn drive() -> Drive {
        Drive { speed_mm_s: 10.0, force: None, power: Some(500), passes: 1 }
    }

    fn top_left(tool: ToolKind) -> DeviceProfile {
        DeviceProfile { origin: crate::output::profile::Origin::TopLeft, tool, ..DeviceProfile::grbl_laser() }
    }

    fn path(op: Operation) -> Toolpath {
        Toolpath { strokes: vec![Stroke { op, points: vec![(1.0, 2.0), (3.5, 2.0)], closed: false }] }
    }

    #[test]
    fn a_drag_knife_on_grbl_moves_with_g0_and_g1_only() {
        let job = emit(&path(Operation::BasicCut), &top_left(ToolKind::DragKnife { offset_mm: 0.25, swivel_threshold_deg: 30.0 }), drive());
        assert_eq!(job.lines, ["G21", "G90", "G0 X1 Y2", "G1 X3.5 Y2 F600", "M5", "G0 X0 Y0"]);
        assert_eq!(job.seconds[3], 0.25);
        assert_eq!(job.stop, b"!\x18M5\n");
        assert_eq!((job.pause.as_deref(), job.resume.as_deref()), (Some(b"!".as_slice()), Some(b"~".as_slice())));
    }

    #[test]
    fn a_laser_scores_at_light_power_and_cuts_at_full_power() {
        let laser = top_left(ToolKind::Laser { power_max: 1000, dynamic: false });
        assert!(emit(&path(Operation::Score), &laser, drive()).lines.contains(&"M3 S150".to_owned()));
        assert!(emit(&path(Operation::BasicCut), &laser, drive()).lines.contains(&"M3 S500".to_owned()));
    }
}
