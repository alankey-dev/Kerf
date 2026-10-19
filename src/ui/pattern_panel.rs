//! Pattern fill (spec 34): the colour swatch's picker with its Pattern tab, the swatch's pattern chip, and
//! Edit pattern, which moves, scales and turns a shape's pattern on the canvas without moving the shape.

use std::sync::Arc;

use egui::{Color32, Context, Frame, Id, Margin, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use super::Kerf;
use super::canvas::Grid;
use super::flow_kit;
use super::tokens::*;
use crate::assets::{Texture, Textures, Wrap};
use crate::design::{Design, Pt, Shape};
use crate::edit::{Edit, Subject};
use crate::effects::Placement;
use crate::patterns::{Bundled, COLOURWAYS, Colourway, MIN_SCALE, PatternFill, PatternMode, placement};
use crate::trace::worker::{Answer, Done, Job, Source};
use crate::units::{self, Units, length_field, number_field};

/// What the picker needs the app for: storing a bundled pattern's image, or picking a file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Asked {
    Bundled(Bundled, &'static Colourway),
    Upload,
}

/// What a picked pattern fills: the selection, or one part (by master shape id) of the selected instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Selection,
    Part(u64),
}

impl Target {
    /// The edit that fills the target with `fill`, or with `None` prints its colour again.
    pub fn edit(self, fill: Option<PatternFill>) -> Edit {
        match self {
            Target::Selection => Edit::Pattern(fill),
            Target::Part(part) => Edit::Override { part, change: crate::components::PartChange::Pattern(fill) },
        }
    }
}

/// What one frame of the picker chose.
#[derive(Default)]
pub struct Picked {
    pub colour: Option<Color32>,
    pub edit: Option<Edit>,
    pub asked: Option<Asked>,
}

const PICKER_W: f32 = 268.0;
const TILE: f32 = 52.0;
/// A swatch thumbnail's side in pixels.
const THUMB_PX: u32 = 64;

/// The pattern every shape in `shapes` shares, if they share one.
pub fn shared<'a>(shapes: &[&'a Shape]) -> Option<&'a PatternFill> {
    let first = shapes.first()?.pattern.as_ref()?;
    shapes.iter().all(|s| s.pattern.as_ref() == Some(first)).then_some(first)
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Colour,
    Pattern,
}

/// The picker in a popover beside the swatch: an RGB picker, and for shapes that can take one a Pattern
/// tab of bundled patterns, Upload image, and the shared pattern's settings.
#[expect(clippy::too_many_arguments, reason = "the swatch's popover state, plus the design and palette the colour picker reads")]
pub fn picker(
    ui: &Ui,
    swatch: &Response,
    fill: Option<Color32>,
    printable: bool,
    current: Option<&PatternFill>,
    units: Units,
    design: &Design,
    palette: &mut super::colour_picker::Palette,
) -> Picked {
    let popup = swatch.id.with("picker");
    if swatch.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
        let tab = if current.is_some() { Tab::Pattern } else { Tab::Colour };
        ui.data_mut(|d| d.insert_temp(popup.with("tab"), tab));
    }
    let mut picked = Picked::default();
    if !ui.memory(|m| m.is_popup_open(popup)) {
        return picked;
    }
    let mut tab = ui.data(|d| d.get_temp(popup.with("tab"))).filter(|_| printable).unwrap_or(Tab::Colour);
    let frame = Frame::NONE.fill(BG_PANEL).corner_radius(4).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).inner_margin(Margin::same(12)).shadow(egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: SHADOW,
    });
    // Beside the inspector rather than over it, so the swatch and Edit pattern stay in view.
    let at = pos2((swatch.rect.left() - PICKER_W - 20.0).max(0.0), swatch.rect.top());
    let area = egui::Area::new(popup).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        frame.show(ui, |ui| {
            ui.set_width(PICKER_W - 24.0);
            if printable && let Some(t) = flow_kit::segmented(ui, tab, &[(Tab::Colour, "Solid colour"), (Tab::Pattern, "Pattern")]) {
                tab = t;
                ui.data_mut(|d| d.insert_temp(popup.with("tab"), t));
            }
            ui.add_space(8.0);
            match tab {
                Tab::Colour => {
                    if let Some(c) = super::colour_picker::show(ui, fill, design, palette) {
                        picked.colour = Some(c);
                    }
                }
                Tab::Pattern => pattern_tab(ui, current, units, &mut picked),
            }
        })
    });
    let outside = ui.ctx().input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !swatch.rect.contains(p) && !area.response.rect.contains(p)));
    if outside {
        ui.memory_mut(|m| m.close_popup());
    }
    picked
}

