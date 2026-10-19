//! Measurements and distance lines on the canvas. Pure geometry in design inches, y pointing down.

use crate::design::Pt;
use crate::snap::Aabb;
use crate::units::Units;

/// A ruler laid between two points with the Measure tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measurement {
    pub from: Pt,
    pub to: Pt,
}

impl Measurement {
    pub fn dx(&self) -> f32 {
        self.to.0 - self.from.0
    }

    pub fn dy(&self) -> f32 {
        self.to.1 - self.from.1
    }

    pub fn distance(&self) -> f32 {
        self.dx().hypot(self.dy())
    }

    /// 0 along +x and counterclockwise positive as the screen reads, so up the screen is 90.
    pub fn angle_deg(&self) -> f32 {
        (self.from.1 - self.to.1).atan2(self.dx()).to_degrees()
    }

    /// "5 in · 3 in x 4 in · 53.13°": the distance, its horizontal and vertical spans, and the angle.
    pub fn label(&self, units: Units) -> String {
        let angle = format!("{:.2}", self.angle_deg());
        let angle = angle.trim_end_matches('0').trim_end_matches('.');
        format!("{} · {} · {angle}°", units.len(self.distance()), units.size(self.dx().abs(), self.dy().abs()))
    }
}

/// `to` moved onto the nearest 45 degree ray from `from`, keeping its reach along that ray.
pub fn constrain_45(from: Pt, to: Pt) -> Pt {
    let (vx, vy) = (to.0 - from.0, to.1 - from.1);
    if vx == 0.0 && vy == 0.0 {
        return to;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let (sin, cos) = ((vy.atan2(vx) / step).round() * step).sin_cos();
    let reach = vx * cos + vy * sin;
    (from.0 + reach * cos, from.1 + reach * sin)
}

/// A red line Option-hover draws between the selection and the shape under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceLine {
    pub from: Pt,
    pub to: Pt,
    pub length: f32,
}

/// Figma's distance lines from `selection` to `other`. Apart on an axis, the gap on that axis; overlapping
/// on both, the distance between each pair of corresponding edges. Lines run through the middle of the
/// span the boxes share on the other axis, or through the selection's middle where they share none.
pub fn distance_lines(selection: Aabb, other: Aabb) -> Vec<DistanceLine> {
    let s = [(selection.min_x, selection.max_x), (selection.min_y, selection.max_y)];
    let o = [(other.min_x, other.max_x), (other.min_y, other.max_y)];
    let through = |axis: usize| {
        let (lo, hi) = (s[axis].0.max(o[axis].0), s[axis].1.min(o[axis].1));
        if lo <= hi { (lo + hi) / 2.0 } else { (s[axis].0 + s[axis].1) / 2.0 }
    };
    let apart = |axis: usize| s[axis].1 < o[axis].0 || o[axis].1 < s[axis].0;
    let mut spans = Vec::new();
    if apart(0) || apart(1) {
        for axis in [0, 1].into_iter().filter(|&a| apart(a)) {
            spans.push(if s[axis].1 < o[axis].0 { (axis, s[axis].1, o[axis].0) } else { (axis, s[axis].0, o[axis].1) });
        }
    } else {
        for axis in [0, 1] {
            spans.push((axis, s[axis].0, o[axis].0));
            spans.push((axis, s[axis].1, o[axis].1));
        }
    }
    spans
        .into_iter()
        .filter(|&(_, a, b)| a != b)
        .map(|(axis, a, b)| {
            let at = through(1 - axis);
            let (from, to) = if axis == 0 { ((a, at), (b, at)) } else { ((at, a), (at, b)) };
            DistanceLine { from, to, length: (b - a).abs() }
        })
        .collect()
}

/// What a move or resize under way has changed since the pointer went down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Readout {
    Moved { dx: f32, dy: f32 },
    Resized { w: f32, h: f32 },
}

