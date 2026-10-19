//! Spec 32: the command palette. Cmd+K, a query and Enter run a command through the same path the
//! menus use, disabled rows say why, and an empty query lists what was run last.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::commands::{Command, Cx};
use super::palette::{self, Row, SEARCH_LABEL, Sources};
use super::test_harness::{Fixture, assert_snapshot, key, snapshot_harness, two_squares_apart};
use super::{Kerf, toolbar};
use crate::design::{Combine, ShapeKind};
use crate::edit::Edit;
use crate::palette::Section;
use crate::text::FontRef;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn type_text(harness: &mut Harness<'_, Kerf>, text: &str) {
    harness.input_mut().events.push(egui::Event::Text(text.into()));
    harness.run();
}

fn open_palette(harness: &mut Harness<'_, Kerf>) {
    key(harness, Key::K, Modifiers::COMMAND);
    assert!(harness.state().palette.is_open(), "the palette shortcut opens the palette");
    harness.get_by_label(SEARCH_LABEL);
}

/// Opens the palette, types `query` and presses Enter.
fn run_query(harness: &mut Harness<'_, Kerf>, query: &str) {
    open_palette(harness);
    type_text(harness, query);
    key(harness, Key::Enter, Modifiers::NONE);
}

/// The rows the open palette lists right now, by label, with the section each is listed under.
fn listing(app: &Kerf) -> Vec<(Section, String)> {
    let rows = rows(app);
    app.palette.listing(&rows, crate::project::now()).into_iter().map(|(s, i)| (s, rows[i].label.clone())).collect()
}

fn rows(app: &Kerf) -> Vec<Row> {
    palette::rows(&Sources {
        cx: Cx::new(&app.design, &app.selection, app.view.align_to),
        history: &app.history,
        on_canvas: true,
        system_fonts: app.fonts.families().unwrap_or_default(),
        writing_fonts: &app.fonts.writing(),
        projects: app.palette.projects(),
        board_in: app.view.artboard_in,
    })
}

fn two_selected() -> Fixture {
    let mut harness = snapshot_harness(SIZE);
    _ = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    harness
}

#[test]
fn weld_from_the_palette_is_one_undo_step() {
    let mut harness = two_selected();
    run_query(&mut harness, "weld");
    assert!(!harness.state().palette.is_open(), "Enter runs and closes");
    assert_eq!(
        harness.state().design.shapes[0].kind,
        crate::design::ShapeKind::Compound(crate::design::CompoundOp::Boolean(crate::design::Combine::Weld)),
        "the squares welded live"
    );
    key(&mut harness, Key::Z, Modifiers::COMMAND);
    let kinds: Vec<_> = harness.state().design.shapes.iter().map(|s| s.kind.clone()).collect();
    assert_eq!(kinds, [ShapeKind::Square, ShapeKind::Square], "one Undo restores both squares");
}

#[test]
fn weld_with_one_shape_is_disabled_with_the_toolbar_reason() {
    let mut harness = two_selected();
    harness.state_mut().selection = vec![1];
    harness.run();
    open_palette(&mut harness);
    type_text(&mut harness, "weld");
    assert!(harness.get_by_label("Weld").is_disabled());
    let app = harness.state();
    let row = rows(app).into_iter().find(|r| r.label == "Weld").unwrap();
    let cx = Cx::new(&app.design, &app.selection, app.view.align_to);
    let toolbar = toolbar::combine::entries(&cx).into_iter().flatten().find(|e| e.command == Command::Edit(Edit::Combine(Combine::Weld))).unwrap();
    assert_eq!(row.blocked.map(|b| b.reason()), Some(toolbar.tip_body()), "the row gives the toolbar tooltip's reason");

    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().palette.is_open(), "Enter on a disabled row keeps the palette open");
    assert_eq!(harness.state().design.shapes.len(), 2, "and changes nothing");
}

#[test]
fn heart_is_inserted_and_selected() {
    let mut harness = snapshot_harness(SIZE);
    run_query(&mut harness, "heart");
    let app = harness.state();
    let heart = app.design.shapes.last().expect("a shape was inserted");
    assert_eq!(heart.kind, ShapeKind::Heart);
    assert_eq!(app.selection, [heart.id]);
}

#[test]
fn a_font_name_sets_the_selected_text_font() {
    let mut harness = snapshot_harness(SIZE);
    let id = harness.state_mut().design.add_text("Hi".into(), FontRef::Inter, 1.0, egui::Color32::BLACK, 2.0, 2.0);
    harness.state_mut().selection = vec![id];
    harness.run();
    run_query(&mut harness, "space grotesk");
    let app = harness.state();
    assert_eq!(app.design.shapes.len(), 1, "the font changed rather than a new text being placed");
    assert!(matches!(&app.design.shapes[0].kind, ShapeKind::Text { font: FontRef::SpaceGrotesk, .. }));
}

#[test]
fn a_font_with_no_text_selected_places_a_text_in_it() {
    let mut harness = snapshot_harness(SIZE);
    run_query(&mut harness, "space grotesk");
    let app = harness.state();
    assert!(matches!(&app.design.shapes[..], [s] if matches!(&s.kind, ShapeKind::Text { font: FontRef::SpaceGrotesk, .. }) && app.selection == [s.id]));
}

