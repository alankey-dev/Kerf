//! The editor's own behaviour tests: the Make flow end to end, canvas tools, menus, units, Home and the
//! chrome around them. Tests for one spec live in that spec's `tests_specNN.rs`; the shared harness is
//! `test_harness.rs`.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::test_harness::{
    Fixture, REFERENCE_CSS, any_near, assert_snapshot, bare, click_at, device_is, double_click_at, drag_along, frame, key, launch, look_at_snapshot, move_to, pixel,
    pump, rect_of, snapshot_harness, step_is, two_squares_apart,
};
use super::*;
use crate::device::DeviceState;
use crate::make_flow::{DonePhase, Outcome};

#[test]
fn clicks_through_make_it_with_the_simulated_machine() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));

    harness.get_by_label("Shapes").click();
    harness.step();
    harness.get_by_label("Square").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 1);
    harness.get_by_label("Make").click();
    harness.step();
    harness.step();
    assert!(harness.state().flow.is_some(), "Make starts the flow");
    harness.get_by_label("Material Load Type");
    harness.get_by_label("Continue").click();
    pump(&mut harness, "device list", |a| a.flow.as_ref().is_some_and(|f| !f.devices.is_empty()));

    harness.get_by_label("Select Machine");
    harness.get_all_by_label("Connect with USB").next().expect("My Machine USB button").click();
    pump(&mut harness, "connected", |a| step_is(a, MakeStep::ConnectMachine(crate::make_flow::ConnectPhase::Connected)));
    harness.get_by_label("Yay! Your machine is connected and ready to make!");
    harness.get_all_by_label("Continue").last().expect("modal Continue").click();
    pump(&mut harness, "Set step", |a| step_is(a, MakeStep::Set));

    harness.get_by_label("Browse All Materials").click();
    harness.step();
    harness.get_by_label("Cardstock  (Fine-Point Blade, Clamp B)").click();
    harness.get_by_label("More").click();
    pump(&mut harness, "waiting on material", |a| device_is(a, DeviceState::WaitingOnMaterial));
    harness.get_by_label("Next").click();
    pump(&mut harness, "waiting on load", |a| device_is(a, DeviceState::WaitingOnLoad));
    assert!(harness.state().last_sent.as_ref().is_some_and(|l| l.json.contains("M0 0 L144 0 L144 144 L0 144 Z")));

    harness.get_by_label("Load Fine-Point Blade in Clamp B");
    harness.get_by_label("Load/Unload").click();
    pump(&mut harness, "Go step", |a| step_is(a, MakeStep::Go));
    harness.get_by_label("Go").click();
    let checking = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Checking };
    pump(&mut harness, "cut complete", |a| step_is(a, checking));

    harness.get_by_label("Check your cut");
    harness.get_by_label("Looks good").click();
    let unloading = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloading };
    pump(&mut harness, "unloading", |a| step_is(a, unloading) && device_is(a, DeviceState::WaitingOnUnload));
    harness.get_by_label("Unload mat").click();
    let unloaded = MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloaded };
    pump(&mut harness, "unloaded", |a| step_is(a, unloaded));
    harness.get_by_label("Back to canvas").click();
    harness.step();
    assert!(harness.state().flow.is_none());
    assert!(harness.state().machine.is_some(), "machine is remembered for the next Make");
}

#[test]
fn renders_the_canvas_snapshot() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    assert_snapshot(&mut harness, "canvas");

    harness.get_by_label("Zoom in").click();
    harness.run();
    assert_eq!(harness.state().view.zoom_pct(), 125);
    harness.get_by_label("Zoom out").click();
    harness.run();
    harness.get_by_label("Zoom out").click();
    harness.run();
    assert_eq!(harness.state().view.zoom_pct(), 75);
}

#[test]
fn renders_the_editor_and_operation_menu() {
    let mut harness = snapshot_harness(REFERENCE_CSS);
    look_at_snapshot(&mut harness, "reference-1-blank.png");

    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    assert!(!harness.state().selection.is_empty());
    harness.get_by_label("Basic Cut").click();
    harness.run();
    look_at_snapshot(&mut harness, "reference-2-shapes.png");

    harness.get_by_label("Foil").click();
    harness.run();
    harness.get_by_label("Medium").click();
    harness.run();
    let op = harness.state().design.shapes[0].operation;
    assert_eq!(op, crate::design::Operation::Foil(crate::design::FoilWeight::Medium));
}

#[test]
fn renders_the_editor_with_handles() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    harness.get_by_label("Shapes").click(); // close the panel so the shape is in the open
    harness.run();
    assert_eq!(harness.state().selection.len(), 1);
    assert_snapshot(&mut harness, "editor");

    harness.get_by_label("Layer Square");
}

fn two_overlapping_squares(harness: &mut Harness<'_, Kerf>) {
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    harness.get_by_label("Shapes").click();
    harness.run();
    let ids: Vec<u64> = harness.state().design.shapes.iter().map(|s| s.id).collect();
    harness.state_mut().design.shapes[1].x_in = harness.state().design.shapes[0].x_in + 1.0;
    harness.state_mut().design.shapes[1].y_in = harness.state().design.shapes[0].y_in;
    harness.state_mut().selection = ids;
    harness.run();
}

#[test]
fn renders_combine_and_welds() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    two_overlapping_squares(&mut harness);
    harness.get_by_label("Combine").click();
    harness.run();
    harness.get_by_label("Weld").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 3, "a live weld group holding both squares");
    assert_eq!(harness.state().design.shapes[0].kind, crate::design::ShapeKind::Compound(crate::design::CompoundOp::Boolean(crate::design::Combine::Weld)));
    assert_eq!(harness.state().selection, [harness.state().design.shapes[0].id], "the group is selected");
    assert_snapshot(&mut harness, "welded");
}

#[test]
fn renders_offset_effect() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    two_overlapping_squares(&mut harness);
    harness.state_mut().selection = vec![harness.state().design.shapes[0].id];
    harness.run();
    harness.get_by_label("Effects").click();
    harness.run();
    harness.get_by_label("Offset").click();
    harness.run();
    harness.get_by_label("Apply").click();
    harness.run();
    let design = &harness.state().design;
    assert_eq!(design.shapes.len(), 2, "the offset is an effect, not a new shape");
    assert_eq!(design.geometry(&design.shapes[0]).pieces().len(), 2, "drawn behind the square");
    assert_snapshot(&mut harness, "offset");
}

