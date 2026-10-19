//! Warp: bends a shape's outline inside an envelope. Pure maths over a shape's local box.
//!
//! A warped shape's `width_in` x `height_in` is the warped frame, so the selection frame hugs the bent
//! outline. Every style displaces by `bend * height` (or `bend * width` when vertical), so in unit-box
//! coordinates the warp does not depend on the box size. `apply` warps the base outline in its box and
//! stretches the warped bounds back onto that same box; `Shape::set_warp` resizes the box about its
//! centre by the ratio between the old and new warped extents, measured on the base outline at 1 x 1.

use serde::{Deserialize, Serialize};

use crate::design::{Contour, Pt, Seg, Shape, ShapeKind};
use crate::geometry::{FLATTEN_TOL_IN, flatten};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum WarpStyle {
    Arch,
    Bulge,
    Wave,
    Flag,
    Rise,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Warp {
    pub style: WarpStyle,
    /// -1.0 to 1.0, shown as -100 to 100.
    pub bend: f32,
    /// false bends along the width, true along the height.
    pub vertical: bool,
}

/// Edges are cut into steps of at most this fraction of the box on either axis, so a straight edge
/// bends smoothly instead of keeping only its two ends.
const STEPS: f32 = 48.0;

impl WarpStyle {
    pub const ALL: [WarpStyle; 5] = [Self::Arch, Self::Bulge, Self::Wave, Self::Flag, Self::Rise];

    pub fn label(self) -> &'static str {
        match self {
            Self::Arch => "Arch",
            Self::Bulge => "Bulge",
            Self::Wave => "Wave",
            Self::Flag => "Flag",
            Self::Rise => "Rise",
        }
    }

    /// The displacement across the bend direction per unit of `bend * height`, at `u` along it and `v`
    /// across it. Negative is up.
    fn shift(self, u: f32, v: f32) -> f32 {
        let arc = 1.0 - (2.0 * u - 1.0).powi(2);
        let wave = -0.5 * (std::f32::consts::TAU * u).sin();
        match self {
            Self::Arch => -arc,
            Self::Bulge => arc * (2.0 * v - 1.0),
            Self::Wave => wave,
            Self::Flag => wave * u,
            Self::Rise => -u,
        }
    }
}

impl Warp {
    /// Where the local point `p` of a `size` box lands, before any refit. Canvas y runs down, so "up"
    /// is negative y.
    pub fn point(self, (x, y): Pt, (w, h): Pt) -> Pt {
        let unit = |a: f32, len: f32| if len > 0.0 { a / len } else { 0.0 };
        let (u, v) = (unit(x, w), unit(y, h));
        if self.vertical { (x + self.bend * w * self.style.shift(v, u), y) } else { (x, y + self.bend * h * self.style.shift(u, v)) }
    }
}

/// `contours` (in a `size` box) densified and flattened at `FLATTEN_TOL_IN`, every point warped, then
/// the warped bounds stretched back onto `0..size`. One line contour out per contour in, so
/// hidden-contour indices still line up.
pub fn apply(warp: Warp, contours: &[Contour], size: Pt) -> Vec<Contour> {
    Bend::fit(warp, contours, (0.0, 0.0), size).contours(contours)
}

/// A warp fitted inside a frame at `origin` of `size`: where any point of the frame lands once the warped
/// bounds of the fitted contours are stretched back onto the frame. A Warp after an Offset or Sticker
/// bends every piece, and their print artwork, through one `Bend`.
pub struct Bend {
    warp: Warp,
    origin: Pt,
    size: Pt,
    min: Pt,
    scale: Pt,
}

impl Bend {
    pub fn fit(warp: Warp, contours: &[Contour], origin: Pt, size: Pt) -> Bend {
        let pts: Vec<Pt> = densified(contours, size).concat().into_iter().map(|(x, y)| warp.point((x - origin.0, y - origin.1), size)).collect();
        let (min, max) = bounds(&pts);
        let fit = |len: f32, extent: f32| if extent > 0.0 { len / extent } else { 1.0 };
        Bend { warp, origin, size, min, scale: (fit(size.0, max.0 - min.0), fit(size.1, max.1 - min.1)) }
    }

