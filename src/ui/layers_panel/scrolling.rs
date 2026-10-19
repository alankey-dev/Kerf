//! A long Layers list lays out only the rows in view, and everything off screen still works: selecting
//! scrolls to it, a drag drops on rows it scrolled in, and a row scrolled in renames in place.

use std::time::Instant;

use egui::{Key, Modifiers, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::design::ShapeKind;
use crate::ui::test_harness::{Fixture, click_at, double_click_at, key, move_to, press_at, rect_of, snapshot_harness};

const LAYERS: u64 = 300;

/// Layers 1 to `LAYERS`, named "Row 1" and so on, listed with the highest on top.
fn long_list() -> Fixture {
    let mut harness = snapshot_harness(vec2(1400.0, 900.0));
    for id in 1..=LAYERS {
        harness.state_mut().design.add_shape(ShapeKind::Square);
        harness.state_mut().design.shape_mut(id).unwrap().name = Some(format!("Row {id}"));
    }
    harness.run();
    harness
}

/// The ids whose rows have an accessibility node, on screen or not.
fn built(harness: &Fixture) -> Vec<u64> {
    (1..=LAYERS).filter(|id| harness.query_by_label(&format!("Layer Row {id}")).is_some()).collect()
}

/// The ids whose rows are within the list's frame, from the tabs above it to the bar below.
fn shown(harness: &Fixture) -> Vec<u64> {
    let (top, bottom) = (rect_of(harness, "Components").bottom(), bar_top(harness));
    built(harness)
        .into_iter()
        .filter(|id| {
            let r = rect_of(harness, &format!("Layer Row {id}"));
            r.top() >= top && r.bottom() <= bottom + 1.0
        })
        .collect()
}

fn bar_top(harness: &Fixture) -> f32 {
    rect_of(harness, "Layers Slice").top()
}

/// A point on `id`'s name.
fn on_name(harness: &Fixture, id: u64) -> Pos2 {
    let r = rect_of(harness, &format!("Layer Row {id}"));
    pos2(r.left() + 60.0, r.top() + 14.0)
}

fn scroll_list(harness: &mut Fixture, by: f32) {
    let over = pos2(1300.0, bar_top(harness) - 100.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(over));
    harness.input_mut().events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, by), modifiers: Modifiers::NONE });
    harness.run_steps(8);
}

fn in_view(harness: &Fixture, id: u64) -> bool {
    shown(harness).contains(&id)
}

#[test]
fn only_the_rows_in_view_are_laid_out() {
    let harness = long_list();
    let built = built(&harness);
    assert!(built.len() > 10, "the list shows its first rows: {}", built.len());
    assert!(built.len() < 40, "{} of {LAYERS} rows were built", built.len());
    assert_eq!(built.last(), Some(&LAYERS), "the top of the z-order lists first");
}

#[test]
fn a_selection_made_off_screen_scrolls_its_row_into_view() {
    let mut harness = long_list();
    assert!(!in_view(&harness, 1));
    harness.state_mut().selection = vec![1];
    harness.run_steps(12);
    assert!(in_view(&harness, 1), "the list scrolled to the selected bottom row");
    harness.state_mut().selection = vec![LAYERS];
    harness.run_steps(12);
    assert!(in_view(&harness, LAYERS), "and back to the top one");
}

#[test]
fn a_scrolled_row_selects_and_hides_like_any_other() {
    let mut harness = long_list();
    scroll_list(&mut harness, -1500.0);
    let shown = shown(&harness);
    let id = shown[shown.len() / 2];
    assert!(id < LAYERS - 20, "row {id} was off screen at first");
    harness.get_by_label(&format!("Hide Row {id}")).click();
    harness.run();
    assert!(harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().hidden, "its eye hides it");
    let at = on_name(&harness, id);
    click_at(&mut harness, at);
    assert_eq!(harness.state().selection, vec![id], "its name selects it");
}