#[test]
fn renders_text_object() {
    use crate::text::FontRef;
    let mut harness = snapshot_harness(egui::vec2(1200.0, 700.0));
    let id = harness.state_mut().design.add_text("Hello".into(), FontRef::SpaceGrotesk, 1.3, egui::Color32::from_rgb(0x6a, 0x2c, 0xc2), 2.0, 2.5);
    harness.state_mut().selection = vec![id];
    harness.state_mut().tool = canvas::Tool::Text;
    harness.run();
    assert!(matches!(harness.state().design.shapes[0].kind, crate::design::ShapeKind::Text { .. }));
    assert_eq!(harness.state().design.shapes[0].operation, crate::design::Operation::Pen);
    assert_snapshot(&mut harness, "text");
}

#[test]
fn renders_multiline_centred_text_in_a_stable_box() {
    use crate::design::ShapeKind;
    use crate::text::{FontStyle, TextAlign};
    let mut harness = snapshot_harness(egui::vec2(1200.0, 700.0));
    let id = harness.state_mut().design.add_text(String::new(), crate::text::FontRef::Inter, 1.0, egui::Color32::BLACK, 2.0, 2.0);
    let before = harness.state().design.shapes[0].kind.clone();
    if let Some(s) = harness.state_mut().design.shape_mut(id)
        && let ShapeKind::Text { content, style, align, line_space, .. } = &mut s.kind
    {
        *content = "AVATAR\ngyp jig\nace".into();
        *style = FontStyle::BoldItalic;
        *align = TextAlign::Centre;
        *line_space = 200.0;
    }
    harness.state_mut().design.refit_text(id, &before);
    harness.state_mut().selection = vec![id];
    harness.run();
    let shape = &harness.state().design.shapes[0];
    let natural = shape.kind.text_layout().unwrap().size;
    assert!((shape.height_in - natural.1).abs() < 1e-4, "box follows the three-line layout");
    assert_snapshot(&mut harness, "08-text-multiline");
}

#[test]
fn renders_drawn_paths() {
    use crate::design::Operation;
    let mut harness = snapshot_harness(egui::vec2(1200.0, 760.0));
    // A smooth pencil stroke: raw points simplified and fitted, as the Pencil tool does.
    let raw: Vec<(f32, f32)> = (0..40).map(|i| i as f32 * 0.1).map(|t| (1.0 + t, 2.0 + (t * 1.5).sin() * 0.8)).collect();
    let simplified = crate::geometry::rdp(&raw, 0.02);
    let pencil = crate::geometry::smooth_open_path(&simplified);
    harness.state_mut().design.add_path(pencil, egui::Color32::from_rgb(0x6a, 0x2c, 0xc2), Operation::Pen);
    // A pen path: a corner, a line, then a curve, left open.
    let pen = crate::design::Contour {
        start: (1.0, 5.0),
        segs: vec![crate::design::Seg::Line((3.0, 5.0)), crate::design::Seg::Cubic((4.0, 4.0), (5.0, 6.5), (6.0, 5.5))],
        closed: false,
    };
    harness.state_mut().design.add_path(pen, egui::Color32::BLACK, Operation::Pen);
    harness.state_mut().tool = canvas::Tool::Pen;
    harness.run();
    assert_snapshot(&mut harness, "drawn-paths");
    assert_eq!(harness.state().design.shapes.len(), 2);
    assert!(harness.state().design.shapes.iter().all(|s| s.operation == Operation::Pen));
}

#[test]
fn in_progress_pen_anchors_use_the_handle_treatment() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 760.0));
    harness.state_mut().tool = canvas::Tool::Pen;
    harness.state_mut().view.begin_pen_for_test(&[((2.0, 2.0), None), ((4.0, 3.0), Some((0.8, -0.6))), ((6.0, 2.0), None)]);
    let img = frame(&mut harness);
    let solid_accent = img.pixels().filter(|p| p.0[..3] == [0x70, 0x48, 0xe8]).count();
    let white_in_accent = img.pixels().filter(|p| p.0[..3] == [0xff, 0xff, 0xff]).count();
    assert!(solid_accent > 0 && white_in_accent > 0);
}

#[test]
fn top_bar_menus_reach_home_and_hold_the_title() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 400.0));
    assert_eq!(harness.state().title, "Untitled-1");
    for label in ["File", "Edit", "View", "Preview", "Make", "Export"] {
        harness.get_by_label(label);
    }
    harness.get_by_label("File").click();
    harness.run();
    for label in ["New", "Save", "Export...", "Prepare for Design Space"] {
        harness.get_by_label(label);
    }
    harness.get_by_label("Home").click();
    harness.run();
    assert!(harness.state().tab == chrome::Tab::Home);
}

#[test]
fn placing_with_a_tool_returns_to_select() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(&mut harness, egui::pos2(600.0, 400.0));
    assert_eq!(harness.state().design.shapes.len(), 1, "text placed");
    assert!(harness.state().tool == canvas::Tool::Select, "Text tool reverts to Select");

    harness.state_mut().tool = canvas::Tool::Pencil;
    drag_along(&mut harness, &[egui::pos2(500.0, 300.0), egui::pos2(540.0, 320.0), egui::pos2(580.0, 300.0), egui::pos2(620.0, 330.0)]);
    assert_eq!(harness.state().design.shapes.len(), 2, "pencil stroke placed");
    assert!(harness.state().tool == canvas::Tool::Select, "Pencil reverts to Select");

    harness.state_mut().tool = canvas::Tool::Pen;
    click_at(&mut harness, egui::pos2(450.0, 500.0));
    click_at(&mut harness, egui::pos2(520.0, 520.0));
    assert!(harness.state().tool == canvas::Tool::Pen, "Pen stays while anchors are placed");
    harness.input_mut().events.push(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 3, "pen path placed");
    assert!(harness.state().tool == canvas::Tool::Select, "Pen reverts to Select");
}

