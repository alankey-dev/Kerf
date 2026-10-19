//! The pivot crosshair drag (spec 52): where a dragged pivot lands. It snaps to the selection frame's
//! own corners, edge midpoints and centre, and to the design's spec-47 snap points, then reads back as
//! the shape's local `pivot`.

use crate::design::Pt;
use crate::transform::{Frame, Handle};

/// The frame's own snap points: its four corners, four edge midpoints and centre.
fn frame_points(f: &Frame) -> Vec<Pt> {
    let mut pts: Vec<Pt> = Handle::RESIZE.iter().map(|&h| f.handle_pos(h)).collect();
    pts.push(f.centre);
    pts
}

/// Where a pivot dragged to `pointer` lands: the nearest of the frame's own points and `extra` (the
/// design's snap targets) within `threshold` inches, else `pointer` itself.
pub fn snapped(frame: &Frame, extra: &[Pt], threshold: f32, pointer: Pt) -> Pt {
    let dist2 = |a: Pt, b: Pt| (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2);
    frame_points(frame)
        .into_iter()
        .chain(extra.iter().copied())
        .map(|t| (t, dist2(t, pointer)))
        .filter(|&(_, d)| d <= threshold * threshold)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(pointer, |(t, _)| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame { centre: (2.0, 2.0), width_in: 2.0, height_in: 2.0, rotation_deg: 0.0, skew_deg: (0.0, 0.0), flip: (false, false) }
    }

    #[test]
    fn a_drag_near_a_corner_snaps_to_it() {
        let f = frame();
        // The top-left corner sits at (1, 1); a drag that lands just off it snaps there.
        assert_eq!(snapped(&f, &[], 0.3, (1.1, 0.9)), (1.0, 1.0));
    }

    #[test]
    fn a_drag_in_open_space_keeps_the_pointer() {
        // (2.5, 2.5) is more than the threshold from every corner, midpoint and the centre.
        assert_eq!(snapped(&frame(), &[], 0.3, (2.5, 2.5)), (2.5, 2.5));
    }

    #[test]
    fn an_extra_target_wins_when_it_is_nearer() {
        let f = frame();
        assert_eq!(snapped(&f, &[(5.0, 5.0)], 0.3, (4.95, 5.05)), (5.0, 5.0));
    }
}
