//! The bar shown after an SVG import. It reads out what came in and what was lost from the pure
//! [`ImportReport`], and when strokes arrived it offers Treat hairlines as (Cut, Pen or Score) and
//! Outline strokes, each applied to every imported stroke in one undo step. Unlike the transient
//! [`super::notice`] bar it stays until dismissed, since the user acts on it.

use egui::{Align2, Area, Context, Frame, Id, Order, Sense, Stroke, WidgetInfo, WidgetType, vec2};

use super::chrome::{icon_button, text, text_width};
use super::fonts;
use super::icons::Icon;
use super::tokens::*;
use crate::design::Operation;
use crate::svg_import::ImportReport;

const ROW_H: f32 = 28.0;

/// The label on the Treat hairlines as row, also the test handle.
pub const HAIRLINE_LABEL: &str = "Treat hairlines as";
/// The label on the Outline strokes option, also the test handle.
pub const OUTLINE_LABEL: &str = "Outline strokes";

pub struct ImportNotice {
    message: String,
    /// The imported stroke leaves, by id and stroke width in inches. The Treat hairlines as choice
    /// re-operations them together; Outline strokes expands each to a filled ring of its own width.
    pub strokes: Vec<(u64, f32)>,
    /// The operation the strokes currently carry, so the row shows which option is live.
    choice: Operation,
}

/// What the user picked in the notice, for the caller to apply to [`ImportNotice::strokes`] before it
/// snapshots the design.
pub enum NoticeAction {
    /// Re-operation every imported stroke.
    Operation(Operation),
    /// Expand every imported stroke to a filled ring.
    Outline,
}

impl ImportNotice {
    /// Builds the notice from a report and the imported stroke leaves. The default Treat hairlines as
    /// is Cut when every stroke is a hairline, else Pen.
    pub fn new(report: &ImportReport, strokes: Vec<(u64, f32)>) -> Self {
        let choice = if report.all_hairline() { Operation::BasicCut } else { Operation::Pen };
        Self { message: report.summary(), strokes, choice }
    }

    /// The ids of the imported stroke shapes.
    pub fn stroke_ids(&self) -> Vec<u64> {
        self.strokes.iter().map(|(id, _)| *id).collect()
    }
}

/// The three operations the Treat hairlines as row offers, with their labels.
const CHOICES: [(Operation, &str); 3] = [(Operation::BasicCut, "Cut"), (Operation::Pen, "Pen"), (Operation::Score, "Score")];

/// Paints the import notice. Returns what the user just picked, if anything.
pub fn show(ctx: &Context, slot: &mut Option<ImportNotice>) -> Option<NoticeAction> {
    let Some(notice) = slot else { return None };
    let mut action = None;
    let mut close = false;
    let frame = Frame::NONE
        .fill(BG_PANEL)
        .corner_radius(8)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .inner_margin(egui::Margin { left: 14, right: 6, top: 4, bottom: 4 })
        .shadow(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: SHADOW });
    Area::new(Id::new("import_notice")).order(Order::Foreground).anchor(Align2::CENTER_TOP, vec2(0.0, TOP_BAR_H + TOOLBAR_H + 12.0)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let font = fonts::regular(13.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(text_width(ui, &notice.message, font.clone()), ROW_H), Sense::hover());
                resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &notice.message));
                text(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, &notice.message, font, TEXT_PRIMARY);

                if !notice.strokes.is_empty() {
                    ui.add_space(12.0);
                    let title = fonts::regular(13.0);
                    let (rect, resp) = ui.allocate_exact_size(vec2(text_width(ui, HAIRLINE_LABEL, title.clone()), ROW_H), Sense::hover());
                    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, HAIRLINE_LABEL));
                    text(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, HAIRLINE_LABEL, title, TEXT_MUTED);
                    for (op, label) in CHOICES {
                        ui.add_space(6.0);
                        if option(ui, label, &format!("{HAIRLINE_LABEL} {label}"), op == notice.choice) {
                            action = Some(NoticeAction::Operation(op));
                        }
                    }
                    ui.add_space(14.0);
                    if option(ui, OUTLINE_LABEL, OUTLINE_LABEL, false) {
                        action = Some(NoticeAction::Outline);
                    }
                }

                ui.add_space(4.0);
                close = icon_button(ui, Icon::Close, "Dismiss notice", true).clicked();
            });
        });
    });
    if let Some(NoticeAction::Operation(op)) = &action {
        notice.choice = *op;
    }
    if close {
        *slot = None;
    }
    action
}

/// One pill-sized choice. The live one is drawn in the accent colour and underlined on hover. The
/// `aria` label is distinct from the visible word so it never clashes with a tool of the same name.
fn option(ui: &mut egui::Ui, label: &str, aria: &str, selected: bool) -> bool {
    let font = if selected { fonts::medium(13.0) } else { fonts::regular(13.0) };
    let width = text_width(ui, label, font.clone());
    let (rect, resp) = ui.allocate_exact_size(vec2(width + 8.0, ROW_H), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, aria));
    let colour = if selected { ACCENT_PRIMARY } else { TEXT_MUTED };
    if resp.hovered() || selected {
        ui.painter().hline(rect.shrink2(vec2(4.0, 0.0)).x_range(), rect.center().y + 9.0, Stroke::new(1.0_f32, colour));
    }
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, font, colour);
    resp.clicked()
}
