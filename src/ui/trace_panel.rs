//! The trace panel (spec 28): an uploaded bitmap on the left with its proposed cut lines, and the
//! Background, Colours and Clean-up steps on the right. Every change re-traces a 768 px copy on the
//! image worker; Apply traces the full image there and places the result as one undo step.

use std::sync::Arc;
use std::time::Duration;

use egui::{Align2, Color32, Context, Frame, Margin, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};
use image::RgbaImage;

use super::chrome::{icon_button, text};
use super::icons::{Icon, paint_icon_rotated};
use super::tokens::*;
use super::tooltip::tip;
use super::{Kerf, flow_kit, fonts};
use crate::assets::AssetId;
use crate::design::{Contour, Operation, Pt, Seg, Shape, ShapeKind};
use crate::edit::{Edit, Placement, Subject};
use crate::trace::raster::{Recipe, Step};
use crate::trace::worker::{FULL_PX, Imported, Job, Preview, Product, TraceJob, Traced, Worker};
use crate::trace::{Colours, Layering, Palette, TraceSettings};
use crate::units::Units;

/// The preview re-traces this long after the last change, so a slider drag traces once it rests.
const DEBOUNCE_S: f64 = 0.05;
const CONTROLS_W: f32 = 360.0;
const VIEW_MARGIN: f32 = 32.0;
const CHECKER: f32 = 8.0;
const CHECKER_DARK: Color32 = Color32::from_rgb(0xe3, 0xe6, 0xea);
const SWATCH: f32 = 26.0;
const ZOOM_RANGE: (f32, f32) = (0.25, 16.0);
const SPECKLE_MAX_SQ_IN: f32 = 0.25;
const BRUSH_RANGE_IN: (f32, f32) = (0.02, 0.5);
const DEFAULT_POSTERISE: u8 = 4;
/// Zoomed in, each pixel shows as a crisp square, so the edge of a removal is easy to judge.
const PIXELS: egui::TextureOptions = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, ..egui::TextureOptions::LINEAR };

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    RemoveColour,
    Erase,
    Restore,
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Threshold {
        level: u8,
        invert: bool,
    },
    /// `palette` is `None` until the preview built one for `count`; the full trace reuses it so a
    /// swatch means the same colour at both resolutions.
    Posterise {
        count: u8,
        palette: Option<Palette>,
    },
}

/// Everything that changes the trace.
#[derive(Clone, Debug, PartialEq)]
struct Edits {
    recipe: Recipe,
    mode: Mode,
    layering: Layering,
    product: Product,
    speckle_sq_in: f32,
    smoothing: f32,
    corner_deg: f32,
}

/// The image the panel traces: the full decoded image and its preview copy live here, never in
/// `Design`, and only while the panel is open.
struct Source {
    px: (u32, u32),
    size_in: Pt,
    image: Arc<RgbaImage>,
    preview: Arc<RgbaImage>,
}

pub(super) struct TracePanel {
    source: Source,
    edits: Edits,
    /// Recipes before each Background change, for the panel's Undo.
    undo: Vec<Recipe>,
    tool: Tool,
    tolerance: f32,
    contiguous: bool,
    brush_in: f32,
    zoom: f32,
    pan: Vec2,
    stroke: Option<Vec<(f32, f32)>>,
    shown: Option<egui::TextureHandle>,
    /// The preview's cut lines, flattened, in 0..1 of the image.
    lines: Vec<Vec<(f32, f32)>>,
    /// When the edits changed and the preview has not been asked for yet.
    dirty_since: Option<f64>,
    /// Set while Apply's full trace runs.
    applying: bool,
}

enum Event {
    Close,
    Apply,
    Cancel,
}

impl TracePanel {
    pub(super) fn open(imported: &Imported) -> Self {
        let size_in = ShapeKind::Bitmap { asset: imported.asset.clone(), px: imported.px, clip: Vec::new() }.default_size_in();
        let defaults = TraceSettings::default();
        let px_per_in = imported.px.0 as f32 / size_in.0;
        Self {
            source: Source { px: imported.px, size_in, image: imported.image.clone(), preview: imported.preview.clone() },
            edits: Edits {
                recipe: Recipe::default(),
                mode: Mode::Threshold { level: 128, invert: false },
                layering: Layering::Stacked,
                product: Product::SingleCut,
                speckle_sq_in: (defaults.speckle_px as f32 / px_per_in).powi(2),
                smoothing: defaults.smoothing,
                corner_deg: defaults.corner_deg,
            },
            undo: Vec::new(),
            tool: Tool::RemoveColour,
            tolerance: 10.0,
            contiguous: true,
            brush_in: 0.1,
            zoom: 1.0,
            pan: Vec2::ZERO,
            stroke: None,
            shown: None,
            lines: Vec::new(),
            dirty_since: Some(f64::NEG_INFINITY),
            applying: false,
        }
    }