#[test]
fn properties_only_show_with_a_selection() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    assert!(harness.query_by_label("Properties").is_none(), "empty canvas hides Properties");
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(&mut harness, egui::pos2(600.0, 400.0));
    harness.run();
    assert!(harness.query_by_label("Properties").is_some(), "a selection shows Properties");
}

fn only_contour(harness: &Harness<'_, Kerf>) -> crate::design::Contour {
    let shapes = &harness.state().design.shapes;
    assert_eq!(shapes.len(), 1);
    shapes[0].contours().remove(0)
}

#[test]
fn pen_closes_by_clicking_its_first_anchor() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    harness.state_mut().tool = canvas::Tool::Pen;
    for p in [egui::pos2(451.0, 503.0), egui::pos2(560.0, 500.0), egui::pos2(500.0, 580.0)] {
        click_at(&mut harness, p);
    }
    click_at(&mut harness, egui::pos2(451.0, 503.0));
    assert!(only_contour(&harness).closed, "a click on the first anchor closes the path");
    assert!(harness.state().tool == canvas::Tool::Select);
}

#[test]
fn pen_closes_when_the_closing_click_wobbles_into_a_drag() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    harness.state_mut().tool = canvas::Tool::Pen;
    for p in [egui::pos2(451.0, 503.0), egui::pos2(560.0, 500.0), egui::pos2(500.0, 580.0)] {
        click_at(&mut harness, p);
    }
    drag_along(&mut harness, &[egui::pos2(452.0, 503.0), egui::pos2(458.0, 507.0), egui::pos2(462.0, 509.0)]);
    let c = only_contour(&harness);
    assert!(c.closed, "pressing on the first anchor closes even if the mouse moves");
    assert_eq!(c.segs.len(), 3, "no extra anchor at the start");
}

fn draw_open_pen(harness: &mut Harness<'_, Kerf>) {
    harness.state_mut().tool = canvas::Tool::Pen;
    for p in [egui::pos2(450.0, 500.0), egui::pos2(560.0, 500.0), egui::pos2(500.0, 580.0)] {
        click_at(harness, p);
    }
    key(harness, egui::Key::Enter, egui::Modifiers::NONE);
    harness.state_mut().selection.clear();
    harness.run();
}

#[test]
fn double_clicking_a_pen_path_edits_its_nodes() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    draw_open_pen(&mut harness);
    let before = harness.state().design.shapes[0].world_bounds();
    double_click_at(&mut harness, egui::pos2(505.0, 500.0));
    drag_along(&mut harness, &[egui::pos2(560.0, 500.0), egui::pos2(580.0, 480.0), egui::pos2(596.0, 464.0)]);
    let after = harness.state().design.shapes[0].world_bounds();
    let moved = 36.0 / 72.0;
    assert!((after.2 - before.2 - moved).abs() < 0.02, "the grabbed node moved right: {before:?} -> {after:?}");
    assert!((after.0 - before.0).abs() < 0.001, "the other nodes stay put");
    assert_eq!(harness.state().design.shapes.len(), 1, "editing nodes does not move the shape");

    key(&mut harness, egui::Key::Z, egui::Modifiers::COMMAND);
    let undone = harness.state().design.shapes[0].world_bounds();
    assert!((undone.2 - before.2).abs() < 0.001, "one undo restores the node");
}

#[test]
fn double_clicking_a_library_shape_edits_its_nodes_and_converts_on_the_first_edit() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let design = &mut harness.state_mut().design;
    let id = design.add_shape(crate::design::ShapeKind::Rectangle);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (5.0, 4.0, 3.0, 2.0);
    let original = s.clone();
    harness.run();
    let to_screen = |(x, y): (f32, f32)| egui::pos2(84.0, 121.0) + egui::vec2(x, y) * 72.0;
    let before = harness.state().design.shapes[0].world_bounds();
    double_click_at(&mut harness, to_screen((6.5, 5.0)));
    assert!(harness.state().view.nodes.as_ref().is_some_and(|e| e.shape == id), "double-click edits the shape's nodes");
    assert_eq!(harness.state().design.shapes[0], original, "entering node editing leaves the shape alone");

    drag_along(&mut harness, &[to_screen((8.0, 6.0)), to_screen((8.25, 6.0)), to_screen((8.5, 6.0))]);
    let edited = &harness.state().design.shapes[0];
    assert!(matches!(edited.kind, crate::design::ShapeKind::Path { .. }), "the first edit makes it a path");
    assert_eq!(edited.name.as_deref(), Some("Rectangle"), "it keeps its name in Layers");
    assert_eq!((edited.operation, edited.fill), (original.operation, original.fill));
    let after = edited.world_bounds();
    assert!((after.2 - before.2 - 0.5).abs() < 0.02, "the grabbed corner moved right: {before:?} -> {after:?}");
    assert!((after.0 - before.0).abs() < 0.001 && (after.1 - before.1).abs() < 0.001, "the other corners stay put");

    key(&mut harness, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(harness.state().design.shapes[0], original, "one undo restores the rectangle");
}

#[test]
fn node_editing_shows_curve_handles() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    let curve = crate::design::Contour {
        start: (5.0, 5.0),
        segs: vec![crate::design::Seg::Cubic((5.5, 4.0), (6.5, 4.0), (7.0, 5.0)), crate::design::Seg::Cubic((7.5, 6.0), (8.5, 6.0), (9.0, 5.0))],
        closed: false,
    };
    let id = harness.state_mut().design.add_path(curve, egui::Color32::BLACK, crate::design::Operation::Pen).unwrap();
    harness.state_mut().selection = vec![id];
    harness.run();
    let to_screen = |(x, y): (f32, f32)| egui::pos2(84.0, 121.0) + egui::vec2(x, y) * 72.0;
    double_click_at(&mut harness, to_screen((6.0, 4.25)));
    assert!(harness.state().view.nodes.as_ref().is_some_and(|e| e.shape == id), "double-click edits the path's nodes");
    let handles = [(5.5, 4.0), (6.5, 4.0), (7.5, 6.0), (8.5, 6.0)].map(|p| egui::Rect::from_center_size(to_screen(p), egui::Vec2::splat(10.0)));
    let image = harness.render().expect("wgpu render");
    assert!(handles.iter().all(|h| !any_near(&image, *h, tokens::ACCENT_PRIMARY, 24)), "no handles until a node is picked");

    click_at(&mut harness, to_screen((7.0, 5.0)));
    move_to(&mut harness, to_screen((7.0, 8.0)));
    assert_eq!(harness.state().view.nodes.as_ref().map(|e| e.selected.len()), Some(1), "the middle node is picked");
    let image = assert_snapshot(&mut harness, "node-edit-curve");
    for (h, at) in handles.iter().zip(["first out", "middle in", "middle out", "last in"]) {
        assert!(any_near(&image, *h, tokens::ACCENT_PRIMARY, 24), "the {at} handle is drawn at {h:?}");
    }
}

