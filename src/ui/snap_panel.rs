//! The Snapping popover (spec 47): a switch per target class and the angle Shift locks to. It opens
//! above the magnet from the chevron beside it, or from View > Snapping... Each change is one
//! [`Edit::Snap`], so it is one undo step and the project remembers it.

use egui::{Align2, Area, Context, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, vec2};

use crate::design::Design;
use crate::edit::{Edit, Subject};
use crate::snap::{SnapClass, SnapOptions};

use super::chrome::text;
use super::fonts;
use super::popover::{clicked_outside, menu_frame, titled};
use super::tokens::*;

const WIDTH: f32 = 240.0;
const STEPS: [f32; 4] = [15.0, 30.0, 45.0, 90.0];

fn popup_id() -> Id {
    Id::new("snap-panel")
}

/// Opens the popover; it shows from the next time the canvas paints beside the magnet.
pub fn open(ctx: &Context) {
    ctx.memory_mut(|m| m.open_popup(popup_id()));
}

pub fn is_open(ctx: &Context) -> bool {
    ctx.memory(|m| m.is_popup_open(popup_id()))
}

/// The popover above the magnet `anchor`, while open. A change runs one [`Edit::Snap`].
pub fn show(ui: &Ui, anchor: Rect, design: &mut Design, selection: &mut Vec<u64>) {
    if !is_open(ui.ctx()) {
        return;
    }
    let mut options = design.snap;
    let area = Area::new(popup_id()).order(Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(anchor.left_top() - vec2(0.0, 6.0)).show(ui.ctx(), |ui| {
        menu_frame()
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(WIDTH);
                body(ui, &mut options);
            })
            .response
    });
    if options != design.snap {
        _ = crate::edit::run(design, selection, &Edit::Snap(options), Subject::Selection);
    }
    let rect = area.response.rect;
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || clicked_outside(ui.ctx(), &[anchor, rect]) {
        ui.memory_mut(|m| m.close_popup());
    }
}

fn body(ui: &mut Ui, options: &mut SnapOptions) {
    titled(ui, "Snapping");
    for class in SnapClass::ALL {
        let mut on = options.enabled(class);
        if ui.checkbox(&mut on, class.label()).changed() {
            options.set(class, on);
        }
    }
    ui.add_space(6.0);
    text(ui.painter(), ui.cursor().min, Align2::LEFT_TOP, "Shift angle", fonts::semibold(12.0), TEXT_MUTED);
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for step in STEPS {
            if step_chip(ui, step, options.angle_step_deg == step).clicked() {
                options.angle_step_deg = step;
            }
        }
    });
}

/// One angle-step button, filled while chosen.
fn step_chip(ui: &mut Ui, step: f32, selected: bool) -> egui::Response {
    let label = format!("{step}\u{b0}");
    let (rect, resp) = ui.allocate_exact_size(vec2((WIDTH - 12.0) / 4.0, 28.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &label));
    let fill = if selected || resp.hovered() { ACCENT_HOVER } else { BG_PANEL };
    let edge = if selected { ACCENT_PRIMARY } else { BORDER_SUBTLE };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, &label, fonts::regular(13.0), TEXT_PRIMARY);
    resp
}
