//! Spec 22: unsaved work is never discarded, and a failed save says so. Spec 33 replaced the question
//! before leaving with tabs and autosave.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, snapshot_harness};
use crate::design::ShapeKind;
use crate::project;

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

fn edited() -> Fixture {
    let mut harness = snapshot_harness(SIZE);
    harness.state_mut().design.add_shape(ShapeKind::Star);
    harness.run();
    harness
}

fn file_menu(harness: &mut Harness<'_, Kerf>, item: &str) {
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label(item).click();
    harness.run();
}

fn asking(harness: &Harness<'_, Kerf>) -> bool {
    harness.query_by_label("Don't save").is_some()
}

#[test]
fn new_opens_a_tab_and_keeps_the_edited_one() {
    let mut harness = edited();
    file_menu(&mut harness, "New");
    assert!(!asking(&harness), "nothing asks: the edited tab stays open");
    assert_eq!((harness.state().tab_count(), harness.state().active_index()), (2, 1));
    assert_eq!(harness.state().title, "Untitled-2");
    assert!(harness.state().design.shapes.is_empty(), "the new tab is blank");
    let first = harness.state().docs().next().expect("the first tab");
    assert_eq!((first.title.as_str(), first.design.shapes.len()), ("Untitled-1", 1), "the edited tab keeps its star");
    assert!(project::list(&harness.projects).is_empty(), "and nothing reaches Home");
}

#[test]
fn save_then_new_keeps_the_work_on_disk() {
    let mut harness = edited();
    file_menu(&mut harness, "Save");
    file_menu(&mut harness, "New");
    assert!(harness.state().design.shapes.is_empty(), "New opens a blank tab");
    let saved = project::list(&harness.projects);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].project().unwrap().design.shapes.len(), 1, "the star is in the saved project");

    file_menu(&mut harness, "New");
    assert_eq!(harness.state().tab_count(), 2, "New on an untouched blank tab takes its place");
    assert_eq!(harness.state().title, "Untitled-2", "the saved tab is still called Untitled-1");
}

#[test]
fn home_edit_and_make_open_tabs_without_asking() {
    let mut harness = edited();
    let other = project::Project::new(crate::design::Design::default());
    project::save(&harness.projects, &project::Project { name: "Other".into(), ..other }).unwrap();
    file_menu(&mut harness, "Home");
    harness.get_by_label("Edit").click();
    harness.run();
    assert!(!asking(&harness), "Edit from Home asks nothing");
    assert_eq!((harness.state().tab_count(), harness.state().title.as_str()), (2, "Other"));
    assert_eq!(harness.state().docs().next().map(|d| d.design.shapes.len()), Some(1), "the edited tab stays open");

    file_menu(&mut harness, "Home");
    harness.get_by_label("Make").click();
    harness.run_steps(3);
    assert!(!asking(&harness));
    assert_eq!(harness.state().tab_count(), 2, "Make on an open project goes to its tab");
    assert_eq!(harness.state().title, "Other");
    assert!(harness.state().flow.is_some(), "straight into Make");
}

#[test]
fn closing_the_window_asks_nothing_and_keeps_the_work() {
    let mut harness = edited();
    let id = harness.state().autosave.id.clone();
    harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    harness.run();
    assert!(!asking(&harness), "the window closes without a question");
    harness.state().saving.saver().expect("a projects folder").flush();
    let autosave = crate::store::read_autosave(&harness.projects, &id).unwrap().expect("the edit is autosaved on the way out");
    assert_eq!(autosave.design.shapes.len(), 1);
    let session = crate::store::session::read(&harness.projects).expect("a session");
    assert_eq!((session.tabs, session.pid), (vec![id], None), "the session is clean");
}

#[test]
fn a_failed_save_keeps_the_project_and_shows_a_dismissable_error() {
    let mut harness = edited();
    file_menu(&mut harness, "Save");
    let id = harness.state().project.as_ref().unwrap().id.clone();
    let before = std::fs::read_to_string(project::json_path(&harness.projects, &id)).unwrap();

    harness.state_mut().design.add_shape(ShapeKind::Heart);
    let file = project::json_path(&harness.projects, &id);
    project::refuse_writes(&harness.projects, &file, true);
    file_menu(&mut harness, "Save");
    project::refuse_writes(&harness.projects, &file, false);

    assert_eq!(harness.state().project.as_ref().map(|p| p.id.as_str()), Some(id.as_str()), "the project keeps its id");
    assert_eq!(std::fs::read_to_string(project::json_path(&harness.projects, &id)).unwrap(), before, "the old file is intact");
    assert!(harness.state().error.as_deref().is_some_and(|e| e.starts_with("Couldn't save")), "{:?}", harness.state().error);
    assert!(harness.state().has_unsaved_changes(), "the work still counts as unsaved");

    harness.get_by_label("Dismiss error").click();
    harness.run();
    assert!(harness.state().error.is_none());
    file_menu(&mut harness, "Save");
    assert_eq!(project::list(&harness.projects).len(), 1, "the retry saves over the same project, no fork");
    assert!(!harness.state().has_unsaved_changes());
}

#[test]
fn an_unreadable_project_shows_a_card_instead_of_vanishing() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 820.0));
    std::fs::create_dir_all(&harness.projects).unwrap();
    std::fs::write(harness.projects.join("torn.json"), "{\"id\":\"torn\",\"na").unwrap();
    file_menu(&mut harness, "Home");
    assert!(harness.query_by_label_contains("Couldn't open torn.json").is_some(), "the card names the file");
    harness.get_by_label("Delete torn.json").click();
    harness.run();
    harness.get_by_label("Confirm delete").click();
    harness.run();
    assert!(!harness.projects.join("torn.json").exists(), "and it can be deleted");
}
