//! Repeating a transform: Power Duplicate and Transform Again's remembered step, Step and repeat's grid
//! of copies, and Rotate copies' ring (spec 52). Pure: every function works in inch space on frames and
//! shapes, and the caller assigns ids, groups the copies and makes the undo step.

use crate::design::{Design, Pt, Shape};
use crate::transform::{self, Frame};

/// The last duplicate's step, kept on the document so the next Cmd+D repeats it and Transform Again
/// applies it without a copy. `from` is the pose of the shapes the last duplicate was made from, `to`
/// the pose of the copies it made, and `ids` those copies. Cleared when the selection becomes something
/// other than `ids`, so an unrelated Cmd+D is a plain Duplicate again.
#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateMemory {
    pub from: Frame,
    pub to: Frame,
    pub ids: Vec<u64>,
    /// Whether this step came from a Duplicate, so Cmd+D repeats it as Power Duplicate. A step recorded
    /// from a bare transform gesture (a rotate on the canvas) leaves Cmd+D a plain Duplicate; only
    /// Transform Again (Cmd+Shift+D) repeats it (spec 52).
    pub from_duplicate: bool,
}

/// Step and repeat's settings: how many copies, the offset between them, and an optional turn and
/// shrink compounded each step. With `grid`, the copies fill rows of `columns` with a second offset
/// down the column axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepRepeat {
    pub copies: u32,
    pub offset_in: Pt,
    pub rotation_deg: f32,
    /// Per-step size multiplier, 1.0 for no change (the popover's Scale percent over 100).
    pub scale: f32,
    pub grid: Option<Grid>,
}

/// Step and repeat's Grid toggle: `columns` across, each row dropped by `offset_in`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub columns: u32,
    pub offset_in: Pt,
}

/// Rotate copies' settings: how many copies in the ring and the angle they span. The pivot they turn
/// about is resolved by the caller (the selection's pivot or its frame centre), so a menu default needs
/// no concrete point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotateCopies {
    pub copies: u32,
    pub total_deg: f32,
}

/// The copies [`StepRepeat`] makes, each as the originals carried to a new pose. The source is not
/// included: the caller keeps it and groups it with these. Copy `k` (from 1) sits `k` offsets on,
/// turned `k` steps and scaled `scale` to the `k`, about its own centre.
pub fn step_and_repeat(design: &Design, originals: &[Shape], s: StepRepeat) -> Vec<Vec<Shape>> {
    let Some(from) = Frame::of(design, originals) else { return Vec::new() };
    (1..=s.copies)
        .map(|k| {
            let (col, row) = s.grid.map_or((k, 0), |g| (k % g.columns.max(1), k / g.columns.max(1)));
            let (dx, dy) = s.grid.map_or((0.0, 0.0), |g| (row as f32 * g.offset_in.0, row as f32 * g.offset_in.1));
            let centre = (from.centre.0 + col as f32 * s.offset_in.0 + dx, from.centre.1 + col as f32 * s.offset_in.1 + dy);
            let factor = s.scale.powi(k as i32);
            let to = Frame {
                centre,
                width_in: from.width_in * factor,
                height_in: from.height_in * factor,
                rotation_deg: from.rotation_deg + k as f32 * s.rotation_deg,
                ..from
            };
            transform::remap_to(originals, &from, &to)
        })
        .collect()
}

