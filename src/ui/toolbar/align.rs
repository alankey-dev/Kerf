//! The Align popover: a two-column grid of Align buttons over a row of Distribute buttons, under an Align
//! to toggle while the selection sits in a container.

use egui::{Response, Sense, Stroke, Ui, vec2};

use crate::containers::AlignTo;
use crate::design::Align;
use crate::edit::Edit;
use crate::ui::commands::{Cx, Entry};
use crate::ui::icons::Icon;
use crate::ui::popover::{option_button, popover, titled};
use crate::ui::tokens::BORDER_SUBTLE;
use crate::ui::tooltip::tip;

const WIDTH: f32 = 322.0;
const GAP: f32 = 12.0;

/// Row-major: the left column is Top, Vertical, Bottom, Centre; the right is Left, Horizontal, Right.
pub(super) const ALIGN: [Align; 7] = [Align::Top, Align::Left, Align::VCentre, Align::HCentre, Align::Bottom, Align::Right, Align::Centre];
pub(super) const DISTRIBUTE: [Align; 2] = [Align::DistributeV, Align::DistributeH];

/// Every tile, Align then Distribute, resolved against the Align To choice.
pub(super) fn entries(cx: &Cx) -> Vec<Entry> {
    ALIGN.iter().chain(&DISTRIBUTE).map(|&op| cx.entry(cx.align(op))).collect()
}

/// Buttons that need more units (lone shapes and whole links) than the selection has stay visible,
/// disabled. The toggle writes the Align To choice into `align_to`.
pub fn menu(ui: &Ui, anchor: &Response, cx: &Cx, align_to: &mut AlignTo) -> Option<Edit> {
    popover(ui, anchor, "menu", WIDTH, |ui| {
        if cx.design.align_container(cx.on).is_some() {
            titled(ui, "Align to");
            align_to_toggle(ui, cx.design.align_target(cx.on, *align_to), align_to);
            ui.add_space(16.0);
        }
        let cx = Cx { align_to: *align_to, ..*cx };
        let tiles = entries(&cx);
        let (align, distribute) = tiles.split_at(ALIGN.len());
        titled(ui, "Align");
        let skips_locked = cx.design.position_locked(cx.on);
        let aligned = grid(ui, align, skips_locked);
        ui.add_space(16.0);
        let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 1.0), Sense::hover());
        ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, BORDER_SUBTLE));
        ui.add_space(16.0);
        titled(ui, "Distribute");
        let distributed = grid(ui, distribute, skips_locked);
        aligned.or(distributed)
    })
}

fn align_to_toggle(ui: &mut Ui, current: AlignTo, align_to: &mut AlignTo) {
    let cell = vec2((WIDTH - GAP) / 2.0, 32.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (to, label, body) in [
            (AlignTo::Selection, "Selection", "Align the selected objects to one another."),
            (AlignTo::Container, "Container", "Align the selected objects to their container's padded edges."),
        ] {
            let on = current == to;
            let resp = ui.add_sized(cell, egui::Button::new(label).selected(on));
            let name = format!("Align to {label}");
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &name));
            if tip(resp, &name, body, None).clicked() {
                *align_to = to;
            }
        }
    });
}

/// Added to each enabled tile's tooltip while the selection holds a position-locked object.
const SKIPS_LOCKED: &str = "Position-locked objects stay where they are.";

fn grid(ui: &mut Ui, tiles: &[Entry], skips_locked: bool) -> Option<Edit> {
    let cell = vec2((WIDTH - GAP) / 2.0, 40.0);
    let mut picked = None;
    for (r, row) in tiles.chunks(2).enumerate() {
        if r > 0 {
            ui.add_space(GAP);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for e in row {
                let short = e.info.short.unwrap_or(e.label);
                let resp = ui.allocate_ui(cell, |ui| option_button(ui, e.info.icon.unwrap_or(Icon::Align), short, e.label, e.enabled())).inner;
                let body = if skips_locked && e.enabled() { format!("{} {SKIPS_LOCKED}", e.tip_body()) } else { e.tip_body() };
                if tip(resp, e.label, &body, None).clicked() && e.enabled() {
                    picked = Some(e.edit().clone());
                }
            }
        });
    }
    picked
}
