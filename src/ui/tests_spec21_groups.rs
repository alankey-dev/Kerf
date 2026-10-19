//! Spec 21 phase 2: live boolean and effect groups, driven through the real app.

use egui::{Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, assert_snapshot, bare, double_click_at, key, move_to, pick, pixel, press_at, right_click_at, snapshot_harness, two_squares_apart};
use crate::containers::{Axis, ContainerRole, Cross, Flow, Sizing};
use crate::design::{Combine, CompoundOp, Design, Pt, Shape, ShapeKind};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

type App<'a> = Harness<'a, Kerf>;

fn add(d: &mut Design, kind: ShapeKind, (x, y): Pt, (w, h): Pt) -> u64 {
    let id = d.add_shape(kind);
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, w, h);
    id
}

/// The app over `design`, nothing selected and nothing to undo, and a map from inches to screen points.
fn app(design: Design) -> (Fixture, impl Fn(Pt) -> Pos2) {
    let mut harness = snapshot_harness(SIZE);
    let to_screen = two_squares_apart(&mut harness);
    harness.state_mut().design = design;
    harness.run();
    (harness, to_screen)
}

fn shape(harness: &App<'_>, id: u64) -> Shape {
    harness.state().design.shapes.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no shape {id}")).clone()
}

/// Selects `ids` and picks `op` from the Layers bar's Combine popover. Returns the new group.
fn combine(harness: &mut App<'_>, ids: &[u64], op: &str) -> u64 {
    harness.state_mut().selection = ids.to_vec();
    harness.run();
    harness.get_by_label("Layers Combine").click();
    harness.run();
    harness.get_by_label(op).click();
    harness.run();
    let [g] = harness.state().selection[..] else { panic!("Combine leaves the group selected: {:?}", harness.state().selection) };
    assert!(matches!(shape(harness, g).compound(), Some(CompoundOp::Boolean(_))), "a live boolean group");
    g
}

fn undo(harness: &mut App<'_>) {
    key(harness, egui::Key::Z, egui::Modifiers::COMMAND);
}

/// The inspector field labelled `label` as it reads.
fn field(harness: &App<'_>, label: &str) -> String {
    harness.get_all_by_label(label).find_map(|n| n.value()).unwrap_or_else(|| panic!("no {label} field with a value"))
}

fn type_into(harness: &mut App<'_>, label: &str, text: &str) {
    let node = || harness.get_all_by_label(label).find(|n| n.value().is_some()).unwrap();
    node().focus();
    harness.run();
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_all_by_label(label).find(|n| n.value().is_some()).unwrap().type_text(text);
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-2
}

/// Turn `deg` into 0..360 so 450 and 90 compare equal.
fn turn(deg: f32) -> f32 {
    deg.rem_euclid(360.0)
}

/// A press held long enough to drag, moved through `path`, then released with `modifiers` held throughout.
fn drag_held(harness: &mut App<'_>, path: &[Pos2], modifiers: egui::Modifiers) {
    harness.input_mut().modifiers = modifiers;
    move_to(harness, path[0]);
    press_at(harness, path[0], true);
    harness.run_steps(10);
    for &p in &path[1..] {
        move_to(harness, p);
    }
    press_at(harness, *path.last().unwrap(), false);
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    harness.run();
}

/// A back square with a front square overlapping its right edge, welded.
fn weld_of_two() -> (Design, u64, u64) {
    let mut d = Design::default();
    let a = add(&mut d, ShapeKind::Square, (2.0, 2.0), (2.0, 2.0));
    let b = add(&mut d, ShapeKind::Square, (3.0, 3.0), (2.0, 1.0));
    (d, a, b)
}

#[test]
fn rotating_a_group_by_its_handle_in_steps_turns_it_and_its_children_to_90() {
    let (d, a, b) = weld_of_two();
    let (mut harness, to_screen) = app(d);
    let g = combine(&mut harness, &[a, b], "Weld");
    let was = [a, b].map(|id| shape(&harness, id).rotation_deg);
    let group = shape(&harness, g);
    let px = to_screen((1.0, 0.0)).x - to_screen((0.0, 0.0)).x;
    let frame = crate::transform::Frame::of(&harness.state().design, [&group]).unwrap();
    let centre = to_screen(group.centre_in());
    let handle = to_screen(frame.rotate_handle_pos(super::selection::ROTATE_ARM_PX / px));
    let r = handle.distance(centre);
    let arc: Vec<Pos2> = (0..=9).map(|i| (-90.0 + 10.0 * i as f32).to_radians()).map(|a| centre + vec2(a.cos(), a.sin()) * r).collect();
    drag_held(&mut harness, &arc, egui::Modifiers::NONE);

    let group = shape(&harness, g);
    assert!(near(turn(group.rotation_deg), 90.0), "the group turned to 90: {}", group.rotation_deg);
    assert_eq!(field(&harness, "Rotation"), "90", "the inspector reads 90");
    for (id, was) in [a, b].into_iter().zip(was) {
        assert!(near(turn(shape(&harness, id).rotation_deg), turn(was + 90.0)), "child {id} turned by 90: {}", shape(&harness, id).rotation_deg);
    }
    assert_eq!(harness.state().history.depth(), 2, "Combine, then the whole rotate drag as one step");
}

