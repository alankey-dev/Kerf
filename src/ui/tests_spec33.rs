//! Spec 33: autosave, version history, crash recovery and tabs, driven through the app.

use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_snapshot, key, launch, snapshot_harness};
use crate::design::{Design, ShapeKind};
use crate::project::{self, Project};
use crate::store::session::{self, Session};
use crate::store::{self, VersionKind};

/// The kind a Square tile now inserts: a live parametric rectangle (spec 49).
fn inserted_square() -> ShapeKind {
    ShapeKind::Parametric(crate::parametric::Params::Rectangle { corners: [0.0; 4], linked: true, scale_with_shape: false })
}

const SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);
const MINUTE_MS: u64 = 60_000;
/// The harness clock: Monday 12 October 2026, 10:42 UTC.
const NOW_MS: u64 = super::autosave::FIXED_MS;

fn flush(fixture: &Fixture) {
    fixture.state().saving.saver().expect("a projects folder").flush();
}

fn design(kinds: &[ShapeKind]) -> Design {
    let mut d = Design::default();
    for k in kinds {
        d.add_shape(k.clone());
    }
    d
}

fn kinds(d: &Design) -> Vec<ShapeKind> {
    d.shapes.iter().map(|s| s.kind.clone()).collect()
}

fn click(fixture: &mut Fixture, label: &str) {
    fixture.get_by_label(label).click();
    fixture.run();
}

fn file_menu(fixture: &mut Fixture, item: &str) {
    click(fixture, "File");
    click(fixture, item);
}

#[test]
fn an_edit_autosaves_after_two_quiet_seconds_and_the_title_says_so() {
    let mut fixture = snapshot_harness(SIZE);
    fixture.get_by_label("Saved");
    fixture.get_by_label("Shapes").click();
    fixture.step();
    click(&mut fixture, "Square");
    fixture.get_by_label("Edited");
    let id = fixture.state().autosave.id.clone();
    assert!(!store::autosave_path(&fixture.projects, &id).exists(), "nothing is written straight away");

    for _ in 0..12 {
        fixture.step();
    }
    flush(&fixture);
    fixture.step();

    let autosave = store::read_autosave(&fixture.projects, &id).unwrap().expect("autosave/<id>.json exists");
    assert_eq!(kinds(&autosave.design), [inserted_square()]);
    fixture.get_by_label("Autosaved 10:42");
    assert!(project::list(&fixture.projects).is_empty(), "a never-saved project is not on Home");

    file_menu(&mut fixture, "Save");
    flush(&fixture);
    fixture.get_by_label("Saved");
    assert!(!store::autosave_path(&fixture.projects, &id).exists(), "Save clears the autosave");
    assert_eq!(store::read_index(&fixture.projects, &id).unwrap().versions[0].kind, VersionKind::Manual);
}

#[test]
fn the_session_names_the_open_project_and_a_clean_quit_clears_the_pid() {
    let fixture = snapshot_harness(SIZE);
    flush(&fixture);
    let id = fixture.state().autosave.id.clone();
    assert_eq!(session::read(&fixture.projects), Some(Session { tabs: vec![id.clone()], active: 0, pid: Some(std::process::id()) }));
    fixture.state().clean_exit();
    flush(&fixture);
    assert_eq!(session::read(&fixture.projects), Some(Session { tabs: vec![id], active: 0, pid: None }));
}