fn pattern_tab(ui: &mut Ui, current: Option<&PatternFill>, units: Units, picked: &mut Picked) {
    let thumbs = thumbnails(ui.ctx());
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    for (row, &pattern) in Bundled::ALL.iter().enumerate() {
        ui.horizontal(|ui| {
            for (col, colourway) in COLOURWAYS.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::click());
                let name = format!("{}, {}", pattern.label(), colourway.name);
                resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &name));
                let resp = super::tooltip::tip(resp, pattern.label(), &format!("{} colourway. Fills the selection with this repeating pattern.", colourway.name), None);
                let texture = &thumbs[row * COLOURWAYS.len() + col];
                ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                let edge = if resp.hovered() { Stroke::new(2.0_f32, ACCENT_PRIMARY) } else { Stroke::new(1.0_f32, BORDER_SUBTLE) };
                ui.painter().rect_stroke(rect, 3.0, edge, egui::StrokeKind::Inside);
                if resp.clicked() {
                    picked.asked = Some(Asked::Bundled(pattern, colourway));
                }
            }
        });
    }
    ui.add_space(4.0);
    if flow_kit::secondary_button(ui, "Upload image", true).clicked() {
        picked.asked = Some(Asked::Upload);
    }
    let Some(fill) = current else { return };
    ui.add_space(4.0);
    flow_kit::divider(ui);
    let mut next = fill.clone();
    if let Some(mode) = flow_kit::segmented(ui, fill.mode, &PatternMode::ALL.map(|m| (m, m.label()))) {
        (next.mode, next.scale) = (mode, 1.0);
    }
    let half = (ui.available_width() - 6.0) / 2.0;
    match fill.mode {
        PatternMode::Tile => {
            if let Some(typed) = row(ui, "Tile width", |ui, w| length_field(ui, "Tile width", Some(fill.scale), units, w)) {
                next.scale = typed.apply(fill.scale).max(MIN_SCALE);
            }
        }
        _ => {
            if let Some(typed) = row(ui, "Scale", |ui, w| number_field(ui, "Pattern scale", Some(fill.scale * 100.0), "%", w)) {
                next.scale = (typed.apply(fill.scale * 100.0) / 100.0).max(MIN_SCALE);
            }
        }
    }
    if let Some(typed) = row(ui, "Rotation", |ui, w| number_field(ui, "Pattern rotation", Some(fill.rotation_deg), "°", w)) {
        next.rotation_deg = typed.apply(fill.rotation_deg).rem_euclid(360.0);
    }
    ui.horizontal(|ui| {
        if let Some(typed) =
            ui.allocate_ui(vec2(half, units::FIELD_H), |ui| row(ui, "X", |ui, w| length_field(ui, "Pattern offset X", Some(fill.offset_in.0), units, w))).inner
        {
            next.offset_in.0 = typed.apply(fill.offset_in.0);
        }
        if let Some(typed) =
            ui.allocate_ui(vec2(half, units::FIELD_H), |ui| row(ui, "Y", |ui, w| length_field(ui, "Pattern offset Y", Some(fill.offset_in.1), units, w))).inner
        {
            next.offset_in.1 = typed.apply(fill.offset_in.1);
        }
    });
    if next != *fill {
        picked.edit = Some(Edit::Pattern(Some(next)));
    }
    ui.add_space(4.0);
    if flow_kit::secondary_button(ui, "Remove pattern", true).clicked() {
        picked.edit = Some(Edit::Pattern(None));
    }
}

/// A muted caption and the field `body` draws in the width left.
fn row<R>(ui: &mut Ui, label: &str, body: impl FnOnce(&mut Ui, f32) -> R) -> R {
    ui.horizontal(|ui| {
        let caption_w = if label.len() > 1 { 64.0 } else { 14.0 };
        let (r, _) = ui.allocate_exact_size(vec2(caption_w, units::FIELD_H), Sense::hover());
        super::chrome::text(ui.painter(), r.left_center(), egui::Align2::LEFT_CENTER, label, super::fonts::regular(11.0), TEXT_MUTED);
        let w = ui.available_width();
        body(ui, w)
    })
    .inner
}

