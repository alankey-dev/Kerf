use egui::{Response, Ui};

use crate::design::Arrange;
use crate::edit::Edit;
use crate::ui::commands::Cx;
use crate::ui::popover::{command_row, popover};

pub(super) const ARRANGE: [Edit; 4] =
    [Edit::Arrange(Arrange::ToFront), Edit::Arrange(Arrange::Forward), Edit::Arrange(Arrange::Backward), Edit::Arrange(Arrange::ToBack)];

pub fn menu(ui: &Ui, anchor: &Response, cx: &Cx) -> Option<Edit> {
    popover(ui, anchor, "menu", 180.0, |ui| ARRANGE.map(|e| cx.entry(e)).into_iter().find(|e| command_row(ui, e)).map(|e| e.edit().clone()))
}