/// A session that crashed while "Birthday card" (saved at 09:00) and "Gift tag" (never saved) had
/// unsaved changes, autosaved at 10:30 and 10:35.
fn crashed() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("projects");
    let at = |ms: u64| ms / 1000;
    let card = Project {
        id: "card".into(),
        name: "Birthday card".into(),
        design: design(&[ShapeKind::Star]),
        created: at(NOW_MS - 102 * MINUTE_MS),
        modified: at(NOW_MS - 102 * MINUTE_MS),
    };
    project::save(&dir, &card).unwrap();
    store::write_autosave(&dir, &Project { design: design(&[ShapeKind::Star, ShapeKind::Heart]), modified: at(NOW_MS - 12 * MINUTE_MS), ..card }).unwrap();
    let tag = Project { id: "tag".into(), name: "Gift tag".into(), design: design(&[ShapeKind::Circle]), created: 1, modified: at(NOW_MS - 7 * MINUTE_MS) };
    store::write_autosave(&dir, &tag).unwrap();
    session::write(&dir, &Session { tabs: vec!["card".into(), "tag".into()], active: 0, pid: Some(4_000_000) }).unwrap();
    root
}

#[test]
fn after_a_crash_the_recovery_dialog_lists_the_work_and_restore_all_reopens_it() {
    let mut fixture = launch(crashed(), SIZE);
    fixture.get_by_label("Restore Birthday card, unsaved changes from 10:30");
    fixture.get_by_label("Restore Gift tag, unsaved changes from 10:35");
    assert_snapshot(&mut fixture, "33-recovery");

    click(&mut fixture, "Restore all");
    assert!(fixture.query_by_label("Restore all").is_none(), "the dialog closes");
    let app = fixture.state();
    assert_eq!(app.project.as_ref().map(|p| p.id.as_str()), Some("card"));
    assert_eq!((app.title.as_str(), kinds(&app.design)), ("Birthday card", vec![ShapeKind::Star, ShapeKind::Heart]));
    fixture.get_by_label("Edited");
    flush(&fixture);
    assert!(store::autosave_path(&fixture.projects, "tag").exists(), "the other restored project keeps its autosave for next time");

    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(kinds(&fixture.state().design), [ShapeKind::Star], "one Undo returns to the saved file");
}

#[test]
fn discarding_recovery_moves_saved_work_into_versions() {
    let mut fixture = launch(crashed(), SIZE);
    click(&mut fixture, "Discard");
    flush(&fixture);
    assert!(fixture.state().design.shapes.is_empty(), "the blank canvas stays");
    assert!(!store::autosave_path(&fixture.projects, "card").exists());
    assert!(!store::autosave_path(&fixture.projects, "tag").exists());
    let versions = store::read_index(&fixture.projects, "card").unwrap().versions;
    assert_eq!(versions.len(), 1);
    let kept = store::read_version(&fixture.projects, "card", versions[0].id).unwrap();
    assert_eq!((versions[0].kind, kinds(&kept.design)), (VersionKind::Auto, vec![ShapeKind::Star, ShapeKind::Heart]));
}

#[test]
fn opening_a_project_with_a_newer_autosave_offers_it() {
    let root = crashed();
    session::write(&root.path().join("projects"), &Session::default()).unwrap();
    let mut fixture = launch(root, SIZE);
    assert!(fixture.query_by_label("Restore all").is_none(), "a clean session offers no recovery");
    click(&mut fixture, "Edit");
    fixture.get_by_label("Restore Birthday card?");
    click(&mut fixture, "Restore autosave");
    assert_eq!(kinds(&fixture.state().design), [ShapeKind::Star, ShapeKind::Heart]);
    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(kinds(&fixture.state().design), [ShapeKind::Star], "Restore is one undo step");
}

/// "Wreath" saved at 10:30 as a heart, with a star saved at 09:15 and a circle autosaved on Sunday.
fn versioned() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("projects");
    let saved_at = (NOW_MS - 12 * MINUTE_MS) / 1000;
    let wreath = |kind: ShapeKind| Project { id: "wreath".into(), name: "Wreath".into(), design: design(&[kind]), created: saved_at, modified: saved_at };
    store::record_version(&dir, &wreath(ShapeKind::Circle), VersionKind::Auto, NOW_MS - (18 * 60 + 37) * MINUTE_MS).unwrap();
    store::save_project(&dir, &wreath(ShapeKind::Star), NOW_MS - 87 * MINUTE_MS).unwrap();
    store::name_version(&dir, "wreath", store::VersionId(NOW_MS - 87 * MINUTE_MS), "First draft".into()).unwrap();
    store::save_project(&dir, &wreath(ShapeKind::Heart), NOW_MS - 12 * MINUTE_MS).unwrap();
    root
}

