//! Spec 30: Optimise layout in Prepare nests the mats, shows what it saved, and is one undo step;
//! a design changed since shows a notice.

use std::time::Duration;

use egui::{Color32, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{Fixture, assert_flow_snapshot, rect_of, snapshot_harness};
use crate::composition::compose_on;
use crate::composition::nested::material_length;
use crate::design::{Design, Operation, ShapeKind};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// Black hearts, stars, circles and an upright strip, laid out as a user might leave them: packing
/// puts them on a mat with gaps that nesting closes.
fn mixed(fixture: &mut Fixture) {
    let mut d = Design::default();
    let shapes = [
        (ShapeKind::Rectangle, (0.5, 0.5, 1.0, 5.0)),
        (ShapeKind::Heart, (2.0, 0.5, 3.0, 3.0)),
        (ShapeKind::Star, (5.5, 0.5, 2.0, 2.0)),
        (ShapeKind::Circle, (8.0, 0.5, 1.5, 1.5)),
        (ShapeKind::Heart, (2.0, 4.0, 1.5, 1.5)),
        (ShapeKind::Triangle, (4.0, 4.0, 2.5, 2.0)),
        (ShapeKind::Circle, (7.0, 4.0, 1.0, 1.0)),
        (ShapeKind::Star, (9.0, 4.0, 1.25, 1.25)),
    ];
    for (kind, (x, y, w, h)) in shapes {
        let id = d.add_shape(kind);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in, s.fill, s.operation) = (x, y, w, h, Color32::BLACK, Operation::BasicCut);
    }
    fixture.state_mut().design = d;
    fixture.state_mut().nest.budget = Duration::from_secs(120);
    fixture.run();
}

fn to_prepare(fixture: &mut Fixture) {
    fixture.get_by_label("Make").click();
    fixture.run_steps(10);
    assert!(fixture.state().flow.is_some(), "Make opens Prepare");
}

/// Steps the app until `done` holds. The nesting thread's layout is waited for by joining it, so the
/// next frame collects it; no clock is involved.
fn wait(fixture: &mut Fixture, what: &str, done: impl Fn(&Kerf) -> bool) {
    fixture.state_mut().nest.wait_finished();
    for _ in 0..10 {
        if done(fixture.state()) {
            fixture.run_steps(3);
            return;
        }
        fixture.step();
    }
    panic!("{what} not reached");
}

fn optimise(fixture: &mut Fixture) {
    fixture.get_by_label("Optimise layout").click();
    fixture.step();
    wait(fixture, "the optimised layout", |app| app.design.nesting.is_some() && app.flow.as_ref().is_some_and(|f| f.design.nesting.is_some()));
}

/// Material used by the flow's cut mats, and what today's packing of the same design would use.
fn lengths(app: &Kerf) -> (f32, f32) {
    let flow = app.flow.as_ref().unwrap();
    let size = flow.setup.mat_size;
    let plain = Design { nesting: None, ..flow.design.clone() };
    let packed = compose_on(&plain, size, flow.setup.copies, flow.print.paper);
    (material_length(&plain, &packed, size), material_length(&flow.design, &flow.mats, size))
}

#[test]
fn optimise_layout_nests_shows_the_saving_and_undo_optimise_restores_the_packing() {
    let mut fixture = snapshot_harness(SIZE);
    mixed(&mut fixture);
    to_prepare(&mut fixture);
    let packed = fixture.state().flow.as_ref().unwrap().mats.clone();
    let depth = fixture.state().history.depth();
    assert_flow_snapshot(&mut fixture, "30-before");

    optimise(&mut fixture);
    let (before, after) = lengths(fixture.state());
    assert!(after < before, "nesting uses less material: {after} against {before}");
    assert_eq!(fixture.state().history.depth(), depth + 1, "one undo step");
    let saved = ((1.0 - after / before) * 100.0).round();
    let line = format!("Material: {} to {}, {saved} % less", fixture.state().units.len(before), fixture.state().units.len(after));
    fixture.get_by_label(&line);
    assert_ne!(fixture.state().flow.as_ref().unwrap().mats, packed, "Prepare shows the nested mats");
    assert_flow_snapshot(&mut fixture, "30-after");

    let text = rect_of(&fixture, &line).union(rect_of(&fixture, "Undo optimise"));
    let card = egui::Rect::from_x_y_ranges(8.0..=232.0, text.top() - 32.0..=text.bottom() + 32.0);
    let image = fixture.render().expect("wgpu render");
    let crop = image::imageops::crop_imm(&image, card.left() as u32, card.top() as u32, card.width() as u32, card.height() as u32).to_image();
    super::test_harness::assert_image_snapshot(&crop, "30-summary");

    fixture.get_by_label("Undo optimise").click();
    fixture.run_steps(3);
    assert_eq!(fixture.state().design.nesting, None);
    assert_eq!(fixture.state().history.depth(), depth);
    assert_eq!(fixture.state().flow.as_ref().unwrap().mats, packed, "the packed layout is back");
    assert!(fixture.query_by_label("Undo optimise").is_none(), "the card goes with the layout");
}

