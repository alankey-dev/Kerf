//! Spec 39: the app starts on Home, and the interactive tutorial.

use egui_kittest::kittest::Queryable;

use super::chrome::Tab;
use super::test_harness::{Fixture, assert_snapshot, click_at, key, launch, pixel, rect_of, snapshot_harness};
use super::tutorial::{Advance, STEPS, Target};
use crate::design::{Design, ShapeKind};
use crate::project::{self, Project};
use crate::store::session::{self, Session};

const SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);

fn saved(dir: &std::path::Path, id: &str, name: &str, kind: ShapeKind) {
    let mut design = Design::default();
    design.add_shape(kind);
    project::save(dir, &Project { id: id.into(), name: name.into(), design, created: 1, modified: 1 }).unwrap();
}

#[test]
fn a_fresh_launch_shows_home() {
    let mut fixture = launch(tempfile::tempdir().unwrap(), SIZE);
    assert!(fixture.state().tab == Tab::Home);
    fixture.get_by_label("Tab Untitled-1");
    assert_snapshot(&mut fixture, "39-home-start");
    fixture.get_by_label("New Project").click();
    fixture.run();
    assert!(fixture.state().tab == Tab::Canvas, "a card goes to the canvas");
}

#[test]
fn a_restored_session_waits_in_the_tab_strip_while_home_shows() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("projects");
    saved(&dir, "card", "Birthday card", ShapeKind::Star);
    saved(&dir, "tag", "Gift tag", ShapeKind::Circle);
    session::write(&dir, &Session { tabs: vec!["card".into(), "tag".into()], active: 1, pid: None }).unwrap();

    let mut fixture = launch(root, SIZE);
    assert!(fixture.state().tab == Tab::Home);
    fixture.get_by_label("Tab Birthday card");
    assert_eq!(fixture.state().title, "Gift tag", "the tab left active is still active");
    fixture.get_by_label("Tab Birthday card").click();
    fixture.run();
    assert!(fixture.state().tab == Tab::Canvas, "a tab goes to the canvas");
    assert_eq!(fixture.state().title, "Birthday card");
}

#[test]
fn opening_a_file_from_the_command_line_goes_straight_to_the_canvas() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = root.path().join("elsewhere");
    saved(&elsewhere, "loose", "Loose", ShapeKind::Heart);
    let mut fixture = launch(root, SIZE);
    fixture.state_mut().open_file(&project::json_path(&elsewhere, "loose")).unwrap();
    fixture.run();
    assert!(fixture.state().tab == Tab::Canvas);
    assert_eq!(fixture.state().design.shapes.len(), 1);
}

fn tour(app: &super::Kerf) -> Option<usize> {
    app.tutorial.as_ref().map(|t| t.step)
}

fn step_titled(title: &str) -> usize {
    STEPS.iter().position(|s| s.title == title).unwrap_or_else(|| panic!("no step {title}"))
}

/// Help > Interactive tutorial, from the canvas.
fn start(fixture: &mut Fixture) {
    fixture.get_by_label("Help").click();
    fixture.run();
    fixture.get_by_label("Interactive tutorial").click();
    settle(fixture);
}

/// Frames enough for queued acts to run and the card to fade in. The Make flow repaints on a timer, so
/// `run` would never return there.
fn settle(fixture: &mut Fixture) {
    fixture.run_steps(4);
}

/// The card's forward button: Next or Finish, or Show me while the step waits on the user.
fn forward(fixture: &mut Fixture) {
    let label = ["Next step", "Finish tutorial", "Show me"].into_iter().find(|l| fixture.query_by_label(l).is_some()).expect("a way forward");
    fixture.get_by_label(label).click();
    settle(fixture);
}

fn walk_to(fixture: &mut Fixture, title: &str) {
    let to = step_titled(title);
    while tour(fixture.state()).is_some_and(|s| s < to) {
        forward(fixture);
    }
    assert_eq!(tour(fixture.state()), Some(to));
}

fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().flat_map(|e| if e.path().is_dir() { files_under(&e.path()) } else { vec![e.path()] }).collect()
}

#[test]
fn the_tutorial_waits_for_the_help_menu_and_escape_ends_it_without_writing_anything() {
    let fixture = launch(tempfile::tempdir().unwrap(), SIZE);
    assert!(tour(fixture.state()).is_none(), "nothing starts on launch");
    let mut fixture = super::test_harness::open_blank_tab(fixture);
    fixture.run_steps(20);
    assert!(tour(fixture.state()).is_none(), "nor later");
    let before = files_under(&fixture.projects);

    start(&mut fixture);
    assert_eq!(tour(fixture.state()), Some(0));
    fixture.get_by_label("Tab Tutorial");
    fixture.get_by_label(&format!("Step 1 of {}", STEPS.len()));
    walk_to(&mut fixture, "Add a shape");
    fixture.get_by_label("Show me").click();
    settle(&mut fixture);
    key(&mut fixture, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(tour(fixture.state()).is_none(), "Escape ends the tour");
    assert!(fixture.query_by_label("Tab Tutorial").is_none(), "and closes its tab");
    assert!(fixture.state().tab == Tab::Canvas && fixture.state().title == "Untitled-1", "back where it began");
    assert!(fixture.state().design.shapes.is_empty());

    fixture.state().saving.saver().expect("a projects folder").flush();
    fixture.run();
    let after: Vec<_> = files_under(&fixture.projects).into_iter().filter(|f| !before.contains(f) && !f.ends_with("session.json")).collect();
    assert_eq!(after, Vec::<std::path::PathBuf>::new(), "no project, autosave or version was written");
    assert_eq!(session::read(&fixture.projects).map(|s| s.tabs.len()), Some(1), "the session never names the Tutorial tab");
}

#[test]
fn on_the_add_a_shape_step_the_cut_out_takes_clicks_and_the_dim_does_not() {
    let mut fixture = snapshot_harness(SIZE);
    start(&mut fixture);
    walk_to(&mut fixture, "Add a shape");
    let shapes = fixture.state().design.shapes.len();

    let pen = rect_of(&fixture, "Pen").center();
    click_at(&mut fixture, pen);
    settle(&mut fixture);
    assert!(fixture.state().tool == super::canvas::Tool::Select, "a click on the dim reaches nothing under it");
    assert_eq!(tour(fixture.state()), Some(step_titled("Add a shape")));

    let square = rect_of(&fixture, "Square").center();
    click_at(&mut fixture, square);
    settle(&mut fixture);
    assert_eq!(fixture.state().design.shapes.len(), shapes + 1, "a click in the cut-out adds the shape");
    assert_eq!(tour(fixture.state()), Some(step_titled("Add a shape") + 1), "and moves the tour on");
}

#[test]
fn every_step_finds_its_target_and_every_show_me_does_its_step() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    start(&mut fixture);
    for (i, step) in STEPS.iter().enumerate() {
        assert_eq!(tour(fixture.state()), Some(i));
        if !matches!(step.target, Target::Centre) {
            assert!(fixture.state().tutorial.as_ref().is_some_and(|t| t.hole().is_some()), "step {} ({}) has no target on screen", i + 1, step.title);
        }
        if let Advance::When { .. } = step.advance {
            fixture.get_by_label("Show me").click();
            settle(&mut fixture);
            assert_eq!(tour(fixture.state()), Some(i + 1), "Show me on {} does the step", step.title);
        } else {
            forward(&mut fixture);
        }
    }
    assert!(tour(fixture.state()).is_none(), "Finish ends the tour");
    assert!(fixture.query_by_label("Tab Tutorial").is_none());
}

