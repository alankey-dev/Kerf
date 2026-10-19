//! Spec 17: node editing (add, remove, convert, smooth) and node kinds from the Pen, driven through the app.

use egui::{Key, Modifiers, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::test_harness::{assert_snapshot, click_at, double_click_at, drag_along, key, move_to, snapshot_harness};
use super::{canvas, commands, node_edit};
use crate::design::{Contour, Operation, Pt, Seg};
use crate::path_edit::{NodeKind, NodePath};

const PX: f32 = 72.0;

/// Where the artboard's top-left corner sits on screen at 100 %, found by placing a probe text.
fn origin(harness: &mut Harness<'_, Kerf>) -> Pos2 {
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Text;
    let probe = pos2(600.0, 400.0);
    click_at(harness, probe);
    let text = harness.state().design.shapes[0].clone();
    harness.state_mut().design = Default::default();
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().selection.clear();
    harness.run();
    probe - vec2(text.x_in, text.y_in) * PX
}

fn screen(origin: Pos2, (x, y): Pt) -> Pos2 {
    origin + vec2(x, y) * PX
}

fn holding(harness: &mut Harness<'_, Kerf>, modifiers: Modifiers, act: impl FnOnce(&mut Harness<'_, Kerf>)) {
    harness.input_mut().modifiers = modifiers;
    act(harness);
    harness.input_mut().modifiers = Modifiers::NONE;
    harness.run();
}

fn path(harness: &Harness<'_, Kerf>) -> NodePath {
    let shapes = &harness.state().design.shapes;
    assert_eq!(shapes.len(), 1, "one path");
    NodePath::read(&shapes[0])
}

fn node_count(harness: &Harness<'_, Kerf>) -> usize {
    path(harness).contours[0].nodes.len()
}

fn outline(harness: &Harness<'_, Kerf>) -> Vec<Pt> {
    harness.state().design.shapes[0].world_polylines().remove(0).0
}

fn dist_to_polyline(p: Pt, line: &[Pt]) -> f32 {
    line.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = (dx * dx + dy * dy).max(1e-12);
            let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
            (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
        })
        .fold(f32::MAX, f32::min)
}

/// A Pen path through `pts` (world inches), finished with Enter and deselected.
fn pen_path(harness: &mut Harness<'_, Kerf>, o: Pos2, pts: &[Pt]) {
    harness.state_mut().tool = canvas::Tool::Pen;
    for &p in pts {
        click_at(harness, screen(o, p));
    }
    key(harness, Key::Enter, Modifiers::NONE);
    harness.state_mut().selection.clear();
    harness.run();
}

/// Four nodes along y = 5 whose two middle nodes are Cusps with unequal, bent handles.
fn wavy_path(harness: &mut Harness<'_, Kerf>) -> u64 {
    let c = Contour {
        start: (3.0, 5.0),
        segs: vec![Seg::Cubic((3.5, 4.5), (4.2, 4.4), (5.0, 5.0)), Seg::Cubic((5.3, 5.6), (6.4, 5.8), (7.0, 5.0)), Seg::Cubic((7.6, 4.6), (8.5, 4.5), (9.0, 5.0))],
        closed: false,
    };
    let id = harness.state_mut().design.add_path(c, egui::Color32::BLACK, Operation::Pen).unwrap();
    harness.state_mut().selection = vec![id];
    harness.run();
    id
}

#[test]
fn clicking_a_segment_adds_a_node_without_changing_the_outline_and_undo_removes_it() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    pen_path(&mut harness, o, &[(4.0, 5.0), (7.0, 5.0), (5.5, 7.0)]);
    double_click_at(&mut harness, screen(o, (5.0, 5.0)));
    assert!(harness.state().view.nodes.is_some(), "double-click edits the path's nodes");
    let (before, count) = (outline(&harness), node_count(&harness));

    move_to(&mut harness, screen(o, (6.0, 5.0)));
    assert_snapshot(&mut harness, "17-add-ghost");
    click_at(&mut harness, screen(o, (6.0, 5.0)));
    assert_eq!(node_count(&harness), count + 1, "a click on a segment adds a node");
    let after = outline(&harness);
    let worst = after.iter().map(|&p| dist_to_polyline(p, &before)).fold(0.0, f32::max);
    assert!(worst < 0.001, "the outline is unchanged, worst {worst}");

    key(&mut harness, Key::Z, Modifiers::COMMAND);
    assert_eq!(node_count(&harness), count, "one Undo removes the added node");
}

