use egui::{Align2, Color32, Context, Frame, Rect, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::bridge::mat_placements;
use crate::composition::{Mat, compose};
use crate::design::Design;
use crate::make_flow::{MakeFlow, MatSetup};
use crate::output::DeviceProfile;
use crate::units::Units;

use super::chrome::{icon_button, text, text_width};
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::tokens::*;
use super::tooltip::tip;

const PREVIEW_BOX: f32 = 100.0;
const CARD_PREVIEW: f32 = 120.0;
const LINE_H: f32 = 24.0;
/// A 12 x 12 in cutting area sits on a 12.5 x 13.5 in mat: a quarter inch each side, half an inch above.
const MAT_MARGIN_IN: (f32, f32, f32) = (0.25, 0.5, 1.0);
/// `MAT_VISUALIZATION_DETAILS_TOOLTIP_TEXT`, with its em dash rewritten as a colon.
const DETAILS_TIP: &str = "See how your design will cut: each mat represents a separate load. During Make, you can adjust materials, copies, and placement.";
const OVERFLOW_WARNING: &str = "Too large for this mat";

/// The Make flow's right panel: `mat-visualization-panel` for the mats being cut, 1px left border.
pub fn panel(ctx: &Context, flow: &MakeFlow, units: Units) {
    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin::same(16));
    let panel = egui::SidePanel::right("project-preview").exact_width(PREVIEW_PANEL_W).resizable(false).frame(frame).show_separator_line(false);
    panel.show(ctx, |ui| {
        let full = ui.max_rect().expand(16.0);
        ui.painter().vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
        ui.label(egui::RichText::new("Project Preview").font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| body(ui, &flow.design, &flow.mats, &flow.setup, flow.profile(), units));
    });
}

/// The editor's Preview button: the same mat details in a dismissible floating card, never permanent
/// chrome. Returns false once closed.
pub fn overlay(ctx: &Context, design: &Design, setup: &MatSetup, units: Units) -> bool {
    let mut open = true;
    let frame = Frame::NONE.fill(BG_PANEL).corner_radius(8).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).inner_margin(egui::Margin::same(16)).shadow(egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: SHADOW,
    });
    egui::Area::new(egui::Id::new("preview-overlay")).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(PREVIEW_PANEL_W - 32.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Project Preview").font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    open = !icon_button(ui, Icon::Close, "Close Project Preview", true).clicked();
                });
            });
            ui.add_space(8.0);
            let mats = compose(design, setup.mat_size, setup.copies);
            let tallest = ctx.screen_rect().height() * 0.7;
            egui::ScrollArea::vertical().max_height(tallest).min_scrolled_height(tallest).show(ui, |ui| body(ui, design, &mats, setup, None, units));
        });
    });
    open
}

fn body(ui: &mut Ui, design: &Design, mats: &[Mat], setup: &MatSetup, profile: Option<&DeviceProfile>, units: Units) {
    if !mat_details_header(ui, mats.len()) {
        return;
    }
    ui.add_space(8.0);
    for (n, mat) in mats.iter().enumerate() {
        if n > 0 {
            let (spacer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
            ui.painter().hline(spacer.x_range(), spacer.center().y.round() + 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
        }
        mat_card(ui, n + 1, mat, design, setup, profile, units);
    }
}

/// `mat-visualization__accordionHeader`: title, info tooltip, count badge and a minus or plus toggle.
/// Clicking anywhere on it collapses or expands the cards. Returns whether the cards show.
fn mat_details_header(ui: &mut Ui, count: usize) -> bool {
    let id = ui.id().with("mat-details-open");
    let open = ui.data(|d| d.get_temp(id).unwrap_or(true));
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, "Mat Details"));
    let open = open != resp.clicked();
    ui.data_mut(|d| d.insert_temp(id, open));

    let font = fonts::medium(14.0);
    let label_w = text_width(ui, "Mat Details", font.clone());
    text(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, "Mat Details", font, TEXT_PRIMARY);
    let info = Rect::from_min_size(pos2(rect.left() + label_w + 5.0, rect.center().y - 8.0), Vec2::splat(16.0));
    paint_icon(ui, Icon::Info, info, TEXT_PRIMARY);
    let info_resp = ui.interact(info, id.with("info"), Sense::hover()).on_hover_cursor(egui::CursorIcon::Help);
    info_resp.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "About Mat Details"));
    tip(info_resp, "Mat Details", DETAILS_TIP, None);

    let badge = pos2(rect.right() - 16.0 - 10.0 - 8.0, rect.center().y);
    ui.painter().circle_filled(badge, 8.0, ACCENT_PRIMARY);
    text(ui.painter(), badge, Align2::CENTER_CENTER, &count.to_string(), fonts::medium(11.0), Color32::WHITE);
    let toggle = if open { Icon::Minus } else { Icon::Plus };
    paint_icon(ui, toggle, Rect::from_min_size(pos2(rect.right() - 16.0, rect.center().y - 8.0), Vec2::splat(16.0)), TEXT_PRIMARY);
    open
}

