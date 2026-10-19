//! Spec 34: the Measure tool, the drag read-out and Option-hover distance lines, driven through the app.

use egui::{Key, Modifiers, OutputCommand, vec2};
use egui_kittest::kittest::Queryable;

use super::super::canvas::Tool;
use super::super::test_harness::{Fixture, assert_snapshot, drag_along, key, move_to, press_at, snapshot_harness, two_squares_apart};
use crate::measure::{DistanceLine, Measurement, Readout, distance_lines};

fn copied(fixture: &Fixture) -> Vec<String> {
    let commands = &fixture.output().platform_output.commands;
    commands.iter().filter_map(|c| if let OutputCommand::CopyText(text) = c { Some(text.clone()) } else { None }).collect()
}

#[test]
fn the_measure_tool_rules_between_corners_copies_and_clears() {
    let mut fixture = snapshot_harness(vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().design.set_snapping(true);
    let rect = &mut fixture.state_mut().design.shapes[0];
    (rect.x_in, rect.y_in, rect.width_in, rect.height_in) = (1.0, 1.0, 3.0, 4.0);
    fixture.state_mut().design.shapes[1].x_in = 6.0;
    fixture.run();

    fixture.get_by_label("Measure").click();
    fixture.run();
    assert!(fixture.state().tool == Tool::Measure, "the rail picks Measure");
    key(&mut fixture, Key::V, Modifiers::NONE);
    key(&mut fixture, Key::M, Modifiers::NONE);
    assert!(fixture.state().tool == Tool::Measure, "M picks Measure");

    drag_along(&mut fixture, &[at((1.1, 1.1)), at((1.5, 1.5)), at((2.0, 2.0))]);
    assert!(fixture.state().view.measure.measurement.is_some());
    drag_along(&mut fixture, &[at((1.04, 4.95)), at((2.0, 3.0)), at((3.96, 1.04))]);
    let measured = fixture.state().view.measure.measurement;
    assert_eq!(measured, Some(Measurement { from: (1.0, 5.0), to: (4.0, 1.0) }), "a new drag replaces the last, snapped to both corners");
    let label = "5 in · 3 in x 4 in · 53.13°";
    fixture.get_by_label(label);
    assert!(fixture.state().design.shapes.iter().all(|s| s.x_in == 1.0 || s.x_in == 6.0), "measuring moves nothing");

    fixture.input_mut().events.push(egui::Event::Key { key: Key::C, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    fixture.step();
    assert_eq!(copied(&fixture), vec![label.to_owned()], "the copy shortcut copies the label");

    assert_snapshot(&mut fixture, "34-measure");
    key(&mut fixture, Key::Escape, Modifiers::NONE);
    assert_eq!(fixture.state().view.measure.measurement, None);
    assert!(fixture.query_by_label(label).is_none(), "Escape clears the label");
}

#[test]
fn shift_keeps_a_measurement_to_45_degrees() {
    let mut fixture = snapshot_harness(vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().tool = Tool::Measure;
    fixture.run();
    fixture.input_mut().modifiers = Modifiers::SHIFT;
    drag_along(&mut fixture, &[at((7.0, 7.0)), at((7.5, 7.5)), at((9.0, 7.2))]);
    fixture.input_mut().modifiers = Modifiers::NONE;
    let m = fixture.state().view.measure.measurement.expect("a measurement");
    assert!((m.to.1 - m.from.1).abs() < 1e-4, "held level: {m:?}");
}

#[test]
fn moving_reads_out_the_change_and_resizing_the_new_size_until_release() {
    let mut fixture = snapshot_harness(vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut fixture);
    fixture.state_mut().selection = vec![1];
    fixture.run();
    let square = fixture.state().design.shapes[0].clone();
    let inside = (square.x_in + 0.5, square.y_in + 0.5);

    move_to(&mut fixture, at(inside));
    press_at(&mut fixture, at(inside), true);
    move_to(&mut fixture, at((inside.0 + 0.5, inside.1)));
    move_to(&mut fixture, at((inside.0 + 1.0, inside.1 + 0.5)));
    let moved = fixture.state().design.shapes[0].clone();
    let readout = Readout::Moved { dx: moved.x_in - square.x_in, dy: moved.y_in - square.y_in }.label(fixture.state().units);
    assert!(moved.x_in > square.x_in && moved.y_in > square.y_in, "the drag moved the square");
    fixture.get_by_label(&readout);
    press_at(&mut fixture, at((inside.0 + 1.0, inside.1 + 0.5)), false);
    assert!(fixture.query_by_label(&readout).is_none(), "the read-out goes on release");

    let square = fixture.state().design.shapes[0].clone();
    let corner = (square.x_in + square.width_in, square.y_in + square.height_in);
    move_to(&mut fixture, at(corner));
    press_at(&mut fixture, at(corner), true);
    move_to(&mut fixture, at(corner) + vec2(4.0, 4.0));
    move_to(&mut fixture, at((corner.0 + 1.0, corner.1 + 1.0)));
    let resized = fixture.state().design.shapes[0].clone();
    let size = fixture.state().units.size(resized.width_in, resized.height_in);
    assert!(resized.width_in > square.width_in, "the drag resized");
    assert_eq!(fixture.get_all_by_label(&size).count(), 2, "the pointer read-out joins the size badge");
    press_at(&mut fixture, at((corner.0 + 1.0, corner.1 + 1.0)), false);
    assert_eq!(fixture.get_all_by_label(&size).count(), 1, "only the badge stays after release");
}

#[test]
fn option_hover_draws_distance_lines_to_the_shape_under_the_pointer() {
    let mut fixture = snapshot_harness(vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut fixture);
    (fixture.state_mut().design.shapes[1].x_in, fixture.state_mut().design.shapes[1].y_in) = (5.0, 2.0);
    fixture.state_mut().selection = vec![1];
    fixture.run();
    let bounds: Vec<_> = fixture.state().design.shapes.iter().map(|s| s.aabb()).collect();
    let over = bounds[1].centre();

    move_to(&mut fixture, at(over));
    assert!(fixture.state().view.measure.distances.is_empty(), "nothing without Option");

    fixture.input_mut().modifiers = Modifiers::ALT;
    move_to(&mut fixture, at((over.0 + 0.1, over.1)));
    let lines = fixture.state().view.measure.distances.clone();
    assert_eq!(lines, distance_lines(bounds[0], bounds[1]));
    let [DistanceLine { length, .. }] = lines[..] else { panic!("one gap between side-by-side squares: {lines:?}") };
    assert!((length - (bounds[1].min_x - bounds[0].max_x)).abs() < 1e-4);
    fixture.get_by_label(&fixture.state().units.len(length));
    assert_snapshot(&mut fixture, "34-distances");

    fixture.input_mut().modifiers = Modifiers::NONE;
    move_to(&mut fixture, at((over.0, over.1 + 0.1)));
    assert!(fixture.state().view.measure.distances.is_empty(), "releasing Option clears them");
}
