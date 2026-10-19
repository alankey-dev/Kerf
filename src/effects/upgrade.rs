//! Reads a saved shape of any format. Format 2 and earlier stored a warp in its own `warp` key; it
//! becomes the first stack entry. Format 3 and earlier stored hidden contours as `contour_hidden`; they
//! become the `contours` mask. Upgrading at the `Shape` level reaches `Flattened` parts too.
//!
//! An Offset saved before spec 34 stored a signed distance and Round or Sharp corners; [`OffsetRepr`]
//! reads it whatever the format version, so an Offset inside any shape or part upgrades.

use egui::Color32;
use serde::Deserialize;

use super::Effects;
use super::model::{Corners, Direction, Offset, Style};
use crate::design::{Operation, Shape, ShapeKind};
use crate::geometry::Join;

/// Every field a saved shape may hold. `From<ShapeRepr> for Shape` is an exhaustive literal, so a field
/// added to `Shape` without one here does not compile.
#[derive(Deserialize)]
pub(crate) struct ShapeRepr {
    id: u64,
    kind: ShapeKind,
    x_in: f32,
    y_in: f32,
    width_in: f32,
    height_in: f32,
    rotation_deg: f32,
    flip_x: bool,
    flip_y: bool,
    #[serde(with = "crate::design::color_serde")]
    fill: Color32,
    operation: Operation,
    hidden: bool,
    #[serde(default)]
    contours: Option<super::ContourMask>,
    /// Format 3 and earlier.
    #[serde(default)]
    contour_hidden: Option<Vec<bool>>,
    #[serde(default)]
    link: Option<crate::links::Link>,
    #[serde(default)]
    pen: crate::pens::Pen,
    #[serde(default)]
    effects: Effects,
    /// Format 2 and earlier.
    #[serde(default)]
    warp: Option<crate::warp::Warp>,
    #[serde(default)]
    container: Option<crate::containers::ContainerRole>,
    #[serde(default)]
    parent: Option<u64>,
    #[serde(default)]
    skew_deg: (f32, f32),
    #[serde(default)]
    pivot: Option<crate::design::Pt>,
    #[serde(default)]
    grain_lock: bool,
    #[serde(default)]
    lock_aspect: bool,
    #[serde(default)]
    lock_position: bool,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    pattern: Option<crate::patterns::PatternFill>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    monogram: Option<Box<crate::monogram::MonogramSpec>>,
}

impl From<ShapeRepr> for Shape {
    fn from(r: ShapeRepr) -> Shape {
        let mut effects = r.effects;
        if r.warp.is_some() && effects.is_empty() {
            effects.set_warp(r.warp);
        }
        let effects = effects.validated(&r.kind);
        Shape {
            id: r.id,
            kind: r.kind,
            x_in: r.x_in,
            y_in: r.y_in,
            width_in: r.width_in,
            height_in: r.height_in,
            rotation_deg: r.rotation_deg,
            flip_x: r.flip_x,
            flip_y: r.flip_y,
            fill: r.fill,
            operation: r.operation,
            hidden: r.hidden,
            contours: r.contours.unwrap_or_else(|| super::ContourMask { hidden: r.contour_hidden.unwrap_or_default(), enabled: true }),
            link: r.link,
            pen: r.pen,
            effects,
            container: r.container,
            parent: r.parent,
            skew_deg: r.skew_deg,
            pivot: r.pivot,
            grain_lock: r.grain_lock,
            lock_aspect: r.lock_aspect,
            lock_position: r.lock_position,
            locked: r.locked,
            pattern: r.pattern,
            name: r.name,
            monogram: r.monogram,
        }
    }
}

/// Every field a saved Offset may hold.
#[derive(Deserialize)]
pub(crate) struct OffsetRepr {
    /// Negative meant inside before spec 34.
    distance_in: f32,
    #[serde(default)]
    direction: Option<Direction>,
    #[serde(default)]
    join: Option<Join>,
    /// Before spec 34.
    #[serde(default)]
    corners: Option<Corners>,
    #[serde(default)]
    style: Option<Style>,
}

