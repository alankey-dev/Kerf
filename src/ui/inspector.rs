//! The 240 px right inspector: Properties on top (live numeric fields, operation, colour) and Layers
//! below, split by a 1px separator.

use egui::{Align, Align2, Color32, Context, Frame, Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, Operation, Shape};
use crate::pens::Pen;

use crate::expr::Typed;
use crate::units::{FIELD_H, Units, length_field, number_field};

use super::chrome::text;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::layers_actions;
use super::layers_panel;
use super::pattern_panel;
use super::tokens::*;
use super::toolbar::{operation_dropdown, operation_menu, pen};
use crate::edit::{Edit, Lock, Subject};
use crate::transform::{Axis, Field};

const COLUMN_GAP: f32 = 8.0;
/// The aspect chain between W and H, and the gap either side of it.
const CHAIN_W: f32 = 14.0;
const CHAIN_GAP: f32 = 3.0;
/// Two shapes share a value when they differ by less than this (inches or degrees).
const SAME: f32 = 1e-3;

/// The values every selected shape agrees on; `None` where the selection is empty or disagrees.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Common {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub w: Option<f32>,
    pub h: Option<f32>,
    pub rotation: Option<f32>,
    pub skew_x: Option<f32>,
    pub skew_y: Option<f32>,
    pub operation: Option<Operation>,
    pub fill: Option<Color32>,
    pub pen: Option<Pen>,
}

impl Common {
    pub fn of(shapes: &[&Shape]) -> Self {
        let near = |a: &f32, b: &f32| (a - b).abs() < SAME;
        let angle = |a: &f32, b: &f32| (a - b).rem_euclid(360.0).min((b - a).rem_euclid(360.0)) < SAME;
        Self {
            x: shared(shapes, |s| s.x_in, near),
            y: shared(shapes, |s| s.y_in, near),
            w: shared(shapes, |s| s.width_in, near),
            h: shared(shapes, |s| s.height_in, near),
            rotation: shared(shapes, |s| s.rotation_deg.rem_euclid(360.0), angle),
            skew_x: shared(shapes, |s| s.skew_deg.0, near),
            skew_y: shared(shapes, |s| s.skew_deg.1, near),
            operation: shared(shapes, |s| s.operation, |a, b| a == b),
            fill: shared(shapes, |s| s.fill, |a, b| a == b),
            pen: shared(shapes, |s| s.pen, |a, b| a == b),
        }
    }
}

fn shared<T: Copy>(shapes: &[&Shape], get: impl Fn(&Shape) -> T, same: impl Fn(&T, &T) -> bool) -> Option<T> {
    let first = get(shapes.first()?);
    shapes.iter().all(|s| same(&first, &get(s))).then_some(first)
}

/// Returns what the colour picker's Pattern tab needs the app for, and a command a panel raised (Edit master).
pub fn show(
    ctx: &Context,
    design: &mut Design,
    selection: &mut Vec<u64>,
    contour_open: &mut bool,
    units: Units,
    board: (f32, f32),
    palette: &mut super::colour_picker::Palette,
) -> (Option<(pattern_panel::Asked, pattern_panel::Target)>, Option<super::commands::Command>) {
    let mut asked = None;
    let mut command = None;
    let panel = egui::SidePanel::right("inspector").exact_width(INSPECTOR_W).resizable(false).frame(Frame::NONE.fill(BG_PANEL)).show_separator_line(false);
    panel.show(ctx, |ui| {
        let full = ui.max_rect();
        ui.painter().vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
        let bar = egui::TopBottomPanel::bottom("layers-actions").exact_height(layers_actions::BAR_H).frame(Frame::NONE).show_separator_line(false);
        bar.show_inside(ui, |ui| layers_actions::show(ui, design, selection, contour_open));
        if !selection.is_empty() {
            Frame::NONE.inner_margin(Margin { left: 12, right: 12, top: 12, bottom: 12 }).show(ui, |ui| {
                asked = properties(ui, design, selection, units, board, palette).map(|a| (a, pattern_panel::Target::Selection));
                super::shape_panel::section(ui, design, selection, units);
                super::container_panel::show(ui, design, selection, units);
                super::effects_list::section(ui, design, selection, units, contour_open);
                asked = super::instance_overrides::section(ui, design, selection, units, palette).or(asked);
            });
            let y = ui.cursor().top();
            ui.painter().hline(full.x_range(), y + 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            ui.add_space(1.0);
        }
        Frame::NONE.inner_margin(Margin { left: 12, right: 12, top: 12, bottom: 0 }).show(ui, |ui| {
            if super::components_panel::tabs(ui) {
                command = super::components_panel::show(ui, design, selection);
                layers_panel::finish_rename(ui, design, selection, &[]);
            } else {
                layers_panel::show(ui, design, selection, units, contour_open);
            }
        });
    });
    super::effects_list::editor(ctx, design, selection, units);
    (asked, command)
}

pub fn section_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(fonts::semibold(13.0)).color(TEXT_PRIMARY));
    ui.add_space(8.0);
}

