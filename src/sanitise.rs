//! The one gate untrusted designs pass: an opened project and an imported SVG. Values serde or usvg
//! accept but the geometry cannot use (non-finite numbers, sizes far past the canvas) are dropped or
//! clamped here, so nothing downstream has to guard against them.

use crate::design::{Contour, Design, Pt, Seg, Shape, ShapeKind};
use crate::transform::MAX_SIZE_IN;

pub(crate) const MIN_SIZE_IN: f32 = 0.001;

/// What [`Design::sanitise`] changed. Empty for a design that was already sound.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Repairs {
    /// Shapes removed because a number they need is not finite.
    pub dropped: Vec<u64>,
    /// Shapes kept but resized, moved or reset to fit.
    pub clamped: Vec<u64>,
}

impl Repairs {
    pub fn is_empty(&self) -> bool {
        self.dropped.is_empty() && self.clamped.is_empty()
    }
}

impl std::fmt::Display for Repairs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = |n: usize| if n == 1 { "1 shape".to_owned() } else { format!("{n} shapes") };
        let mut parts = Vec::new();
        if !self.dropped.is_empty() {
            parts.push(format!("dropped {} with unusable numbers", count(self.dropped.len())));
        }
        if !self.clamped.is_empty() {
            parts.push(format!("resized or moved {} to fit the canvas", count(self.clamped.len())));
        }
        write!(f, "{}", parts.join("; "))
    }
}

impl Design {
    /// Drops shapes that cannot be drawn and clamps the rest into the canvas. Idempotent.
    pub fn sanitise(&mut self) -> Repairs {
        let mut repairs = Repairs::default();
        sanitise_shapes(&mut self.shapes, &mut repairs);
        // A Compound evaluates its children, so a parent cycle would recurse without end.
        self.break_parent_cycles();
        self.sanitise_components(&mut repairs);
        if self.nesting.as_ref().is_some_and(|n| !n.is_sound()) {
            self.nesting = None;
        }
        repairs
    }
}

pub(crate) fn sanitise_shapes(shapes: &mut Vec<Shape>, repairs: &mut Repairs) {
    shapes.retain_mut(|shape| match sanitise_shape(shape, repairs) {
        Some(clamped) => {
            if clamped {
                repairs.clamped.push(shape.id);
            }
            true
        }
        None => {
            repairs.dropped.push(shape.id);
            false
        }
    });
}

/// `None` when the shape must go; otherwise whether anything was clamped.
fn sanitise_shape(s: &mut Shape, repairs: &mut Repairs) -> Option<bool> {
    let numbers = [s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg];
    if !numbers.iter().all(|v| v.is_finite()) || !kind_is_finite(&s.kind) {
        return None;
    }
    let before = (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg);
    let largest = s.width_in.max(s.height_in);
    if largest > MAX_SIZE_IN {
        let scale = MAX_SIZE_IN / largest;
        (s.width_in, s.height_in) = (s.width_in * scale, s.height_in * scale);
    }
    s.width_in = s.width_in.max(MIN_SIZE_IN);
    s.height_in = s.height_in.max(MIN_SIZE_IN);
    s.x_in = s.x_in.clamp(-MAX_SIZE_IN, MAX_SIZE_IN);
    s.y_in = s.y_in.clamp(-MAX_SIZE_IN, MAX_SIZE_IN);
    if !(-360.0..=360.0).contains(&s.rotation_deg) {
        s.rotation_deg = s.rotation_deg.rem_euclid(360.0);
    }
    let mut clamped = before != (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg);
    if s.effects.warp().is_some_and(|w| !w.bend.is_finite()) {
        s.effects.set_warp(None);
        clamped = true;
    }
    clamped |= s.effects.clamp_lengths(MAX_SIZE_IN);
    if let Some(role) = &mut s.container {
        clamped |= reset(&mut role.padding_in, 0.0, MAX_SIZE_IN);
        if let Some(flow) = &mut role.layout {
            clamped |= reset(&mut flow.gap_in, 0.0, MAX_SIZE_IN);
        }
    }
    if let Some(p) = &mut s.pattern {
        clamped |= clamp_pattern(p);
    }
    match &mut s.kind {
        ShapeKind::Text { size_in, letter_space, line_space, wrap_in, curve, .. } => {
            clamped |= reset(size_in, 0.01, MAX_SIZE_IN);
            clamped |= reset(letter_space, -200.0, 1000.0);
            clamped |= reset(line_space, -500.0, 2000.0);
            if let Some(c) = curve {
                clamped |= reset(&mut c.diameter_in, -MAX_SIZE_IN, MAX_SIZE_IN);
            }
            if wrap_in.is_some_and(|w| !w.is_finite() || w <= 0.0) {
                *wrap_in = None;
                clamped = true;
            }
        }
        ShapeKind::Flattened { parts, .. } => {
            let dropped = repairs.dropped.len();
            sanitise_shapes(parts, repairs);
            clamped |= repairs.dropped.len() > dropped;
        }
        ShapeKind::Instance { overrides, .. } => {
            for p in overrides.values_mut().filter_map(|o| o.pattern.as_mut().and_then(Option::as_mut)) {
                clamped |= clamp_pattern(p);
            }
        }
        _ => {}
    }
    Some(clamped)
}

