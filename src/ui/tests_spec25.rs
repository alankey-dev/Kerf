//! Spec 25: File > Export, the top bar's Export and Cmd+E open one dialog, and its Export writes the
//! planned files; Prepare for Design Space writes each mat's SVG and the guide.

use egui::{Color32, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_snapshot, key, pick, snapshot_harness};
use crate::design::{Design, Operation, ShapeKind};
use crate::export::{DESIGN_SPACE_GUIDE, Format, Scope};

/// A red square and a blue heart, so the design composes onto two mats.
fn two_mats(fixture: &mut Fixture) -> tempfile::TempDir {
    let mut d = Design::default();
    for (kind, fill, x) in [(ShapeKind::Square, Color32::RED, 1.0), (ShapeKind::Heart, Color32::BLUE, 4.0)] {
        let id = d.add_shape(kind);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.fill, s.operation) = (x, 1.0, fill, Operation::BasicCut);
    }
    let out = tempfile::tempdir().unwrap();
    let app = fixture.state_mut();
    app.design = d;
    app.export_dir = Some(out.path().to_owned());
    fixture.run();
    out
}

fn written(dir: &tempfile::TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn file_export_as_dxf_per_mat_writes_one_dxf_per_mat() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    let out = two_mats(&mut fixture);
    fixture.get_by_label("File").click();
    fixture.run();
    pick(&mut fixture, "Export...");
    assert!(fixture.state().export.is_some(), "File > Export... opens the dialog");
    fixture.get_by_label("DXF").click();
    fixture.run();
    fixture.get_by_label("One per mat").click();
    fixture.run();
    let request = fixture.state().export.as_ref().unwrap().request;
    assert_eq!((request.format, request.scope()), (Format::Dxf, Scope::PerMat));
    assert_snapshot(&mut fixture, "25-export-dialog-dxf");
    pick(&mut fixture, "Export");
    assert!(fixture.state().export.is_none(), "Export closes the dialog");
    assert_eq!(written(&out), ["Untitled-1 - Mat 1 - Red - Basic Cut.dxf", "Untitled-1 - Mat 2 - Blue - Basic Cut.dxf"]);
    let dxf = std::fs::read_to_string(out.path().join("Untitled-1 - Mat 1 - Red - Basic Cut.dxf")).unwrap();
    assert!(dxf.contains("AC1009") && dxf.contains("Basic_Cut-Square"), "{dxf}");
}

#[test]
fn the_top_bar_export_button_opens_the_same_dialog() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    two_mats(&mut fixture);
    fixture.get_by_label("Export").click();
    fixture.run();
    assert!(fixture.state().export.is_some());
    fixture.get_by_label("Export design");
    assert_snapshot(&mut fixture, "25-export-dialog");
    pick(&mut fixture, "Cancel");
    assert!(fixture.state().export.is_none(), "Cancel closes it");
}

#[test]
fn cmd_e_opens_the_dialog() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    two_mats(&mut fixture);
    key(&mut fixture, Key::E, Modifiers::COMMAND);
    assert!(fixture.state().export.is_some());
}

#[test]
fn export_is_blocked_on_an_empty_canvas() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    key(&mut fixture, Key::E, Modifiers::COMMAND);
    assert!(fixture.state().export.is_none(), "nothing would be written");
}

#[test]
fn prepare_for_design_space_writes_each_mat_and_the_guide() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    let out = two_mats(&mut fixture);
    fixture.get_by_label("File").click();
    fixture.run();
    pick(&mut fixture, "Prepare for Design Space");
    assert_eq!(written(&out), [DESIGN_SPACE_GUIDE, "Untitled-1 - Mat 1 - Red - Basic Cut.svg", "Untitled-1 - Mat 2 - Blue - Basic Cut.svg"]);
}
