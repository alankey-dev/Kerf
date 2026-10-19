//! Spec 34: Inset alongside Offset, with Round, Miter and Bevel corners.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::design::{Contour, Seg, ShapeKind};
use crate::edit::Edit;
use crate::effects::{Direction, Effect, Local, Offset, Reach, Style};
use crate::geometry::{DEFAULT_MITER_LIMIT, Join};
use crate::ui::Kerf;
use crate::ui::test_harness::{Fixture, assert_snapshot, key, snapshot_harness};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// One selected 2 x 2 square at (3, 3) with a live 0.25 in round outset, and nothing to undo.
fn square_with_offset() -> (Fixture, u64) {
    let mut harness = snapshot_harness(SIZE);
    let design = &mut harness.state_mut().design;
    let id = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    let ring = Offset { distance_in: 0.25, direction: Direction::Outset, join: Join::Round, style: None };
    design.apply(&Edit::AddEffect { effect: Effect::Offset(ring), reach: Reach::EachShape }, &[id]).unwrap();
    harness.state_mut().selection = vec![id];
    harness.run();
    (harness, id)
}

fn offset_of(harness: &Harness<'_, Kerf>, id: u64) -> Offset {
    match harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().effects.entries()[0].effect {
        Effect::Offset(o) => o,
        other => panic!("not an Offset: {other:?}"),
    }
}

/// The world width of the shape's Offset piece.
fn ring_width(harness: &Harness<'_, Kerf>, id: u64) -> f32 {
    let design = &harness.state().design;
    let geometry = design.geometry(design.shapes.iter().find(|s| s.id == id).unwrap());
    let b = geometry.pieces().iter().find(|p| p.id.local != Local::Body).unwrap().bounds;
    b.max_x - b.min_x
}

fn click(harness: &mut Harness<'_, Kerf>, label: &str) {
    harness.get_by_label(label).click();
    harness.run();
}

#[test]
fn switching_a_live_offset_to_inset_and_bevel_follows_in_the_geometry_and_the_row() {
    let (mut harness, id) = square_with_offset();
    let depth = harness.state().history.depth();
    click(&mut harness, "Offset 0.25 in outset, round");
    assert!(crate::ui::effects_list::editing(&harness.ctx).is_some(), "the chip opens the Offset editor");
    assert!(harness.query_by_role_and_label(Role::TextInput, "Miter limit").is_none(), "no limit while the corners are round");

    click(&mut harness, "Inset");
    assert_eq!(offset_of(&harness, id).direction, Direction::Inset);
    assert!((ring_width(&harness, id) - 1.5).abs() < 1e-3, "an inset 0.25 inside the 2 in square: {}", ring_width(&harness, id));
    assert!(harness.query_by_label("Offset 0.25 in inset, round").is_some(), "the row names the direction");
    assert_eq!(harness.state().history.depth(), depth + 1, "one step");

    click(&mut harness, "Bevel");
    assert_eq!(offset_of(&harness, id).join, Join::Bevel);
    assert!(harness.query_by_label("Offset 0.25 in inset, bevel").is_some(), "the row names the corners");
    assert_eq!(harness.state().history.depth(), depth + 2, "one step more");

    click(&mut harness, "Miter");
    assert_eq!(offset_of(&harness, id).join, Join::Miter { limit: DEFAULT_MITER_LIMIT });
    assert!(harness.query_by_role_and_label(Role::TextInput, "Miter limit").is_some(), "Miter shows its limit");

    for (back, expected) in [(depth + 2, Join::Bevel), (depth + 1, Join::Round)] {
        key(&mut harness, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!((harness.state().history.depth(), offset_of(&harness, id).join), (back, expected), "one Undo per change");
    }
    key(&mut harness, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(offset_of(&harness, id).direction, Direction::Outset);
    assert!((ring_width(&harness, id) - 2.5).abs() < 1e-3, "back to the outset");
}

#[test]
fn an_inset_wider_than_the_shape_says_so_and_stays_in_place() {
    let (mut harness, id) = square_with_offset();
    click(&mut harness, "Offset 0.25 in outset, round");
    click(&mut harness, "Inset");
    assert!(harness.query_by_label("Some parts are too thin for this inset").is_none());
    harness.get_by_role_and_label(Role::TextInput, "Offset distance").focus();
    harness.run();
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "Offset distance").type_text("1.5");
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    assert_eq!(offset_of(&harness, id).distance_in, 1.5);
    assert!(harness.query_by_label("Some parts are too thin for this inset").is_some(), "the editor says why nothing shows");
    let design = &harness.state().design;
    let shape = design.shapes.iter().find(|s| s.id == id).unwrap();
    assert!(shape.effects.entries()[0].enabled, "the entry stays, on");
    assert_eq!(design.geometry(shape).pieces().len(), 1, "and adds nothing");
}

/// A cross with arms 1.2 in wide in a 2.4 in box, whose notches turn an inset's corners.
fn cross() -> ShapeKind {
    let (a, b, c) = (0.0, 0.6, 1.8);
    let d = 2.4;
    let pts = [(b, a), (c, a), (c, b), (d, b), (d, c), (c, c), (c, d), (b, d), (b, c), (a, c), (a, b), (b, b)];
    let contour = Contour { start: pts[0], segs: pts[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true };
    ShapeKind::Imported { contours: vec![contour], view_box: (d, d) }
}

/// Outsets of stars on the top row, where the corner styles show on the points, and insets of crosses
/// below, where they show in the notches: Round, Miter and Bevel from left to right. The offsets are
/// violet so they read against the body.
#[test]
fn snapshot_outset_and_inset_with_each_corner_style() {
    let mut harness = snapshot_harness(SIZE);
    let design = &mut harness.state_mut().design;
    let joins = [Join::Round, Join::Miter { limit: DEFAULT_MITER_LIMIT }, Join::Bevel];
    for (row, direction) in [Direction::Outset, Direction::Inset].into_iter().enumerate() {
        for (col, join) in joins.into_iter().enumerate() {
            let id = design.add_shape(if direction == Direction::Outset { ShapeKind::Star } else { cross() });
            let s = design.shape_mut(id).unwrap();
            (s.x_in, s.y_in, s.width_in, s.height_in) = (1.5 + 3.0 * col as f32, 1.0 + 3.0 * row as f32, 2.4, 2.4);
            let style = Style { fill: egui::Color32::from_rgb(0x9b, 0x7b, 0xf0), ..Style::of(s) };
            let offset = Offset { distance_in: 0.3, direction, join, style: Some(style) };
            design.apply(&Edit::AddEffect { effect: Effect::Offset(offset), reach: Reach::EachShape }, &[id]).unwrap();
        }
    }
    harness.run();
    click(&mut harness, "Offset 0.3 in inset, miter");
    assert_snapshot(&mut harness, "34-inset");
}