fn clamp_pattern(p: &mut crate::patterns::PatternFill) -> bool {
    let mut clamped = reset(&mut p.aspect, 0.01, 100.0);
    clamped |= reset(&mut p.scale, crate::patterns::MIN_SCALE, MAX_SIZE_IN);
    clamped |= reset(&mut p.rotation_deg, -360.0, 360.0);
    clamped |= reset(&mut p.offset_in.0, -MAX_SIZE_IN, MAX_SIZE_IN);
    clamped | reset(&mut p.offset_in.1, -MAX_SIZE_IN, MAX_SIZE_IN)
}

/// Clamps `v` into `lo..=hi`, treating a non-finite value as `lo`. Returns whether it changed.
fn reset(v: &mut f32, lo: f32, hi: f32) -> bool {
    let fixed = if v.is_finite() { v.clamp(lo, hi) } else { lo.max(0.0).min(hi) };
    std::mem::replace(v, fixed) != fixed
}

fn kind_is_finite(kind: &ShapeKind) -> bool {
    let pt = |p: &Pt| p.0.is_finite() && p.1.is_finite();
    let view_box = |v: &Pt| pt(v) && v.0 >= 0.0 && v.1 >= 0.0;
    let contours = |cs: &[Contour]| {
        cs.iter().all(|c| {
            pt(&c.start)
                && c.segs.iter().all(|s| match s {
                    Seg::Line(p) => pt(p),
                    Seg::Cubic(a, b, p) => pt(a) && pt(b) && pt(p),
                })
        })
    };
    match kind {
        ShapeKind::Imported { contours: cs, view_box: v } | ShapeKind::Path { contours: cs, view_box: v, .. } => view_box(v) && contours(cs),
        ShapeKind::Flattened { view_box: v, .. } => view_box(v),
        ShapeKind::Bitmap { px, clip, .. } => px.0 > 0 && px.1 > 0 && contours(clip),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Design {
        let mut d = Design::default();
        d.add_shape(ShapeKind::Square);
        d
    }

    #[test]
    fn a_huge_svg_imports_at_canvas_size() {
        let svg = include_bytes!("../tests/fixtures/malformed/huge-path.svg");
        let mut d = Design::default();
        d.add_imported(crate::shape_library::import_svg(svg).expect("usvg accepts it"));
        let repairs = d.sanitise();
        assert_eq!(repairs.clamped.len(), 1, "{repairs:?}");
        let s = &d.shapes[0];
        assert!(s.width_in <= MAX_SIZE_IN && s.height_in <= MAX_SIZE_IN, "{} x {}", s.width_in, s.height_in);
        let points: usize = s.world_rings(crate::geometry::FLATTEN_TOL_IN).iter().map(Vec::len).sum();
        assert!(points < 100_000, "{points} points");
    }

    #[test]
    fn a_sound_design_is_untouched() {
        let mut d = square();
        let before = d.clone();
        assert!(d.sanitise().is_empty());
        assert_eq!(d, before);
    }

    #[test]
    fn non_finite_shapes_are_dropped_and_reported() {
        let mut d = square();
        d.add_shape(ShapeKind::Circle);
        let (keep, drop) = (d.shapes[0].id, d.shapes[1].id);
        d.shapes[1].x_in = f32::NAN;
        let repairs = d.sanitise();
        assert_eq!(repairs.dropped, [drop]);
        assert_eq!(d.shapes.iter().map(|s| s.id).collect::<Vec<_>>(), [keep]);
        assert_eq!(repairs.to_string(), "dropped 1 shape with unusable numbers");
    }

    #[test]
    fn absurd_sizes_scale_down_to_the_canvas_keeping_their_aspect() {
        let mut d = square();
        (d.shapes[0].width_in, d.shapes[0].height_in) = (1e10, 5e9);
        d.shapes[0].x_in = -1e20;
        let repairs = d.sanitise();
        assert_eq!(repairs.clamped, [d.shapes[0].id]);
        assert_eq!((d.shapes[0].width_in, d.shapes[0].height_in), (MAX_SIZE_IN, MAX_SIZE_IN / 2.0));
        assert_eq!(d.shapes[0].x_in, -MAX_SIZE_IN);
        assert!(d.sanitise().is_empty(), "idempotent");
    }

    #[test]
    fn non_finite_geometry_and_text_numbers_are_handled() {
        let mut d = Design::default();
        let bad = Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((f32::INFINITY, 0.0), (1.0, 1.0), (2.0, 0.0))], closed: true };
        d.add_shape(ShapeKind::Imported { contours: vec![bad], view_box: (2.0, 1.0) });
        let text = d.add_shape(ShapeKind::Text {
            content: "Hi".into(),
            font: crate::text::FontRef::Inter,
            size_in: 1.0,
            style: Default::default(),
            letter_space: 0.0,
            line_space: 0.0,
            align: Default::default(),
            wrap_in: None,
            v_align: Default::default(),
            curve: None,
            path: None,
        });
        if let ShapeKind::Text { size_in, wrap_in, .. } = &mut d.shapes[1].kind {
            (*size_in, *wrap_in) = (f32::INFINITY, Some(f32::NAN));
        }
        let repairs = d.sanitise();
        assert_eq!(repairs.dropped.len(), 1, "the imported path with an infinite point goes");
        assert_eq!(repairs.clamped, [text]);
        let ShapeKind::Text { size_in, wrap_in, .. } = d.shapes[0].kind else { panic!("text kept") };
        assert!(size_in.is_finite() && wrap_in.is_none());
    }
}