#[test]
fn cmd_z_in_prepare_undoes_the_optimise_and_the_canvas_can_redo_it() {
    let mut fixture = snapshot_harness(SIZE);
    mixed(&mut fixture);
    to_prepare(&mut fixture);
    let packed = fixture.state().flow.as_ref().unwrap().mats.clone();
    optimise(&mut fixture);
    let layout = fixture.state().design.nesting.clone();

    fixture.input_mut().modifiers = Modifiers::COMMAND;
    fixture.input_mut().events.push(egui::Event::Key { key: Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    fixture.step();
    fixture.input_mut().modifiers = Modifiers::NONE;
    fixture.run_steps(3);
    assert_eq!(fixture.state().design.nesting, None);
    assert_eq!(fixture.state().flow.as_ref().unwrap().mats, packed);

    fixture.get_by_label("Cancel").click();
    fixture.run_steps(3);
    assert!(fixture.state().flow.is_none(), "back on the canvas");
    super::test_harness::key(&mut fixture, Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(fixture.state().design.nesting, layout, "Redo brings the layout back");
}

#[test]
fn a_design_changed_after_optimising_shows_the_notice_and_optimises_again() {
    let mut fixture = snapshot_harness(SIZE);
    mixed(&mut fixture);
    to_prepare(&mut fixture);
    optimise(&mut fixture);
    fixture.get_by_label("Cancel").click();
    fixture.run_steps(3);
    assert!(fixture.state().flow.is_none());

    let star = fixture.state().design.shapes[2].id;
    fixture.state_mut().selection = vec![star];
    fixture.run();
    super::test_harness::key(&mut fixture, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(fixture.state().history.depth(), 2, "the optimise and the nudge");

    to_prepare(&mut fixture);
    fixture.get_by_label("The design changed since the layout was optimised");
    let packed = {
        let app = fixture.state();
        let plain = Design { nesting: None, ..app.design.clone() };
        let flow = app.flow.as_ref().unwrap();
        compose_on(&plain, flow.setup.mat_size, flow.setup.copies, flow.print.paper)
    };
    assert_eq!(fixture.state().flow.as_ref().unwrap().mats, packed, "a stale layout falls back to packing");

    fixture.get_by_label("Optimise again").click();
    fixture.step();
    wait(&mut fixture, "the layout again", |app| {
        app.flow
            .as_ref()
            .is_some_and(|f| crate::composition::nested::status(&f.design, f.setup.mat_size, f.setup.copies) == crate::composition::nested::LayoutStatus::Current)
    });
    assert!(fixture.query_by_label("The design changed since the layout was optimised").is_none());
}

#[test]
fn cancel_optimise_leaves_the_layout_unchanged() {
    let mut fixture = snapshot_harness(SIZE);
    mixed(&mut fixture);
    to_prepare(&mut fixture);
    fixture.state_mut().flow.as_mut().unwrap().set_copies(20);
    fixture.run_steps(3);
    let packed = fixture.state().flow.as_ref().unwrap().mats.clone();
    fixture.get_by_label("Optimise layout").click();
    fixture.run_steps(2);
    assert!(fixture.state().nest.running(), "a long run is still going");
    fixture.get_by_label("Optimising layout");
    fixture.get_by_label("Cancel optimise").click();
    fixture.run_steps(5);
    assert!(!fixture.state().nest.running());
    fixture.run_steps(3);
    assert_eq!(fixture.state().design.nesting, None);
    assert_eq!(fixture.state().flow.as_ref().unwrap().mats, packed);
}

#[test]
fn the_settings_popover_and_the_grain_lock_edit_what_nesting_reads() {
    let mut fixture = snapshot_harness(SIZE);
    mixed(&mut fixture);
    let strip = fixture.state().design.shapes[0].id;
    fixture.state_mut().selection = vec![strip];
    fixture.run();
    fixture.get_by_label("Lock grain").click();
    fixture.run();
    assert!(fixture.state().design.shapes[0].grain_lock);
    fixture.get_by_label("Unlock grain");

    to_prepare(&mut fixture);
    fixture.get_by_label("Layout settings").click();
    fixture.run_steps(3);
    fixture.get_by_label("Free").click();
    fixture.run_steps(3);
    fixture.get_by_label("45 degrees").click();
    fixture.run_steps(3);
    fixture.get_by_label("Use holes").click();
    fixture.run_steps(3);
    let settings = fixture.state().nest.settings;
    assert_eq!(settings.rotation, crate::nesting::Rotation::Free(crate::nesting::FreeStep::FortyFive));
    assert!(!settings.use_holes);

    optimise(&mut fixture);
    let flow = fixture.state().flow.as_ref().unwrap();
    let turns: Vec<f32> = flow.mats.iter().flat_map(|m| &m.items).filter(|i| i.piece.owner == strip).map(|i| i.rotation_deg).collect();
    assert!(turns.iter().all(|d| *d == 0.0 || *d == 180.0), "the locked strip keeps its grain: {turns:?}");
    assert_eq!(fixture.state().design.nesting.as_ref().unwrap().settings, settings, "the layout records its settings");
}
