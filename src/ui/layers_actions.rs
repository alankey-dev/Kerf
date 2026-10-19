//! The action bar under the Layers list: Slice, Combine, Attach, Flatten and Contour, the same
//! operations as the edit toolbar, next to the list where the objects are picked.

use egui::{Align2, Color32, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::containers::AlignTo;
use crate::design::{Combine, Design};
use crate::edit::{Edit, Subject};

use super::chrome::{text, text_width};
use super::commands::{Cx, Entry};
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::tokens::*;
use super::toolbar::combine;
use super::tooltip::tip;

pub const BAR_H: f32 = 56.0;

/// One slot of the bar. Attach and Flatten each swap for their inverse when the selection calls for it;
/// Combine opens the Combine tiles and Contour the contour panel.
pub(super) enum Slot {
    One(Edit),
    Either(Edit, Edit),
    Combine,
    Contour,
}

pub(super) const BAR: [Slot; 5] =
    [Slot::One(Edit::Combine(Combine::Slice)), Slot::Combine, Slot::Either(Edit::Attach, Edit::Detach), Slot::Either(Edit::Flatten, Edit::Unflatten), Slot::Contour];

const COMBINE_TIP: &str = "Weld the selection into one shape, or merge it with Unite, Subtract, Intersect or Exclude.";
const CONTOUR_TIP: &str = "Hide or show individual cut lines of a shape with more than one.";

/// A slot as painted: its label, icon, tooltip body, shortcut and whether it is live.
pub(super) struct Button {
    pub label: &'static str,
    icon: Icon,
    tip: String,
    keys: Option<egui::KeyboardShortcut>,
    pub enabled: bool,
    /// The edit a click runs; `None` for Combine and Contour.
    pub edit: Option<Edit>,
}

impl Button {
    fn of(e: Entry) -> Self {
        Self { label: e.label, icon: e.info.icon.unwrap_or(Icon::Combine), tip: e.tip_body(), keys: e.info.shortcut, enabled: e.enabled(), edit: Some(e.edit().clone()) }
    }
}

/// The bar's buttons for `selection`. Combine is live when its tiles can weld; Contour when one shape
/// with several contours is selected.
pub(super) fn buttons(design: &Design, selection: &[u64]) -> Vec<Button> {
    let cx = Cx::new(design, selection, AlignTo::default());
    BAR.iter()
        .map(|slot| match slot {
            Slot::One(e) => Button::of(cx.entry(e.clone())),
            Slot::Either(a, b) => Button::of(cx.either(a.clone(), b.clone())),
            Slot::Combine => Button {
                label: "Combine",
                icon: Icon::Combine,
                tip: COMBINE_TIP.into(),
                keys: None,
                enabled: cx.entry(Edit::Combine(Combine::Weld)).enabled(),
                edit: None,
            },
            Slot::Contour => {
                let enabled = matches!(selection, [id] if design.shapes.iter().any(|s| s.id == *id && crate::effects::body_contours(s).len() > 1));
                Button { label: "Contour", icon: Icon::Contour, tip: CONTOUR_TIP.into(), keys: None, enabled, edit: None }
            }
        })
        .collect()
}

/// Paints the bar into `ui`, which the inspector reserves at its foot, and runs the clicked button.
pub fn show(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>, contour_open: &mut bool) {
    let bar = ui.max_rect();
    ui.painter().hline(bar.x_range(), bar.top() + 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
    let row = buttons(design, selection);
    // Each button is as wide as its label, and the space left over is shared evenly around them.
    let widths: Vec<f32> = row.iter().map(|b| text_width(ui, b.label, fonts::regular(12.0)) + if b.label == "Combine" { 8.0 } else { 6.0 }).collect();
    let gap = (bar.width() - widths.iter().sum::<f32>()) / row.len() as f32;
    let mut x = bar.left() + gap / 2.0;
    let mut picked = None;
    for (b, w) in row.iter().zip(widths) {
        let rect = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(w, BAR_H - 6.0)).expand2(vec2(gap / 2.0 - 1.0, 0.0));
        x += w + gap;
        let resp = button(ui, rect, b);
        if !b.enabled {
            // Otherwise a popover left open when the selection shrank would reappear on the next pair.
            if b.label == "Combine" && ui.memory(|m| m.is_popup_open(resp.id.with("menu"))) {
                ui.memory_mut(|m| m.close_popup());
            }
            continue;
        }
        match (&b.edit, b.label) {
            (None, "Combine") => picked = picked.or(combine::menu(ui, &resp, &Cx::new(design, selection, AlignTo::default()))),
            (None, _) if resp.clicked() => *contour_open = !*contour_open,
            (Some(edit), _) if resp.clicked() => picked = Some(edit.clone()),
            _ => {}
        }
    }
    if let Some(edit) = picked {
        _ = crate::edit::run(design, selection, &edit, Subject::Selection);
    }
}

/// A 24 px icon over a 12 px label. The accessible name is prefixed "Layers" so it stays distinct from
/// the edit toolbar's button of the same name.
fn button(ui: &mut Ui, rect: Rect, b: &Button) -> Response {
    let sense = if b.enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(("layers-action", b.label)), sense);
    let keys = b.keys.map(|k| ui.ctx().format_shortcut(&k));
    let resp = tip(resp, b.label, &b.tip, keys.as_deref());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, b.enabled, format!("Layers {}", b.label)));
    let ink = if b.enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    if b.enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let combine = b.label == "Combine";
    let centre = pos2(rect.center().x, rect.top() + 15.0);
    let icon_at = if combine { centre - vec2(4.0, 0.0) } else { centre };
    paint_icon(ui, b.icon, Rect::from_center_size(icon_at, Vec2::splat(24.0)), ink);
    if combine {
        caret(ui, icon_at + vec2(17.0, 0.0), ink);
    }
    text(ui.painter(), pos2(rect.center().x, rect.bottom() - 4.0), Align2::CENTER_BOTTOM, b.label, fonts::regular(12.0), ink);
    resp
}

