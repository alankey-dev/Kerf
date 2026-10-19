//! Overcut and drag-knife offset compensation, in millimetres.

use super::profile::ToolKind;
use super::toolpath::{Stroke, Toolpath, is_cut};

type P = (f32, f32);

const ARC_STEP_DEG: f32 = 10.0;

/// Overcut first, then the drag-knife offset; both touch cut strokes only.
///
/// A drag knife's tip trails its holder by `offset_mm`, so the holder path leads every tip point by
/// `offset_mm` along the direction of travel. At each vertex P, entered along unit direction a and
/// left along b, the holder goes to P + off·a, then, when the turn exceeds the swivel threshold,
/// around an arc of radius off centred on P from a to b the short way in steps of at most 10°, then
/// to P + off·b. A closed stroke becomes open: it starts with a lead-in on its closing edge, off
/// before the start vertex, so the blade swivels to the first edge at the start corner like any other
/// corner, then runs the ring back to the start (and on through any overcut).
pub fn compensate(path: &Toolpath, tool: &ToolKind, overcut_mm: f32) -> Toolpath {
    let strokes = path
        .strokes
        .iter()
        .map(|s| {
            if !is_cut(s.op) {
                return s.clone();
            }
            let ring = dedup(&s.points);
            let closed = s.closed && ring.len() > 2;
            let overcuts = closed && overcut_mm > 0.0;
            let mut points = if overcuts { overcut(&ring, overcut_mm) } else { ring.clone() };
            let ToolKind::DragKnife { offset_mm, swivel_threshold_deg } = *tool else {
                return Stroke { op: s.op, points, closed: closed && !overcuts };
            };
            if closed {
                let start = ring[0];
                let lead_in = sub(start, scale(unit(sub(start, *ring.last().unwrap())), offset_mm));
                if !overcuts {
                    points.push(start);
                }
                points.insert(0, lead_in);
            }
            Stroke { op: s.op, points: holder_path(&points, offset_mm, swivel_threshold_deg), closed: false }
        })
        .collect();
    Toolpath { strokes }
}

/// The ring back to its start, then `length` further along its own first segments.
fn overcut(ring: &[P], length: f32) -> Vec<P> {
    let mut points = ring.to_vec();
    points.push(ring[0]);
    let mut left = length;
    for i in 0..ring.len() {
        let (from, to) = (ring[i], ring[(i + 1) % ring.len()]);
        let edge = dist(from, to);
        if edge >= left {
            points.push(add(from, scale(unit(sub(to, from)), left)));
            break;
        }
        points.push(to);
        left -= edge;
    }
    points
}

fn holder_path(tip: &[P], off: f32, threshold_deg: f32) -> Vec<P> {
    if tip.len() < 2 {
        return tip.to_vec();
    }
    let dirs: Vec<P> = tip.windows(2).map(|w| unit(sub(w[1], w[0]))).collect();
    let mut out = vec![add(tip[0], scale(dirs[0], off))];
    for (i, &p) in tip.iter().enumerate().skip(1).take(tip.len() - 2) {
        let (a, b) = (dirs[i - 1], dirs[i]);
        out.push(add(p, scale(a, off)));
        let turn = cross(a, b).atan2(dot(a, b));
        if turn.abs().to_degrees() > threshold_deg {
            let steps = (turn.abs().to_degrees() / ARC_STEP_DEG).ceil() as usize;
            out.extend((1..steps).map(|k| add(p, scale(rotate(a, turn * k as f32 / steps as f32), off))));
        }
        out.push(add(p, scale(b, off)));
    }
    out.push(add(*tip.last().unwrap(), scale(*dirs.last().unwrap(), off)));
    out
}

fn dedup(points: &[P]) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn scale(a: P, k: f32) -> P {
    (a.0 * k, a.1 * k)
}

fn dot(a: P, b: P) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: P, b: P) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

fn dist(a: P, b: P) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn unit(a: P) -> P {
    let len = a.0.hypot(a.1);
    if len == 0.0 { (0.0, 0.0) } else { scale(a, 1.0 / len) }
}

