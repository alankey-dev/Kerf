//! The tutorial's spotlight: a dim layer over the whole window with a rounded cut-out around the step's
//! target, a ring around the cut-out, and the step's card beside it with an arrow pointing in. Clicks
//! inside the cut-out reach the app; clicks on the dim go nowhere.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align2, Area, Color32, Context, CornerRadius, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use super::chrome::text;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::modal;
use super::tokens::*;
use super::tooltip::tip;

/// Room between the target and the cut-out's edge.
const PAD: f32 = 8.0;
const RADIUS: f32 = 8.0;
const RING: f32 = 2.0;
const CARD_W: f32 = 320.0;
const CARD_MARGIN: i8 = 16;
/// From the cut-out to the card, the arrow included.
const GAP: f32 = 14.0;
const ARROW: f32 = 8.0;
/// The card keeps this far inside the window.
const EDGE: f32 = 12.0;
/// How long a step takes to fade in, and the spotlight to move to its target.
pub const FADE_S: f32 = 0.15;

/// What the card says and which buttons it offers.
pub struct Card<'a> {
    pub section: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    /// The step's shortcut, formatted for this platform.
    pub keys: Option<String>,
    /// From 1.
    pub step: usize,
    pub of: usize,
    pub back: bool,
    pub show_me: bool,
    /// The forward button's label, or `None` while the step waits on the user.
    pub next: Option<&'a str>,
    pub skip: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    Back,
    Next,
    ShowMe,
    Skip,
    Close,
}

/// Where the spotlight is this frame. `hole` is the cut-out, or `None` for a centred card over plain dim.
/// `dim` and `fade` run from 0 to 1: the dim layer's strength and the card's.
#[derive(Clone, Copy)]
pub struct Spot {
    pub hole: Option<Rect>,
    pub dim: f32,
    pub fade: f32,
}

/// Which side of the cut-out the card sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Right,
    Left,
    Below,
    Above,
}

pub fn show(ctx: &Context, spot: Spot, card: &Card) -> Option<Pressed> {
    dim_layer(ctx, spot);
    card_area(ctx, spot, card)
}

/// The cut-out around `target`, as the overlay draws it.
pub fn hole_around(target: Rect) -> Rect {
    target.expand(PAD)
}

fn dim_layer(ctx: &Context, spot: Spot) {
    let screen = ctx.screen_rect();
    let fill = TUTORIAL_DIM.gamma_multiply(spot.dim);
    let area = Area::new(Id::new("tutorial-dim")).order(Order::Middle).fixed_pos(screen.min).movable(false).show(ctx, |ui| {
        ui.set_clip_rect(screen);
        let bands = match spot.hole {
            Some(hole) => {
                let hole = hole.intersect(screen);
                let r = RADIUS.min(hole.width() / 2.0).min(hole.height() / 2.0);
                ui.painter().add(corners(hole, r, fill));
                let ring = ACCENT_PRIMARY.gamma_multiply(spot.dim);
                ui.painter().rect_stroke(hole.expand(RING), r + RING, Stroke::new(4.0_f32, ring.gamma_multiply(0.25)), StrokeKind::Outside);
                ui.painter().rect_stroke(hole, r, Stroke::new(RING, ring), StrokeKind::Outside);
                around(screen, hole)
            }
            None => vec![screen],
        };
        for (i, band) in bands.into_iter().enumerate().filter(|(_, b)| b.is_positive()) {
            ui.painter().rect_filled(band, 0.0, fill);
            ui.interact(band, Id::new(("tutorial-dim", i)), Sense::click_and_drag());
        }
    });
    ctx.move_to_top(area.response.layer_id);
}

/// The four bands of `screen` around `hole`: above, below, left and right of it.
fn around(screen: Rect, hole: Rect) -> Vec<Rect> {
    vec![
        Rect::from_min_max(screen.min, pos2(screen.right(), hole.top())),
        Rect::from_min_max(pos2(screen.left(), hole.bottom()), screen.max),
        Rect::from_min_max(pos2(screen.left(), hole.top()), pos2(hole.left(), hole.bottom())),
        Rect::from_min_max(pos2(hole.right(), hole.top()), pos2(screen.right(), hole.bottom())),
    ]
}

