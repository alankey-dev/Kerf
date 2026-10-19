//! Flatten: several shapes held as one Print Then Cut shape. Rather than merging the layers into one
//! printable image, Kerf keeps the parts as vectors so the canvas can draw each in its own fill,
//! and cuts the weld of their outlines.

use crate::design::{Contour, Design, Operation, Pt, Seg, Shape, ShapeKind};
use crate::effects::{Ink, Lines, Paint, image_corners};
use crate::geometry::{Boolean, FLATTEN_TOL_IN, Ring, boolean};

impl Design {
    /// Flattens `ids` into one shape at the back-most input's z position. Returns its id.
    pub fn flatten(&mut self, ids: &[u64]) -> Option<u64> {
        let shape = flattened(self.next_id(), self.subtree_parts(ids))?;
        self.replace_with(ids, vec![shape]).first().copied()
    }

    /// Puts a flattened shape's parts back on the canvas where it was, with fresh ids and links.
    /// Returns the restored ids, or nothing when `id` is not a flattened shape.
    pub fn unflatten(&mut self, id: u64) -> Vec<u64> {
        let Some(flat) = self.shapes.iter().find(|s| s.id == id) else { return Vec::new() };
        let ShapeKind::Flattened { parts, view_box } = &flat.kind else { return Vec::new() };
        let restored = (self.next_id()..).zip(parts).map(|(id, part)| Shape { id, container: None, parent: None, ..placed_part(flat, part, *view_box) }).collect();
        let ids = self.replace_with(&[id], restored);
        self.relink_copies(&ids);
        ids
    }
}

/// A Print Then Cut shape holding world-space `parts` (back to front), placed at their combined bounds.
/// `None` when the parts have no outline at all.
pub fn flattened(id: u64, parts: Vec<Shape>) -> Option<Shape> {
    let crate::snap::Aabb { min_x, min_y, max_x, max_y } =
        parts.iter().map(|p| crate::effects::evaluate(p).bounds()).filter(|b| b.min_x <= b.max_x).reduce(crate::snap::Aabb::union)?;
    let view_box = ((max_x - min_x).max(0.01), (max_y - min_y).max(0.01));
    let (fill, pen) = (parts[0].fill, parts[0].pen);
    let parts = parts.into_iter().map(|p| Shape { x_in: p.x_in - min_x, y_in: p.y_in - min_y, ..p }).collect();
    Some(Shape {
        x_in: min_x,
        y_in: min_y,
        width_in: view_box.0,
        height_in: view_box.1,
        fill,
        operation: Operation::PrintThenCut,
        pen,
        ..Shape::new(id, ShapeKind::Flattened { parts, view_box })
    })
}

/// The cut outline: the weld of every visible part, scaled from `view_box` to a `size` box.
pub fn outline(parts: &[Shape], view_box: Pt, size: Pt) -> Vec<Contour> {
    let groups: Vec<Vec<Ring>> = parts.iter().filter(|p| !p.hidden).map(|p| crate::effects::evaluate(p).rings(FLATTEN_TOL_IN)).collect();
    let (sx, sy) = scale(view_box, size);
    boolean(&groups, Boolean::Weld)
        .into_iter()
        .filter(|r| r.len() >= 3)
        .map(|r| {
            let mut pts = r.into_iter().map(|(x, y)| (x * sx, y * sy));
            let start = pts.next().unwrap();
            Contour { start, segs: pts.map(Seg::Line).collect(), closed: true }
        })
        .collect()
}

/// What the canvas fills for `shape`, back to front: its own outline in its fill, an image's pixels
/// inside its cut line, or for a flattened shape every visible part in the part's own ink.
pub fn paint_layers(shape: &Shape) -> Vec<(Lines, Ink)> {
    let geometry = crate::effects::evaluate(shape);
    geometry
        .pieces()
        .iter()
        .flat_map(|p| match &p.paint {
            Paint::Fill => vec![(p.lines.clone(), Ink::Colour(shape.fill))],
            Paint::Image(asset) => {
                let corners = image_corners((shape.width_in, shape.height_in), crate::effects::Placement::of(shape).placer());
                vec![(p.lines.clone(), Ink::Image { asset: asset.clone(), corners, repeat: false })]
            }
            Paint::Print(layers) => layers.clone(),
        })
        .collect()
}

/// `part` moved from the flattened shape's local frame to the canvas, following the flattened shape's
/// scale, flip and rotation. A rotated part under an uneven scale keeps its own aspect ratio.
fn placed_part(flat: &Shape, part: &Shape, view_box: Pt) -> Shape {
    let (sx, sy) = scale(view_box, (flat.width_in, flat.height_in));
    let (cx, cy) = part.centre_in();
    let (wx, wy) = flat.to_world((cx * sx, cy * sy));
    let (w, h) = (part.width_in * sx, part.height_in * sy);
    // A single-axis mirror reverses the sense of the part's own rotation.
    let turn = if flat.flip_x != flat.flip_y { -part.rotation_deg } else { part.rotation_deg };
    Shape {
        x_in: wx - w / 2.0,
        y_in: wy - h / 2.0,
        width_in: w,
        height_in: h,
        rotation_deg: flat.rotation_deg + turn,
        flip_x: part.flip_x != flat.flip_x,
        flip_y: part.flip_y != flat.flip_y,
        ..part.clone()
    }
}

