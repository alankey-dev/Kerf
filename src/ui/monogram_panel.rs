//! The Monogram panel (spec 42): letters, theme, style, frame, fonts, adjustments, output and operation,
//! over a live preview. It sits in the Shapes panel's slot and never touches the design until Insert or
//! Update; the app runs those as edits.

use std::rc::Rc;

use egui::{Align2, Color32, Context, Frame as EguiFrame, Id, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, Pt};
use crate::edit::{Edit, Insert, Subject};
use crate::monogram::{Adjust, Frame, MakeAs, MonogramSpec, Output, Style, Theme};
use crate::snap::Aabb;
use crate::text::FontRef;
use crate::units::Units;

use super::chrome::text;
use super::flow_kit;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::popover::popover;
use super::tokens::*;
use super::tooltip::tip;

const PREVIEW_H: f32 = 210.0;
const GAP: f32 = 8.0;
/// Letters drawn in the tiles before any are typed.
const SAMPLE: &str = "ABC";
/// Previews kept between frames, newest last.
const CACHED: usize = 48;

/// What the panel asks the app to do.
pub enum PanelEvent {
    Insert(MonogramSpec),
    Update(u64, MonogramSpec),
    Close,
}

/// The open panel: the spec being built, and the monogram it came from when editing one.
pub struct MonogramPanel {
    pub spec: MonogramSpec,
    pub editing: Option<u64>,
    previews: Vec<(MonogramSpec, Rc<Preview>)>,
}

impl MonogramPanel {
    /// A new monogram in the first theme.
    pub fn new() -> Self {
        Self { spec: MonogramSpec::default(), editing: None, previews: Vec::new() }
    }

    /// Editing monogram `id`, made from `spec`.
    pub fn editing(id: u64, spec: MonogramSpec) -> Self {
        Self { spec, editing: Some(id), previews: Vec::new() }
    }

    /// `spec` drawn as it would be inserted, kept until it falls out of the cache.
    fn preview(&mut self, spec: &MonogramSpec) -> Rc<Preview> {
        if let Some(i) = self.previews.iter().position(|(s, _)| s == spec) {
            let hit = self.previews.remove(i);
            let preview = hit.1.clone();
            self.previews.push(hit);
            return preview;
        }
        let preview = Rc::new(Preview::of(spec));
        if self.previews.len() >= CACHED {
            self.previews.remove(0);
        }
        self.previews.push((spec.clone(), preview.clone()));
        preview
    }
}

/// One piece of a preview: its closed rings and open lines in inches, and its colour.
type PreviewPiece = (Vec<Vec<Pt>>, Vec<Vec<Pt>>, Color32);

/// A monogram evaluated as the canvas would draw it, so Knockout and Welded show their results.
struct Preview {
    pieces: Vec<PreviewPiece>,
    bounds: Aabb,
    notices: Vec<String>,
}

impl Preview {
    fn of(spec: &MonogramSpec) -> Preview {
        let mut design = Design::default();
        design.insert_monogram(spec, Some((0.0, 0.0)));
        let notices = crate::monogram::layout(spec, &crate::system_fonts::current()).notices;
        let mut pieces = Vec::new();
        let mut bounds = Aabb::EMPTY;
        for shape in design.drawn() {
            let geometry = design.geometry(shape);
            for piece in geometry.pieces() {
                let rings = geometry.piece_rings(piece, crate::geometry::FLATTEN_TOL_IN);
                let open = piece.lines.iter().filter(|(_, closed)| !closed).map(|(pts, _)| pts.clone()).collect();
                bounds = bounds.union(piece.bounds);
                pieces.push((rings, open, piece.style.fill));
            }
        }
        Preview { pieces, bounds, notices }
    }