#[test]
fn deleting_a_selected_node_keeps_the_path() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    draw_open_pen(&mut harness);
    double_click_at(&mut harness, egui::pos2(505.0, 500.0));
    click_at(&mut harness, egui::pos2(500.0, 580.0));
    key(&mut harness, egui::Key::Backspace, egui::Modifiers::NONE);
    assert_eq!(only_contour(&harness).segs.len(), 1, "one node removed, the path remains");
    key(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    key(&mut harness, egui::Key::Backspace, egui::Modifiers::NONE);
    assert_eq!(harness.state().design.shapes.len(), 0, "after Escape the path is selected as a whole again");
}

#[test]
fn text_drags_from_the_gap_between_its_letters() {
    let mut harness = snapshot_harness(egui::vec2(1200.0, 800.0));
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(&mut harness, egui::pos2(600.0, 400.0));
    key(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    let before = harness.state().design.shapes[0].clone();
    harness.state_mut().selection.clear();
    harness.run();
    // Above the lower-case x, below the top of the box: inside the text's frame but on no glyph.
    let click = egui::pos2(600.0, 400.0);
    let px = 72.0 * harness.state().view.zoom;
    let start = click + egui::vec2(before.width_in * 0.62, before.height_in * 0.12) * px;
    drag_along(&mut harness, &[start, start + egui::vec2(10.0, 0.0), start + egui::vec2(60.0, 40.0)]);
    let after = &harness.state().design.shapes[0];
    assert!(after.x_in > before.x_in + 0.2, "text moved right: {} -> {}", before.x_in, after.x_in);
}

#[test]
fn rail_icons_explain_themselves_on_hover() {
    let mut harness = snapshot_harness(egui::vec2(900.0, 700.0));
    let pencil = rect_of(&harness, "Pencil");
    harness.get_by_label("Pencil").hover();
    for _ in 0..60 {
        harness.step();
    }
    harness.run();
    let body = rect_of(&harness, "Draw freehand. The stroke is smoothed into a curve.");
    assert!(body.left() > pencil.right() && body.top() < pencil.center().y, "the card opens beside the icon: {body:?} {pencil:?}");
    let image = assert_snapshot(&mut harness, "rail-tooltip");
    assert_eq!(super::test_harness::rgb(&image, egui::pos2(body.left() - 6.0, body.center().y)), [0xff; 3], "the card is white");
    assert!(any_near(&image, body, tokens::TEXT_MUTED, 40), "the body text is drawn");

    move_to(&mut harness, egui::pos2(500.0, 500.0));
    assert!(harness.query_by_label("Draw freehand. The stroke is smoothed into a curve.").is_none(), "the card closes when the pointer leaves");
}

#[test]
fn rail_selects_tools_and_the_flyout_inserts_shapes() {
    let mut harness = snapshot_harness(egui::vec2(900.0, 700.0));
    for (label, tool) in [("Pen", canvas::Tool::Pen), ("Pencil", canvas::Tool::Pencil), ("Text", canvas::Tool::Text), ("Select", canvas::Tool::Select)] {
        harness.get_by_label(label).click();
        harness.run();
        assert!(harness.state().tool == tool, "{label}");
    }
    harness.get_by_label("Pen").click();
    harness.run();
    harness.get_by_label("Shapes").click();
    harness.run();
    assert!(harness.state().shapes_open);
    harness.get_by_label("Heart").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 1);
}

#[test]
fn inspector_fields_edit_the_selection_on_enter() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    harness.get_by_label("Shapes").click();
    harness.run();
    let width = harness.get_by_label("W");
    width.focus();
    harness.run();
    harness.get_by_label("W").type_text("3");
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    let shape = &harness.state().design.shapes[0];
    assert_eq!(shape.width_in, 3.0);
    harness.get_by_label("Rotation").focus();
    harness.run();
    harness.get_by_label("Rotation").type_text("45");
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().design.shapes[0].rotation_deg, 45.0);
}

#[test]
fn project_preview_shows_only_during_make_and_as_a_floating_card() {
    let mut harness = snapshot_harness(egui::vec2(1500.0, 800.0));
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Heart").click();
    harness.run();
    assert!(harness.query_by_label("Close Project Preview").is_none(), "no permanent preview in the editor");
    harness.get_by_label("Preview").click();
    harness.run();
    assert!(harness.query_by_label("Close Project Preview").is_some());
    harness.get_by_label("Close Project Preview").click();
    harness.run();
    assert!(!harness.state().preview_open);
    harness.get_by_label("Make").click();
    harness.run_steps(4);
    assert!(harness.state().flow.is_some());
}

#[test]
fn the_artboard_floats_over_the_void_and_follows_the_mat() {
    use crate::design::ShapeKind;
    let mut harness = snapshot_harness(egui::vec2(1500.0, 1000.0));
    harness.run();
    assert_eq!(harness.state().view.artboard_in, (12.0, 12.0));
    let design = &mut harness.state_mut().design;
    for (kind, x, y) in [(ShapeKind::Star, 1.0, 1.0), (ShapeKind::Heart, 5.0, 2.0), (ShapeKind::Hexagon, 3.0, 6.0)] {
        let id = design.add_shape(kind);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, 3.0, 3.0);
    }
    harness.run();
    assert_snapshot(&mut harness, "artboard");
    harness.state_mut().mat = MachineFamily::Mini.preferred_mat();
    harness.run();
    assert_eq!(harness.state().view.artboard_in, (4.5, 6.5));
    assert_snapshot(&mut harness, "artboard-joy");
}