#[test]
fn a_typed_width_on_a_group_reads_back_and_scales_its_children() {
    let (d, a, b) = weld_of_two();
    let (mut harness, _) = app(d);
    let g = combine(&mut harness, &[a, b], "Weld");
    let was = shape(&harness, g).width_in;
    let widths = [a, b].map(|id| shape(&harness, id).width_in);
    type_into(&mut harness, "W", "4");
    assert!(near(shape(&harness, g).width_in, 4.0), "the group is 4 in wide: {}", shape(&harness, g).width_in);
    assert_eq!(field(&harness, "W"), "4", "and the inspector reads it back");
    for (id, w) in [a, b].into_iter().zip(widths) {
        assert!(near(shape(&harness, id).width_in, w * 4.0 / was), "child {id} scaled with it: {}", shape(&harness, id).width_in);
    }
}

#[test]
fn a_group_inside_a_hug_row_settles_to_the_same_design_frame_after_frame() {
    let mut d = Design::default();
    let a = add(&mut d, ShapeKind::Square, (1.0, 1.0), (1.0, 1.0));
    let b = add(&mut d, ShapeKind::Square, (1.5, 1.5), (1.0, 1.0));
    let c = add(&mut d, ShapeKind::Square, (5.0, 1.0), (1.0, 1.0));
    let row = d.contain(&[a, b, c]).unwrap();
    d.shape_mut(row).unwrap().container =
        Some(ContainerRole { padding_in: 0.25, layout: Some(Flow { axis: Axis::Row, gap_in: 0.25, cross: Cross::Start, sizing: Sizing::Hug }) });
    d.settle(None);
    let (mut harness, _) = app(d);
    let g = combine(&mut harness, &[a, b], "Weld");
    assert_eq!(shape(&harness, g).parent, Some(row), "the group takes the welded squares' place in the row");
    let once = harness.state().design.clone();
    let depth = harness.state().history.depth();
    harness.run_steps(5);
    assert_eq!(harness.state().design, once, "frames settle to the same design");
    assert_eq!(harness.state().history.depth(), depth, "and record nothing");
    let (row, g) = (shape(&harness, row), shape(&harness, g));
    assert!(near(row.width_in, 0.25 + g.width_in + 0.25 + 1.0 + 0.25), "the row hugs the group: widths {} and {}", row.width_in, g.width_in);
}

#[test]
fn cmd_dragging_the_front_child_of_a_subtract_moves_the_hole_live_and_undoes_in_one_step() {
    let mut d = Design::default();
    let back = add(&mut d, ShapeKind::Square, (2.0, 2.0), (4.0, 4.0));
    let front = add(&mut d, ShapeKind::Square, (3.0, 3.0), (2.0, 2.0));
    let (mut harness, to_screen) = app(d);
    combine(&mut harness, &[back, front], "Subtract");
    harness.state_mut().selection.clear();
    harness.run();
    let probe = (3.5, 4.0);
    let in_hole = |h: &mut App<'_>| pixel(h, to_screen(probe)) == bare(h, to_screen(probe));
    assert!(in_hole(&mut harness), "the probe starts in the hole");
    let depth = harness.state().history.depth();

    let from = to_screen((4.0, 4.0));
    let path: Vec<Pos2> = (0..=10).map(|i| from + vec2(i as f32 * 0.15, 0.0) * (to_screen((1.0, 0.0)).x - to_screen((0.0, 0.0)).x)).collect();
    harness.input_mut().modifiers = egui::Modifiers::COMMAND;
    move_to(&mut harness, path[0]);
    press_at(&mut harness, path[0], true);
    for &p in &path[1..] {
        move_to(&mut harness, p);
    }
    assert_eq!(harness.state().selection, [front], "Cmd picks the child under the pointer, inside the hole");
    assert!(shape(&harness, front).x_in > 4.0, "the child moves: {}", shape(&harness, front).x_in);
    assert_eq!(shape(&harness, back).x_in, 2.0, "and only the child");
    assert!(!in_hole(&mut harness), "mid-drag, the hole has moved off the probe");
    press_at(&mut harness, *path.last().unwrap(), false);
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    harness.run();
    assert_eq!(harness.state().history.depth(), depth + 1, "the drag is one step");

    undo(&mut harness);
    assert_eq!((shape(&harness, front).x_in, shape(&harness, front).y_in), (3.0, 3.0), "one Undo puts the child back");
    assert!(in_hole(&mut harness), "and the hole with it");
}