/// The dim in the hole's four corners, outside their rounding: a fan from each corner to its arc.
fn corners(hole: Rect, r: f32, fill: Color32) -> Shape {
    const SEGMENTS: usize = 8;
    let mut mesh = Mesh::default();
    let quarter = std::f32::consts::FRAC_PI_2;
    let corners = [
        (hole.left_top(), hole.left_top() + vec2(r, r), 2.0),
        (hole.right_top(), hole.right_top() + vec2(-r, r), 3.0),
        (hole.right_bottom(), hole.right_bottom() + vec2(-r, -r), 0.0),
        (hole.left_bottom(), hole.left_bottom() + vec2(r, -r), 1.0),
    ];
    for (corner, centre, quadrant) in corners {
        let base = mesh.vertices.len() as u32;
        let vertex = |pos: Pos2| Vertex { pos, uv: WHITE_UV, color: fill };
        mesh.vertices.push(vertex(corner));
        for i in 0..=SEGMENTS {
            let a = (quadrant + i as f32 / SEGMENTS as f32) * quarter;
            mesh.vertices.push(vertex(centre + vec2(a.cos(), a.sin()) * r));
        }
        for i in 0..SEGMENTS as u32 {
            mesh.add_triangle(base, base + 1 + i, base + 2 + i);
        }
    }
    Shape::mesh(mesh)
}

fn card_area(ctx: &Context, spot: Spot, card: &Card) -> Option<Pressed> {
    let id = Id::new("tutorial-card");
    let screen = ctx.screen_rect();
    let size = ctx.memory(|m| m.area_rect(id)).map_or(vec2(CARD_W, 200.0), |r| r.size());
    let (at, side) = match spot.hole {
        Some(hole) => place(hole, size, screen),
        None => (screen.center() - size / 2.0, None),
    };
    let rise = (1.0 - spot.fade) * 6.0;
    let shown = Area::new(id).order(Order::Tooltip).fixed_pos(at + vec2(0.0, rise)).movable(false).constrain(false).show(ctx, |ui| {
        ui.multiply_opacity(spot.fade);
        let frame =
            super::popover::menu_frame().corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(CARD_MARGIN)).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE));
        frame.show(ui, |ui| body(ui, card)).inner
    });
    if let (Some(hole), Some(side)) = (spot.hole, side) {
        arrow(ctx, shown.response.layer_id, shown.response.rect, hole, side, spot.fade);
    }
    shown.inner
}

/// Where the card goes: beside the cut-out on the first side it fits, right, left, below then above;
/// over the cut-out's lower right when it fits on none.
fn place(hole: Rect, size: Vec2, screen: Rect) -> (Pos2, Option<Side>) {
    let inside = screen.shrink(EDGE);
    let y_beside = (hole.center().y - size.y / 2.0).clamp(inside.top(), (inside.bottom() - size.y).max(inside.top()));
    let x_over = (hole.center().x - size.x / 2.0).clamp(inside.left(), (inside.right() - size.x).max(inside.left()));
    let options = [
        (pos2(hole.right() + GAP, y_beside), Side::Right),
        (pos2(hole.left() - GAP - size.x, y_beside), Side::Left),
        (pos2(x_over, hole.bottom() + GAP), Side::Below),
        (pos2(x_over, hole.top() - GAP - size.y), Side::Above),
    ];
    let fits = |at: Pos2| inside.contains_rect(Rect::from_min_size(at, size));
    match options.into_iter().find(|&(at, _)| fits(at)) {
        Some((at, side)) => (at, Some(side)),
        None => {
            let corner = hole.intersect(inside).max - size - Vec2::splat(EDGE);
            (corner.clamp(inside.min, (inside.max - size).max(inside.min)), None)
        }
    }
}

