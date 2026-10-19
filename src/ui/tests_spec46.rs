//! Spec 46: the editable grid. The Grid popover from the ruler corner, its presets, the two snap
//! switches, and units that leave the grid where it is.

use egui::{Key, Modifiers, Pos2, Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, bare, bare_frame, drag_along, key, move_to, rect_of, right_click_at, snapshot_harness, two_squares_apart};
use super::tokens::{BG_ARTBOARD, GRID_MINOR};
use crate::units::Units;

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);
/// Clear of the two squares and of every horizontal grid line.
const ROW_IN: f32 = 6.1;

fn rgb(c: egui::Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// The pixel a vertical grid line `x_in` from the artboard edge paints.
fn column(to_screen: &impl Fn((f32, f32)) -> Pos2, x_in: f32) -> Pos2 {
    let p = to_screen((x_in, ROW_IN));
    pos2(p.x.round() + 0.5, p.y)
}

fn open_grid_panel(harness: &mut Harness<'_, Kerf>) {
    let corner = rect_of(harness, "Ruler units").center();
    right_click_at(harness, corner);
    assert!(harness.query_by_label("Spacing").is_some(), "a right-click on the ruler corner opens the Grid popover");
}

fn close_grid_panel(harness: &mut Harness<'_, Kerf>) {
    key(harness, Key::Escape, Modifiers::NONE);
    assert!(harness.query_by_label("Spacing").is_none(), "Escape closes it");
}

/// Drags the first square by `dx_in` from its centre and returns where its left edge lands.
fn drag_first_by(harness: &mut Harness<'_, Kerf>, to_screen: &impl Fn((f32, f32)) -> Pos2, dx_in: f32) -> f32 {
    let s = harness.state().design.shapes[0].clone();
    // Grab off the centre so the drag moves the shape rather than its pivot crosshair (spec 52).
    let (cx, cy) = (s.x_in + s.width_in / 4.0, s.y_in + s.height_in / 2.0);
    drag_along(harness, &[to_screen((cx, cy)), to_screen((cx + dx_in / 2.0, cy)), to_screen((cx + dx_in, cy))]);
    harness.state().design.shapes[0].x_in
}

#[test]
fn the_quarter_inch_preset_draws_quarters_and_snaps_to_them() {
    let mut harness = snapshot_harness(SIZE);
    let to_screen = two_squares_apart(&mut harness);
    harness.state_mut().design.snap.grid = true;
    move_to(&mut harness, pos2(1300.0, 850.0));
    assert_ne!(bare(&mut harness, column(&to_screen, 0.125)), rgb(BG_ARTBOARD), "the default grid has an eighth inch line");

    open_grid_panel(&mut harness);
    assert_snapshot(&mut harness, "46-grid-panel");
    harness.get_by_label("1/4 in").click();
    harness.run();
    let grid = harness.state().design.grid.expect("the preset sets the grid");
    assert_eq!((grid.major_in, grid.subdivisions, grid.snap_in()), (1.0, 4, 0.25));
    assert_eq!(harness.state().history.depth(), 1, "one undo step");
    close_grid_panel(&mut harness);
    move_to(&mut harness, pos2(1300.0, 850.0));

    let frame = bare_frame(&mut harness);
    let at = |x_in: f32| {
        let p = column(&to_screen, x_in);
        let px = frame.get_pixel(p.x as u32, p.y as u32).0;
        [px[0], px[1], px[2]]
    };
    for quarter in [0.25, 0.5, 0.75, 1.25, 7.75] {
        assert_eq!(at(quarter), rgb(GRID_MINOR), "a subdivision at {quarter}");
    }
    for eighth in [0.125, 0.375, 7.625] {
        assert_eq!(at(eighth), rgb(BG_ARTBOARD), "no line at {eighth} now");
    }

    let x = drag_first_by(&mut harness, &to_screen, 0.23);
    assert!((x - 1.25).abs() < 1e-4, "a drag to 1.23 snaps to the 1.25 line, at {x}");
    let x = drag_first_by(&mut harness, &to_screen, 0.12);
    assert!((x - 1.37).abs() < 0.01, "1.37 is near no quarter, and the old eighth at 1.375 is gone, at {x}");
}

#[test]
fn with_snap_to_grid_off_a_drag_still_meets_another_shapes_edge() {
    let mut harness = snapshot_harness(SIZE);
    let to_screen = two_squares_apart(&mut harness);
    harness.state_mut().design.set_snapping(true);
    harness.state_mut().design.shapes[1].x_in = 4.1;
    harness.run();

    open_grid_panel(&mut harness);
    harness.get_by_label("Snap to grid").click();
    harness.run();
    close_grid_panel(&mut harness);
    let design = &harness.state().design;
    assert!(!design.snap.grid && design.snap.shape_edges, "only the grid snap is off");
    assert_eq!(harness.get_by_label("Snapping").toggled(), Some(egui::accesskit::Toggled::True), "the magnet still reads on");

    let x = drag_first_by(&mut harness, &to_screen, 0.23);
    assert!((x - 1.23).abs() < 0.01, "no grid line pulls it to 1.25, at {x}");
    let width = harness.state().design.shapes[0].width_in;
    let x = drag_first_by(&mut harness, &to_screen, 4.07 - width - x);
    assert!((x + width - 4.1).abs() < 1e-4, "its right edge meets the other square's left edge at 4.1, at {}", x + width);
}

#[test]
fn switching_to_centimetres_leaves_the_grid_lines_where_they_are() {
    let mut harness = snapshot_harness(SIZE);
    let to_screen = two_squares_apart(&mut harness);
    let away = pos2(1300.0, 850.0);
    move_to(&mut harness, away);
    let inside = Rect::from_min_max(to_screen((0.1, 0.1)), to_screen((11.9, 11.9))).intersect(Rect::from_min_size(Pos2::ZERO, SIZE));
    let pixels = |image: &image::RgbaImage| -> Vec<[u8; 4]> {
        let (x0, x1, y0, y1) = (inside.left() as u32, inside.right() as u32, inside.top() as u32, inside.bottom() as u32);
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| image.get_pixel(x, y).0).collect()
    };
    let before = pixels(&bare_frame(&mut harness));

    let corner = rect_of(&harness, "Ruler units").center();
    super::test_harness::click_at(&mut harness, corner);
    assert_eq!(harness.state().units, Units::Metric);
    move_to(&mut harness, away);
    let after = pixels(&bare_frame(&mut harness));
    assert!(before == after, "the grid inside the artboard is unchanged");
    assert_eq!(harness.state().design.grid, None, "and the design is untouched");
}

#[test]
fn a_5mm_grid_with_its_ruler() {
    let mut harness = snapshot_harness(SIZE);
    harness.state_mut().units = Units::Metric;
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().design.shapes.clear();
    harness.run();
    open_grid_panel(&mut harness);
    harness.get_by_label("5 mm").click();
    harness.run();
    harness.get_by_label("Origin X").focus();
    harness.run();
    harness.get_by_label("Origin X").type_text("5mm");
    harness.run();
    harness.press_key(Key::Enter);
    harness.run();
    close_grid_panel(&mut harness);
    move_to(&mut harness, pos2(1300.0, 850.0));
    let grid = harness.state().design.grid.unwrap();
    assert!((grid.snap_in() - 0.5 / 2.54).abs() < 1e-5 && (grid.origin_in.0 - 0.5 / 2.54).abs() < 1e-5, "{grid:?}");
    assert_snapshot(&mut harness, "46-grid-5mm");
}