    /// Paints the monogram centred in `rect`, as large as fits.
    fn paint(&self, ui: &Ui, rect: Rect, ink: Option<Color32>) {
        if self.bounds.is_empty() {
            return;
        }
        let scale = (rect.width() / self.bounds.width().max(0.01)).min(rect.height() / self.bounds.height().max(0.01));
        let (cx, cy) = self.bounds.centre();
        let map = |(x, y): Pt| (rect.center().x + (x - cx) * scale, rect.center().y + (y - cy) * scale);
        for (rings, open, fill) in &self.pieces {
            let colour = ink.unwrap_or(*fill);
            let screen: Vec<Vec<Pt>> = rings.iter().map(|r| r.iter().map(|&p| map(p)).collect()).collect();
            let (points, tris) = crate::geometry::triangulate(&screen);
            let mut mesh = egui::Mesh::default();
            for p in &points {
                mesh.colored_vertex(pos2(p.0, p.1), colour);
            }
            for [a, b, c] in tris {
                mesh.add_triangle(a as u32, b as u32, c as u32);
            }
            ui.painter().add(mesh);
            for line in open {
                let pts = line.iter().map(|&p| map(p)).map(|(x, y)| pos2(x, y)).collect();
                ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5_f32, colour)));
            }
        }
    }
}

impl super::Kerf {
    /// Shows the open Monogram panel and runs what it asks for: Insert at the centre of the view, or
    /// Update in place, each one edit. Either closes the panel.
    pub(super) fn monogram_panel(&mut self, ctx: &Context) {
        let Some(panel) = &mut self.doc.monogram else { return };
        let edit = match show(ctx, panel, self.units) {
            None => return,
            Some(PanelEvent::Close) => None,
            Some(PanelEvent::Insert(spec)) => Some(Edit::Insert(Insert::Monogram { spec: Box::new(spec), at: Some(self.doc.view.centre_in()) })),
            Some(PanelEvent::Update(id, spec)) => Some(Edit::Monogram { id, spec: Box::new(spec) }),
        };
        self.doc.monogram = None;
        if let Some(edit) = edit {
            _ = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &edit, Subject::Selection);
        }
    }
}

/// The panel. Returns what it asks the app to do this frame.
pub fn show(ctx: &Context, panel: &mut MonogramPanel, units: Units) -> Option<PanelEvent> {
    let frame = EguiFrame::NONE.fill(BG_PANEL).inner_margin(egui::Margin { left: 15, right: 15, top: 0, bottom: 12 });
    egui::SidePanel::left("monogram-panel")
        .exact_width(SHAPES_PANEL_W)
        .resizable(false)
        .frame(frame)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().vline(full.right() + 15.0 - 0.5, full.y_range().expand(12.0), Stroke::new(1.0_f32, BORDER_SUBTLE));
            let mut event = header(ui, panel.editing.is_some());
            let footer_h = 44.0;
            let body_h = (ui.available_height() - footer_h).max(80.0);
            egui::ScrollArea::vertical().max_height(body_h).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(GAP, 6.0);
                body(ui, panel, units);
            });
            ui.add_space(8.0);
            let ready = !panel.spec.initials().is_empty();
            let label = if panel.editing.is_some() { "Update" } else { "Insert" };
            let resp = ui.add_sized(vec2(ui.available_width(), 32.0), |ui: &mut Ui| flow_kit::primary_button(ui, label, ready));
            let resp = if ready {
                tip(
                    resp,
                    label,
                    if panel.editing.is_some() { "Replace the monogram with these settings, in place." } else { "Add the monogram at the centre of the view." },
                    None,
                )
            } else {
                tip(resp, label, "Type at least one letter.", None)
            };
            if resp.clicked() && ready {
                event = Some(match panel.editing {
                    Some(id) => PanelEvent::Update(id, panel.spec.clone()),
                    None => PanelEvent::Insert(panel.spec.clone()),
                });
            }
            event
        })
        .inner
}

fn header(ui: &mut Ui, editing: bool) -> Option<PanelEvent> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let title = if editing { "Edit monogram" } else { "Monogram" };
    text(ui.painter(), row.left_center(), Align2::LEFT_CENTER, title, fonts::semibold(14.0), TEXT_PRIMARY);
    let close = Rect::from_center_size(pos2(row.right() - 12.0, row.center().y), Vec2::splat(24.0));
    let resp = tip(ui.interact(close, Id::new("monogram-close"), Sense::click()), "Close", "Close the Monogram panel without changing the canvas.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Close Monogram"));
    if resp.hovered() {
        ui.painter().rect_filled(close, 4.0, ACCENT_HOVER);
    }
    paint_icon(ui, Icon::Close, close.shrink(4.0), TEXT_MUTED);
    resp.clicked().then_some(PanelEvent::Close)
}

