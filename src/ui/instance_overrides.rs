//! The inspector's Parts section for one selected instance (spec 31): a row per part with its colour or
//! pattern, operation and, on a Pen part, its pen. Changing one overrides it on this instance only; a dot marks
//! an overridden part. Right-click a row to reset that part; Reset overrides clears them all.

use egui::{Align2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::components::{PartChange, expand, master_shapes};
use crate::design::{Design, Operation, Shape, ShapeKind};
use crate::edit::{Edit, Subject};
use crate::units::Units;

use super::chrome::text;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::inspector::section_title;
use super::pattern_panel::{Asked, Target, paint_chip, picker};
use super::popover::{disabled_row, text_row};
use super::tokens::*;
use super::toolbar::{operation_icon, operation_menu, pen};
use super::tooltip::tip;

const ROW_H: f32 = 28.0;
const SWATCH: f32 = 18.0;
const OP_W: f32 = 104.0;

/// One part as its row shows it: the master shape it comes from, and its look on this instance.
struct PartRow {
    master: u64,
    part: Shape,
    overridden: bool,
}

/// The section, when the selection is one instance. Returns what a part's pattern picker needs the app
/// for: storing a bundled pattern, or picking an image.
pub fn section(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>, units: Units, palette: &mut super::colour_picker::Palette) -> Option<(Asked, Target)> {
    let (rows, any) = part_rows(design, selection)?;
    ui.add_space(12.0);
    section_title(ui, "Parts");
    let (mut edit, mut asked) = (None, None);
    for (n, row) in rows.iter().enumerate() {
        let (e, a) = part_row(ui, n + 1, row, units, design, palette);
        edit = e.or(edit);
        asked = a.or(asked);
    }
    ui.add_space(4.0);
    if reset_button(ui, any) {
        edit = Some(Edit::ResetOverrides);
    }
    if let Some(edit) = edit {
        _ = crate::edit::run(design, selection, &edit, Subject::Selection);
    }
    asked
}

/// The parts of the one selected instance, front first as Layers lists them, and whether it overrides any.
fn part_rows(design: &Design, selection: &[u64]) -> Option<(Vec<PartRow>, bool)> {
    let [id] = selection[..] else { return None };
    let instance = design.shapes.iter().find(|s| s.id == id)?;
    let ShapeKind::Instance { component, overrides } = &instance.kind else { return None };
    let master = master_shapes(design, *component)?;
    let parts = expand(design, instance)?;
    let rows =
        master.iter().zip(parts).filter(|(m, _)| m.compound().is_none()).map(|(m, part)| PartRow { master: m.id, part, overridden: overrides.contains_key(&m.id) });
    Some((rows.rev().collect(), !overrides.is_empty()))
}

/// Part `n`'s row: swatch, name, override dot and operation. Returns the edit it asked for, and what its
/// pattern picker needs the app for.
fn part_row(ui: &mut Ui, n: usize, row: &PartRow, units: Units, design: &Design, palette: &mut super::colour_picker::Palette) -> (Option<Edit>, Option<(Asked, Target)>) {
    let PartRow { master, part, overridden } = row;
    let name = part.kind.label();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, format!("Part {n} {name}")));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let y = rect.center().y;
    let swatch = swatch(ui, Rect::from_center_size(pos2(rect.left() + 4.0 + SWATCH / 2.0, y), Vec2::splat(SWATCH)), n, part);
    let ink = if part.hidden { DISABLED_TEXT } else { TEXT_PRIMARY };
    text(ui.painter(), pos2(rect.left() + SWATCH + 12.0, y), Align2::LEFT_CENTER, name, fonts::regular(13.0), ink);
    let op_rect = Rect::from_min_size(pos2(rect.right() - OP_W, y - 11.0), vec2(OP_W, 22.0));
    if *overridden {
        let dot = ui.interact(Rect::from_center_size(pos2(op_rect.left() - 10.0, y), Vec2::splat(10.0)), resp.id.with("dot"), Sense::hover());
        let dot = tip(dot, "Overridden", "This instance changes this part. Right-click the row to reset it.", None);
        dot.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, format!("Part {n} overridden")));
        ui.painter().circle_filled(dot.rect.center(), 3.0, ACCENT_PRIMARY);
    }
    let op = operation_button(ui, op_rect, n, part.operation);
    let mut edit = operation_menu(ui, &op, Some(part.operation)).map(|op| Edit::Override { part: *master, change: PartChange::Operation(op) });
    let mut asked = None;
    let colour = match part.operation {
        Operation::Pen => pen::menu(ui, &swatch, Some(part.pen)).map(|pick| {
            let mut picked = part.clone();
            picked.pick_pen(pick);
            PartChange::Pen(picked.pen)
        }),
        _ => {
            let printable = part.takes_pattern();
            let picked = picker(ui, &swatch, Some(part.fill), printable, part.pattern.as_ref().filter(|_| printable), units, design, palette);
            asked = picked.asked;
            match picked.edit {
                Some(Edit::Pattern(fill)) => Some(PartChange::Pattern(fill)),
                _ => picked.colour.map(PartChange::Fill),
            }
        }
    };
    edit = edit.or(colour.map(|change| Edit::Override { part: *master, change }));
    resp.context_menu(|ui| {
        ui.set_width(180.0);
        if !overridden {
            disabled_row(ui, "Reset this part", "This part already matches its master.");
        } else if text_row(ui, "Reset this part").clicked() {
            edit = Some(Edit::ResetPart(*master));
            ui.close_menu();
        }
    });
    (edit, asked.map(|a| (a, Target::Part(*master))))
}

