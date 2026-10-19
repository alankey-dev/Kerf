//! Cubic Bezier to circular arcs, for DXF's bulged polyline vertices. Each cubic becomes a biarc (two
//! tangent-continuous arcs meeting at a point chosen so both arcs' tangent legs are equal); one that
//! strays past the tolerance is split in half and fitted again.

use crate::design::Pt;

/// An arc (or a straight line, when `bulge` is 0) from the previous point to `end`. `bulge` is
/// tan(sweep / 4), positive counterclockwise in a Y-up frame, as DXF stores it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub end: Pt,
    pub bulge: f32,
}

const MAX_DEPTH: u32 = 10;
const SAMPLES: usize = 16;
const EPS: f32 = 1e-6;

/// Arcs from `p0` along the cubic `p0, c1, c2, p3`, each within `tol` of the curve.
pub fn fit(p0: Pt, c1: Pt, c2: Pt, p3: Pt, tol: f32) -> Vec<Arc> {
    let mut out = Vec::new();
    fit_into([p0, c1, c2, p3], tol, 0, &mut out);
    out
}

fn fit_into(cubic: [Pt; 4], tol: f32, depth: u32, out: &mut Vec<Arc>) {
    let [p0, _, _, p3] = cubic;
    if dist(p0, p3) < EPS && dist(p0, cubic[1]) < EPS && dist(p0, cubic[2]) < EPS {
        return;
    }
    if let Some(arcs) = biarc(cubic).filter(|arcs| error(cubic, p0, arcs) <= tol) {
        out.extend(arcs);
        return;
    }
    if depth >= MAX_DEPTH {
        out.extend((1..=SAMPLES).map(|i| Arc { end: at(cubic, i as f32 / SAMPLES as f32), bulge: 0.0 }));
        return;
    }
    let (a, b) = split(cubic);
    fit_into(a, tol, depth + 1, out);
    fit_into(b, tol, depth + 1, out);
}

/// The two arcs, or one line when the cubic is straight. `None` when the tangents admit no biarc.
fn biarc(cubic: [Pt; 4]) -> Option<Vec<Arc>> {
    let [p0, c1, c2, p3] = cubic;
    let t0 = unit(first_nonzero(p0, &[c1, c2, p3])?);
    let t1 = unit(first_nonzero_rev(p3, &[c2, c1, p0])?);
    let v = sub(p3, p0);
    let off_line = |t: Pt| cross(t, v).abs() / len(v).max(EPS);
    if off_line(t0) < 1e-5 && off_line(t1) < 1e-5 && dot(t0, v) > 0.0 && dot(t1, v) > 0.0 {
        return Some(vec![Arc { end: p3, bulge: 0.0 }]);
    }
    let t = add(t0, t1);
    let denom = 2.0 * (1.0 - dot(t0, t1));
    let vt = dot(v, t);
    let d = if denom.abs() < EPS {
        if vt <= EPS {
            return None;
        }
        dot(v, v) / (2.0 * vt)
    } else {
        (-vt + (vt * vt + denom * dot(v, v)).sqrt()) / denom
    };
    if !d.is_finite() || d <= 0.0 {
        return None;
    }
    let (q0, q1) = (add(p0, scale(t0, d)), sub(p3, scale(t1, d)));
    let joint = scale(add(q0, q1), 0.5);
    let joint_tangent = sub(q1, q0);
    let a = Arc { end: joint, bulge: bulge_from_tangent(p0, t0, joint)? };
    let b = Arc { end: p3, bulge: bulge_from_tangent(joint, joint_tangent, p3)? };
    Some(vec![a, b])
}

/// tan(sweep / 4) for the arc leaving `from` along `tangent` and ending at `to`. The tangent-chord angle
/// is half the sweep.
fn bulge_from_tangent(from: Pt, tangent: Pt, to: Pt) -> Option<f32> {
    let chord = sub(to, from);
    let half_sweep = cross(tangent, chord).atan2(dot(tangent, chord));
    let bulge = (half_sweep / 2.0).tan();
    (bulge.is_finite() && half_sweep.abs() < std::f32::consts::PI * 0.99).then_some(bulge)
}

/// The arc's centre and radius, or `None` for a straight segment.
pub fn centre_radius(start: Pt, arc: Arc) -> Option<(Pt, f32)> {
    if arc.bulge.abs() < EPS {
        return None;
    }
    let chord = sub(arc.end, start);
    let l = len(chord);
    let sagitta = arc.bulge.abs() * l / 2.0;
    let radius = (l * l / 4.0 + sagitta * sagitta) / (2.0 * sagitta);
    let left = (-chord.1 / l, chord.0 / l);
    let mid = scale(add(start, arc.end), 0.5);
    Some((add(mid, scale(left, arc.bulge.signum() * (radius - sagitta))), radius))
}

/// Points along the arc from `start`, excluding `start`, `n` steps.
#[cfg(test)]
pub fn sample(start: Pt, arc: Arc, n: usize) -> Vec<Pt> {
    let Some((centre, radius)) = centre_radius(start, arc) else {
        return (1..=n).map(|i| lerp(start, arc.end, i as f32 / n as f32)).collect();
    };
    let sweep = 4.0 * arc.bulge.atan();
    let a0 = (start.1 - centre.1).atan2(start.0 - centre.0);
    (1..=n).map(|i| a0 + sweep * i as f32 / n as f32).map(|a| (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())).collect()
}

