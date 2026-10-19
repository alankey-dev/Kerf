//! Ruler guides on the canvas (spec 45): pulling one out of a ruler, hovering and dragging it, the
//! position popover, the right-click menus and the View rows. The model and its rules are `guides.rs`.
//! A drag changes the design once, on release, so each gesture is one undo step.

use egui::{Area, Color32, Context, CursorIcon, Id, Key, LayerId, Order, Painter, Pos2, Rect, Response, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, Pt};
use crate::edit::{Edit, Subject};
use crate::guides::{Axis, GuideEdit, Release, RulerGuide, hit, release};
use crate::snap::{self, Guide};
use crate::units::{self, Units};

use super::canvas::{Grid, SNAP_PX};
use super::commands::{Command, Cx};
use super::flow_kit as kit;
use super::fonts;
use super::popover::{clicked_outside, menu_frame};
use super::tokens::*;

const POPOVER_W: f32 = 200.0;
const MARKER_PX: f32 = 5.0;

/// A guide held by the pointer: a new one from a ruler (`from` is `None`) or an existing one.
#[derive(Clone, Copy)]
struct Drag {
    axis: Axis,
    from: Option<u64>,
    /// The snapped position it would land at.
    at_in: f32,
    /// Where the pointer is, unsnapped, which decides between landing, deleting and cancelling.
    pointer_in: Pt,
}

/// The popover or menu open on a guide, or with `guide` `None`, the rulers' menu.
#[derive(Clone, Copy)]
struct Open {
    guide: Option<u64>,
    at: Pos2,
    menu: bool,
    /// The frame it opened on, whose click must not also close it.
    frame: u64,
}

#[derive(Clone, Copy, Default)]
struct State {
    drag: Option<Drag>,
    hover: Option<u64>,
    open: Option<Open>,
}

fn state_id() -> Id {
    Id::new("ruler-guides")
}

fn state(ctx: &Context) -> State {
    ctx.data(|d| d.get_temp(state_id())).unwrap_or_default()
}

/// The top ruler, the left ruler and their corner, in screen space, as `canvas::paint_rulers` draws them.
fn rulers(art: Rect) -> (Rect, Rect, Rect) {
    let top = Rect::from_min_max(pos2(art.left(), art.top() - RULER), pos2(art.right(), art.top()));
    let left = Rect::from_min_max(pos2(art.left() - RULER, art.top()), pos2(art.left(), art.bottom()));
    (top, left, Rect::from_min_size(art.min - egui::Vec2::splat(RULER), egui::Vec2::splat(RULER)))
}

fn board_in(grid: &Grid, art: Rect) -> Pt {
    (art.width() / grid.scale, art.height() / grid.scale)
}