    /// The trace settings for a copy of the image `long_px` on its long side. Speckle is an area on
    /// the placed image, so it covers the same patch at every resolution.
    fn settings(&self, long_px: u32) -> TraceSettings {
        let px_per_in = long_px as f32 / self.source.size_in.0.max(self.source.size_in.1);
        let colours = match &self.edits.mode {
            Mode::Threshold { level, invert } => Colours::Threshold { level: *level, invert: *invert },
            Mode::Posterise { palette, .. } => Colours::Posterise(palette.clone().unwrap_or(Palette { swatches: Vec::new(), merged: Vec::new() })),
        };
        TraceSettings {
            colours,
            layering: self.edits.layering,
            speckle_px: (self.edits.speckle_sq_in.sqrt() * px_per_in).round() as u32,
            smoothing: self.edits.smoothing,
            corner_deg: self.edits.corner_deg,
        }
    }

    fn job(&self, image: &Arc<RgbaImage>) -> TraceJob {
        let new_palette = match &self.edits.mode {
            Mode::Posterise { count, palette: None } => Some(*count),
            _ => None,
        };
        let long = image.width().max(image.height()).min(FULL_PX);
        TraceJob { image: image.clone(), recipe: self.edits.recipe.clone(), settings: self.settings(long), product: self.edits.product, new_palette }
    }

    fn can_apply(&self) -> bool {
        let palette_ready = !matches!(self.edits.mode, Mode::Posterise { palette: None, .. });
        !self.applying && palette_ready && !self.lines.is_empty()
    }

    /// Takes a finished preview; the palette it built becomes the Posterise palette.
    pub(super) fn landed(&mut self, ctx: &Context, preview: Preview) {
        if let (Some(built), Mode::Posterise { palette, .. }) = (preview.palette, &mut self.edits.mode) {
            *palette = Some(built);
        }
        let [w, h] = preview.shown.size.map(|n| n.max(1) as f32);
        self.lines =
            preview.layers.iter().flat_map(|l| &l.contours).map(|c| crate::geometry::flatten(c, 0.5).into_iter().map(|(x, y)| (x / w, y / h)).collect()).collect();
        match &mut self.shown {
            Some(handle) => handle.set(preview.shown, PIXELS),
            None => self.shown = Some(ctx.load_texture("trace-preview", preview.shown, PIXELS)),
        }
    }

    pub(super) fn applying(&self) -> bool {
        self.applying
    }

    pub(super) fn apply_failed(&mut self) {
        self.applying = false;
    }

    fn changed(&mut self, now: f64) {
        self.dirty_since = Some(now);
    }

    /// Records a Background change for Undo. A new background means a new Posterise palette.
    fn edit_recipe(&mut self, now: f64, f: impl FnOnce(&mut Recipe)) {
        self.undo.push(self.edits.recipe.clone());
        f(&mut self.edits.recipe);
        self.forget_palette();
        self.changed(now);
    }

    fn undo_step(&mut self, now: f64) {
        if let Some(recipe) = self.undo.pop() {
            self.edits.recipe = recipe;
            self.forget_palette();
            self.changed(now);
        }
    }

    fn forget_palette(&mut self) {
        if let Mode::Posterise { palette, .. } = &mut self.edits.mode {
            *palette = None;
        }
    }

