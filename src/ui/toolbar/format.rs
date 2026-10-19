//! The Format Text popover: letter and line spacing, wrapping, text box alignment and Select
//! Ungrouping.

use egui::{Align2, Color32, Painter, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::text::VAlign;
use crate::text_ops::Split;
use crate::ui::chrome::{text, text_width};
use crate::ui::fonts;
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::popover::{popover, titled};
use crate::ui::tokens::*;
use crate::ui::tooltip::tip;

use crate::text_ops::{LETTER_SPACE, LINE_SPACE, TextChange};

use super::text::TextCommon;
use crate::units::number_field;

const WIDTH: f32 = 300.0;
const BUTTON: f32 = 40.0;
const GAP: f32 = 8.0;
const FIELD_W: f32 = 120.0;

/// Drawn locally: the wrap and ungroup glyphs are used nowhere else.
#[derive(Clone, Copy)]
enum Glyph {
    Icon(Icon),
    NoWrap,
    Wrap,
    Letters,
    Lines,
    Layers,
}

/// Stays open while the controls change, unlike a menu: every edit is reported but none closes it.
pub fn menu(ui: &Ui, anchor: &Response, common: &TextCommon) -> Option<TextChange> {
    let mut change = None;
    popover(ui, anchor, "format", WIDTH, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
            titled(ui, "Format Text");
            let letter = spacing(ui, "Letter Space", Icon::LetterSpace, LETTER_SPACE, common.letter_space).map(TextChange::LetterSpace);
            let line = spacing(ui, "Line Space", Icon::LineSpace, LINE_SPACE, common.line_space).map(TextChange::LineSpace);
            let wrap = row(ui, "Text wrapping", 2, |ui| {
                let off = toggle(ui, Glyph::NoWrap, "No wrap", "Keep each line as typed.", common.wrap == Some(false), true);
                let on = toggle(ui, Glyph::Wrap, "Wrap in box", "Wrap words at the box width. Drag a side handle to reflow.", common.wrap == Some(true), true);
                off.then_some(TextChange::Wrap(false)).or(on.then_some(TextChange::Wrap(true)))
            });
            let wrapped = common.wrap == Some(true);
            let v_align = row(ui, "Text Box Align", 3, |ui| {
                let mut picked = None;
                for (icon, label, v) in
                    [(Icon::AlignTop, "Top", VAlign::Top), (Icon::AlignVCentre, "Middle", VAlign::Middle), (Icon::AlignBottom, "Bottom", VAlign::Bottom)]
                {
                    if toggle(ui, Glyph::Icon(icon), label, "Where the lines sit in a wrapped box.", wrapped && common.v_align == Some(v), wrapped) {
                        picked = Some(TextChange::VAlign(v));
                    }
                }
                picked
            });
            ui.add_space(8.0);
            let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 17.0), Sense::hover());
            ui.painter().hline(rule.x_range(), rule.center().y, Stroke::new(1.0_f32, BORDER_SUBTLE));
            let split = row(ui, "Select Ungrouping", 3, |ui| {
                let letters = toggle(ui, Glyph::Letters, "Letters", "Split into one text per letter, grouped, each where it sat.", false, common.letters);
                let lines = toggle(ui, Glyph::Lines, "Lines", "Split into one text per line, grouped, each where it sat.", false, common.lines);
                toggle(ui, Glyph::Layers, "Layers", "This font has one layer.", false, false);
                letters.then_some(TextChange::Split(Split::Letters)).or(lines.then_some(TextChange::Split(Split::Lines)))
            });
            change = letter.or(line).or(wrap).or(v_align).or(split);
        });
        None::<()>
    });
    change
}

