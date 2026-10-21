use egui::{Align2, Area, Color32, Context, Frame, Order, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, FoilWeight, Operation};
use crate::edit::Edit;
use crate::units::Units;

use super::chrome::{text, text_width};
use super::commands::Cx;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::popover::{clicked_outside, menu_frame, popover};
use super::tokens::*;
use super::tooltip::tip;

mod align;
mod arrange;
pub(super) mod combine;
mod corner;
pub(super) mod effects;
mod flip;
pub(super) mod font_menu;
mod format;
pub(super) mod node;
pub mod pen;
mod text;

pub(super) use text::rewrap;

/// `min-width: 200px` including the 8 px row inset either side.
const MENU_W: f32 = 184.0;
const ROW_H: f32 = 32.0;
const ROW_GAP: f32 = 2.0;
const DROPDOWN_H: f32 = 32.0;
const DIVIDER_W: f32 = 9.0;
/// `moreButtonWidth`.
const MORE_W: f32 = 120.0;
const MORE_MENU_W: f32 = 160.0;

#[derive(Clone, Copy)]
enum MenuRow {
    Header(Icon, &'static str),
    Choice(Operation, &'static str),
    Foil,
    IconChoice(Icon, Operation, &'static str),
    Divider,
}

/// The operation-type menu, in template order.
const MENU: [MenuRow; 15] = [
    MenuRow::Header(Icon::Cut, "Cut"),
    MenuRow::Choice(Operation::BasicCut, "Basic"),
    MenuRow::Choice(Operation::WavyCut, "Wavy"),
    MenuRow::Choice(Operation::Perforate, "Perforate"),
    MenuRow::Divider,
    MenuRow::Header(Icon::Draw, "Draw"),
    MenuRow::Choice(Operation::Pen, "Pen"),
    MenuRow::Foil,
    MenuRow::Choice(Operation::Score, "Score"),
    MenuRow::Choice(Operation::Deboss, "Deboss"),
    MenuRow::Choice(Operation::Engrave, "Engrave"),
    MenuRow::Divider,
    MenuRow::IconChoice(Icon::Print, Operation::PrintThenCut, "Print Then Cut"),
    MenuRow::Divider,
    MenuRow::IconChoice(Icon::Guides, Operation::Guide, "Guide"),
];

/// The tools after the text group, in toolbar order. Those that do not fit move into the More menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Flip,
    Arrange,
    Align,
    Effects,
    Combine,
    Contour,
}

impl Tool {
    const ALL: [Tool; 6] = [Self::Flip, Self::Arrange, Self::Align, Self::Effects, Self::Combine, Self::Contour];

    fn icon(self) -> Icon {
        match self {
            Self::Flip => Icon::Flip,
            Self::Arrange => Icon::Arrange,
            Self::Align => Icon::Align,
            Self::Effects => Icon::Effects,
            Self::Combine => Icon::Combine,
            Self::Contour => Icon::Contour,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Flip => "Flip",
            Self::Arrange => "Arrange",
            Self::Align => "Align",
            Self::Effects => "Effects",
            Self::Combine => "Combine",
            Self::Contour => "Contour",
        }
    }

    fn anchor(self) -> Option<crate::ui::anchors::Anchor> {
        use crate::ui::anchors::Anchor;
        match self {
            Self::Align => Some(Anchor::Align),
            Self::Effects => Some(Anchor::Effects),
            Self::Combine => Some(Anchor::Combine),
            Self::Flip | Self::Arrange | Self::Contour => None,
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Flip => "Mirror the selection horizontally or vertically.",
            Self::Arrange => "Move objects forward or backward in the stack.",
            Self::Align => "Line up or evenly space two or more selected objects.",
            Self::Effects => "Add an offset outline, turn the selection into a sticker, or bend it with a warp.",
            Self::Combine => "Slice, weld or flatten the selection, or merge it with Unite, Subtract, Intersect or Exclude.",
            Self::Contour => "Hide or show individual cut lines of a shape with more than one.",
        }
    }
}

/// How many leading items of `widths` fit in `available`, leaving room for a `more_w` button when
/// any are left out.
fn fit(widths: &[f32], available: f32, more_w: f32) -> usize {
    if widths.iter().sum::<f32>() <= available {
        return widths.len();
    }
    widths
        .iter()
        .scan(0.0, |used, w| {
            *used += w;
            Some(*used)
        })
        .take_while(|used| used + more_w <= available)
        .count()
}

/// The edit strip under the top bar. Always present so the canvas never jumps; a tool is disabled while
/// none of its popover's commands can run. Text selections get the text group first. Every tool acts on
/// the whole selection. Position, size, rotation, operation and colour live in the inspector. While a
/// path's nodes are edited the node toolbar stands in.
pub fn show(ctx: &Context, design: &mut Design, selection: &mut Vec<u64>, contour_open: &mut bool, units: Units, view: &mut super::canvas::View) {
    if view.nodes.is_some() {
        return node::show(ctx, design, &mut view.nodes);
    }
    if view.corners.is_some() {
        return corner::show(ctx, design, selection, &mut view.corners, units);
    }
    let shapes: Vec<&crate::design::Shape> = selection.iter().filter_map(|id| design.shapes.iter().find(|s| s.id == *id)).collect();
    let text_common = text::TextCommon::of(&shapes);
    let primary = shapes.first();
    let cx = Cx::new(design, selection, view.align_to);
    let any = |edits: &[Edit]| edits.iter().any(|e| cx.entry(e.clone()).enabled());
    let can_flip = any(&flip::FLIP);
    let can_arrange = any(&arrange::ARRANGE);
    let can_align = align::entries(&cx).iter().any(|e| e.enabled());
    let can_combine = combine::available(&cx);
    let has_contours = primary.is_some_and(|s| crate::effects::body_contours(s).len() > 1);
    let enabled = |t: Tool| match t {
        Tool::Flip => can_flip,
        Tool::Arrange => can_arrange,
        Tool::Effects => primary.is_some(),
        Tool::Align => can_align,
        Tool::Combine => can_combine,
        Tool::Contour => has_contours,
    };
    let mut text_change = None;
    let mut picked: Option<Edit> = None;
    let align_to = &mut view.align_to;

    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin { left: 8, right: 4, top: 4, bottom: 4 });
    let panel = egui::TopBottomPanel::top("edit-toolbar").exact_height(TOOLBAR_H).frame(frame).show_separator_line(false);
    panel.show(ctx, |ui| {
        let bar = ui.max_rect().expand2(vec2(8.0, 4.0));
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if let Some(common) = &text_common {
                text_change = text::group(ui, common);
            }
            let widths: Vec<f32> = Tool::ALL.iter().enumerate().map(|(i, t)| tool_width(ui, t.label()) + if i > 0 { DIVIDER_W } else { 0.0 }).collect();
            let shown = fit(&widths, ui.available_width(), MORE_W);
            let mut open = |ui: &mut Ui, t: Tool, anchor: &Response| {
                let edit = match t {
                    Tool::Flip => flip::menu(ui, anchor, &cx),
                    Tool::Arrange => arrange::menu(ui, anchor, &cx),
                    Tool::Align => align::menu(ui, anchor, &cx, align_to),
                    Tool::Effects => primary.and_then(|p| effects::menu(ui, anchor, units, p, text_common.is_some(), &cx)),
                    Tool::Combine => combine::menu(ui, anchor, &cx),
                    Tool::Contour => {
                        if anchor.clicked() {
                            *contour_open = !*contour_open;
                        }
                        None
                    }
                };
                picked = picked.take().or(edit);
            };
            for (i, &t) in Tool::ALL[..shown].iter().enumerate() {
                if i > 0 {
                    divider(ui);
                }
                let anchor = tool(ui, t.icon(), t.label(), enabled(t), t.tip());
                if let Some(target) = t.anchor() {
                    crate::ui::anchors::mark(ui, target, anchor.rect);
                }
                if enabled(t) {
                    open(ui, t, &anchor);
                }
            }
            let hidden = &Tool::ALL[shown..];
            if hidden.is_empty() {
                return;
            }
            ui.add_space(ui.available_width() - MORE_W);
            let more = more_button(ui, hidden.len());
            let picked = popover(ui, &more, "more", MORE_MENU_W, |ui| hidden.iter().copied().find(|&t| more_row(ui, t.label(), enabled(t)).clicked()));
            // A hidden tool's popover hangs off the More button and opens as if its own button were clicked.
            for &t in hidden.iter().filter(|&&t| enabled(t)) {
                let mut anchor = ui.interact(more.rect, more.id.with(t.label()), Sense::hover());
                if picked == Some(t) {
                    anchor.flags.insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
                }
                open(ui, t, &anchor);
            }
        });
    });

    for edit in text_change.map(Edit::Text).into_iter().chain(picked) {
        if super::text_path::begin_pick(design, selection, view, &edit) {
            continue;
        }
        if let Some(notice) = super::notice::run_edit(design, selection, &edit) {
            view.notice = Some(notice);
        }
    }
}