#[test]
fn version_history_previews_and_restores_an_older_version_as_one_undo_step() {
    let mut fixture = launch(versioned(), SIZE);
    click(&mut fixture, "Edit");
    file_menu(&mut fixture, "Version history");
    fixture.get_by_label("Monday 12 October");
    fixture.get_by_label("Sunday 11 October");

    click(&mut fixture, "Version 09:15 Saved");
    let app = fixture.state();
    assert_eq!(app.autosave.preview().map(kinds), Some(vec![ShapeKind::Star]), "the canvas previews the star");
    assert_eq!(kinds(&app.design), [ShapeKind::Heart], "the design itself is untouched");
    assert_snapshot(&mut fixture, "33-versions");

    click(&mut fixture, "Restore version");
    assert_eq!(kinds(&fixture.state().design), [ShapeKind::Star]);
    assert!(fixture.state().autosave.preview().is_none(), "Restore ends the preview");
    fixture.get_by_label("Version 10:42 Before restore");
    let index = store::read_index(&fixture.projects, "wreath").unwrap();
    assert_eq!(index.versions.last().map(|v| v.kind), Some(VersionKind::BeforeRestore));

    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(kinds(&fixture.state().design), [ShapeKind::Heart], "one Undo returns");
}

#[test]
fn naming_a_version_pins_it() {
    let mut fixture = launch(versioned(), SIZE);
    click(&mut fixture, "Edit");
    file_menu(&mut fixture, "Version history");
    click(&mut fixture, "Version 10:30 Saved");
    click(&mut fixture, "Name version");
    fixture.get_by_label("Name this version").type_text("Final");
    fixture.run();
    key(&mut fixture, egui::Key::Enter, egui::Modifiers::NONE);
    flush(&fixture);
    let index = store::read_index(&fixture.projects, "wreath").unwrap();
    let named = index.versions.iter().find(|v| v.id.0 == NOW_MS - 12 * MINUTE_MS).unwrap();
    assert_eq!((named.name.as_deref(), named.pinned), (Some("Final"), true));
    click(&mut fixture, "Close version history");
    assert!(fixture.state().autosave.versions.is_none());
}

/// Two saved projects: "Birthday card" (a star, the newer, so first on Home) and "Gift tag" (a circle).
fn two_saved() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("projects");
    let saved = |id: &str, name: &str, kind: ShapeKind, minutes_ago: u64| {
        let at = (NOW_MS - minutes_ago * MINUTE_MS) / 1000;
        project::save(&dir, &Project { id: id.into(), name: name.into(), design: design(&[kind]), created: at, modified: at }).unwrap();
    };
    saved("card", "Birthday card", ShapeKind::Star, 30);
    saved("tag", "Gift tag", ShapeKind::Circle, 60);
    root
}

/// Home > Edit on the card named `name`.
fn edit_from_home(fixture: &mut Fixture, name: &str) {
    if fixture.query_by_label("Home tab").is_some() {
        click(fixture, "Home tab");
    } else {
        file_menu(fixture, "Home");
    }
    let at = fixture.state().home_projects.iter().position(|e| e.project().is_some_and(|p| p.name == name)).expect("on Home");
    fixture.get_all_by_label("Edit").nth(at).expect("its Edit button").click();
    fixture.run();
}

fn add_square(fixture: &mut Fixture) {
    if fixture.query_by_label("Square").is_none() {
        fixture.get_by_label("Shapes").click();
        fixture.step();
    }
    click(fixture, "Square");
}

fn titles(fixture: &Fixture) -> Vec<String> {
    fixture.state().docs().map(|d| d.title.clone()).collect()
}

