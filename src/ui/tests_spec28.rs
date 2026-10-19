//! Spec 28: Upload a bitmap, trace it in the trace panel, and Apply its cut layers, single cut or
//! Print Then Cut image; the image itself draws from the asset store.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use std::path::{Path, PathBuf};

use super::Kerf;
use super::test_harness::{Fixture, assert_snapshot, bare, key, pixel, rect_of, snapshot_harness, two_squares_apart};
use crate::assets::{Texture, Textures};
use crate::design::{Contour, Design, History, Operation, Seg, ShapeKind};
use crate::links::LinkKind;
use crate::trace::worker::{Job, Source};

pub(super) const UPLOAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/trace/upload-bands.png");
const DISC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/trace/core-disc.png");
const THREE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/trace/core-three.png");

/// How many answers a test follows before it gives up: an upload takes a handful (import, preview, the
/// preview a placed trace asks for), so this only ends a loop whose condition can never hold.
const MAX_ANSWERS: usize = 100;

/// Steps until `done` holds. The image worker answers on its own thread, so between frames this blocks
/// on its next reply rather than sleeping; a debounce in the panel passes with the frames' own time.
pub(super) fn wait_for(fixture: &mut Fixture, what: &str, done: impl Fn(&Kerf) -> bool) {
    for _ in 0..MAX_ANSWERS {
        if done(fixture.state()) {
            fixture.run_steps(3);
            return;
        }
        fixture.state_mut().images.wait_answered();
        fixture.step();
    }
    panic!("{what} never happened");
}

fn upload(fixture: &mut Fixture, file: &Path) {
    fixture.state_mut().upload_file = Some(file.to_owned());
    fixture.get_by_label("Upload").click();
    fixture.step();
}

/// A click that steps frames rather than running until idle: the panel repaints while the worker
/// traces, so `Harness::run` would never settle.
fn tap(fixture: &mut Fixture, pos: egui::Pos2) {
    fixture.input_mut().events.push(egui::Event::PointerMoved(pos));
    fixture.step();
    for pressed in [true, false] {
        let button = egui::PointerButton::Primary;
        fixture.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
        fixture.step();
    }
}

/// The panel's preview shows its latest edits.
fn previewed(app: &Kerf) -> bool {
    app.trace.as_ref().is_some_and(|p| p.settled()) && !app.images.busy()
}

/// Uploads `file` and waits for the trace panel's first preview.
pub(super) fn open_panel(fixture: &mut Fixture, file: &Path) {
    upload(fixture, file);
    wait_for(fixture, "the trace panel's first preview", previewed);
}

pub(super) fn apply(fixture: &mut Fixture) {
    fixture.get_by_label("Apply").click();
    fixture.step();
    wait_for(fixture, "the trace is placed", |app| app.trace.is_none() && !app.design.shapes.is_empty());
}

/// An empty canvas with history cleared, and the map from inches to screen points.
pub(super) fn empty_canvas(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> egui::Pos2 + use<> {
    let to_screen = two_squares_apart(fixture);
    fixture.state_mut().design = Design::default();
    fixture.state_mut().history = History::default();
    fixture.run();
    to_screen
}

#[test]
fn upload_opens_the_trace_panel_and_discarding_adds_nothing() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    _ = empty_canvas(&mut fixture);
    open_panel(&mut fixture, Path::new(UPLOAD));
    assert!(fixture.state().design.shapes.is_empty(), "nothing lands before Apply");
    fixture.get_by_label("Image to trace");
    assert!(fixture.state().trace.as_ref().unwrap().cut_lines() > 0, "the preview proposes a cut line");
    let assets = crate::assets::dir(&fixture.projects);
    assert_eq!(std::fs::read_dir(&assets).unwrap().count(), 1, "the upload is in the asset store");

    fixture.get_by_label("Discard").click();
    fixture.run();
    assert!(fixture.state().trace.is_none(), "the panel closes");
    assert!(fixture.state().design.shapes.is_empty() && !fixture.state().history.can_undo(), "nothing was added");
}