/// Every command the Flip, Arrange, Align and Combine popovers offer, resolved for `cx`.
#[cfg(test)]
pub(super) fn entries(cx: &Cx) -> Vec<super::commands::Entry> {
    let menus = flip::FLIP.iter().chain(&arrange::ARRANGE).map(|e| cx.entry(e.clone()));
    menus.chain(align::entries(cx)).chain(combine::entries(cx).into_iter().flatten()).collect()
}

pub(super) fn operation_icon(op: Operation) -> Icon {
    match op {
        Operation::BasicCut | Operation::WavyCut | Operation::Perforate => Icon::Cut,
        Operation::PrintThenCut => Icon::Print,
        Operation::Guide => Icon::Guides,
        _ => Icon::Draw,
    }
}

pub(super) fn operation_dropdown(ui: &mut Ui, op: Option<Operation>, width: f32) -> Response {
    let label = op.map_or("Mixed", Operation::label);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, DROPDOWN_H), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label));
    let open = ui.memory(|m| m.is_popup_open(resp.id.with("menu")));
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, if resp.hovered() { ACCENT_HOVER } else { BG_PANEL });
    let edge = if open { Stroke::new(1.0_f32, ACCENT_PRIMARY) } else { Stroke::new(1.0_f32, BORDER_SUBTLE) };
    painter.rect_stroke(rect, 4.0, edge, StrokeKind::Inside);
    if let Some(op) = op {
        paint_icon(ui, operation_icon(op), Rect::from_min_size(pos2(rect.left() + 6.0, rect.center().y - 10.0), Vec2::splat(20.0)), TEXT_PRIMARY);
    }
    text(ui.painter(), pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(13.0), TEXT_PRIMARY);
    paint_icon(ui, Icon::Chevron, Rect::from_min_size(pos2(rect.right() - 26.0, rect.center().y - 10.0), Vec2::splat(20.0)), TEXT_PRIMARY);
    resp
}

