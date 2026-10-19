//! Machine order: plot order, then inside-out, then nearest neighbour from the origin.

use crate::composition::plot_order;

use super::toolpath::{Stroke, Toolpath};

type P = (f32, f32);

/// Draws before cuts, each stroke after every stroke inside it, and otherwise the nearest next entry
/// from the head, which starts at the work area's top-left.
pub fn order(path: Toolpath) -> Toolpath {
    let mut strokes = path.strokes;
    strokes.sort_by_key(|s| plot_order(s.op));
    let mut out = Vec::with_capacity(strokes.len());
    let mut head = (0.0, 0.0);
    while let Some(first) = strokes.first() {
        let group = plot_order(first.op);
        let len = strokes.iter().take_while(|s| plot_order(s.op) == group).count();
        let rest = strokes.split_off(len);
        head = inside_out_nearest(strokes, head, &mut out);
        strokes = rest;
    }
    Toolpath { strokes: out }
}

/// Appends one plot-order group to `out` and returns where the head ends.
fn inside_out_nearest(group: Vec<Stroke>, mut head: P, out: &mut Vec<Stroke>) -> P {
    let boxes: Vec<_> = group.iter().map(|s| bounds(&s.points)).collect();
    let contains = |a: usize, b: usize| a != b && group[a].closed && inside_box(boxes[a], group[b].points[0]) && inside(&group[a].points, group[b].points[0]);
    let mut waiting_on: Vec<usize> = (0..group.len()).map(|a| (0..group.len()).filter(|&b| contains(a, b)).count()).collect();
    let mut done = vec![false; group.len()];
    for _ in 0..group.len() {
        let open = || (0..group.len()).filter(|&i| !done[i]);
        let ready: Vec<usize> = open().filter(|&i| waiting_on[i] == 0).collect();
        // Strokes whose first points sit inside each other wait on each other; break the cycle by distance.
        let candidates = if ready.is_empty() { open().collect() } else { ready };
        let next = candidates.into_iter().map(|i| (i, entry(&group[i], head))).min_by(|a, b| a.1.1.total_cmp(&b.1.1));
        let Some((i, (stroke, _))) = next else { break };
        done[i] = true;
        for (a, waiting) in waiting_on.iter_mut().enumerate() {
            if contains(a, i) {
                *waiting -= 1;
            }
        }
        head = if stroke.closed { stroke.points[0] } else { *stroke.points.last().unwrap() };
        out.push(stroke);
    }
    head
}

/// The stroke as the head enters it, nearest end or vertex first, with the squared entry distance.
fn entry(stroke: &Stroke, head: P) -> (Stroke, f32) {
    let d = |p: &P| (p.0 - head.0).powi(2) + (p.1 - head.1).powi(2);
    let mut s = stroke.clone();
    if s.closed {
        let (start, dist) = s.points.iter().map(d).enumerate().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        s.points.rotate_left(start);
        return (s, dist);
    }
    let (first, last) = (d(&s.points[0]), d(s.points.last().unwrap()));
    if last < first {
        s.points.reverse();
    }
    (s, first.min(last))
}

fn bounds(points: &[P]) -> (f32, f32, f32, f32) {
    points.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
}

