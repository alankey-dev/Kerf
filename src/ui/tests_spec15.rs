//! Spec 15: system fonts in the text toolbar's font menu, driven through the app.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::test_harness::{REFERENCE_CSS, assert_snapshot, click_at, snapshot_harness};
use super::{Kerf, canvas};
use crate::design::{Shape, ShapeKind};
use crate::text::FontRef;

use super::toolbar::font_menu;

fn place_text(harness: &mut Harness<'_, Kerf>) -> Shape {
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(harness, egui::pos2(700.0, 400.0));
    harness.state().design.shapes.last().unwrap().clone()
}

#[test]
fn the_font_menu_searches_and_picks_a_system_family() {
    let family = crate::system_fonts::FIXTURE_FAMILIES[0].to_owned();
    let mut harness = snapshot_harness(REFERENCE_CSS);
    let placed = place_text(&mut harness);

    harness.get_by_label("Font").click();
    harness.run();
    assert!(harness.query_by_label("Search fonts").is_some(), "the menu opens with a search field");
    assert!(harness.query_by_label("Loading fonts...").is_none(), "the scan has finished");
    assert!(harness.query_by_label(&family).is_some(), "the fixture families are listed");
    assert_snapshot(&mut harness, "15-font-menu");

    harness.get_by_label("Search fonts").type_text(&family);
    harness.run();
    assert!(harness.query_by_label("Space Grotesk").is_none(), "the search hides fonts that do not match");
    harness.get_by_label(&family).click();
    harness.run();

    let picked = harness.state().design.shapes.iter().find(|s| s.id == placed.id).unwrap().clone();
    assert!(matches!(&picked.kind, ShapeKind::Text { font: FontRef::System(name), .. } if *name == family), "{:?}", picked.kind);
    assert_ne!(picked.contours(), placed.contours(), "the outline is set in {family}");
    assert!(harness.query_by_label("Search fonts").is_none(), "picking closes the menu");
}

/// The font menu alone, with the app's fonts installed on the first frame and shown from the next.
fn menu_harness<'a>(current: &'a FontRef, system: Option<&'a [String]>) -> Harness<'a> {
    let mut installed = false;
    Harness::new_ui(move |ui| {
        if !installed {
            super::fonts::install(ui.ctx());
            installed = true;
            return;
        }
        font_menu::show(ui, Some(current), system, &[]);
    })
}

#[test]
fn the_menu_shows_loading_until_the_scan_lands_then_flags_a_missing_family() {
    let current = FontRef::System("Gone Sans".into());
    let mut harness = menu_harness(&current, None);
    harness.run();
    assert!(harness.query_by_label("Loading fonts...").is_some(), "the menu opens before discovery completes");
    assert!(harness.query_by_label("Gone Sans (missing)").is_none(), "nothing is missing until the scan lands");
    assert!(harness.query_by_label("Inter").is_some(), "bundled fonts are offered meanwhile");

    let installed = vec!["Arial".to_owned()];
    let mut harness = menu_harness(&current, Some(&installed));
    harness.run();
    assert!(harness.query_by_label("Gone Sans (missing)").is_some());
    assert!(harness.query_by_label("Arial").is_some());
    assert!(harness.query_by_label("Loading fonts...").is_none());
}
