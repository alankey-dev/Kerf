//! What was done to a layer, shown where the layer is: chips after a Layers row's name, the effect list
//! under the row and in the inspector, and the editor an entry opens beside its row. Every change is an
//! `Edit::Effect` on that one shape, so the selection is left alone and each change is one undo step.

use egui::{Align2, Area, Context, Id, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, Shape};
use crate::edit::{Edit, Subject};
use crate::effects::{EffectEdit, EntryRef, Row, RowKind, RowView, count, has_row, rows};
use crate::units::Units;

use super::chrome::{text, text_width};
use super::fonts;
use super::icons::{Icon, paint_icon, paint_icon_rotated};
use super::names::fitted;
use super::popover::{clicked_outside, menu_frame};
use super::tokens::*;
use super::tooltip::tip;

/// The second line a Layers row with effects gets for its chips.
pub(super) const CHIPS_H: f32 = 18.0;
/// Chips shown before the rest fold into "+N".
const MAX_CHIPS: usize = 3;
const CHIP: f32 = 18.0;
const LINE_H: f32 = 24.0;
const EDITOR_W: f32 = 248.0;

/// What a chip, a list line or the disclosure asked for.
#[derive(Clone, Debug)]
pub(super) enum Act {
    /// An edit of the row's shape: its eye, its removal or its place in the stack.
    Edit(Edit),
    /// Edit the row in its panel, opened beside `Rect`.
    Open(EntryRef, Rect),
    /// Show or hide the effect list under the Layers row.
    Toggle,
}

fn icon(kind: RowKind) -> Icon {
    match kind {
        RowKind::Curve => Icon::Curve,
        RowKind::Contour => Icon::Contour,
        RowKind::Warp => Icon::Warp,
        RowKind::Offset => Icon::Offset,
        RowKind::Sticker => Icon::Sticker,
    }
}

fn ink(row: &RowView, hovered: bool) -> egui::Color32 {
    match (row.active(), hovered) {
        (_, true) => TEXT_PRIMARY,
        (true, false) => TEXT_MUTED,
        (false, false) => TEXT_MUTED.gamma_multiply(0.45),
    }
}

/// Whether `shape` lists anything, so its Layers row shows chips and a disclosure.
pub(super) fn any(shape: &Shape) -> bool {
    count(shape) > 0
}

/// How tall `list` is for `shape` when lines are `gap` apart, without laying it out.
pub(super) fn list_height(shape: &Shape, gap: f32) -> f32 {
    count(shape) as f32 * (LINE_H + gap)
}

/// The chips after a Layers row's name, from `x` to at most `max_x` on the line `y`: up to three, then
/// "+N" for the rest, then the disclosure. A chip opens its entry; the disclosure shows the list.
#[expect(clippy::too_many_arguments, reason = "one row's chips, read where they are painted")]
pub(super) fn chips(ui: &mut Ui, design: &Design, shape: &Shape, units: Units, x: f32, y: f32, max_x: f32, open: bool) -> Option<Act> {
    let rows = rows(shape, &design.geometry(shape), units);
    let more_w = |n: usize| if n == 0 { 0.0 } else { text_width(ui, &format!("+{n}"), fonts::regular(11.0)) + 4.0 };
    let room = |shown: usize| x + shown as f32 * CHIP + more_w(rows.len() - shown) + CHIP <= max_x;
    let shown = (0..=rows.len().min(MAX_CHIPS)).rev().find(|&n| room(n)).unwrap_or(0);
    let mut act = None;
    let mut left = x;
    for row in &rows[..shown] {
        let rect = Rect::from_min_size(pos2(left, y - CHIP / 2.0), Vec2::splat(CHIP));
        let resp = ui.interact(rect, ui.id().with((shape.id, row.key, "chip")), Sense::click());
        let note = match (row.enabled, row.idle) {
            (false, _) => "Turned off. Click to edit it.",
            (true, true) => "Adds nothing. Click to edit it.",
            (true, false) => "Click to edit it.",
        };
        let resp = tip(resp, &row.label(), note, None);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, row.label()));
        if resp.hovered() {
            ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
        }
        paint_icon(ui, icon(row.kind), rect.shrink(2.0), ink(row, resp.hovered()));
        if resp.clicked() {
            act = Some(Act::Open(EntryRef { shape: shape.id, row: row.key }, rect));
        }
        left += CHIP;
    }
    let hidden = rows.len() - shown;
    if hidden > 0 {
        let label = format!("+{hidden}");
        text(ui.painter(), pos2(left + 2.0, y), Align2::LEFT_CENTER, &label, fonts::regular(11.0), TEXT_MUTED);
        left += more_w(hidden);
    }
    let rect = Rect::from_min_size(pos2(left, y - CHIP / 2.0), Vec2::splat(CHIP));
    super::anchors::mark(ui, super::anchors::Anchor::LayerEffects, Rect::from_min_max(pos2(x, rect.top()), rect.max));
    let verb = if open { "Hide" } else { "Show" };
    let label = format!("{verb} effects of {}", design.layer_name(shape));
    let resp = tip(ui.interact(rect, ui.id().with((shape.id, "effects-disclosure")), Sense::click()), &label, "Each effect with its eye, settings and removal.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    let turn = if open { 0.0 } else { -std::f32::consts::FRAC_PI_2 };
    paint_icon_rotated(ui, Icon::Chevron, rect.shrink(3.0), if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED }, turn);
    if resp.clicked() {
        act = Some(Act::Toggle);
    }
    act
}

