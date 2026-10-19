//! The tab strip above the top bar: Home, then one chip per open document with its title, a dot while
//! it has unsaved changes and a close button. It only reports clicks as commands; `Kerf::run` acts.

use egui::{Align2, Context, CornerRadius, Frame, Key, KeyboardShortcut, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::edit::AppCommand;

use super::chrome::{text, text_width};
use super::commands::Command;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::tokens::*;
use super::tooltip::tip;

const STRIP_H: f32 = 34.0;
const CHIP_H: f32 = 28.0;
const TITLE_MAX_W: f32 = 160.0;
const CLOSE: f32 = 18.0;
const DIGITS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

/// One open document as the strip shows it.
pub(super) struct Chip<'a> {
    pub(super) title: &'a str,
    pub(super) edited: bool,
}

/// Shows the strip. `active` is the active document's chip, or `None` while Home shows.
pub(super) fn show(ctx: &Context, chips: &[Chip], active: Option<usize>) -> Option<Command> {
    let frame = Frame::NONE.fill(BG_CANVAS_VOID).inner_margin(egui::Margin { left: TRAFFIC_LIGHT_PAD as i8, right: 10, top: 0, bottom: 0 });
    let panel = egui::TopBottomPanel::top("tab-bar").exact_height(STRIP_H).frame(frame).show_separator_line(false);
    panel
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(full.x_range().expand(TRAFFIC_LIGHT_PAD), full.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if home_chip(ui, active.is_none()) {
                    picked = Some(AppCommand::Home);
                }
                for (i, chip) in chips.iter().enumerate() {
                    picked = picked.take().or(doc_chip(ui, i, chip, active == Some(i)));
                }
                super::anchors::mark(ui, super::anchors::Anchor::TabStrip, ui.min_rect());
            });
            picked.map(Command::App)
        })
        .inner
}

/// A chip's body: the raised white tab when active, an accent wash on hover. Returns its rect and response.
fn chip(ui: &mut Ui, width: f32, active: bool, id: egui::Id) -> (Rect, egui::Response) {
    let (slot, _) = ui.allocate_exact_size(vec2(width, STRIP_H), Sense::hover());
    let rect = Rect::from_min_size(pos2(slot.left(), slot.bottom() - CHIP_H), vec2(width, CHIP_H));
    let resp = ui.interact(rect, id, Sense::click());
    let round = CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 };
    if active {
        ui.painter().rect(rect, round, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
        ui.painter().hline(rect.x_range().shrink(1.0), rect.bottom() - 0.5, Stroke::new(1.0_f32, BG_PANEL));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 2.0)), 6, ACCENT_HOVER);
    }
    (rect, resp)
}

fn home_chip(ui: &mut Ui, active: bool) -> bool {
    let font = fonts::medium(13.0);
    let (rect, resp) = chip(ui, text_width(ui, "Home", font.clone()) + 28.0, active, ui.id().with("home-chip"));
    let resp = tip(resp, "Home", "Your saved projects.", None);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, "Home tab"));
    let ink = if active { TEXT_PRIMARY } else { TEXT_MUTED };
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, "Home", font, ink);
    resp.clicked()
}

fn doc_chip(ui: &mut Ui, i: usize, chip_: &Chip, active: bool) -> Option<AppCommand> {
    let font = fonts::medium(13.0);
    let title_w = text_width(ui, chip_.title, font.clone()).min(TITLE_MAX_W);
    let (rect, resp) = chip(ui, 12.0 + title_w + 8.0 + 6.0 + 6.0 + CLOSE + 6.0, active, ui.id().with(("tab-chip", i)));
    let shortcut = DIGITS.get(i).map(|&k| ui.ctx().format_shortcut(&KeyboardShortcut::new(super::commands::TAB_MODIFIERS, k)));
    let resp = tip(resp, chip_.title, if chip_.edited { "Unsaved changes, kept by autosave." } else { "" }, shortcut.as_deref());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, format!("Tab {}", chip_.title)));
    let ink = if active { TEXT_PRIMARY } else { TEXT_MUTED };
    let title_rect = Rect::from_min_size(pos2(rect.left() + 12.0, rect.top()), vec2(title_w, rect.height()));
    text(&ui.painter().with_clip_rect(title_rect), title_rect.left_center(), Align2::LEFT_CENTER, chip_.title, font, ink);
    let dot = pos2(title_rect.right() + 8.0 + 3.0, rect.center().y);
    if chip_.edited {
        ui.painter().circle_filled(dot, 3.0, ACCENT_PRIMARY);
    }
    let close_rect = Rect::from_center_size(pos2(rect.right() - 6.0 - CLOSE / 2.0, rect.center().y), Vec2::splat(CLOSE));
    let close = ui.interact(close_rect, ui.id().with(("tab-close", i)), Sense::click());
    let keys = active.then(|| ui.ctx().format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::W)));
    let close = tip(close, "Close tab", "Its unsaved changes are kept and offered when you open it again.", keys.as_deref());
    close.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Close {}", chip_.title)));
    if close.hovered() {
        ui.painter().rect_filled(close_rect, 4, ACCENT_HOVER);
    }
    if active || resp.hovered() || close.hovered() {
        paint_icon(ui, Icon::Close, close_rect.shrink(3.0), if close.hovered() { TEXT_PRIMARY } else { TEXT_MUTED });
    }
    if close.clicked() { Some(AppCommand::CloseTab(Some(i))) } else { resp.clicked().then_some(AppCommand::Tab(i)) }
}
