//! The per-shape geometry accessors the test suite read before spec 21, answered by the evaluator. The
//! suite written against them stays unchanged and pins the new pipeline. Production code reads
//! `Design::geometry`.

use super::{body_contours, evaluate};
use crate::design::{Contour, Pt, Shape};
use crate::snap::Aabb;

impl Shape {
    pub fn all_contours(&self) -> Vec<Contour> {
        body_contours(self)
    }

    pub fn contours(&self) -> Vec<Contour> {
        evaluate(self).contours().cloned().collect()
    }

    pub fn contour_count(&self) -> usize {
        body_contours(self).len()
    }

    pub fn world_rings(&self, tol: f32) -> Vec<Vec<Pt>> {
        evaluate(self).rings(tol)
    }

    pub fn world_polylines(&self) -> Vec<(Vec<Pt>, bool)> {
        evaluate(self).lines().cloned().collect()
    }

    pub fn world_contours(&self) -> Vec<Contour> {
        let geometry = evaluate(self);
        geometry.pieces().iter().flat_map(|p| geometry.world_contours(p)).collect()
    }

    pub fn contains_in(&self, px: f32, py: f32) -> bool {
        evaluate(self).contains((px, py))
    }

    pub fn world_bounds(&self) -> (f32, f32, f32, f32) {
        let b = self.aabb();
        (b.min_x, b.min_y, b.max_x, b.max_y)
    }

    pub fn aabb(&self) -> Aabb {
        evaluate(self).bounds()
    }
}

impl crate::design::Design {
    /// A Sticker added live around `ids` and expanded: the Flattened shape Sticker made before effects
    /// were live. Returns its id.
    pub fn expanded_sticker(&mut self, ids: &[u64], border_in: f32, corners: super::Corners) -> Option<u64> {
        let effect = super::Effect::Sticker(super::Sticker { border_in, corners });
        let live = self.apply(&crate::edit::Edit::AddEffect { effect, reach: super::Reach::Together }, ids).ok()?;
        self.apply(&crate::edit::Edit::Expand, &live).ok()?.first().copied()
    }
}
