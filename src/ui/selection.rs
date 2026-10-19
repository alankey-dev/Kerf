//! The selection's chrome on the canvas: its frame and handles, the size badge and the floating action bar.

use std::f32::consts::PI;

use egui::{Id, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::containers::AlignTo;
use crate::design::{Arrange, Design, Shape};
use crate::edit::{Edit, Subject};
use crate::transform::{Axis, Frame, Handle};
use crate::units::Units;

use super::canvas::{Grid, paint_round_handle, snap_px};
use super::commands::{Cx, Entry};
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::popover;
use super::tokens::*;
use super::tooltip::tip;

pub(super) const ROTATE_ARM_PX: f32 = 22.0;
/// Pointer reach around a handle centre, so every target is at least 18 px across.
const REACH_PX: f32 = 9.0;
/// Handle outlines are stroked on their centre line, so each path is one stroke narrower than drawn.
const HANDLE_STROKE_PX: f32 = 2.0;
const CORNER_R: f32 = (10.0 - HANDLE_STROKE_PX) / 2.0;
const PILL_LEN: f32 = 20.0 - HANDLE_STROKE_PX;
const PILL_THICK: f32 = 8.0 - HANDLE_STROKE_PX;
const CROSS_HALF: f32 = 12.0;
const GAP: f32 = 8.0;
const BADGE_H: f32 = 28.0;
const BADGE_PAD_X: f32 = 16.0;
const BUTTON: f32 = 32.0;
const BAR_PAD: f32 = 6.0;
const BAR_SPACING: f32 = 4.0;
const MORE: &str = "More actions";

pub enum Grip {
    Resize(Handle),
    Rotate,
    /// The centre crosshair, dragged to set the rotation pivot (spec 52).
    Pivot,
}

/// The one shape a selection holds, if it holds exactly one.
fn single<'a>(design: &'a Design, selection: &[u64]) -> Option<&'a Shape> {
    match selection {
        [id] => design.shapes.iter().find(|s| s.id == *id),
        _ => None,
    }
}

/// Where the selection's pivot crosshair sits in world inches: the shape's own pivot, or the frame centre.
pub fn pivot_pos(design: &Design, selection: &[u64], frame: &Frame) -> (f32, f32) {
    frame.pivot_world(single(design, selection).and_then(|s| s.pivot))
}

pub struct Chrome {
    pub select_tool: bool,
    /// Any drag on the canvas: move, resize, rotate or marquee.
    pub dragging: bool,
    pub resizing: bool,
    pub canvas: Rect,
}

fn frame_of(design: &Design, selection: &[u64]) -> Option<Frame> {
    Frame::of(design, design.shapes.iter().filter(|s| selection.contains(&s.id)))
}

/// The handle under `p`, the rotate handle first since it can sit over a small frame's edge.
pub fn grip_at(design: &Design, selection: &[u64], grid: &Grid, p: Pos2) -> Option<Grip> {
    if design.position_locked(selection) {
        return None;
    }
    let f = frame_of(design, selection)?;
    let near = |q| grid.to_screen(q).distance(p) <= REACH_PX;
    if near(f.rotate_handle_pos(ROTATE_ARM_PX / grid.scale)) {
        return Some(Grip::Rotate);
    }
    // The centre crosshair sets the pivot; only a lone shape carries one.
    if single(design, selection).is_some() && near(pivot_pos(design, selection, &f)) {
        return Some(Grip::Pivot);
    }
    Handle::RESIZE.into_iter().find(|&h| near(f.handle_pos(h))).map(Grip::Resize)
}

/// Writes each shape over the design's shape with the same id.
pub fn write_back(design: &mut Design, shapes: Vec<Shape>) {
    for shape in shapes {
        if let Some(s) = design.shape_mut(shape.id) {
            *s = shape;
        }
    }
}