fn kinds_of(fixture: &Fixture, title: &str) -> Vec<ShapeKind> {
    fixture.state().docs().find(|d| d.title == title).map(|d| kinds(&d.design)).expect("open")
}

#[test]
fn each_tab_keeps_its_own_undo() {
    let mut fixture = launch(two_saved(), SIZE);
    edit_from_home(&mut fixture, "Birthday card");
    add_square(&mut fixture);
    edit_from_home(&mut fixture, "Gift tag");
    add_square(&mut fixture);
    assert_eq!(titles(&fixture), ["Birthday card", "Gift tag"], "the untouched blank tab gave way to the first project");

    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(kinds_of(&fixture, "Gift tag"), [ShapeKind::Circle], "Undo takes back the tag's square");
    assert_eq!(kinds_of(&fixture, "Birthday card"), [ShapeKind::Star, inserted_square()], "and leaves the card alone");
    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(kinds_of(&fixture, "Gift tag"), [ShapeKind::Circle], "the tag has nothing more to undo");
    assert_eq!(kinds_of(&fixture, "Birthday card"), [ShapeKind::Star, inserted_square()], "Undo never crosses tabs");

    click(&mut fixture, "Tab Birthday card");
    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(kinds(&fixture.state().design), [ShapeKind::Star], "the card's own Undo");
}

#[test]
fn opening_a_project_already_open_shows_its_tab() {
    let mut fixture = launch(two_saved(), SIZE);
    edit_from_home(&mut fixture, "Birthday card");
    edit_from_home(&mut fixture, "Gift tag");
    assert_eq!((fixture.state().tab_count(), fixture.state().active_index()), (2, 1));
    edit_from_home(&mut fixture, "Birthday card");
    assert_eq!((fixture.state().tab_count(), fixture.state().active_index()), (2, 0), "no second copy; its tab is active");
    assert_eq!(fixture.state().title, "Birthday card");
}

#[test]
fn tab_keys_close_cycle_and_jump() {
    let mut fixture = launch(two_saved(), SIZE);
    edit_from_home(&mut fixture, "Birthday card");
    edit_from_home(&mut fixture, "Gift tag");
    key(&mut fixture, egui::Key::T, egui::Modifiers::COMMAND);
    assert_eq!(titles(&fixture), ["Birthday card", "Gift tag", "Untitled-1"], "the new-tab shortcut opens a blank tab, numbered from 1 again");
    assert!(fixture.state().docs().all(|d| d.tool == super::canvas::Tool::Select), "the new-tab shortcut is not the Text tool's T");

    key(&mut fixture, egui::Key::Tab, egui::Modifiers::CTRL);
    assert_eq!(fixture.state().title, "Birthday card", "Ctrl+Tab wraps to the first tab");
    key(&mut fixture, egui::Key::Tab, egui::Modifiers::CTRL | egui::Modifiers::SHIFT);
    assert_eq!(fixture.state().title, "Untitled-1", "Ctrl+Shift+Tab wraps back");
    key(&mut fixture, egui::Key::Num2, egui::Modifiers::COMMAND | egui::Modifiers::ALT);
    assert_eq!(fixture.state().title, "Gift tag", "command and 2 jumps to the second tab");

    key(&mut fixture, egui::Key::W, egui::Modifiers::COMMAND);
    assert_eq!(titles(&fixture), ["Birthday card", "Untitled-1"], "the close-tab shortcut closes it");
    assert_eq!(fixture.state().title, "Untitled-1", "the tab to its right takes over");
    click(&mut fixture, "Close Untitled-1");
    assert!(fixture.query_by_label("Tab Birthday card").is_none(), "one tab left: the canvas hides the strip");
    key(&mut fixture, egui::Key::W, egui::Modifiers::COMMAND);
    assert_eq!(titles(&fixture), ["Untitled-1"], "closing the last tab leaves a blank one");
    assert!(fixture.state().design.shapes.is_empty());
}

