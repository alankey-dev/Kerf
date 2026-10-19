//! What Layers and the inspector list for a shape: its curve and hidden contours, pinned first, then its
//! stack in order. A derived view, so nothing is stored twice.

use super::eval::{Geometry, Local};
use super::model::{Corners, Direction, Effect, EffectId};
use crate::design::{Shape, ShapeKind};
use crate::geometry::Join;
use crate::units::Units;

/// Which row of a shape's list: a pinned source setting, or a stack entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    Curve,
    Contour,
    Entry(EffectId),
}

/// One row of one shape, as a panel or an edit names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntryRef {
    pub shape: u64,
    pub row: Row,
}

/// The kind of row, which picks its icon and its editing panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Curve,
    Contour,
    Warp,
    Offset,
    Sticker,
}

impl RowKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Curve => "Curve",
            Self::Contour => "Contour",
            Self::Warp => "Warp",
            Self::Offset => "Offset",
            Self::Sticker => "Sticker",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowView {
    pub key: Row,
    pub kind: RowKind,
    /// Its parameters in the current units, for example "0.25 in inset, miter".
    pub summary: String,
    /// Every row has an eye.
    pub enabled: bool,
    /// On, but adding nothing, as an inset that consumes the whole shape does. It looks turned off.
    pub idle: bool,
    /// Only stack entries reorder; Curve and Contour always come first.
    pub draggable: bool,
}

impl RowView {
    /// The tooltip line: "Offset 0.25 in inset, miter".
    pub fn label(&self) -> String {
        format!("{} {}", self.kind.name(), self.summary)
    }

    /// Whether it shows as applying: on and adding something.
    pub fn active(&self) -> bool {
        self.enabled && !self.idle
    }
}

fn corners(c: Corners) -> &'static str {
    match c {
        Corners::Round => "round",
        Corners::Sharp => "sharp",
    }
}

fn join(j: Join) -> &'static str {
    match j {
        Join::Round => "round",
        Join::Miter { .. } => "miter",
        Join::Bevel => "bevel",
    }
}

fn direction(d: Direction) -> &'static str {
    match d {
        Direction::Outset => "outset",
        Direction::Inset => "inset",
    }
}

/// How many rows `rows` lists for `shape`, without evaluating it.
pub fn count(shape: &Shape) -> usize {
    usize::from(matches!(shape.kind, ShapeKind::Text { curve: Some(_), .. })) + usize::from(shape.contours.any()) + shape.effects.entries().len()
}

/// Curve (text with a curve), then Contour (any hidden), then the stack in order. `geometry` is the
/// shape's evaluation, which says whether an Offset added anything.
pub fn rows(shape: &Shape, geometry: &Geometry, units: Units) -> Vec<RowView> {
    let mut out = Vec::new();
    if let ShapeKind::Text { curve: Some(c), .. } = &shape.kind {
        let side = if c.diameter_in >= 0.0 { "text on top" } else { "text below" };
        out.push(RowView {
            key: Row::Curve,
            kind: RowKind::Curve,
            summary: format!("{} across, {side}", units.len(c.diameter_in.abs())),
            enabled: c.enabled,
            idle: false,
            draggable: false,
        });
    }
    if shape.contours.any() {
        let n = shape.contours.hidden.iter().filter(|&&h| h).count();
        out.push(RowView { key: Row::Contour, kind: RowKind::Contour, summary: format!("{n} hidden"), enabled: shape.contours.enabled, idle: false, draggable: false });
    }
    for e in shape.effects.entries() {
        let (kind, summary) = match e.effect {
            Effect::Warp(w) => (RowKind::Warp, format!("{}, bend {}{}", w.style.label(), (w.bend * 100.0).round(), if w.vertical { ", vertical" } else { "" })),
            Effect::Offset(o) => (RowKind::Offset, format!("{} {}, {}", units.len(o.distance_in), direction(o.direction), join(o.join))),
            Effect::Sticker(s) => (RowKind::Sticker, format!("{} border, {}", units.len(s.border_in), corners(s.corners))),
        };
        let idle = e.enabled && kind == RowKind::Offset && !geometry.pieces().iter().any(|p| p.id.local == Local::Effect(e.id));
        out.push(RowView { key: Row::Entry(e.id), kind, summary, enabled: e.enabled, idle, draggable: true });
    }
    out
}

/// Whether `shape` still lists `row`, so a panel open on it can tell when to close.
pub fn has_row(shape: &Shape, row: Row) -> bool {
    match row {
        Row::Curve => matches!(shape.kind, ShapeKind::Text { curve: Some(_), .. }),
        Row::Contour => shape.contours.any(),
        Row::Entry(id) => shape.effects.get(id).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::model::{Effects, Offset};
    use crate::warp::{Warp, WarpStyle};

    #[test]
    fn pinned_rows_come_first_then_the_stack_in_order() {
        let mut d = crate::design::Design::default();
        let t = d.add_text("Hi".into(), crate::text::FontRef::Inter, 0.5, egui::Color32::BLACK, 0.0, 0.0);
        d.set_text_curve(t, -3.0);
        let s = d.shape_mut(t).unwrap();
        s.contours.hidden = vec![true, false];
        let mut fx = Effects::default();
        fx.push(Effect::Offset(Offset { distance_in: 0.25, direction: Direction::Inset, join: Join::Miter { limit: 4.0 }, style: None }));
        fx.set_warp(Some(Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false }));
        s.effects = fx;
        let s = d.shapes.iter().find(|s| s.id == t).unwrap();
        let rows = rows(s, &d.geometry(s), Units::Imperial);
        assert_eq!(count(s), rows.len());
        let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [RowKind::Curve, RowKind::Contour, RowKind::Warp, RowKind::Offset]);
        assert_eq!(rows[3].label(), "Offset 0.25 in inset, miter");
        assert_eq!(rows[0].label(), "Curve 3 in across, text below");
        assert_eq!(rows[2].label(), "Warp Arch, bend 50");
        assert!(!rows[0].draggable && rows[3].draggable);
    }
}
