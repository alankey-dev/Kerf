use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, snapshot_harness};
use super::tokens::{RAIL_W, SHAPES_PANEL_W};
use crate::design::ShapeKind;
use crate::shape_library::Category;

fn open_shapes() -> Fixture {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.get_by_label("Shapes").click();
    harness.run();
    harness
}

fn rect(harness: &Fixture, label: &str) -> egui::Rect {
    let b = harness.get_by_label(label).bounding_box().unwrap();
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

#[test]
fn filter_button_sits_inside_the_panel_flush_with_the_third_column() {
    let harness = open_shapes();
    let filter = rect(&harness, "Filter shapes");
    let third = rect(&harness, "Triangle");
    let first = rect(&harness, "Line");
    assert!(filter.left() >= first.left(), "filter {filter:?} starts inside the grid, first tile {first:?}");
    assert!((filter.right() - third.right()).abs() <= 1.0, "filter right {} vs third column right {}", filter.right(), third.right());
    assert!(filter.bottom() <= first.top(), "the header sits above the grid");
}

/// The labels of the shape tiles the panel lays out, in grid order.
fn tiles(harness: &Fixture) -> Vec<&'static str> {
    let panel_right = f64::from(RAIL_W + SHAPES_PANEL_W);
    ShapeKind::free()
        .iter()
        .map(ShapeKind::label)
        .filter(|label| harness.query_all_by_label(label).any(|n| n.bounding_box().is_some_and(|b| b.x1 <= panel_right)))
        .collect()
}

fn all_tiles() -> Vec<&'static str> {
    ShapeKind::free().iter().map(ShapeKind::label).collect()
}

#[test]
fn stars_and_bursts_narrows_the_grid_and_clear_filters_restores_it() {
    let mut harness = open_shapes();
    assert_eq!(tiles(&harness), all_tiles());
    harness.get_by_label("Filter shapes").click();
    harness.run();
    harness.get_by_label("Stars and Bursts").click();
    harness.run();
    assert!(harness.query_by_label("Clear filters").is_some(), "toggling a category keeps the popover open");
    let shown = tiles(&harness);
    assert!(!shown.is_empty());
    for label in &shown {
        let kind = ShapeKind::free().into_iter().find(|k| k.label() == *label).unwrap();
        assert_eq!(Category::of(&kind), Some(Category::StarsAndBursts), "{label} is not a star or burst");
    }
    harness.get_by_label("Clear filters").click();
    harness.run();
    assert_eq!(tiles(&harness), all_tiles());
}

#[test]
fn a_search_with_no_match_offers_clear() {
    let mut harness = open_shapes();
    harness.get_by_label("Search shapes").type_text("zzqx");
    harness.run();
    assert!(tiles(&harness).is_empty());
    harness.get_by_label("No shapes match");
    harness.get_by_label("Clear").click();
    harness.run();
    assert_eq!(tiles(&harness), all_tiles());
    assert!(harness.query_by_label("No shapes match").is_none());
}

#[test]
fn search_and_filters_survive_collapsing_the_panel() {
    let mut harness = open_shapes();
    harness.get_by_label("Search shapes").type_text("star");
    harness.run();
    let narrowed = tiles(&harness);
    assert!(narrowed.len() < all_tiles().len());
    harness.get_by_label("Collapse Shapes").click();
    harness.run();
    assert!(!harness.state().shapes_open);
    harness.get_by_label("Shapes").click();
    harness.run();
    assert_eq!(tiles(&harness), narrowed);
    harness.get_by_label("Star").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 1, "a filtered tile still inserts its shape");
}

#[test]
fn resting_on_a_shape_tile_names_it_and_its_category() {
    let mut harness = open_shapes();
    assert!(harness.query_by_label("Stars and Bursts").is_none(), "no card before hovering");
    harness.get_by_label("Star").hover();
    for _ in 0..60 {
        harness.step();
    }
    harness.run();
    assert!(harness.query_by_label("Stars and Bursts").is_some(), "the card names the shape's category");
}