fn body(ui: &mut Ui, panel: &mut MonogramPanel, units: Units) {
    let main = panel.preview(&panel.spec.clone());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), PREVIEW_H), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, "Monogram preview"));
    ui.painter().rect(rect, 8.0, BG_CANVAS_VOID, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    if panel.spec.initials().is_empty() {
        text(ui.painter(), rect.center(), Align2::CENTER_CENTER, "Type your initials below", fonts::regular(13.0), TEXT_MUTED);
    } else {
        main.paint(ui, rect.shrink(18.0), None);
    }
    for notice in &main.notices {
        flow_kit::notice(ui, flow_kit::Notice::Warning, notice);
    }

    heading(ui, "Letters", None);
    let field = flow_kit::text_field(ui, &mut panel.spec.letters, "Up to 3 letters");
    field.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Letters"));
    if field.changed() {
        panel.spec.letters = panel.spec.letters.chars().take(3).collect();
    }
    ui.label(RichText::new(panel.spec.style.help()).font(fonts::regular(12.0)).color(TEXT_MUTED));

    let shown: String = if panel.spec.initials().is_empty() { SAMPLE.into() } else { panel.spec.letters.clone() };
    heading(ui, "Theme", Some(panel.spec.theme.as_deref().unwrap_or("Custom")));
    if let Some(theme) = grid(ui, crate::monogram::themes(), 3, 1.0, |ui, rect, theme: &Theme| {
        let mut spec = MonogramSpec { letters: shown.clone(), ..MonogramSpec::default() };
        spec.apply_theme(theme);
        let preview = panel.preview(&spec);
        let selected = panel.spec.theme.as_deref() == Some(theme.name.as_str());
        tile(ui, rect, &format!("{} theme", theme.name), &theme.name, selected, |ui, art| preview.paint(ui, art, None))
    }) {
        panel.spec.apply_theme(theme);
    }

    heading(ui, "Style", None);
    if let Some(&style) = grid(ui, &Style::ALL, 3, 1.0, |ui, rect, &style: &Style| {
        let spec = MonogramSpec { letters: shown.clone(), style, frame: Frame::None, fonts: panel.spec.fonts.clone(), ..MonogramSpec::default() };
        let preview = panel.preview(&spec);
        tile(ui, rect, &format!("{} style", style.label()), style.label(), panel.spec.style == style, |ui, art| preview.paint(ui, art, Some(TEXT_PRIMARY)))
    }) {
        panel.spec.style = style;
        follow_theme(&mut panel.spec);
    }

    heading(ui, "Frame", None);
    if let Some(&frame) = grid(ui, &Frame::ALL, 4, 1.0, |ui, rect, &frame: &Frame| {
        tile(ui, rect, &format!("{} frame", frame.label()), frame.label(), panel.spec.frame == frame, |ui, art| paint_frame(ui, art, frame))
    }) {
        panel.spec.frame = frame;
        follow_theme(&mut panel.spec);
    }

    heading(ui, "Fonts", None);
    let sides = panel.spec.style.has_sides();
    let centre_label = if sides { "Centre font" } else { "Font" };
    if let Some(font) = font_picker(ui, centre_label, &panel.spec.fonts.centre, true) {
        panel.spec.fonts.centre = font;
        follow_theme(&mut panel.spec);
    }
    if sides && let Some(font) = font_picker(ui, "Side font", &panel.spec.fonts.sides, true) {
        panel.spec.fonts.sides = font;
        follow_theme(&mut panel.spec);
    }

    heading(ui, "Adjust", None);
    let framed = panel.spec.frame != Frame::None;
    let adjust = &mut panel.spec.adjust;
    length_row(ui, "Letter spacing", &mut adjust.spacing_in, Adjust::SPACING_IN, units, panel.spec.style != Style::Single);
    length_row(ui, "Centre size", &mut adjust.centre_in, Adjust::CENTRE_IN, units, true);
    length_row(ui, "Frame padding", &mut adjust.padding_in, Adjust::PADDING_IN, units, framed);
    length_row(ui, "Frame thickness", &mut adjust.thickness_in, Adjust::THICKNESS_IN, units, framed);

    heading(ui, "Output", None);
    let pen = panel.spec.operation == MakeAs::Pen;
    let outputs: Vec<(Output, &str)> = Output::ALL.iter().map(|o| (*o, o.label())).collect();
    let picked = ui.add_enabled_ui(!pen, |ui| flow_kit::segmented(ui, panel.spec.output_made(), &outputs)).inner;
    if let Some(output) = picked {
        panel.spec.output = output;
    }
    let output_help = match panel.spec.output_made() {
        _ if pen => "A pen draws lines, so Pen keeps the frame and letters as separate shapes.",
        Output::Layered => "The frame and each letter stay separate shapes.",
        Output::Knockout => "The letters are cut out of a solid frame.",
        Output::Welded => "The frame and letters join into one outline.",
    };
    ui.label(RichText::new(output_help).font(fonts::regular(12.0)).color(TEXT_MUTED));

    heading(ui, "Operation", None);
    let operations: Vec<(MakeAs, &str)> = MakeAs::ALL.iter().map(|o| (*o, o.label())).collect();
    if let Some(operation) = flow_kit::segmented(ui, panel.spec.operation, &operations) {
        panel.spec.operation = operation;
    }
    if pen {
        ui.label(RichText::new("Fonts with a writing font draw in it.").font(fonts::regular(12.0)).color(TEXT_MUTED));
    }
}

