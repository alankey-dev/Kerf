//! The canvas cues for operations without a standard glyph: a legend pill listing the operations in the
//! design (click one to select its shapes), a hover hint naming a shape's operation, and the View menu's
//! Show as made toggle.

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use super::canvas::{Grid, PILL_H, PILL_W};
use super::op_style::{self, Mode};
use super::tokens::*;
use super::{chrome, fonts, tooltip};
use crate::design::{Design, Operation, Shape};
use crate::pens::Pen;

const H: f32 = 36.0;
const SWATCH: Vec2 = vec2(18.0, 12.0);
const PAD: f32 = 12.0;
const GAP: f32 = 12.0;
/// Where the legend starts: right of the zoom pill, 45 from the canvas edge, and its magnet button,
/// which ends `4 + PILL_H` past the pill.
const LEFT: f32 = 45.0 + PILL_W + 4.0 + PILL_H + 8.0;
const BOTTOM: f32 = 25.0;

/// On the canvas in this mode: not hidden, and not a guide when showing the design as made.
fn shown(s: &Shape, mode: Mode) -> bool {
    !s.hidden && !(mode == Mode::AsMade && s.operation == Operation::Guide)
}

/// Each operation on a shown shape once, in menu order, with the colour of its first shape.
pub fn present(design: &Design, mode: Mode) -> Vec<(Operation, Color32)> {
    let mut out: Vec<(Operation, Color32)> = Vec::new();
    for s in design.shapes.iter().filter(|s| shown(s, mode)) {
        if !out.iter().any(|(op, _)| *op == s.operation) {
            out.push((s.operation, s.fill));
        }
    }
    out.sort_by_key(|(op, _)| ORDER.iter().position(|o| o == op));
    out
}

const ORDER: [Operation; 12] = {
    use crate::design::FoilWeight::*;
    [
        Operation::BasicCut,
        Operation::WavyCut,
        Operation::Perforate,
        Operation::PrintThenCut,
        Operation::Pen,
        Operation::Foil(Fine),
        Operation::Foil(Medium),
        Operation::Foil(Bold),
        Operation::Score,
        Operation::Deboss,
        Operation::Engrave,
        Operation::Guide,
    ]
};

/// The legend pill in the canvas's bottom-left corner, beside the zoom pill.
pub fn show(ui: &mut Ui, canvas: Rect, design: &Design, selection: &mut Vec<u64>, mode: Mode) {
    let ops = present(design, mode);
    if ops.is_empty() {
        return;
    }
    let font = fonts::regular(13.0);
    let width_of = |widths: &[f32]| widths.iter().sum::<f32>() + GAP * (widths.len() - 1) as f32 + 2.0 * PAD;
    let room = canvas.width() - LEFT - 16.0;
    let labelled: Vec<f32> = ops.iter().map(|(op, _)| SWATCH.x + 6.0 + chrome::text_width(ui, op.label(), font.clone())).collect();
    // Too many operations to name in the space left: swatches only, each still named by its tooltip.
    let named = width_of(&labelled) <= room;
    let widths = if named { labelled } else { vec![SWATCH.x; ops.len()] };
    let total = width_of(&widths);
    if total > room {
        return;
    }
    let pill = Rect::from_min_size(pos2(canvas.left() + LEFT, canvas.bottom() - BOTTOM - H), vec2(total, H));
    // The whole pill takes the pointer, so a click between items never reaches the canvas below.
    ui.interact(pill, ui.id().with("legend"), Sense::click_and_drag());
    super::anchors::mark(ui, super::anchors::Anchor::OpLegend, pill);
    let painter = ui.painter();
    painter.rect_filled(pill, H / 2.0, Color32::WHITE);
    painter.rect_stroke(pill, H / 2.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let mut x = pill.left() + PAD;
    for ((op, colour), w) in ops.into_iter().zip(widths) {
        let item = Rect::from_min_size(pos2(x, pill.top() + 4.0), vec2(w, H - 8.0));
        let target = item.expand2(vec2(GAP / 2.0 - 1.0, 0.0));
        x += w + GAP;
        let label = op.label();
        let resp = ui.interact(target, ui.id().with(("legend", label)), Sense::click());
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("{label} shapes")));
        let resp = tooltip::tip(resp, label, &format!("Select every {label} shape."), None);
        if resp.hovered() {
            ui.painter().rect_filled(target, 6.0, ACCENT_HOVER);
        }
        let swatch = Rect::from_min_size(pos2(item.left(), item.center().y - SWATCH.y / 2.0), SWATCH);
        paint_swatch(ui, swatch, op, colour, mode);
        if named {
            chrome::text(ui.painter(), pos2(swatch.right() + 6.0, item.center().y), Align2::LEFT_CENTER, label, font.clone(), TEXT_PRIMARY);
        }
        if resp.clicked() {
            // Select Same > Operation for this pill, so the rule lives once in `select` (spec 50).
            *selection = crate::select::operation(design, op);
        }
    }
}