/// Every bundled pattern in every colourway as a small texture, made once per context.
fn thumbnails(ctx: &Context) -> Arc<Vec<egui::TextureHandle>> {
    let id = Id::new("pattern-thumbnails");
    if let Some(made) = ctx.data(|d| d.get_temp::<Arc<Vec<egui::TextureHandle>>>(id)) {
        return made;
    }
    let made: Vec<egui::TextureHandle> = Bundled::ALL
        .iter()
        .flat_map(|&p| COLOURWAYS.iter().map(move |c| (p, c)))
        .map(|(p, c)| {
            let img = crate::patterns::thumbnail(p, c, THUMB_PX);
            let image = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            ctx.load_texture(format!("pattern-{}-{}", p.label(), c.name), image, egui::TextureOptions::LINEAR)
        })
        .collect();
    let made = Arc::new(made);
    ctx.data_mut(|d| d.insert_temp(id, made.clone()));
    made
}

/// The swatch's chip showing `fill`'s image, one tile across. Grey while the image loads.
pub fn paint_chip(ui: &Ui, chip: Rect, fill: &PatternFill) {
    let texture = Textures::of(ui.ctx()).map(|t| t.get(&fill.asset, Wrap::Repeat));
    match texture {
        Some(Texture::Ready(t)) => {
            ui.painter().image(t.id(), chip, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0 / fill.aspect)), Color32::WHITE);
        }
        _ => _ = ui.painter().rect_filled(chip, 3.0, MAT_GREY),
    }
    ui.painter().rect_stroke(chip, 3.0, Stroke::new(1.0_f32, BORDER_SUBTLE), egui::StrokeKind::Inside);
}

fn editing_id() -> Id {
    Id::new("edit-pattern")
}

/// The shape whose pattern Edit pattern is moving: on while that shape is the whole selection and still
/// carries a pattern it can print.
pub fn editing(ctx: &Context, design: &Design, selection: &[u64]) -> Option<u64> {
    let id = ctx.data(|d| d.get_temp::<u64>(editing_id()))?;
    let ok = selection == [id] && design.shapes.iter().any(|s| s.id == id && s.pattern.is_some() && s.takes_pattern());
    ok.then_some(id)
}

