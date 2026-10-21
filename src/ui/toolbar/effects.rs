//! The Effects popover, and the panel bodies the Layers effect list reuses to edit an entry after the fact.
//!
//! A panel targets either the selection or one stack entry. On the selection, Warp edits the first Warp
//! entry live and Offset and Sticker hold a draft until Apply, which adds one entry; with one object
//! selected the panel then edits that entry by id. Nothing a panel does on a frame appends an entry, and
//! typed fields commit on Enter or focus loss, so each committed change is one undo step.

use egui::{Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2};

use crate::design::{Shape, ShapeKind};
use crate::edit::{Edit, TextPathEdit};
use crate::effects::{Corners, Direction, Effect, EffectEdit, EntryRef, Offset, Reach, Row, Sticker, TooThin};
use crate::geometry::{DEFAULT_MITER_LIMIT, Join};
use crate::ui::commands::Cx;
use crate::ui::fonts;
use crate::ui::icons::{Icon, paint_icon_rotated};
use crate::ui::popover::{command_icon_row, icon_row, popover};
use crate::ui::tokens::{ACCENT_HOVER, ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY, WARNING_TEXT};
use crate::ui::tooltip::tip;
use crate::units::{FIELD_W, Units, length_field, number_field};
use crate::warp::{Warp, WarpStyle};

const WIDTH: f32 = 248.0;
const TILE: f32 = 40.0;

const NEW_OFFSET: Offset = Offset { distance_in: 0.25, direction: Direction::Outset, join: Join::Round, style: None };
const NEW_STICKER: Sticker = Sticker { border_in: 0.125, corners: Corners::Round };
const NEW_WARP: Warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };

/// Which body the popover shows. Every opening starts at the menu.
#[derive(Clone, Copy)]
enum Panel {
    Menu,
    Offset,
    Sticker,
    Warp,
    Curve,
}

/// The Effects popover on the selection in `cx`. `primary` is the first selected shape; when it is the
/// only one, an Offset or Sticker it applies stays open on the new entry. Curve and Convert to Paths are
/// offered when `all_text`. Offset and Sticker are greyed when the model's check rules them out.
pub fn menu(ui: &Ui, anchor: &Response, units: Units, primary: &Shape, all_text: bool, cx: &Cx) -> Option<Edit> {
    let single = cx.on.len() == 1;
    let panel_id = anchor.id.with("panel");
    let target_id = anchor.id.with("target");
    if anchor.clicked() {
        ui.data_mut(|d| {
            d.insert_temp(panel_id, Panel::Menu);
            d.remove::<EntryRef>(target_id);
        });
    }
    let mut live = None;
    let mut convert = false;
    let mut on_path = false;
    let applied = popover(ui, anchor, "effects", WIDTH, |ui| {
        let panel = ui.data(|d| d.get_temp(panel_id)).unwrap_or(Panel::Menu);
        let picked = match panel {
            Panel::Menu => {
                ui.spacing_mut().item_spacing.y = 4.0;
                let mut picked = None;
                let offset = Edit::AddEffect { effect: Effect::Offset(NEW_OFFSET), reach: Reach::Together };
                let sticker = Edit::AddEffect { effect: Effect::Sticker(NEW_STICKER), reach: Reach::Together };
                for (icon, edit, panel) in [(Icon::Offset, offset, Panel::Offset), (Icon::Sticker, sticker, Panel::Sticker)] {
                    if command_icon_row(ui, icon, &cx.entry(edit)) {
                        picked = Some(panel);
                    }
                }
                let curve = all_text.then_some((Icon::Curve, "Curve", Panel::Curve));
                for (icon, label, panel) in [(Icon::Warp, "Warp", Panel::Warp)].into_iter().chain(curve) {
                    if icon_row(ui, icon, label).clicked() {
                        picked = Some(panel);
                    }
                }
                if all_text && command_icon_row(ui, Icon::Pen, &cx.entry(Edit::ConvertToPaths)) {
                    convert = true;
                }
                if command_icon_row(ui, Icon::Curve, &cx.entry(Edit::TextPath(TextPathEdit::Attach))) {
                    on_path = true;
                }
                picked
            }
            Panel::Offset => title_row(ui, "Offset"),
            Panel::Sticker => title_row(ui, "Sticker"),
            Panel::Warp => title_row(ui, "Warp"),
            Panel::Curve if matches!(&primary.kind, ShapeKind::Text { path: Some(_), .. }) => title_row(ui, "Path"),
            Panel::Curve => title_row(ui, "Curve"),
        };
        if convert {
            return Some(Edit::ConvertToPaths);
        }
        if on_path {
            return Some(Edit::TextPath(TextPathEdit::Attach));
        }
        if let Some(next) = picked {
            ui.data_mut(|d| {
                d.insert_temp(panel_id, next);
                d.remove::<EntryRef>(target_id);
            });
            ui.ctx().request_repaint();
            return None;
        }
        ui.spacing_mut().item_spacing.y = 8.0;
        let target = ui.data(|d| d.get_temp::<EntryRef>(target_id));
        let entry = target.and_then(|at| match at.row {
            Row::Entry(id) if at.shape == primary.id => primary.effects.get(id).map(|e| (at, e.effect)),
            _ => None,
        });
        if target.is_some() && entry.is_none() {
            ui.memory_mut(|m| m.close_popup());
            return None;
        }
        let (applied, edited) = match (panel, entry) {
            (Panel::Menu, _) => (None, None),
            (Panel::Offset | Panel::Sticker, Some((at, effect))) => (done(ui), edit_entry(ui, units, cx.design, primary, at, effect)),
            (Panel::Offset, None) => {
                let draft_id = anchor.id.with("offset");
                let (mut o, mut weld) = ui.data_mut(|d| *d.get_temp_mut_or(draft_id, (NEW_OFFSET, true)));
                offset_body(ui, units, &mut o, None);
                ui.checkbox(&mut weld, "Weld offsets");
                ui.data_mut(|d| d.insert_temp(draft_id, (o, weld)));
                let reach = if weld { Reach::Together } else { Reach::EachShape };
                apply(ui, primary, single, target_id, Edit::AddEffect { effect: Effect::Offset(o), reach })
            }
            (Panel::Sticker, None) => {
                let draft_id = anchor.id.with("sticker");
                let mut s = ui.data_mut(|d| *d.get_temp_mut_or(draft_id, NEW_STICKER));
                sticker_body(ui, units, &mut s);
                ui.data_mut(|d| d.insert_temp(draft_id, s));
                apply(ui, primary, single, target_id, Edit::AddEffect { effect: Effect::Sticker(s), reach: Reach::Together })
            }
            (Panel::Warp, _) if !primary.can_warp() => {
                ui.label(egui::RichText::new(primary.warp_blocked().reason()).font(fonts::regular(13.0)).color(TEXT_MUTED));
                (None, None)
            }
            (Panel::Warp, _) => warp_panel(ui, anchor, primary.effects.first_warp()),
            (Panel::Curve, _) if matches!(&primary.kind, ShapeKind::Text { path: Some(_), .. }) => crate::ui::text_path::path_panel(ui, units, &primary.kind),
            (Panel::Curve, _) => curve_panel(ui, units, &primary.kind),
        };
        live = edited;
        applied
    });
    applied.or(live)
}

