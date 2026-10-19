//! The swatch picker (spec 56), drawn inside the colour popover in place of a bare egui colour wheel. Top
//! to bottom it shows a hex field, the hue/saturation/value area, the design's own colours, the recent
//! colours and the user's saved swatches, with the bundled palettes to add from. Clicking any swatch
//! returns the colour for the caller to apply in one undo step, as the swatch has always done.

use egui::{Color32, Key, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, vec2};

use crate::design::Design;
use crate::palettes::{self, hex_string};
use crate::settings::{RecentColours, Swatch};

use super::tokens::*;
use super::{flow_kit, fonts, tooltip};

/// The recent colours and saved swatches the picker reads and writes, borrowed from the app so a pick is
/// remembered. `changed` is raised when either list changes, so the caller saves `settings.json`.
pub struct Palette<'a> {
    recent: &'a mut RecentColours,
    saved: &'a mut Vec<Swatch>,
    pub changed: bool,
}

impl<'a> Palette<'a> {
    pub fn new(recent: &'a mut RecentColours, saved: &'a mut Vec<Swatch>) -> Self {
        Self { recent, saved, changed: false }
    }

    /// A discretely chosen colour (a swatch click, a committed hex) goes to the front of Recent.
    fn record(&mut self, colour: Color32) {
        self.recent.record(colour);
        self.changed = true;
    }
}

const SWATCH: f32 = 18.0;

/// Draws the picker and returns the colour to apply this frame, if the user chose one. A drag of the HSV
/// area applies live but is not recorded; a hex commit or a swatch click is.
pub fn show(ui: &mut Ui, current: Option<Color32>, design: &Design, palette: &mut Palette) -> Option<Color32> {
    let mut chosen = None;
    let mut record = false;

    if let Some(c) = hex_field(ui, current) {
        chosen = Some(c);
        record = true;
    }

    ui.add_space(8.0);
    let mut hsv = current.unwrap_or(Color32::BLACK);
    if egui::color_picker::color_picker_color32(ui, &mut hsv, egui::color_picker::Alpha::Opaque) {
        chosen = Some(hsv);
    }

    let uses = palettes::project_colours(design);
    if !uses.is_empty() {
        section(ui, "Project colours");
        swatches(
            ui,
            &mut chosen,
            &mut record,
            uses.iter().map(|u| {
                let shapes = if u.count == 1 { "shape" } else { "shapes" };
                let suffix = if u.pen { " pen" } else { "" };
                (u.colour, format!("{}, {} {shapes}{suffix}", palettes::name_of(u.colour), u.count))
            }),
        );
    }

    if !design.swatches.is_empty() {
        section(ui, "Project palette");
        swatches(ui, &mut chosen, &mut record, design.swatches.iter().map(|s| (s.colour, format!("{} {}", s.name, hex_string(s.colour)))));
    }

    if !palette.recent.is_empty() {
        section(ui, "Recent");
        let recent: Vec<Color32> = palette.recent.iter().collect();
        swatches(ui, &mut chosen, &mut record, recent.iter().map(|&c| (c, format!("Recent {}", hex_string(c)))));
    }

    section(ui, "Saved");
    saved_row(ui, current, palette, &mut chosen, &mut record);

    if let Some(c) = chosen
        && record
    {
        palette.record(c);
    }
    chosen
}

/// A muted caption above a section, with a little space before it.
fn section(ui: &mut Ui, label: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(label).font(fonts::regular(11.0)).color(TEXT_MUTED));
    ui.add_space(4.0);
}

/// The hex field: accepts `#RGB`, `#RRGGBB`, `RRGGBB` and CSS names on Enter, showing a message when the
/// text does not parse. Returns the parsed colour on a good commit.
fn hex_field(ui: &mut Ui, current: Option<Color32>) -> Option<Color32> {
    #[derive(Clone, Default)]
    struct Draft {
        text: String,
        error: Option<String>,
    }
    let id = ui.id().with("hex");
    let draft_id = id.with("draft");
    let focused = ui.memory(|m| m.has_focus(id));
    let shown = current.map(hex_string).unwrap_or_default();
    let mut draft: Draft = ui.data(|d| d.get_temp(draft_id)).filter(|_| focused).unwrap_or(Draft { text: shown, error: None });

    let width = ui.available_width();
    let resp = ui.add(egui::TextEdit::singleline(&mut draft.text).id(id).desired_width(width).hint_text("#RRGGBB or a colour name").font(fonts::regular(13.0)));
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Hex colour"));

    let mut out = None;
    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    if resp.lost_focus() && enter && !draft.text.trim().is_empty() {
        match palettes::parse_colour(&draft.text) {
            Ok(colour) => {
                out = Some(colour);
                draft.error = None;
            }
            Err(message) => {
                draft.error = Some(message);
                resp.request_focus();
            }
        }
    }
    if let Some(message) = &draft.error {
        tooltip::card(&resp, message, "Type a hex colour such as #D0202E, or a colour name.", None);
    }
    let editing = resp.has_focus();
    ui.data_mut(|d| {
        if editing {
            d.insert_temp(draft_id, draft);
        } else {
            d.remove_temp::<Draft>(draft_id);
        }
    });
    out
}

