//! A modal question over a faint scrim, and the error banner. Home's delete confirmations, the offer
//! to restore an autosave and the one-Make question share the modal; every failure the user should see
//! goes to the banner.

use egui::{Align2, Area, Color32, Context, Frame, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::text;
use super::fonts;
use super::tokens::*;

/// A modal's button, right-aligned in the order given.
pub struct Choice<'a> {
    pub label: &'a str,
    /// The accessibility label, when the visible one is ambiguous ("Delete" vs "Confirm delete").
    pub a11y: &'a str,
    pub primary: bool,
}

/// Shows the modal; `Some(i)` once choice `i` is clicked. Escape picks `escape`.
pub fn show(ctx: &Context, id: &str, title: &str, detail: &str, choices: &[Choice], escape: usize) -> Option<usize> {
    let screen = ctx.screen_rect();
    Area::new(Id::new((id, "scrim"))).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.allocate_rect(screen, Sense::click_and_drag());
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(28));
    });
    let mut answer = None;
    let frame = Frame::NONE.fill(BG_PANEL).corner_radius(8).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).inner_margin(egui::Margin::same(20)).shadow(egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: SHADOW,
    });
    let width = 300.0_f32.max(choices.iter().map(|c| button_w(c.label).max(104.0)).sum());
    Area::new(Id::new(id)).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(title).font(fonts::semibold(15.0)).color(TEXT_PRIMARY));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(detail).font(fonts::regular(13.0)).color(TEXT_MUTED));
            ui.add_space(16.0);
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
            let mut right = row.right();
            for (i, choice) in choices.iter().enumerate().rev() {
                let w = button_w(choice.label);
                let rect = Rect::from_min_size(pos2(right - w, row.top()), vec2(w, 28.0));
                if button(ui, rect, choice.label, choice.a11y, choice.primary) {
                    answer = Some(i);
                }
                right -= w + 8.0;
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(escape);
    }
    answer
}

/// A button's width: two sizes for words, and room for a phrase like "Detach 3 instances and delete".
fn button_w(label: &str) -> f32 {
    match label.len() {
        0..=8 => 80.0,
        9..=14 => 104.0,
        n => n as f32 * 7.0 + 24.0,
    }
}

/// A rounded text button: filled accent when `primary`, outlined otherwise.
pub fn button(ui: &mut Ui, rect: Rect, label: &str, a11y: &str, primary: bool) -> bool {
    let resp = ui.interact(rect, ui.id().with(("modal-btn", rect.left() as i32, a11y)), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, a11y));
    let painter = ui.painter();
    let (fill, ink) = match (primary, resp.hovered()) {
        (true, false) => (ACCENT_PRIMARY, ON_PRIMARY),
        (true, true) => (PRIMARY_HOVER, ON_PRIMARY),
        (false, false) => (BG_PANEL, ACCENT_PRIMARY),
        (false, true) => (ACCENT_HOVER, ACCENT_PRIMARY),
    };
    painter.rect(rect, 11, fill, Stroke::new(1.0_f32, ACCENT_PRIMARY), StrokeKind::Inside);
    text(painter, rect.center(), Align2::CENTER_CENTER, label, fonts::medium(13.0), ink);
    resp.clicked()
}

/// A banner at the top centre naming what failed. Returns true when the user dismisses it.
pub fn error_banner(ctx: &Context, message: &str) -> bool {
    let mut dismissed = false;
    let frame = Frame::NONE
        .fill(WARNING_BG)
        .corner_radius(8)
        .stroke(Stroke::new(1.0_f32, WARNING_TEXT))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .shadow(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: SHADOW });
    Area::new(Id::new("error-banner")).order(Order::Tooltip).anchor(Align2::CENTER_TOP, vec2(0.0, 64.0)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(message).font(fonts::regular(13.0)).color(TEXT_PRIMARY));
                ui.add_space(12.0);
                let (rect, _) = ui.allocate_exact_size(vec2(80.0, 26.0), Sense::hover());
                dismissed = button(ui, rect, "Dismiss", "Dismiss error", false);
            });
        });
    });
    dismissed
}
