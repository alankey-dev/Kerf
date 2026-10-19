//! View navigation (spec 48): the fits, zoom about a point, the pan clamp, the Space hand, middle-button
//! and arrow-key panning, and the 120 ms glide between views. The maths are pure functions over a
//! [`Camera`]; [`input`] and [`run`] apply them to a [`View`].
//!
//! The canvas maps a point `p` in inches to the screen as
//! `viewport.center() + RULER / 2 + zoom * (pan_pt + 72 * (p - artboard / 2))`, the same as
//! [`View::artboard_rect`].

use egui::{CursorIcon, Key, PointerButton, Pos2, Rect, Response, Ui, Vec2, vec2};

use crate::design::{Design, POINTS_PER_INCH};
use crate::edit::AppCommand;
use crate::snap::Aabb;

use super::canvas::{View, ZOOM_MAX, ZOOM_MIN, next_zoom};
use super::tokens::RULER;

/// Free space kept around whatever a fit shows.
pub const FIT_MARGIN_PX: f32 = 24.0;
/// Zoom to selection stops here, so a tiny shape does not fill the screen with one curve.
pub const SELECTION_MAX_ZOOM: f32 = 8.0;
/// However far it is panned, this much of the artboard stays on screen on each axis.
pub const KEEP_VISIBLE_PX: f32 = 48.0;
/// The Zoom to menu's levels, in per cent.
pub const ZOOM_TO_PCT: [u32; 6] = [25, 50, 100, 200, 400, 800];
const GLIDE_S: f64 = 0.12;
/// One arrow-key pan, or five times as far with Shift.
const ARROW_PAN_PX: f32 = 48.0;
const ARROW_PAN_BIG_PX: f32 = 240.0;

/// Where the view looks: the zoom, and the artboard centre's offset from the viewport centre in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    pub pan_pt: Vec2,
}

/// What navigation keeps across frames, per view.
#[derive(Default)]
pub struct Nav {
    /// The canvas as last drawn; the fits size to it.
    pub viewport: Option<Rect>,
    /// The pointer over the canvas last frame, which Zoom In and Zoom Out zoom about.
    pointer: Option<Pos2>,
    /// The input clock last frame, in seconds.
    now: f64,
    /// Whether view changes glide this frame: not under test, nor when animations are turned off.
    animate: bool,
    glide: Option<Glide>,
    /// A drag begun with Space held, which keeps panning until the button is released.
    hand_drag: bool,
}

#[derive(Clone, Copy)]
struct Glide {
    from: Camera,
    to: Camera,
    start: f64,
}

/// The camera that shows `target` (in inches) whole inside `viewport`, centred, with [`FIT_MARGIN_PX`]
/// around it and `lead_px` more above and to the left (the rulers, when fitting the artboard).
pub fn fit(target: Aabb, artboard_in: (f32, f32), viewport: Rect, lead_px: f32, max_zoom: f32) -> Camera {
    let room = viewport.size() - Vec2::splat(2.0 * FIT_MARGIN_PX + lead_px);
    let size_pt = vec2(target.width(), target.height()).max(Vec2::splat(1e-3)) * POINTS_PER_INCH;
    let zoom = (room.x / size_pt.x).min(room.y / size_pt.y).clamp(ZOOM_MIN, max_zoom);
    let (cx, cy) = target.centre();
    let half = vec2(artboard_in.0, artboard_in.1) / 2.0;
    let pan_pt = Vec2::splat((lead_px - RULER) / (2.0 * zoom)) - (vec2(cx, cy) - half) * POINTS_PER_INCH;
    Camera { zoom, pan_pt }
}

/// `cam` zoomed to `zoom` with the point under `anchor` held still. `viewport` is the canvas on screen.
pub fn zoom_about(cam: Camera, zoom: f32, anchor: Pos2, viewport: Rect) -> Camera {
    let from_centre = anchor - content_centre(viewport);
    Camera { zoom, pan_pt: cam.pan_pt + from_centre / zoom - from_centre / cam.zoom }
}

