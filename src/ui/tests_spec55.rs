//! Spec 55: the Corner tool. Dragging a node's widget rounds it, a marquee plus a typed radius rounds
//! several nodes, and Round all corners rounds every point. Each change is one undo step, and Make sees
//! the rounded outline.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2, Vec2, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::corners::{Corner, CornerStyle};
use crate::design::ShapeKind;
use crate::ui::canvas::Tool;
use crate::ui::test_harness::{Fixture, assert_snapshot, click_at, drag_along, key, snapshot_harness};

const SIZE: Vec2 = vec2(1400.0, 900.0);

fn grid_map(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> Pos2 + use<> {
    fixture.state_mut().tool = Tool::Text;
    let probe = pos2(640.0, 420.0);
    click_at(fixture, probe);
    let text = fixture.state().design.shapes.last().unwrap().clone();
    let px = 72.0 * fixture.state().view.zoom;
    let origin = probe - vec2(text.x_in, text.y_in) * px;
    fixture.state_mut().design.remove_many(&[text.id]);
    fixture.state_mut().tool = Tool::Select;
    fixture.run();
    move |(x, y)| origin + vec2(x, y) * px
}

fn insert(fixture: &mut Fixture, label: &str) -> u64 {
    fixture.get_by_label("Shapes").click();
    fixture.run();
    fixture.get_by_label(label).click();
    fixture.run();
    fixture.get_by_label("Shapes").click();
    fixture.run();
    fixture.state().selection[0]
}

fn place(fixture: &mut Fixture, id: u64, x: f32, y: f32, w: f32, h: f32) {
    let s = fixture.state_mut().design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, w, h);
    fixture.run();
}

fn set_field(fixture: &mut Fixture, label: &str, text: &str) {
    fixture.get_by_role_and_label(Role::TextInput, label).focus();
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::A);
    fixture.run();
    fixture.get_by_role_and_label(Role::TextInput, label).type_text(text);
    fixture.run();
    fixture.press_key(Key::Enter);
    fixture.run();
}

fn path_corners(fixture: &Fixture, id: u64) -> Vec<Corner> {
    match &fixture.state().design.shapes.iter().find(|s| s.id == id).unwrap().kind {
        ShapeKind::Path { corners, .. } => corners.iter().flatten().copied().collect(),
        other => panic!("{other:?} is not a path"),
    }
}

fn is_parametric(fixture: &Fixture, id: u64) -> bool {
    matches!(fixture.state().design.shapes.iter().find(|s| s.id == id).unwrap().kind, ShapeKind::Parametric(_))
}

#[test]
fn dragging_a_square_corner_widget_rounds_it_and_one_undo_squares_it() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let sq = insert(&mut fixture, "Square");
    place(&mut fixture, sq, 3.0, 3.0, 2.0, 2.0);
    fixture.state_mut().tool = Tool::Corner;
    fixture.run();
    fixture.run();

    // Drag the top-left node's widget inward along the diagonal.
    drag_along(&mut fixture, &[map((3.0, 3.0)), map((3.5, 3.5))]);
    let corners = path_corners(&fixture, sq);
    assert!(corners.iter().any(|c| c.radius_in > 0.1), "the dragged corner rounds: {corners:?}");
    assert!(corners.iter().filter(|c| c.radius_in > 0.1).count() == 1, "only the one corner rounds: {corners:?}");
    assert_snapshot(&mut fixture, "55-corner-widget");

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(is_parametric(&fixture, sq), "one undo squares it back to a parametric rectangle");
}

#[test]
fn a_marquee_selects_two_nodes_and_typing_three_millimetres_rounds_both() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let sq = insert(&mut fixture, "Square");
    place(&mut fixture, sq, 3.0, 3.0, 2.0, 2.0);
    fixture.state_mut().tool = Tool::Corner;
    fixture.run();
    fixture.run();

    // Marquee the two top corners at (3, 3) and (5, 3).
    drag_along(&mut fixture, &[map((2.5, 2.5)), map((5.5, 3.5))]);
    set_field(&mut fixture, "Corner radius", "3mm");

    let set: Vec<Corner> = path_corners(&fixture, sq).into_iter().filter(|c| c.radius_in > 0.0).collect();
    assert_eq!(set.len(), 2, "both marqueed corners round, and no others");
    assert!(set.iter().all(|c| (c.radius_in - 3.0 / 25.4).abs() < 0.01), "each rounds to 3 mm: {set:?}");
}

#[test]
fn round_all_corners_rounds_every_point_of_a_star_and_make_sees_it() {
    let mut fixture = snapshot_harness(SIZE);
    let star = insert(&mut fixture, "Star");
    place(&mut fixture, star, 3.0, 3.0, 3.0, 3.0);
    fixture.state_mut().tool = Tool::Corner;
    fixture.run();
    fixture.run();

    let sharp = fixture.state().design.geometry(fixture.state().design.shapes.iter().find(|s| s.id == star).unwrap()).lines().map(|(p, _)| p.len()).sum::<usize>();

    fixture.get_by_label("Round all corners").click();
    fixture.run();

    let corners = path_corners(&fixture, star);
    assert!(corners.len() >= 10 && corners.iter().all(|c| c.radius_in > 0.0), "every star point rounds: {corners:?}");
    // Make reads the evaluated outline, so the rounded arcs add points the sharp star did not have.
    let rounded = fixture.state().design.geometry(fixture.state().design.shapes.iter().find(|s| s.id == star).unwrap()).lines().map(|(p, _)| p.len()).sum::<usize>();
    assert!(rounded > sharp, "the cut outline is the rounded one: {sharp} -> {rounded}");
}

#[test]
fn the_three_corner_styles_each_render() {
    let mut fixture = snapshot_harness(SIZE);
    let hex = insert(&mut fixture, "Hexagon");
    place(&mut fixture, hex, 3.0, 3.0, 4.0, 4.0);
    fixture.state_mut().tool = Tool::Corner;
    fixture.run();
    fixture.run();
    fixture.get_by_label("Round all corners").click();
    fixture.run();

    // Give the first three corners a visible radius and one of each style.
    let s = fixture.state_mut().design.shape_mut(hex).unwrap();
    if let ShapeKind::Path { corners, .. } = &mut s.kind {
        for (c, style) in corners[0].iter_mut().zip([CornerStyle::Round, CornerStyle::Chamfer, CornerStyle::Inverted].into_iter().cycle()) {
            c.radius_in = 0.6;
            c.style = style;
        }
    }
    fixture.run();
    assert_snapshot(&mut fixture, "55-three-styles");
}