/// Apply on a draft. With one object it adds the entry live and the panel follows that entry; over
/// several it applies and closes.
fn apply(ui: &mut Ui, primary: &Shape, single: bool, target_id: egui::Id, add: Edit) -> (Option<Edit>, Option<Edit>) {
    if !ui.button("Apply").clicked() {
        return (None, None);
    }
    if !single {
        return (Some(add), None);
    }
    let at = EntryRef { shape: primary.id, row: Row::Entry(primary.effects.next_id()) };
    ui.data_mut(|d| d.insert_temp(target_id, at));
    (None, Some(add))
}

/// Done closes a panel that is editing an entry live.
fn done(ui: &mut Ui) -> Option<Edit> {
    if ui.button("Done").clicked() {
        ui.memory_mut(|m| m.close_popup());
    }
    None
}

/// The body for one existing Offset, Sticker or Warp entry of `shape`, filled with its stored values. A
/// committed change replaces the entry; a drag replaces it every frame and folds into one undo step.
pub(in crate::ui) fn edit_entry(ui: &mut Ui, units: Units, design: &crate::design::Design, shape: &Shape, at: EntryRef, effect: Effect) -> Option<Edit> {
    let mut edited = effect;
    let changed = match &mut edited {
        Effect::Offset(o) => {
            let thin = match at.row {
                Row::Entry(id) => design.too_thin(shape, id),
                _ => None,
            };
            offset_body(ui, units, o, thin)
        }
        Effect::Sticker(s) => sticker_body(ui, units, s),
        Effect::Warp(w) => warp_body(ui, w),
    };
    (changed && edited != effect).then_some(Edit::Effect { at, op: EffectEdit::Replace(edited) })
}