#[test]
fn quitting_asks_nothing_and_the_next_launch_reopens_the_tabs() {
    let mut fixture = launch(two_saved(), SIZE);
    edit_from_home(&mut fixture, "Birthday card");
    add_square(&mut fixture);
    edit_from_home(&mut fixture, "Gift tag");
    fixture.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    fixture.run();
    assert!(fixture.query_by_label("Don't save").is_none(), "nothing asks");
    flush(&fixture);
    let autosave = store::read_autosave(&fixture.projects, "card").unwrap().expect("the card's edit is autosaved");
    assert_eq!(kinds(&autosave.design), [ShapeKind::Star, inserted_square()]);
    let clean = Session { tabs: vec!["card".into(), "tag".into()], active: 1, pid: None };
    assert_eq!(session::read(&fixture.projects), Some(clean), "a clean session names both tabs");

    let root = std::mem::replace(&mut fixture.root, tempfile::tempdir().unwrap());
    drop(fixture);
    let mut relaunched = launch(root, SIZE);
    assert_eq!(titles(&relaunched), ["Birthday card", "Gift tag"], "the tabs come back in order");
    assert_eq!(relaunched.state().title, "Gift tag", "with the same one active");
    assert!(relaunched.query_by_label("Restore all").is_none(), "a clean quit is no crash");
    click(&mut relaunched, "Tab Birthday card");
    relaunched.get_by_label("Restore Birthday card?");
    click(&mut relaunched, "Restore autosave");
    assert_eq!(kinds(&relaunched.state().design), [ShapeKind::Star, inserted_square()], "the unsaved square is back");
}

#[test]
fn restore_all_reopens_every_recovered_tab() {
    let mut fixture = launch(crashed(), SIZE);
    click(&mut fixture, "Restore all");
    assert_eq!(titles(&fixture), ["Birthday card", "Gift tag"], "both reopen, in session order");
    assert_eq!(fixture.state().active_index(), 0, "the first is active");
    assert_eq!(kinds_of(&fixture, "Gift tag"), [ShapeKind::Circle], "the never-saved tag comes back from its autosave");
    click(&mut fixture, "Tab Gift tag");
    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    assert!(fixture.state().design.shapes.is_empty(), "its restore is one undo step too");
}

/// A click while a Make flow is open, which repaints on a timer so `run` never settles.
fn tap(fixture: &mut Fixture, label: &str) {
    fixture.get_by_label(label).click();
    fixture.run_steps(3);
}

#[test]
fn one_make_runs_at_a_time() {
    let mut fixture = launch(two_saved(), SIZE);
    edit_from_home(&mut fixture, "Birthday card");
    edit_from_home(&mut fixture, "Gift tag");
    click(&mut fixture, "Tab Birthday card");
    tap(&mut fixture, "Make");
    assert!(fixture.state().flow.is_some(), "the card is making");
    let making = |fixture: &Fixture| fixture.state().docs().map(|d| d.flow.is_some()).collect::<Vec<_>>();

    tap(&mut fixture, "Tab Gift tag");
    tap(&mut fixture, "Make");
    fixture.get_by_label("Birthday card is using the machine. One Make runs at a time.");
    tap(&mut fixture, "Not now");
    assert_eq!(making(&fixture), [true, false], "Not now leaves the card's Make alone");

    tap(&mut fixture, "Make");
    tap(&mut fixture, "Go to Birthday card");
    assert_eq!((fixture.state().active_index(), making(&fixture)), (0, vec![true, false]), "Go to shows the running Make");

    tap(&mut fixture, "Tab Gift tag");
    tap(&mut fixture, "Make");
    tap(&mut fixture, "Cancel that Make");
    assert_eq!((fixture.state().active_index(), making(&fixture)), (1, vec![false, true]), "the card's Make is cancelled and the tag's starts");
}

