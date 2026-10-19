//! The Font dropdown's menu: a search field over "Kerf fonts" (bundled), "Writing" (single-line fonts a
//! pen draws once) and "System fonts" (installed, alphabetical), with a "Writing only" chip. Each name
//! is drawn in its own face once that face has loaded; a writing font's name is drawn in its strokes.

use egui::{Align, Align2, Context, FontId, Id, Key, Rect, Response, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::system_fonts::WritingFont;
use crate::text::{FontRef, TextSpec};
use crate::ui::chrome::{text, text_width};
use crate::ui::fonts;
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::tokens::*;

const SEARCH: &str = "Search fonts";
const WRITING_ONLY: &str = "Writing only";
const LIST_H: f32 = 360.0;
/// The em a writing font's name is drawn at, in points, and its line width.
const WRITING_EM: f32 = 15.0;
const WRITING_LINE: f32 = 1.25;

fn search_id() -> Id {
    Id::new("font menu search")
}

fn writing_only_id() -> Id {
    Id::new("font menu writing only")
}

/// Clears the search, for when the menu opens.
pub fn reset(ctx: &Context) {
    ctx.data_mut(|d| d.remove_temp::<String>(search_id()));
}

/// The menu body. `system` is `None` while the font scan is still running. Returns the font picked,
/// by click or by Enter in the search field, which takes the first match.
pub fn show(ui: &mut Ui, current: Option<&FontRef>, system: Option<&[String]>, writing: &[WritingFont]) -> Option<FontRef> {
    let stored = ui.data(|d| d.get_temp::<String>(search_id()));
    let mut query = stored.clone().unwrap_or_default();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let field = TextEdit::singleline(&mut query).hint_text(SEARCH).frame(false).font(fonts::regular(14.0)).text_color(TEXT_PRIMARY).vertical_align(Align::Center);
    let resp = ui.put(rect.shrink2(vec2(10.0, 1.0)), field);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, SEARCH));
    let edge = if resp.has_focus() { ACCENT_PRIMARY } else { BORDER_SUBTLE };
    ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    if stored.is_none() {
        resp.request_focus();
    }
    let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    ui.data_mut(|d| d.insert_temp(search_id(), query.clone()));
    ui.add_space(6.0);
    let mut writing_only = ui.data(|d| d.get_temp::<bool>(writing_only_id())).unwrap_or(false);
    if chip(ui, WRITING_ONLY, writing_only).clicked() {
        writing_only = !writing_only;
        ui.data_mut(|d| d.insert_temp(writing_only_id(), writing_only));
    }
    ui.add_space(2.0);

    let needle = query.trim().to_lowercase();
    let matches = |name: &str| name.to_lowercase().contains(&needle);
    let bundled: Vec<FontRef> = if writing_only { Vec::new() } else { FontRef::BUNDLED.into_iter().filter(|f| matches(&f.label())).collect() };
    let pens: Vec<&WritingFont> = writing.iter().filter(|w| matches(&w.name)).collect();
    let installed: Vec<&String> = if writing_only { Vec::new() } else { system.unwrap_or_default().iter().filter(|name| matches(name)).collect() };
    if entered {
        let first_pen = || pens.first().map(|w| FontRef::Stroke(w.id.clone()));
        return bundled.first().cloned().or_else(first_pen).or_else(|| installed.first().map(|&name| FontRef::System(name.clone())));
    }

    ScrollArea::vertical()
        .max_height(LIST_H)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let mut picked = None;
            if !writing_only {
                heading(ui, "Kerf fonts");
                for font in bundled {
                    if font_row(ui, &font.label(), current == Some(&font), |ctx| fonts::typeface(ctx, &font, 15.0)).clicked() {
                        picked = Some(font);
                    }
                }
            }
            heading(ui, "Writing");
            if let Some(FontRef::Stroke(id)) = current
                && !writing.iter().any(|w| w.id == *id)
                && matches(&id.0)
            {
                note(ui, &format!("{} (missing)", id.0));
            }
            if pens.is_empty() {
                note(ui, "No matching fonts");
            }
            for pen in pens {
                let font = FontRef::Stroke(pen.id.clone());
                if writing_row(ui, &pen.name, &font, current == Some(&font)).clicked() {
                    picked = Some(font);
                }
            }
            if writing_only {
                return picked;
            }
            heading(ui, "System fonts");
            if let (Some(FontRef::System(name)), Some(list)) = (current, system)
                && !list.contains(name)
                && matches(name)
            {
                note(ui, &format!("{name} (missing)"));
            }
            match system {
                None => note(ui, "Loading fonts..."),
                Some(_) if installed.is_empty() => note(ui, "No matching fonts"),
                Some(_) => {
                    for name in installed {
                        let selected = matches!(current, Some(FontRef::System(n)) if n == name);
                        let face = |ctx: &Context| crate::ui::font_preview::family(ctx, name).map_or_else(|| fonts::regular(15.0), |f| FontId::new(15.0, f));
                        if font_row(ui, name, selected, face).clicked() {
                            picked = Some(FontRef::System(name.clone()));
                        }
                    }
                }
            }
            picked
        })
        .inner
}

