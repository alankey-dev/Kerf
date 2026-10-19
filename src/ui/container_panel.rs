//! The inspector's Container section (spec 18).

use std::num::NonZeroU16;

use egui::{Align2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, vec2};

use crate::containers::{AlignTo, Axis, Cross, Flow, Sizing};
use crate::design::Design;
use crate::edit::{Edit, Subject};
use crate::units::{FIELD_W, Units, length_field, number_field};

use super::chrome::text;
use super::fonts;
use super::inspector::section_title;
use super::tokens::*;

const MAX_LENGTH_IN: f32 = 10.0;
const CAPTION_W: f32 = 68.0;
const SEGMENT_H: f32 = 24.0;

/// Shown when exactly one container is selected: its padding, its auto layout and Remove container.
pub fn show(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>, units: Units) {
    let [id] = selection[..] else { return };
    let Some(mut role) = design.shapes.iter().find(|s| s.id == id).and_then(|s| s.container) else { return };
    ui.add_space(12.0);
    section_title(ui, "Container");
    row(ui, "Padding", |ui| {
        _ = length_field(ui, "Container padding", Some(role.padding_in), units, FIELD_W).is_some_and(|t| t.set(&mut role.padding_in, 0.0..=MAX_LENGTH_IN))
    });
    let axis = role.layout.map(|f| f.axis);
    let picked = {
        let grid = Axis::Grid { columns: NonZeroU16::MIN.saturating_add(1) };
        let radial = Axis::Radial { radius_in: 2.0, start_deg: 0.0, sweep_deg: 360.0, rotate_items: true };
        let options = [("None", None), ("Row", Some(Axis::Row)), ("Column", Some(Axis::Column)), ("Grid", Some(grid)), ("Radial", Some(radial))];
        segmented(ui, "Layout", &options, axis, |a, b| a.as_ref().map(std::mem::discriminant) == b.as_ref().map(std::mem::discriminant))
    };
    if let Some(axis) = picked {
        let flow = role.layout.unwrap_or(Flow { axis: Axis::Row, gap_in: 0.25, cross: Cross::Start, sizing: Sizing::Hug });
        role.layout = axis.map(|axis| Flow { axis, ..flow });
    }
    if let Some(flow) = &mut role.layout {
        if let Axis::Radial { radius_in, start_deg, sweep_deg, rotate_items } = &mut flow.axis {
            row(ui, "Radius", |ui| _ = length_field(ui, "Radial radius", Some(*radius_in), units, FIELD_W).is_some_and(|t| t.set(radius_in, 0.0..=MAX_LENGTH_IN)));
            row(ui, "Start", |ui| _ = number_field(ui, "Radial start", Some(*start_deg), "°", FIELD_W).is_some_and(|t| t.set(start_deg, -360.0..=360.0)));
            row(ui, "Sweep", |ui| _ = number_field(ui, "Radial sweep", Some(*sweep_deg), "°", FIELD_W).is_some_and(|t| t.set(sweep_deg, -360.0..=360.0)));
            if let Some(on) = segmented(ui, "Rotate items", &[("On", true), ("Off", false)], *rotate_items, |a, b| a == b) {
                *rotate_items = on;
            }
        } else {
            row(ui, "Gap", |ui| _ = length_field(ui, "Layout gap", Some(flow.gap_in), units, FIELD_W).is_some_and(|t| t.set(&mut flow.gap_in, 0.0..=MAX_LENGTH_IN)));
            if let Some(c) = segmented(ui, "Align items", &[("Start", Cross::Start), ("Centre", Cross::Centre), ("End", Cross::End)], flow.cross, |a, b| a == b) {
                flow.cross = c;
            }
            if let Some(s) = segmented(ui, "Size", &[("Hug", Sizing::Hug), ("Fixed", Sizing::Fixed)], flow.sizing, |a, b| a == b) {
                flow.sizing = s;
            }
            if let Axis::Grid { columns } = &mut flow.axis {
                row(ui, "Columns", |ui| {
                    let mut n = f32::from(columns.get());
                    if number_field(ui, "Grid columns", Some(n), "", FIELD_W).is_some_and(|t| t.set(&mut n, 1.0..=20.0)) {
                        *columns = NonZeroU16::new(n.round() as u16).unwrap_or(NonZeroU16::MIN);
                    }
                });
            }
        }
    }
    ui.add_space(4.0);
    let e = super::commands::Cx::new(design, selection, AlignTo::default()).entry(Edit::Uncontain);
    let remove = ui.add_sized(vec2(ui.available_width(), 28.0), egui::Button::new(e.label));
    remove.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, e.label));
    if super::tooltip::tip(remove, e.label, &e.tip_body(), None).clicked() {
        _ = crate::edit::run(design, selection, e.edit(), Subject::Selection);
        return;
    }
    if let Some(s) = design.shape_mut(id) {
        s.container = Some(role);
    }
}

fn row<R>(ui: &mut Ui, caption: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(CAPTION_W, SEGMENT_H), Sense::hover());
        ui.put(rect, egui::Label::new(RichText::new(caption).font(fonts::regular(12.0)).color(TEXT_MUTED)));
        body(ui)
    })
    .inner
}

/// A row of toggle buttons, one per option, labelled "`group` `name`" for tests and screen readers.
/// Captioned above; the cells share the full width equally and the chosen one is filled with the accent.
fn segmented<T: Copy>(ui: &mut Ui, group: &str, options: &[(&str, T)], current: T, same: impl Fn(&T, &T) -> bool) -> Option<T> {
    ui.label(RichText::new(group).font(fonts::regular(12.0)).color(TEXT_MUTED));
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), SEGMENT_H), Sense::hover());
    ui.painter().rect_stroke(bar, 4.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let cell = bar.width() / options.len() as f32;
    let mut picked = None;
    for (i, &(name, value)) in options.iter().enumerate() {
        let on = same(&value, &current);
        let rect = Rect::from_min_size(bar.min + vec2(i as f32 * cell, 0.0), vec2(cell, SEGMENT_H));
        let resp = ui.interact(rect, ui.id().with((group, name)), Sense::click());
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, format!("{group} {name}")));
        let painter = ui.painter();
        if on {
            painter.rect_filled(rect.shrink(1.0), 3.0, ACCENT_PRIMARY);
        } else if resp.hovered() {
            painter.rect_filled(rect.shrink(1.0), 3.0, ACCENT_HOVER);
        }
        let ink = if on { ON_PRIMARY } else { TEXT_PRIMARY };
        text(painter, rect.center(), Align2::CENTER_CENTER, name, fonts::regular(12.0), ink);
        if resp.clicked() && !on {
            picked = Some(value);
        }
    }
    picked
}