/// `mat-visualization-card`: a 120 px preview, then Load N, the joined operations, the mat size and the
/// mat style. An open device has no mat style, so its work area stands in.
fn mat_card(ui: &mut Ui, load: usize, mat: &Mat, design: &Design, setup: &MatSetup, profile: Option<&DeviceProfile>, units: Units) {
    let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), CARD_PREVIEW + 8.0), Sense::hover());
    let preview = Rect::from_min_size(card.min + Vec2::splat(4.0), Vec2::splat(CARD_PREVIEW));
    paint_mat(ui, Rect::from_center_size(preview.center(), Vec2::splat(PREVIEW_BOX)), mat, design, setup);

    let label = mat.label();
    let mut lines = vec![(format!("Load {load}"), fonts::semibold(14.0), TEXT_PRIMARY)];
    let style = profile.map_or_else(|| setup.load_type.label().to_owned(), |p| units.size(p.work_area_in().0, p.work_area_in().1));
    for line in [label.as_str(), setup.mat_size.name(units), style.as_str()] {
        lines.push((line.to_owned(), fonts::regular(14.0), TEXT_MUTED));
    }
    if mat.overflow {
        lines.push((OVERFLOW_WARNING.to_owned(), fonts::medium(14.0), TEXT_PRIMARY));
    }
    let x = preview.right() + 4.0;
    let top = preview.center().y - (lines.len() - 1) as f32 * LINE_H / 2.0;
    for (i, (line, font, ink)) in lines.iter().enumerate() {
        let at = text(ui.painter(), pos2(x, top + LINE_H * i as f32), Align2::LEFT_CENTER, line, font.clone(), *ink);
        ui.interact(at, ui.id().with(("mat-card", load, i)), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, line));
    }
}

/// White lines on a dark material, dark ones on a light material.
fn ink_on(fill: Color32) -> Color32 {
    let luma = 0.299 * f32::from(fill.r()) + 0.587 * f32::from(fill.g()) + 0.114 * f32::from(fill.b());
    if luma < 128.0 { Color32::WHITE } else { Color32::from_gray(0x33) }
}

/// The mat as `mat-preview` draws it: grey mat, the material colour inside, an inch grid, the packed paths.
fn paint_mat(ui: &Ui, fit: Rect, mat: &Mat, design: &Design, setup: &MatSetup) {
    let size = setup.mat_size;
    let (side, above, below) = MAT_MARGIN_IN;
    let mat_in = vec2(size.width_in + 2.0 * side, size.height_in + above + below);
    let px_per_in = (fit.width() / mat_in.x).min(fit.height() / mat_in.y);
    let backing = Rect::from_center_size(fit.center(), mat_in * px_per_in);
    let painter = ui.painter();
    for (grow, alpha) in [(2.0, 10u8), (1.0, 22)] {
        painter.rect_filled(backing.expand(grow).translate(vec2(0.0, 1.0)), 3.0, Color32::from_black_alpha(alpha));
    }
    painter.rect_filled(backing, 2.0, MAT_GREY);
    let notch = Rect::from_center_size(pos2(backing.center().x, backing.bottom() - 0.35 * px_per_in), vec2(1.2, 0.18) * px_per_in);
    painter.rect_filled(notch, 1.0, Color32::from_gray(0xe6));
    let area = Rect::from_min_size(backing.min + vec2(side, above) * px_per_in, vec2(size.width_in, size.height_in) * px_per_in);
    painter.rect_filled(area, 0.0, mat.colour);
    let ink = ink_on(mat.colour);
    let grid = Stroke::new(1.0_f32, ink.gamma_multiply(0.25));
    for i in 1..size.width_in as u32 {
        painter.vline((area.left() + i as f32 * px_per_in).round() + 0.5, area.y_range(), grid);
    }
    for i in 1..size.height_in as u32 {
        painter.hline(area.x_range(), (area.top() + i as f32 * px_per_in).round() + 0.5, grid);
    }

    let to_mat = super::canvas::Grid { origin: area.min, scale: px_per_in }.matrix();
    for p in mat_placements(design, mat, setup) {
        super::op_style::paint_placed(painter, design, p.shape, p.piece().id, super::op_style::in_inches(p.transform).then(to_mat), px_per_in, mat.colour);
    }
}
