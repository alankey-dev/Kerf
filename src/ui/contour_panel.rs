use egui::{Align2, Color32, Context, Frame, Pos2, Rect, Sense, Stroke, Ui, Vec2, vec2};

use crate::design::Design;

use super::chrome::{icon_button, text};
use super::fonts;
use super::icons::Icon;
use super::tokens::*;

const PANEL_W: f32 = 280.0;
const PREVIEW: f32 = 248.0;

/// Shows each contour of a shape; clicking one toggles its visibility. Returns false once closed.
pub fn show(ctx: &Context, design: &mut Design, id: u64) -> bool {
    let frame = Frame::NONE.fill(Color32::WHITE).inner_margin(egui::Margin::same(16));
    let panel = egui::SidePanel::right("contour").exact_width(PANEL_W).resizable(false).frame(frame).show_separator_line(false);
    panel
        .show(ctx, |ui| {
            let full = ui.max_rect().expand(16.0);
            ui.painter().vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
            let open = ui
                .horizontal(|ui| {
                    let (title, _) = ui.allocate_exact_size(vec2(200.0, 24.0), Sense::hover());
                    text(ui.painter(), title.left_center(), Align2::LEFT_CENTER, "Contour", fonts::display(16.0), TEXT_PRIMARY);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| !icon_button(ui, Icon::Close, "Close Contour", true).clicked()).inner
                })
                .inner;
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Tap a contour to hide or show it.").font(fonts::regular(13.0)).color(TEXT_MUTED));
            ui.add_space(8.0);
            if let Some(toggle) = preview(ui, design, id) {
                toggle_contour(design, id, toggle);
            }
            open
        })
        .inner
}

/// Draws the shape's contours in its local frame and returns the index of any that was clicked.
fn preview(ui: &mut Ui, design: &Design, id: u64) -> Option<usize> {
    let shape = design.shapes.iter().find(|s| s.id == id)?;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(PREVIEW), Sense::hover());
    ui.painter().rect_filled(rect, 6.0, Color32::from_gray(0xf6));
    let contours = crate::effects::body_contours(shape);
    let scale = (PREVIEW - 24.0) / shape.width_in.max(shape.height_in).max(0.01);
    let origin = rect.center() - vec2(shape.width_in, shape.height_in) * scale / 2.0;
    let place = |(x, y): (f32, f32)| origin + vec2(x, y) * scale;

    let mut clicked = None;
    for (i, contour) in contours.iter().enumerate() {
        let pts: Vec<Pos2> = contour.flatten().into_iter().map(place).collect();
        let hidden = shape.contours.hidden.get(i).copied().unwrap_or(false);
        let id = ui.id().with(("contour", i));
        let resp = ui.interact(Rect::from_points(&pts), id, Sense::click());
        let colour = match (hidden, resp.hovered()) {
            (true, _) => Color32::from_gray(0xc0),
            (false, true) => ACCENT_PRIMARY,
            (false, false) => TEXT_PRIMARY,
        };
        ui.painter().add(egui::Shape::closed_line(pts, Stroke::new(if resp.hovered() { 2.5_f32 } else { 1.5 }, colour)));
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

fn toggle_contour(design: &mut Design, id: u64, index: usize) {
    if let Some(shape) = design.shape_mut(id) {
        let count = crate::effects::body_contours(shape).len();
        if shape.contours.hidden.len() < count {
            shape.contours.hidden.resize(count, false);
        }
        if let Some(flag) = shape.contours.hidden.get_mut(index) {
            *flag = !*flag;
        }
    }
}