    /// Asks for a preview once the edits have rested; true while one is owed or running.
    fn schedule(&mut self, ctx: &Context, worker: &mut Worker, now: f64) -> bool {
        if let Some(since) = self.dirty_since
            && !self.applying
        {
            let wait = since + DEBOUNCE_S - now;
            if wait <= 0.0 {
                self.dirty_since = None;
                worker.submit(Job::Preview(self.job(&self.source.preview)));
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(wait));
            }
        }
        self.dirty_since.is_some() || worker.busy()
    }

    fn show(&mut self, ctx: &Context, worker: &mut Worker, units: Units) -> Option<Event> {
        let now = ctx.input(|i| i.time);
        let stale = self.schedule(ctx, worker, now);
        if stale {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        let mut event = header(ctx, self.source.px);
        let frame = Frame::NONE.fill(BG_PANEL).inner_margin(Margin::same(16));
        egui::SidePanel::right("trace-controls").exact_width(CONTROLS_W).resizable(false).frame(frame).show_separator_line(false).show(ctx, |ui| {
            let full = ui.max_rect().expand(16.0);
            ui.painter().vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
            flow_kit::themed(ui, |ui| {
                let footer = egui::TopBottomPanel::bottom("trace-footer").frame(Frame::NONE).show_separator_line(false);
                event = event.take().or(footer.show_inside(ui, |ui| self.footer(ui, now)).inner);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_enabled_ui(!self.applying, |ui| self.steps(ui, units, now));
                });
            });
        });
        egui::CentralPanel::default().frame(Frame::NONE.fill(BG_CANVAS_VOID)).show(ctx, |ui| self.viewport(ui, stale, now));
        if !self.applying && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo_step(now);
        }
        event
    }

    fn steps(&mut self, ui: &mut Ui, units: Units, now: f64) {
        ui.spacing_mut().item_spacing.y = 8.0;
        section(ui, "Background", |ui| self.background(ui, units, now));
        flow_kit::divider(ui);
        section(ui, "Colours", |ui| self.colours(ui, now));
        flow_kit::divider(ui);
        section(ui, "Clean-up", |ui| self.clean_up(ui, units, now));
    }

    fn background(&mut self, ui: &mut Ui, units: Units, now: f64) {
        let tools = [(Tool::RemoveColour, "Remove colour"), (Tool::Erase, "Erase"), (Tool::Restore, "Restore")];
        if let Some(t) = flow_kit::segmented(ui, self.tool, &tools) {
            self.tool = t;
        }
        match self.tool {
            Tool::RemoveColour => {
                flow_kit::body_text(ui, "Click the image to remove that colour.", true);
                slider(ui, "Tolerance", &mut self.tolerance, 0.0..=100.0, |v| format!("{}", v.round()));
                flow_kit::toggle_row(ui, "Contiguous", &mut self.contiguous);
            }
            Tool::Erase | Tool::Restore => {
                flow_kit::body_text(ui, "Drag over the image to paint.", true);
                slider(ui, "Brush size", &mut self.brush_in, BRUSH_RANGE_IN.0..=BRUSH_RANGE_IN.1, |v| units.len(v));
            }
        }
        let mut edges = self.edits.recipe.edges;
        if flow_kit::toggle_row(ui, "Stop at edges", &mut edges).clicked() {
            self.edit_recipe(now, |r| r.edges = edges);
        }
        flow_kit::controls(ui, |ui| {
            let auto = flow_kit::secondary_button(ui, "Auto remove", true);
            let auto = tip(auto, "Auto remove", "Remove the colour around the border of the image.", None);
            if auto.clicked() {
                self.edit_recipe(now, |r| r.steps.push(Step::AutoRemove));
            }
            let undo = flow_kit::secondary_button(ui, "Undo", !self.undo.is_empty());
            let keys = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z));
            if tip(undo, "Undo", "Take back the last background change.", Some(&keys)).clicked() {
                self.undo_step(now);
            }
        });
        let mut drop = None;
        for (i, step) in self.edits.recipe.steps.iter().enumerate() {
            let name = step_name(step, i);
            ui.horizontal(|ui| {
                ui.set_min_height(28.0);
                flow_kit::body_text(ui, &name, false);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, &format!("Remove {name}"), true).clicked() {
                        drop = Some(i);
                    }
                });
            });
        }
        if let Some(i) = drop {
            self.edit_recipe(now, |r| _ = r.steps.remove(i));
        }
    }

    fn colours(&mut self, ui: &mut Ui, now: f64) {
        let posterise = matches!(self.edits.mode, Mode::Posterise { .. });
        if let Some(pick) = flow_kit::segmented(ui, posterise, &[(false, "Threshold"), (true, "Posterise")]) {
            self.edits.mode = if pick { Mode::Posterise { count: DEFAULT_POSTERISE, palette: None } } else { Mode::Threshold { level: 128, invert: false } };
            if pick && self.edits.product == Product::SingleCut {
                self.edits.product = Product::CutLayers;
            }
            self.changed(now);
        }
        let mut changed = false;
        match &mut self.edits.mode {
            Mode::Threshold { level, invert } => {
                let mut v = f32::from(*level);
                changed |= slider(ui, "Threshold", &mut v, 0.0..=255.0, |v| format!("{}", v.round()));
                *level = v.round() as u8;
                changed |= flow_kit::toggle_row(ui, "Invert", invert).clicked();
            }
            Mode::Posterise { count, palette } => {
                flow_kit::controls(ui, |ui| {
                    caption(ui, "Colours");
                    ui.add_space(ui.available_width() - 3.0 * 28.0 - 16.0);
                    let fewer = icon_button(ui, Icon::Minus, "Fewer colours", *count > 2);
                    let shown = count.to_string();
                    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("{shown} colours")));
                    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, &shown, fonts::medium(13.0), TEXT_PRIMARY);
                    let more = icon_button(ui, Icon::Plus, "More colours", *count < 16);
                    let next = if fewer.clicked() {
                        *count - 1
                    } else if more.clicked() {
                        *count + 1
                    } else {
                        *count
                    };
                    if next != *count {
                        *count = next;
                        *palette = None;
                        changed = true;
                    }
                });
                match palette {
                    Some(p) => changed |= swatches(ui, p),
                    None => _ = flow_kit::body_text(ui, "Finding colours", true),
                }
            }
        }
        if changed {
            self.changed(now);
        }
    }

    fn clean_up(&mut self, ui: &mut Ui, units: Units, now: f64) {
        let mut changed = slider(ui, "Speckle", &mut self.edits.speckle_sq_in, 0.0..=SPECKLE_MAX_SQ_IN, |v| units.area(v));
        changed |= slider(ui, "Smoothing", &mut self.edits.smoothing, 0.0..=100.0, |v| format!("{}", v.round()));
        changed |= slider(ui, "Corner threshold", &mut self.edits.corner_deg, 0.0..=180.0, |v| format!("{}°", v.round()));
        if changed {
            self.changed(now);
        }
    }

    /// The output choice and the actions, pinned under the steps.
    fn footer(&mut self, ui: &mut Ui, now: f64) -> Option<Event> {
        ui.add_enabled_ui(!self.applying, |ui| {
            flow_kit::divider(ui);
            ui.spacing_mut().item_spacing.y = 8.0;
            caption(ui, "Output");
            let products = [(Product::CutLayers, "Cut layers"), (Product::SingleCut, "Single cut"), (Product::PrintThenCut, "Print Then Cut")];
            if let Some(p) = flow_kit::segmented(ui, self.edits.product, &products) {
                self.edits.product = p;
                self.changed(now);
            }
            if self.edits.product == Product::CutLayers {
                let layering = [(Layering::Stacked, "Stacked"), (Layering::SideBySide, "Side by side")];
                flow_kit::controls(ui, |ui| {
                    caption(ui, "Layering");
                    if let Some(l) = flow_kit::segmented(ui, self.edits.layering, &layering) {
                        self.edits.layering = l;
                        self.changed(now);
                    }
                });
                let note = match self.edits.layering {
                    Layering::Stacked => "Each layer sits under the ones above it, for layered cardstock.",
                    Layering::SideBySide => "Layers butt up without overlapping, for vinyl.",
                };
                flow_kit::body_text(ui, note, true);
            }
        });
        ui.add_space(8.0);
        if self.applying {
            busy_bar(ui, "Tracing");
        }
        let mut event = None;
        flow_kit::actions(ui, |ui| {
            if self.applying {
                if flow_kit::secondary_button(ui, "Cancel", true).clicked() {
                    event = Some(Event::Cancel);
                }
            } else {
                if flow_kit::primary_button(ui, "Apply", self.can_apply()).clicked() {
                    event = Some(Event::Apply);
                }
                if flow_kit::secondary_button(ui, "Discard", true).clicked() {
                    event = Some(Event::Close);
                }
            }
        });
        event
    }

    fn viewport(&mut self, ui: &mut Ui, stale: bool, now: f64) {
        let (area, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let (iw, ih) = (self.source.preview.width().max(1) as f32, self.source.preview.height().max(1) as f32);
        if resp.hovered() {
            let (scroll, zoom, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            self.pan += scroll;
            if zoom != 1.0 {
                let before = self.zoom;
                self.zoom = (self.zoom * zoom).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
                if let Some(p) = pointer {
                    let from_centre = p - (area.center() + self.pan);
                    self.pan -= from_centre * (self.zoom / before - 1.0);
                }
            }
        }
        let fit = ((area.width() - 2.0 * VIEW_MARGIN) / iw).min((area.height() - 2.0 * VIEW_MARGIN) / ih).max(0.01);
        let image = Rect::from_center_size(area.center() + self.pan, vec2(iw, ih) * fit * self.zoom);
        let to_screen = |(x, y): (f32, f32)| pos2(image.left() + x * image.width(), image.top() + y * image.height());
        let to_image = |p: Pos2| ((p.x - image.left()) / image.width(), (p.y - image.top()) / image.height());
        let inside = |(x, y): (f32, f32)| (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y);

        let painter = ui.painter_at(area);
        checkerboard(&painter, image.intersect(area));
        if let Some(t) = &self.shown {
            painter.image(t.id(), image, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
        painter.rect_stroke(image, 0.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Outside);
        for ring in &self.lines {
            let points: Vec<Pos2> = ring.iter().map(|p| to_screen(*p)).collect();
            painter.add(egui::Shape::closed_line(points.clone(), Stroke::new(3.5_f32, Color32::WHITE)));
            painter.add(egui::Shape::closed_line(points, Stroke::new(1.5_f32, ACCENT_PRIMARY)));
        }
        ui.interact(image, ui.id().with("trace-image"), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, "Image to trace"));

        let long_in = self.source.size_in.0.max(self.source.size_in.1);
        let brush_px = self.brush_in / long_in * image.width().max(image.height());
        let erase = self.tool == Tool::Erase;
        match self.tool {
            Tool::RemoveColour => {
                if resp.clicked()
                    && let Some(at) = resp.interact_pointer_pos().map(to_image).filter(|p| inside(*p))
                {
                    let (tolerance, contiguous) = (self.tolerance, self.contiguous);
                    self.edit_recipe(now, |r| r.steps.push(Step::RemoveColour { at, tolerance, contiguous }));
                }
                if resp.dragged() {
                    self.pan += resp.drag_delta();
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
            }
            Tool::Erase | Tool::Restore => {
                let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
                if (resp.drag_started() || resp.clicked() || resp.dragged())
                    && let Some(p) = resp.interact_pointer_pos()
                {
                    self.stroke.get_or_insert_with(Vec::new).push(to_image(p));
                }
                if (resp.drag_stopped() || resp.clicked())
                    && let Some(path) = self.stroke.take()
                {
                    let radius = self.brush_in / long_in;
                    self.edit_recipe(now, |r| r.steps.push(Step::Brush { erase, radius, path }));
                }
                let ink = if erase { Color32::from_rgba_unmultiplied(0xff, 0x40, 0x40, 0x60) } else { Color32::from_rgba_unmultiplied(0x70, 0x48, 0xe8, 0x60) };
                if let Some(path) = &self.stroke {
                    for p in path {
                        painter.circle_filled(to_screen(*p), brush_px, ink);
                    }
                }
                if let Some(p) = pointer {
                    painter.circle_stroke(p, brush_px, Stroke::new(1.0_f32, TEXT_PRIMARY));
                }
            }
        }
        if stale {
            spinner(ui, pos2(area.left() + 24.0, area.top() + 24.0), now);
        }
        self.zoom_controls(ui, area);
    }

    fn zoom_controls(&mut self, ui: &mut Ui, area: Rect) {
        let pill = Rect::from_min_size(pos2(area.left() + 16.0, area.bottom() - 16.0 - 36.0), vec2(124.0, 36.0));
        ui.painter().rect(pill, 18.0, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
        let mut zoom = self.zoom;
        ui.scope_builder(egui::UiBuilder::new().max_rect(pill.shrink2(vec2(6.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if icon_button(ui, Icon::Minus, "Zoom out", zoom > ZOOM_RANGE.0).clicked() {
                zoom /= 1.5;
            }
            let (rect, fit) = ui.allocate_exact_size(vec2(56.0, 28.0), Sense::click());
            let fit = tip(fit, "Fit", "Show the whole image.", None);
            fit.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Fit image"));
            text(ui.painter(), rect.center(), Align2::CENTER_CENTER, &format!("{}%", (zoom * 100.0).round()), fonts::regular(13.0), TEXT_PRIMARY);
            if fit.clicked() {
                zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
            if icon_button(ui, Icon::Plus, "Zoom in", zoom < ZOOM_RANGE.1).clicked() {
                zoom *= 1.5;
            }
        });
        self.zoom = zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
    }
}

fn step_name(step: &Step, i: usize) -> String {
    let n = i + 1;
    match step {
        Step::RemoveColour { tolerance, contiguous: true, .. } => format!("{n}. Colour, tolerance {}", tolerance.round()),
        Step::RemoveColour { tolerance, contiguous: false, .. } => format!("{n}. Colour everywhere, tolerance {}", tolerance.round()),
        Step::AutoRemove => format!("{n}. Auto remove"),
        Step::Brush { erase: true, .. } => format!("{n}. Erase"),
        Step::Brush { erase: false, .. } => format!("{n}. Restore"),
    }
}

/// The title bar: the panel's name, the image's size and a close button.
fn header(ctx: &Context, px: (u32, u32)) -> Option<Event> {
    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(Margin::symmetric(16, 6));
    egui::TopBottomPanel::top("trace-header")
        .exact_height(TOP_BAR_H)
        .frame(frame)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let full = ui.max_rect().expand2(vec2(16.0, 6.0));
            ui.painter().hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            ui.horizontal_centered(|ui| {
                ui.add_space(TRAFFIC_LIGHT_PAD - 16.0);
                ui.label(RichText::new("Trace image").font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{} x {} px", px.0, px.1)).font(fonts::regular(13.0)).color(TEXT_MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    icon_button(ui, Icon::Close, "Close without adding", true).clicked().then_some(Event::Close)
                })
                .inner
            })
            .inner
        })
        .inner
}

/// A collapsible step, open until the user folds it.
fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let id = resp.id.with("open");
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true) != resp.clicked();
    ui.data_mut(|d| d.insert_temp(id, open));
    resp.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, title));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4, ACCENT_HOVER);
    }
    let turn = if open { 0.0 } else { -std::f32::consts::FRAC_PI_2 };
    paint_icon_rotated(ui, Icon::Chevron, Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(16.0)), TEXT_PRIMARY, turn);
    text(ui.painter(), pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, title, fonts::semibold(14.0), TEXT_PRIMARY);
    if open {
        Frame::NONE.inner_margin(Margin { left: 8, right: 0, top: 0, bottom: 4 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            body(ui);
        });
    }
}