fn properties(
    ui: &mut Ui,
    design: &mut Design,
    selection: &mut Vec<u64>,
    units: Units,
    board: (f32, f32),
    palette: &mut super::colour_picker::Palette,
) -> Option<pattern_panel::Asked> {
    section_title(ui, "Properties");
    let shapes: Vec<&Shape> = selection.iter().filter_map(|id| design.shapes.iter().find(|s| s.id == *id)).collect();
    let enabled = !shapes.is_empty();
    let common = Common::of(&shapes);
    let col = (ui.available_width() - COLUMN_GAP) / 2.0;
    let mut edit: Option<(Field, Typed)> = None;
    let mut take = |field: Field, typed: Option<Typed>| {
        if let Some(typed) = typed {
            edit = Some((field, typed));
        }
    };
    let all = |lock: Lock| enabled && shapes.iter().all(|s| lock.of(s));
    let (aspect, pinned) = (all(Lock::Aspect), all(Lock::Position));
    let mut lock = None;
    let (mut skew_x, mut skew_y) = (None, None);
    ui.spacing_mut().item_spacing = vec2(COLUMN_GAP, 6.0);
    ui.add_enabled_ui(enabled && !pinned, |ui| {
        ui.horizontal(|ui| {
            take(Field::X, labelled(ui, "X", col, |ui, w| length_field(ui, "X", common.x, units, w)));
            take(Field::Y, labelled(ui, "Y", col, |ui, w| length_field(ui, "Y", common.y, units, w)));
        });
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = CHAIN_GAP;
        let half = (ui.available_width() - CHAIN_W - 2.0 * CHAIN_GAP) / 2.0;
        take(Field::W, ui.add_enabled_ui(enabled && !pinned, |ui| labelled(ui, "W", half, |ui, w| length_field(ui, "W", common.w, units, w))).inner);
        if toggle(ui, vec2(CHAIN_W, FIELD_H), aspect, enabled, ASPECT, paint_chain) {
            lock = Some(Edit::Lock { lock: Lock::Aspect, on: !aspect });
        }
        take(Field::H, ui.add_enabled_ui(enabled && !pinned, |ui| labelled(ui, "H", half, |ui, w| length_field(ui, "H", common.h, units, w))).inner);
    });
    ui.horizontal(|ui| {
        let rot_w = ui.available_width() - FIELD_H - COLUMN_GAP;
        let rotation = ui.add_enabled_ui(enabled && !pinned, |ui| labelled(ui, "Rotation", rot_w, |ui, w| number_field(ui, "Rotation", common.rotation, "°", w))).inner;
        take(Field::Rotation, rotation);
        let padlock = |ui: &Ui, r: Rect, ink: Color32, on: bool| paint_icon(ui, if on { Icon::Lock } else { Icon::Unlock }, r, ink);
        if toggle(ui, Vec2::splat(FIELD_H), pinned, enabled, POSITION, padlock) {
            lock = Some(Edit::Lock { lock: Lock::Position, on: !pinned });
        }
    });
    ui.horizontal(|ui| {
        let half = (ui.available_width() - COLUMN_GAP) / 2.0;
        skew_x = ui
            .add_enabled_ui(enabled && !pinned, |ui| labelled(ui, "Skew X", half, |ui, w| number_field(ui, "Skew X", common.skew_x, "°", w)))
            .inner
            .map(|value| Edit::Skew { axis: Axis::Horizontal, value });
        skew_y = ui
            .add_enabled_ui(enabled && !pinned, |ui| labelled(ui, "Skew Y", half, |ui, w| number_field(ui, "Skew Y", common.skew_y, "°", w)))
            .inner
            .map(|value| Edit::Skew { axis: Axis::Vertical, value });
    });
    super::anchors::mark(ui, super::anchors::Anchor::Properties, ui.min_rect());

    ui.add_space(4.0);
    caption(ui, "Operation");
    let mut op = None;
    if enabled {
        let width = ui.available_width();
        let dropdown = operation_dropdown(ui, common.operation, width);
        super::anchors::mark(ui, super::anchors::Anchor::Operation, dropdown.rect);
        op = operation_menu(ui, &dropdown, common.operation);
    } else {
        flat_placeholder(ui, "No selection");
    }
    ui.add_space(2.0);
    let colour_caption = caption(ui, "Colour");
    let grain = super::prepare_nest::grain_toggle(ui, colour_caption.rect, enabled.then(|| shapes.iter().all(|s| s.grain_lock))).map(Edit::GrainLock);
    let pen_layers = common.operation == Some(Operation::Pen);
    let name = if pen_layers { Some(common.pen.map_or("Mixed", Pen::colour_name)) } else { None };
    let printable = enabled && shapes.iter().all(|s| s.takes_pattern());
    let pattern = pattern_panel::shared(&shapes).filter(|_| printable);
    let swatch = colour_swatch(ui, common.fill, name, enabled, pattern);
    super::anchors::mark(ui, super::anchors::Anchor::Colour, colour_caption.rect.union(swatch.rect));
    let mut picked = pattern_panel::Picked::default();
    let pen_pick = if pen_layers { pen::menu(ui, &swatch, common.pen) } else { None };
    if !pen_layers {
        picked = pattern_panel::picker(ui, &swatch, common.fill, printable, pattern, units, design, palette);
    }
    pattern_panel::edit_toggle(ui, design, selection);

    let field = edit.map(|(field, value)| Edit::Field { field, value, board_in: board });
    for edit in [field, lock, skew_x, skew_y, op.map(Edit::Operation), picked.colour.map(Edit::Colour), picked.edit, pen_pick.map(Edit::Pen), grain].into_iter().flatten()
    {
        _ = crate::edit::run(design, selection, &edit, Subject::Selection);
    }
    picked.asked
}

