//! Spec 48: zoom to fit, zoom to selection, the Zoom to menu and the Space hand, driven through the app.

use egui::{Event, Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, click_at, drag_along, key, pick, snapshot_harness};
use super::tokens::RULER;
use super::view_nav::FIT_MARGIN_PX;
use super::{canvas, commands};
use crate::design::{Design, POINTS_PER_INCH, ShapeKind};
use crate::edit::AppCommand;
use crate::path_edit::NodePath;

const SIZE: egui::Vec2 = vec2(1200.0, 800.0);

fn viewport(harness: &Harness<'_, Kerf>) -> Rect {
    harness.state().view.nav.viewport.expect("the canvas has been drawn")
}

fn artboard(harness: &Harness<'_, Kerf>) -> Rect {
    harness.state().view.artboard_rect(viewport(harness))
}

/// Where the shape's frame sits on screen.
fn on_screen(harness: &Harness<'_, Kerf>, id: u64) -> Rect {
    let app = harness.state();
    let shape = app.design.shapes.iter().find(|s| s.id == id).unwrap();
    let b = app.design.geometry(shape).bounds();
    let (origin, px) = (artboard(harness).min, POINTS_PER_INCH * app.view.zoom);
    Rect::from_min_max(origin + vec2(b.min_x, b.min_y) * px, origin + vec2(b.max_x, b.max_y) * px)
}

fn pill(harness: &Harness<'_, Kerf>) -> Option<String> {
    harness.get_by_label("Zoom").value()
}

fn space(harness: &mut Harness<'_, Kerf>, pressed: bool) {
    harness.input_mut().events.push(Event::Key { key: Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    harness.run();
}

#[test]
fn zoom_in_three_times_then_cmd_0_shows_the_whole_artboard_with_the_margin() {
    let mut harness = snapshot_harness(SIZE);
    for _ in 0..3 {
        harness.get_by_label("Zoom in").click();
        harness.run();
    }
    assert_eq!(harness.state().view.zoom_pct(), 175);
    assert!(!viewport(&harness).contains_rect(artboard(&harness)), "zoomed in, the artboard overflows the view");

    key(&mut harness, Key::Num0, Modifiers::COMMAND);
    let (vp, art) = (viewport(&harness).shrink(FIT_MARGIN_PX), artboard(&harness));
    let with_rulers = Rect::from_min_max(art.min - vec2(RULER, RULER), art.max);
    assert!(vp.expand(0.5).contains_rect(with_rulers), "{with_rulers:?} inside {vp:?}");
    let tight = (with_rulers.top() - vp.top()).abs().min((with_rulers.left() - vp.left()).abs());
    assert!(tight < 0.5, "the fit fills one axis: {with_rulers:?} against {vp:?}");
}

#[test]
fn cmd_2_fills_the_view_with_a_small_selected_shape() {
    let mut harness = snapshot_harness(SIZE);
    let mut design = Design::default();
    let id = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (8.0, 9.0, 1.5, 1.5);
    harness.state_mut().design = design;
    harness.state_mut().selection = vec![id];
    harness.run();
    let small = on_screen(&harness, id);

    key(&mut harness, Key::Num2, Modifiers::COMMAND);
    let (vp, shape) = (viewport(&harness), on_screen(&harness, id));
    assert!(shape.height() > small.height() * 2.0, "{small:?} grew to {shape:?}");
    assert!((shape.height() - (vp.width().min(vp.height()) - 2.0 * FIT_MARGIN_PX)).abs() < 1.0, "the square fills the view less the margin");
    assert!(shape.center().distance(vp.center()) < 1.0, "and sits in its centre");
    let pct = harness.state().view.zoom_pct();
    assert_ne!(pct, 100);
    assert_eq!(pill(&harness), Some(format!("{pct}%")));
}

#[test]
fn zoom_to_selection_is_disabled_with_nothing_selected() {
    let harness = snapshot_harness(SIZE);
    let app = harness.state();
    let entry = commands::Cx::new(&app.design, &app.selection, app.view.align_to).app(AppCommand::ZoomToSelection, &app.history);
    assert_eq!(entry.tip_body(), "Select something to zoom to.");
}

#[test]
fn space_pans_mid_pen_path_and_the_path_carries_on() {
    let mut harness = snapshot_harness(SIZE);
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Pen;
    let c = viewport(&harness).center();
    click_at(&mut harness, c + vec2(-150.0, 0.0));
    click_at(&mut harness, c + vec2(0.0, -100.0));
    let before = artboard(&harness);

    space(&mut harness, true);
    drag_along(&mut harness, &[c, c + vec2(60.0, 40.0), c + vec2(120.0, 80.0)]);
    space(&mut harness, false);
    assert_eq!(artboard(&harness).min, before.min + vec2(120.0, 80.0), "the hand dragged the view");
    assert!(harness.state().tool == canvas::Tool::Pen, "the Pen tool came back");

    click_at(&mut harness, c + vec2(150.0, 0.0));
    key(&mut harness, Key::Enter, Modifiers::NONE);
    let shapes = &harness.state().design.shapes;
    assert_eq!(shapes.len(), 1, "one path");
    assert_eq!(NodePath::read(&shapes[0]).contours[0].nodes.len(), 3);
}

#[test]
fn the_zoom_pill_menu_sets_200_percent() {
    let mut harness = snapshot_harness(SIZE);
    harness.get_by_label("Zoom").click();
    harness.run();
    assert_snapshot(&mut harness, "48-zoom-menu");
    pick(&mut harness, "200%");
    assert_eq!(harness.state().view.zoom_pct(), 200);
    assert_eq!(pill(&harness).as_deref(), Some("200%"));
    assert!(harness.query_by_label("Zoom to fit").is_none(), "the menu closed");
}

#[test]
fn a_pinch_zooms_about_the_pointer() {
    let mut harness = snapshot_harness(SIZE);
    let pointer: Pos2 = viewport(&harness).center() + vec2(140.0, -90.0);
    let world = |h: &Harness<'_, Kerf>| (pointer - artboard(h).min) / (POINTS_PER_INCH * h.state().view.zoom);
    harness.input_mut().events.push(Event::PointerMoved(pointer));
    harness.run();
    let before = world(&harness);
    harness.input_mut().events.push(Event::Zoom(2.0));
    harness.run();
    assert_eq!(harness.state().view.zoom_pct(), 200);
    assert!((world(&harness) - before).length() * POINTS_PER_INCH * 2.0 < 0.5, "the point under the pointer stayed put");
}

#[test]
fn the_arrow_keys_pan_with_nothing_selected() {
    let mut harness = snapshot_harness(SIZE);
    let before = artboard(&harness);
    key(&mut harness, Key::ArrowRight, Modifiers::NONE);
    let after = artboard(&harness);
    assert!(after.left() < before.left() && after.top() == before.top(), "{before:?} to {after:?}");
}