fn caption(ui: &mut Ui, words: &str) {
    ui.label(RichText::new(words).font(fonts::medium(13.0)).color(TEXT_PRIMARY));
}

/// A caption and its value on one line, the slider under them. True while it moves.
fn slider(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, shown: impl Fn(f32) -> String) -> bool {
    let shown = shown(*value);
    ui.horizontal(|ui| {
        caption(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(&shown).font(fonts::regular(13.0)).color(TEXT_MUTED)));
    });
    let enabled = ui.is_enabled();
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let track = rect.shrink2(vec2(8.0, 0.0));
    let (lo, hi) = (*range.start(), *range.end());
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let next = lo + t * (hi - lo);
        if next != *value {
            *value = next;
            resp.mark_changed();
        }
    }
    let v = f64::from(*value);
    resp.widget_info(|| WidgetInfo::slider(enabled, v, label));
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let knob = pos2(track.left() + t * track.width(), track.center().y);
    let painter = ui.painter();
    let ink = if enabled { ACCENT_PRIMARY } else { MAT_GREY };
    painter.rect_filled(Rect::from_min_max(pos2(track.left(), knob.y - 2.0), pos2(track.right(), knob.y + 2.0)), 2, BORDER_SUBTLE);
    painter.rect_filled(Rect::from_min_max(pos2(track.left(), knob.y - 2.0), pos2(knob.x, knob.y + 2.0)), 2, ink);
    painter.circle(knob, 7.0, BG_PANEL, Stroke::new(if resp.hovered() || resp.dragged() { 2.0_f32 } else { 1.5 }, ink));
    resp.changed()
}