/// The pointer input that belongs to guides this frame: a pull from a ruler, a drag of a guide, a click,
/// double-click or right-click on one or on the rulers. Returns true when guides took it, so the canvas
/// leaves it alone. `select_tool` is false while another tool or node editing owns the canvas; rulers
/// still give guides then, but existing guides cannot be grabbed.
#[allow(clippy::too_many_arguments, reason = "the canvas's frame state, passed through from one call")]
pub fn input(
    ui: &Ui,
    response: &Response,
    design: &mut Design,
    selection: &mut Vec<u64>,
    grid: &Grid,
    art: Rect,
    snapping: bool,
    select_tool: bool,
    units: Units,
) -> bool {
    let ctx = ui.ctx().clone();
    let mut st = state(&ctx);
    let (top, left, corner) = rulers(art);
    let in_rulers = |p: Pos2| response.rect.contains(p) && (top.contains(p) || left.contains(p) || corner.contains(p));
    let grabbable = select_tool && !design.guides_hidden;
    let guide_at = |design: &Design, p: Pos2| (grabbable && art.contains(p)).then(|| hit(&design.guides, design.guides_locked, grid.to_inches(p), grid.scale)).flatten();
    let mut took = false;

    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let at = grid.to_inches(p);
        let pulled = [(top, Axis::Horizontal), (left, Axis::Vertical)].into_iter().find(|(r, _)| r.contains(p)).map(|(_, axis)| axis);
        if let Some(axis) = pulled {
            st.drag = Some(Drag { axis, from: None, at_in: axis.of(at), pointer_in: at });
        } else if super::selection::grip_at(design, selection, grid, p).is_none()
            && let Some(g) = guide_at(design, p).and_then(|id| design.guide(id).copied())
        {
            st.drag = Some(Drag { axis: g.axis, from: Some(g.id), at_in: g.at_in, pointer_in: at });
        }
    }
    if let Some(mut drag) = st.drag {
        took = true;
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
            drag.pointer_in = grid.to_inches(p);
            drag.at_in = snap_along(design, &drag, snapping, grid, art);
        }
        st.drag = Some(drag);
        if response.drag_stopped() {
            st.drag = None;
            let edit = match (release(drag.axis, drag.pointer_in, board_in(grid, art)), drag.from) {
                (Release::Land(_), None) => Some(GuideEdit::Add { axis: drag.axis, at_in: drag.at_in }),
                (Release::Land(_), Some(id)) => Some(GuideEdit::Move { id, at_in: drag.at_in }),
                (Release::OnRuler, Some(id)) => Some(GuideEdit::Delete(id)),
                (Release::OnRuler | Release::Away, _) => None,
            };
            if let Some(edit) = edit {
                _ = crate::edit::run(design, selection, &Edit::Guide(edit), Subject::Selection);
            }
        } else if !response.dragged() {
            st.drag = None;
        }
    }

    let frame = ctx.cumulative_pass_nr();
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        if in_rulers(p) {
            took = true;
        } else if let Some(id) = guide_at(design, p) {
            took = true;
            if response.double_clicked() {
                st.open = Some(Open { guide: Some(id), at: p, menu: false, frame });
            }
        }
    }
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        if in_rulers(p) {
            took = true;
            st.open = Some(Open { guide: None, at: p, menu: true, frame });
        } else if let Some(id) = guide_at(design, p) {
            took = true;
            st.open = Some(Open { guide: Some(id), at: p, menu: true, frame });
        }
    }

    st.hover = match (st.drag, ui.input(|i| i.pointer.hover_pos())) {
        (Some(d), _) => d.from,
        (None, Some(p)) if response.hovered() => guide_at(design, p),
        _ => None,
    };
    let cursor_on = st.drag.map(|d| d.axis).or_else(|| st.hover.and_then(|id| design.guide(id)).map(|g| g.axis));
    match cursor_on {
        Some(Axis::Vertical) => ctx.set_cursor_icon(CursorIcon::ResizeHorizontal),
        Some(Axis::Horizontal) => ctx.set_cursor_icon(CursorIcon::ResizeVertical),
        None => {}
    }

    if let Some(open) = st.open {
        st.open = popover(&ctx, design, selection, open, units).then_some(open);
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    took
}

/// The position a held guide would land at: the pointer along its axis, snapped to the grid, the shapes'
/// edges and centres, the mat centre and the other guides while snapping is on.
fn snap_along(design: &Design, drag: &Drag, snapping: bool, grid: &Grid, art: Rect) -> f32 {
    if !snapping {
        return drag.axis.of(drag.pointer_in);
    }
    let (w, h) = board_in(grid, art);
    let mat = snap::Aabb { min_x: 0.0, min_y: 0.0, max_x: w, max_y: h };
    let rest: Vec<RulerGuide> = design.shown_guides().iter().filter(|g| Some(g.id) != drag.from).copied().collect();
    let targets = snap::cache::line_targets(design, &[], &rest, None, mat, SNAP_PX / grid.scale, &design.snap);
    let candidate = snap::Candidate { points: vec![(drag.pointer_in, snap::Kind::Edge)], frame: None };
    let s = snap::snap(&candidate, &targets, &design.snap);
    drag.axis.of((drag.pointer_in.0 + s.dx, drag.pointer_in.1 + s.dy))
}

