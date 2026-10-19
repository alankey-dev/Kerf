use egui_kittest::kittest::{Node, Queryable};

use super::test_harness::{save_snapshot, snapshot_harness};
use crate::design::ShapeKind;
use crate::links::LinkKind;

fn top(n: Node<'_>) -> f64 {
    n.bounding_box().unwrap().y0
}

fn bottom(n: Node<'_>) -> f64 {
    n.bounding_box().unwrap().y1
}

#[test]
fn attach_in_the_bar_links_the_pair_and_turns_into_detach() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    for kind in [ShapeKind::Square, ShapeKind::Circle] {
        harness.state_mut().design.add_shape(kind);
    }
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    harness.get_by_label("Layers Attach").hover();
    for _ in 0..4 {
        harness.step();
    }
    save_snapshot(&mut harness, "11-layers-actions.png");

    harness.get_by_label("Layers Attach").click();
    harness.run();
    let links: Vec<_> = harness.state().design.shapes.iter().map(|s| s.link).collect();
    assert!(links.iter().all(|l| l.is_some_and(|l| l.kind == LinkKind::Attach)), "both shapes carry an Attach link");
    assert_eq!(links[0].map(|l| l.id), links[1].map(|l| l.id), "and it is the same link");
    assert!(harness.query_by_label("Layers Attach").is_none());
    harness.get_by_label("Layers Detach").click();
    harness.run();
    assert!(harness.state().design.shapes.iter().all(|s| s.link.is_none()), "Detach unlinks them again");
}

#[test]
fn one_shape_leaves_slice_and_combine_disabled() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.state_mut().design.add_shape(ShapeKind::Square);
    harness.state_mut().selection = vec![1];
    harness.run();
    assert!(harness.get_by_label("Layers Slice").is_disabled());
    assert!(harness.get_by_label("Layers Combine").is_disabled());
    assert!(harness.get_by_label("Layers Attach").is_disabled());
    harness.get_by_label("Layers Combine").click();
    harness.run();
    assert!(harness.query_by_label("Weld").is_none(), "a disabled Combine opens no popover");
}

#[test]
fn combine_in_the_bar_opens_its_popover_above_the_button() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    for _ in 0..2 {
        harness.state_mut().design.add_shape(ShapeKind::Square);
    }
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    harness.get_by_label("Layers Combine").click();
    harness.run();
    save_snapshot(&mut harness, "11-layers-combine.png");
    let button = top(harness.get_by_label("Layers Combine"));
    assert!(bottom(harness.get_by_label("Weld")) < button, "the popover sits above the bar");
    harness.get_by_label("Weld").click();
    harness.run();
    assert_eq!(
        harness.state().design.shapes[0].kind,
        crate::design::ShapeKind::Compound(crate::design::CompoundOp::Boolean(crate::design::Combine::Weld)),
        "Weld from the bar joins the pair live"
    );

    harness.state_mut().design.add_shape(ShapeKind::Circle);
    let ids: Vec<u64> = harness.state().design.shapes.iter().map(|s| s.id).collect();
    harness.state_mut().selection = ids.clone();
    harness.run();
    harness.get_by_label("Layers Combine").click();
    harness.run();
    assert!(harness.query_by_label("Weld").is_some());
    harness.state_mut().selection = vec![ids[0]];
    harness.run();
    harness.state_mut().selection = ids;
    harness.run();
    assert!(harness.query_by_label("Weld").is_none(), "a popover dropped by a shrinking selection stays closed");
}

#[test]
fn a_long_layers_list_scrolls_above_the_bar() {
    let size = egui::vec2(1400.0, 900.0);
    let mut harness = snapshot_harness(size);
    for _ in 0..30 {
        harness.state_mut().design.add_shape(ShapeKind::Square);
    }
    harness.state_mut().selection = vec![30];
    harness.run();
    save_snapshot(&mut harness, "11-layers-scroll.png");
    let bar_top = top(harness.get_by_label("Layers Slice"));
    assert!(bottom(harness.get_by_label("Layers Slice")) <= f64::from(size.y), "the bar is on screen");
    let rows = |h: &egui_kittest::Harness<'_, super::Kerf>| h.get_all_by_label("Layer Square").map(top).collect::<Vec<_>>();
    let before = rows(&harness);
    assert!(before.len() < 30 && before.iter().all(|&y| y < bar_top), "30 rows overflow the space above the bar, and only those in view are built");

    let over_list = egui::pos2(size.x - 120.0, f64::midpoint(before[0], bar_top) as f32);
    harness.input_mut().events.push(egui::Event::PointerMoved(over_list));
    harness.input_mut().events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -400.0), modifiers: egui::Modifiers::NONE });
    harness.run_steps(8);
    let after = rows(&harness);
    assert!(after[0] < before[0], "the list scrolled");
    assert_eq!(top(harness.get_by_label("Layers Slice")), bar_top, "the bar did not move");
}