/// The part's colour chip; on a Pen part it opens the pen picker, otherwise the colour picker.
fn swatch(ui: &mut Ui, rect: Rect, n: usize, part: &Shape) -> Response {
    let resp = ui.interact(rect, ui.id().with(("part-swatch", n)), Sense::click());
    let body = if part.operation == Operation::Pen { "Pick this part's pen for this instance only." } else { "Pick this part's colour for this instance only." };
    let resp = tip(resp, "Part colour", body, None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Part {n} colour")));
    let edge = if resp.hovered() { ACCENT_PRIMARY } else { BORDER_SUBTLE };
    match part.pattern.as_ref().filter(|_| part.takes_pattern()) {
        Some(fill) => paint_chip(ui, rect, fill),
        None => _ = ui.painter().rect(rect, 4.0, part.fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside),
    }
    resp
}

/// A compact operation dropdown: icon, name and chevron.
fn operation_button(ui: &mut Ui, rect: Rect, n: usize, op: Operation) -> Response {
    let resp = ui.interact(rect, ui.id().with(("part-operation", n)), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, format!("Part {n} operation")));
    let fill = if resp.hovered() { ACCENT_HOVER } else { BG_PANEL };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    paint_icon(ui, operation_icon(op), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(14.0)), TEXT_MUTED);
    text(ui.painter(), pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, op.label(), fonts::regular(12.0), TEXT_PRIMARY);
    paint_icon(ui, Icon::Chevron, Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), Vec2::splat(14.0)), TEXT_MUTED);
    resp
}

fn reset_button(ui: &mut Ui, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let resp = tip(resp, "Reset overrides", "Make every part of this instance match its master again.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, "Reset overrides"));
    let (fill, ink) = match (enabled, resp.hovered()) {
        (false, _) => (BG_PANEL, DISABLED_TEXT),
        (true, true) => (ACCENT_HOVER, ACCENT_PRIMARY),
        (true, false) => (BG_PANEL, ACCENT_PRIMARY),
    };
    ui.painter().rect(rect, 14.0, fill, Stroke::new(1.0_f32, if enabled { ACCENT_PRIMARY } else { BORDER_SUBTLE }), StrokeKind::Inside);
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, "Reset overrides", fonts::medium(13.0), ink);
    resp.clicked()
}