#[test]
fn back_returns_to_the_step_before_and_prepares_it_again() {
    let mut fixture = snapshot_harness(SIZE);
    start(&mut fixture);
    walk_to(&mut fixture, STEPS[9].title);
    fixture.state_mut().shapes_open = true;
    settle(&mut fixture);
    fixture.get_by_label("Previous step").click();
    settle(&mut fixture);
    assert_eq!(tour(fixture.state()), Some(8));
    assert!(!fixture.state().shapes_open, "{} closes the Shapes panel again", STEPS[8].title);
}

#[test]
fn the_dim_darkens_everything_but_the_cut_out() {
    let mut fixture = snapshot_harness(SIZE);
    let far = egui::pos2(1100.0, 400.0);
    let lit = pixel(&mut fixture, far);
    start(&mut fixture);
    walk_to(&mut fixture, "Tools");
    let rail = rect_of(&fixture, "Pen");
    let inside = egui::pos2(rail.left() + 3.0, rail.center().y);
    let image = assert_snapshot(&mut fixture, "39-tutorial-rail");
    let dimmed = super::test_harness::rgb(&image, far);
    let a = f32::from(super::tokens::TUTORIAL_DIM.a()) / 255.0;
    for (d, l) in dimmed.iter().zip(lit) {
        assert!((f32::from(*d) - f32::from(l) * (1.0 - a)).abs() <= 3.0, "{dimmed:?} is {lit:?} under the dim");
    }
    assert_eq!(super::test_harness::rgb(&image, inside), [255, 255, 255], "the rail shows through the cut-out");
}

#[test]
fn snapshots_of_the_inspector_and_make_steps() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    start(&mut fixture);
    walk_to(&mut fixture, "Exact values");
    assert_snapshot(&mut fixture, "39-tutorial-inspector");
    walk_to(&mut fixture, "Prepare");
    super::test_harness::assert_flow_snapshot(&mut fixture, "39-tutorial-make");
}

#[test]
fn new_during_the_tour_adds_a_tab_beside_the_tutorial() {
    let mut fixture = snapshot_harness(SIZE);
    start(&mut fixture);
    walk_to(&mut fixture, "Start something new");
    key(&mut fixture, egui::Key::T, egui::Modifiers::COMMAND);
    settle(&mut fixture);
    assert!(tour(fixture.state()).is_some(), "the tour carries on");
    fixture.get_by_label("Tab Tutorial");
    assert_eq!(fixture.state().docs().filter(|d| d.transient).count(), 1, "the Tutorial tab is still the transient one");
}

#[test]
fn show_me_works_on_the_tutorial_tab_even_from_another_tab() {
    let mut fixture = snapshot_harness(SIZE);
    for kind in [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Square, ShapeKind::Circle] {
        fixture.state_mut().design.add_shape(kind);
    }
    fixture.run();
    start(&mut fixture);
    walk_to(&mut fixture, "Cut a window");
    key(&mut fixture, egui::Key::Tab, egui::Modifiers::CTRL);
    assert!(!fixture.state().transient, "the user's tab is in front");
    fixture.get_by_label("Show me").click();
    settle(&mut fixture);
    assert_eq!(tour(fixture.state()), Some(step_titled("Cut a window") + 1));
    let user = fixture.state().docs().find(|d| !d.transient).expect("the user's tab");
    assert!(user.design.shapes.iter().all(|s| !matches!(s.kind, ShapeKind::Compound(_))), "the user's design is untouched");
}

#[test]
fn escape_leaves_node_editing_without_ending_the_tour() {
    let mut fixture = snapshot_harness(SIZE);
    start(&mut fixture);
    walk_to(&mut fixture, "Point tools");
    key(&mut fixture, egui::Key::Escape, egui::Modifiers::NONE);
    settle(&mut fixture);
    assert!(tour(fixture.state()).is_some(), "Escape belonged to node editing");
    key(&mut fixture, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(tour(fixture.state()).is_none(), "the next Escape ends the tour");
}