#[test]
fn three_tabs_show_on_the_strip_with_an_edited_dot() {
    let mut fixture = launch(two_saved(), SIZE);
    edit_from_home(&mut fixture, "Birthday card");
    add_square(&mut fixture);
    edit_from_home(&mut fixture, "Gift tag");
    key(&mut fixture, egui::Key::T, egui::Modifiers::COMMAND);
    fixture.get_by_label("Tab Birthday card");
    fixture.get_by_label("Close Gift tag");
    assert_snapshot(&mut fixture, "33-tabs");
}

/// Home, from the tab strip when it shows and the File menu otherwise.
fn go_home(fixture: &mut Fixture) {
    if fixture.query_by_label("Home tab").is_some() {
        click(fixture, "Home tab");
    } else {
        file_menu(fixture, "Home");
    }
}

#[test]
fn a_closed_never_saved_tab_is_offered_back_on_home() {
    let mut fixture = snapshot_harness(SIZE);
    add_square(&mut fixture);
    key(&mut fixture, egui::Key::W, egui::Modifiers::COMMAND);
    assert!(fixture.state().design.shapes.is_empty(), "the close-tab shortcut leaves a blank tab");

    go_home(&mut fixture);
    fixture.get_by_label("Open unsaved Untitled-1");
    assert_snapshot(&mut fixture, "33-unsaved-card");
    click(&mut fixture, "Open unsaved Untitled-1");
    fixture.get_by_label("Restore Untitled-1?");
    click(&mut fixture, "Restore autosave");
    assert_eq!(kinds(&fixture.state().design), [inserted_square()], "the closed work is back");
    assert!(fixture.state().project.is_none(), "still never saved");

    go_home(&mut fixture);
    assert!(fixture.query_by_label("Open unsaved Untitled-1").is_none(), "an open tab is not also a card");
}

#[test]
fn closing_a_clean_blank_tab_leaves_no_card() {
    let mut fixture = snapshot_harness(SIZE);
    key(&mut fixture, egui::Key::T, egui::Modifiers::COMMAND);
    key(&mut fixture, egui::Key::W, egui::Modifiers::COMMAND);
    add_square(&mut fixture);
    key(&mut fixture, egui::Key::Z, egui::Modifiers::COMMAND);
    key(&mut fixture, egui::Key::W, egui::Modifiers::COMMAND);
    go_home(&mut fixture);
    assert!(fixture.query_by_label_contains("Open unsaved").is_none());
    assert!(fixture.state().home_unsaved.is_empty());
}

#[test]
fn deleting_an_unsaved_card_asks_then_removes_its_autosave() {
    let mut fixture = snapshot_harness(SIZE);
    add_square(&mut fixture);
    key(&mut fixture, egui::Key::W, egui::Modifiers::COMMAND);
    go_home(&mut fixture);
    let id = fixture.state().home_unsaved[0].id().to_owned();
    assert!(store::autosave_path(&fixture.projects, &id).exists());

    click(&mut fixture, "Delete Untitled-1");
    click(&mut fixture, "Cancel delete");
    fixture.get_by_label("Open unsaved Untitled-1");
    click(&mut fixture, "Delete Untitled-1");
    click(&mut fixture, "Confirm delete");
    flush(&fixture);
    assert!(fixture.query_by_label("Open unsaved Untitled-1").is_none());
    assert!(!store::autosave_path(&fixture.projects, &id).exists());
}

#[test]
fn an_unreadable_autosave_is_listed_so_it_can_be_deleted() {
    let root = tempfile::tempdir().unwrap();
    let autosaves = root.path().join("projects").join("autosave");
    std::fs::create_dir_all(&autosaves).unwrap();
    std::fs::write(autosaves.join("torn.json"), b"{ not json").unwrap();
    let mut fixture = launch(root, SIZE);
    fixture.get_by_label_contains("Couldn't open autosave/torn.json");
    click(&mut fixture, "Delete autosave/torn.json");
    click(&mut fixture, "Confirm delete");
    assert!(!autosaves.join("torn.json").exists());
}