impl Readout {
    pub fn label(&self, units: Units) -> String {
        match *self {
            Self::Moved { dx, dy } => format!("x {}, y {}", units.len(dx), units.len(dy)),
            Self::Resized { w, h } => units.size(w, h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn aabb(x: f32, y: f32, w: f32, h: f32) -> Aabb {
        Aabb { min_x: x, min_y: y, max_x: x + w, max_y: y + h }
    }

    #[test]
    fn the_diagonal_of_a_3_by_4_rectangle_is_5_at_53_degrees() {
        use crate::snap::{Candidate, Kind, SnapOptions, Targets, snap};
        let rect = aabb(1.0, 1.0, 3.0, 4.0);
        let corners = [(rect.min_x, rect.min_y), (rect.max_x, rect.min_y), (rect.min_x, rect.max_y), (rect.max_x, rect.max_y)];
        let targets = Targets { points: corners.iter().map(|&p| (p, Kind::Node)).collect(), threshold: 0.1, ..Targets::default() };
        let snap_to = |p: Pt| {
            let s = snap(&Candidate { points: vec![(p, Kind::Node)], frame: None }, &targets, &SnapOptions::default());
            (p.0 + s.dx, p.1 + s.dy)
        };
        let m = Measurement { from: snap_to((1.04, 4.97)), to: snap_to((3.97, 1.02)) };
        assert!(close(m.from.0, 1.0) && close(m.from.1, 5.0), "from {:?}", m.from);
        assert!(close(m.to.0, 4.0) && close(m.to.1, 1.0), "to {:?}", m.to);
        assert!(close(m.distance(), 5.0), "distance {}", m.distance());
        assert!(close(m.angle_deg(), 53.1301), "angle {}", m.angle_deg());
        assert!(close(m.dx(), 3.0) && close(m.dy(), -4.0), "components {} {}", m.dx(), m.dy());
    }

    #[test]
    fn angles_read_counterclockwise_as_on_screen() {
        let angle = |to: Pt| Measurement { from: (0.0, 0.0), to }.angle_deg();
        assert!(close(angle((1.0, 0.0)), 0.0));
        assert!(close(angle((0.0, -1.0)), 90.0), "up the screen is 90");
        assert!(close(angle((-1.0, 0.0)), 180.0));
        assert!(close(angle((0.0, 1.0)), -90.0), "down the screen is -90");
    }

    #[test]
    fn constrain_45_projects_onto_the_nearest_eighth_of_a_turn() {
        let c = constrain_45((1.0, 1.0), (4.0, 1.3));
        assert!(close(c.0, 4.0) && close(c.1, 1.0), "nearly flat goes flat: {c:?}");
        let c = constrain_45((0.0, 0.0), (0.2, -3.0));
        assert!(close(c.0, 0.0) && close(c.1, -3.0), "nearly vertical goes vertical: {c:?}");
        let c = constrain_45((0.0, 0.0), (2.0, -1.8));
        assert!(close(c.0, 1.9) && close(c.1, -1.9), "near the diagonal lands on it: {c:?}");
        assert_eq!(constrain_45((2.0, 2.0), (2.0, 2.0)), (2.0, 2.0));
    }

    #[test]
    fn the_label_gives_distance_components_and_angle_in_the_current_units() {
        let m = Measurement { from: (0.0, 4.0), to: (3.0, 0.0) };
        assert_eq!(m.label(Units::Imperial), "5 in · 3 in x 4 in · 53.13°");
        assert_eq!(m.label(Units::Metric), "12.7 cm · 7.62 cm x 10.16 cm · 53.13°");
        assert_eq!(Measurement { from: (0.0, 0.0), to: (2.0, 0.0) }.label(Units::Imperial), "2 in · 2 in x 0 in · 0°");
    }

    fn sorted(mut lines: Vec<DistanceLine>) -> Vec<DistanceLine> {
        lines.sort_by(|a, b| a.from.0.total_cmp(&b.from.0).then(a.from.1.total_cmp(&b.from.1)));
        lines
    }

    #[test]
    fn boxes_apart_on_one_axis_show_the_gap_across_their_shared_span() {
        let lines = distance_lines(aabb(0.0, 0.0, 2.0, 2.0), aabb(5.0, 1.0, 2.0, 4.0));
        assert_eq!(lines, vec![DistanceLine { from: (2.0, 1.5), to: (5.0, 1.5), length: 3.0 }]);
        let lines = distance_lines(aabb(0.0, 6.0, 2.0, 2.0), aabb(0.0, 0.0, 2.0, 1.0));
        assert_eq!(lines, vec![DistanceLine { from: (1.0, 6.0), to: (1.0, 1.0), length: 5.0 }]);
    }

    #[test]
    fn diagonally_apart_boxes_show_a_gap_on_each_axis() {
        let lines = sorted(distance_lines(aabb(0.0, 0.0, 2.0, 2.0), aabb(4.0, 5.0, 1.0, 1.0)));
        assert_eq!(lines, vec![DistanceLine { from: (1.0, 2.0), to: (1.0, 5.0), length: 3.0 }, DistanceLine { from: (2.0, 1.0), to: (4.0, 1.0), length: 2.0 },]);
    }

    #[test]
    fn a_nested_box_shows_its_distance_to_each_edge_of_its_container() {
        let lines = sorted(distance_lines(aabb(2.0, 3.0, 2.0, 1.0), aabb(0.0, 0.0, 10.0, 5.0)));
        assert_eq!(
            lines,
            vec![
                DistanceLine { from: (2.0, 3.5), to: (0.0, 3.5), length: 2.0 },
                DistanceLine { from: (3.0, 3.0), to: (3.0, 0.0), length: 3.0 },
                DistanceLine { from: (3.0, 4.0), to: (3.0, 5.0), length: 1.0 },
                DistanceLine { from: (4.0, 3.5), to: (10.0, 3.5), length: 6.0 },
            ]
        );
    }

    #[test]
    fn overlapping_boxes_show_corresponding_edges_and_skip_shared_ones() {
        let lines = sorted(distance_lines(aabb(0.0, 0.0, 4.0, 4.0), aabb(1.0, 0.0, 5.0, 2.0)));
        assert_eq!(
            lines,
            vec![
                DistanceLine { from: (0.0, 1.0), to: (1.0, 1.0), length: 1.0 },
                DistanceLine { from: (2.5, 4.0), to: (2.5, 2.0), length: 2.0 },
                DistanceLine { from: (4.0, 1.0), to: (6.0, 1.0), length: 2.0 },
            ]
        );
    }

    #[test]
    fn a_readout_names_the_move_or_the_new_size() {
        assert_eq!(Readout::Moved { dx: 1.5, dy: -0.25 }.label(Units::Imperial), "x 1.5 in, y -0.25 in");
        assert_eq!(Readout::Resized { w: 2.0, h: 1.0 }.label(Units::Metric), "5.08 cm x 2.54 cm");
    }
}