/// The Cut / Draw / Print Then Cut / Guide menu, with the Foil weights in a submenu.
pub(super) fn operation_menu(ui: &Ui, anchor: &Response, current: Option<Operation>) -> Option<Operation> {
    let id = anchor.id.with("menu");
    let foil_id = id.with("foil");
    if anchor.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
        ui.data_mut(|d| d.insert_temp(foil_id, false));
    }
    if !ui.memory(|m| m.is_popup_open(id)) {
        return None;
    }
    let mut picked = None;
    let mut foil_row = None;
    let area = Area::new(id).order(Order::Foreground).fixed_pos(anchor.rect.left_bottom() + vec2(0.0, 7.0)).show(ui.ctx(), |ui| {
        menu_frame().show(ui, |ui| {
            ui.set_width(MENU_W);
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in MENU {
                match row {
                    MenuRow::Header(icon, label) => header_row(ui, icon, label),
                    MenuRow::Divider => menu_divider(ui),
                    MenuRow::Choice(op, label) => {
                        if choice_row(ui, None, label, Some(op) == current, false).clicked() {
                            picked = Some(op);
                        }
                    }
                    MenuRow::IconChoice(icon, op, label) => {
                        if choice_row(ui, Some(icon), label, Some(op) == current, false).clicked() {
                            picked = Some(op);
                        }
                    }
                    MenuRow::Foil => {
                        let resp = choice_row(ui, None, "Foil", matches!(current, Some(Operation::Foil(_))), true);
                        if resp.clicked() {
                            ui.data_mut(|d| d.insert_temp(foil_id, !d.get_temp::<bool>(foil_id).unwrap_or(false)));
                        }
                        foil_row = Some(resp.rect);
                    }
                }
            }
        });
    });
    let mut keep_clear = vec![anchor.rect, area.response.rect];
    let foil_open = ui.data(|d| d.get_temp::<bool>(foil_id).unwrap_or(false));
    if let (true, Some(row)) = (foil_open, foil_row) {
        let screen = ui.ctx().screen_rect();
        let x = if area.response.rect.right() + 162.0 > screen.right() { area.response.rect.left() - 162.0 } else { area.response.rect.right() + 2.0 };
        let at = pos2(x, row.top() - 16.0);
        let sub = Area::new(foil_id).order(Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            menu_frame().show(ui, |ui| {
                ui.set_width(160.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for weight in FoilWeight::ALL {
                    if choice_row(ui, None, weight.label(), current == Some(Operation::Foil(weight)), false).clicked() {
                        picked = Some(Operation::Foil(weight));
                    }
                }
            });
        });
        keep_clear.push(sub.response.rect);
    }
    if picked.is_some() || clicked_outside(ui.ctx(), &keep_clear) {
        ui.memory_mut(|m| m.close_popup());
    }
    picked
}

