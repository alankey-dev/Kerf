//! Hover help for icon controls: a bold name, one line on what it does, and its shortcut, in a card
//! beside the control with an arrow pointing at it.

use egui::{Align2, Area, Color32, Context, CornerRadius, Frame, LayerId, Order, Pos2, Rect, Response, RichText, Shape, Stroke, pos2, vec2};

use super::fonts;
use super::tokens::*;

const GAP: f32 = 4.0;
const ARROW: f32 = 6.0;

/// Which side of the control the card sits on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Right,
    Below,
    Above,
}

/// Rail items open to the right, controls in the lower half of the window open upwards, the rest below.
fn side_for(widget: Rect, screen: Rect) -> Side {
    if widget.right() <= screen.left() + RAIL_W + 0.5 {
        Side::Right
    } else if widget.center().y > screen.center().y {
        Side::Above
    } else {
        Side::Below
    }
}

pub fn tip(resp: Response, title: &str, body: &str, shortcut: Option<&str>) -> Response {
    if due(&resp) {
        card(&resp, title, body, shortcut);
    }
    resp
}

/// The same card, shown now whether or not the pointer rests on the control.
pub fn card(resp: &Response, title: &str, body: &str, shortcut: Option<&str>) {
    let ctx = &resp.ctx;
    let side = side_for(resp.rect, ctx.screen_rect());
    let target = resp.rect;
    let (pivot, at) = match side {
        Side::Right => (Align2::LEFT_CENTER, pos2(target.right() + GAP + ARROW, target.center().y)),
        Side::Below => (Align2::CENTER_TOP, pos2(target.center().x, target.bottom() + GAP + ARROW)),
        Side::Above => (Align2::CENTER_BOTTOM, pos2(target.center().x, target.top() - GAP - ARROW)),
    };
    let id = resp.id.with("tip");
    let shown = Area::new(id).order(Order::Tooltip).pivot(pivot).fixed_pos(at).constrain(true).interactable(false).show(ctx, |ui| {
        card_frame().show(ui, |ui| {
            ui.set_max_width(240.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(title).font(fonts::semibold(13.0)).color(TEXT_PRIMARY));
            if !body.is_empty() {
                ui.label(RichText::new(body).font(fonts::regular(13.0)).color(TEXT_MUTED));
            }
            if let Some(keys) = shortcut {
                ui.add_space(2.0);
                ui.label(RichText::new(keys).font(fonts::semibold(12.0)).color(ACCENT_PRIMARY));
            }
        });
    });
    paint_arrow(ctx, LayerId::new(Order::Tooltip, id), shown.response.rect, target, side);
}

/// egui's own rule, cut down: the pointer has rested on the control for `tooltip_delay` without a click,
/// scroll or press since.
fn due(resp: &Response) -> bool {
    if !resp.hovered() {
        return false;
    }
    let delay = resp.ctx.style().interaction.tooltip_delay;
    let (idle, down) = resp.ctx.input(|i| {
        let idle = i.pointer.time_since_last_movement().min(i.pointer.time_since_last_click()).min(i.time_since_last_scroll());
        (idle, i.pointer.any_down())
    });
    if down {
        return false;
    }
    if idle < delay {
        resp.ctx.request_repaint_after_secs(delay - idle);
        return false;
    }
    true
}

fn card_frame() -> Frame {
    Frame::NONE
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .shadow(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: SHADOW })
}

/// A triangle on the card's near edge, its tip `GAP` from the control's centre line. It covers the card
/// border where they meet, so the two read as one outline.
fn paint_arrow(ctx: &Context, layer: LayerId, card: Rect, target: Rect, side: Side) {
    let inset = 8.0 + ARROW;
    let (base, dir, along) = match side {
        Side::Right => (pos2(card.left(), target.center().y.clamp(card.top() + inset, card.bottom() - inset)), vec2(-1.0, 0.0), vec2(0.0, 1.0)),
        Side::Below => (pos2(target.center().x.clamp(card.left() + inset, card.right() - inset), card.top()), vec2(0.0, -1.0), vec2(1.0, 0.0)),
        Side::Above => (pos2(target.center().x.clamp(card.left() + inset, card.right() - inset), card.bottom()), vec2(0.0, 1.0), vec2(1.0, 0.0)),
    };
    let tip: Pos2 = base + dir * ARROW;
    let (a, b) = (base + along * ARROW - dir, base - along * ARROW - dir);
    let painter = ctx.layer_painter(layer);
    painter.add(Shape::convex_polygon(vec![a, tip, b], Color32::WHITE, Stroke::NONE));
    painter.add(Shape::line(vec![base + along * ARROW, tip, base - along * ARROW], Stroke::new(1.0_f32, BORDER_SUBTLE)));
}