/// The pan nearest `pan_pt` that keeps [`KEEP_VISIBLE_PX`] of the artboard (or all of it, when smaller)
/// inside a viewport of `viewport_px` on each axis.
pub fn clamp_pan(pan_pt: Vec2, zoom: f32, artboard_in: (f32, f32), viewport_px: Vec2) -> Vec2 {
    let art_px = vec2(artboard_in.0, artboard_in.1) * POINTS_PER_INCH * zoom;
    let axis = |pan_px: f32, art: f32, vp: f32| {
        let keep = KEEP_VISIBLE_PX.min(art).min(vp);
        let lo = -vp / 2.0 - RULER / 2.0 + keep - art / 2.0;
        let hi = vp / 2.0 - RULER / 2.0 - keep + art / 2.0;
        pan_px.clamp(lo, hi)
    };
    let pan_px = pan_pt * zoom;
    vec2(axis(pan_px.x, art_px.x, viewport_px.x), axis(pan_px.y, art_px.y, viewport_px.y)) / zoom
}

/// Partway from `from` to `to` at `t` in 0..=1, eased; the zoom moves by ratio so a big jump reads even.
fn tween(from: Camera, to: Camera, t: f32) -> Camera {
    let t = t * t * (3.0 - 2.0 * t);
    let zoom = (from.zoom.ln() + (to.zoom.ln() - from.zoom.ln()) * t).exp();
    Camera { zoom, pan_pt: from.pan_pt + (to.pan_pt - from.pan_pt) * t }
}

/// The point [`View::artboard_rect`] measures from.
fn content_centre(viewport: Rect) -> Pos2 {
    viewport.center() + Vec2::splat(RULER / 2.0)
}

fn camera(view: &View) -> Camera {
    Camera { zoom: view.zoom, pan_pt: view.pan_pt }
}

fn set(view: &mut View, cam: Camera) {
    view.zoom = cam.zoom;
    view.pan_pt = cam.pan_pt;
}

/// Where the view is headed: the end of a glide in progress, else where it is.
fn destination(view: &View) -> Camera {
    view.nav.glide.map_or_else(|| camera(view), |g| g.to)
}

/// Moves the view to `to`, its pan clamped, gliding there when animations are on.
pub fn go(view: &mut View, to: Camera) {
    let Some(viewport) = view.nav.viewport else { return };
    let to = Camera { zoom: to.zoom, pan_pt: clamp_pan(to.pan_pt, to.zoom, view.artboard_in, viewport.size()) };
    if view.nav.animate {
        view.nav.glide = Some(Glide { from: camera(view), to, start: view.nav.now });
    } else {
        view.nav.glide = None;
        set(view, to);
    }
}

/// The frame of the selection, its groups' members included, in inches.
fn selection_bounds(design: &Design, selection: &[u64]) -> Aabb {
    let reach = design.with_descendants(selection);
    design.drawn().filter(|s| reach.contains(&s.id)).map(|s| design.geometry(s).bounds()).fold(Aabb::EMPTY, Aabb::union)
}

fn artboard(view: &View) -> Aabb {
    Aabb { min_x: 0.0, min_y: 0.0, max_x: view.artboard_in.0, max_y: view.artboard_in.1 }
}

/// Runs a zoom command. Zoom In and Out step about the pointer when it is over the canvas, about the
/// view centre otherwise; the other levels hold the view centre. Other commands are ignored.
pub fn run(view: &mut View, design: &Design, selection: &[u64], app: &AppCommand) {
    let Some(viewport) = view.nav.viewport else { return };
    let to = destination(view);
    let level = |zoom: f32| zoom_about(to, zoom, content_centre(viewport), viewport);
    match app {
        AppCommand::ZoomIn | AppCommand::ZoomOut => {
            let anchor = view.nav.pointer.unwrap_or(content_centre(viewport));
            let zoom = next_zoom(to.zoom, *app == AppCommand::ZoomIn);
            go(view, zoom_about(to, zoom, anchor, viewport));
        }
        AppCommand::ResetZoom => go(view, level(1.0)),
        AppCommand::ZoomTo(pct) => go(view, level((*pct as f32 / 100.0).clamp(ZOOM_MIN, ZOOM_MAX))),
        AppCommand::ZoomToFit => view.fit_to(artboard(view), RULER, ZOOM_MAX),
        AppCommand::ZoomToSelection => {
            let frame = selection_bounds(design, selection);
            if !frame.is_empty() {
                view.fit_to(frame, 0.0, SELECTION_MAX_ZOOM);
            }
        }
        AppCommand::ZoomToAll => {
            let all = design.drawn().map(|s| design.geometry(s).bounds()).fold(Aabb::EMPTY, Aabb::union);
            match all.is_empty() {
                true => view.fit_to(artboard(view), RULER, ZOOM_MAX),
                false => view.fit_to(all, 0.0, ZOOM_MAX),
            }
        }
        _ => {}
    }
}