/// Renders one selected black square and returns its black bounding box and the image.
fn selected_square(zoom: f32) -> (image::RgbaImage, (u32, u32, u32, u32)) {
    use crate::design::ShapeKind;
    let mut harness = snapshot_harness(egui::vec2(1500.0, 1000.0));
    let id = harness.state_mut().design.add_shape(ShapeKind::Square);
    let s = harness.state_mut().design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    harness.state_mut().selection = vec![id];
    harness.state_mut().view.zoom = zoom;
    let img = frame(&mut harness);
    let black = |p: &image::Rgba<u8>| p.0[..3] == [0, 0, 0];
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, _) in img.enumerate_pixels().filter(|(x, y, p)| *x < 1250 && (90..900).contains(y) && black(p)) {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (img, (x0, y0, x1, y1))
}

fn accent_pixels_near(img: &image::RgbaImage, (cx, cy): (u32, u32), half: u32) -> usize {
    let near = |p: &image::Rgba<u8>| p.0[..3].iter().zip([0x70, 0x48, 0xe8]).all(|(a, b): (&u8, i32)| (*a as i32 - b).abs() < 24);
    (cy - half..=cy + half).flat_map(|y| (cx - half..=cx + half).map(move |x| (x, y))).filter(|&(x, y)| near(img.get_pixel(x, y))).count()
}

#[test]
fn selection_frame_is_a_one_pixel_outline_with_constant_size_handles() {
    let mut anchor_counts = Vec::new();
    for zoom in [1.0, 2.0] {
        let (img, (x0, y0, x1, y1)) = selected_square(zoom);
        let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
        assert_eq!(img.get_pixel(cx + 15, cy + 15).0[..3], [0, 0, 0], "no tint inside the frame, clear of the centre cross, at {zoom}");
        let ry = y0 + (y1 - y0) / 4;
        let edge = img.get_pixel(x0 - 1, ry).0;
        assert!(edge[2] > 0xc0 && edge[0] < 0x90, "a 1px accent frame hugs the left edge at {zoom}: {edge:?}");
        let outside = img.get_pixel(x0 - 3, ry).0;
        assert!(outside[0] > 0xe0, "nothing but the frame outside it at {zoom}: {outside:?}");
        anchor_counts.push(accent_pixels_near(&img, (x0, y0), 9));
    }
    let diff = anchor_counts[0].abs_diff(anchor_counts[1]);
    assert!(diff <= 6, "handle size does not scale with zoom: {anchor_counts:?}");
    assert!(anchor_counts[0] > 8, "a 6 px handle is drawn at the corner: {anchor_counts:?}");
}

#[test]
fn view_menu_sets_units_and_grid() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 500.0));
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label("Centimetres").click();
    harness.run();
    assert_eq!(harness.state().units, Units::Metric);
    harness.get_by_label("Major lines").click();
    harness.run();
    assert_eq!(harness.state().design.grid.map(|g| g.visible), Some(crate::grid::GridVisibility::Major));
}

/// Three saved projects in the fixture's own directory, listed on the Home tab.
fn home_with_projects() -> Fixture {
    use crate::design::ShapeKind;
    use crate::project::{self, Project};
    let mut harness = snapshot_harness(egui::vec2(1200.0, 820.0));
    let make = |kinds: &[ShapeKind]| {
        let mut d = Design::default();
        for k in kinds {
            d.add_shape(k.clone());
        }
        d
    };
    let projects = [
        ("Logo Draft", make(&[ShapeKind::Star, ShapeKind::Circle])),
        ("Gift Tags", make(&[ShapeKind::Square, ShapeKind::Heart])),
        ("Poster", make(&[ShapeKind::Hexagon])),
    ];
    let newest = project::now();
    for (age, (name, d)) in (0..).zip(projects) {
        let p = Project { name: name.into(), ..Project::new(d) };
        project::save(&harness.projects, &Project { modified: newest - age, ..p }).unwrap();
    }
    harness.state_mut().tab = chrome::Tab::Home;
    harness.state_mut().home_projects = project::list(&harness.projects);
    harness.run();
    harness
}

#[test]
fn renders_home_with_projects() {
    let mut harness = home_with_projects();
    let image = assert_snapshot(&mut harness, "home");
    let mut previous = f32::MIN;
    for name in ["Logo Draft", "Gift Tags", "Poster"] {
        let trash = rect_of(&harness, &format!("Delete {name}"));
        assert!(trash.left() > previous, "{name} follows the card before it");
        previous = trash.left();
        let thumb = egui::Rect::from_min_max(egui::pos2(trash.right() - 210.0, 0.0), egui::pos2(trash.right(), trash.top() - 40.0));
        assert!(any_near(&image, thumb, egui::Color32::BLACK, 16), "{name}'s thumbnail shows its shapes");
    }
}

#[test]
fn home_delete_asks_first_then_removes_both_files() {
    use crate::project;
    let mut harness = home_with_projects();
    let poster = harness.state().home_projects.iter().filter_map(project::Entry::project).find(|p| p.name == "Poster").unwrap().id.clone();
    let dir = harness.projects.clone();

    harness.get_by_label("Delete Poster").click();
    harness.run();
    assert!(harness.state().home_confirm.is_some());
    harness.get_by_label("Cancel delete").click();
    harness.run();
    assert!(project::json_path(&dir, &poster).exists(), "cancel keeps the project");

    harness.get_by_label("Delete Poster").click();
    harness.run();
    harness.get_by_label("Confirm delete").click();
    harness.run();
    assert!(!project::json_path(&dir, &poster).exists());
    assert!(!project::thumb_path(&dir, &poster).exists());
    let names: Vec<_> = harness.state().home_projects.iter().filter_map(project::Entry::project).map(|p| p.name.clone()).collect();
    assert_eq!(names.len(), 2);
    assert!(!names.contains(&"Poster".to_owned()));
    assert!(harness.query_by_label("Delete Poster").is_none(), "its card is gone");
}