#[test]
fn a_drag_drops_on_a_row_the_list_scrolled_in() {
    let mut harness = long_list();
    let from = on_name(&harness, LAYERS);
    move_to(&mut harness, from);
    press_at(&mut harness, from, true);
    move_to(&mut harness, from + vec2(0.0, 8.0));
    let edge = pos2(from.x, bar_top(&harness) - 12.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(edge));
    for _ in 0..16 {
        harness.step();
    }
    assert!(shown(&harness).iter().all(|&id| id < LAYERS - 10), "the list scrolled while the row was held");

    let mid = pos2(from.x, bar_top(&harness) / 2.0);
    move_to(&mut harness, mid);
    harness.run_steps(8);
    let shown = shown(&harness);
    let target = shown[shown.len() / 2];
    let r = rect_of(&harness, &format!("Layer Row {target}"));
    let drop = pos2(r.left() + 60.0, r.top() + 4.0);
    move_to(&mut harness, drop);
    press_at(&mut harness, drop, false);

    let order: Vec<u64> = harness.state().design.shapes.iter().map(|s| s.id).collect();
    let at = |id| order.iter().position(|&i| i == id).unwrap();
    assert_eq!(at(LAYERS), at(target) + 1, "the row lands in the gap above row {target}");
    assert_eq!(harness.state().selection, vec![LAYERS], "and is selected");
}

#[test]
fn a_row_scrolled_in_renames_in_place() {
    let mut harness = long_list();
    scroll_list(&mut harness, -2000.0);
    let shown = shown(&harness);
    let id = shown[shown.len() / 2];
    harness.state_mut().selection = vec![id];
    harness.run_steps(8);
    let at = on_name(&harness, id);
    double_click_at(&mut harness, at);
    assert!(harness.query_by_label("Layer name").is_some(), "the name opens for typing");
    harness.input_mut().events.push(egui::Event::Text("Scrolled".into()));
    harness.run();
    key(&mut harness, Key::Enter, Modifiers::NONE);
    let d = &harness.state().design;
    assert_eq!(d.layer_name(d.shapes.iter().find(|s| s.id == id).unwrap()), "Scrolled");
    assert!(harness.query_by_label("Layer Scrolled").is_some(), "the row shows its new name");
}

#[test]
fn renaming_an_off_screen_selection_scrolls_to_its_field() {
    let mut harness = long_list();
    harness.state_mut().selection = vec![1];
    harness.run_steps(12);
    scroll_list(&mut harness, 5000.0);
    assert!(!in_view(&harness, 1));
    key(&mut harness, Key::F2, Modifiers::NONE);
    harness.run_steps(12);
    assert!(in_view(&harness, 1), "the list scrolled to the row being renamed");
    assert!(harness.query_by_label("Layer name").is_some(), "its field is open");
}

/// The heavy scenario's idle frame with the Layers tab and with the Components tab, which lists
/// nothing, so their difference is the Layers list's cost.
#[test]
#[ignore = "timing; run on the release build"]
fn the_layers_list_costs_what_is_in_view_not_what_is_in_the_design() {
    let mut f = snapshot_harness(vec2(1600.0, 1000.0));
    let design = crate::bench::build(crate::bench::DEFAULT_SEED, crate::bench::DEFAULT_OBJECTS);
    let written = crate::bench::write(&f.root.path().join("bench"), &design).unwrap();
    crate::bench::install_image(&f.projects).unwrap();
    f.state_mut().open_file(&written.project).unwrap();
    f.run();
    let median = |f: &mut Fixture| {
        let mut ms: Vec<f32> = (0..240)
            .map(|_| {
                let clock = Instant::now();
                f.step();
                clock.elapsed().as_secs_f32() * 1000.0
            })
            .collect();
        ms.sort_by(f32::total_cmp);
        ms[ms.len() / 2]
    };
    let mut layers = Vec::new();
    let mut without = Vec::new();
    for _ in 0..3 {
        f.get_by_label("Layers").click();
        f.run();
        layers.push(median(&mut f));
        f.get_by_label("Components").click();
        f.run();
        without.push(median(&mut f));
    }
    layers.sort_by(f32::total_cmp);
    without.sort_by(f32::total_cmp);
    println!("\nidle frame, Layers tab {:.2} ms, Components tab {:.2} ms, Layers list {:.2} ms", layers[1], without[1], layers[1] - without[1]);
}