pub(crate) fn scale(from: Pt, to: Pt) -> Pt {
    let factor = |to: f32, from: f32| if from > 0.0 { to / from } else { 1.0 };
    (factor(to.0, from.0), factor(to.1, from.1))
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;
    use crate::effects::Lines as Polylines;
    use crate::links::LinkKind;

    fn square(id: u64, x: f32, y: f32, fill: Color32) -> Shape {
        Shape { x_in: x, y_in: y, width_in: 2.0, height_in: 2.0, fill, operation: Operation::BasicCut, pen: Default::default(), ..Shape::new(id, ShapeKind::Square) }
    }

    fn inputs() -> Vec<Shape> {
        let mut star = Shape { kind: ShapeKind::Star, rotation_deg: 30.0, flip_x: true, ..square(2, 1.5, 0.5, Color32::RED) };
        star.width_in = 1.5;
        vec![square(1, 0.0, 0.0, Color32::BLUE), star, square(3, 5.0, 4.0, Color32::GREEN)]
    }

    fn assert_close(a: &Shape, b: &Shape) {
        let near = |x: f32, y: f32| (x - y).abs() < 0.001;
        assert!(near(a.x_in, b.x_in) && near(a.y_in, b.y_in), "position {:?} vs {:?}", (a.x_in, a.y_in), (b.x_in, b.y_in));
        assert!(near(a.width_in, b.width_in) && near(a.height_in, b.height_in), "size {:?} vs {:?}", (a.width_in, a.height_in), (b.width_in, b.height_in));
        assert!(near(a.rotation_deg.rem_euclid(360.0), b.rotation_deg.rem_euclid(360.0)), "rotation {} vs {}", a.rotation_deg, b.rotation_deg);
        assert_eq!((a.flip_x, a.flip_y, a.fill, a.operation, &a.kind), (b.flip_x, b.flip_y, b.fill, b.operation, &b.kind));
    }

    #[test]
    fn flatten_then_unflatten_restores_the_inputs() {
        let mut d = Design { shapes: inputs(), ..Default::default() };
        let flat = d.flatten(&[1, 2, 3]).expect("flattens");
        assert_eq!(d.shapes.len(), 1);
        assert_eq!(d.shapes[0].operation, Operation::PrintThenCut);
        let restored = d.unflatten(flat);
        assert_eq!(restored.len(), 3);
        for (a, b) in d.shapes.iter().zip(inputs()) {
            assert_close(a, &b);
        }
    }

    #[test]
    fn unflatten_follows_a_moved_rotated_and_mirrored_flattened_shape() {
        let mut d = Design { shapes: inputs(), ..Default::default() };
        let flat = d.flatten(&[1, 2, 3]).unwrap();
        let s = d.shape_mut(flat).unwrap();
        (s.x_in, s.rotation_deg, s.flip_y) = (s.x_in + 3.0, 70.0, true);
        let drawn: Vec<Polylines> = paint_layers(&d.shapes[0]).into_iter().map(|(lines, _)| lines).collect();
        d.unflatten(flat);
        for (shape, lines) in d.shapes.iter().zip(drawn) {
            for ((a, _), (b, _)) in shape.world_polylines().iter().zip(&lines) {
                for (p, q) in a.iter().zip(b) {
                    assert!((p.0 - q.0).abs() < 0.001 && (p.1 - q.1).abs() < 0.001, "{p:?} vs {q:?}");
                }
            }
        }
    }

    #[test]
    fn the_cut_outline_is_the_weld_and_each_part_keeps_its_fill() {
        let mut d = Design { shapes: vec![square(1, 0.0, 0.0, Color32::BLUE), square(2, 1.0, 0.0, Color32::RED)], ..Default::default() };
        d.flatten(&[1, 2]).unwrap();
        let flat = &d.shapes[0];
        assert_eq!(flat.contours().len(), 1, "one welded outline");
        let rings = flat.world_rings(FLATTEN_TOL_IN);
        assert!((crate::geometry::even_odd_area(&rings) - 6.0).abs() < 1e-2, "3 x 2 union");
        let fills: Vec<Ink> = paint_layers(flat).into_iter().map(|(_, fill)| fill).collect();
        assert_eq!(fills, [Ink::Colour(Color32::BLUE), Ink::Colour(Color32::RED)]);
    }

    #[test]
    fn unflatten_gives_a_flattened_group_fresh_ids_and_its_link_back() {
        let mut d = Design { shapes: inputs(), ..Default::default() };
        d.group(&[1, 2]).unwrap();
        let flat = d.flatten(&[1, 2]).unwrap();
        let restored = d.unflatten(flat);
        assert!(restored.iter().all(|id| *id > 3), "fresh ids {restored:?}");
        let links: Vec<_> = d.shapes.iter().filter(|s| restored.contains(&s.id)).map(|s| s.link).collect();
        assert!(links[0].is_some_and(|l| l.kind == LinkKind::Group) && links[0] == links[1], "{links:?}");
    }

    #[test]
    fn a_flattened_shape_survives_a_project_round_trip() {
        let mut d = Design { shapes: inputs(), ..Default::default() };
        d.flatten(&[1, 2, 3]).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Design>(&json).unwrap(), d);
    }
}
