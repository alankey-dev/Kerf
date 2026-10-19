//! HPGL for vinyl cutters.

use super::profile::{DeviceProfile, Drive};
use super::toolpath::{Toolpath, to_machine, traced};
use super::{Job, ToolChange, number};
use crate::design::Operation;

const TO_PEN: &str = "Swap the knife for the pen, then press Resume";
const TO_KNIFE: &str = "Swap the pen back for the knife, then press Resume";

pub fn emit(path: &Toolpath, profile: &DeviceProfile, drive: Drive) -> Job {
    let mut lines = vec!["IN;".to_owned(), format!("VS{};", number(drive.speed_mm_s / 10.0))];
    lines.extend(drive.force.map(|f| format!("FS{f};")));
    let mut seconds = vec![0.0; lines.len()];
    let mut tool_changes = Vec::new();
    let mm_per_unit = 1.0 / profile.units_per_mm;
    let travel = |from: (f32, f32), to: (f32, f32)| (to.0 - from.0).hypot(to.1 - from.1) * mm_per_unit / drive.speed_mm_s;
    let xy = |(x, y): (f32, f32)| format!("{},{}", x.round() as i64, y.round() as i64);

    let mut head = to_machine((0.0, 0.0), profile);
    let mut pen_loaded = false;
    for stroke in &path.strokes {
        let is_pen = stroke.op == Operation::Pen;
        if profile.pen_change && is_pen != pen_loaded {
            tool_changes.push(ToolChange { before_line: lines.len(), message: if is_pen { TO_PEN } else { TO_KNIFE }.into() });
            pen_loaded = is_pen;
        }
        let points: Vec<_> = traced(stroke).map(|p| to_machine(p, profile)).collect();
        lines.push(format!("PU{};", xy(points[0])));
        seconds.push(travel(head, points[0]));
        lines.push(format!("PD{};", points[1..].iter().map(|&p| xy(p)).collect::<Vec<_>>().join(",")));
        seconds.push(points.windows(2).map(|w| travel(w[0], w[1])).sum());
        head = *points.last().unwrap();
    }
    lines.push("PU;".into());
    seconds.push(0.0);
    lines.push("PU0,0;".into());
    seconds.push(travel(head, (0.0, 0.0)));

    // A cancel can land mid-instruction, since chunks split anywhere; the leading `;` ends it first.
    let stop = [b";PU;".as_slice(), profile.abort.as_bytes()].concat();
    Job { protocol: profile.protocol, lines, seconds, stop, pause: None, resume: None, tool_changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::toolpath::{Stroke, tests::*};

    #[test]
    fn golden_square_knife() {
        let job = job_for(&two_inch_square(), &DeviceProfile::hpgl_vinyl_cutter());
        assert_eq!(job.seconds.len(), job.lines.len());
        assert_golden("square-knife.hpgl", &job);
    }

    fn drive() -> Drive {
        Drive { speed_mm_s: 100.0, force: None, power: None, passes: 1 }
    }

    fn line(op: Operation, y: f32) -> Stroke {
        Stroke { op, points: vec![(0.0, y), (10.0, y)], closed: false }
    }

    #[test]
    fn coordinates_are_whole_plotter_units_in_the_profiles_frame() {
        let profile = DeviceProfile { origin: crate::output::profile::Origin::TopLeft, swap_xy: true, ..DeviceProfile::hpgl_vinyl_cutter() };
        let path = Toolpath { strokes: vec![Stroke { op: Operation::BasicCut, points: vec![(1.01, 2.0), (3.0, 4.0)], closed: false }] };
        let job = emit(&path, &profile, drive());
        assert_eq!(job.lines, ["IN;", "VS10;", "PU80,40;", "PD160,120;", "PU;", "PU0,0;"]);
        assert!((job.seconds[3] - 2.0f32.hypot(2.0) / 100.0).abs() < 1e-3, "{}", job.seconds[3]);
        assert_eq!(job.stop, b";PU;");
        assert_eq!((job.pause.as_ref(), job.resume.as_ref()), (None, None));
    }

    #[test]
    fn a_pen_run_stops_for_a_tool_change_on_the_way_in_and_out() {
        let profile = DeviceProfile {
            pen_change: true,
            abort: "AB;".into(),
            origin: crate::output::profile::Origin::TopLeft,
            swap_xy: false,
            ..DeviceProfile::hpgl_vinyl_cutter()
        };
        let path = Toolpath { strokes: vec![line(Operation::Pen, 1.0), line(Operation::Pen, 2.0), line(Operation::BasicCut, 3.0)] };
        let job = emit(&path, &profile, drive());
        let changes: Vec<_> = job.tool_changes.iter().map(|c| (job.lines[c.before_line].as_str(), c.message.as_str())).collect();
        assert_eq!(changes, [("PU0,40;", TO_PEN), ("PU0,120;", TO_KNIFE)]);
        assert_eq!(job.stop, b";PU;AB;");
    }

    #[test]
    fn without_pen_change_there_are_no_stops() {
        let path = Toolpath { strokes: vec![line(Operation::Pen, 1.0), line(Operation::BasicCut, 3.0)] };
        assert!(emit(&path, &DeviceProfile::hpgl_vinyl_cutter(), drive()).tool_changes.is_empty());
    }
}