fn caption(ui: &mut Ui, label: &str) -> Response {
    ui.label(RichText::new(label).font(fonts::regular(11.0)).color(TEXT_MUTED))
}

fn flat_placeholder(ui: &mut Ui, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), FIELD_H + 4.0), Sense::hover());
    ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    text(ui.painter(), rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, label, fonts::regular(13.0), DISABLED_TEXT);
}

/// What a lock toggle says: its name, which is also its accessibility label, and its tooltip body.
struct LockText {
    title: &'static str,
    body: &'static str,
}

const ASPECT: LockText = LockText { title: "Lock aspect ratio", body: "Keep width and height in proportion. Shift turns it off for one drag." };
const POSITION: LockText = LockText { title: "Lock position", body: "Stop the selection being moved, resized, rotated or flipped. It can still be recoloured." };

/// An icon toggle, accent-washed while `on`. Returns whether it was clicked.
fn toggle(ui: &mut Ui, size: Vec2, on: bool, enabled: bool, says: LockText, paint: impl FnOnce(&Ui, Rect, Color32, bool)) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let resp = super::tooltip::tip(resp, says.title, says.body, None);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, on, says.title));
    let ink = match (enabled, on) {
        (false, _) => DISABLED_TEXT,
        (true, true) => ACCENT_PRIMARY,
        (true, false) => TEXT_MUTED,
    };
    if enabled && (on || resp.hovered()) {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(13.0)), ink, on);
    resp.clicked()
}

/// Two chain links, joined while the aspect is locked and pulled apart while it is free.
fn paint_chain(ui: &Ui, r: Rect, ink: Color32, on: bool) {
    let stroke = Stroke::new(1.5_f32, ink);
    let apart = if on { 0.0 } else { 1.5 };
    let link = vec2(5.0, 7.5);
    for (dy, sign) in [(-2.5, -1.0), (2.5, 1.0)] {
        let centre = r.center() + vec2(0.0, dy + sign * apart);
        ui.painter().rect_stroke(Rect::from_center_size(centre, link), 2.5, stroke, StrokeKind::Middle);
    }
}

/// A muted caption, then the field `body` draws in the width left.
pub(super) fn labelled<R>(ui: &mut Ui, label: &str, width: f32, body: impl FnOnce(&mut Ui, f32) -> R) -> R {
    let label_w = if label.len() > 1 { 48.0 } else { 14.0 };
    ui.allocate_ui_with_layout(vec2(width, FIELD_H), egui::Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (lab, _) = ui.allocate_exact_size(vec2(label_w, FIELD_H), Sense::hover());
        text(ui.painter(), lab.left_center(), Align2::LEFT_CENTER, label, fonts::regular(11.0), TEXT_MUTED);
        body(ui, (width - label_w - 4.0).max(24.0))
    })
    .inner
}

