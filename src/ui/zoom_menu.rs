//! The zoom pill's percentage (spec 48): a button that opens the Zoom to menu of fixed levels and the
//! three fits, each fit with its shortcut.

use egui::{Align2, Rect, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, History};
use crate::edit::AppCommand;

use super::chrome::text;
use super::commands::{Command, Cx, Entry};
use super::fonts;
use super::popover::popover;
use super::tokens::*;
use super::tooltip::tip;
use super::view_nav::ZOOM_TO_PCT;

const WIDTH: f32 = 220.0;
const FITS: [AppCommand; 3] = [AppCommand::ZoomToFit, AppCommand::ZoomToSelection, AppCommand::ZoomToAll];

/// The percentage button at `rect`, showing `pct`, and its menu. Returns the level or fit picked.
pub fn show(ui: &mut Ui, rect: Rect, pct: u32, design: &Design, selection: &[u64]) -> Option<AppCommand> {
    let value = ui.interact(rect, ui.id().with("zoom-value"), Sense::click());
    let keys = |app: AppCommand| super::commands::info(&Command::App(app)).shortcut.map(|k| ui.ctx().format_shortcut(&k)).unwrap_or_default();
    let shortcuts = format!("{} fit, {} actual size, {} selection", keys(AppCommand::ZoomToFit), keys(AppCommand::ResetZoom), keys(AppCommand::ZoomToSelection));
    let value = tip(value, "Zoom", "Choose a zoom level, or fit the mat, the selection or everything.", Some(&shortcuts));
    value.widget_info(|| {
        let mut info = WidgetInfo::labeled(WidgetType::Button, true, "Zoom");
        info.current_text_value = Some(format!("{pct}%"));
        info
    });
    if value.hovered() {
        ui.painter().rect_filled(rect, rect.height() / 2.0, ACCENT_HOVER);
    }
    // Zoom commands never read the history; the empty one only satisfies `Cx::app`.
    let cx = Cx::new(design, selection, Default::default());
    let history = History::default();
    popover(ui, &value, "zoom-to", WIDTH, |ui| {
        let mut picked = None;
        for pct in ZOOM_TO_PCT {
            if super::popover::text_row(ui, &format!("{pct}%")).clicked() {
                picked = Some(AppCommand::ZoomTo(pct));
            }
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        for fit in FITS {
            let entry = cx.app(fit.clone(), &history);
            if fit_row(ui, &entry) {
                picked = Some(fit);
            }
        }
        picked
    })
}

/// A fit's row: its label, its shortcut on the right, greyed with the reason on hover while it cannot run.
fn fit_row(ui: &mut Ui, e: &Entry) -> bool {
    let enabled = e.enabled();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, e.label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, e.label, fonts::regular(14.0), ink);
    if let Some(keys) = e.keys(ui.ctx()) {
        text(ui.painter(), pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, &keys, fonts::regular(13.0), TEXT_MUTED);
    }
    let resp = if enabled { resp } else { tip(resp, e.label, &e.tip_body(), None) };
    enabled && resp.clicked()
}