/// The Posterise palette. Clicking a swatch merges it into its nearest neighbour; clicking a merged
/// one brings it back. The last live swatch cannot be merged. True when the palette changed.
fn swatches(ui: &mut Ui, palette: &mut Palette) -> bool {
    let mut toggled = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        let live = palette.live().len();
        for (i, swatch) in palette.swatches.iter().enumerate() {
            let merged = palette.merged.contains(&i);
            let can = merged || live > 1;
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), if can { Sense::click() } else { Sense::hover() });
            let label = format!("Colour {}", i + 1);
            resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, can, !merged, &label));
            let resp = if merged {
                tip(resp, &label, "Merged into its nearest colour. Click to bring it back.", None)
            } else {
                tip(resp, &label, "Click to merge into its nearest colour.", None)
            };
            let fill = if merged { swatch.colour.gamma_multiply(0.35) } else { swatch.colour };
            ui.painter().rect(rect, 4, fill, Stroke::new(1.0_f32, if resp.hovered() { ACCENT_PRIMARY } else { BORDER_SUBTLE }), StrokeKind::Inside);
            if merged {
                ui.painter().line_segment([rect.left_bottom() + vec2(4.0, -4.0), rect.right_top() + vec2(-4.0, 4.0)], Stroke::new(1.5_f32, TEXT_PRIMARY));
            }
            if resp.clicked() {
                toggled = Some(i);
            }
        }
    });
    let Some(i) = toggled else { return false };
    match palette.merged.iter().position(|&m| m == i) {
        Some(at) => _ = palette.merged.remove(at),
        None => palette.merged.push(i),
    }
    true
}