fn inside_box((x0, y0, x1, y1): (f32, f32, f32, f32), (x, y): P) -> bool {
    (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
}

/// Even-odd point in polygon.
fn inside(ring: &[P], (px, py): P) -> bool {
    let mut result = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let ((xi, yi), (xj, yj)) = (ring[i], ring[j]);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            result = !result;
        }
        j = i;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Design, Operation, ShapeKind};
    use crate::output::toolpath::{self, tests::*};
    use crate::shape_library::BuiltinId;
    use egui::Color32;

    fn square(x: f32, y: f32, side: f32) -> Stroke {
        Stroke { op: Operation::BasicCut, points: vec![(x, y), (x + side, y), (x + side, y + side), (x, y + side)], closed: true }
    }

    fn area(s: &Stroke) -> f32 {
        let (x0, y0, x1, y1) = bounds(&s.points);
        (x1 - x0) * (y1 - y0)
    }

    fn box_inside(inner: &Stroke, outer: &Stroke) -> bool {
        let ((a, b, c, d), (e, f, g, h)) = (bounds(&inner.points), bounds(&outer.points));
        inner != outer && a >= e && b >= f && c <= g && d <= h
    }

    /// Asserts every stroke whose bounding box sits inside another's comes earlier, and counts such pairs.
    fn inner_first(path: &Toolpath) -> usize {
        let mut pairs = 0;
        for (i, outer) in path.strokes.iter().enumerate() {
            for (j, inner) in path.strokes.iter().enumerate() {
                if box_inside(inner, outer) {
                    assert!(j < i, "stroke {j} sits inside stroke {i} but runs after it");
                    pairs += 1;
                }
            }
        }
        pairs
    }

    fn built(design: &Design) -> Toolpath {
        toolpath::build(design, &only_mat(design), &setup(), 0.05)
    }

    /// Head travel from the origin through each stroke's first point.
    fn travel(path: &Toolpath) -> f32 {
        let mut head = (0.0f32, 0.0f32);
        let mut total = 0.0;
        for s in &path.strokes {
            total += (s.points[0].0 - head.0).hypot(s.points[0].1 - head.1);
            head = if s.closed { s.points[0] } else { *s.points.last().unwrap() };
        }
        total
    }

    #[test]
    fn the_rings_inner_contour_runs_before_its_outer_one() {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Builtin(BuiltinId::named("Ring").unwrap()));
        let path = order(built(&design));
        assert_eq!(path.strokes.len(), 2);
        assert!(area(&path.strokes[0]) < area(&path.strokes[1]), "inner ring first");
        assert_eq!(inner_first(&path), 1);
    }

    #[test]
    fn each_counter_of_boa_runs_before_its_letter() {
        let mut design = Design::default();
        design.add_text("BOA".into(), crate::text::FontRef::Inter, 1.0, Color32::BLACK, 1.0, 1.0);
        design.shapes[0].operation = Operation::BasicCut;
        let canvas = built(&design);
        let mut reversed = canvas.clone();
        reversed.strokes.reverse();
        for input in [canvas, reversed] {
            let path = order(input);
            assert_eq!(inner_first(&path), 4, "B has two counters, O and A one each");
        }
    }

    #[test]
    fn a_cards_outline_runs_after_everything_on_it() {
        let mut design = Design::default();
        let card = design.add_shape(ShapeKind::Rectangle);
        let heart = design.add_shape(ShapeKind::Heart);
        let star = design.add_shape(ShapeKind::Star);
        for (id, (x, y, w, h)) in [(card, (0.0, 0.0, 5.0, 4.0)), (heart, (0.5, 0.5, 1.5, 1.5)), (star, (2.5, 1.5, 1.5, 1.5))] {
            let s = design.shape_mut(id).unwrap();
            (s.x_in, s.y_in, s.width_in, s.height_in, s.operation) = (x, y, w, h, Operation::BasicCut);
        }
        design.attach(&[card, heart, star]).unwrap();
        let path = order(built(&design));
        assert_eq!(path.strokes.len(), 3);
        assert_eq!(area(path.strokes.last().unwrap()), path.strokes.iter().map(area).fold(0.0, f32::max), "card last");
        assert_eq!(inner_first(&path), 2);
    }

    #[test]
    fn draws_finish_before_cuts() {
        let mut pen = square(0.0, 0.0, 5.0);
        pen.op = Operation::Pen;
        let path = order(Toolpath { strokes: vec![square(1.0, 1.0, 1.0), pen] });
        assert_eq!(path.strokes[0].op, Operation::Pen);
    }

    #[test]
    fn a_grid_starts_top_left_and_sweeps_the_top_row_first() {
        let (tl, tr, bl, br) = (square(10.0, 10.0, 20.0), square(35.0, 10.0, 20.0), square(10.0, 45.0, 20.0), square(35.0, 45.0, 20.0));
        let path = order(Toolpath { strokes: vec![br.clone(), bl.clone(), tr.clone(), tl.clone()] });
        let visited: Vec<_> = path.strokes.iter().map(|s| bounds(&s.points)).collect();
        // A closed stroke ends where it began, so from the top-right square the one below it is nearest.
        let expected: Vec<_> = [tl, tr, br, bl].iter().map(|s| bounds(&s.points)).collect();
        assert_eq!(visited, expected);
    }

    #[test]
    fn nearest_neighbour_travels_less_than_canvas_order_on_a_shuffled_layout() {
        let cells: Vec<_> = (0..16).map(|i| square((i % 4) as f32 * 30.0, (i / 4) as f32 * 30.0, 20.0)).collect();
        let shuffled = Toolpath { strokes: [13, 2, 7, 0, 11, 4, 15, 9, 1, 14, 6, 3, 10, 5, 12, 8].iter().map(|&i| cells[i].clone()).collect() };
        let ordered = order(shuffled.clone());
        assert_eq!(ordered.strokes.len(), 16);
        assert!(travel(&ordered) < travel(&shuffled) / 2.0, "{} vs {}", travel(&ordered), travel(&shuffled));
    }

    #[test]
    fn a_closed_ring_starts_at_the_vertex_nearest_the_head() {
        let ring = Stroke { op: Operation::BasicCut, points: vec![(30.0, 30.0), (10.0, 30.0), (10.0, 10.0), (30.0, 10.0)], closed: true };
        let path = order(Toolpath { strokes: vec![ring] });
        assert_eq!(path.strokes[0].points, [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)]);
    }

    #[test]
    fn an_open_stroke_reverses_when_its_end_is_nearer() {
        let line = |a, b| Stroke { op: Operation::Score, points: vec![a, b], closed: false };
        let path = order(Toolpath { strokes: vec![line((100.0, 0.0), (5.0, 0.0)), line((50.0, 50.0), (110.0, 0.0))] });
        assert_eq!(path.strokes[0].points, [(5.0, 0.0), (100.0, 0.0)]);
        assert_eq!(path.strokes[1].points, [(110.0, 0.0), (50.0, 50.0)], "the head is at (100, 0)");
    }
}