/// One line per row of `shape`, indented by `indent`: an eye, the icon, name and settings, Edit and
/// remove. Stack entries drag to reorder.
pub(super) fn list(ui: &mut Ui, design: &Design, shape: &Shape, units: Units, indent: f32) -> Option<Act> {
    let rows = rows(shape, &design.geometry(shape), units);
    let entries = rows.iter().filter(|r| r.draggable).count();
    let first_entry = rows.len() - entries;
    let mut act = None;
    let mut lines: Vec<Rect> = Vec::new();
    let mut dragged = None;
    for (i, row) in rows.iter().enumerate() {
        let (full, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE_H), Sense::hover());
        let rect = full.with_min_x(full.min.x + indent);
        lines.push(rect);
        let at = EntryRef { shape: shape.id, row: row.key };
        let name = row.kind.name();
        let sense = if row.draggable { Sense::click_and_drag() } else { Sense::hover() };
        let resp = ui.interact(rect, ui.id().with((shape.id, row.key, "line")), sense);
        if row.draggable {
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Drag {name} to reorder")));
        }
        if resp.hovered() || resp.dragged() {
            ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
        }
        if resp.dragged() {
            dragged = Some(i);
        }
        if row.draggable && resp.drag_stopped() {
            dragged = Some(i);
        }
        let y = rect.center().y;
        if eye(ui, pos2(rect.left() + 12.0, y), (shape.id, row.key), row.enabled, name) {
            act = Some(Act::Edit(Edit::Effect { at, op: EffectEdit::Enable(!row.enabled) }));
        }
        paint_icon(ui, icon(row.kind), Rect::from_center_size(pos2(rect.left() + 32.0, y), Vec2::splat(14.0)), ink(row, false));
        let words = rect.with_min_x(rect.left() + 44.0).with_max_x(rect.right() - 48.0);
        let ink = if row.active() { TEXT_PRIMARY } else { DISABLED_TEXT };
        let (after, cut) = fitted(ui.painter(), pos2(words.left(), y), Align2::LEFT_CENTER, name, fonts::medium(12.0), ink, words.width());
        let rest = words.with_min_x(after.right() + 6.0);
        let (summary, cut_summary) = fitted(ui.painter(), rest.left_center(), Align2::LEFT_CENTER, &row.summary, fonts::regular(11.0), TEXT_MUTED, rest.width());
        if (cut || cut_summary) && ui.rect_contains_pointer(after.union(summary)) {
            let mut on_words = resp.clone();
            on_words.rect = after.union(summary);
            tip(on_words, &format!("{name} {}", row.summary), "", None);
        }
        if small_button(ui, pos2(rect.right() - 34.0, y), (shape.id, row.key, "edit"), Icon::Pencil, &format!("Edit {name} effect")) {
            act = Some(Act::Open(at, rect));
        }
        if small_button(ui, pos2(rect.right() - 12.0, y), (shape.id, row.key, "remove"), Icon::Close, &format!("Remove {name} effect")) {
            act = Some(Act::Edit(Edit::Effect { at, op: EffectEdit::Remove }));
        }
    }
    if let (Some(from), Some(p)) = (dragged, ui.input(|i| i.pointer.latest_pos())) {
        let stack = &lines[first_entry..];
        let before = stack.iter().position(|r| p.y < r.center().y).unwrap_or(stack.len());
        let released = ui.input(|i| i.pointer.primary_released());
        let from_stack = from - first_entry;
        if released {
            let to = if before > from_stack { before - 1 } else { before };
            if to != from_stack {
                act = Some(Act::Edit(Edit::Effect { at: EntryRef { shape: shape.id, row: rows[from].key }, op: EffectEdit::MoveTo(to) }));
            }
        } else if let Some(edge) = stack.get(before).map(|r| r.top() - 1.0).or_else(|| stack.last().map(|r| r.bottom() + 1.0)) {
            let x = stack[0].left();
            ui.painter().hline(x..=stack[0].right(), edge, Stroke::new(2.0_f32, ACCENT_PRIMARY));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }
    act
}

fn small_button(ui: &mut Ui, centre: Pos2, id: impl std::hash::Hash, icon: Icon, label: &str) -> bool {
    let r = Rect::from_center_size(centre, Vec2::splat(18.0));
    let resp = tip(ui.interact(r, ui.id().with(id), Sense::click()), label, "", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    paint_icon(ui, icon, Rect::from_center_size(centre, Vec2::splat(13.0)), if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED });
    resp.clicked()
}