/// A wrapped grid of clickable swatches. A click sets `chosen` and asks for it to be recorded.
fn swatches(ui: &mut Ui, chosen: &mut Option<Color32>, record: &mut bool, items: impl Iterator<Item = (Color32, String)>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (colour, label) in items {
            if swatch_button(ui, colour, &label).clicked() {
                *chosen = Some(colour);
                *record = true;
            }
        }
    });
}

/// The Saved section: the user's swatches, a + to add the current colour, right-click to rename or
/// delete, and the bundled palettes to add a whole set from.
fn saved_row(ui: &mut Ui, current: Option<Color32>, palette: &mut Palette, chosen: &mut Option<Color32>, record: &mut bool) {
    let editing_id = ui.id().with("rename");
    let drag_id = ui.id().with("drag");
    let mut editing: Option<usize> = ui.data(|d| d.get_temp(editing_id));
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let mut remove: Option<usize> = None;
    let mut rects: Vec<Rect> = Vec::new();

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (i, swatch) in palette.saved.iter().enumerate() {
            let label = format!("{} {}", swatch.name, hex_string(swatch.colour));
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click_and_drag());
            rects.push(rect);
            let resp = tooltip::tip(resp, &label, "Apply this colour, or drag to reorder your swatches.", None);
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
            ui.painter().rect(rect, 4.0, swatch.colour, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
            if resp.hovered() || dragging == Some(i) {
                ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0_f32, ACCENT_PRIMARY), StrokeKind::Outside);
            }
            if resp.drag_started() {
                dragging = Some(i);
            }
            if resp.clicked() {
                *chosen = Some(swatch.colour);
                *record = true;
            }
            resp.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    editing = Some(i);
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    remove = Some(i);
                    ui.close_menu();
                }
            });
        }
        if let Some(colour) = current {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
            let resp = tooltip::tip(resp, "Save colour", "Add the current colour to your saved swatches.", None);
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Save colour"));
            let ink = if resp.hovered() { ACCENT_PRIMARY } else { TEXT_MUTED };
            ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0_f32, ink), StrokeKind::Inside);
            ui.painter().line_segment([rect.center() - vec2(5.0, 0.0), rect.center() + vec2(5.0, 0.0)], Stroke::new(1.5_f32, ink));
            ui.painter().line_segment([rect.center() - vec2(0.0, 5.0), rect.center() + vec2(0.0, 5.0)], Stroke::new(1.5_f32, ink));
            if resp.clicked() {
                palette.saved.push(Swatch { name: palettes::name_of(colour).to_owned(), colour });
                palette.changed = true;
            }
        }
    });

    if let Some(from) = dragging {
        let (released, pointer) = ui.input(|i| (i.pointer.any_released(), i.pointer.interact_pos()));
        if released {
            if let Some(to) = pointer.and_then(|p| nearest_swatch(&rects, p))
                && to != from
                && from < palette.saved.len()
            {
                let swatch = palette.saved.remove(from);
                palette.saved.insert(to.min(palette.saved.len()), swatch);
                palette.changed = true;
            }
            dragging = None;
        }
    }

    if let Some(i) = editing
        && let Some(swatch) = palette.saved.get_mut(i)
    {
        ui.add_space(4.0);
        let field = flow_kit::text_field(ui, &mut swatch.name, "Swatch name");
        field.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Swatch name"));
        if field.changed() {
            palette.changed = true;
        }
        if field.lost_focus() {
            editing = None;
        }
    }

    ui.add_space(6.0);
    if let Some(name) = flow_kit::dropdown(ui, "Add from palette", "", &palettes::BUNDLED.map(|p| (p.name, p.name)))
        && let Some(bundled) = palettes::BUNDLED.iter().find(|p| p.name == name)
    {
        for &colour in bundled.colours {
            palette.saved.push(Swatch { name: palettes::name_of(colour).to_owned(), colour });
        }
        palette.changed = true;
    }

    if let Some(i) = remove {
        if i < palette.saved.len() {
            palette.saved.remove(i);
            palette.changed = true;
        }
        editing = None;
    }
    ui.data_mut(|d| match editing {
        Some(i) => d.insert_temp(editing_id, i),
        None => {
            d.remove_temp::<usize>(editing_id);
        }
    });
    ui.data_mut(|d| match dragging {
        Some(i) => d.insert_temp(drag_id, i),
        None => {
            d.remove_temp::<usize>(drag_id);
        }
    });
}

/// The index of the saved swatch whose rectangle sits nearest `p`, by centre distance, for a reorder drop.
fn nearest_swatch(rects: &[Rect], p: Pos2) -> Option<usize> {
    rects.iter().enumerate().min_by(|(_, a), (_, b)| a.center().distance(p).total_cmp(&b.center().distance(p))).map(|(i, _)| i)
}

/// One colour swatch: a rounded fill with a subtle border, an accent ring on hover, and `label` as its
/// accessible name and tooltip.
fn swatch_button(ui: &mut Ui, colour: Color32, label: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    let resp = tooltip::tip(resp, label, "Apply this colour to the selection.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    ui.painter().rect(rect, 4.0, colour, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    if resp.hovered() {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0_f32, ACCENT_PRIMARY), StrokeKind::Outside);
    }
    resp
}
