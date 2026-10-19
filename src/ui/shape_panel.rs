//! The inspector's Shape section (spec 49). It shows one field per parameter of the selected shapes
//! when they all share a `ParamKind`, with the shape's toggles below. A typed value applies to every
//! selected shape, as the text toolbar does, and each commit runs one `Edit::Params`, so it is one undo
//! step. Mixed values show blank.

use egui::Ui;

use crate::design::{Design, ShapeKind};
use crate::edit::{Edit, Subject};
use crate::expr::Typed;
use crate::parametric::{ArcClose, ParamField, ParamKind, ParamOp, Params};
use crate::units::{Units, length_field, number_field};

use super::inspector::{labelled, section_title};

pub fn section(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>, units: Units) {
    let params: Vec<Params> = selection
        .iter()
        .filter_map(|id| design.shapes.iter().find(|s| s.id == *id))
        .filter_map(|s| match &s.kind {
            ShapeKind::Parametric(p) => Some(*p),
            _ => None,
        })
        .collect();
    // Shown only when every selected shape is parametric and of one family.
    if params.is_empty() || params.len() != selection.len() {
        return;
    }
    let kind = params[0].kind();
    if params.iter().any(|p| p.kind() != kind) {
        return;
    }
    let fields = params[0].fields();
    let has_toggles = matches!(kind, ParamKind::Rectangle | ParamKind::Arc);
    if fields.is_empty() && !has_toggles {
        return;
    }

    ui.add_space(8.0);
    section_title(ui, "Shape");
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
    let width = ui.available_width();
    let mut op: Option<ParamOp> = None;

    for &field in fields {
        let label = field_label(field, kind);
        let common = shared(&params, field).map(|v| display(field, v));
        let typed = labelled(ui, label, width, |ui, w| {
            if field.is_length() { length_field(ui, label, shared(&params, field), units, w) } else { number_field(ui, label, common, suffix(field), w) }
        });
        if let Some(t) = typed {
            op = Some(ParamOp::Set { field, value: to_stored(field, t) });
        }
    }

    match kind {
        ParamKind::Rectangle => {
            let mut linked = shared_bool(&params, |p| matches!(p, Params::Rectangle { linked: true, .. })).unwrap_or(false);
            if ui.checkbox(&mut linked, "Link corners").changed() {
                op = Some(ParamOp::Link(linked));
            }
            let mut scale = shared_bool(&params, |p| matches!(p, Params::Rectangle { scale_with_shape: true, .. })).unwrap_or(false);
            if ui.checkbox(&mut scale, "Scale with shape").changed() {
                op = Some(ParamOp::ScaleWithShape(scale));
            }
        }
        ParamKind::Arc => {
            let close = params.iter().find_map(|p| match p {
                Params::Arc { close, .. } => Some(*close),
                _ => None,
            });
            ui.horizontal(|ui| {
                for (c, name) in [(ArcClose::Pie, "Pie"), (ArcClose::Chord, "Chord"), (ArcClose::Open, "Open")] {
                    if ui.selectable_label(close == Some(c), name).clicked() {
                        op = Some(ParamOp::Close(c));
                    }
                }
            });
        }
        _ => {}
    }

    if let Some(op) = op {
        _ = crate::edit::run(design, selection, &Edit::Params(op), Subject::Selection);
    }
}

/// The field's value that every selected shape agrees on, else `None` for a mixed selection.
fn shared(params: &[Params], field: ParamField) -> Option<f32> {
    let first = params.first()?.get(field)?;
    params.iter().all(|p| p.get(field).is_some_and(|v| (v - first).abs() < 1e-4)).then_some(first)
}

fn shared_bool(params: &[Params], on: impl Fn(&Params) -> bool) -> Option<bool> {
    let first = on(params.first()?);
    params.iter().all(|p| on(p) == first).then_some(first)
}

/// Fractions are stored 0..1 but shown as a percentage.
fn is_fraction(field: ParamField) -> bool {
    matches!(field, ParamField::Inner | ParamField::ArcInner | ParamField::Thickness | ParamField::Bar)
}

fn display(field: ParamField, stored: f32) -> f32 {
    if is_fraction(field) { stored * 100.0 } else { stored }
}

fn to_stored(field: ParamField, typed: Typed) -> Typed {
    match (typed, is_fraction(field)) {
        (Typed::To(v), true) => Typed::To(v / 100.0),
        (other, _) => other,
    }
}

fn suffix(field: ParamField) -> &'static str {
    match field {
        ParamField::ArcStart | ParamField::ArcSweep => "\u{b0}",
        _ if is_fraction(field) => "%",
        _ => "",
    }
}

fn field_label(field: ParamField, kind: ParamKind) -> &'static str {
    match field {
        ParamField::Radius => "Radius",
        ParamField::Sides if kind == ParamKind::Star => "Points",
        ParamField::Sides => "Sides",
        ParamField::Fillet => "Round",
        ParamField::Inner => "Inner",
        ParamField::Point => "Point",
        ParamField::Valley => "Valley",
        ParamField::ArcStart => "Start",
        ParamField::ArcSweep => "Sweep",
        ParamField::ArcInner => "Hole",
        ParamField::Thickness => "Width",
        ParamField::Bar => "Bar",
    }
}