/// An eye for one effect row: open while it applies, struck through while it is off.
fn eye(ui: &mut Ui, centre: Pos2, id: impl std::hash::Hash, on: bool, name: &str) -> bool {
    let r = Rect::from_center_size(centre, Vec2::splat(18.0));
    let label = if on { format!("Turn off {name} effect") } else { format!("Turn on {name} effect") };
    let resp = tip(ui.interact(r, ui.id().with((id, "eye")), Sense::click()), &label, "Off keeps the settings but leaves it out of the canvas and Make.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    let stroke = Stroke::new(1.2_f32, if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED });
    let lid = |sign: f32| -> Vec<Pos2> {
        (0..=8)
            .map(|i| {
                let x = -6.0 + 12.0 * i as f32 / 8.0;
                centre + vec2(x, sign * 3.5 * (1.0 - (x / 6.0).powi(2)))
            })
            .collect()
    };
    ui.painter().add(egui::Shape::line(lid(-1.0), stroke));
    ui.painter().add(egui::Shape::line(lid(1.0), stroke));
    ui.painter().circle_stroke(centre, 1.8, stroke);
    if !on {
        ui.painter().line_segment([centre + vec2(-7.0, -6.0), centre + vec2(7.0, 6.0)], stroke);
    }
    resp.clicked()
}

/// The inspector's Effects section for the one selected shape, when it has any.
pub(super) fn section(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>, units: Units, contour_open: &mut bool) {
    let [id] = selection[..] else { return };
    let Some(shape) = design.shapes.iter().find(|s| s.id == id).filter(|s| any(s)) else { return };
    ui.add_space(8.0);
    super::inspector::section_title(ui, "Applied effects");
    if let Some(act) = list(ui, design, shape, units, 0.0) {
        run(ui.ctx(), design, selection, id, act, contour_open);
    }
}

/// Carries out `act` on shape `id`. Opening the Contour row shows the Contour panel on that shape.
pub(super) fn run(ctx: &Context, design: &mut Design, selection: &mut Vec<u64>, id: u64, act: Act, contour_open: &mut bool) {
    match act {
        Act::Edit(edit) => _ = crate::edit::run(design, selection, &edit, Subject::Ids(&[id])),
        Act::Open(EntryRef { row: Row::Contour, .. }, _) => {
            *selection = vec![id];
            *contour_open = true;
        }
        Act::Open(at, anchor) => ctx.data_mut(|d| d.insert_temp(editor_id(), Editing { at, anchor })),
        Act::Toggle => {}
    }
}

#[derive(Clone, Copy)]
struct Editing {
    at: EntryRef,
    anchor: Rect,
}

fn editor_id() -> Id {
    Id::new("effect-editor")
}

/// The entry being edited, if its editor is open.
#[cfg(test)]
pub(super) fn editing(ctx: &Context) -> Option<EntryRef> {
    ctx.data(|d| d.get_temp::<Editing>(editor_id())).map(|e| e.at)
}

/// The editor an entry opened, beside its row, filled with the stored values; changes apply live.
/// It closes on Done, a click elsewhere, or once its entry is gone (removed, undone or expanded).
pub(super) fn editor(ctx: &Context, design: &mut Design, selection: &mut Vec<u64>, units: Units) {
    let Some(Editing { at, anchor }) = ctx.data(|d| d.get_temp::<Editing>(editor_id())) else { return };
    let Some(shape) = design.shapes.iter().find(|s| s.id == at.shape).filter(|s| has_row(s, at.row)) else {
        ctx.data_mut(|d| d.remove::<Editing>(editor_id()));
        return;
    };
    let name = rows(shape, &design.geometry(shape), units).into_iter().find(|r| r.key == at.row).map_or("Effect", |r| r.kind.name());
    let mut close = false;
    // Beside the inspector, level with the row it edits, so the list stays in view.
    let corner = pos2(anchor.left().min(ctx.screen_rect().right() - INSPECTOR_W) - 8.0, anchor.top() - 8.0);
    let area = Area::new(editor_id()).order(Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(corner).constrain(true).show(ctx, |ui| {
        menu_frame()
            .show(ui, |ui| {
                ui.set_width(EDITOR_W);
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(name).font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = super::chrome::icon_button(ui, Icon::Close, &format!("Close {name}"), true).clicked()
                    });
                });
                let edit = match at.row {
                    Row::Entry(id) => shape.effects.get(id).and_then(|e| super::toolbar::effects::edit_entry(ui, units, design, shape, at, e.effect)),
                    Row::Curve => super::toolbar::effects::curve_body(ui, units, &shape.kind).map(Edit::Curve),
                    Row::Contour => None,
                };
                close |= ui.button("Done").clicked();
                edit
            })
            .inner
    });
    if let Some(edit) = area.inner {
        _ = crate::edit::run(design, selection, &edit, Subject::Ids(&[at.shape]));
    }
    if close || clicked_outside(ctx, &[anchor, area.response.rect]) {
        ctx.data_mut(|d| d.remove::<Editing>(editor_id()));
    }
}