#[test]
fn marquee_two_nodes_then_symmetric_makes_their_handles_equal_and_opposite() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    wavy_path(&mut harness);
    key(&mut harness, Key::A, Modifiers::NONE);
    assert!(harness.state().view.nodes.is_some(), "A edits the selected path's nodes");

    drag_along(&mut harness, &[screen(o, (4.5, 3.5)), screen(o, (6.0, 5.0)), screen(o, (7.5, 6.5))]);
    let selected = harness.state().view.nodes.as_ref().unwrap().selected.clone();
    assert_eq!(selected.iter().map(|id| id.node).collect::<Vec<_>>(), vec![1, 2], "the marquee selects the two middle nodes");

    harness.get_by_label("Symmetric").click();
    harness.run();
    for n in &path(&harness).contours[0].nodes[1..3] {
        let (i, o) = (n.inn.unwrap(), n.out.unwrap());
        let gap = (i.0 + o.0 - 2.0 * n.pt.0).hypot(i.1 + o.1 - 2.0 * n.pt.1);
        assert!(gap < 1e-3, "handles mirror about the anchor: {n:?}");
        assert_eq!(n.kind, NodeKind::Symmetric);
    }
}

#[test]
fn option_click_removes_a_node() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    pen_path(&mut harness, o, &[(4.0, 5.0), (6.0, 5.0), (8.0, 5.0), (7.0, 7.0)]);
    double_click_at(&mut harness, screen(o, (5.0, 5.0)));
    holding(&mut harness, Modifiers::ALT, |h| click_at(h, screen(o, (6.0, 5.0))));
    assert_eq!(node_count(&harness), 3, "Option-click removes the node under the pointer");
}

#[test]
fn pen_option_drag_places_a_cusp() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    harness.state_mut().tool = canvas::Tool::Pen;
    click_at(&mut harness, screen(o, (3.0, 5.0)));
    holding(&mut harness, Modifiers::ALT, |h| drag_along(h, &[screen(o, (5.0, 5.0)), screen(o, (5.3, 4.6)), screen(o, (6.0, 4.0))]));
    click_at(&mut harness, screen(o, (7.0, 5.0)));
    key(&mut harness, Key::Enter, Modifiers::NONE);
    let kinds = path(&harness).contours[0].kinds();
    assert_eq!(kinds, vec![NodeKind::Straight, NodeKind::Cusp, NodeKind::Straight], "click, Option-drag, click");
}

#[test]
fn pen_drag_places_a_symmetric_node_that_editing_shows() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    harness.state_mut().tool = canvas::Tool::Pen;
    click_at(&mut harness, screen(o, (3.0, 5.0)));
    drag_along(&mut harness, &[screen(o, (5.0, 5.0)), screen(o, (5.5, 4.5)), screen(o, (6.0, 4.0))]);
    click_at(&mut harness, screen(o, (7.0, 5.0)));
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(path(&harness).contours[0].nodes[1].kind, NodeKind::Symmetric);
}

fn lit(harness: &Harness<'_, Kerf>, label: &str) -> bool {
    harness.get_by_label(label).toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn node_toolbar_highlights_a_shared_kind_and_none_when_mixed() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    pen_path(&mut harness, o, &[(3.0, 5.0), (5.0, 5.0), (7.0, 6.0), (9.0, 5.0)]);
    double_click_at(&mut harness, screen(o, (4.0, 5.0)));
    click_at(&mut harness, screen(o, (5.0, 5.0)));
    harness.get_by_label("Smooth").click();
    harness.run();
    assert_eq!(path(&harness).contours[0].nodes[1].kind, NodeKind::Smooth);
    assert!(lit(&harness, "Smooth"), "the converted node's kind is lit");
    holding(&mut harness, Modifiers::SHIFT, |h| click_at(h, screen(o, (7.0, 6.0))));
    for kind in [NodeKind::Straight, NodeKind::Cusp, NodeKind::Smooth, NodeKind::Symmetric] {
        assert!(!lit(&harness, kind.label()), "{kind:?} is not lit for a mixed selection");
    }
    assert_snapshot(&mut harness, "17-node-toolbar");
    assert!(harness.query_by_label("Flip").is_none(), "the node toolbar replaces the edit toolbar");
    harness.get_by_label("Done").click();
    harness.run();
    assert!(harness.state().view.nodes.is_none(), "Done leaves node editing");
}