#[test]
fn deleting_the_open_project_blanks_the_canvas_and_clear_all_empties_home() {
    use crate::project;
    let mut harness = home_with_projects();
    let open = harness.state().home_projects[0].project().unwrap().clone();
    harness.state_mut().design = open.design.clone();
    harness.state_mut().project = Some(open.clone());
    harness.state_mut().title = open.name.clone();

    harness.get_by_label(&format!("Delete {}", open.name)).click();
    harness.run();
    harness.get_by_label("Confirm delete").click();
    harness.run();
    assert!(harness.state().design.shapes.is_empty(), "back to a blank canvas");
    assert!(harness.state().project.is_none());
    assert_eq!(harness.state().title, "Untitled-1");

    harness.get_by_label("Clear all projects").click();
    harness.run();
    assert_eq!(harness.state().home_confirm, Some(home::Confirm::All));
    harness.get_by_label("Confirm delete").click();
    harness.run();
    assert!(harness.state().home_projects.is_empty());
    assert!(project::list(&harness.projects).is_empty());
    let left: Vec<_> = std::fs::read_dir(&harness.projects).unwrap().flatten().map(|e| e.file_name()).filter(|n| n != "session.json").collect();
    assert_eq!(left, Vec::<std::ffi::OsString>::new());
}

#[test]
fn renders_contour_panel_and_hides_a_hole() {
    use crate::design::{Contour, Operation, Seg, Shape, ShapeKind};
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let line = |pts: &[(f32, f32)]| Contour { start: pts[0], segs: pts[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true };
    let outer = line(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
    let inner = line(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)]);
    let shape = Shape {
        x_in: 2.0,
        y_in: 2.0,
        width_in: 4.0,
        height_in: 4.0,
        fill: egui::Color32::from_rgb(0x2e, 0x86, 0x6b),
        operation: Operation::BasicCut,
        pen: Default::default(),
        ..Shape::new(1, ShapeKind::Imported { contours: vec![outer, inner], view_box: (4.0, 4.0) })
    };
    harness.state_mut().design.shapes.push(shape);
    harness.state_mut().selection = vec![1];
    harness.state_mut().contour_open = true;
    let image = assert_snapshot(&mut harness, "contour");
    let green = [0x2e, 0x86, 0x6b];
    let is_green = |x: u32, y: u32| image.get_pixel(x, y).0[..3] == green;
    let rows: Vec<u32> = (0..image.height()).filter(|&y| (0..image.width()).filter(|&x| is_green(x, y)).count() > 100).collect();
    let cols: Vec<u32> = (0..image.width()).filter(|&x| (0..image.height()).filter(|&y| is_green(x, y)).count() > 100).collect();
    let (left, right, top, bottom) = (cols[0] as f32, cols[cols.len() - 1] as f32, rows[0] as f32, rows[rows.len() - 1] as f32);
    assert!((right - left - 4.0 * 72.0).abs() < 3.0 && (bottom - top - 4.0 * 72.0).abs() < 3.0, "the 4 in square spans ({left}, {top}) to ({right}, {bottom})");
    let hole = egui::pos2((left + right) / 2.0 + 30.0, (top + bottom) / 2.0 + 30.0);
    let ring = egui::pos2(left + 36.0, hole.y);
    assert_eq!(super::test_harness::rgb(&image, ring), green, "the ring is filled");
    assert_eq!(super::test_harness::rgb(&image, hole), bare(&mut harness, hole), "the hole shows the artboard");

    harness.state_mut().design.shapes[0].contours.hidden = vec![false, true];
    let image = assert_snapshot(&mut harness, "contour-filled");
    assert_eq!(super::test_harness::rgb(&image, hole), green, "hiding the inner contour fills the hole");
    assert_eq!(pixel(&mut harness, ring), green);
}

fn click_with(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2, modifiers: egui::Modifiers) {
    move_to(harness, pos);
    harness.input_mut().modifiers = modifiers;
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers });
        harness.run();
    }
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    harness.run();
}

fn links(harness: &Harness<'_, Kerf>) -> Vec<Option<crate::links::Link>> {
    harness.state().design.shapes.iter().map(|s| s.link).collect()
}

#[test]
fn grouped_shapes_select_and_drag_together() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    key(&mut harness, egui::Key::G, egui::Modifiers::COMMAND);
    assert!(links(&harness).iter().all(|l| l.is_some_and(|l| l.kind == LinkKind::Group)), "the group shortcut groups both");

    harness.state_mut().selection.clear();
    harness.run();
    click_at(&mut harness, at((2.0, 2.0)));
    assert_eq!(harness.state().selection, vec![1, 2], "clicking one member selects the group");

    harness.state_mut().selection.clear();
    harness.run();
    let before: Vec<(f32, f32)> = harness.state().design.shapes.iter().map(|s| (s.x_in, s.y_in)).collect();
    drag_along(&mut harness, &[at((5.0, 2.0)), at((5.1, 2.0)), at((5.5, 2.5))]);
    let moved: Vec<(f32, f32)> = harness.state().design.shapes.iter().zip(&before).map(|(s, b)| (s.x_in - b.0, s.y_in - b.1)).collect();
    assert!(moved[0].0 > 0.3 && moved[0].1 > 0.3, "the dragged member moved {moved:?}");
    assert_eq!(moved[0], moved[1], "both members moved by the same amount");
}

#[test]
fn cmd_click_selects_one_group_member() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    harness.state_mut().design.group(&[1, 2]);
    harness.run();
    click_with(&mut harness, at((5.0, 2.0)), egui::Modifiers::COMMAND);
    assert_eq!(harness.state().selection, vec![2], "command-click picks just that member");
    click_at(&mut harness, at((5.0, 2.0)));
    assert_eq!(harness.state().selection, vec![1, 2], "a plain click selects the whole group again");
}

#[test]
fn attach_then_detach_leaves_no_links() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    key(&mut harness, egui::Key::A, egui::Modifiers::ALT);
    assert!(links(&harness).iter().all(|l| l.is_some_and(|l| l.kind == LinkKind::Attach)), "Option+A attaches");
    key(&mut harness, egui::Key::A, egui::Modifiers::ALT | egui::Modifiers::SHIFT);
    assert_eq!(links(&harness), vec![None, None], "Option+Shift+A detaches");
}

