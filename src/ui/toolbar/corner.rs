//! The Corner toolbar, which stands in for the edit toolbar while the Corner tool is active (spec 55):
//! a Radius field applying to the selected nodes, the three corner styles, Round all corners, and Done.

use egui::{Context, Frame, Stroke, vec2};

use crate::corners::{CornerOp, CornerStyle};
use crate::design::Design;
use crate::edit::{Edit, Subject};
use crate::units::{Units, length_field};

use super::super::corner_tool::{CornerEdit, display_path};
use super::super::tokens::*;

const FIELD_W: f32 = 96.0;

pub fn show(ctx: &Context, design: &mut Design, selection: &mut Vec<u64>, corners: &mut Option<CornerEdit>, units: Units) {
    let Some(edit) = corners.as_mut() else { return };
    let Some(shape) = design.shapes.iter().find(|s| s.id == edit.shape).cloned() else {
        *corners = None;
        return;
    };
    let path = display_path(&shape);
    let eligible = path.eligible(&edit.selected);
    // The radius the field shows: the one the selected eligible nodes share, else the working radius.
    let shared = eligible.iter().filter_map(|id| path.node(*id)).map(|n| n.corner.radius_in).reduce(|a, b| if (a - b).abs() < 1e-4 { a } else { f32::NAN });
    let shown = match shared {
        Some(r) if r.is_finite() && r > 0.0 => r,
        _ => edit.radius_in,
    };

    let mut op: Option<CornerOp> = None;
    let mut done = false;

    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin { left: 8, right: 4, top: 4, bottom: 4 });
    let panel = egui::TopBottomPanel::top("edit-toolbar").exact_height(TOOLBAR_H).frame(frame).show_separator_line(false);
    panel.show(ctx, |ui| {
        let bar = ui.max_rect().expand2(vec2(8.0, 4.0));
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
        ui.horizontal_centered(|ui| {
            ui.label("Radius");
            if let Some(typed) = length_field(ui, "Corner radius", Some(shown), units, FIELD_W) {
                let radius = typed.apply(shown).max(0.0);
                edit.radius_in = radius;
                if eligible.is_empty() {
                    op = Some(CornerOp::RoundAll { radius_in: radius });
                } else {
                    op = Some(CornerOp::Set { nodes: eligible.clone(), radius: Some(radius), style: None });
                }
            }
            super::divider(ui);
            for style in CornerStyle::ALL {
                let on = edit.style == style;
                if ui.selectable_label(on, style.label()).clicked() {
                    edit.style = style;
                    if !eligible.is_empty() {
                        op = Some(CornerOp::Set { nodes: eligible.clone(), radius: None, style: Some(style) });
                    }
                }
            }
            super::divider(ui);
            if ui.button("Round all corners").clicked() {
                op = Some(CornerOp::RoundAll { radius_in: edit.radius_in.max(super::super::corner_tool::DEFAULT_RADIUS_IN) });
            }
            ui.add_space(12.0);
            ui.label(egui::RichText::new(units.len(edit.radius_in)).color(TEXT_MUTED));
            ui.add_space(ui.available_width() - 56.0);
            done = ui.button("Done").clicked();
        });
    });

    if let Some(op) = op {
        _ = crate::edit::run(design, selection, &Edit::Corners(op), Subject::Ids(&[shape.id]));
    }
    if done {
        *corners = None;
    }
}
