//! Spec 34: pattern fill for Print Then Cut, applied from the colour swatch's Pattern tab, moved with
//! Edit pattern, and printed by Make's Print step.

use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::design::{Operation, ShapeKind};
use crate::make_flow::MakeStep;
use crate::patterns::{Bundled, COLOURWAYS, PatternMode};
use crate::ui::test_harness::{Fixture, any_near, assert_snapshot, bare, click_at, drag_along, key, pixel, snapshot_harness, step_is, two_squares_apart};
use crate::ui::tests_spec28::{UPLOAD, wait_for};

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// A selected 3 in Print Then Cut heart at (1, 1) with nothing to undo, and the map from inches to screen.
fn heart(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> egui::Pos2 + use<> {
    let to_screen = two_squares_apart(fixture);
    let state = fixture.state_mut();
    state.design = crate::design::Design::default();
    let id = state.design.add_shape(ShapeKind::Heart);
    let s = state.design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in, s.fill) = (1.0, 1.0, 3.0, 3.0, Color32::from_rgb(90, 90, 90));
    s.set_operation(Operation::PrintThenCut);
    state.selection = vec![id];
    state.history = crate::design::History::default();
    fixture.run();
    to_screen
}

fn apply_stripes(fixture: &mut Fixture) {
    fixture.get_by_role_and_label(Role::Button, "Colour").click();
    fixture.run();
    fixture.get_by_label("Pattern").click();
    fixture.run();
    fixture.get_by_label("Stripes, Berry").click();
    fixture.run();
}

fn berry() -> &'static crate::patterns::Colourway {
    &COLOURWAYS[0]
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

#[test]
fn a_bundled_pattern_fills_a_print_then_cut_heart_shows_in_its_swatch_and_undoes_once() {
    let mut fixture = snapshot_harness(SIZE);
    let to_screen = heart(&mut fixture);
    apply_stripes(&mut fixture);

    let stripes = crate::assets::id_of(&crate::patterns::generate(Bundled::Stripes, berry()));
    let fill = fixture.state().design.shapes[0].pattern.clone().expect("the heart has a pattern");
    assert_eq!((fill.asset.clone(), fill.mode), (stripes.clone(), PatternMode::Tile));
    assert!(crate::assets::dir(&fixture.projects).join(format!("{stripes}.png")).exists(), "the pattern's image is in the asset store");
    assert_eq!(fixture.state().history.depth(), 1, "one undo step");

    click_at(&mut fixture, egui::pos2(SIZE.x - 120.0, 60.0));
    assert!(fixture.query_by_label("Stripes, Berry").is_none(), "a click outside closes the picker");
    let swatch = fixture.get_by_role_and_label(Role::Button, "Colour").value();
    assert_eq!(swatch.as_deref(), Some("Pattern"), "the swatch names the pattern");
    let r = fixture.get_by_role_and_label(Role::Button, "Colour").bounding_box().unwrap();
    let chip = egui::Rect::from_min_size(egui::pos2(r.x0 as f32 + 6.0, r.y0 as f32 + 6.0), egui::Vec2::splat(20.0));
    let frame = fixture.render().expect("wgpu render");
    assert!(any_near(&frame, chip, rgb(berry().ink), 12), "and shows its stripes");

    let inside = |p: (f32, f32)| to_screen(p);
    let area = egui::Rect::from_min_max(inside((1.8, 2.0)), inside((3.2, 3.2)));
    let frame = assert_snapshot(&mut fixture, "34-pattern");
    assert!(any_near(&frame, area, rgb(berry().ink), 12) && any_near(&frame, area, rgb(berry().ground), 12), "the canvas tiles the stripes inside the heart");
    assert_eq!(pixel(&mut fixture, to_screen((1.15, 1.15))), bare(&mut fixture, to_screen((1.15, 1.15))), "nothing outside the heart");

    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert!(fixture.state().design.shapes[0].pattern.is_none(), "one Undo removes it");
}

#[test]
fn make_prints_the_pattern_on_the_print_sheet() {
    let mut fixture = snapshot_harness(SIZE);
    _ = heart(&mut fixture);
    apply_stripes(&mut fixture);
    let out = tempfile::tempdir().unwrap();
    fixture.state_mut().export_dir = Some(out.path().to_owned());

    fixture.get_by_label("Make").click();
    fixture.run_steps(4);
    fixture.get_by_label("Continue").click();
    fixture.run_steps(2);
    assert!(step_is(fixture.state(), MakeStep::Print), "the page opens on Print");
    let flow = fixture.state().flow.as_ref().unwrap();
    let sheet = crate::print_sheet::sheet(&flow.design, flow.mat().unwrap(), &flow.setup, flow.print.bleed).unwrap();
    let art = sheet.layers.last().unwrap();
    assert!(art.picture.as_ref().is_some_and(|p| p.tile.is_some()), "the print sheet tiles the pattern: {:?}", art.picture);

    fixture.get_by_label("Save print sheet (PDF)").click();
    fixture.run_steps(2);
    let [file] = &std::fs::read_dir(out.path()).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>()[..] else { panic!("one print sheet") };
    let pdf = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
    assert_eq!(pdf.matches(" Do\n").count(), 2, "the bleed stamp, then the pattern");
}

