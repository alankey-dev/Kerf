//! The one gate untrusted designs pass: an opened project and an imported SVG. Values serde or usvg
//! accept but the geometry cannot use (non-finite numbers, sizes far past the canvas) are dropped or
//! clamped here, so nothing downstream has to guard against them.

use crate::design::{Contour, Design, Pt, Seg, Shape, ShapeKind};
use crate::transform::{MAX_SIZE_IN, MAX_SKEW_DEG};
use std::collections::HashSet;

pub(crate) const MIN_SIZE_IN: f32 = 0.001;

/// Ids at or above this are refused on load, so `Design::next_id` can always add one.
pub(crate) const ID_LIMIT: u64 = 1 << 63;

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
        self.repair_ids(&mut repairs);
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

impl Design {
    /// Keeps every id below [`ID_LIMIT`] and every shape id unique across the canvas and the masters. A
    /// shape or component that breaks either is dropped, a link id past the limit is cut, and a child
    /// left without its parent is freed.
    fn repair_ids(&mut self, repairs: &mut Repairs) {
        let mut seen = HashSet::new();
        self.components.retain(|c| {
            let keep = c.id < ID_LIMIT && seen.insert(c.id);
            if !keep {
                repairs.dropped.push(c.id);
            }
            keep
        });
        self.link_names.retain(|id, _| *id < ID_LIMIT);
        let mut seen = HashSet::new();
        let lists = std::iter::once(&mut self.shapes).chain(self.components.iter_mut().map(|c| &mut c.shapes));
        for list in lists {
            for s in list.iter_mut().filter(|s| s.link.is_some_and(|l| l.id >= ID_LIMIT)) {
                s.link = None;
                repairs.clamped.push(s.id);
            }
            let before = list.len();
            list.retain(|s| {
                let keep = s.id < ID_LIMIT && seen.insert(s.id);
                if !keep {
                    repairs.dropped.push(s.id);
                }
                keep
            });
            if list.len() != before {
                let ids: HashSet<u64> = list.iter().map(|s| s.id).collect();
                for s in list.iter_mut().filter(|s| s.parent.is_some_and(|p| !ids.contains(&p))) {
                    s.parent = None;
                }
            }
        }
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
    clamped |= reset(&mut s.skew_deg.0, -MAX_SKEW_DEG, MAX_SKEW_DEG);
    clamped |= reset(&mut s.skew_deg.1, -MAX_SKEW_DEG, MAX_SKEW_DEG);
    if let Some(pivot) = &mut s.pivot {
        if pivot.0.is_finite() && pivot.1.is_finite() {
            clamped |= reset(&mut pivot.0, -MAX_SIZE_IN, MAX_SIZE_IN);
            clamped |= reset(&mut pivot.1, -MAX_SIZE_IN, MAX_SIZE_IN);
        } else {
            s.pivot = None;
            clamped = true;
        }
    }
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
    if let Some(spec) = &mut s.monogram {
        let adjust = spec.adjust.clamped();
        clamped |= std::mem::replace(&mut spec.adjust, adjust) != adjust;
    }
    match &mut s.kind {
        ShapeKind::Text { size_in, letter_space, line_space, wrap_in, curve, path, .. } => {
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
            if path.as_ref().is_some_and(|p| !text_path_is_walkable(p)) {
                *path = None;
                clamped = true;
            }
        }
        ShapeKind::Parametric(params) => clamped |= params.clamp_fields(MAX_SIZE_IN),
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

/// A path the sampler can walk: at least one segment, finite points and offset, a usable view box.
fn text_path_is_walkable(p: &crate::text_path::TextPath) -> bool {
    p.offset_in.is_finite() && p.contours.iter().any(|c| !c.segs.is_empty()) && view_box_is_usable(&p.view_box) && contours_are_finite(&p.contours)
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

fn pt_is_finite(p: &Pt) -> bool {
    p.0.is_finite() && p.1.is_finite()
}

fn view_box_is_usable(v: &Pt) -> bool {
    pt_is_finite(v) && v.0 >= 0.0 && v.1 >= 0.0
}

fn contours_are_finite(cs: &[Contour]) -> bool {
    cs.iter().all(|c| {
        pt_is_finite(&c.start)
            && c.segs.iter().all(|s| match s {
                Seg::Line(p) => pt_is_finite(p),
                Seg::Cubic(a, b, p) => pt_is_finite(a) && pt_is_finite(b) && pt_is_finite(p),
            })
    })
}

fn kind_is_finite(kind: &ShapeKind) -> bool {
    match kind {
        ShapeKind::Imported { contours: cs, view_box: v } | ShapeKind::Path { contours: cs, view_box: v, .. } => view_box_is_usable(v) && contours_are_finite(cs),
        ShapeKind::Flattened { view_box: v, .. } => view_box_is_usable(v),
        ShapeKind::Bitmap { px, clip, .. } => px.0 > 0 && px.1 > 0 && contours_are_finite(clip),
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

    // The property test below mutates every numeric field of every shape kind into hostile values and
    // runs the result through `sanitise`. Its corpus is built with a full `Shape` literal and an
    // exhaustive `ShapeKind` match, so a new field or kind fails to compile here until it is covered.

    fn contour(points: &[Pt], closed: bool) -> Contour {
        Contour { start: points[0], segs: points[1..].iter().map(|&p| Seg::Line(p)).collect(), closed }
    }

    fn asset() -> crate::assets::AssetId {
        crate::assets::AssetId::try_from("ab".repeat(32)).unwrap()
    }

    /// Names a kind with no wildcard arm: adding a `ShapeKind` variant breaks this until `corpus` has it.
    fn kind_name(kind: &ShapeKind) -> &'static str {
        match kind {
            ShapeKind::Line => "Line",
            ShapeKind::TriangleDown => "TriangleDown",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Star => "Star",
            ShapeKind::StarThin => "StarThin",
            ShapeKind::Octagon => "Octagon",
            ShapeKind::Pentagon => "Pentagon",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::Heart => "Heart",
            ShapeKind::Plus => "Plus",
            ShapeKind::Equals => "Equals",
            ShapeKind::Circle => "Circle",
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Square => "Square",
            ShapeKind::Pill => "Pill",
            ShapeKind::RoundedSquare => "RoundedSquare",
            ShapeKind::Oval => "Oval",
            ShapeKind::Arch => "Arch",
            ShapeKind::HalfCircle => "HalfCircle",
            ShapeKind::Builtin(_) => "Builtin",
            ShapeKind::Parametric(_) => "Parametric",
            ShapeKind::Imported { .. } => "Imported",
            ShapeKind::Path { .. } => "Path",
            ShapeKind::Text { .. } => "Text",
            ShapeKind::Flattened { .. } => "Flattened",
            ShapeKind::Bitmap { .. } => "Bitmap",
            ShapeKind::Compound(_) => "Compound",
            ShapeKind::Instance { .. } => "Instance",
        }
    }

    /// One of every kind: each fixed shape, every parametric family, and every variant that carries data.
    fn kinds() -> Vec<ShapeKind> {
        use crate::parametric::{ArcClose, Params};
        let triangle = [(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)];
        let fixed = [
            ShapeKind::Line,
            ShapeKind::TriangleDown,
            ShapeKind::Triangle,
            ShapeKind::Diamond,
            ShapeKind::Star,
            ShapeKind::StarThin,
            ShapeKind::Octagon,
            ShapeKind::Pentagon,
            ShapeKind::Hexagon,
            ShapeKind::Heart,
            ShapeKind::Plus,
            ShapeKind::Equals,
            ShapeKind::Circle,
            ShapeKind::Rectangle,
            ShapeKind::Square,
            ShapeKind::Pill,
            ShapeKind::RoundedSquare,
            ShapeKind::Oval,
            ShapeKind::Arch,
            ShapeKind::HalfCircle,
        ];
        let params = [
            Params::Rectangle { corners: [0.1, 0.2, 0.3, 0.4], linked: false, scale_with_shape: true },
            Params::Ellipse,
            Params::Polygon { sides: 6, corner: 0.1, turn: 10.0 },
            Params::Star { points: 5, inner: 0.4, corner: 0.1, inner_corner: 0.05, turn: 10.0 },
            Params::Arc { start_deg: 10.0, sweep_deg: 200.0, inner: 0.3, close: ArcClose::Pie },
            Params::Ring { thickness: 0.3 },
            Params::Line,
            Params::Plus { bar: 0.3 },
        ];
        let text_path = crate::text_path::TextPath {
            contours: vec![contour(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0)], false)],
            view_box: (2.0, 1.0),
            offset_in: 0.2,
            side: Default::default(),
            align: Default::default(),
        };
        let text = |curve, path| ShapeKind::Text {
            content: "Hi there".into(),
            font: crate::text::FontRef::Inter,
            size_in: 1.0,
            style: Default::default(),
            letter_space: 10.0,
            line_space: 10.0,
            align: Default::default(),
            wrap_in: Some(3.0),
            v_align: Default::default(),
            curve,
            path,
        };
        let mut kinds: Vec<ShapeKind> = fixed.into_iter().chain(params.into_iter().map(ShapeKind::Parametric)).collect();
        kinds.extend([
            ShapeKind::Builtin(crate::shape_library::BuiltinId::all().next().unwrap()),
            ShapeKind::Imported { contours: vec![contour(&triangle, true)], view_box: (1.0, 1.0) },
            ShapeKind::Path { contours: vec![contour(&triangle, false)], view_box: (1.0, 1.0), node_kinds: Vec::new(), corners: Vec::new() },
            text(Some(crate::effects::Curve { diameter_in: 3.0, enabled: true }), None),
            text(None, Some(text_path)),
            ShapeKind::Flattened { parts: vec![Shape::new(40, ShapeKind::Square)], view_box: (1.0, 1.0) },
            ShapeKind::Bitmap { asset: asset(), px: (10, 10), clip: vec![contour(&triangle, true)] },
            ShapeKind::Compound(crate::design::CompoundOp::Effects),
            ShapeKind::Instance { component: 1, overrides: [(2, crate::components::PartOverride { pattern: Some(Some(pattern())), ..Default::default() })].into() },
        ]);
        kinds
    }

    fn pattern() -> crate::patterns::PatternFill {
        crate::patterns::PatternFill { asset: asset(), aspect: 1.0, mode: crate::patterns::PatternMode::Tile, scale: 1.0, rotation_deg: 10.0, offset_in: (0.5, 0.5) }
    }

    /// A shape of `kind` with every optional field set, written as a full literal so a new `Shape` field
    /// fails to compile here until the corpus fills it.
    fn full_shape(kind: ShapeKind) -> Shape {
        use crate::effects::{Corners, Direction, Effect, Effects, Offset, Sticker};
        let mut effects = Effects::default();
        effects.push(Effect::Warp(crate::warp::Warp { style: crate::warp::WarpStyle::Wave, bend: 0.5, vertical: false }));
        effects.push(Effect::Offset(Offset { distance_in: 0.1, direction: Direction::Outset, join: Corners::Round.join(), style: None }));
        effects.push(Effect::Sticker(Sticker { border_in: 0.1, corners: Corners::Round }));
        Shape {
            id: 10,
            kind,
            x_in: 1.0,
            y_in: 2.0,
            width_in: 3.0,
            height_in: 2.0,
            rotation_deg: 15.0,
            flip_x: true,
            flip_y: true,
            fill: egui::Color32::RED,
            operation: crate::design::Operation::PrintThenCut,
            hidden: true,
            contours: crate::effects::ContourMask { hidden: vec![false, true], enabled: true },
            link: Some(crate::links::Link { id: 90, kind: crate::links::LinkKind::Group }),
            pen: Default::default(),
            effects,
            container: Some(crate::containers::ContainerRole {
                padding_in: 0.2,
                layout: Some(crate::containers::Flow { axis: crate::containers::Axis::Row, gap_in: 0.1, cross: Default::default(), sizing: Default::default() }),
            }),
            parent: Some(999),
            grain_lock: true,
            skew_deg: (10.0, -10.0),
            pivot: Some((0.5, 0.5)),
            lock_aspect: true,
            lock_position: true,
            locked: true,
            pattern: Some(pattern()),
            name: Some("Layer".into()),
            monogram: Some(Box::default()),
        }
    }

    fn design_of(shape: Shape) -> Design {
        let master = Shape::new(2, ShapeKind::Square);
        let component = crate::components::Component { id: 1, name: "Master".into(), shapes: vec![master], size_in: (1.0, 1.0) };
        Design { shapes: vec![shape], components: vec![component], ..Design::default() }
    }

    /// Every number leaf in `value`, by JSON pointer, with whether it is a float.
    fn number_leaves(value: &serde_json::Value, at: &mut String, out: &mut Vec<(String, bool)>) {
        match value {
            serde_json::Value::Number(n) => out.push((at.clone(), n.is_f64())),
            serde_json::Value::Array(items) => {
                for (i, v) in items.iter().enumerate() {
                    let len = at.len();
                    at.push_str(&format!("/{i}"));
                    number_leaves(v, at, out);
                    at.truncate(len);
                }
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let len = at.len();
                    at.push_str(&format!("/{k}"));
                    number_leaves(v, at, out);
                    at.truncate(len);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_shape_kind_and_field_survives_hostile_numbers() {
        let floats = [1e39, -1e39, 1e30, -1e30, 1e12, -1.0, 0.0, 1e-30];
        let ints = [0u64, 1, u32::MAX as u64, u64::MAX, 1 << 63];
        let mut mutations = 0;
        let mut names = std::collections::BTreeSet::new();
        for kind in kinds() {
            let name = kind_name(&kind);
            names.insert(name);
            let design = design_of(full_shape(kind));
            let mut clean = design.clone();
            clean.sanitise();
            let plain = format!("{:?}", clean.shapes);
            assert!(!plain.contains("NaN") && !plain.contains("inf"), "the corpus itself is sound: {plain}");

            let baseline = serde_json::to_value(&design).unwrap();
            let mut leaves = Vec::new();
            number_leaves(&baseline, &mut String::new(), &mut leaves);
            assert!(!leaves.is_empty());
            for (at, is_float) in leaves {
                let hostile: Vec<serde_json::Value> =
                    if is_float { floats.iter().map(|&f| serde_json::json!(f)).collect() } else { ints.iter().map(|&i| serde_json::json!(i)).collect() };
                for value in hostile {
                    let mut json = baseline.clone();
                    *json.pointer_mut(&at).unwrap() = value.clone();
                    // serde may refuse the value outright, which is as safe as repairing it.
                    let Ok(mut d) = serde_json::from_value::<Design>(json) else { continue };
                    mutations += 1;
                    d.sanitise();
                    let ctx = format!("{name} at {at} = {value}");
                    let shown = format!("{:?}", d.shapes);
                    assert!(!shown.contains("NaN") && !shown.contains("inf"), "{ctx}: a non-finite number survived");
                    // Effects are checked for finiteness above but evaluated apart: a Warp then an Offset on a
                    // sliver box does not finish, whatever loaded it, and a canvas-sized Sticker is slow.
                    let started = std::time::Instant::now();
                    for s in &d.shapes {
                        let bare = Shape { effects: Default::default(), ..s.clone() };
                        let rings = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bare.world_rings(crate::geometry::FLATTEN_TOL_IN)));
                        let Ok(rings) = rings else { panic!("{ctx}: evaluating the outline panicked") };
                        let points: usize = rings.iter().map(Vec::len).sum();
                        assert!(points < 1_000_000, "{ctx}: {points} points");
                    }
                    assert!(started.elapsed().as_secs() < 5, "{ctx}: took {:?}", started.elapsed());
                    assert!(d.next_id() > 0);
                    assert!(d.sanitise().is_empty(), "{ctx}: sanitise is idempotent");
                }
            }
        }
        assert_eq!(names.len(), 29, "{names:?}");
        assert!(mutations > 1000, "{mutations}");
    }
}