/// A caption, then a slider beside a number field. The slider reports while dragging, the field on commit.
fn spacing(ui: &mut Ui, label: &str, icon: Icon, range: std::ops::RangeInclusive<f32>, value: Option<f32>) -> Option<f32> {
    caption(ui, label);
    ui.add_space(6.0);
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.spacing_mut().slider_width = ui.available_width() - 16.0 - FIELD_W;
        let mut v = value.unwrap_or(0.0);
        let slider = ui.add(egui::Slider::new(&mut v, range).show_value(false));
        slider.widget_info(|| WidgetInfo::slider(true, f64::from(v), format!("{label} slider")));
        if slider.changed() {
            out = Some(v.round());
        }
        let (glyph, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        paint_icon(ui, icon, glyph, TEXT_PRIMARY);
        if let Some(typed) = number_field(ui, label, value, "", FIELD_W - 20.0 - 16.0).and_then(|t| t.against(value)) {
            out = Some(typed);
        }
    });
    ui.add_space(14.0);
    out
}

/// A caption on the left with its `count` buttons packed to the right.
fn row<R>(ui: &mut Ui, label: &str, count: usize, buttons: impl FnOnce(&mut Ui) -> Option<R>) -> Option<R> {
    ui.horizontal(|ui| {
        ui.set_min_height(BUTTON + GAP);
        ui.spacing_mut().item_spacing.x = GAP;
        caption(ui, label);
        let packed = count as f32 * BUTTON + (count - 1) as f32 * GAP;
        ui.add_space((ui.available_width() - packed).max(0.0));
        buttons(ui)
    })
    .inner
}

/// Painted rather than a label widget, so the control it captions owns the accessible name.
fn caption(ui: &mut Ui, label: &str) {
    let font = fonts::medium(13.0);
    let (rect, _) = ui.allocate_exact_size(vec2(text_width(ui, label, font.clone()), 20.0), Sense::hover());
    text(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, label, font, TEXT_MUTED);
}

/// A 40 px icon button, tinted when `selected`; disabled ones explain why in their tooltip.
fn toggle(ui: &mut Ui, glyph: Glyph, label: &str, body: &str, selected: bool, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BUTTON), if enabled { Sense::click() } else { Sense::hover() });
    let resp = tip(resp, label, body, None);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, enabled, selected, label));
    if enabled && (selected || resp.hovered()) {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    let ink = match (enabled, selected) {
        (false, _) => DISABLED_TEXT,
        (true, true) => ACCENT_PRIMARY,
        (true, false) => TEXT_PRIMARY,
    };
    let inner = Rect::from_center_size(rect.center(), Vec2::splat(24.0));
    match glyph {
        Glyph::Icon(icon) => paint_icon(ui, icon, inner, ink),
        other => paint_glyph(ui.painter(), inner, ink, other),
    }
    enabled && resp.clicked()
}

fn paint_glyph(painter: &Painter, r: Rect, ink: Color32, glyph: Glyph) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let stroke = Stroke::new(1.5_f32, ink);
    let line = |pts: &[(f32, f32)]| painter.add(egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
    let boxed = |x0: f32, y0: f32, x1: f32, y1: f32| painter.rect_stroke(Rect::from_two_pos(at(x0, y0), at(x1, y1)), 2.0, stroke, egui::StrokeKind::Middle);
    match glyph {
        Glyph::NoWrap => {
            line(&[(0.1, 0.4), (0.85, 0.4), (0.7, 0.28)]);
            line(&[(0.1, 0.62), (0.85, 0.62)]);
        }
        Glyph::Wrap => {
            line(&[(0.1, 0.25), (0.9, 0.25)]);
            line(&[(0.1, 0.5), (0.8, 0.5), (0.9, 0.6), (0.8, 0.72), (0.45, 0.72), (0.58, 0.62)]);
            line(&[(0.45, 0.72), (0.58, 0.82)]);
            line(&[(0.1, 0.72), (0.3, 0.72)]);
        }
        Glyph::Letters => {
            boxed(0.08, 0.2, 0.44, 0.8);
            boxed(0.56, 0.2, 0.92, 0.8);
        }
        Glyph::Lines => {
            boxed(0.15, 0.1, 0.85, 0.42);
            boxed(0.15, 0.58, 0.85, 0.9);
        }
        Glyph::Layers => {
            boxed(0.08, 0.2, 0.68, 0.6);
            boxed(0.32, 0.42, 0.92, 0.82);
        }
        Glyph::Icon(_) => {}
    }
}
