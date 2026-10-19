//! Popovers that drop below a toolbar button, and the rows, tiles and buttons that go inside them.

use std::hash::Hash;

use egui::{Align2, Area, Color32, Context, CornerRadius, Frame, Order, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::text;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::tokens::*;
use super::tooltip::tip;

/// One square in a [`tile_grid`]. `tip` is the hover body; for a disabled tile it says why.
pub struct Tile<'a> {
    pub icon: Icon,
    pub label: &'a str,
    pub enabled: bool,
    pub tip: &'a str,
}

/// Opens below `anchor` when it is clicked, `width` wide inside the frame. Closes on a click away or when
/// `body` returns `Some`, and passes that value back. `id` tells apart two popovers on one anchor.
pub fn popover<R>(ui: &Ui, anchor: &Response, id: impl Hash, width: f32, body: impl FnOnce(&mut Ui) -> Option<R>) -> Option<R> {
    let id = anchor.id.with(id);
    if anchor.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    if !ui.memory(|m| m.is_popup_open(id)) {
        return None;
    }
    // Anchors in the lower half of the window, like the Layers action bar, open upwards.
    let (pivot, at) = if anchor.rect.center().y > ui.ctx().screen_rect().center().y {
        (Align2::LEFT_BOTTOM, anchor.rect.left_top() - vec2(0.0, 7.0))
    } else {
        (Align2::LEFT_TOP, anchor.rect.left_bottom() + vec2(0.0, 7.0))
    };
    let area = Area::new(id).order(Order::Foreground).pivot(pivot).fixed_pos(at).show(ui.ctx(), |ui| {
        menu_frame().show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui)
        })
    });
    let picked = area.inner.inner;
    if picked.is_some() || (!anchor.clicked() && clicked_outside(ui.ctx(), &[anchor.rect, area.response.rect])) {
        ui.memory_mut(|m| m.close_popup());
    }
    picked
}

/// The popover heading: semibold 14 with 16 px below.
pub fn titled(ui: &mut Ui, title: &str) {
    ui.label(egui::RichText::new(title).font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
    ui.add_space(16.0);
}

/// Rows of 72 px icon tiles with a caption under each. Returns the index of the enabled tile clicked.
pub fn tile_grid(ui: &mut Ui, columns: usize, tiles: &[Tile]) -> Option<usize> {
    const GAP: f32 = 12.0;
    let mut picked = None;
    for (r, row) in tiles.chunks(columns).enumerate() {
        if r > 0 {
            ui.add_space(GAP);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (c, t) in row.iter().enumerate() {
                if tile(ui, t).clicked() && t.enabled {
                    picked = Some(r * columns + c);
                }
            }
        });
    }
    picked
}

fn tile(ui: &mut Ui, t: &Tile) -> Response {
    const SIDE: f32 = 72.0;
    const CAPTION_H: f32 = 28.0;
    let sense = if t.enabled { Sense::click() } else { Sense::hover() };
    let (cell, resp) = ui.allocate_exact_size(vec2(SIDE, SIDE + CAPTION_H), sense);
    let resp = tip(resp, t.label, t.tip, None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, t.enabled, t.label));
    let square = Rect::from_min_size(cell.min, Vec2::splat(SIDE));
    let fill = if t.enabled && resp.hovered() { ACCENT_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect(square, 12, fill, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let (ink, caption) = if t.enabled { (TEXT_PRIMARY, TEXT_MUTED) } else { (DISABLED_TEXT, DISABLED_TEXT) };
    paint_icon(ui, t.icon, Rect::from_center_size(square.center(), Vec2::splat(24.0)), ink);
    text(ui.painter(), pos2(square.center().x, square.bottom() + 6.0), Align2::CENTER_TOP, t.label, fonts::regular(13.0), caption);
    resp
}

/// A 40 px menu row: 24 px icon, 14 px label.
pub fn icon_row(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    icon_row_when(ui, icon, label, true)
}

/// An icon row for a command: greyed and unclickable while it cannot run, with the reason on hover.
/// Returns whether it was clicked.
pub fn command_icon_row(ui: &mut Ui, icon: Icon, e: &super::commands::Entry) -> bool {
    let resp = icon_row_when(ui, icon, e.label, e.enabled());
    if !e.enabled() {
        super::tooltip::tip(resp.clone(), e.label, &e.tip_body(), None);
    }
    e.enabled() && resp.clicked()
}

fn icon_row_when(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(24.0)), ink);
    text(ui.painter(), pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), ink);
    resp
}

/// A 32 px text-only menu row.
pub fn text_row(ui: &mut Ui, label: &str) -> Response {
    row(ui, label, true)
}

/// A text row for a command: greyed and unclickable while it cannot run, with the reason on hover.
/// Returns whether it was clicked.
pub fn command_row(ui: &mut Ui, e: &super::commands::Entry) -> bool {
    let resp = row(ui, e.label, e.enabled());
    if !e.enabled() {
        super::tooltip::tip(resp.clone(), e.label, &e.tip_body(), None);
    }
    e.enabled() && resp.clicked()
}

/// A greyed, unclickable text row that says `why` on hover.
pub fn disabled_row(ui: &mut Ui, label: &str, why: &str) {
    let resp = row(ui, label, false);
    super::tooltip::tip(resp, label, why, None);
}

fn row(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), ink);
    resp
}

/// A bordered 40 px button, icon left of the label, filling the available width. `name` is its
/// accessible name, for when `label` alone repeats elsewhere in the popover. A disabled button never clicks.
pub fn option_button(ui: &mut Ui, icon: Icon, label: &str, name: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), sense);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    let fill = if enabled && resp.hovered() { ACCENT_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect(rect, 8, fill, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), Vec2::splat(24.0)), ink);
    text(ui.painter(), pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), ink);
    resp
}

/// `patch-menu`: 4 px radius, 16 px vertical padding, `0 10px 20px #12121233`.
pub fn menu_frame() -> Frame {
    Frame::NONE.fill(Color32::WHITE).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(8, 16)).shadow(egui::Shadow {
        offset: [0, 10],
        blur: 20,
        spread: 0,
        color: SHADOW,
    })
}

pub fn clicked_outside(ctx: &Context, keep: &[Rect]) -> bool {
    ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !keep.iter().any(|r| r.contains(p))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn tile_grid_reports_only_enabled_tiles() {
        let tiles = [
            Tile { icon: Icon::Weld, label: "Weld", enabled: true, tip: "Join into one shape." },
            Tile { icon: Icon::Slice, label: "Slice", enabled: false, tip: "Select exactly two objects to slice." },
        ];
        let mut harness = Harness::builder().with_size(egui::vec2(200.0, 130.0)).wgpu().build_ui_state(
            |ui, picked: &mut Vec<usize>| {
                menu_frame().show(ui, |ui| picked.extend(tile_grid(ui, 4, &tiles)));
            },
            Vec::new(),
        );

        harness.get_by_label("Slice").click();
        harness.run();
        assert!(harness.state().is_empty(), "a disabled tile returns None");

        harness.get_by_label("Weld").click();
        harness.run();
        assert_eq!(harness.state(), &[0]);
    }
}