fn rotate((x, y): P, angle: f32) -> P {
    let (s, c) = angle.sin_cos();
    (x * c - y * s, x * s + y * c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Operation;

    const KNIFE: ToolKind = ToolKind::DragKnife { offset_mm: 0.25, swivel_threshold_deg: 30.0 };

    fn pen() -> ToolKind {
        ToolKind::Pen { up: "M5".into(), down: "M3 S30".into(), settle_ms: 0 }
    }

    fn square(op: Operation) -> Stroke {
        Stroke { op, points: vec![(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 20.0)], closed: true }
    }

    fn one(stroke: Stroke, tool: &ToolKind, overcut_mm: f32) -> Stroke {
        compensate(&Toolpath { strokes: vec![stroke] }, tool, overcut_mm).strokes.remove(0)
    }

    fn near(a: P, b: P) -> bool {
        dist(a, b) < 1e-4
    }

    #[test]
    fn a_quarter_mm_knife_swings_a_quarter_mm_arc_around_each_corner_of_a_square() {
        let out = one(square(Operation::BasicCut), &KNIFE, 0.0);
        assert!(!out.closed);
        for corner in square(Operation::BasicCut).points {
            let on_arc: Vec<_> = out.points.iter().filter(|&&p| (dist(p, corner) - 0.25).abs() < 1e-4).collect();
            // 90° in 10° steps: both ends and eight points between.
            assert!(on_arc.len() >= 10, "corner {corner:?}: {on_arc:?}");
        }
        assert!(near(out.points[0], (10.0, 10.0)), "the lead-in starts the holder on the start vertex: {:?}", out.points[0]);
        assert!(near(*out.points.last().unwrap(), (10.0, 9.75)), "back to the start along the closing edge");
    }

    #[test]
    fn a_circle_of_64_segments_gets_no_arcs() {
        let points = (0..64).map(|i| std::f32::consts::TAU * i as f32 / 64.0).map(|a| (50.0 + 20.0 * a.cos(), 50.0 + 20.0 * a.sin())).collect();
        let out = one(Stroke { op: Operation::BasicCut, points, closed: true }, &KNIFE, 0.0);
        // The lead-in, 64 vertices with two holder points each, and the return to the start.
        assert_eq!(out.points.len(), 1 + 64 * 2 + 1);
    }

    #[test]
    fn a_1_mm_overcut_runs_a_closed_square_1_mm_past_its_start_along_the_first_edge() {
        let out = one(square(Operation::BasicCut), &pen(), 1.0);
        assert!(!out.closed);
        assert_eq!(out.points, [(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 20.0), (10.0, 10.0), (11.0, 10.0)]);
    }

    #[test]
    fn an_overcut_longer_than_the_first_edge_turns_the_corner() {
        let out = one(square(Operation::BasicCut), &pen(), 12.0);
        assert_eq!(&out.points[4..], [(10.0, 10.0), (20.0, 10.0), (20.0, 12.0)]);
    }

    #[test]
    fn draw_strokes_are_never_extended_or_offset() {
        for op in [Operation::Pen, Operation::Score] {
            assert_eq!(one(square(op), &KNIFE, 1.0), square(op));
        }
    }

    #[test]
    fn pen_and_laser_tools_get_no_offset() {
        let laser = ToolKind::Laser { power_max: 1000, dynamic: true };
        for tool in [pen(), laser] {
            assert_eq!(one(square(Operation::BasicCut), &tool, 0.0), square(Operation::BasicCut));
        }
    }

    #[test]
    fn knife_and_overcut_together_run_past_the_start_with_the_offset() {
        let out = one(square(Operation::BasicCut), &KNIFE, 1.0);
        assert!(near(out.points[1], (10.0, 9.75)), "the lead-in runs along the closing edge: {:?}", out.points[1]);
        assert!(near(*out.points.last().unwrap(), (11.25, 10.0)), "{:?}", out.points.last());
    }
}