/// The small down-pointing triangle beside Combine's icon that says it opens a menu.
fn caret(ui: &Ui, centre: egui::Pos2, ink: Color32) {
    let points = vec![centre + vec2(-3.5, -1.5), centre + vec2(3.5, -1.5), centre + vec2(0.0, 2.5)];
    ui.painter().add(Shape::convex_polygon(points, ink, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    fn shapes(n: usize) -> Design {
        let mut d = Design::default();
        for _ in 0..n {
            d.add_shape(ShapeKind::Square);
        }
        d
    }

    fn slot(d: &Design, sel: &[u64], label: &str) -> Button {
        buttons(d, sel).into_iter().find(|b| b.label == label).unwrap()
    }

    #[test]
    fn attach_swaps_for_detach_once_the_selection_is_attached() {
        let mut d = shapes(2);
        assert_eq!(buttons(&d, &[1, 2])[2].label, "Attach");
        d.attach(&[1, 2]);
        assert_eq!(buttons(&d, &[1, 2])[2].label, "Detach");
        assert!(!Cx::new(&d, &[1, 2], AlignTo::default()).entry(Edit::Attach).enabled(), "an attached pair is one unit");
    }

    #[test]
    fn one_shape_enables_neither_slice_nor_combine() {
        let d = shapes(3);
        for (sel, slice, combine) in [(&[1][..], false, false), (&[1, 2], true, true), (&[1, 2, 3], false, true)] {
            assert_eq!(slot(&d, sel, "Slice").enabled, slice, "Slice with {sel:?}");
            assert_eq!(slot(&d, sel, "Combine").enabled, combine, "Combine with {sel:?}");
        }
        assert!(!slot(&d, &[1], "Contour").enabled, "a square has one contour");
    }
}
