//! Spec 49: live shape parameters. The Shapes panel and palette place parametric presets, the inspector
//! shows a Shape section per parameter, the canvas drags a rectangle's corner handles, and node editing
//! converts a parametric shape to a path. Old projects upgrade on open.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2, Vec2, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::design::ShapeKind;
use crate::parametric::{ArcClose, Params};
use crate::ui::canvas::Tool;
use crate::ui::test_harness::{Fixture, assert_snapshot, click_at, drag_along, key, snapshot_harness};

const SIZE: Vec2 = vec2(1400.0, 900.0);

/// A map from world inches to screen points, found by dropping a probe shape at a known point and
/// reading back where it landed, then clearing it.
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

/// Inserts the free shape whose tile is `label` through the Shapes panel, as a user would, and leaves it
/// selected. The panel is closed again so the canvas is clear.
fn insert(fixture: &mut Fixture, label: &str) -> u64 {
    fixture.get_by_label("Shapes").click();
    fixture.run();
    fixture.get_by_label(label).click();
    fixture.run();
    fixture.get_by_label("Shapes").click();
    fixture.run();
    fixture.state().selection[0]
}

fn params(fixture: &Fixture, id: u64) -> Params {
    match fixture.state().design.shapes.iter().find(|s| s.id == id).unwrap().kind {
        ShapeKind::Parametric(p) => p,
        ref other => panic!("{other:?} is not parametric"),
    }
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

#[test]
fn setting_points_in_the_inspector_changes_the_star_in_one_undo_step() {
    let mut fixture = snapshot_harness(SIZE);
    let star = insert(&mut fixture, "Star");
    assert!(matches!(params(&fixture, star), Params::Star { points: 5, .. }), "a fresh star has five points");
    assert_snapshot(&mut fixture, "49-shape-section");

    let depth = fixture.state().history.depth();
    set_field(&mut fixture, "Points", "8");
    assert!(matches!(params(&fixture, star), Params::Star { points: 8, .. }), "the canvas shows eight points");
    assert_eq!(fixture.state().history.depth(), depth + 1, "one commit, one undo step");

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(matches!(params(&fixture, star), Params::Star { points: 5, .. }), "Undo shows five");
}

#[test]
fn dragging_a_corner_handle_rounds_the_rectangle_and_reads_out_the_radius() {
    let mut fixture = snapshot_harness(SIZE);
    let map = grid_map(&mut fixture);
    let rect = insert(&mut fixture, "Square");
    // Put it at a known spot so the corner handle's screen point is easy to reach.
    let s = fixture.state_mut().design.shape_mut(rect).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    fixture.run();
    assert!(matches!(params(&fixture, rect), Params::Rectangle { corners: [0.0, 0.0, 0.0, 0.0], .. }), "it starts square");

    // The top-left handle sits at the corner; drag it inward along the diagonal.
    drag_along(&mut fixture, &[map((3.0, 3.0)), map((3.4, 3.4))]);
    let Params::Rectangle { corners, .. } = params(&fixture, rect) else { unreachable!() };
    assert!(corners.iter().all(|&r| r > 0.1), "every linked corner rounds: {corners:?}");
    assert_snapshot(&mut fixture, "49-corner-handles");
}

#[test]
fn pressing_a_converts_a_star_to_a_path_with_a_notice_and_undo_brings_it_back() {
    let mut fixture = snapshot_harness(SIZE);
    let star = insert(&mut fixture, "Star");
    key(&mut fixture, Key::A, Modifiers::NONE);
    assert!(matches!(fixture.state().design.shapes.iter().find(|s| s.id == star).unwrap().kind, ShapeKind::Path { .. }), "the star becomes a path");
    fixture.get_by_label("Editing nodes drops the shape's parameters.");

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(matches!(params(&fixture, star), Params::Star { .. }), "Undo makes it a star again");
    // Its Shape section is back, so the Points field is there once more.
    fixture.get_by_role_and_label(Role::TextInput, "Points");
}

#[test]
fn an_old_pentagon_opens_and_the_inspector_shows_its_sides() {
    let old = r#"{"format_version":13,"id":"penta","name":"Old","design":{"shapes":[{"id":1,"kind":"Pentagon","x_in":1.0,"y_in":1.0,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]}]},"created":1,"modified":1}"#;
    let (project, _) = crate::project::parse(old).expect("a pre-spec project opens");
    assert!(matches!(project.design.shapes[0].kind, ShapeKind::Parametric(Params::Polygon { sides: 5, .. })), "it upgrades to a five-sided polygon");

    let mut fixture = snapshot_harness(SIZE);
    fixture.state_mut().design = project.design;
    fixture.state_mut().selection = vec![1];
    fixture.run();
    let sides = fixture.get_by_role_and_label(Role::TextInput, "Sides").value().unwrap_or_default();
    assert_eq!(sides, "5", "the inspector shows Sides 5");
}

#[test]
fn an_arc_draws_as_a_pie_a_chord_or_an_open_line_by_its_closure() {
    let mut fixture = snapshot_harness(SIZE);
    for (i, close) in [ArcClose::Pie, ArcClose::Chord, ArcClose::Open].into_iter().enumerate() {
        let kind = ShapeKind::Parametric(Params::Arc { start_deg: 0.0, sweep_deg: 270.0, inner: 0.0, close });
        let id = fixture.state_mut().design.add_shape(kind);
        let s = fixture.state_mut().design.shape_mut(id).unwrap();
        (s.x_in, s.y_in) = (1.0 + i as f32 * 3.0, 3.0);
    }
    fixture.run();
    assert_snapshot(&mut fixture, "49-arc");
}
