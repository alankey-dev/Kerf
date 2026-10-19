//! Spec 45: ruler guides pulled from the rulers, snapped to, typed, locked and dragged back to delete.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2, pos2};
use egui_kittest::kittest::Queryable;

use crate::edit::{Edit, Subject, run};
use crate::guides::{Axis, GuideEdit};
use crate::ui::test_harness::{Fixture, assert_snapshot, double_click_at, drag_along, frame, key, move_to, press_at, rgb, snapshot_harness, two_squares_apart};
use crate::ui::tokens::{ACCENT_PRIMARY, RULER};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// Two squares, nothing selected, and the map from inches to the screen.
fn board(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> Pos2 + use<> {
    two_squares_apart(fixture)
}

fn add_guide(fixture: &mut Fixture, axis: Axis, at_in: f32) -> u64 {
    let app = fixture.state_mut();
    run(&mut app.doc.design, &mut app.doc.selection, &Edit::Guide(GuideEdit::Add { axis, at_in }), Subject::Selection).unwrap();
    let id = app.doc.design.guides.last().unwrap().id;
    fixture.run();
    id
}

/// The points a drag passes through, so egui sees it start, move and end.
fn path(from: Pos2, to: Pos2) -> Vec<Pos2> {
    (0..=8).map(|i| from + (to - from) * (i as f32 / 8.0)).collect()
}

fn guides(fixture: &Fixture) -> Vec<(Axis, f32)> {
    fixture.state().design.guides.iter().map(|g| (g.axis, g.at_in)).collect()
}

/// The middle of the top ruler above `x_in`.
fn top_ruler(at: &impl Fn((f32, f32)) -> Pos2, x_in: f32) -> Pos2 {
    at((x_in, 0.0)) - egui::vec2(0.0, RULER / 2.0)
}

fn near(a: [u8; 3], b: egui::Color32, tolerance: u8) -> bool {
    a.iter().zip([b.r(), b.g(), b.b()]).all(|(&x, y)| x.abs_diff(y) <= tolerance)
}

#[test]
fn a_guide_pulled_from_the_top_ruler_lands_where_released_and_undoes_in_one_step() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    let depth = fixture.state().history.depth();
    drag_along(&mut fixture, &path(top_ruler(&at, 8.0), at((8.0, 2.0))));
    let placed = guides(&fixture);
    assert_eq!(placed.len(), 1, "{placed:?}");
    assert_eq!(placed[0].0, Axis::Horizontal);
    assert!((placed[0].1 - 2.0).abs() < 1e-3, "{placed:?}");
    assert_eq!(fixture.state().history.depth(), depth + 1, "the pull is one undo step");
    assert!(fixture.state().selection.is_empty(), "a guide never selects");

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(guides(&fixture).is_empty());
}

#[test]
fn a_pull_released_back_over_the_ruler_or_off_the_artboard_places_nothing() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    drag_along(&mut fixture, &[top_ruler(&at, 8.0), at((8.0, 1.0)), at((8.0, 2.0)), top_ruler(&at, 8.0)]);
    drag_along(&mut fixture, &path(top_ruler(&at, 8.0), at((14.0, 2.0))));
    assert!(guides(&fixture).is_empty());
}

#[test]
fn a_shape_dragged_near_a_guide_snaps_to_it_and_the_guide_lights() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    fixture.state_mut().design.set_snapping(true);
    // 3.06 sits between the grid lines at 3 and 3.125, so only the guide can explain the snap.
    add_guide(&mut fixture, Axis::Horizontal, 3.06);
    let line = |fixture: &mut Fixture| {
        let image = frame(fixture);
        let p = at((10.0, 3.06));
        rgb(&image, pos2(p.x, p.y.floor()))
    };
    let resting = line(&mut fixture);
    assert!(!near(resting, ACCENT_PRIMARY, 12), "at rest the guide is drawn at 60 %: {resting:?}");

    let square = fixture.state().design.shapes[0].clone();
    let grab = at((square.x_in + square.width_in / 2.0, square.y_in + square.height_in / 2.0));
    let to = grab + egui::vec2(0.0, (3.07 - square.y_in) * 72.0);
    move_to(&mut fixture, grab);
    press_at(&mut fixture, grab, true);
    for p in path(grab, to).into_iter().skip(1) {
        move_to(&mut fixture, p);
    }
    let held = line(&mut fixture);
    assert!(near(held, ACCENT_PRIMARY, 12), "the guide lights at full accent while snapped to: {held:?}");
    press_at(&mut fixture, to, false);
    fixture.run();
    let top = fixture.state().design.shapes[0].y_in;
    assert!((top - 3.06).abs() < 1e-4, "the top edge snapped to the guide: {top}");
}

