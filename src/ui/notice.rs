//! A one-line notice under the edit toolbar, such as "Writing fonts draw with a pen", with an optional
//! Undo link. It goes when closed, after a few seconds, or once the design changes (an Undo or the next
//! edit), so it never describes a design that is no longer on the canvas.

use egui::{Align2, Area, Context, Frame, Id, Order, Sense, Stroke, WidgetInfo, WidgetType, vec2};

use super::chrome::{icon_button, text, text_width};
use super::fonts;
use super::icons::Icon;
use super::tokens::*;
use crate::design::{Design, Operation};
use crate::edit::{Edit, Subject};
use crate::text_ops::TextChange;

const SHOWN_FOR_S: f64 = 8.0;
/// The close button's height, which every item in the row shares so their text lines up.
const ROW_H: f32 = 28.0;

pub struct Notice {
    message: String,
    undo: bool,
    /// When it goes, in `ctx` input time; set on the first frame it shows.
    until: Option<f64>,
    /// The design it describes, as it stood on the first frame it showed.
    design: Option<Design>,
}

impl Notice {
    pub fn new(message: impl Into<String>, undo: bool) -> Self {
        Self { message: message.into(), undo, until: None, design: None }
    }

    #[cfg(test)]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Runs `edit` on the selection. When it set a writing font that turned a cut into Pen, returns the
/// notice that says so.
pub fn run_edit(design: &mut Design, selection: &mut Vec<u64>, edit: &Edit) -> Option<Notice> {
    let ops = |design: &Design, ids: &[u64]| -> Vec<(u64, Operation)> { design.shapes.iter().filter(|s| ids.contains(&s.id)).map(|s| (s.id, s.operation)).collect() };
    let before = ops(design, selection);
    _ = crate::edit::run(design, selection, edit, Subject::Selection);
    let to_writing = matches!(edit, Edit::Text(TextChange::Font(font)) if font.is_writing());
    let to_pen = |&(id, op): &(u64, Operation)| op == Operation::Pen && before.iter().any(|&(was_id, was)| was_id == id && was != Operation::Pen);
    (to_writing && ops(design, selection).iter().any(to_pen)).then(|| Notice::new("Writing fonts draw with a pen", true))
}

/// Paints `slot`'s notice, clearing it when it is closed, out of time or out of date. Returns true when
/// its Undo link is clicked.
pub fn show(ctx: &Context, slot: &mut Option<Notice>, design: &Design) -> bool {
    let Some(notice) = slot else { return false };
    let now = ctx.input(|i| i.time);
    let until = *notice.until.get_or_insert(now + SHOWN_FOR_S);
    let shown = notice.design.get_or_insert_with(|| design.clone());
    if now >= until || shown != design {
        *slot = None;
        return false;
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now));
    let (mut undo, mut close) = (false, false);
    let frame = Frame::NONE
        .fill(BG_PANEL)
        .corner_radius(8)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .inner_margin(egui::Margin { left: 14, right: 6, top: 4, bottom: 4 })
        .shadow(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: SHADOW });
    Area::new(Id::new("notice")).order(Order::Foreground).anchor(Align2::CENTER_TOP, vec2(0.0, TOP_BAR_H + TOOLBAR_H + 12.0)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let font = fonts::regular(13.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(text_width(ui, &notice.message, font.clone()), ROW_H), Sense::hover());
                resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &notice.message));
                text(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, &notice.message, font, TEXT_PRIMARY);
                if notice.undo {
                    ui.add_space(8.0);
                    let font = fonts::medium(13.0);
                    let width = text_width(ui, "Undo", font.clone());
                    let (rect, resp) = ui.allocate_exact_size(vec2(width + 8.0, ROW_H), Sense::click());
                    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Link, true, "Undo"));
                    if resp.hovered() {
                        ui.painter().hline(rect.shrink2(vec2(4.0, 0.0)).x_range(), rect.center().y + 9.0, Stroke::new(1.0_f32, ACCENT_PRIMARY));
                    }
                    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, "Undo", font, ACCENT_PRIMARY);
                    undo = resp.clicked();
                }
                ui.add_space(4.0);
                close = icon_button(ui, Icon::Close, "Dismiss notice", true).clicked();
            });
        });
    });
    if undo || close {
        *slot = None;
    }
    undo
}