fn arrow(ctx: &Context, layer: LayerId, card: Rect, hole: Rect, side: Side, fade: f32) {
    let inset = 12.0 + ARROW;
    let across_y = hole.center().y.clamp(card.top() + inset, card.bottom() - inset);
    let across_x = hole.center().x.clamp(card.left() + inset, card.right() - inset);
    let (base, dir, along) = match side {
        Side::Right => (pos2(card.left(), across_y), vec2(-1.0, 0.0), vec2(0.0, 1.0)),
        Side::Left => (pos2(card.right(), across_y), vec2(1.0, 0.0), vec2(0.0, 1.0)),
        Side::Below => (pos2(across_x, card.top()), vec2(0.0, -1.0), vec2(1.0, 0.0)),
        Side::Above => (pos2(across_x, card.bottom()), vec2(0.0, 1.0), vec2(1.0, 0.0)),
    };
    let tip = base + dir * ARROW;
    let (a, b) = (base + along * ARROW - dir, base - along * ARROW - dir);
    let painter = ctx.layer_painter(layer);
    painter.add(Shape::convex_polygon(vec![a, tip, b], Color32::WHITE.gamma_multiply(fade), Stroke::NONE));
    painter.add(Shape::line(vec![base + along * ARROW, tip, base - along * ARROW], Stroke::new(1.0_f32, BORDER_SUBTLE.gamma_multiply(fade))));
}

fn body(ui: &mut Ui, card: &Card) -> Option<Pressed> {
    let width = CARD_W - 2.0 * f32::from(CARD_MARGIN);
    ui.set_width(width);
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut pressed = None;

    let (head, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    text(ui.painter(), head.left_center(), Align2::LEFT_CENTER, card.section, fonts::medium(12.0), ACCENT_PRIMARY);
    let close = Rect::from_center_size(pos2(head.right() - 10.0, head.center().y), Vec2::splat(22.0));
    if close_button(ui, close) {
        pressed = Some(Pressed::Close);
    }
    let progress = format!("Step {} of {}", card.step, card.of);
    let line = text(ui.painter(), pos2(close.left() - 8.0, head.center().y), Align2::RIGHT_CENTER, &progress, fonts::regular(12.0), TEXT_MUTED);
    ui.interact(line, ui.id().with("progress"), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &progress));

    ui.add_space(10.0);
    ui.label(egui::RichText::new(card.title).font(fonts::semibold(16.0)).color(TEXT_PRIMARY));
    ui.add_space(6.0);
    ui.label(egui::RichText::new(card.body).font(fonts::regular(13.5)).color(TEXT_MUTED).line_height(Some(19.0)));
    if let Some(keys) = &card.keys {
        ui.add_space(10.0);
        keys_chip(ui, keys);
    }

    ui.add_space(16.0);
    let (track, _) = ui.allocate_exact_size(vec2(width, 3.0), Sense::hover());
    ui.painter().rect_filled(track, 1.5, BORDER_SUBTLE);
    let done = track.width() * card.step as f32 / card.of.max(1) as f32;
    ui.painter().rect_filled(Rect::from_min_size(track.min, vec2(done, track.height())), 1.5, ACCENT_PRIMARY);

    ui.add_space(14.0);
    let (row, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let mut right = row.right();
    let mut button = |label: &str, a11y: &str, primary: bool, w: f32| {
        let rect = Rect::from_min_size(pos2(right - w, row.top()), vec2(w, row.height()));
        right -= w + 8.0;
        modal::button(ui, rect, label, a11y, primary)
    };
    if let Some(next) = card.next {
        let a11y = if card.step == card.of { "Finish tutorial" } else { "Next step" };
        if button(next, a11y, true, 80.0) {
            pressed = Some(Pressed::Next);
        }
    }
    if card.show_me && button("Show me", "Show me", card.next.is_none(), 88.0) {
        pressed = Some(Pressed::ShowMe);
    }
    if card.back && button("Back", "Previous step", false, 72.0) {
        pressed = Some(Pressed::Back);
    }
    if card.skip && skip_link(ui, row) {
        pressed = Some(Pressed::Skip);
    }
    pressed
}

fn close_button(ui: &mut Ui, rect: Rect) -> bool {
    let resp = ui.interact(rect, ui.id().with("tutorial-close"), Sense::click());
    let resp = tip(resp, "Close tutorial", "Ends the tour and closes its tab.", Some("Esc"));
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Close tutorial"));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, ACCENT_HOVER);
    }
    paint_icon(ui, Icon::Close, rect.shrink(4.0), if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED });
    resp.clicked()
}