#[test]
fn a_plain_click_on_a_child_selects_the_group() {
    let (d, a, b) = weld_of_two();
    let (mut harness, to_screen) = app(d);
    let g = combine(&mut harness, &[a, b], "Weld");
    harness.state_mut().selection.clear();
    harness.run();
    super::test_harness::click_at(&mut harness, to_screen((4.5, 3.5)));
    assert_eq!(harness.state().selection, [g]);
}

#[test]
fn dragging_a_layers_row_into_a_weld_joins_the_weld() {
    let mut d = Design::default();
    let circle = add(&mut d, ShapeKind::Circle, (2.0, 2.0), (2.0, 2.0));
    let square = add(&mut d, ShapeKind::Square, (3.0, 2.0), (2.0, 2.0));
    let triangle = add(&mut d, ShapeKind::Triangle, (4.5, 2.0), (2.0, 2.0));
    let (mut harness, _) = app(d);
    let g = combine(&mut harness, &[circle, square], "Weld");
    harness.state_mut().selection.clear();
    harness.run();
    let width = |h: &App<'_>| h.state().design.geometry(&shape(h, g)).bounds().width();
    assert!(near(width(&harness), 3.0));

    let row = |h: &App<'_>, label: &str| {
        let r = h.get_by_label(label).bounding_box().unwrap();
        pos2(r.x0 as f32 + 60.0, r.y0 as f32 + 14.0)
    };
    let (from, onto) = (row(&harness, "Layer Triangle"), row(&harness, "Layer Weld"));
    move_to(&mut harness, from);
    press_at(&mut harness, from, true);
    move_to(&mut harness, from + vec2(0.0, 8.0));
    move_to(&mut harness, onto);
    press_at(&mut harness, onto, false);

    assert_eq!(shape(&harness, triangle).parent, Some(g), "the triangle is the weld's child");
    let pieces = harness.state().design.geometry(&shape(&harness, g)).pieces().len();
    assert_eq!(pieces, 1, "one welded outline");
    assert!(near(width(&harness), 4.5), "the welded outline grows to take it in: {} {:?}", width(&harness), harness.state().design.shapes);
}

#[test]
fn the_op_dropdown_changes_the_group_and_greys_slice_for_three_children() {
    let mut d = Design::default();
    let ids: Vec<u64> = (0..3).map(|i| add(&mut d, ShapeKind::Square, (2.0 + i as f32, 2.0), (2.0, 2.0))).collect();
    let (mut harness, _) = app(d);
    let g = combine(&mut harness, &ids, "Weld");
    harness.get_by_label("Operation Weld").click();
    harness.run();
    assert!(harness.get_by_label("Slice").is_disabled(), "Slice takes exactly two");
    harness.get_by_label("Slice").hover();
    for _ in 0..60 {
        harness.step();
    }
    harness.run();
    harness.get_by_label(&crate::edit::Blocked::SliceNeedsTwo.reason());
    harness.get_by_label("Subtract").click();
    harness.run();
    assert_eq!(shape(&harness, g).compound(), Some(CompoundOp::Boolean(Combine::Subtract)));
    harness.get_by_label("Operation Subtract");
    assert!(harness.query_by_label("Slice").is_none(), "picking closes the list");
    undo(&mut harness);
    assert_eq!(shape(&harness, g).compound(), Some(CompoundOp::Boolean(Combine::Weld)), "one Undo restores the op");
}