#[test]
fn single_cut_after_removing_the_background_is_one_cut_shape_and_one_undo_removes_it() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    _ = empty_canvas(&mut fixture);
    open_panel(&mut fixture, Path::new(DISC));
    let image = rect_of(&fixture, "Image to trace");
    tap(&mut fixture, image.min + egui::vec2(3.0, 3.0));
    assert_eq!(fixture.state().trace.as_ref().unwrap().background_steps(), 1, "the click removes the background colour");
    wait_for(&mut fixture, "the preview after the removal", previewed);
    let corner = image.min + egui::vec2(2.0, 2.0);
    let shown = pixel(&mut fixture, corner);
    assert!(shown != [255, 255, 255], "the removed corner shows the checkerboard, not white: {shown:?}");
    assert_snapshot(&mut fixture, "28-background");

    fixture.get_by_label("Single cut").click();
    fixture.step();
    apply(&mut fixture);
    let app = fixture.state();
    let [shape] = &app.design.shapes[..] else { panic!("one shape, not {}", app.design.shapes.len()) };
    assert!(matches!(shape.kind, ShapeKind::Imported { .. }), "{:?}", shape.kind);
    assert_eq!(shape.operation, Operation::BasicCut);
    assert_eq!(app.selection, [shape.id], "the result is selected");
    let disc_in = 2.0 * crate::trace::fixtures::DISC_RADIUS / 100.0;
    assert!((shape.width_in - disc_in).abs() < 0.03 && (shape.height_in - disc_in).abs() < 0.03, "the disc at 100 px per inch: {} x {}", shape.width_in, shape.height_in);

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(fixture.state().design.shapes.is_empty(), "one Undo removes the trace");
    assert!(!fixture.state().history.can_undo(), "Apply was one undo step");
}

#[test]
fn posterise_three_as_cut_layers_is_a_group_of_three_coloured_shapes() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    _ = empty_canvas(&mut fixture);
    open_panel(&mut fixture, Path::new(THREE));
    fixture.get_by_label("Posterise").click();
    fixture.step();
    fixture.get_by_label("Fewer colours").click();
    fixture.step();
    wait_for(&mut fixture, "a three colour palette", |app| previewed(app) && app.trace.as_ref().unwrap().swatch_count() == Some(3));
    assert_eq!(fixture.get_by_label("Cut layers").toggled(), Some(egui::accesskit::Toggled::True), "Posterise picks Cut layers");
    assert_snapshot(&mut fixture, "28-trace-panel");
    apply(&mut fixture);

    let app = fixture.state();
    assert_eq!(app.design.shapes.len(), 3);
    let links: Vec<_> = app.design.shapes.iter().map(|s| s.link).collect();
    assert!(links.iter().all(|l| l.is_some_and(|l| l.kind == LinkKind::Group) && *l == links[0]), "one Group: {links:?}");
    let mut fills: Vec<_> = app.design.shapes.iter().map(|s| s.fill).collect();
    fills.dedup();
    assert_eq!(fills.len(), 3, "three colours: {fills:?}");
    assert!(app.design.shapes.iter().all(|s| s.operation == Operation::BasicCut && matches!(s.kind, ShapeKind::Imported { .. })));
    fixture.get_by_label("Layer Group");
    assert_eq!(fixture.get_all_by_label("Layer Imported").count(), 3, "Layers lists the three layers");
}