#[test]
fn undo_reverts_each_link_command_in_one_step() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    let steps = [
        (egui::Key::G, egui::Modifiers::COMMAND),
        (egui::Key::G, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
        (egui::Key::A, egui::Modifiers::ALT),
        (egui::Key::A, egui::Modifiers::ALT | egui::Modifiers::SHIFT),
    ];
    for (k, m) in steps {
        let before = links(&harness);
        key(&mut harness, k, m);
        assert_ne!(links(&harness), before, "{k:?} {m:?} changed the links");
        key(&mut harness, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(links(&harness), before, "one undo reverts {k:?} {m:?}");
        key(&mut harness, egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
    }
}

#[test]
fn edit_menu_lists_link_commands_with_their_shortcuts() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().selection = vec![1, 2];
    harness.run();
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Attach").click();
    harness.run();
    assert!(links(&harness).iter().all(|l| l.is_some_and(|l| l.kind == LinkKind::Attach)), "Edit > Attach attaches");
}

#[test]
fn layers_panel_nests_attached_members_under_a_parent_row() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    harness.state_mut().design.add_shape(crate::design::ShapeKind::Circle);
    harness.state_mut().design.attach(&[1, 2]);
    harness.run();
    let corner = |n: egui_kittest::kittest::Node<'_>| n.bounding_box().map(|r| (r.x0, r.y0)).unwrap();
    let parent = corner(harness.get_by_label("Layer Attached"));
    let members: Vec<(f64, f64)> = harness.get_all_by_label("Layer Square").map(corner).collect();
    let circle = corner(harness.get_by_label("Layer Circle"));
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|m| m.1 > parent.1 && m.0 > parent.0), "members sit indented beneath the parent");
    assert!(circle.1 < parent.1, "the circle is on top, so it lists first");

    harness.get_by_label("Layer Attached").click();
    harness.run();
    let mut picked = harness.state().selection.clone();
    picked.sort();
    assert_eq!(picked, vec![1, 2], "the parent row selects the whole link");
    harness.get_by_label("Collapse Attached").click();
    harness.run();
    assert_eq!(harness.query_all_by_label("Layer Square").count(), 0, "collapsed rows hide their members");
    harness.get_by_label("Hide Attached").click();
    harness.run();
    assert!(harness.state().design.shapes.iter().filter(|s| s.link.is_some()).all(|s| s.hidden), "hide acts on every member");
    harness.get_by_label("Delete Attached").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 1, "delete removes every member");
}

/// How many inspector fields (X, Y, W, H) carry this unit suffix.
fn fields_in(harness: &Harness<'_, Kerf>, units: Units) -> usize {
    harness.query_all_by_label(units.suffix()).count()
}

fn numeric(harness: &Harness<'_, Kerf>, label: &str) -> f64 {
    harness.get_by_label(label).numeric_value().unwrap()
}

fn pick_centimetres(harness: &mut Harness<'_, Kerf>) {
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label("Centimetres").click();
    harness.run();
    harness.press_key(egui::Key::Escape);
    harness.run();
}

#[test]
fn centimetres_from_the_view_menu_reach_every_length() {
    let mut harness = snapshot_harness(egui::vec2(1500.0, 900.0));
    let size_in = 1.0;
    let id = harness.state_mut().design.add_text("Units".into(), crate::text::FontRef::Inter, size_in, egui::Color32::BLACK, 2.0, 2.0);
    harness.state_mut().selection = vec![id];
    harness.run();
    assert_eq!((fields_in(&harness, Units::Imperial), fields_in(&harness, Units::Metric)), (4, 0));
    assert!((numeric(&harness, "Font size") - f64::from(size_in * 72.0)).abs() < 1e-3);

    pick_centimetres(&mut harness);
    assert_eq!(harness.state().units, Units::Metric);
    assert_eq!((fields_in(&harness, Units::Imperial), fields_in(&harness, Units::Metric)), (0, 4), "the inspector suffixes read cm");
    assert!((numeric(&harness, "Font size") - f64::from(size_in * 72.0)).abs() < 1e-3, "font size stays in points");

    harness.get_by_label("Effects").click();
    harness.run();
    harness.get_by_label("Offset").click();
    harness.run();
    assert!((numeric(&harness, "Offset distance") - 0.25 * 2.54).abs() < 1e-3, "offset distance shows in cm");
    harness.get_by_label("Effects").click();
    harness.run();

    harness.get_by_label("Preview").click();
    harness.run();
    let metric_mat = MachineFamily::StudioPro.preferred_mat().name(Units::Metric);
    assert!(harness.query_by_label(metric_mat).is_some(), "Project Preview reads {metric_mat}");
    harness.get_by_label("Close Project Preview").click();
    harness.run();

    harness.get_by_label("Make").click();
    harness.run_steps(4);
    assert_eq!(harness.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Material Size").value().as_deref(), Some(metric_mat), "the Prepare dropdown reads cm");
    assert!(harness.query_by_label(metric_mat).is_some(), "the Make flow's preview card reads cm");
}

#[test]
fn a_width_typed_in_centimetres_is_stored_in_inches() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.state_mut().units = Units::Metric;
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("W").focus();
    harness.run();
    harness.get_by_label("W").type_text("5.08");
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    assert!((harness.state().design.shapes[0].width_in - 2.0).abs() < 1e-4, "5.08 cm is 2 in");
}

#[test]
fn an_offset_typed_in_centimetres_is_applied_in_inches() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.state_mut().units = Units::Metric;
    let id = harness.state_mut().design.add_shape(crate::design::ShapeKind::Square);
    let s = harness.state_mut().design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    harness.state_mut().selection = vec![id];
    harness.run();
    harness.get_by_label("Effects").click();
    harness.run();
    harness.get_by_label("Offset").click();
    harness.run();
    harness.get_by_label("Offset distance").focus();
    harness.run();
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_label("Offset distance").type_text("1.27");
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    assert!((numeric(&harness, "Offset distance") - 1.27).abs() < 1e-3, "the field reads back in cm: {}", numeric(&harness, "Offset distance"));
    harness.get_by_label("Apply").click();
    harness.run();
    let design = &harness.state().design;
    assert_eq!(design.shapes.len(), 1, "the offset is an effect on the square");
    let ring = design.geometry(&design.shapes[0]).pieces()[0].bounds;
    assert!((ring.max_x - ring.min_x - 3.0).abs() < 0.02, "a 1.27 cm offset grows each side by 0.5 in: {ring:?}");
}