/// A Pen anchor at `p`, moved onto a ruler guide within snapping distance while snapping is on.
pub fn snapped(design: &Design, snapping: bool, grid: &Grid, p: Pos2) -> Pt {
    let at = grid.to_inches(p);
    if !snapping || !design.snap.guides {
        return at;
    }
    let mut targets = snap::Targets { threshold: SNAP_PX / grid.scale, ..Default::default() };
    for g in design.shown_guides() {
        match g.axis {
            Axis::Vertical => targets.v_lines.push((g.at_in, snap::Kind::Guide)),
            Axis::Horizontal => targets.h_lines.push((g.at_in, snap::Kind::Guide)),
        }
    }
    let candidate = snap::Candidate { points: vec![(at, snap::Kind::Edge)], frame: None };
    let s = snap::snap(&candidate, &targets, &design.snap);
    (at.0 + s.dx, at.1 + s.dy)
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::Vertical => "Vertical guide",
        Axis::Horizontal => "Horizontal guide",
    }
}

/// The position popover, or with `open.menu` the right-click menu, which adds the rows for every guide.
/// Returns whether it stays open.
fn popover(ctx: &Context, design: &mut Design, selection: &mut Vec<u64>, open: Open, units: Units) -> bool {
    let guide = open.guide.and_then(|id| design.guide(id).copied());
    if open.guide.is_some() && guide.is_none() {
        return false;
    }
    let mut picked: Option<GuideEdit> = None;
    let mut close = false;
    let area = Area::new(state_id().with("popover")).order(Order::Foreground).fixed_pos(open.at + vec2(8.0, 8.0)).show(ctx, |ui| {
        menu_frame().show(ui, |ui| {
            ui.set_width(POPOVER_W);
            if let Some(g) = guide {
                ui.label(egui::RichText::new(axis_name(g.axis)).font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
                ui.add_space(8.0);
                let movable = !g.locked && !design.guides_locked;
                ui.add_enabled_ui(movable, |ui| {
                    if let Some(typed) = units::length_field(ui, "Guide position", Some(g.at_in), units, units::FIELD_W) {
                        picked = Some(GuideEdit::Move { id: g.id, at_in: typed.apply(g.at_in) });
                    }
                });
                ui.add_space(8.0);
                let mut locked = g.locked;
                if ui.checkbox(&mut locked, "Lock guide").changed() {
                    picked = Some(GuideEdit::SetLocked { id: g.id, locked });
                }
                let delete = kit::secondary_button(ui, "Delete guide", movable);
                delete.widget_info(|| WidgetInfo::labeled(WidgetType::Button, movable, "Delete guide"));
                if delete.clicked() {
                    picked = Some(GuideEdit::Delete(g.id));
                    close = true;
                }
            }
            if open.menu {
                if guide.is_some() {
                    ui.separator();
                }
                // The guide menu rows read as clean text with a violet hover, not grey buttons.
                let w = &mut ui.visuals_mut().widgets;
                w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                w.inactive.bg_stroke = Stroke::NONE;
                if let Some(Command::Edit(Edit::Guide(edit))) = rows(ui, design, guide.is_some()) {
                    picked = Some(edit);
                    close = true;
                }
            }
        });
    });
    if let Some(edit) = picked {
        _ = crate::edit::run(design, selection, &Edit::Guide(edit), Subject::Selection);
    }
    let away = ctx.cumulative_pass_nr() != open.frame && clicked_outside(ctx, &[area.response.rect]);
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    !(close || away || escape)
}

/// View > Show guides, Lock guides and Clear guides, for the View menu.
pub fn menu_rows(ui: &mut Ui, design: &Design) -> Option<Command> {
    rows(ui, design, false)
}

/// The guide rows, each labelled and resolved through `commands`; `on_guide` names Lock all guides as
/// the right-click menu on one guide does.
fn rows(ui: &mut Ui, design: &Design, on_guide: bool) -> Option<Command> {
    let cx = Cx::new(design, &[], Default::default());
    let mut picked = None;
    for (edit, on) in [(GuideEdit::Show, Some(!design.guides_hidden)), (GuideEdit::LockAll, Some(design.guides_locked)), (GuideEdit::Clear, None)] {
        if on_guide && edit == GuideEdit::Show {
            continue;
        }
        let e = cx.entry(Edit::Guide(edit));
        let label = if on_guide && edit == GuideEdit::LockAll { "Lock all guides" } else { e.label };
        let keys = e.keys(ui.ctx()).unwrap_or_default();
        let button = egui::Button::new(label).selected(on.unwrap_or(false)).shortcut_text(keys);
        let resp = ui.add_enabled(e.enabled(), button);
        resp.widget_info(|| match on {
            Some(on) => WidgetInfo::selected(WidgetType::Checkbox, e.enabled(), on, label),
            None => WidgetInfo::labeled(WidgetType::Button, e.enabled(), label),
        });
        let resp = if e.enabled() { resp } else { resp.on_disabled_hover_text(e.tip_body()) };
        if resp.clicked() {
            ui.close_menu();
            picked = Some(e.command.clone());
        }
    }
    picked
}

/// Paints the guides above the shapes, a held guide where it would land, and for the guide hovered or
/// held its marker on the ruler and, while held, its position beside the pointer. `lit` are this frame's
/// smart guide lines: a guide a move snapped to draws at full accent.
pub fn paint(ui: &mut Ui, painter: &Painter, grid: &Grid, art: Rect, design: &Design, lit: &[Guide], units: Units) {
    let st = state(ui.ctx());
    let held = st.drag.filter(|d| matches!(release(d.axis, d.pointer_in, board_in(grid, art)), Release::Land(_)));
    let is_lit = |g: &RulerGuide| {
        lit.iter().any(|l| match (*l, g.axis) {
            (Guide::V(x), Axis::Vertical) => (x - g.at_in).abs() < 1e-4,
            (Guide::H(y), Axis::Horizontal) => (y - g.at_in).abs() < 1e-4,
            _ => false,
        })
    };
    let line = |axis: Axis, at_in: f32, colour: Color32| {
        let stroke = Stroke::new(1.0_f32, colour);
        match axis {
            Axis::Vertical => painter.vline(grid.to_screen((at_in, 0.0)).x.floor() + 0.5, art.y_range(), stroke),
            Axis::Horizontal => painter.hline(art.x_range(), grid.to_screen((0.0, at_in)).y.floor() + 0.5, stroke),
        }
    };
    for g in design.shown_guides().iter().filter(|g| st.drag.is_none_or(|d| d.from != Some(g.id))) {
        let full = st.hover == Some(g.id) || is_lit(g);
        line(g.axis, g.at_in, if full { ACCENT_PRIMARY } else { ACCENT_PRIMARY.gamma_multiply(0.6) });
    }
    if let Some(d) = held {
        line(d.axis, d.at_in, ACCENT_PRIMARY);
    }

    let marked = held.map(|d| (d.axis, d.at_in)).or_else(|| st.hover.and_then(|id| design.guide(id)).filter(|_| st.drag.is_none()).map(|g| (g.axis, g.at_in)));
    let Some((axis, at_in)) = marked else { return };
    let (top, left, _) = rulers(art);
    let over = painter.clone().with_layer_id(LayerId::new(Order::Middle, state_id().with("marks")));
    let marker = match axis {
        Axis::Vertical => {
            let x = grid.to_screen((at_in, 0.0)).x;
            [pos2(x - MARKER_PX, top.bottom() - MARKER_PX - 1.0), pos2(x + MARKER_PX, top.bottom() - MARKER_PX - 1.0), pos2(x, top.bottom())]
        }
        Axis::Horizontal => {
            let y = grid.to_screen((0.0, at_in)).y;
            [pos2(left.right() - MARKER_PX - 1.0, y - MARKER_PX), pos2(left.right() - MARKER_PX - 1.0, y + MARKER_PX), pos2(left.right(), y)]
        }
    };
    over.with_clip_rect(top.union(left).intersect(painter.clip_rect())).add(egui::Shape::convex_polygon(marker.to_vec(), ACCENT_PRIMARY, Stroke::NONE));
    if held.is_some()
        && let Some(p) = ui.input(|i| i.pointer.latest_pos())
    {
        let name = match axis {
            Axis::Vertical => "X",
            Axis::Horizontal => "Y",
        };
        super::measure_tool::readout(ui, &over, format!("{name} {}", units.len(at_in)), p);
    }
}