fn checkerboard(painter: &egui::Painter, rect: Rect) {
    if !rect.is_positive() {
        return;
    }
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let (cols, rows) = ((rect.width() / CHECKER).ceil() as usize, (rect.height() / CHECKER).ceil() as usize);
    for row in 0..rows {
        for col in (row % 2..cols).step_by(2) {
            let min = rect.min + vec2(col as f32, row as f32) * CHECKER;
            painter.rect_filled(Rect::from_min_size(min, Vec2::splat(CHECKER)).intersect(rect), 0.0, CHECKER_DARK);
        }
    }
}

fn spinner(ui: &mut Ui, centre: Pos2, now: f64) {
    let rect = Rect::from_center_size(centre, Vec2::splat(28.0));
    ui.painter().circle(centre, 14.0, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE));
    let start = (now * 4.0) as f32;
    let points: Vec<Pos2> = (0..=16).map(|i| start + i as f32 / 16.0 * 4.5).map(|a| centre + 8.0 * vec2(a.cos(), a.sin())).collect();
    ui.painter().add(egui::Shape::line(points, Stroke::new(2.0_f32, ACCENT_PRIMARY)));
    ui.interact(rect, ui.id().with("trace-spinner"), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::ProgressIndicator, true, "Updating preview"));
}

/// An indeterminate bar: the full trace reports no progress, so a segment sweeps the track.
fn busy_bar(ui: &mut Ui, label: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::ProgressIndicator, true, label));
    let track = Rect::from_min_max(pos2(rect.left(), rect.center().y - 4.0), pos2(rect.right() - 72.0, rect.center().y + 4.0));
    let painter = ui.painter();
    painter.rect_filled(track, 4, BORDER_SUBTLE);
    let t = (ui.input(|i| i.time) * 0.8).fract() as f32;
    let w = track.width() * 0.3;
    let x0 = track.left() - w + t * (track.width() + w);
    let segment = Rect::from_min_max(pos2(x0.max(track.left()), track.top()), pos2((x0 + w).min(track.right()), track.bottom()));
    painter.rect_filled(segment, 4, ACCENT_PRIMARY);
    text(painter, pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, label, fonts::medium(13.0), TEXT_PRIMARY);
    ui.add_space(8.0);
    resp
}

