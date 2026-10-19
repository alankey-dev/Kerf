//! Spec 37: an uploaded Print Then Cut image prints its pixels on the print sheet, and a flattened image
//! keeps its picture on the canvas and in its Home thumbnail.

use std::path::Path;

use egui::Color32;
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_snapshot, pixel, snapshot_harness, step_is};
use super::tests_spec28::{UPLOAD, apply, empty_canvas, open_panel};
use crate::design::ShapeKind;
use crate::edit::{Edit, Subject};
use crate::make_flow::MakeStep;
use crate::project::{self, Project};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// Uploads the banded fixture as a Print Then Cut image at (1, 1) and returns the canvas's map from
/// inches to screen points.
fn upload_print_then_cut(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> egui::Pos2 + use<> {
    let to_screen = empty_canvas(fixture);
    open_panel(fixture, Path::new(UPLOAD));
    fixture.get_by_label("Print Then Cut").click();
    fixture.step();
    apply(fixture);
    let image = &mut fixture.state_mut().design.shapes[0];
    (image.x_in, image.y_in) = (1.0, 1.0);
    fixture.state_mut().selection.clear();
    fixture.run();
    to_screen
}

fn dark(rgb: [u8; 3]) -> bool {
    rgb.iter().all(|&c| c < 70)
}

#[test]
fn an_uploaded_print_then_cut_image_prints_its_pixels_on_the_print_sheet() {
    let mut fixture = snapshot_harness(SIZE);
    _ = upload_print_then_cut(&mut fixture);
    let out = tempfile::tempdir().unwrap();
    fixture.state_mut().export_dir = Some(out.path().to_owned());

    fixture.get_by_label("Make").click();
    fixture.run_steps(4);
    fixture.get_by_label("Continue").click();
    fixture.run_steps(2);
    assert!(step_is(fixture.state(), MakeStep::Print), "the page opens on Print");
    fixture.get_by_label("Save print sheet (PDF)").click();
    fixture.run_steps(2);

    let [file] = &std::fs::read_dir(out.path()).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>()[..] else { panic!("one print sheet") };
    let pdf = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
    assert!(fixture.state().flow.as_ref().unwrap().print.bleed, "Add Bleed starts on");
    assert_eq!(pdf.matches(" Do\n").count(), 2, "the bleed stamp, then the picture");
    assert_eq!(pdf.matches("/Subtype /Image").count(), 4, "each with its alpha mask");
    assert_eq!(pdf.matches("/SMask").count(), 2);
    assert!(pdf.contains("/Width 240"), "the upload's own 240 pixels, under 300 dpi at 2.4 in");
}

#[test]
fn a_flattened_image_keeps_its_picture_on_the_canvas_and_on_home() {
    let mut fixture = snapshot_harness(SIZE);
    let to_screen = upload_print_then_cut(&mut fixture);
    let state = &mut fixture.state_mut().doc;
    let heart = state.design.add_shape(ShapeKind::Heart);
    let s = state.design.shape_mut(heart).unwrap();
    (s.x_in, s.y_in, s.fill) = (4.0, 1.0, Color32::RED);
    state.selection = state.design.shapes.iter().map(|s| s.id).collect();
    crate::edit::run(&mut state.design, &mut state.selection, &Edit::Flatten, Subject::Selection).expect("flattens");
    state.selection.clear();
    fixture.run();
    assert!(matches!(fixture.state().design.shapes[..], [ref flat] if matches!(flat.kind, ShapeKind::Flattened { .. })), "one flattened shape");
    assert!(dark(pixel(&mut fixture, to_screen((2.2, 1.8)))), "the image's dark circle shows on the canvas");
    let band = pixel(&mut fixture, to_screen((1.15, 1.5)));
    assert!(band[1] > band[0] && band[1] > band[2], "and its green band: {band:?}");

    let project = Project { name: "Pictures".into(), ..Project::new(fixture.state().design.clone()) };
    project::save(&fixture.projects, &project).unwrap();
    let thumb = image::open(project::thumb_path(&fixture.projects, &project.id)).unwrap().into_rgb8();
    assert!(thumb.pixels().any(|p| dark(p.0)), "the thumbnail draws the picture, not a flat outline");
    assert!(thumb.pixels().any(|p| p.0 == [255, 0, 0]), "and the heart in its own red");

    let listed = project::list(&fixture.projects);
    let app = fixture.state_mut();
    (app.tab, app.home_projects) = (super::chrome::Tab::Home, listed);
    assert_snapshot(&mut fixture, "37-thumbnail");
}