fn keys_chip(ui: &mut Ui, keys: &str) {
    let font = fonts::semibold(12.0);
    let w = super::chrome::text_width(ui, keys, font.clone()) + 14.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("Shortcut {keys}")));
    ui.painter().rect(rect, 5.0, ACCENT_HOVER, Stroke::new(1.0_f32, ACCENT_PRIMARY.gamma_multiply(0.3)), StrokeKind::Inside);
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, keys, font, ACCENT_PRIMARY);
}

fn skip_link(ui: &mut Ui, row: Rect) -> bool {
    let font = fonts::regular(12.5);
    let label = "Skip tutorial";
    let w = super::chrome::text_width(ui, label, font.clone());
    let rect = Rect::from_min_size(row.left_top(), vec2(w, row.height()));
    let resp = ui.interact(rect, ui.id().with("tutorial-skip"), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let ink = if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED };
    let drawn = text(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, label, font, ink);
    if resp.hovered() {
        ui.painter().hline(drawn.x_range(), drawn.bottom() + 1.0, Stroke::new(1.0_f32, ink));
    }
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(1200.0, 800.0) };
    const CARD: Vec2 = vec2(CARD_W, 200.0);

    #[test]
    fn the_card_tries_right_then_left_then_below_then_above() {
        let side = |hole: Rect| place(hole, CARD, SCREEN).1;
        assert_eq!(side(Rect::from_min_size(pos2(10.0, 300.0), vec2(40.0, 40.0))), Some(Side::Right));
        assert_eq!(side(Rect::from_min_size(pos2(1100.0, 300.0), vec2(80.0, 40.0))), Some(Side::Left));
        assert_eq!(side(Rect::from_min_size(pos2(100.0, 10.0), vec2(1000.0, 40.0))), Some(Side::Below));
        assert_eq!(side(Rect::from_min_size(pos2(100.0, 700.0), vec2(1000.0, 80.0))), Some(Side::Above));
        assert_eq!(side(SCREEN.shrink(20.0)), None, "a target filling the window takes the card inside it");
    }

    #[test]
    fn the_card_stays_inside_the_window() {
        for hole in [Rect::from_min_size(pos2(10.0, 0.0), vec2(40.0, 10.0)), Rect::from_min_size(pos2(1150.0, 760.0), vec2(40.0, 40.0)), SCREEN] {
            let (at, _) = place(hole, CARD, SCREEN);
            assert!(SCREEN.contains_rect(Rect::from_min_size(at, CARD)), "{hole:?} put the card at {at:?}");
        }
    }

    #[test]
    fn the_dim_bands_leave_only_the_hole_uncovered() {
        let hole = Rect::from_min_max(pos2(100.0, 200.0), pos2(300.0, 260.0));
        let bands = around(SCREEN, hole);
        for p in [pos2(5.0, 5.0), pos2(150.0, 100.0), pos2(50.0, 230.0), pos2(400.0, 230.0), pos2(150.0, 700.0)] {
            assert!(bands.iter().any(|b| b.contains(p)), "{p:?} is dimmed");
        }
        assert!(!bands.iter().any(|b| b.contains(hole.center())), "the hole's centre is clear");
    }
}