fn map_contour(c: &Contour, f: impl Fn(Pt) -> Pt) -> Contour {
    let seg = |s: &Seg| match *s {
        Seg::Line(a) => Seg::Line(f(a)),
        Seg::Cubic(a, b, c) => Seg::Cubic(f(a), f(b), f(c)),
    };
    Contour { start: f(c.start), segs: c.segs.iter().map(seg).collect(), closed: c.closed }
}

/// One Basic Cut shape from pixel contours: scaled to inches once, here, and placed at their bounds.
fn cut_shape(contours: &[Contour], scale: Pt, fill: Color32) -> Option<Shape> {
    let inches: Vec<Contour> = contours.iter().map(|c| map_contour(c, |(x, y)| (x * scale.0, y * scale.1))).collect();
    let points: Vec<Pt> = inches.iter().flat_map(|c| crate::geometry::flatten(c, 0.005)).collect();
    let (x0, y0, x1, y1) = points.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)));
    if points.len() < 3 || x1 <= x0 || y1 <= y0 {
        return None;
    }
    let local = inches.iter().map(|c| map_contour(c, |(x, y)| (x - x0, y - y0))).collect();
    let view_box = (x1 - x0, y1 - y0);
    Some(Shape {
        x_in: x0,
        y_in: y0,
        width_in: view_box.0,
        height_in: view_box.1,
        fill,
        operation: Operation::BasicCut,
        pen: crate::pens::Pen::nearest(fill, Default::default()),
        ..Shape::new(0, ShapeKind::Imported { contours: local, view_box })
    })
}

/// What Apply places for `traced`, an image shown `size_in` big at the origin, and whether to group it.
fn placement(traced: Traced, size_in: Pt, stored: Option<AssetId>) -> Placement {
    let scale = (size_in.0 / traced.px.0 as f32, size_in.1 / traced.px.1 as f32);
    let shapes = match (traced.product, stored) {
        (Product::PrintThenCut, Some(asset)) => {
            let clip = traced.layers.into_iter().flat_map(|l| l.contours).collect();
            vec![Shape { width_in: size_in.0, height_in: size_in.1, ..Shape::new(0, ShapeKind::Bitmap { asset, px: traced.px, clip }) }]
        }
        _ => traced.layers.iter().filter_map(|l| cut_shape(&l.contours, scale, l.colour)).collect(),
    };
    Placement { shapes, group: true }
}

impl Kerf {
    /// The trace panel in place of the canvas while an upload is being traced.
    pub(super) fn trace_screen(&mut self, ctx: &Context) {
        self.collect_upload(ctx);
        let Some(panel) = self.doc.trace.as_mut() else { return };
        // The panel submits previews itself, and Apply submits the full trace below.
        self.image_owner = Some(self.doc.autosave.id.clone());
        match panel.show(ctx, &mut self.images, self.units) {
            Some(Event::Close) => {
                self.images.cancel();
                self.doc.trace = None;
                tracing::info!(target: "cmd", "trace panel closed without adding");
            }
            Some(Event::Apply) => {
                let job = panel.job(&panel.source.image.clone());
                tracing::info!(target: "cmd", product = ?job.product, px = ?panel.source.px, "trace apply");
                panel.applying = true;
                panel.dirty_since = None;
                self.images.submit(Job::Full(job));
            }
            Some(Event::Cancel) => {
                self.images.cancel();
                panel.applying = false;
                tracing::info!(target: "flow", "full trace cancelled");
            }
            None => {}
        }
    }

