//! Spec 53: SVG import fidelity. Driving the real Upload entry point with fixture SVGs.

use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

use super::import_notice;
use super::test_harness::{Fixture, assert_snapshot, key, snapshot_harness};
use crate::design::Operation;

const STROKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/svg/stroked.svg");

fn upload(fixture: &mut Fixture, file: &str) {
    fixture.state_mut().upload_file = Some(PathBuf::from(file));
    fixture.get_by_label("Upload").click();
    fixture.step();
    fixture.state_mut().wait_svg_answered();
    fixture.step();
}

fn op_counts(fixture: &Fixture) -> (usize, usize) {
    let shapes = &fixture.state().design.shapes;
    let cut = shapes.iter().filter(|s| s.operation == Operation::BasicCut).count();
    let pen = shapes.iter().filter(|s| s.operation == Operation::Pen).count();
    (cut, pen)
}

#[test]
fn stroked_svg_imports_as_hairlines_and_pen_switches_in_one_undo_step() {
    let mut fixture = snapshot_harness(vec2(1000.0, 700.0));
    upload(&mut fixture, STROKED);
    fixture.run();

    // Three stroke-only hairlines came in, so they default to Cut and the notice offers the choice.
    assert_eq!(fixture.state().design.shapes.len(), 3);
    assert_eq!(op_counts(&fixture), (3, 0));
    fixture.get_by_label(import_notice::HAIRLINE_LABEL);
    assert_snapshot(&mut fixture, "53-import-notice");

    // Choosing Pen switches every imported stroke's operation.
    fixture.get_by_label(&format!("{} Pen", import_notice::HAIRLINE_LABEL)).click();
    fixture.step();
    assert_eq!(op_counts(&fixture), (0, 3));

    // The switch is one undo step: undoing returns them all to Cut.
    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    fixture.step();
    assert_eq!(op_counts(&fixture), (3, 0));
}