    pub fn point(&self, (x, y): Pt) -> Pt {
        let (wx, wy) = self.warp.point((x - self.origin.0, y - self.origin.1), self.size);
        ((wx - self.min.0) * self.scale.0 + self.origin.0, (wy - self.min.1) * self.scale.1 + self.origin.1)
    }

    /// `contours` densified and bent, one line contour out per contour in.
    pub fn contours(&self, contours: &[Contour]) -> Vec<Contour> {
        densified(contours, self.size)
            .into_iter()
            .zip(contours)
            .map(|(pts, c)| {
                let mut pts = pts.into_iter().map(|p| self.point(p));
                let start = pts.next().unwrap_or_default();
                Contour { start, segs: pts.map(Seg::Line).collect(), closed: c.closed }
            })
            .collect()
    }
}

/// Each contour as a polyline whose steps are at most `1 / STEPS` of the box per axis.
fn densified(contours: &[Contour], (w, h): Pt) -> Vec<Vec<Pt>> {
    let steps = |d: f32, len: f32| if len > 0.0 { d.abs() * STEPS / len } else { 0.0 };
    contours
        .iter()
        .map(|c| {
            let pts = flatten(c, FLATTEN_TOL_IN);
            let mut out = vec![pts[0]];
            for (&a, &b) in pts.iter().zip(pts.iter().skip(1).chain(c.closed.then_some(&pts[0]))) {
                let n = steps(b.0 - a.0, w).max(steps(b.1 - a.1, h)).ceil().max(1.0) as usize;
                let at = |i: usize| {
                    let t = i as f32 / n as f32;
                    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
                };
                out.extend((1..n).map(at).chain([b]));
            }
            if c.closed {
                out.pop();
            }
            out
        })
        .collect()
}

fn bounds(pts: &[Pt]) -> (Pt, Pt) {
    pts.iter().fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(min, max), &(x, y)| ((min.0.min(x), min.1.min(y)), (max.0.max(x), max.1.max(y))))
}

/// The warped outline's size in the unit box, `(1, 1)` unwarped. An axis with no extent counts as 1
/// so the ratio between two extents stays finite.
fn extent(warp: Option<Warp>, unit_outline: &[Contour]) -> Pt {
    let Some(warp) = warp else { return (1.0, 1.0) };
    let pts: Vec<Pt> = densified(unit_outline, (1.0, 1.0)).concat().into_iter().map(|p| warp.point(p, (1.0, 1.0))).collect();
    let (min, max) = bounds(&pts);
    let positive = |e: f32| if e > 0.0 { e } else { 1.0 };
    (positive(max.0 - min.0), positive(max.1 - min.1))
}

impl ShapeKind {
    /// A flattened shape or an image paints its artwork unwarped, so a warp would cut a different outline from the one
    /// drawn. A Compound's children keep their own boxes, which a warp of the group could not follow.
    pub fn can_warp(&self) -> bool {
        !matches!(self, ShapeKind::Flattened { .. } | ShapeKind::Bitmap { .. } | ShapeKind::Compound(_) | ShapeKind::Instance { .. })
    }
}

impl Shape {
    pub fn can_warp(&self) -> bool {
        self.kind.can_warp()
    }

    /// Why a Warp is refused for this shape.
    pub fn warp_blocked(&self) -> crate::edit::Blocked {
        match self.kind {
            ShapeKind::Bitmap { .. } => crate::edit::Blocked::ImageCannotWarp,
            ShapeKind::Compound(_) | ShapeKind::Instance { .. } => crate::edit::Blocked::GroupCannotWarp,
            _ => crate::edit::Blocked::CannotWarp,
        }
    }