/// The Colour field: a chip and the hex, the pen's `name` on Pen layers, or the pattern that fills the
/// selection. Click it to open a picker.
fn colour_swatch(ui: &mut Ui, fill: Option<Color32>, name: Option<&str>, enabled: bool, pattern: Option<&crate::patterns::PatternFill>) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), FIELD_H + 4.0), sense);
    let says = match (pattern, fill) {
        (Some(_), _) => "Pattern".to_owned(),
        (None, Some(c)) => name.map_or_else(|| hex(c), str::to_owned),
        (None, None) => "Mixed".to_owned(),
    };
    resp.widget_info(|| WidgetInfo { current_text_value: Some(says.clone()), ..WidgetInfo::labeled(WidgetType::Button, enabled, "Colour") });
    let bg = if enabled && resp.hovered() { ACCENT_HOVER } else { BG_PANEL };
    ui.painter().rect(rect, 4.0, bg, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let chip = Rect::from_min_size(rect.min + vec2(6.0, 6.0), Vec2::splat(FIELD_H - 8.0));
    match fill {
        _ if !enabled => {
            text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, "No selection", fonts::regular(13.0), DISABLED_TEXT);
        }
        _ if let Some(p) = pattern => {
            pattern_panel::paint_chip(ui, chip, p);
            text(ui.painter(), pos2(chip.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, "Pattern", fonts::regular(13.0), TEXT_PRIMARY);
        }
        Some(c) => {
            ui.painter().rect(chip, 3.0, c, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
            text(ui.painter(), pos2(chip.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, &says, fonts::regular(13.0), TEXT_PRIMARY);
        }
        None => {
            ui.painter().rect(chip, 3.0, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
            ui.painter().line_segment([chip.left_bottom(), chip.right_top()], Stroke::new(1.0_f32, TEXT_MUTED));
            text(ui.painter(), pos2(chip.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, "Mixed", fonts::regular(13.0), TEXT_MUTED);
        }
    }
    resp
}

fn hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::transform::set_field;

    fn two_shapes() -> Design {
        let mut d = Design::default();
        d.add_shape(ShapeKind::Square);
        d.add_shape(ShapeKind::Circle);
        d
    }

    #[test]
    fn common_values_blank_out_where_shapes_disagree() {
        let mut d = two_shapes();
        d.shapes[0].width_in = 2.0;
        d.shapes[1].width_in = 2.0;
        d.shapes[0].x_in = 1.0;
        d.shapes[1].x_in = 3.0;
        d.shapes[0].rotation_deg = 0.0;
        d.shapes[1].rotation_deg = 360.0;
        let refs: Vec<&Shape> = d.shapes.iter().collect();
        let c = Common::of(&refs);
        assert_eq!(c.w, Some(2.0));
        assert_eq!(c.x, None);
        assert_eq!(c.rotation, Some(0.0), "0 and 360 are the same angle");
        assert_eq!(Common::of(&[]), Common::default());
    }

    #[test]
    fn set_field_edits_position_size_and_rotation() {
        let board = (12.0, 12.0);
        let mut d = two_shapes();
        let s = &mut d.shapes[0];
        let (w, h) = (s.width_in, s.height_in);
        set_field(s, Field::X, 2.5, board);
        assert_eq!(s.x_in, 2.5);
        set_field(s, Field::W, w * 2.0, board);
        assert_eq!((s.width_in, s.height_in), (w * 2.0, h));
        s.lock_aspect = true;
        set_field(s, Field::W, w * 4.0, board);
        assert!((s.height_in - h * 2.0).abs() < 1e-3, "height follows the 2:1 ratio");
        set_field(s, Field::Rotation, -90.0, board);
        assert_eq!(s.rotation_deg, 270.0);
    }

    #[test]
    fn typed_position_can_sit_off_the_mat_but_not_past_one_board() {
        let board = (12.0, 12.0);
        let mut d = two_shapes();
        let s = &mut d.shapes[0];
        set_field(s, Field::X, -3.0, board);
        assert_eq!(s.x_in, -3.0, "a negative X parks the shape in the void");
        set_field(s, Field::X, -100.0, board);
        assert_eq!(s.x_in, -12.0, "but no further out than one board");
    }
}