/// Once style, frame or fonts differ from the theme they came from, the theme shows as Custom.
fn follow_theme(spec: &mut MonogramSpec) {
    let same = |t: &Theme| t.style == spec.style && t.frame == spec.frame && t.fonts == spec.fonts;
    if spec.theme.as_deref().and_then(crate::monogram::theme).is_some_and(|t| !same(t)) {
        spec.theme = None;
    }
}

fn heading(ui: &mut Ui, title: &str, detail: Option<&str>) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(fonts::semibold(13.0)).color(TEXT_PRIMARY));
        if let Some(detail) = detail {
            ui.label(RichText::new(detail).font(fonts::regular(12.0)).color(TEXT_MUTED));
        }
    });
}

/// `items` in rows of `columns` square-ish cells; returns the one clicked.
fn grid<'a, T>(ui: &mut Ui, items: &'a [T], columns: usize, aspect: f32, mut cell: impl FnMut(&mut Ui, Rect, &T) -> bool) -> Option<&'a T> {
    let w = (ui.available_width() - GAP * (columns as f32 - 1.0)) / columns as f32;
    let h = w * aspect + 18.0;
    let mut picked = None;
    for row in items.chunks(columns) {
        let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        for (i, item) in row.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(band.left() + i as f32 * (w + GAP), band.top()), vec2(w, h));
            if cell(ui, rect, item) {
                picked = Some(item);
            }
        }
    }
    picked
}

/// A tile with artwork above a caption. True when clicked.
fn tile(ui: &mut Ui, rect: Rect, label: &str, caption: &str, selected: bool, art: impl FnOnce(&Ui, Rect)) -> bool {
    let resp = ui.interact(rect, Id::new(("monogram-tile", label)), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, label));
    let card = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - 18.0));
    let (fill, edge) = match (selected, resp.hovered()) {
        (true, _) => (ACCENT_HOVER, ACCENT_PRIMARY),
        (false, true) => (ACCENT_HOVER, BORDER_SUBTLE),
        (false, false) => (BG_PANEL, BORDER_SUBTLE),
    };
    ui.painter().rect(card, 6.0, fill, Stroke::new(if selected { 1.5_f32 } else { 1.0 }, edge), StrokeKind::Inside);
    art(ui, card.shrink(card.width() * 0.14));
    let ink = if selected { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    text(ui.painter(), pos2(rect.center().x, rect.max.y - 8.0), Align2::CENTER_CENTER, caption, fonts::regular(11.0), ink);
    resp.clicked()
}

