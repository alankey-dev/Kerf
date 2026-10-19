//! The pen picker a Pen layer's Colour swatch opens: a size dropdown above a scrolling list of pen colours.

use egui::{Align2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::pens::{PEN_COLOURS, Pen, PenPick, PenSize};
use crate::ui::chrome::text;
use crate::ui::fonts;
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::popover::popover;
use crate::ui::tokens::*;

const WIDTH: f32 = 200.0;
const ROW_H: f32 = 32.0;
const SWATCH: f32 = 20.0;
const LIST_H: f32 = 280.0;

/// Opens below `anchor`. `current` is the pen every selected shape shares, if they agree. Picking a size
/// leaves the picker open so a colour can follow; picking a colour closes it.
pub fn menu(ui: &Ui, anchor: &Response, current: Option<Pen>) -> Option<PenPick> {
    if anchor.clicked() {
        ui.data_mut(|d| d.remove_temp::<bool>(sizes_open_id()));
    }
    let mut size_pick = None;
    let colour_pick = popover(ui, anchor, "pens", WIDTH, |ui| {
        size_pick = size_dropdown(ui, current.map(|p| p.size));
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .max_height(LIST_H)
            .show(ui, |ui| (0..PEN_COLOURS.len()).find(|&i| colour_row(ui, i, current.map(|p| p.colour) == Some(i)).clicked()))
            .inner
    });
    colour_pick.map(PenPick::Colour).or(size_pick.map(PenPick::Size))
}

/// A bordered field naming the size; clicking it lists the four sizes in place below.
fn size_dropdown(ui: &mut Ui, size: Option<PenSize>) -> Option<PenSize> {
    let open_id = sizes_open_id();
    let open = ui.data(|d| d.get_temp(open_id).unwrap_or(false));
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let label = size.map_or("Mixed sizes", PenSize::label);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, "Pen size"));
    let fill = if resp.hovered() { ACCENT_HOVER } else { BG_PANEL };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    text(ui.painter(), pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(13.0), TEXT_PRIMARY);
    let chevron = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(14.0));
    paint_icon(ui, Icon::Chevron, chevron, TEXT_MUTED);
    let mut picked = None;
    if open {
        for s in PenSize::ALL {
            if size_row(ui, s, size == Some(s)).clicked() {
                picked = Some(s);
            }
        }
    }
    let open = picked.is_none() && open != resp.clicked();
    ui.data_mut(|d| d.insert_temp(open_id, open));
    picked
}

fn sizes_open_id() -> egui::Id {
    egui::Id::new("pen-sizes-open")
}

fn size_row(ui: &mut Ui, size: PenSize, current: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, current, size.label()));
    if current || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    text(ui.painter(), pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, size.label(), fonts::regular(13.0), TEXT_PRIMARY);
    resp
}

fn colour_row(ui: &mut Ui, index: usize, current: bool) -> Response {
    let (name, colour) = PEN_COLOURS[index];
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, current, name));
    if current || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let chip = Rect::from_center_size(pos2(rect.left() + 8.0 + SWATCH / 2.0, rect.center().y), Vec2::splat(SWATCH));
    ui.painter().rect(chip, 4.0, colour, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let font = if current { fonts::semibold(14.0) } else { fonts::regular(14.0) };
    text(ui.painter(), pos2(chip.right() + 12.0, rect.center().y), Align2::LEFT_CENTER, name, font, TEXT_PRIMARY);
    resp
}