/// The furthest any sampled point of the cubic lies from the nearest arc.
fn error(cubic: [Pt; 4], start: Pt, arcs: &[Arc]) -> f32 {
    let mut from = start;
    let spans: Vec<(Pt, Arc)> = arcs
        .iter()
        .map(|&a| {
            let span = (from, a);
            from = a.end;
            span
        })
        .collect();
    (1..SAMPLES).map(|i| at(cubic, i as f32 / SAMPLES as f32)).map(|p| spans.iter().map(|&(s, a)| distance_to_arc(p, s, a)).fold(f32::MAX, f32::min)).fold(0.0, f32::max)
}

fn distance_to_arc(p: Pt, start: Pt, arc: Arc) -> f32 {
    let along_chord = |p: Pt| {
        let chord = sub(arc.end, start);
        let t = (dot(sub(p, start), chord) / dot(chord, chord).max(EPS)).clamp(0.0, 1.0);
        dist(p, lerp(start, arc.end, t))
    };
    match centre_radius(start, arc) {
        None => along_chord(p),
        Some((centre, radius)) => {
            let on_circle = (dist(p, centre) - radius).abs();
            let ends = dist(p, start).min(dist(p, arc.end));
            if within_sweep(p, start, arc, centre) { on_circle } else { ends }
        }
    }
}

fn within_sweep(p: Pt, start: Pt, arc: Arc, centre: Pt) -> bool {
    let sweep = 4.0 * arc.bulge.atan();
    let angle = |q: Pt| (q.1 - centre.1).atan2(q.0 - centre.0);
    let tau = std::f32::consts::TAU;
    let rel = ((angle(p) - angle(start)) * sweep.signum()).rem_euclid(tau);
    rel <= sweep.abs() + 1e-4
}

fn split(c: [Pt; 4]) -> ([Pt; 4], [Pt; 4]) {
    let m = |a: Pt, b: Pt| lerp(a, b, 0.5);
    let (ab, bc, cd) = (m(c[0], c[1]), m(c[1], c[2]), m(c[2], c[3]));
    let (abc, bcd) = (m(ab, bc), m(bc, cd));
    let mid = m(abc, bcd);
    ([c[0], ab, abc, mid], [mid, bcd, cd, c[3]])
}

fn at(c: [Pt; 4], t: f32) -> Pt {
    let u = 1.0 - t;
    let (a, b, cc, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * c[0].0 + b * c[1].0 + cc * c[2].0 + d * c[3].0, a * c[0].1 + b * c[1].1 + cc * c[2].1 + d * c[3].1)
}

fn first_nonzero(from: Pt, towards: &[Pt]) -> Option<Pt> {
    towards.iter().map(|&p| sub(p, from)).find(|v| len(*v) > EPS)
}

fn first_nonzero_rev(to: Pt, from: &[Pt]) -> Option<Pt> {
    from.iter().map(|&p| sub(to, p)).find(|v| len(*v) > EPS)
}

fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

fn scale(a: Pt, s: f32) -> Pt {
    (a.0 * s, a.1 * s)
}

fn dot(a: Pt, b: Pt) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: Pt, b: Pt) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

fn len(a: Pt) -> f32 {
    dot(a, a).sqrt()
}

fn dist(a: Pt, b: Pt) -> f32 {
    len(sub(a, b))
}

fn unit(a: Pt) -> Pt {
    scale(a, 1.0 / len(a))
}

fn lerp(a: Pt, b: Pt, t: f32) -> Pt {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KAPPA: f32 = 0.552_284_8;

    #[test]
    fn a_quarter_circle_cubic_fits_arcs_of_its_radius() {
        let r = 2.0;
        let arcs = fit((r, 0.0), (r, r * KAPPA), (r * KAPPA, r), (0.0, r), 0.001);
        assert!(!arcs.is_empty());
        let mut from = (r, 0.0);
        for arc in arcs {
            let (centre, radius) = centre_radius(from, arc).expect("an arc, not a line");
            assert!((radius - r).abs() < 0.001, "radius {radius}");
            assert!(dist(centre, (0.0, 0.0)) < 0.002, "centre {centre:?}");
            assert!(arc.bulge > 0.0, "counterclockwise");
            from = arc.end;
        }
        assert!(dist(from, (0.0, r)) < 1e-5);
    }

    #[test]
    fn a_straight_cubic_is_one_line_with_zero_bulge() {
        assert_eq!(fit((0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), 0.001), [Arc { end: (3.0, 3.0), bulge: 0.0 }]);
    }

    #[test]
    fn an_s_curve_stays_within_tolerance() {
        let cubic = [(0.0, 0.0), (1.0, 2.0), (2.0, -2.0), (3.0, 0.0)];
        let arcs = fit(cubic[0], cubic[1], cubic[2], cubic[3], 0.001);
        let mut points = vec![cubic[0]];
        for arc in &arcs {
            let from = *points.last().unwrap();
            points.extend(sample(from, *arc, 8));
        }
        for i in 0..=100 {
            let p = at(cubic, i as f32 / 100.0);
            let near = points.windows(2).map(|w| distance_to_arc(p, w[0], Arc { end: w[1], bulge: 0.0 })).fold(f32::MAX, f32::min);
            assert!(near < 0.002, "{p:?} is {near} from the arcs");
        }
    }
}