#[test]
fn print_then_cut_places_one_image_and_make_shows_a_print_then_cut_mat() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    _ = empty_canvas(&mut fixture);
    open_panel(&mut fixture, Path::new(UPLOAD));
    fixture.get_by_label("Print Then Cut").click();
    fixture.step();
    apply(&mut fixture);

    let app = fixture.state();
    assert_eq!(app.design.shapes.len(), 1);
    let shape = &app.design.shapes[0];
    let ShapeKind::Bitmap { px, clip, .. } = &shape.kind else { panic!("a Bitmap, not {:?}", shape.kind) };
    assert_eq!(*px, (240, 160));
    assert_eq!(clip.len(), 1, "cut along the image's outline");
    assert_eq!(shape.operation, Operation::PrintThenCut);
    assert_eq!((shape.width_in, shape.height_in), (2.4, 1.6), "100 pixels per inch");
    assert_eq!(app.selection, [shape.id], "the image is selected");

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(fixture.state().design.shapes.is_empty(), "one Undo removes the image");
    key(&mut fixture, Key::Z, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    assert_eq!(fixture.state().design.shapes.len(), 1);

    fixture.get_by_label("Make").click();
    fixture.run_steps(4);
    let mats = &fixture.state().flow.as_ref().expect("Make opens").mats;
    assert_eq!(mats.len(), 1);
    assert!(mats[0].print_then_cut && mats[0].label() == "Print Then Cut", "{:?}", mats[0]);
}

#[test]
fn cancel_stops_the_full_trace_and_keeps_the_panel() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    _ = empty_canvas(&mut fixture);
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.png");
    crate::trace::fixtures::three_fixture(3000).save(&big).unwrap();
    open_panel(&mut fixture, &big);
    fixture.get_by_label("Apply").click();
    fixture.run_steps(2);
    assert!(fixture.state().images.busy(), "the full trace runs on the worker");
    fixture.get_by_label("Tracing");
    fixture.get_by_label("Cancel").click();
    fixture.step();
    assert!(!fixture.state().images.busy(), "the trace is cancelled");
    fixture.run_steps(10);
    assert!(fixture.state().design.shapes.is_empty(), "a cancelled trace adds nothing");
    assert!(fixture.state().trace.is_some(), "the panel stays open");
    assert!(!fixture.get_by_label("Apply").is_disabled(), "Apply is ready again");
}

#[test]
fn the_image_shows_clipped_to_its_cut_line() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    let to_screen = empty_canvas(&mut fixture);
    open_panel(&mut fixture, Path::new(UPLOAD));
    fixture.get_by_label("Print Then Cut").click();
    fixture.step();
    apply(&mut fixture);
    let app = fixture.state_mut();
    let shape = &mut app.design.shapes[0];
    (shape.x_in, shape.y_in) = (1.0, 1.0);
    let ShapeKind::Bitmap { clip, .. } = &mut shape.kind else { unreachable!() };
    let octagon = [(70.0, 0.0), (170.0, 0.0), (240.0, 50.0), (240.0, 110.0), (170.0, 160.0), (70.0, 160.0), (0.0, 110.0), (0.0, 50.0)];
    *clip = vec![Contour { start: octagon[0], segs: octagon[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true }];
    app.selection.clear();
    fixture.run();

    assert_snapshot(&mut fixture, "28-ptc");
    let dark = pixel(&mut fixture, to_screen((2.2, 1.8)));
    assert!(dark.iter().all(|&c| c < 70), "the image's dark circle shows at its centre: {dark:?}");
    let band = pixel(&mut fixture, to_screen((1.15, 1.5)));
    assert!(band[1] > band[0] && band[1] > band[2], "the green band shows inside the clip: {band:?}");
    let corner = to_screen((1.05, 1.05));
    assert_eq!(pixel(&mut fixture, corner), bare(&mut fixture, corner), "the clipped-off corner shows the canvas");
}

#[test]
fn a_too_large_image_names_its_size_in_the_error_banner() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    let dir = tempfile::tempdir().unwrap();
    let huge: PathBuf = dir.path().join("huge.png");
    std::fs::write(&huge, crate::assets::tests::lying_png(7746, 7746)).unwrap();
    upload(&mut fixture, &huge);
    wait_for(&mut fixture, "the error banner", |app| app.error.is_some());
    let error = fixture.state().error.clone().unwrap();
    assert!(error.contains("7746 x 7746") && error.contains("megapixels"), "{error}");
    assert!(fixture.state().design.shapes.is_empty());
}