#[test]
fn units_survive_a_relaunch() {
    let size = egui::vec2(1200.0, 700.0);
    let mut harness = snapshot_harness(size);
    pick_centimetres(&mut harness);
    let Fixture { harness, root, .. } = harness;
    drop(harness);
    let relaunched = launch(root, size);
    assert_eq!(relaunched.state().units, Units::Metric);
}

#[test]
fn rail_tooltip_sits_right_of_the_rail_level_with_its_icon() {
    let mut harness = snapshot_harness(egui::vec2(900.0, 700.0));
    let pencil = harness.get_by_label("Pencil").bounding_box().unwrap();
    harness.get_by_label("Pencil").hover();
    for _ in 0..60 {
        harness.step();
    }
    harness.run();
    let body = harness.get_by_label("Draw freehand. The stroke is smoothed into a curve.").bounding_box().unwrap();
    assert!(body.x0 > tokens::RAIL_W as f64, "card starts right of the rail: {}", body.x0);
    let centre = (pencil.y0 + pencil.y1) / 2.0;
    assert!(body.y0 - 40.0 < centre && centre < body.y1 + 40.0, "card is level with the icon: {centre} vs {}..{}", body.y0, body.y1);
}

#[test]
fn flip_arrange_and_align_menus_act_on_the_selection() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    two_overlapping_squares(&mut harness);
    let pick = |harness: &mut Fixture, tool: &str, item: &str| {
        harness.get_by_label(tool).click();
        harness.run();
        harness.get_by_label(item).click();
        harness.run();
    };

    pick(&mut harness, "Align", "Align Left");
    let shapes = &harness.state().design.shapes;
    assert!((shapes[0].x_in - shapes[1].x_in).abs() < 1e-4, "left edges meet: {} vs {}", shapes[0].x_in, shapes[1].x_in);

    let first = harness.state().design.shapes[0].id;
    harness.state_mut().selection = vec![first];
    harness.run();
    pick(&mut harness, "Arrange", "Move to Front");
    assert_eq!(harness.state().design.shapes.last().unwrap().id, first, "moved to the front");

    pick(&mut harness, "Flip", "Flip Horizontal");
    pick(&mut harness, "Flip", "Flip Vertical");
    let flipped = harness.state().design.shapes.iter().find(|s| s.id == first).unwrap();
    assert!(flipped.flip_x && flipped.flip_y, "flipped both ways");
}

#[test]
fn align_popover_disables_distribute_for_two_shapes() {
    let mut harness = snapshot_harness(egui::vec2(1000.0, 700.0));
    two_overlapping_squares(&mut harness);
    harness.state_mut().design.shapes[1].y_in += 1.5;
    harness.run();
    harness.get_by_label("Align").click();
    harness.run();

    for name in ["Distribute Vertical", "Distribute Horizontal"] {
        assert!(harness.get_by_label(name).is_disabled(), "{name} needs three objects");
    }
    let before = harness.state().design.clone();
    harness.get_by_label("Distribute Horizontal").click();
    harness.run();
    assert_eq!(harness.state().design, before, "a disabled button does nothing");

    assert!(!harness.get_by_label("Align Top").is_disabled());
    harness.get_by_label("Align Top").click();
    harness.run();
    let shapes = &harness.state().design.shapes;
    assert!((shapes[0].y_in - shapes[1].y_in).abs() < 1e-4, "top edges meet: {} vs {}", shapes[0].y_in, shapes[1].y_in);
}

#[test]
fn combine_popover_enables_slice_for_exactly_two_units() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    two_overlapping_squares(&mut harness);
    let third = harness.state_mut().design.add_shape(crate::design::ShapeKind::Circle);
    let two = harness.state().selection.clone();
    harness.state_mut().selection.push(third);
    harness.run();
    harness.get_by_label("Combine").click();
    harness.run();
    assert!(harness.get_by_label("Slice").is_disabled(), "three units cannot slice");
    assert!(!harness.get_by_label("Weld").is_disabled());

    harness.state_mut().selection = two;
    harness.run();
    assert!(!harness.get_by_label("Slice").is_disabled(), "two units slice");

    harness.get_by_label("Slice").click();
    harness.run();
    assert_eq!(harness.state().design.shapes.len(), 4, "the two squares went into a slice group beside the circle");
    assert_eq!(harness.state().selection.len(), 1, "the group is selected");
}

#[test]
fn flatten_prints_then_cuts_and_unflatten_restores() {
    use crate::design::{Operation, ShapeKind};
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    two_overlapping_squares(&mut harness);
    harness.state_mut().design.shapes[1].fill = egui::Color32::from_rgb(0xe0, 0x40, 0x40);
    harness.get_by_label("Combine").click();
    harness.run();
    harness.get_by_label("Flatten").click();
    harness.run();
    let shapes = &harness.state().design.shapes;
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0].kind, ShapeKind::Flattened { .. }));
    assert_eq!(shapes[0].operation, Operation::PrintThenCut);
    assert_eq!(harness.state().selection, [shapes[0].id], "the flattened shape is selected");
    harness.get_by_label(Operation::PrintThenCut.label());

    harness.get_by_label("Combine").click();
    harness.run();
    assert!(harness.query_by_label("Flatten").is_none(), "Unflatten takes the Flatten slot");
    harness.get_by_label("Unflatten").click();
    harness.run();
    let fills: Vec<egui::Color32> = harness.state().design.shapes.iter().map(|s| s.fill).collect();
    assert_eq!(fills, [egui::Color32::BLACK, egui::Color32::from_rgb(0xe0, 0x40, 0x40)]);
}
