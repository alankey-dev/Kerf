//! Spec 53, deferred deep-fidelity half: groups and layer names survive import as Kerf containers,
//! and Outline strokes expands imported strokes to filled rings. Driven through the real Upload
//! entry point with fixture SVGs.

use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

use super::import_notice;
use super::test_harness::{Fixture, assert_snapshot, key, snapshot_harness};
use crate::design::{Operation, ShapeKind};

const LAYERED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/svg/layered.svg");
const OUTLINE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/svg/outline.svg");

fn upload(fixture: &mut Fixture, file: &str) {
    fixture.state_mut().upload_file = Some(PathBuf::from(file));
    fixture.get_by_label("Upload").click();
    fixture.step();
}

/// The shape named `name`, by its layer name.
fn named(fixture: &Fixture, name: &str) -> crate::design::Shape {
    let design = &fixture.state().design;
    design.shapes.iter().find(|s| design.layer_name(s) == name).unwrap_or_else(|| panic!("no shape named {name}")).clone()
}

#[test]
fn layered_svg_imports_as_named_nested_containers() {
    let mut fixture = snapshot_harness(vec2(1000.0, 700.0));
    upload(&mut fixture, LAYERED);
    fixture.run();

    // Two layer containers, the nested tree container and four leaf paths: seven shapes in all.
    assert_eq!(fixture.state().design.shapes.len(), 7);

    let background = named(&fixture, "Background");
    let foreground = named(&fixture, "Foreground");
    let tree = named(&fixture, "tree");
    assert!(background.container.is_some() && background.parent.is_none(), "Background is a top-level container");
    assert!(foreground.container.is_some() && foreground.parent.is_none(), "Foreground is a top-level container");
    assert_eq!(tree.parent, Some(foreground.id), "the tree nests inside Foreground");
    assert!(tree.container.is_some(), "tree is a container");
    assert_eq!(named(&fixture, "sky").parent, Some(background.id));
    assert_eq!(named(&fixture, "hill").parent, Some(background.id));
    assert_eq!(named(&fixture, "trunk").parent, Some(tree.id));
    assert_eq!(named(&fixture, "leaves").parent, Some(tree.id));

    // Layers shows the file's layer names as rows.
    fixture.get_by_label("Layer Background");
    fixture.get_by_label("Layer Foreground");
    fixture.get_by_label("Layer tree");
    assert_snapshot(&mut fixture, "53-layered-import");
}

#[test]
fn outline_strokes_rings_imported_strokes_in_one_undo_step() {
    let mut fixture = snapshot_harness(vec2(1000.0, 700.0));
    upload(&mut fixture, OUTLINE);
    fixture.run();

    // The red-stroked rectangle came in as one Pen path.
    assert_eq!(fixture.state().design.shapes.len(), 1);
    let before = fixture.state().design.shapes[0].clone();
    assert_eq!(before.operation, Operation::Pen);
    let ShapeKind::Imported { contours, .. } = &before.kind else { panic!("not imported") };
    assert_eq!(contours.len(), 1, "the stroked rectangle is one contour");

    // Outline strokes expands it to a filled ring that cuts.
    fixture.get_by_label(import_notice::OUTLINE_LABEL).click();
    fixture.step();
    let after = fixture.state().design.shapes[0].clone();
    assert_eq!(after.operation, Operation::BasicCut, "a filled ring cuts");
    let ShapeKind::Imported { contours, .. } = &after.kind else { panic!("not imported") };
    assert_eq!(contours.len(), 2, "an outer and an inner loop");

    // Outlined shapes are no longer strokes, so the stroke row retires on the next frame.
    fixture.run();
    assert!(fixture.query_by_label(import_notice::OUTLINE_LABEL).is_none(), "the Outline strokes option is gone");

    // One undo returns the single pen stroke.
    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    fixture.step();
    let undone = fixture.state().design.shapes[0].clone();
    assert_eq!(undone.operation, Operation::Pen);
    let ShapeKind::Imported { contours, .. } = &undone.kind else { panic!("not imported") };
    assert_eq!(contours.len(), 1);
}