#[test]
fn delete_is_disabled_below_the_minimum_node_count() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    pen_path(&mut harness, o, &[(3.0, 5.0), (6.0, 5.0)]);
    double_click_at(&mut harness, screen(o, (4.5, 5.0)));
    click_at(&mut harness, screen(o, (3.0, 5.0)));
    assert!(harness.get_by_label("Delete Node").is_disabled(), "a two-node line keeps both nodes");
    key(&mut harness, Key::Backspace, Modifiers::NONE);
    assert_eq!(node_count(&harness), 2);
}

#[test]
fn smoothing_a_pencil_stroke_previews_then_applies_in_one_step() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    harness.state_mut().tool = canvas::Tool::Pencil;
    let wobble = |i: usize| if i.is_multiple_of(2) { 0.03 } else { -0.03 };
    let stroke: Vec<Pos2> = (0..=60).map(|i| screen(o, (3.0 + i as f32 * 0.1, 5.0 + (i as f32 * 0.1).sin() * 0.6 + wobble(i)))).collect();
    drag_along(&mut harness, &stroke);
    key(&mut harness, Key::A, Modifiers::NONE);
    let (before, count) = (harness.state().design.clone(), node_count(&harness));

    harness.get_by_label("Smoothing").click();
    harness.run();
    assert_eq!(harness.state().design, before, "the preview leaves the design alone");
    assert_snapshot(&mut harness, "17-smooth");
    harness.get_by_label("Apply").click();
    harness.run();
    assert!(node_count(&harness) < count, "Smooth drops nodes: {count} -> {}", node_count(&harness));
    key(&mut harness, Key::Z, Modifiers::COMMAND);
    assert_eq!(harness.state().design, before, "one Undo restores the stroke");
}

#[test]
fn right_click_on_a_node_offers_node_actions() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let o = origin(&mut harness);
    pen_path(&mut harness, o, &[(3.0, 5.0), (5.0, 5.0), (7.0, 6.0)]);
    double_click_at(&mut harness, screen(o, (4.0, 5.0)));
    let at = screen(o, (5.0, 5.0));
    move_to(&mut harness, at);
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    harness.run();
    harness.get_all_by_label("Delete Node").last().unwrap().click();
    harness.run();
    assert_eq!(node_count(&harness), 2, "the menu's Delete Node removes the node right-clicked");
    assert!(harness.state().view.nodes.is_some(), "still editing nodes");
}

#[test]
fn node_shortcuts_do_not_clash() {
    // While nodes are edited the design-edit keys are off, so only app commands and tool keys stay live.
    let mut taken: Vec<egui::KeyboardShortcut> = commands::KEYED.iter().filter(|(c, _)| commands::stays_live(c)).map(|&(_, k)| k).collect();
    for key in [Key::V, Key::P, Key::N, Key::T] {
        taken.push(egui::KeyboardShortcut::new(Modifiers::NONE, key));
    }
    let mut seen: Vec<egui::KeyboardShortcut> = Vec::new();
    for (name, keys) in node_edit::SHORTCUTS {
        assert!(!seen.contains(&keys), "{name} repeats a node shortcut");
        seen.push(keys);
        assert!(!taken.contains(&keys), "{name} clashes with an app shortcut");
    }
    let (name, start) = node_edit::SHORTCUTS[0];
    assert!(!commands::KEYED.iter().any(|(_, k)| *k == start), "{name} clashes with a command on the canvas");
}