/// Paints the chrome and runs the action bar's and More menu's picks.
pub fn show(ui: &mut Ui, painter: &Painter, grid: &Grid, design: &mut Design, selection: &mut Vec<u64>, units: Units, chrome: Chrome) {
    let more_id = ui.id().with(("selection", MORE));
    let frame = frame_of(design, selection);
    let bar_shown = chrome.select_tool && !chrome.dragging;
    if frame.is_none() || !bar_shown {
        // The More menu is only drawn with its bar; left open, it would pop up with the next selection.
        ui.memory_mut(|m| {
            if m.is_popup_open(more_id.with("more")) {
                m.close_popup();
            }
        });
    }
    let Some(frame) = frame else { return };
    let corners = [Handle::TopLeft, Handle::TopRight, Handle::BottomRight, Handle::BottomLeft].map(|h| snap_px(grid.to_screen(frame.handle_pos(h))));
    painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(1.0_f32, ACCENT_PRIMARY)));
    if !chrome.select_tool {
        return;
    }
    let mut bounds = Rect::from_points(&corners);
    if !design.position_locked(selection) {
        let pivot = pivot_pos(design, selection, &frame);
        let moved = single(design, selection).is_some_and(|s| s.pivot.is_some());
        bounds = bounds.union(Rect::from_center_size(paint_handles(painter, grid, &frame, pivot, moved), Vec2::ZERO));
    }
    let bounds = bounds.expand(CORNER_R + HANDLE_STROKE_PX);
    if chrome.dragging && !chrome.resizing {
        return;
    }

    let size = units.size(frame.width_in, frame.height_in);
    let galley = painter.layout_no_wrap(size.clone(), fonts::semibold(13.0), ON_PRIMARY);
    let actions = entries(&Cx::new(design, selection, AlignTo::default()));
    let slots = actions.len() + 1;
    let bar = vec2(slots as f32 * BUTTON + (slots - 1) as f32 * BAR_SPACING, BUTTON) + Vec2::splat(2.0 * BAR_PAD);
    let at = place(bounds, vec2(galley.size().x + 2.0 * BADGE_PAD_X, BADGE_H), bar, chrome.canvas);

    painter.rect_filled(at.badge, 8.0, ACCENT_PRIMARY);
    painter.galley(at.badge.center() - galley.size() / 2.0, galley, ON_PRIMARY);
    ui.interact(at.badge, ui.id().with("selection-size"), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &size));
    super::anchors::mark(ui, super::anchors::Anchor::SelectionFrame, bounds.union(at.badge));

    let more = MORE_MENU.map(|e| Cx::new(design, selection, AlignTo::default()).entry(e));
    if let Some(edit) = bar_shown.then(|| action_bar(ui, painter, at.bar, &actions, &more, more_id)).flatten() {
        _ = crate::edit::run(design, selection, &edit, Subject::Selection);
    }
}

/// Paints the handles and the pivot crosshair at `pivot`, and returns where the rotate handle sits on
/// screen. A `moved` pivot (off the centre) is drawn in the accent, so it reads as a set hinge.
fn paint_handles(painter: &Painter, grid: &Grid, frame: &Frame, pivot: (f32, f32), moved: bool) -> Pos2 {
    let at = |h| snap_px(grid.to_screen(frame.handle_pos(h)));
    let rotate = snap_px(grid.to_screen(frame.rotate_handle_pos(ROTATE_ARM_PX / grid.scale)));
    let (sin, cos) = frame.rotation_deg.to_radians().sin_cos();
    let along = vec2(cos, sin);

    let centre = snap_px(grid.to_screen(pivot));
    let cross = if moved { Stroke::new(1.5_f32, ACCENT_PRIMARY) } else { Stroke::new(1.0_f32, TEXT_MUTED.gamma_multiply(0.4)) };
    if moved {
        painter.circle(centre, CROSS_HALF * 0.5, egui::Color32::TRANSPARENT, cross);
    }
    for axis in [along, along.rot90()] {
        painter.line_segment([centre - axis * CROSS_HALF, centre + axis * CROSS_HALF], cross);
    }

    painter.line_segment([at(Handle::Top), rotate], Stroke::new(1.0_f32, ACCENT_PRIMARY));
    let stroke = Stroke::new(HANDLE_STROKE_PX, ACCENT_PRIMARY);
    for h in Handle::RESIZE {
        match h {
            Handle::Top | Handle::Bottom => pill(painter, at(h), along, stroke),
            Handle::Left | Handle::Right => pill(painter, at(h), along.rot90(), stroke),
            _ => _ = painter.circle(at(h), CORNER_R, BG_PANEL, stroke),
        }
    }
    paint_round_handle(painter, rotate);
    rotate
}