impl From<OffsetRepr> for Offset {
    fn from(r: OffsetRepr) -> Offset {
        let inside = if r.distance_in < 0.0 { Direction::Inset } else { Direction::Outset };
        Offset {
            distance_in: r.distance_in.abs(),
            direction: r.direction.unwrap_or(inside),
            join: r.join.or(r.corners.map(Corners::join)).unwrap_or(Join::Round),
            style: r.style,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::design::{Contour, Pt, Seg, Shape, ShapeKind};
    use crate::effects::model::Effect;
    use crate::warp::{Warp, WarpStyle};

    const ARCH: Warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };

    /// `shape` as format 2 saved it: no `effects`, `warp` in a key of its own, hidden contours as
    /// `contour_hidden` and a text curve as a plain `curve_in` number.
    fn format_2(shape: &Shape, warp: Warp) -> Value {
        let mut v = serde_json::to_value(shape).unwrap();
        let o = v.as_object_mut().unwrap();
        o.insert("warp".into(), serde_json::to_value(warp).unwrap());
        let mask = o.remove("contours").unwrap();
        o.insert("contour_hidden".into(), mask["hidden"].clone());
        if let Some(text) = o["kind"].get_mut("Text").and_then(Value::as_object_mut) {
            let diameter = text.remove("curve").and_then(|c| c.get("diameter_in").cloned()).unwrap_or(0.0.into());
            text.insert("curve_in".into(), diameter);
        }
        v
    }

    fn points(contours: &[Contour]) -> Vec<Pt> {
        contours.iter().flat_map(Contour::flatten).collect()
    }

    fn assert_same(a: &[Contour], b: &[Contour]) {
        let (a, b) = (points(a), points(b));
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(&b) {
            assert!((p.0 - q.0).abs() <= 1e-6 && (p.1 - q.1).abs() <= 1e-6, "{p:?} != {q:?}");
        }
    }

    /// The pipeline before spec 21: the kind's outline, warped in its box, hidden contours removed.
    fn pre_spec_outline(s: &Shape, warp: Warp, hidden: &[bool]) -> Vec<Contour> {
        let warped = crate::warp::apply(warp, &s.kind.outline(s.width_in, s.height_in), (s.width_in, s.height_in));
        warped.into_iter().enumerate().filter(|(i, _)| !hidden.get(*i).copied().unwrap_or(false)).map(|(_, c)| c).collect()
    }

    fn holed_square() -> ShapeKind {
        let ring = |a: f32, b: f32| Contour { start: (a, a), segs: [(b, a), (b, b), (a, b)].map(Seg::Line).to_vec(), closed: true };
        ShapeKind::Imported { contours: vec![ring(0.0, 4.0), ring(1.0, 3.0)], view_box: (4.0, 4.0) }
    }

    fn curved_text() -> ShapeKind {
        let mut d = crate::design::Design::default();
        let id = d.add_text("Curve".into(), crate::text::FontRef::Inter, 0.5, egui::Color32::BLACK, 0.0, 0.0);
        let mut kind = d.shape_mut(id).unwrap().kind.clone();
        let ShapeKind::Text { curve, .. } = &mut kind else { unreachable!() };
        *curve = crate::effects::Curve::from_diameter(3.0);
        kind
    }

    #[test]
    fn a_pre_spec_34_offset_loads_as_an_inset_with_a_miter_of_four() {
        use crate::effects::{Direction, Offset};
        use crate::geometry::{DEFAULT_MITER_LIMIT, Join};
        let inset: Offset = serde_json::from_str(r#"{"distance_in":-0.1,"corners":"Sharp","style":null}"#).unwrap();
        assert_eq!(inset, Offset { distance_in: 0.1, direction: Direction::Inset, join: Join::Miter { limit: DEFAULT_MITER_LIMIT }, style: None });
        let outset: Offset = serde_json::from_str(r#"{"distance_in":0.25,"corners":"Round"}"#).unwrap();
        assert_eq!(outset, Offset { distance_in: 0.25, direction: Direction::Outset, join: Join::Round, style: None });
        let saved = serde_json::to_value(inset).unwrap();
        assert!(saved.get("corners").is_none() && saved["direction"] == "Inset", "saved in the new shape: {saved}");
        assert_eq!(serde_json::from_value::<Offset>(saved).unwrap(), inset, "the upgrade is idempotent");
        let old_shape = r#"{"id":1,"kind":"Square","x_in":0,"y_in":0,"width_in":2,"height_in":2,"rotation_deg":0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"effects":{"entries":[{"id":0,"effect":{"Offset":{"distance_in":-0.1,"corners":"Sharp","style":null}}}],"next":1}}"#;
        let loaded: Shape = serde_json::from_str(old_shape).unwrap();
        assert_eq!(loaded.effects.entries()[0].effect, Effect::Offset(inset), "inside a saved shape");
    }

    #[test]
    fn a_pre_spec_warp_curve_and_hidden_contour_load_with_identical_outlines() {
        for (kind, hidden) in [(holed_square(), vec![false, true]), (curved_text(), vec![]), (ShapeKind::Star, vec![])] {
            let shape = Shape { width_in: 3.0, height_in: 2.0, contours: crate::effects::ContourMask { hidden: hidden.clone(), enabled: true }, ..Shape::new(1, kind) };
            let loaded: Shape = serde_json::from_value(format_2(&shape, ARCH)).unwrap();
            assert_eq!(loaded.effects.entries()[0].effect, Effect::Warp(ARCH), "the warp is the first entry");
            assert_eq!(loaded.effects.entries().len(), 1);
            assert_same(&loaded.contours(), &pre_spec_outline(&shape, ARCH, &hidden));
            let saved = serde_json::to_value(&loaded).unwrap();
            assert!(saved.get("warp").is_none() && saved.get("effects").is_some(), "saved in the new shape: {saved}");
            assert_eq!(serde_json::from_value::<Shape>(saved).unwrap(), loaded, "the upgrade is idempotent");
        }
    }

    #[test]
    fn a_pre_spec_sticker_with_a_warped_word_loads_and_renders_identically() {
        let word = Shape { x_in: 0.5, y_in: 0.5, width_in: 2.0, height_in: 0.8, ..Shape::new(2, curved_text()) };
        let backing = Shape { width_in: 3.0, height_in: 1.8, fill: egui::Color32::WHITE, ..Shape::new(1, ShapeKind::Square) };
        let mut sticker = crate::flatten::flattened(3, vec![backing, word]).expect("flattens");
        let mut v = serde_json::to_value(&sticker).unwrap();
        let part = &mut v["kind"]["Flattened"]["parts"][1];
        *part = format_2(&serde_json::from_value(part.clone()).unwrap(), ARCH);
        let loaded: Shape = serde_json::from_value(v).unwrap();

        let ShapeKind::Flattened { parts: loaded_parts, .. } = &loaded.kind else { unreachable!() };
        assert_eq!(loaded_parts[1].effects.warp(), Some(ARCH));
        let ShapeKind::Flattened { parts, .. } = &mut sticker.kind else { unreachable!() };
        assert_same(&loaded_parts[1].contours(), &pre_spec_outline(&parts[1], ARCH, &[]));
        parts[1].effects = loaded_parts[1].effects.clone();
        assert_eq!(loaded, sticker, "the sticker matches one built with the warp");
        assert_eq!(crate::flatten::paint_layers(&loaded), crate::flatten::paint_layers(&sticker), "and paints the same layers");
    }
}