#[test]
fn a_group_expands_from_the_context_menu_and_edits_neither_nodes_nor_text() {
    let (d, a, b) = weld_of_two();
    let (mut harness, to_screen) = app(d);
    let g = combine(&mut harness, &[a, b], "Weld");
    double_click_at(&mut harness, to_screen((3.0, 3.0)));
    assert_eq!(harness.state().selection, [g]);
    assert!(harness.state().view.nodes.is_none() && harness.state().view.text_edit.is_none(), "a group has no nodes or text to edit");

    harness.get_by_label("Effects").click();
    harness.run();
    harness.get_by_label("Warp").click();
    harness.run();
    harness.get_by_label(&crate::edit::Blocked::GroupCannotWarp.reason());
    assert!(harness.query_by_label("Arch").is_none(), "a group offers no warp styles");
    key(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);

    let depth = harness.state().history.depth();
    right_click_at(&mut harness, to_screen((3.0, 3.0)));
    assert!(!harness.get_all_by_label("Expand").last().unwrap().is_disabled(), "Expand is offered on a group");
    pick(&mut harness, "Expand");
    let design = &harness.state().design;
    assert!(design.shapes.iter().all(|s| s.compound().is_none()), "the weld is baked");
    assert_eq!(design.shapes.len(), 1, "into one shape");
    assert_eq!(harness.state().history.depth(), depth + 1, "Expand is one step");
}

#[test]
fn a_subtract_group_lists_its_children_under_it() {
    let mut d = Design::default();
    let back = add(&mut d, ShapeKind::Square, (2.0, 2.0), (4.0, 4.0));
    let front = add(&mut d, ShapeKind::Heart, (3.0, 3.0), (2.0, 2.0));
    add(&mut d, ShapeKind::Circle, (7.0, 2.5), (2.0, 2.0));
    let (mut harness, _) = app(d);
    combine(&mut harness, &[back, front], "Subtract");
    let top = |label: &str| harness.get_by_label(label).bounding_box().unwrap().y0;
    let left = |label: &str| harness.get_by_label(label).bounding_box().unwrap().x0;
    assert!(top("Layer Subtract") < top("Layer Heart") && top("Layer Heart") < top("Layer Square"), "the group, then its children top first");
    assert!(left("Layer Heart") > left("Layer Subtract"), "indented under it");
    harness.get_by_label("Operation Subtract");
    move_to(&mut harness, pos2(700.0, 860.0));
    assert_snapshot(&mut harness, "21-boolean");
}

/// The heavy design of the spec 21 timing test plus a Subtract group: frames of a Cmd-drag of its front
/// child, Undo and Redo of that drag, and opening Make. Run with `cargo test --release -- --ignored group_frames`.
#[test]
#[ignore = "timing; run on the release build"]
fn group_frames_in_a_heavy_design() {
    let mut design = super::tests_spec21::heavy_design();
    let back = add(&mut design, ShapeKind::Square, (3.0, 3.0), (3.0, 3.0));
    let front = add(&mut design, ShapeKind::Heart, (3.75, 3.75), (1.5, 1.5));
    let (mut harness, to_screen) = app(design);
    combine(&mut harness, &[back, front], "Subtract");
    harness.state_mut().selection.clear();
    harness.run();

    let grab = to_screen((4.5, 4.5));
    harness.input_mut().modifiers = egui::Modifiers::COMMAND;
    move_to(&mut harness, grab);
    press_at(&mut harness, grab, true);
    harness.run_steps(10);
    let drag = super::tests_spec21::timed(&mut harness, |h, i| {
        h.input_mut().events.push(egui::Event::PointerMoved(grab + vec2(2.0 * (i + 1) as f32, 1.0 * (i + 1) as f32)));
        h.step();
    });
    press_at(&mut harness, grab + vec2(30.0, 15.0), false);
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    harness.run();
    assert_eq!(harness.state().selection, [front]);
    assert!(shape(&harness, front).x_in > 3.75, "the drag moved the child");

    let undo = super::tests_spec21::timed(&mut harness, |h, i| {
        let modifiers = if i % 2 == 0 { egui::Modifiers::COMMAND } else { egui::Modifiers::COMMAND | egui::Modifiers::SHIFT };
        h.input_mut().modifiers = modifiers;
        h.input_mut().events.push(egui::Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers });
        h.step();
        h.input_mut().modifiers = egui::Modifiers::NONE;
    });
    assert_eq!(shape(&harness, front).x_in, 3.75, "eight Undos and seven Redos leave the drag undone");

    let start = std::time::Instant::now();
    harness.get_by_label("Make").click();
    harness.run_steps(4);
    let make = start.elapsed().as_secs_f64() * 1000.0;
    assert!(harness.state().flow.is_some(), "Make opened");
    eprintln!("heavy design with a group: child drag median {drag:.2} ms, undo and redo median {undo:.2} ms, Make entry {make:.2} ms");
    assert!(drag < 16.0 && undo < 16.0, "drag {drag:.2} ms, undo {undo:.2} ms");
}
