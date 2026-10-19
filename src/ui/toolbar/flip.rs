use egui::{Response, Ui};

use crate::edit::Edit;
use crate::transform::Axis;
use crate::ui::commands::Cx;
use crate::ui::popover::{command_row, popover};

pub(super) const FLIP: [Edit; 2] = [Edit::Flip(Axis::Horizontal), Edit::Flip(Axis::Vertical)];

pub fn menu(ui: &Ui, anchor: &Response, cx: &Cx) -> Option<Edit> {
    popover(ui, anchor, "menu", 160.0, |ui| FLIP.map(|e| cx.entry(e)).into_iter().find(|e| command_row(ui, e)).map(|e| e.edit().clone()))
}
