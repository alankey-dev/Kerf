//! The Grid popover (spec 46): spacing, subdivisions, origin, which lines show and Snap to grid, with
//! presets. It opens from a right-click on the ruler corner, View > Grid settings... or Cmd+'.

use egui::{Align, Align2, Area, Context, Id, Order, Rect, Response, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::Design;
use crate::edit::{Edit, Subject};
use crate::grid::{GridSettings, GridVisibility, MAX_SUBDIVISIONS, PRESETS};
use crate::snap::Aabb;
use crate::units::{FIELD_H, Units, length_field, number_field};

use super::chrome::text;
use super::fonts;
use super::popover::{clicked_outside, menu_frame, titled};
use super::tokens::*;
use super::tooltip::tip;

const WIDTH: f32 = 272.0;
const LABEL_W: f32 = 84.0;
const GAP: f32 = 8.0;

fn popup_id() -> Id {
    Id::new("grid-panel")
}

/// Opens the popover; it shows from the next time the canvas paints its ruler corner.
pub fn open(ctx: &Context) {
    ctx.memory_mut(|m| m.open_popup(popup_id()));
}

pub fn is_open(ctx: &Context) -> bool {
    ctx.memory(|m| m.is_popup_open(popup_id()))
}

/// View > Partial grid: major lines only, or every line again, on the `grid` the canvas shows. One undo step.
pub fn toggle_partial(design: &mut Design, selection: &mut Vec<u64>, grid: GridSettings) {
    let visible = if grid.visible == GridVisibility::Full { GridVisibility::Major } else { GridVisibility::Full };
    _ = crate::edit::run(design, selection, &Edit::Grid(GridSettings { visible, ..grid }), Subject::Selection);
}

/// The popover below and right of the ruler `corner`, while open, editing `grid`, the one the canvas
/// shows. Each change is one [`Edit::Grid`], or for Snap to grid one flip of the design's switch, so each
/// is one undo step.
pub fn show(ui: &Ui, corner: Rect, design: &mut Design, selection: &mut Vec<u64>, grid: GridSettings, units: Units) {
    if !is_open(ui.ctx()) {
        return;
    }
    let mut snap_grid = design.snap.grid;
    let chosen = selection_top_left(design, selection);
    let area = Area::new(popup_id()).order(Order::Foreground).pivot(Align2::LEFT_TOP).fixed_pos(corner.right_bottom() + vec2(4.0, 4.0)).show(ui.ctx(), |ui| {
        menu_frame()
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                body(ui, grid, units, chosen, &mut snap_grid)
            })
            .inner
    });
    let rect = area.response.rect;
    if let Some(changed) = area.inner {
        _ = crate::edit::run(design, selection, &Edit::Grid(changed), Subject::Selection);
    }
    design.snap.grid = snap_grid;
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || clicked_outside(ui.ctx(), &[corner, rect]) {
        ui.memory_mut(|m| m.close_popup());
    }
}

/// The top-left of the selection's bounds, for Use selection.
fn selection_top_left(design: &Design, selection: &[u64]) -> Option<(f32, f32)> {
    let bounds = design.shapes.iter().filter(|s| selection.contains(&s.id)).map(|s| design.geometry(s).bounds()).fold(Aabb::EMPTY, Aabb::union);
    (!bounds.is_empty()).then_some((bounds.min_x, bounds.min_y))
}