#[test]
fn an_empty_query_lists_the_last_command_run_first() {
    let mut harness = two_selected();
    run_query(&mut harness, "flip horizontal");
    run_query(&mut harness, "duplicate");
    assert_eq!(harness.state().design.shapes.len(), 4);
    open_palette(&mut harness);
    let listed = listing(harness.state());
    assert_eq!(listed[..2], [(Section::Recent, "Duplicate".into()), (Section::Recent, "Flip Horizontal".into())]);
    assert!(listed.iter().any(|(s, l)| *s == Section::Suggested && l == "Weld"), "two shapes suggest Weld: {listed:?}");
    let (first, second) = (super::test_harness::rect_of(&harness, "Duplicate"), super::test_harness::rect_of(&harness, "Flip Horizontal"));
    assert!(first.top() < second.top(), "Duplicate is drawn first");
}

#[test]
fn recents_reach_settings_and_come_back_on_relaunch() {
    let mut harness = two_selected();
    run_query(&mut harness, "duplicate");
    let root = std::mem::replace(&mut harness.root, tempfile::tempdir().unwrap());
    drop(harness);
    let mut relaunched = super::test_harness::launch(root, SIZE);
    open_palette(&mut relaunched);
    assert_eq!(listing(relaunched.state()).first(), Some(&(Section::Recent, "Duplicate".into())));
}

#[test]
fn escape_cmd_k_and_a_click_outside_close_it() {
    let mut harness = snapshot_harness(SIZE);
    for close in [Key::Escape, Key::K] {
        open_palette(&mut harness);
        let modifiers = if close == Key::K { Modifiers::COMMAND } else { Modifiers::NONE };
        key(&mut harness, close, modifiers);
        assert!(!harness.state().palette.is_open(), "{close:?} closes it");
    }
    open_palette(&mut harness);
    super::test_harness::click_at(&mut harness, egui::pos2(40.0, SIZE.y - 40.0));
    assert!(!harness.state().palette.is_open(), "a click outside closes it");
    assert!(harness.state().design.shapes.is_empty());
}

#[test]
fn arrows_move_and_cmd_enter_keeps_it_open() {
    let mut harness = two_selected();
    open_palette(&mut harness);
    type_text(&mut harness, "nudge");
    let at = |h: &Fixture| (h.state().design.shapes[0].x_in, h.state().design.shapes[0].y_in);
    let before = at(&harness);
    key(&mut harness, Key::ArrowDown, Modifiers::NONE);
    let picked = listing(harness.state())[1].1.clone();
    key(&mut harness, Key::Enter, Modifiers::COMMAND);
    key(&mut harness, Key::Enter, Modifiers::COMMAND);
    assert!(harness.state().palette.is_open(), "command and Enter keeps the palette open");
    let (dx, dy) = (at(&harness).0 - before.0, at(&harness).1 - before.1);
    let want = match picked.as_str() {
        "Nudge Left" => (-0.25, 0.0),
        "Nudge Right" => (0.25, 0.0),
        "Nudge Up" => (0.0, -0.25),
        _ => (0.0, 0.25),
    };
    assert!((dx - want.0).abs() < 1e-4 && (dy - want.1).abs() < 1e-4, "two runs of {picked}, the second match, moved ({dx}, {dy})");
}

#[test]
fn home_lists_new_and_projects_but_no_canvas_commands() {
    let mut harness = snapshot_harness(SIZE);
    harness.state_mut().design.add_shape(ShapeKind::Square);
    harness.state_mut().title = "Palette project".into();
    harness.run();
    assert!(harness.state_mut().save_to_disk());
    let ctx = harness.ctx.clone();
    harness.state_mut().run(Command::App(crate::edit::AppCommand::Home), &ctx);
    harness.run();
    assert!(harness.state().tab == super::chrome::Tab::Home);
    open_palette(&mut harness);
    type_text(&mut harness, "palette project");
    assert!(harness.query_by_label("Palette project").is_some(), "the saved project is listed");
    assert!(harness.query_by_label("Weld").is_none());
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().tab == super::chrome::Tab::Canvas, "Enter opens the project");
    assert_eq!(harness.state().design.shapes.len(), 1);
}

#[test]
fn snapshot_empty_query() {
    let mut harness = two_selected();
    for key in ["Command:Flip Horizontal", "Shape:Heart", "Command:Duplicate"] {
        harness.state_mut().palette.recents.record(key, crate::project::now());
    }
    open_palette(&mut harness);
    super::test_harness::move_to(&mut harness, egui::pos2(1200.0, 700.0));
    assert_snapshot(&mut harness, "32-palette-empty");
}

#[test]
fn snapshot_query_with_a_disabled_row_and_shortcuts() {
    let mut harness = two_selected();
    harness.state_mut().selection = vec![1];
    harness.run();
    open_palette(&mut harness);
    type_text(&mut harness, "d");
    key(&mut harness, Key::ArrowDown, Modifiers::NONE);
    super::test_harness::move_to(&mut harness, egui::pos2(1200.0, 700.0));
    assert_snapshot(&mut harness, "32-palette-query");
}
