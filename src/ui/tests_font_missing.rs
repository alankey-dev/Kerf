//! A text set in a system family that is not installed cuts in Inter, so Prepare says so and Layers marks
//! the text (retriage 2026-10-14, item 3).

use egui::Color32;
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, snapshot_harness};
use crate::system_fonts::FIXTURE_FAMILIES;
use crate::text::FontRef;

const MISSING: &str = "Zapfino";
const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// The app with one text in `family` on the artboard, and the id of that text.
fn with_text_in(family: &str) -> (Fixture, u64) {
    let mut harness = snapshot_harness(SIZE);
    let id = harness.state_mut().design.add_text("Hello".into(), FontRef::System(family.into()), 1.0, Color32::BLACK, 2.0, 2.0);
    harness.run();
    (harness, id)
}

fn open_prepare(harness: &mut Fixture) {
    harness.get_by_label("Make").click();
    harness.run_steps(4);
    assert!(harness.state().flow.is_some(), "Make opened Prepare");
}

fn warns(harness: &Fixture) -> bool {
    harness.query_by_label_contains("cuts in Inter").is_some()
}

#[test]
fn prepare_names_a_family_that_is_not_installed_and_says_it_cuts_in_inter() {
    let (mut harness, _) = with_text_in(MISSING);
    open_prepare(&mut harness);
    assert!(harness.query_by_label_contains(MISSING).is_some(), "the notice names {MISSING}");
    assert!(warns(&harness), "the notice says the text cuts in Inter");
    assert!(harness.query_by_label("Pick a font").is_some(), "the notice offers a way to fix it");
}

#[test]
fn prepare_stays_quiet_for_an_installed_family() {
    let (mut harness, _) = with_text_in(FIXTURE_FAMILIES[0]);
    open_prepare(&mut harness);
    assert!(!warns(&harness));
    assert!(harness.query_by_label("Pick a font").is_none());
}

#[test]
fn pick_a_font_returns_to_the_canvas_with_the_text_selected_and_picking_an_installed_family_clears_the_notice() {
    let (mut harness, id) = with_text_in(MISSING);
    harness.state_mut().design.add_shape(crate::design::ShapeKind::Square);
    open_prepare(&mut harness);

    harness.get_by_label("Pick a font").click();
    harness.run_steps(4);
    assert!(harness.state().flow.is_none(), "the flow closes");
    assert_eq!(harness.state().selection, vec![id], "the affected text is selected");

    harness.get_by_label("Font").click();
    harness.run();
    harness.get_by_label(FIXTURE_FAMILIES[1]).click();
    harness.run();
    open_prepare(&mut harness);
    assert!(!warns(&harness), "the family is installed now");
}

#[test]
fn layers_marks_the_text_whose_family_is_not_installed() {
    let (harness, _) = with_text_in(MISSING);
    assert!(harness.query_by_label("Font not installed").is_some(), "the row carries the warning glyph");
    let (harness, _) = with_text_in(FIXTURE_FAMILIES[0]);
    assert!(harness.query_by_label("Font not installed").is_none(), "an installed family is not marked");
}

#[test]
fn layers_drops_the_mark_once_an_installed_family_is_picked() {
    let (mut harness, id) = with_text_in(MISSING);
    harness.state_mut().selection = vec![id];
    harness.run();
    assert!(harness.query_by_label("Font not installed").is_some());
    harness.get_by_label("Font").click();
    harness.run();
    harness.get_by_label(FIXTURE_FAMILIES[0]).click();
    harness.run();
    assert!(harness.query_by_label("Font not installed").is_none());
}