    /// Places a finished full trace as one undo step and closes the panel. Runs before
    /// `History::observe`.
    pub(super) fn place_trace(&mut self, ctx: &Context, mut traced: Traced) {
        let Some(panel) = self.doc.trace.take() else { return };
        let stored = traced.stored.take().map(|s| {
            if let Some(textures) = crate::assets::Textures::of(ctx) {
                textures.insert(ctx, &s.asset, s.display, crate::assets::Wrap::Clamp);
            }
            s.asset
        });
        tracing::info!(target: "flow", layers = traced.layers.len(), px = ?traced.px, "full trace done");
        let place = Edit::Place(placement(traced, panel.source.size_in, stored));
        if let Err(blocked) = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &place, Subject::Selection) {
            self.report(format!("Couldn't add the trace. {}", blocked.reason()));
        }
    }
}

#[cfg(test)]
impl TracePanel {
    /// The preview has been asked for since the last edit; with the worker idle, it shows them.
    pub(super) fn settled(&self) -> bool {
        self.dirty_since.is_none()
    }

    pub(super) fn cut_lines(&self) -> usize {
        self.lines.len()
    }

    pub(super) fn swatch_count(&self) -> Option<usize> {
        match &self.edits.mode {
            Mode::Posterise { palette, .. } => palette.as_ref().map(|p| p.swatches.len()),
            Mode::Threshold { .. } => None,
        }
    }

    pub(super) fn background_steps(&self) -> usize {
        self.edits.recipe.steps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::TracedLayer;

    fn square(x0: f32, y0: f32, side: f32) -> Contour {
        let p = [(x0 + side, y0), (x0 + side, y0 + side), (x0, y0 + side)];
        Contour { start: (x0, y0), segs: p.iter().map(|&q| Seg::Line(q)).collect(), closed: true }
    }

    #[test]
    fn cut_layers_scale_pixels_to_inches_at_their_bounds_and_group() {
        let traced = Traced {
            px: (200, 100),
            layers: vec![
                TracedLayer { colour: Color32::RED, contours: vec![square(0.0, 0.0, 200.0)] },
                TracedLayer { colour: Color32::BLUE, contours: vec![square(50.0, 20.0, 40.0)] },
            ],
            product: Product::CutLayers,
            stored: None,
        };
        let p = placement(traced, (4.0, 2.0), None);
        assert!(p.group);
        assert_eq!(p.shapes.len(), 2);
        let blue = &p.shapes[1];
        assert_eq!((blue.fill, blue.operation), (Color32::BLUE, Operation::BasicCut));
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(close(blue.x_in, 1.0) && close(blue.y_in, 0.4), "{} {}", blue.x_in, blue.y_in);
        assert!(close(blue.width_in, 0.8) && close(blue.height_in, 0.8));
    }

    #[test]
    fn print_then_cut_is_one_bitmap_clipped_to_the_outline() {
        let traced = Traced {
            px: (200, 100),
            layers: vec![TracedLayer { colour: Color32::BLACK, contours: vec![square(10.0, 10.0, 50.0)] }],
            product: Product::PrintThenCut,
            stored: None,
        };
        let asset = crate::assets::id_of(&RgbaImage::new(1, 1));
        let p = placement(traced, (4.0, 2.0), Some(asset.clone()));
        let [image] = &p.shapes[..] else { panic!("one shape") };
        assert_eq!((image.width_in, image.height_in, image.operation), (4.0, 2.0, Operation::PrintThenCut));
        let ShapeKind::Bitmap { asset: a, px, clip } = &image.kind else { panic!("a Bitmap") };
        assert_eq!((a, *px, clip.len()), (&asset, (200, 100), 1));
    }

    #[test]
    fn speckle_covers_the_same_area_at_every_resolution() {
        let img = Arc::new(RgbaImage::new(1000, 500));
        let imported = Imported { asset: crate::assets::id_of(&img), px: (1000, 500), image: img.clone(), preview: Arc::new(RgbaImage::new(768, 384)) };
        let mut panel = TracePanel::open(&imported);
        panel.edits.speckle_sq_in = 0.01;
        assert_eq!(panel.settings(1000).speckle_px, 17, "0.1 in at 1000 px across 6 in");
        assert_eq!(panel.settings(768).speckle_px, 13);
    }
}