/// A capsule lying along `along`, so edge handles turn with a rotated frame.
fn pill(painter: &Painter, centre: Pos2, along: Vec2, stroke: Stroke) {
    const STEPS: usize = 8;
    let (r, half) = (PILL_THICK / 2.0, PILL_LEN / 2.0 - PILL_THICK / 2.0);
    let across = along.rot90();
    let cap = |end: Vec2, from: f32| (0..=STEPS).map(move |i| from + PI * i as f32 / STEPS as f32).map(move |a| centre + end + (along * a.cos() + across * a.sin()) * r);
    let points = cap(along * half, -PI / 2.0).chain(cap(-along * half, PI / 2.0)).collect();
    painter.add(egui::Shape::convex_polygon(points, BG_PANEL, stroke));
}

#[derive(Debug, PartialEq)]
pub struct Placement {
    pub badge: Rect,
    pub bar: Rect,
}

/// The badge then the bar below `frame_px`, or above it, bar on top, when the bar would leave the canvas.
/// Both are centred on the frame and kept inside the canvas with a `GAP` margin.
pub fn place(frame_px: Rect, badge: Vec2, bar: Vec2, canvas: Rect) -> Placement {
    let inner = canvas.shrink(GAP);
    let centred = |size: Vec2, top: f32| {
        let left = (frame_px.center().x - size.x / 2.0).clamp(inner.left(), (inner.right() - size.x).max(inner.left()));
        Rect::from_min_size(pos2(left, top), size)
    };
    let below = frame_px.bottom() + GAP;
    let (badge_top, bar_top) = if below + badge.y + GAP + bar.y <= canvas.bottom() {
        (below, below + badge.y + GAP)
    } else {
        let badge_top = frame_px.top() - GAP - badge.y;
        (badge_top, badge_top - GAP - bar.y)
    };
    Placement { badge: centred(badge, badge_top), bar: centred(bar, bar_top) }
}

/// A bar slot: one edit, or the second of two when it can run, else the first. A slot whose edit cannot
/// run is left out of the bar.
pub(super) enum Slot {
    One(Edit),
    Either(Edit, Edit),
}

/// The bar's buttons before More.
pub(super) const BAR: [Slot; 4] =
    [Slot::One(Edit::Delete), Slot::One(Edit::Duplicate), Slot::Either(Edit::Attach, Edit::Detach), Slot::Either(Edit::Group, Edit::Ungroup)];

/// The More menu's rows. The context menu adds Move Forward and Move Backward.
pub(super) const MORE_MENU: [Edit; 5] =
    [Edit::Arrange(Arrange::ToFront), Edit::Arrange(Arrange::ToBack), Edit::Flip(Axis::Horizontal), Edit::Flip(Axis::Vertical), Edit::SetHidden(true)];

/// The bar's buttons for these ids, before More.
pub(super) fn entries(cx: &Cx) -> Vec<Entry> {
    BAR.iter()
        .map(|slot| match slot {
            Slot::One(e) => cx.entry(e.clone()),
            Slot::Either(a, b) => cx.either(a.clone(), b.clone()),
        })
        .filter(Entry::enabled)
        .collect()
}

/// The accessible name of a bar button, distinct from the Edit menu's item so both can be found while
/// the menu is open.
pub(super) fn bar_name(label: &str) -> String {
    format!("{label} selection")
}