fn heading(ui: &mut Ui, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    text(ui.painter(), pos2(rect.left() + 8.0, rect.bottom() - 6.0), Align2::LEFT_BOTTOM, title, fonts::semibold(12.0), TEXT_MUTED);
}

/// A muted, unclickable row, such as "Loading fonts..." or a missing family.
fn note(ui: &mut Ui, label: &str) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
    text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), TEXT_MUTED);
}

/// A rounded toggle the width of its label.
fn chip(ui: &mut Ui, label: &str, on: bool) -> Response {
    let font = fonts::regular(13.0);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let rect = Rect::from_min_size(row.min, vec2(text_width(ui, label, font.clone()) + 24.0, 26.0));
    let resp = ui.interact(rect, ui.id().with(label), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
    let (fill, edge, ink) = match (on, resp.hovered()) {
        (true, _) => (ACCENT_HOVER, ACCENT_PRIMARY, ACCENT_PRIMARY),
        (false, true) => (ACCENT_HOVER, BORDER_SUBTLE, TEXT_PRIMARY),
        (false, false) => (BG_PANEL, BORDER_SUBTLE, TEXT_PRIMARY),
    };
    ui.painter().rect(rect, 13.0, fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, font, ink);
    resp
}

/// A font row, the name drawn in `face`, which is only asked for while the row is on screen.
fn font_row(ui: &mut Ui, label: &str, selected: bool, face: impl FnOnce(&Context) -> FontId) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, label));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if selected { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    let clip = rect.shrink2(vec2(8.0, 0.0));
    ui.painter().with_clip_rect(clip).text(pos2(clip.left(), rect.center().y), Align2::LEFT_CENTER, label, face(ui.ctx()), ink);
    resp
}

/// A writing font's row: its name laid out in the font and drawn as pen lines, and a pen at the right.
fn writing_row(ui: &mut Ui, name: &str, font: &FontRef, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, name));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if selected { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    let pen = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(16.0));
    let clip = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(pen.left() - 6.0, rect.bottom()));
    if let Some(layout) = crate::text::layout(&TextSpec { content: name, font, size_in: 1.0, ..TextSpec::default() }) {
        let origin = pos2(clip.left(), rect.center().y - layout.size.1 * WRITING_EM / 2.0);
        let painter = ui.painter().with_clip_rect(clip);
        for contour in layout.contours() {
            let points = contour.flatten().into_iter().map(|(x, y)| origin + vec2(x, y) * WRITING_EM).collect();
            painter.add(egui::Shape::line(points, Stroke::new(WRITING_LINE, ink)));
        }
    }
    paint_icon(ui, Icon::Pen, pen, TEXT_MUTED);
    resp
}