#[test]
fn a_typed_expression_moves_the_guide_and_the_popover_shows_its_value() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    add_guide(&mut fixture, Axis::Vertical, 8.0);
    let depth = fixture.state().history.depth();
    double_click_at(&mut fixture, at((8.0, 6.0)));
    fixture.get_by_role_and_label(Role::TextInput, "Guide position").focus();
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::A);
    fixture.run();
    fixture.get_by_role_and_label(Role::TextInput, "Guide position").type_text("1in + 5mm");
    fixture.run();
    fixture.press_key(Key::Enter);
    fixture.run();

    let moved = guides(&fixture)[0].1;
    assert!((moved - 1.196_85).abs() < 1e-4, "{moved}");
    assert_eq!(fixture.state().history.depth(), depth + 1);
    assert_eq!(fixture.get_by_role_and_label(Role::TextInput, "Guide position").value().as_deref(), Some("1.197"));
    assert_snapshot(&mut fixture, "45-guide-popover");
}

#[test]
fn with_guides_locked_a_click_on_a_guide_selects_the_shape_beneath() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    let square = fixture.state().design.shapes[0].clone();
    let x = square.x_in + square.width_in / 2.0;
    add_guide(&mut fixture, Axis::Vertical, x);
    let crossing = at((x, square.y_in + square.height_in / 2.0));

    crate::ui::test_harness::click_at(&mut fixture, crossing);
    assert!(fixture.state().selection.is_empty(), "an unlocked guide takes the click");

    key(&mut fixture, Key::Semicolon, Modifiers::COMMAND | Modifiers::ALT);
    assert!(fixture.state().design.guides_locked, "the Lock guides shortcut locks them");
    crate::ui::test_harness::click_at(&mut fixture, crossing);
    assert_eq!(fixture.state().selection, [square.id], "the click reaches the shape");
    drag_along(&mut fixture, &path(crossing, crossing + egui::vec2(60.0, 0.0)));
    assert_eq!(guides(&fixture), [(Axis::Vertical, x)], "the locked guide does not move");
}

#[test]
fn a_guide_dragged_onto_its_ruler_is_deleted() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    add_guide(&mut fixture, Axis::Horizontal, 5.0);
    let depth = fixture.state().history.depth();
    drag_along(&mut fixture, &path(at((9.0, 5.0)), top_ruler(&at, 9.0)));
    assert!(guides(&fixture).is_empty());
    assert_eq!(fixture.state().history.depth(), depth + 1);
}

#[test]
fn a_hovered_guide_marks_its_ruler() {
    let mut fixture = snapshot_harness(SIZE);
    let at = board(&mut fixture);
    add_guide(&mut fixture, Axis::Vertical, 7.5);
    add_guide(&mut fixture, Axis::Horizontal, 4.25);
    move_to(&mut fixture, at((7.5, 8.0)));
    fixture.run();
    let image = assert_snapshot(&mut fixture, "45-guides");
    let marker = at((7.5, 0.0)) - egui::vec2(0.0, 3.0);
    assert!(near(rgb(&image, marker), ACCENT_PRIMARY, 12), "the top ruler shows the hovered guide's marker");
}

#[test]
fn view_rows_hide_and_clear_guides_each_in_one_step() {
    let mut fixture = snapshot_harness(SIZE);
    _ = board(&mut fixture);
    add_guide(&mut fixture, Axis::Vertical, 2.0);
    let depth = fixture.state().history.depth();
    key(&mut fixture, Key::Semicolon, Modifiers::COMMAND);
    assert!(fixture.state().design.guides_hidden, "the Show guides shortcut hides them");
    fixture.get_by_label("View").click();
    fixture.run();
    fixture.get_by_label("Clear guides").click();
    fixture.run();
    assert!(guides(&fixture).is_empty());
    assert_eq!(fixture.state().history.depth(), depth + 2);
}