fn action_bar(ui: &mut Ui, painter: &Painter, rect: Rect, buttons: &[Entry], more_rows: &[Entry], more_id: Id) -> Option<Edit> {
    let shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: SHADOW };
    painter.add(shadow.as_shape(rect, 12.0));
    painter.rect(rect, 12.0, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    super::anchors::mark(ui, super::anchors::Anchor::ActionBar, rect);
    let slot = |i: usize| Rect::from_min_size(rect.min + Vec2::splat(BAR_PAD) + vec2(i as f32 * (BUTTON + BAR_SPACING), 0.0), Vec2::splat(BUTTON));
    let mut picked = None;
    for (i, e) in buttons.iter().enumerate() {
        let name = bar_name(e.label);
        let keys = e.keys(ui.ctx());
        let resp = bar_button(ui, slot(i), ui.id().with(("selection", &name)), e.info.icon.unwrap_or(Icon::More), &name, (e.label, &e.tip_body(), keys.as_deref()));
        if resp.clicked() {
            picked = Some(e.edit().clone());
        }
    }
    let more = bar_button(ui, slot(buttons.len()), more_id, Icon::More, MORE, ("More", "", None));
    let rows = popover::popover(ui, &more, "more", 180.0, |ui| more_rows.iter().find(|e| popover::command_row(ui, e)).map(|e| e.edit().clone()));
    picked.or(rows)
}

fn bar_button(ui: &mut Ui, rect: Rect, id: Id, icon: Icon, name: &str, (title, body, shortcut): (&str, &str, Option<&str>)) -> egui::Response {
    let resp = tip(ui.interact(rect, id, Sense::click()), title, body, shortcut);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    paint_icon(ui, icon, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), TEXT_PRIMARY);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::LinkKind;

    const CANVAS: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(800.0, 600.0) };
    const BADGE: Vec2 = vec2(120.0, 28.0);
    const BAR: Vec2 = vec2(150.0, 44.0);

    #[test]
    fn badge_then_bar_sit_below_the_frame() {
        let frame = Rect::from_min_max(pos2(300.0, 100.0), pos2(400.0, 200.0));
        let at = place(frame, BADGE, BAR, CANVAS);
        assert_eq!(at.badge, Rect::from_min_size(pos2(290.0, 208.0), BADGE));
        assert_eq!(at.bar, Rect::from_min_size(pos2(275.0, 244.0), BAR));
    }

    #[test]
    fn near_the_canvas_bottom_they_stack_above_with_the_bar_on_top() {
        let frame = Rect::from_min_max(pos2(300.0, 450.0), pos2(400.0, 550.0));
        let at = place(frame, BADGE, BAR, CANVAS);
        assert_eq!(at.badge, Rect::from_min_size(pos2(290.0, 414.0), BADGE));
        assert_eq!(at.bar, Rect::from_min_size(pos2(275.0, 362.0), BAR));
    }

    #[test]
    fn both_stay_inside_the_canvas_sideways() {
        let at = place(Rect::from_min_max(pos2(-50.0, 100.0), pos2(10.0, 200.0)), BADGE, BAR, CANVAS);
        assert_eq!((at.badge.left(), at.bar.left()), (8.0, 8.0));
        let at = place(Rect::from_min_max(pos2(780.0, 100.0), pos2(900.0, 200.0)), BADGE, BAR, CANVAS);
        assert_eq!((at.badge.right(), at.bar.right()), (792.0, 792.0));
    }

    fn shapes(links: &[Option<(u64, LinkKind)>]) -> Design {
        let mut d = Design::default();
        for &l in links {
            let id = d.add_shape(crate::design::ShapeKind::Square);
            d.shape_mut(id).unwrap().link = l.map(|(id, kind)| crate::links::Link { id, kind });
        }
        d
    }

    #[test]
    fn link_buttons_follow_the_units_in_the_selection() {
        use Edit::*;
        let ids = |d: &Design| d.shapes.iter().map(|s| s.id).collect::<Vec<_>>();
        let buttons = |d: &Design| entries(&Cx::new(d, &ids(d), AlignTo::default())).into_iter().map(|e| e.edit().clone()).collect::<Vec<_>>();
        assert_eq!(buttons(&shapes(&[None])), [Delete, Duplicate]);
        assert_eq!(buttons(&shapes(&[None, None])), [Delete, Duplicate, Attach, Group]);
        assert_eq!(buttons(&shapes(&[Some((50, LinkKind::Group)), Some((50, LinkKind::Group))])), [Delete, Duplicate, Ungroup]);
        assert_eq!(buttons(&shapes(&[Some((50, LinkKind::Attach)), Some((50, LinkKind::Attach)), None])), [Delete, Duplicate, Detach, Group]);
    }
}