    /// The toolbar Warp: sets or clears the first Warp entry. The box refits so the frame hugs the new
    /// outline, and removing a warp restores the unwarped size exactly. A shape that cannot warp keeps no
    /// warp.
    pub fn set_warp(&mut self, warp: Option<Warp>) {
        if warp.is_some() && !self.can_warp() {
            return;
        }
        self.change_effects(|fx| fx.set_warp(warp));
    }

    /// Resizes the box about its centre by the ratio between the extents of `before` and `after` on the
    /// kind's outline at 1 x 1, so the frame keeps hugging a body bent by a different warp.
    pub(crate) fn refit_warp(&mut self, before: Option<Warp>, after: Option<Warp>) {
        if before == after {
            return;
        }
        let outline = self.kind.outline(1.0, 1.0);
        let (old, new) = (extent(before, &outline), extent(after, &outline));
        let (cx, cy) = self.centre_in();
        self.width_in *= new.0 / old.0;
        self.height_in *= new.1 / old.1;
        self.x_in = cx - self.width_in / 2.0;
        self.y_in = cy - self.height_in / 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Pt = (3.0, 2.0);

    fn square(size: Pt) -> Vec<Contour> {
        ShapeKind::Square.outline(size.0, size.1)
    }

    fn points(contours: &[Contour]) -> Vec<Pt> {
        contours.iter().flat_map(Contour::flatten).collect()
    }

    fn every_warp(bend: f32) -> impl Iterator<Item = Warp> {
        WarpStyle::ALL.into_iter().flat_map(move |style| [false, true].map(|vertical| Warp { style, bend, vertical }))
    }

    #[test]
    fn a_flattened_shape_refuses_a_warp_but_can_lose_one() {
        let mut d = crate::design::Design::default();
        d.add_shape(ShapeKind::Square);
        d.add_shape(ShapeKind::Circle);
        let id = d.flatten(&[1, 2]).expect("flattens");
        let flat = d.shape_mut(id).unwrap();
        assert!(!flat.can_warp());
        flat.set_warp(Some(Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false }));
        assert_eq!(flat.effects.warp(), None);
    }

    #[test]
    fn bend_zero_is_the_identity() {
        for warp in every_warp(0.0) {
            for p in [(0.0, 0.0), (1.2, 0.7), (3.0, 2.0)] {
                assert_eq!(warp.point(p, SIZE), p, "{warp:?}");
            }
            for p in points(&apply(warp, &square(SIZE), SIZE)) {
                let on_edge = p.0.abs() < 1e-5 || (p.0 - SIZE.0).abs() < 1e-5 || p.1.abs() < 1e-5 || (p.1 - SIZE.1).abs() < 1e-5;
                assert!(on_edge, "{warp:?} moved {p:?} off the square");
            }
        }
    }