fn paint_frame(ui: &Ui, rect: Rect, frame: Frame) {
    let rings = frame.outline();
    if rings.is_empty() {
        let r = rect.shrink(rect.width() * 0.2);
        ui.painter().line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5_f32, TEXT_MUTED));
        return;
    }
    let bounds = rings.iter().flatten().fold(Aabb::EMPTY, |acc, &(x, y)| acc.union(Aabb { min_x: x, min_y: y, max_x: x, max_y: y }));
    let scale = (rect.width() / bounds.width()).min(rect.height() / bounds.height());
    let (cx, cy) = bounds.centre();
    let screen: Vec<Vec<Pt>> = rings.iter().map(|r| r.iter().map(|&(x, y)| (rect.center().x + (x - cx) * scale, rect.center().y + (y - cy) * scale)).collect()).collect();
    let (points, tris) = crate::geometry::triangulate(&screen);
    let mut mesh = egui::Mesh::default();
    for p in &points {
        mesh.colored_vertex(pos2(p.0, p.1), TEXT_PRIMARY);
    }
    for [a, b, c] in tris {
        mesh.add_triangle(a as u32, b as u32, c as u32);
    }
    ui.painter().add(mesh);
}

/// A labelled dropdown listing Kerf's fonts (the bundled monogram families among them), the system
/// families and the writing fonts.
fn font_picker(ui: &mut Ui, label: &str, current: &FontRef, enabled: bool) -> Option<FontRef> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(84.0, 28.0), egui::Label::new(RichText::new(label).font(fonts::regular(13.0)).color(TEXT_PRIMARY)));
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), if enabled { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, enabled, label));
        let edge = if resp.hovered() { ACCENT_PRIMARY } else { BORDER_SUBTLE };
        ui.painter().rect(rect, 4.0, BG_PANEL, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
        let face = fonts::typeface(ui.ctx(), current, 13.0);
        text(ui.painter(), rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, &current.label(), face, TEXT_PRIMARY);
        paint_icon(ui, Icon::Chevron, Rect::from_center_size(rect.right_center() - vec2(14.0, 0.0), Vec2::splat(14.0)), TEXT_MUTED);
        if resp.clicked() {
            super::toolbar::font_menu::reset(ui.ctx());
        }
        let catalogue = crate::system_fonts::current();
        let mut families: Vec<String> = crate::system_fonts::DISPLAY.iter().map(|(name, _)| (*name).to_owned()).collect();
        families.extend(catalogue.families().unwrap_or_default().iter().filter(|f| !families.contains(f)).cloned().collect::<Vec<_>>());
        picked = popover(ui, &resp, ("monogram-font", label), 260.0, |ui| super::toolbar::font_menu::show(ui, Some(current), Some(&families), &catalogue.writing()));
    });
    picked
}

/// A caption, a typed length field and a slider for one adjustment.
fn length_row(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, units: Units, enabled: bool) {
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(110.0, 28.0), egui::Label::new(RichText::new(label).font(fonts::regular(13.0)).color(if enabled { TEXT_PRIMARY } else { DISABLED_TEXT })));
            let field_w = 84.0;
            let slider_w = (ui.available_width() - field_w - GAP).max(40.0);
            slider(ui, label, value, &range, slider_w);
            if let Some(typed) = crate::units::length_field(ui, label, Some(*value), units, field_w) {
                *value = typed.apply(*value).clamp(*range.start(), *range.end());
            }
        });
    });
}

fn slider(ui: &mut Ui, label: &str, value: &mut f32, range: &std::ops::RangeInclusive<f32>, width: f32) {
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let track = rect.shrink2(vec2(8.0, 0.0));
    let (lo, hi) = (*range.start(), *range.end());
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        *value = lo + t * (hi - lo);
    }
    let v = f64::from(*value);
    resp.widget_info(|| WidgetInfo::slider(enabled, v, format!("{label} slider")));
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let knob = pos2(track.left() + t * track.width(), track.center().y);
    let ink = if enabled { ACCENT_PRIMARY } else { MAT_GREY };
    let painter = ui.painter();
    painter.rect_filled(Rect::from_min_max(pos2(track.left(), knob.y - 2.0), pos2(track.right(), knob.y + 2.0)), 2, BORDER_SUBTLE);
    painter.rect_filled(Rect::from_min_max(pos2(track.left(), knob.y - 2.0), pos2(knob.x, knob.y + 2.0)), 2, ink);
    painter.circle(knob, 7.0, BG_PANEL, Stroke::new(if resp.hovered() || resp.dragged() { 2.0_f32 } else { 1.5 }, ink));
}