/// The back chevron and the panel's title. Returns the menu when Back is clicked.
fn title_row(ui: &mut Ui, title: &str) -> Option<Panel> {
    let back = ui
        .horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
            let resp = tip(resp, "Back", "Return to the Effects menu.", None);
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Back"));
            if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
            }
            paint_icon_rotated(ui, Icon::Chevron, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), TEXT_PRIMARY, std::f32::consts::FRAC_PI_2);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(title).font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
            resp.clicked()
        })
        .inner;
    ui.add_space(12.0);
    back.then_some(Panel::Menu)
}

/// Direction, distance and corners, and what an inset is too thin for. Returns whether a change was
/// committed.
fn offset_body(ui: &mut Ui, units: Units, o: &mut Offset, thin: Option<TooThin>) -> bool {
    let before = o.direction;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut o.direction, Direction::Outset, "Outset");
        ui.selectable_value(&mut o.direction, Direction::Inset, "Inset");
    });
    let typed = ui
        .horizontal(|ui| {
            ui.label("Distance");
            length_field(ui, "Offset distance", Some(o.distance_in), units, FIELD_W).is_some_and(|t| t.set(&mut o.distance_in, 0.0..=2.0))
        })
        .inner;
    let joined = join(ui, &mut o.join);
    if thin.is_some() {
        ui.label(egui::RichText::new("Some parts are too thin for this inset").font(fonts::regular(13.0)).color(WARNING_TEXT));
    }
    typed | joined | (o.direction != before)
}

/// Border and corners. Returns whether a change was committed.
pub(in crate::ui) fn sticker_body(ui: &mut Ui, units: Units, s: &mut Sticker) -> bool {
    let typed = ui
        .horizontal(|ui| {
            ui.label("Border");
            length_field(ui, "Sticker border", Some(s.border_in), units, FIELD_W).is_some_and(|t| t.set(&mut s.border_in, 0.0..=1.0))
        })
        .inner;
    typed | corners(ui, &mut s.corners)
}

/// Round, Miter or Bevel, and the miter limit while Miter is picked. Returns whether anything changed.
fn join(ui: &mut Ui, join: &mut Join) -> bool {
    let before = *join;
    let limit = match *join {
        Join::Miter { limit } => limit,
        _ => DEFAULT_MITER_LIMIT,
    };
    ui.horizontal(|ui| {
        ui.selectable_value(join, Join::Round, "Round");
        ui.selectable_value(join, Join::Miter { limit }, "Miter");
        ui.selectable_value(join, Join::Bevel, "Bevel");
    });
    if let Join::Miter { limit } = join {
        ui.horizontal(|ui| {
            ui.label("Miter limit");
            _ = number_field(ui, "Miter limit", Some(*limit), "", FIELD_W).is_some_and(|t| t.set(limit, 1.0..=10.0));
        });
    }
    *join != before
}

fn corners(ui: &mut Ui, corners: &mut Corners) -> bool {
    let before = *corners;
    ui.horizontal(|ui| {
        ui.selectable_value(corners, Corners::Round, "Round");
        ui.selectable_value(corners, Corners::Sharp, "Sharp");
    });
    *corners != before
}

/// The Warp panel on the selection: the first Warp's values, or the pending ones, edited live. Returns
/// what Apply or Remove Warp asked for, and any live edit.
fn warp_panel(ui: &mut Ui, anchor: &Response, current: Option<Warp>) -> (Option<Edit>, Option<Edit>) {
    let id = anchor.id.with("warp");
    let mut w = current.unwrap_or_else(|| ui.data(|d| d.get_temp(id)).unwrap_or(NEW_WARP));
    let live = warp_body(ui, &mut w).then_some(Edit::Warp(Some(w)));
    ui.data_mut(|d| d.insert_temp(id, w));
    let applied = ui
        .horizontal(|ui| {
            if ui.button("Apply").clicked() {
                return Some(Edit::Warp(Some(w)));
            }
            let remove = current.is_some() && ui.button("Remove Warp").clicked();
            remove.then_some(Edit::Warp(None))
        })
        .inner;
    (applied, live)
}

/// The style tiles, Bend and orientation. Returns whether anything changed; the Bend field commits on
/// Enter or focus loss.
pub(in crate::ui) fn warp_body(ui: &mut Ui, w: &mut Warp) -> bool {
    let before = *w;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for style in WarpStyle::ALL {
            if style_tile(ui, style, w.style == style).clicked() {
                w.style = style;
            }
        }
    });
    let mut pct = (w.bend * 100.0).round();
    let start = pct;
    ui.horizontal(|ui| {
        ui.label("Bend");
        let slider = ui.add(egui::Slider::new(&mut pct, -100.0..=100.0).show_value(false));
        slider.widget_info(|| WidgetInfo { value: Some(pct.into()), ..WidgetInfo::labeled(WidgetType::Slider, true, "Bend slider") });
        if number_field(ui, "Bend", Some(pct), "%", FIELD_W).is_some_and(|t| t.set(&mut pct, -100.0..=100.0)) {
            pct = pct.round();
        }
    });
    if pct != start {
        w.bend = pct / 100.0;
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut w.vertical, false, "Horizontal");
        ui.selectable_value(&mut w.vertical, true, "Vertical");
    });
    *w != before
}