#[test]
fn an_image_whose_file_is_gone_draws_a_placeholder() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    let to_screen = empty_canvas(&mut fixture);
    let gone = crate::assets::id_of(&image::RgbaImage::new(2, 2));
    let id = fixture.state_mut().design.add_shape(ShapeKind::Bitmap { asset: gone.clone(), px: (300, 200), clip: Vec::new() });
    let shape = fixture.state_mut().design.shape_mut(id).unwrap();
    (shape.x_in, shape.y_in) = (1.0, 1.0);
    fixture.run();
    let textures = Textures::of(&fixture.ctx).expect("installed by Kerf::new");
    let texture = textures.wait_loaded(&gone, crate::assets::Wrap::Clamp);
    assert!(matches!(texture, Texture::Missing), "a missing file reads as Missing, not Loading forever");
    fixture.run();
    let grey = pixel(&mut fixture, to_screen((1.4, 2.6)));
    assert!(grey.iter().all(|&c| (0x90..=0xe0).contains(&c)), "a grey box: {grey:?}");
}

#[test]
fn export_writes_an_images_cut_line_and_says_so() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    let asset = crate::assets::id_of(&image::RgbaImage::new(2, 2));
    fixture.state_mut().design.add_shape(ShapeKind::Bitmap { asset, px: (200, 100), clip: Vec::new() });
    let out = tempfile::tempdir().unwrap();
    fixture.state_mut().export_dir = Some(out.path().to_owned());
    fixture.run();
    key(&mut fixture, Key::E, Modifiers::COMMAND);
    fixture.get_by_label(super::export_dialog::IMAGES_PICTURE_NOTE);
    fixture.get_all_by_label("Export").last().unwrap().click();
    fixture.run();
    let files: Vec<PathBuf> = std::fs::read_dir(out.path()).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let svg = std::fs::read_to_string(&files[0]).unwrap();
    assert!(svg.contains("<path") && !svg.contains("<image"), "its pixels are not in the store, so the cut line only: {svg}");
}

#[test]
fn upload_runs_from_the_command_palette() {
    let mut fixture = snapshot_harness(egui::vec2(1200.0, 800.0));
    _ = empty_canvas(&mut fixture);
    fixture.state_mut().upload_file = Some(PathBuf::from(UPLOAD));
    key(&mut fixture, Key::K, Modifiers::COMMAND);
    fixture.get_by_label(super::palette::SEARCH_LABEL);
    fixture.input_mut().events.push(egui::Event::Text("Upload".into()));
    fixture.run();
    assert_eq!(fixture.get_all_by_label("Upload").count(), 2, "the palette lists Upload beside the rail's");
    let enter = egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    fixture.input_mut().events.push(enter);
    fixture.step();
    wait_for(&mut fixture, "the trace panel opens from the palette", previewed);
    assert!(!fixture.state().palette.is_open(), "the palette closes");
}

#[test]
fn launching_deletes_only_the_images_no_project_uses() {
    let root = tempfile::tempdir().unwrap();
    let projects = root.path().join("projects");
    let assets = crate::assets::dir(&projects);
    let kept = crate::assets::store(&assets, &image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))).unwrap();
    let unused = crate::assets::store(&assets, &image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))).unwrap();
    let mut design = Design::default();
    let id = design.add_shape(ShapeKind::Flattened { parts: Vec::new(), view_box: (1.0, 1.0) });
    let ShapeKind::Flattened { parts, .. } = &mut design.shape_mut(id).unwrap().kind else { unreachable!() };
    parts.push(crate::design::Shape::new(1, ShapeKind::Bitmap { asset: kept.clone(), px: (2, 2), clip: Vec::new() }));
    crate::project::save(&projects, &crate::project::Project::new(design)).unwrap();
    let file = |id: &crate::assets::AssetId| assets.join(format!("{id}.png"));

    let mut fixture = super::test_harness::launch(root, egui::vec2(800.0, 600.0));
    // The worker collects before it takes any job, so its answer to this one means the sweep is done.
    fixture.state_mut().images.submit(Job::Import(Source::Bytes(Vec::new())));
    fixture.state_mut().images.wait_answered();
    assert!(!file(&unused).exists(), "an image nothing references is collected at launch");
    assert!(file(&kept).exists(), "an image inside a flattened shape is kept");
}