fn header_row(ui: &mut Ui, icon: Icon, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H + ROW_GAP), Sense::hover());
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 19.0, rect.center().y), Vec2::splat(24.0)), TEXT_PRIMARY);
    text(ui.painter(), pos2(rect.left() + 40.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::medium(14.0), TEXT_PRIMARY);
}

fn menu_divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 29.0), Sense::hover());
    ui.painter().hline(rect.x_range().expand(8.0), rect.center().y, Stroke::new(1.0_f32, BORDER_SUBTLE));
}

fn choice_row(ui: &mut Ui, icon: Option<Icon>, label: &str, selected: bool, submenu: bool) -> Response {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H + ROW_GAP), Sense::hover());
    let rect = slot.shrink2(vec2(0.0, ROW_GAP / 2.0));
    let resp = ui.interact(rect, ui.id().with(label), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, label));
    let ink = if selected { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    let bg = match (selected, resp.hovered()) {
        (true, _) => ACCENT_HOVER,
        (false, true) => ACCENT_HOVER,
        (false, false) => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, 4.0, bg);
    if selected {
        paint_icon(ui, Icon::Tick, Rect::from_center_size(pos2(rect.left() + 19.0, rect.center().y), Vec2::splat(20.0)), ACCENT_PRIMARY);
    }
    let text_x = match icon {
        Some(icon) => {
            paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 51.0, rect.center().y), Vec2::splat(24.0)), TEXT_MUTED);
            rect.left() + 72.0
        }
        None => rect.left() + 40.0,
    };
    text(ui.painter(), pos2(text_x, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), ink);
    if submenu {
        paint_icon(ui, Icon::SubmenuArrow, Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(8.0)), TEXT_PRIMARY);
    }
    resp
}

/// 1 x 16 px `#adadad`, 4 px either side.
fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(DIVIDER_W, 16.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
}

/// `discrete-icon-button`: 20 px icon, 14 px label, `padding: 4px 8px`, radius 8; disabled at 40 % opacity.
fn tool(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, body: &str) -> Response {
    let font = fonts::regular(14.0);
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(tool_width(ui, label), 32.0), sense);
    let resp = tip(resp, label, body, None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    paint_icon(ui, icon, Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 10.0), Vec2::splat(20.0)), ink);
    text(ui.painter(), pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, font, ink);
    resp
}

fn tool_width(ui: &Ui, label: &str) -> f32 {
    8.0 + 20.0 + 4.0 + text_width(ui, label, fonts::regular(14.0)) + 8.0
}

/// "N More": a 12 px label and 16 px chevron, centred in `moreButtonWidth`.
fn more_button(ui: &mut Ui, count: usize) -> Response {
    let label = format!("{count} More");
    let (rect, resp) = ui.allocate_exact_size(vec2(MORE_W, 32.0), Sense::click());
    let resp = tip(resp, "More", "Tools that do not fit in the toolbar.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    let open = ui.memory(|m| m.is_popup_open(resp.id.with("more")));
    if open || resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    let font = fonts::regular(12.0);
    let w = text_width(ui, &label, font.clone()) + 4.0 + 16.0;
    let left = rect.center().x - w / 2.0;
    text(ui.painter(), pos2(left, rect.center().y), Align2::LEFT_CENTER, &label, font, TEXT_PRIMARY);
    paint_icon(ui, Icon::Chevron, Rect::from_min_size(pos2(left + w - 16.0, rect.center().y - 8.0), Vec2::splat(16.0)), TEXT_PRIMARY);
    resp
}

/// A 32 px More menu row, greyed when its tool is disabled for this selection.
fn more_row(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), ink);
    resp
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn everything_fits_without_a_more_button() {
        assert_eq!(fit(&[50.0, 50.0], 100.0, 120.0), 2);
    }

    #[test]
    fn overflow_keeps_room_for_the_more_button() {
        assert_eq!(fit(&[50.0, 50.0, 50.0], 149.0, 40.0), 2);
        assert_eq!(fit(&[50.0, 50.0, 50.0], 139.0, 40.0), 1);
        assert_eq!(fit(&[50.0, 50.0], 60.0, 120.0), 0);
    }
}