fn body(ui: &mut Ui, grid: GridSettings, units: Units, chosen: Option<(f32, f32)>, snap_grid: &mut bool) -> Option<GridSettings> {
    titled(ui, "Grid");
    let mut out = None;
    if let Some(typed) = row(ui, "Spacing", |ui, w| length_field(ui, "Spacing", Some(grid.major_in), units, w)) {
        let major_in = typed.apply(grid.major_in);
        out = (major_in.is_finite() && major_in > 0.0).then_some(GridSettings { major_in, ..grid });
    }
    if let Some(typed) = row(ui, "Subdivisions", |ui, w| number_field(ui, "Subdivisions", Some(grid.subdivisions as f32), "", w)) {
        let n = typed.apply(grid.subdivisions as f32);
        out = n.is_finite().then(|| GridSettings { subdivisions: (n.round().max(1.0) as u32).min(MAX_SUBDIVISIONS), ..grid });
    }
    let (ox, oy) = grid.origin_in;
    if let Some(typed) = row(ui, "Origin X", |ui, w| length_field(ui, "Origin X", Some(ox), units, w)) {
        out = Some(GridSettings { origin_in: (typed.apply(ox), oy), ..grid });
    }
    if let Some(typed) = row(ui, "Origin Y", |ui, w| length_field(ui, "Origin Y", Some(oy), units, w)) {
        out = Some(GridSettings { origin_in: (ox, typed.apply(oy)), ..grid });
    }
    ui.horizontal(|ui| {
        let half = (WIDTH - GAP) / 2.0;
        let corner = chip(ui, "Reset to mat corner", half, false, grid.origin_in != (0.0, 0.0));
        if tip(corner, "Reset to mat corner", "Start the grid lines at the mat's top-left corner.", None).clicked() {
            out = Some(GridSettings { origin_in: (0.0, 0.0), ..grid });
        }
        let use_sel = chip(ui, "Use selection", half, false, chosen.is_some());
        if let (true, Some(at)) = (tip(use_sel, "Use selection", "Start the grid lines at the selected object's top-left corner.", None).clicked(), chosen) {
            out = Some(GridSettings { origin_in: at, ..grid });
        }
    });
    ui.add_space(4.0);
    if let Some(visible) = row(ui, "Show", |ui, _| visibility_row(ui, grid.visible)) {
        out = Some(GridSettings { visible, ..grid });
    }
    ui.checkbox(snap_grid, "Snap to grid");
    ui.add_space(4.0);
    text(ui.painter(), ui.cursor().min, Align2::LEFT_TOP, "Presets", fonts::semibold(12.0), TEXT_MUTED);
    ui.add_space(20.0);
    for line in PRESETS.chunks(4) {
        ui.horizontal(|ui| {
            for preset in line {
                let on = grid.major_in == preset.major_in && grid.subdivisions == preset.subdivisions;
                let resp = tip(chip(ui, preset.name, (WIDTH - 3.0 * GAP) / 4.0, on, true), preset.name, "Fill in this grid spacing and subdivisions.", None);
                if resp.clicked() {
                    out = Some(grid.with_preset(preset));
                }
            }
        });
    }
    out
}

/// A label on the left and a field filling the rest of the row.
fn row<R>(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui, f32) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(WIDTH, FIELD_H), egui::Layout::left_to_right(Align::Center), |ui| {
        let (lab, _) = ui.allocate_exact_size(vec2(LABEL_W, FIELD_H), Sense::hover());
        text(ui.painter(), lab.left_center(), Align2::LEFT_CENTER, label, fonts::regular(13.0), TEXT_PRIMARY);
        field(ui, WIDTH - LABEL_W - GAP)
    })
    .inner
}

/// Off, Major lines and Full side by side; returns the one clicked when it differs from `current`.
pub fn visibility_row(ui: &mut Ui, current: GridVisibility) -> Option<GridVisibility> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (visible, label) in [(GridVisibility::Off, "Off"), (GridVisibility::Major, "Major lines"), (GridVisibility::Full, "Full")] {
            let w = text_w(ui, label) + 20.0;
            if chip(ui, label, w, visible == current, true).clicked() && visible != current {
                picked = Some(visible);
            }
        }
    });
    picked
}

fn text_w(ui: &Ui, label: &str) -> f32 {
    super::chrome::text_width(ui, label, fonts::regular(13.0))
}

/// A 28 px bordered text button, filled with the accent tint while `selected`.
fn chip(ui: &mut Ui, label: &str, width: f32, selected: bool, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, FIELD_H), sense);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    let fill = if selected || (enabled && resp.hovered()) { ACCENT_HOVER } else { BG_PANEL };
    let edge = if selected { ACCENT_PRIMARY } else { BORDER_SUBTLE };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    text(ui.painter(), pos2(rect.center().x, rect.center().y), Align2::CENTER_CENTER, label, fonts::regular(13.0), ink);
    resp
}