/// The Curve panel on the selection. Returns Straighten as applied, and any live edit.
fn curve_panel(ui: &mut Ui, units: Units, kind: &ShapeKind) -> (Option<Edit>, Option<Edit>) {
    let live = curve_body(ui, units, kind).map(Edit::Curve);
    let curved = matches!(kind, ShapeKind::Text { curve: Some(_), .. });
    let straighten = ui.add_enabled(crate::text_ops::curve_range(kind).is_some() && curved, egui::Button::new("Straighten")).clicked();
    (straighten.then_some(Edit::Curve(0.0)), live)
}

/// Diameter as a slider and a field in the current units, and which side of the circle the text sits on.
/// Returns the new signed diameter when it changed. Text with several lines or wrapping cannot curve, so
/// the controls show disabled.
pub(in crate::ui) fn curve_body(ui: &mut Ui, units: Units, kind: &ShapeKind) -> Option<f32> {
    let ShapeKind::Text { curve, .. } = kind else { return None };
    let curve_in = curve.map_or(0.0, |c| c.diameter_in);
    let range = crate::text_ops::curve_range(kind);
    let enabled = range.is_some();
    let range = range.unwrap_or(1.0..=10.0);
    let mut diameter = if curve_in == 0.0 { *range.end() } else { curve_in.abs().clamp(*range.start(), *range.end()) };
    let mut on_top = curve_in >= 0.0;
    let before = (diameter, on_top);

    if !enabled {
        ui.label(egui::RichText::new("Curve needs a single line without wrapping.").font(fonts::regular(13.0)).color(TEXT_MUTED));
    }
    ui.add_enabled_ui(enabled, |ui| {
        ui.label("Diameter");
        ui.horizontal(|ui| {
            let mut shown = units.from_in(diameter);
            let shown_range = units.from_in(*range.start())..=units.from_in(*range.end());
            ui.spacing_mut().slider_width = 120.0;
            let slider = ui.add(egui::Slider::new(&mut shown, shown_range).show_value(false));
            slider.widget_info(|| WidgetInfo { value: Some(shown.into()), ..WidgetInfo::labeled(WidgetType::Slider, true, "Diameter slider") });
            if slider.changed() {
                diameter = units.to_in(shown);
            }
            _ = length_field(ui, "Diameter", Some(diameter), units, FIELD_W).is_some_and(|t| t.set(&mut diameter, range.clone()));
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut on_top, true, "Text on top");
            ui.selectable_value(&mut on_top, false, "Text below");
        });
    });
    let signed = if on_top { diameter } else { -diameter };
    (enabled && (diameter, on_top) != before).then_some(signed)
}

/// A square tile previewing `style` bent on a square.
fn style_tile(ui: &mut Ui, style: WarpStyle, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::click());
    let resp = tip(resp, style.label(), style_tip(style), None);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, style.label()));
    let fill = if resp.hovered() || selected { ACCENT_HOVER } else { egui::Color32::TRANSPARENT };
    let edge = if selected { Stroke::new(2.0_f32, ACCENT_PRIMARY) } else { Stroke::new(1.0_f32, BORDER_SUBTLE) };
    ui.painter().rect(rect, 8.0, fill, edge, StrokeKind::Inside);
    let preview = Rect::from_center_size(rect.center(), Vec2::splat(20.0));
    let warp = Warp { style, bend: 0.4, vertical: false };
    for contour in crate::warp::apply(warp, &ShapeKind::Square.outline(1.0, 1.0), (1.0, 1.0)) {
        let pts = contour.flatten().into_iter().map(|(x, y)| pos2(preview.left() + x * preview.width(), preview.top() + y * preview.height())).collect();
        ui.painter().add(egui::Shape::closed_line(pts, Stroke::new(1.5_f32, TEXT_PRIMARY)));
    }
    resp
}

fn style_tip(style: WarpStyle) -> &'static str {
    match style {
        WarpStyle::Arch => "Bend the whole shape up into an arch.",
        WarpStyle::Bulge => "Swell the middle outward, or pinch it with a negative bend.",
        WarpStyle::Wave => "Ripple the shape along one full wave.",
        WarpStyle::Flag => "Wave the shape like a flag pinned at its left edge.",
        WarpStyle::Rise => "Slant the shape upward, keeping its edges straight.",
    }
}