/// The canvas's navigation input for this frame, read before any tool: the glide, scroll to pan, pinch
/// and Cmd+scroll to zoom about the pointer, the Space hand, middle-button drag and, when `arrows_pan`,
/// the arrow keys. Returns true while the hand owns the pointer, so no tool sees it this frame.
pub fn input(ui: &Ui, response: &Response, view: &mut View, viewport: Rect, arrows_pan: bool) -> bool {
    let (now, scroll, pinch, hover, shift) = ui.input(|i| (i.time, i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos(), i.modifiers.shift));
    let nav = &mut view.nav;
    nav.viewport = Some(viewport);
    nav.now = now;
    nav.animate = !cfg!(test) && ui.style().animation_time > 0.0;
    nav.pointer = hover.filter(|_| response.hovered());
    if let Some(g) = nav.glide {
        let t = ((now - g.start) / GLIDE_S).clamp(0.0, 1.0) as f32;
        nav.glide = (t < 1.0).then_some(g);
        set(view, tween(g.from, g.to, t));
        ui.ctx().request_repaint();
    }

    if response.hovered() && scroll != Vec2::ZERO {
        view.nav.glide = None;
        view.pan_pt += scroll / view.zoom;
    }
    if response.hovered()
        && pinch != 1.0
        && let Some(p) = hover
    {
        view.nav.glide = None;
        let zoom = (view.zoom * pinch).clamp(ZOOM_MIN, ZOOM_MAX);
        set(view, zoom_about(camera(view), zoom, p, viewport));
    }

    // While a text is being edited Space types a space, so the hand needs the keyboard to be free.
    let keys_free = !ui.ctx().wants_keyboard_input() && view.text_edit.is_none();
    let space = keys_free && ui.input(|i| i.key_down(Key::Space));
    let nav = &mut view.nav;
    if nav.hand_drag && !response.dragged() && !response.drag_stopped() {
        nav.hand_drag = false;
    }
    if space && response.drag_started_by(PointerButton::Primary) {
        nav.hand_drag = true;
    }
    let middle = response.dragged_by(PointerButton::Middle) || response.drag_stopped_by(PointerButton::Middle) || ui.input(|i| i.pointer.middle_down());
    let hand = space || middle || nav.hand_drag;
    let dragging = (hand && response.dragged_by(PointerButton::Primary)) || response.dragged_by(PointerButton::Middle);
    if dragging {
        nav.glide = None;
        view.pan_pt += response.drag_delta() / view.zoom;
    }
    if hand && (response.hovered() || dragging) {
        ui.ctx().set_cursor_icon(if dragging { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }

    if arrows_pan && keys_free {
        let arrows = [(Key::ArrowLeft, vec2(-1.0, 0.0)), (Key::ArrowRight, vec2(1.0, 0.0)), (Key::ArrowUp, vec2(0.0, -1.0)), (Key::ArrowDown, vec2(0.0, 1.0))];
        let step = if shift { ARROW_PAN_BIG_PX } else { ARROW_PAN_PX };
        let moved: Vec2 = ui.input(|i| arrows.iter().filter(|(k, _)| i.key_pressed(*k)).fold(Vec2::ZERO, |sum, &(_, d)| sum + d));
        if moved != Vec2::ZERO {
            view.nav.glide = None;
            view.pan_pt -= moved * step / view.zoom;
        }
    }
    hand
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    const VIEWPORT: Rect = Rect { min: Pos2::ZERO, max: pos2(1000.0, 800.0) };

    fn view_at(cam: Camera) -> View {
        let mut view = View::default();
        set(&mut view, cam);
        view
    }

    /// Where `p`, in inches, lands on screen with the canvas's own mapping.
    fn on_screen(view: &View, p: (f32, f32)) -> Pos2 {
        view.artboard_rect(VIEWPORT).min + vec2(p.0, p.1) * POINTS_PER_INCH * view.zoom
    }

    fn square(x: f32, y: f32, w: f32, h: f32) -> Aabb {
        Aabb { min_x: x, min_y: y, max_x: x + w, max_y: y + h }
    }

    #[test]
    fn zoom_to_selection_caps_a_small_shape_at_800_and_centres_it() {
        let target = square(3.0, 4.0, 1.0, 1.0);
        let cam = fit(target, (12.0, 12.0), VIEWPORT, 0.0, SELECTION_MAX_ZOOM);
        assert_eq!(cam.zoom, 8.0);
        let centre = on_screen(&view_at(cam), target.centre());
        assert!(centre.distance(VIEWPORT.center()) < 0.01, "{centre:?}");
    }

    #[test]
    fn zoom_to_selection_fits_a_wide_shape_to_the_width_with_the_margin() {
        let target = square(1.0, 5.0, 10.0, 2.0);
        let cam = fit(target, (12.0, 12.0), VIEWPORT, 0.0, SELECTION_MAX_ZOOM);
        assert!((cam.zoom - (1000.0 - 2.0 * FIT_MARGIN_PX) / 720.0).abs() < 1e-5, "{}", cam.zoom);
        let view = view_at(cam);
        let (left, right) = (on_screen(&view, (1.0, 6.0)), on_screen(&view, (11.0, 6.0)));
        assert!((left.x - FIT_MARGIN_PX).abs() < 0.01 && (right.x - (1000.0 - FIT_MARGIN_PX)).abs() < 0.01, "{left:?} {right:?}");
        assert!((left.y - 400.0).abs() < 0.01);
    }

    #[test]
    fn fitting_the_artboard_leaves_room_for_the_rulers_and_the_margin() {
        let cam = fit(square(0.0, 0.0, 12.0, 12.0), (12.0, 12.0), VIEWPORT, RULER, ZOOM_MAX);
        let art = view_at(cam).artboard_rect(VIEWPORT);
        assert!((art.top() - RULER - FIT_MARGIN_PX).abs() < 0.01 && (art.bottom() - (800.0 - FIT_MARGIN_PX)).abs() < 0.01, "{art:?}");
        assert!(art.left() - RULER >= FIT_MARGIN_PX && art.right() <= 1000.0 - FIT_MARGIN_PX);
    }

    #[test]
    fn a_pointer_anchored_zoom_keeps_the_point_under_the_pointer() {
        let start = Camera { zoom: 1.0, pan_pt: vec2(40.0, -25.0) };
        let pointer = pos2(730.0, 210.0);
        let before = view_at(start);
        let art = before.artboard_rect(VIEWPORT);
        let world = ((pointer - art.min) / (POINTS_PER_INCH * before.zoom)).to_pos2();
        for zoom in [0.1, 0.35, 2.0, 5.5, 8.0] {
            let after = view_at(zoom_about(start, zoom, pointer, VIEWPORT));
            let moved = on_screen(&after, (world.x, world.y)).distance(pointer);
            assert!(moved < 0.5, "at {zoom} the point moved {moved} px");
        }
    }

    #[test]
    fn the_pan_clamp_keeps_48_px_of_the_artboard_in_view_at_every_zoom_step() {
        let zooms: Vec<f32> = std::iter::successors(Some(ZOOM_MIN), |&z| (z < ZOOM_MAX).then(|| next_zoom(z, true))).collect();
        assert_eq!(zooms.last(), Some(&ZOOM_MAX));
        for zoom in zooms {
            for far in [vec2(1e6, 1e6), vec2(-1e6, -1e6), vec2(1e6, -1e6), vec2(-1e6, 1e6)] {
                let pan_pt = clamp_pan(far, zoom, (12.0, 12.0), VIEWPORT.size());
                let seen = view_at(Camera { zoom, pan_pt }).artboard_rect(VIEWPORT).intersect(VIEWPORT);
                assert!(seen.width() >= KEEP_VISIBLE_PX - 0.01 && seen.height() >= KEEP_VISIBLE_PX - 0.01, "at {zoom} only {:?} is visible", seen.size());
            }
        }
    }

    #[test]
    fn the_pan_clamp_leaves_a_pan_in_range_alone() {
        assert_eq!(clamp_pan(vec2(30.0, -50.0), 1.0, (12.0, 12.0), VIEWPORT.size()), vec2(30.0, -50.0));
    }

    #[test]
    fn a_glide_runs_from_start_to_end_with_the_zoom_by_ratio() {
        let (from, to) = (Camera { zoom: 1.0, pan_pt: Vec2::ZERO }, Camera { zoom: 4.0, pan_pt: vec2(100.0, -40.0) });
        assert_eq!(tween(from, to, 0.0), from);
        assert!((tween(from, to, 1.0).zoom - 4.0).abs() < 1e-5);
        assert!((tween(from, to, 0.5).zoom - 2.0).abs() < 1e-5, "halfway in ratio is the geometric mean");
    }
}