/// The Edit pattern switch under the swatch, shown for one pattern-filled shape.
pub fn edit_toggle(ui: &mut Ui, design: &Design, selection: &[u64]) {
    let [id] = selection else { return };
    if !design.shapes.iter().any(|s| s.id == *id && s.pattern.is_some() && s.takes_pattern()) {
        return;
    }
    let mut on = editing(ui.ctx(), design, selection).is_some();
    let resp = flow_kit::toggle_row(ui, "Edit pattern", &mut on);
    super::tooltip::tip(
        resp.clone(),
        "Edit pattern",
        "Drag inside the shape to move its pattern; drag the frame's handles to scale and turn it. The shape stays put.",
        None,
    );
    if resp.changed() {
        tracing::info!(target: "cmd", shape = id, on, "edit pattern");
        ui.data_mut(|d| match on {
            true => d.insert_temp(editing_id(), *id),
            false => d.remove::<u64>(editing_id()),
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Move,
    Scale,
    Rotate,
}

#[derive(Clone)]
struct Drag {
    grab: Grab,
    from: PatternFill,
    /// Where the press landed, in the shape's own frame.
    start: Pt,
}

/// The pattern's frame on the canvas: the image's corners, or one tile's, in world inches.
struct FrameIn {
    corners: [Pt; 3],
    placement: Placement,
}

impl FrameIn {
    fn of(shape: &Shape, fill: &PatternFill) -> FrameIn {
        let at = Placement::of(shape);
        FrameIn { corners: placement(fill, (shape.width_in, shape.height_in)).corners.map(at.placer()), placement: at }
    }

    fn screen(&self, grid: &Grid) -> [Pos2; 4] {
        let [tl, tr, bl] = self.corners;
        [tl, tr, (tr.0 + bl.0 - tl.0, tr.1 + bl.1 - tl.1), bl].map(|p| grid.to_screen(p))
    }

    /// The scale handle at the bottom-right corner and the rotate handle above the top edge, on screen.
    fn handles(&self, grid: &Grid) -> (Pos2, Pos2) {
        let [tl, tr, br, bl] = self.screen(grid);
        let up = (tl - bl).normalized();
        (br, tl.lerp(tr, 0.5) + up * ROTATE_GAP_PX)
    }
}

const ROTATE_GAP_PX: f32 = 22.0;
const HANDLE_HIT_PX: f32 = 9.0;

/// Edit pattern's canvas input. Returns whether it took the pointer this frame, so the canvas does not
/// also select or move. A press outside the shape and its handles leaves Edit pattern.
pub fn canvas_input(ui: &Ui, response: &Response, design: &mut Design, selection: &mut Vec<u64>, grid: &Grid) -> bool {
    let ctx = ui.ctx();
    let Some(id) = editing(ctx, design, selection) else {
        ctx.data_mut(|d| d.remove::<u64>(editing_id()));
        return false;
    };
    let drag_id = editing_id().with("drag");
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<u64>(editing_id()));
        return true;
    }
    let Some(shape) = design.shapes.iter().find(|s| s.id == id) else { return false };
    let Some(fill) = shape.pattern.clone() else { return false };
    let frame = FrameIn::of(shape, &fill);
    let (scale_at, rotate_at) = frame.handles(grid);
    let near = |p: Pos2, h: Pos2| p.distance(h) <= HANDLE_HIT_PX;
    let grab_at = |p: Pos2| {
        if near(p, scale_at) {
            Some(Grab::Scale)
        } else if near(p, rotate_at) {
            Some(Grab::Rotate)
        } else {
            design.geometry(shape).contains(grid.to_inches(p)).then_some(Grab::Move)
        }
    };
    let pressed = ui.input(|i| i.pointer.press_origin()).filter(|_| response.drag_started() || response.clicked());
    if let Some(p) = pressed
        && grab_at(p).is_none()
    {
        ctx.data_mut(|d| d.remove::<u64>(editing_id()));
        return false;
    }
    if response.drag_started()
        && let Some(p) = pressed
        && let Some(grab) = grab_at(p)
    {
        ctx.data_mut(|d| d.insert_temp(drag_id, Drag { grab, from: fill.clone(), start: frame.placement.to_local(grid.to_inches(p)) }));
    }
    if let Some(hover) = response.hover_pos() {
        let icon = match grab_at(hover) {
            Some(Grab::Move) => egui::CursorIcon::Grab,
            Some(Grab::Scale) => egui::CursorIcon::ResizeNwSe,
            Some(Grab::Rotate) => egui::CursorIcon::Alias,
            None => egui::CursorIcon::Default,
        };
        ctx.set_cursor_icon(icon);
    }
    let size = (shape.width_in, shape.height_in);
    let drag = ctx.data(|d| d.get_temp::<Drag>(drag_id));
    if let (Some(drag), true, Some(p)) = (&drag, response.dragged(), response.interact_pointer_pos()) {
        let to = frame.placement.to_local(grid.to_inches(p));
        let moved = dragged(drag, to, size);
        if moved != fill {
            _ = crate::edit::run(design, selection, &Edit::Pattern(Some(moved)), Subject::Selection);
        }
    }
    if response.drag_stopped() {
        ctx.data_mut(|d| d.remove::<Drag>(drag_id));
    }
    true
}

/// `drag.from` after the pointer moved from `drag.start` to `to`, both in the shape's own frame.
fn dragged(drag: &Drag, to: Pt, (w, h): Pt) -> PatternFill {
    let from = &drag.from;
    let centre = (w / 2.0 + from.offset_in.0, h / 2.0 + from.offset_in.1);
    let rel = |p: Pt| (p.0 - centre.0, p.1 - centre.1);
    let (a, b) = (rel(drag.start), rel(to));
    match drag.grab {
        Grab::Move => PatternFill { offset_in: (from.offset_in.0 + to.0 - drag.start.0, from.offset_in.1 + to.1 - drag.start.1), ..from.clone() },
        Grab::Scale => {
            let k = b.0.hypot(b.1) / a.0.hypot(a.1).max(1e-6);
            PatternFill { scale: (from.scale * k).max(MIN_SCALE), ..from.clone() }
        }
        Grab::Rotate => {
            let turn = (b.1.atan2(b.0) - a.1.atan2(a.0)).to_degrees();
            PatternFill { rotation_deg: (from.rotation_deg + turn).rem_euclid(360.0), ..from.clone() }
        }
    }
}

/// Edit pattern's chrome in place of the selection's: the pattern's frame, its scale handle and its
/// rotate handle.
pub fn paint(painter: &Painter, grid: &Grid, design: &Design, id: u64) {
    let Some(shape) = design.shapes.iter().find(|s| s.id == id) else { return };
    let Some(fill) = &shape.pattern else { return };
    let frame = FrameIn::of(shape, fill);
    let corners = frame.screen(grid);
    let stroke = Stroke::new(1.5_f32, ACCENT_PRIMARY);
    painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
    let (scale_at, rotate_at) = frame.handles(grid);
    painter.line_segment([corners[0].lerp(corners[1], 0.5), rotate_at], Stroke::new(1.0_f32, ACCENT_PRIMARY));
    super::canvas::paint_anchor(painter, scale_at);
    super::canvas::paint_round_handle(painter, rotate_at);
}

impl Kerf {
    /// Does what the picker asked: stores a bundled pattern and fills `target` with it, keeping the
    /// selection's shared settings, or picks an image for it.
    pub(super) fn pattern_asked(&mut self, ctx: &Context, asked: Asked, target: Target) {
        match asked {
            Asked::Bundled(pattern, colourway) => {
                let Some(dir) = self.projects_dir.as_deref().map(crate::assets::dir) else {
                    return self.report("Couldn't add the pattern: there is no projects folder to keep it in.".into());
                };
                let img = crate::patterns::generate(pattern, colourway);
                let asset = match crate::assets::store(&dir, &img) {
                    Ok(asset) => asset,
                    Err(e) => return self.report(format!("Couldn't add the pattern: {e}")),
                };
                if let Some(textures) = Textures::of(ctx) {
                    let shown = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    textures.insert(ctx, &asset, shown, Wrap::Repeat);
                }
                let shapes: Vec<&Shape> = self.doc.selection.iter().filter_map(|id| self.doc.design.shapes.iter().find(|s| s.id == *id)).collect();
                let fill = match shared(&shapes).filter(|_| target == Target::Selection) {
                    Some(kept) if kept.mode == PatternMode::Tile => PatternFill { asset, aspect: 1.0, ..kept.clone() },
                    _ => PatternFill::new(asset, 1.0, PatternMode::Tile),
                };
                self.run_pattern(target.edit(Some(fill)), None);
            }
            Asked::Upload => self.upload_pattern(target),
        }
    }

    /// Asks for an image (or takes the injected `upload_file`) and has the image worker store it; the
    /// pattern lands on today's selection when the worker answers.
    fn upload_pattern(&mut self, target: Target) {
        let picked = self.upload_file.clone().or_else(|| rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file());
        let Some(path) = picked else { return };
        tracing::info!(target: "cmd", path = %path.display(), "pattern upload");
        self.doc.pattern_upload = Some((self.doc.selection.clone(), target));
        self.image_owner = Some(self.doc.autosave.id.clone());
        self.images.submit(Job::Import(Source::Path(path)));
    }

    /// The worker's answer to [`Self::upload_pattern`]: the stored image fills the shapes it was for.
    pub(super) fn pattern_landed(&mut self, on: Vec<u64>, target: Target, done: Done) {
        match done.result {
            Ok(Answer::Imported(imported)) => {
                let (w, h) = imported.px;
                let fill = PatternFill::new(imported.asset, w as f32 / h.max(1) as f32, PatternMode::Fill);
                self.run_pattern(target.edit(Some(fill)), Some(&on));
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!(target: "error", error = %e, "pattern upload failed");
                self.report(format!("Couldn't upload the image: {e}."));
            }
        }
    }

    fn run_pattern(&mut self, edit: Edit, on: Option<&[u64]>) {
        let subject = on.map_or(Subject::Selection, Subject::Ids);
        if let Err(blocked) = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &edit, subject) {
            self.report(blocked.reason());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill() -> PatternFill {
        PatternFill::new(crate::assets::id_of(&image::RgbaImage::new(1, 1)), 1.0, PatternMode::Tile)
    }

    #[test]
    fn dragging_moves_scales_and_turns_the_pattern_about_its_centre() {
        let from = PatternFill { scale: 0.5, ..fill() };
        let at = |grab, start, to| dragged(&Drag { grab, from: from.clone(), start }, to, (2.0, 2.0));
        assert_eq!(at(Grab::Move, (1.0, 1.0), (1.25, 0.5)).offset_in, (0.25, -0.5));
        assert!((at(Grab::Scale, (1.5, 1.0), (2.0, 1.0)).scale - 1.0).abs() < 1e-5, "twice as far from the centre is twice the tile");
        assert!((at(Grab::Rotate, (1.5, 1.0), (1.0, 1.5)).rotation_deg - 90.0).abs() < 1e-3, "a quarter turn about the centre");
        assert_eq!(at(Grab::Scale, (1.5, 1.0), (1.0, 1.0)).scale, MIN_SCALE, "never below the smallest tile");
    }
}