/// A small rectangle drawn in the operation's own style.
fn paint_swatch(ui: &Ui, rect: Rect, op: Operation, colour: Color32, mode: Mode) {
    let ring: Vec<Pos2> = vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    // A pen takes its width from a physical nib; size it as on the canvas at 100 %.
    let look = op_style::look(op, colour, Pen::default(), 72.0, mode);
    ui.painter().extend(op_style::paint(ui.ctx(), &look, &[(ring, true)]));
}

/// "Basic Cut" on its own, or "Basic Cut, Cardstock" once a Make flow has chosen a material.
pub fn hint(op: Operation, material: Option<&str>) -> String {
    match material {
        Some(m) => format!("{}, {m}", op.label()),
        None => op.label().to_owned(),
    }
}

/// After a short rest over an unselected shape, a tooltip by the pointer names its operation (and the
/// material, once chosen). A selected shape already shows its operation in the inspector, and the hint
/// would cover its action bar.
pub fn hover_hint(canvas: &Response, grid: &Grid, design: &Design, selection: &[u64], material: Option<&str>, mode: Mode) {
    let Some(p) = canvas.hover_pos() else { return };
    let (x, y) = grid.to_inches(p);
    let Some(shape) = design.drawn().rev().filter(|s| shown(s, mode)).find(|s| design.geometry(s).contains((x, y))) else { return };
    if selection.contains(&shape.id) {
        return;
    }
    let mut over = canvas.clone();
    over.rect = Rect::from_center_size(p, Vec2::splat(16.0));
    over.id = canvas.id.with(("hint", shape.id));
    tooltip::tip(over, &hint(shape.operation, material), "", None);
}

/// View > Show as made: draws every shape the way the machine will make it.
pub fn show_as_made_toggle(ui: &mut Ui, mode: &mut Mode) {
    let mut on = *mode == Mode::AsMade;
    let resp = ui.checkbox(&mut on, "Show as made");
    if resp.changed() {
        *mode = if on { Mode::AsMade } else { Mode::Design };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::FoilWeight;

    #[test]
    fn the_legend_lists_each_shown_operation_once_in_menu_order() {
        let mut d = Design::default();
        for op in [Operation::Score, Operation::BasicCut, Operation::Guide, Operation::Score, Operation::Foil(FoilWeight::Bold), Operation::Engrave] {
            let id = d.add_shape(crate::design::ShapeKind::Square);
            d.shape_mut(id).unwrap().set_operation(op);
        }
        let last = d.shapes.last().unwrap().id;
        d.shape_mut(last).unwrap().hidden = true;
        let ops = |mode| present(&d, mode).into_iter().map(|(op, _)| op).collect::<Vec<_>>();
        assert_eq!(ops(Mode::Design), [Operation::BasicCut, Operation::Foil(FoilWeight::Bold), Operation::Score, Operation::Guide]);
        assert_eq!(ops(Mode::AsMade), [Operation::BasicCut, Operation::Foil(FoilWeight::Bold), Operation::Score], "guides are not made");
        assert_eq!(crate::select::operation(&d, Operation::Score).len(), 2, "the legend selects both Score shapes through select");
    }

    #[test]
    fn the_hint_names_the_material_once_chosen() {
        assert_eq!(hint(Operation::BasicCut, None), "Basic Cut");
        assert_eq!(hint(Operation::BasicCut, Some("Cardstock")), "Basic Cut, Cardstock");
    }
}