/// The copies [`RotateCopies`] makes round the pivot. The source stays at angle 0 and is not included;
/// copy `k` (from 1) turns `k` steps of `total_deg / copies`, so twelve over 360 sit at 30 degree steps.
pub fn rotate_copies(design: &Design, originals: &[Shape], r: RotateCopies, pivot: Pt) -> Vec<Vec<Shape>> {
    let copies = r.copies.max(1);
    let step = r.total_deg / copies as f32;
    (1..copies).map(|k| transform::rotate_many_about(design, originals, k as f32 * step, pivot)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Operation, ShapeKind};
    use egui::Color32;

    fn square(id: u64, x: f32, y: f32) -> Shape {
        Shape { x_in: x, y_in: y, width_in: 1.0, height_in: 1.0, fill: Color32::BLACK, operation: Operation::BasicCut, ..Shape::new(id, ShapeKind::Square) }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn centre(shapes: &[Shape]) -> Pt {
        shapes[0].centre_in()
    }

    #[test]
    fn power_duplicate_lays_a_row_at_an_even_pitch() {
        // Duplicate, move the copy to a 1 in offset, then repeat: x = 0, 1, 2, 3.
        let d = Design::default();
        let source = square(1, 0.0, 0.0);
        let mut xs = vec![source.x_in];
        let mut from = Frame::of(&d, std::slice::from_ref(&source)).unwrap();
        // The first copy, moved one inch right of the source, then two more presses of Cmd+D.
        let mut now = Frame { centre: (1.5, 0.5), ..from };
        xs.push(now.centre.0 - 0.5);
        for _ in 0..2 {
            let next = now.stepped_from(&from);
            xs.push(next.centre.0 - 0.5);
            (from, now) = (now, next);
        }
        assert_eq!(xs.len(), 4);
        for (i, x) in xs.iter().enumerate() {
            assert!(close(*x, i as f32), "shape {i} at x = {x}");
        }
    }

    #[test]
    fn rotate_copies_spaces_a_ring_evenly() {
        let d = Design::default();
        let pivot = (5.0, 5.0);
        let petal = square(1, 5.0 - 0.5, 1.5); // centre three inches above the pivot
        let copies = rotate_copies(&d, std::slice::from_ref(&petal), RotateCopies { copies: 12, total_deg: 360.0 }, pivot);
        assert_eq!(copies.len(), 11, "eleven new copies plus the source makes twelve");
        let mut centres = vec![petal.centre_in()];
        centres.extend(copies.iter().map(|c| centre(c)));
        let radius = 3.0;
        for (i, c) in centres.iter().enumerate() {
            let angle = (i as f32 * 30.0).to_radians();
            let expect = (pivot.0 + radius * angle.sin(), pivot.1 - radius * angle.cos());
            assert!(close(c.0, expect.0) && close(c.1, expect.1), "copy {i} at {c:?}, expected {expect:?}");
        }
    }

    #[test]
    fn step_and_repeat_makes_a_row_grouped_with_the_source() {
        let d = Design::default();
        let source = square(1, 0.0, 0.0);
        let copies = step_and_repeat(&d, std::slice::from_ref(&source), StepRepeat { copies: 3, offset_in: (1.0, 0.0), rotation_deg: 0.0, scale: 1.0, grid: None });
        assert_eq!(copies.len(), 3);
        for (k, copy) in copies.iter().enumerate() {
            assert!(close(copy[0].x_in, (k + 1) as f32), "copy {k} at x = {}", copy[0].x_in);
        }
    }

    #[test]
    fn step_and_repeat_scales_and_turns_each_step() {
        let d = Design::default();
        let source = square(1, 0.0, 0.0);
        let copies = step_and_repeat(&d, std::slice::from_ref(&source), StepRepeat { copies: 3, offset_in: (2.0, 0.0), rotation_deg: 10.0, scale: 0.5, grid: None });
        assert!(close(copies[0][0].width_in, 0.5) && close(copies[1][0].width_in, 0.25));
        assert!(close(copies[0][0].rotation_deg, 10.0) && close(copies[1][0].rotation_deg, 20.0));
    }

    #[test]
    fn a_grid_step_fills_rows_then_drops_a_row() {
        let d = Design::default();
        let source = square(1, 0.0, 0.0);
        let copies = step_and_repeat(
            &d,
            std::slice::from_ref(&source),
            StepRepeat { copies: 4, offset_in: (1.0, 0.0), rotation_deg: 0.0, scale: 1.0, grid: Some(Grid { columns: 2, offset_in: (0.0, 1.0) }) },
        );
        // Cells 1, 2, 3 after the source at cell 0: (1,0), (0,1), (1,1).
        assert!(close(copies[0][0].x_in, 1.0) && close(copies[0][0].y_in, 0.0));
        assert!(close(copies[1][0].x_in, 0.0) && close(copies[1][0].y_in, 1.0));
        assert!(close(copies[2][0].x_in, 1.0) && close(copies[2][0].y_in, 1.0));
    }
}