    #[test]
    fn arch_moves_the_top_centre_by_bend_times_height() {
        let warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };
        let (x, y) = warp.point((1.5, 0.0), SIZE);
        assert!((x - 1.5).abs() < 1e-6 && (y + 0.5 * SIZE.1).abs() < 1e-6, "top centre went to {:?}", (x, y));
        assert_eq!(warp.point((0.0, 0.0), SIZE), (0.0, 0.0), "the corners stay put");
    }

    #[test]
    fn rise_keeps_straight_lines_straight() {
        for vertical in [false, true] {
            let warp = Warp { style: WarpStyle::Rise, bend: 0.8, vertical };
            let edges = densified(&square(SIZE), SIZE);
            assert!(edges[0].len() > 4, "the edges are densified");
            let corners: Vec<Pt> = points(&square(SIZE)).into_iter().map(|p| warp.point(p, SIZE)).collect();
            let on = |p: Pt, a: Pt, b: Pt| ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)).abs() < 1e-4;
            let on_an_edge = |p: Pt| (0..4).any(|i| on(p, corners[i], corners[(i + 1) % 4]));
            assert!(edges[0].iter().all(|&p| on_an_edge(warp.point(p, SIZE))), "vertical {vertical}: a warped point left its edge");
        }
    }

    #[test]
    fn the_output_fills_the_box() {
        for warp in every_warp(0.7).chain(every_warp(-0.4)) {
            let out = apply(warp, &ShapeKind::Circle.outline(SIZE.0, SIZE.1), SIZE);
            let (min, max) = bounds(&points(&out));
            let fits = |a: f32, b: f32| (a - b).abs() < 1e-4;
            assert!(fits(min.0, 0.0) && fits(min.1, 0.0) && fits(max.0, SIZE.0) && fits(max.1, SIZE.1), "{warp:?}: {min:?} to {max:?}");
            assert!(out.iter().all(|c| c.segs.iter().all(|s| matches!(s, Seg::Line(_)))), "curves come out as lines");
        }
    }

    #[test]
    fn one_contour_out_per_contour_in_even_for_a_flat_box() {
        let warp = Warp { style: WarpStyle::Wave, bend: 1.0, vertical: false };
        let two = [square(SIZE), square((1.0, 1.0))].concat();
        assert_eq!(apply(warp, &two, SIZE).len(), 2);
        let flat = apply(warp, &square((0.0, 0.0)), (0.0, 0.0));
        assert_eq!(flat.len(), 1);
        assert!(points(&flat).iter().all(|p| p.0.is_finite() && p.1.is_finite()), "no NaN from a zero box");
    }

    #[test]
    fn removing_a_warp_restores_size_and_centre() {
        let mut d = crate::design::Design::default();
        let id = d.add_shape(ShapeKind::Star);
        let s = d.shape_mut(id).unwrap();
        let (size, centre) = ((s.width_in, s.height_in), s.centre_in());
        s.set_warp(Some(Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false }));
        assert!(s.height_in > size.1 + 0.1, "the arch grows the frame");
        s.set_warp(Some(Warp { style: WarpStyle::Bulge, bend: -0.3, vertical: true }));
        s.set_warp(None);
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(s.width_in, size.0) && near(s.height_in, size.1), "size {:?}", (s.width_in, s.height_in));
        assert!(near(s.centre_in().0, centre.0) && near(s.centre_in().1, centre.1), "centre {:?}", s.centre_in());
        assert_eq!(s.effects.warp(), None);
    }

    #[test]
    fn a_warped_shape_hugs_its_frame() {
        let mut d = crate::design::Design::default();
        let id = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(id).unwrap();
        s.set_warp(Some(Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false }));
        let (min, max) = bounds(&points(&s.contours()));
        assert!(min.1.abs() < 1e-4 && (max.1 - s.height_in).abs() < 1e-4, "{min:?} to {max:?}, frame height {}", s.height_in);
    }

    #[test]
    fn the_selection_frame_hugs_a_warped_shape() {
        let mut d = crate::design::Design::default();
        let id = d.add_shape(ShapeKind::Star);
        d.shape_mut(id).unwrap().set_warp(Some(Warp { style: WarpStyle::Wave, bend: 0.6, vertical: true }));
        let s = &d.shapes[0];
        let frame = crate::transform::Frame::of(&d, [s]).unwrap();
        let (x0, y0, x1, y1) = s.world_bounds();
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(frame.width_in, x1 - x0) && near(frame.height_in, y1 - y0), "frame {} x {}, outline {:?}", frame.width_in, frame.height_in, (x0, y0, x1, y1));
        assert!(near(frame.centre.0, (x0 + x1) / 2.0) && near(frame.centre.1, (y0 + y1) / 2.0), "centred on the outline");
    }

    #[test]
    fn a_project_saved_before_warps_loads_unwarped() {
        let json = r#"{"id":"pre-warp","name":"Before warp","design":{"shapes":[{"id":1,"kind":"Square","x_in":0.5,"y_in":0.5,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]}]},"created":1,"modified":1}"#;
        let project: crate::project::Project = serde_json::from_str(json).expect("an old project still loads");
        assert_eq!(project.design.shapes[0].effects.warp(), None);
    }
}
