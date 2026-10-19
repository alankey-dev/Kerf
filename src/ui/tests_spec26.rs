//! Spec 26: the cut preview animates each mat's toolpath in machine order, from Prepare and from the
//! canvas View menu, and Prepare's mat list shows each mat's estimated time.

use egui::{Color32, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::test_harness::{Fixture, assert_flow_snapshot, key, pick, rect_of, snapshot_harness};
use crate::design::{Design, Operation, ShapeKind};
use crate::output::estimate::{self, MoveKind};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// A black heart and star, and a red square: two mats, with travel between strokes.
fn design(fixture: &mut Fixture) {
    let mut d = Design::default();
    for (kind, fill, (x, y, side)) in
        [(ShapeKind::Heart, Color32::BLACK, (1.0, 1.0, 3.0)), (ShapeKind::Star, Color32::BLACK, (5.0, 2.0, 1.5)), (ShapeKind::Square, Color32::RED, (1.0, 5.0, 2.0))]
    {
        let id = d.add_shape(kind);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in, s.fill, s.operation) = (x, y, side, side, fill, Operation::BasicCut);
    }
    fixture.state_mut().design = d;
    fixture.run();
}

fn to_prepare(fixture: &mut Fixture) {
    design(fixture);
    fixture.get_by_label("Make").click();
    fixture.run_steps(10);
    assert!(fixture.state().flow.is_some(), "Make opens Prepare");
}

fn readout(fixture: &Fixture) -> String {
    let p = fixture.state().cut_preview.as_ref().expect("the preview is open");
    format!("{} of {}", estimate::clock(p.t), estimate::clock(p.total_s()))
}

#[test]
fn preview_cut_opens_from_prepare_and_play_and_pause_drive_the_readout() {
    let mut fixture = snapshot_harness(SIZE);
    to_prepare(&mut fixture);
    fixture.get_by_label("Preview cut").click();
    fixture.run_steps(3);
    assert!(fixture.state().cut_preview.is_some(), "Preview cut opens the preview");
    let start = readout(&fixture);
    fixture.get_by_label(&start);
    assert!(start.starts_with("0:00 of "), "{start}");

    fixture.get_by_label("Play").click();
    fixture.run_steps(10);
    let playing = readout(&fixture);
    assert_ne!(playing, start, "Play advances the readout");
    fixture.get_by_label(&playing);

    fixture.get_by_label("Pause").click();
    fixture.run_steps(2);
    let paused = fixture.state().cut_preview.as_ref().unwrap().t;
    fixture.run_steps(10);
    assert_eq!(fixture.state().cut_preview.as_ref().unwrap().t, paused, "Pause stops it");
    assert!(!fixture.state().cut_preview.as_ref().unwrap().playing);
}

#[test]
fn dragging_the_scrubber_to_the_end_completes_every_work_move() {
    let mut fixture = snapshot_harness(SIZE);
    to_prepare(&mut fixture);
    fixture.get_by_label("Preview cut").click();
    fixture.run_steps(3);
    let bar = rect_of(&fixture, "Scrubber");
    let path = [bar.left_center() + egui::vec2(2.0, 0.0), bar.center(), bar.right_center() + egui::vec2(40.0, 0.0)];
    fixture.input_mut().events.push(egui::Event::PointerMoved(path[0]));
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: path[0], button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    fixture.step();
    for &at in &path[1..] {
        fixture.input_mut().events.push(egui::Event::PointerMoved(at));
        fixture.step();
    }
    fixture.input_mut().events.push(egui::Event::PointerButton { pos: path[2], button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    fixture.run_steps(2);
    let p = fixture.state().cut_preview.as_ref().unwrap();
    let total = estimate::clock(p.total_s());
    assert_eq!(readout(&fixture), format!("{total} of {total}"), "the end shows the total as elapsed");
    fixture.get_by_label(&format!("{total} of {total}"));

    let flow = fixture.state().flow.as_ref().unwrap();
    let est = super::cut_preview::mat_estimate(flow, flow.mat().unwrap());
    let (_, now) = estimate::position_at(&est, p.t);
    assert_eq!(now, est.motions.len() - 1, "the head is on the last move");
    let works: Vec<_> = est.motions.iter().filter(|m| matches!(m.kind, MoveKind::Work(_))).collect();
    assert!(!works.is_empty());
    assert!(works.iter().all(|m| m.start_s + m.duration_s <= p.t + 1e-4), "every work move is complete");
}

#[test]
fn view_cut_preview_opens_the_overlay_on_the_canvas_and_escape_closes_it() {
    let mut fixture = snapshot_harness(SIZE);
    design(&mut fixture);
    fixture.get_by_label("View").click();
    fixture.run();
    pick(&mut fixture, "Cut preview");
    fixture.run_steps(3);
    assert!(fixture.state().flow.is_none(), "the canvas stays open under it");
    assert!(fixture.state().cut_preview.is_some(), "View > Cut preview opens it");
    fixture.get_by_label("Mat 1 of 2");
    let shapes = fixture.state().design.shapes.len();
    key(&mut fixture, Key::Escape, Modifiers::NONE);
    fixture.run_steps(2);
    assert!(fixture.state().cut_preview.is_none(), "Escape closes it");
    assert_eq!(fixture.state().design.shapes.len(), shapes);
}

#[test]
fn snapshots_the_preview_midway_and_prepares_mat_times() {
    let mut fixture = snapshot_harness(SIZE);
    to_prepare(&mut fixture);
    let flow = fixture.state().flow.as_ref().unwrap();
    let times: Vec<_> = flow.mats.iter().map(|m| estimate::clock(super::cut_preview::mat_estimate(flow, m).total_s)).collect();
    assert_eq!(times.len(), 2);
    for (n, (mat, time)) in flow.mats.clone().iter().zip(&times).enumerate() {
        fixture.get_by_label(&format!("Load {}  {}  {time}", n + 1, mat.label()));
    }
    assert_flow_snapshot(&mut fixture, "26-prepare-times");

    fixture.get_by_label("Preview cut").click();
    fixture.run_steps(3);
    let p = fixture.state_mut().cut_preview.as_mut().unwrap();
    p.t = p.total_s() / 2.0;
    fixture.run_steps(1);
    fixture.get_by_label("Estimate: the simulated machine publishes no speeds, so these times are nominal");
    assert_flow_snapshot(&mut fixture, "26-preview-midway");
}

#[test]
fn the_project_preview_card_shows_an_open_devices_work_area_instead_of_a_mat_style() {
    let mut fixture = snapshot_harness(SIZE);
    let plotter = crate::output::DeviceProfile { id: 1, ..crate::output::DeviceProfile::grbl_pen_plotter() };
    let (w, h) = plotter.work_area_in();
    (fixture.state_mut().devices, fixture.state_mut().last_device) = (vec![plotter], Some(1));
    to_prepare(&mut fixture);
    assert!(fixture.state().flow.as_ref().unwrap().profile().is_some(), "Make starts on the plotter");
    let area = fixture.state().units.size(w, h);
    assert!(fixture.get_all_by_label(&area).next().is_some(), "the card shows the work area");
    assert!(fixture.query_by_label("On Mat").is_none(), "no mat style for an open device");
}