#[test]
fn edit_pattern_drags_the_pattern_not_the_shape_in_one_undo_step() {
    let mut fixture = snapshot_harness(SIZE);
    let to_screen = heart(&mut fixture);
    apply_stripes(&mut fixture);
    fixture.get_by_label("Edit pattern").click();
    fixture.run();
    let before = fixture.state().design.shapes[0].clone();
    let depth = fixture.state().history.depth();

    drag_along(&mut fixture, &[to_screen((2.5, 2.5)), to_screen((2.75, 2.6)), to_screen((3.0, 2.75))]);
    let after = fixture.state().design.shapes[0].clone();
    assert_eq!((after.x_in, after.y_in), (before.x_in, before.y_in), "the heart stays put");
    let (dx, dy) = after.pattern.as_ref().unwrap().offset_in;
    assert!((dx - 0.5).abs() < 0.02 && (dy - 0.25).abs() < 0.02, "the pattern moved with the pointer: {dx}, {dy}");
    assert_eq!(fixture.state().history.depth(), depth + 1, "the drag is one undo step");
    key(&mut fixture, Key::Z, Modifiers::COMMAND);
    assert_eq!(fixture.state().design.shapes[0].pattern, before.pattern);
}

/// The `cmd` lines logged while `act` runs on this thread.
fn cmd_lines(act: impl FnOnce()) -> Vec<String> {
    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sink = Sink::default();
    let writer = sink.clone();
    let subscriber = tracing_subscriber::fmt().with_env_filter("cmd=info").with_writer(move || writer.clone()).with_ansi(false).finish();
    // With a single scoped dispatcher alive, tracing-core caches a callsite's interest from whichever
    // thread first reaches it, so another test could switch `edit::run`'s line off for this one. A
    // second live dispatcher makes every registration consult them all.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    tracing::subscriber::with_default(subscriber, act);
    let text = String::from_utf8_lossy(&sink.0.lock().unwrap()).into_owned();
    text.lines().map(str::to_owned).collect()
}

#[test]
fn an_edit_pattern_drag_runs_as_a_pattern_edit() {
    let mut fixture = snapshot_harness(SIZE);
    let to_screen = heart(&mut fixture);
    apply_stripes(&mut fixture);
    fixture.get_by_label("Edit pattern").click();
    fixture.run();
    let lines = cmd_lines(|| drag_along(&mut fixture, &[to_screen((2.5, 2.5)), to_screen((3.0, 2.75))]));
    let moved = fixture.state().design.shapes[0].pattern.clone().unwrap();
    assert!(moved.offset_in.0 > 0.4, "the drag moved the pattern: {:?}", moved.offset_in);
    let last = lines.iter().rev().find(|l| l.contains("action=Pattern(Some(")).unwrap_or_else(|| panic!("no Pattern edit logged: {lines:#?}"));
    assert!(last.contains(&format!("{:?}", moved.offset_in)), "the last logged edit is the pattern the drag left: {last}");
}

#[test]
fn an_uploaded_image_fills_the_selection_and_a_cut_shape_offers_no_pattern_tab() {
    let mut fixture = snapshot_harness(SIZE);
    _ = heart(&mut fixture);
    fixture.state_mut().upload_file = Some(UPLOAD.into());
    fixture.get_by_role_and_label(Role::Button, "Colour").click();
    fixture.run();
    fixture.get_by_label("Pattern").click();
    fixture.run();
    fixture.get_by_label("Upload image").click();
    fixture.step();
    wait_for(&mut fixture, "the uploaded pattern lands", |app| app.design.shapes[0].pattern.is_some());
    let fill = fixture.state().design.shapes[0].pattern.clone().unwrap();
    assert_eq!(fill.mode, PatternMode::Fill);
    assert!((fill.aspect - 1.5).abs() < 1e-4, "the 240 x 160 upload's aspect");
    assert!(fixture.state().trace.is_none(), "no trace panel for a pattern");

    assert!(fixture.query_by_label("Stripes, Berry").is_some(), "the picker is still open on Pattern");
    fixture.state_mut().design.shapes[0].set_operation(Operation::BasicCut);
    fixture.run();
    assert!(fixture.query_by_label("Stripes, Berry").is_none() && fixture.query_by_label("Pattern").is_none(), "a cut shape takes only a colour");
}
