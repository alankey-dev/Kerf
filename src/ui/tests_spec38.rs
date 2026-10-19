//! Spec 38: the heavy benchmark design in the app, the bench script, and the timing gate.

use std::time::{Duration, Instant};

use egui::{Pos2, pos2, vec2};

use super::bench_mode::{SCRIPTED, Script};
use super::canvas::Tool;
use super::commands::Command;
use super::op_style;
use super::test_harness::{Fixture, click_at, snapshot_harness};
use crate::bench::report::{FrameStats, Phase};
use crate::bench::{self, Counts};
use crate::edit::AppCommand;

const WINDOW: egui::Vec2 = vec2(1600.0, 1000.0);

/// The app with the heavy scenario open from its project file, as `kerf --open` opens it.
fn heavy() -> (Fixture, Counts) {
    let mut f = snapshot_harness(WINDOW);
    let design = bench::build(bench::DEFAULT_SEED, bench::DEFAULT_OBJECTS);
    let written = bench::write(&f.root.path().join("bench"), &design).unwrap();
    bench::install_image(&f.projects).unwrap();
    f.state_mut().open_file(&written.project).unwrap();
    f.step();
    (f, bench::counts(&design))
}

#[test]
fn the_heavy_scenario_opens_from_its_file_with_every_object_and_the_bench_script_plays_each_phase() {
    let (mut f, counts) = heavy();
    assert!(counts.objects >= 150 && counts.layers >= 400, "{counts:?}");
    assert_eq!(bench::counts(&f.state().design), counts, "the opened tab holds the whole scenario");
    let before = f.state().design.clone();
    let mut script = Script::new(&before, 2, Duration::ZERO);
    let (mut played, mut made, mut previewed) = (Vec::new(), false, false);
    while let Some(phase) = script.step(f.state_mut()) {
        if played.last() != Some(&phase) {
            played.push(phase);
        }
        f.step();
        made |= f.state().flow.is_some();
        previewed |= f.state().cut_preview.is_some();
    }
    assert_eq!(played, SCRIPTED);
    assert!(made && previewed, "Make and the cut preview opened");
    let s = f.state();
    assert!(s.flow.is_none() && s.cut_preview.is_none() && s.view.zoom == 1.0, "the script closes what it opened");
    assert_ne!(s.design, before, "the drags moved shapes");
    assert!(f.state_mut().open_file(std::path::Path::new("/nowhere/missing.kerf.json")).is_err());
    assert!(f.state().error.as_deref().is_some_and(|e| e.starts_with("Couldn't open /nowhere/missing.kerf.json")));
}

#[test]
fn a_steady_prepare_frame_triangulates_no_mat_card_again() {
    let (mut f, _) = heavy();
    f.state_mut().scripted = Some(Command::App(AppCommand::Make));
    let opened = op_style::tessellations();
    for _ in 0..3 {
        f.step();
    }
    assert!(f.state().flow.as_ref().is_some_and(|flow| flow.mats.iter().any(|m| m.print_then_cut)), "Prepare shows Print Then Cut mats");
    let settled = op_style::tessellations();
    assert!(settled - opened > 120, "opening Prepare triangulates each Print Then Cut piece once, for its fill and both shadows");
    for _ in 0..5 {
        f.step();
    }
    assert_eq!(op_style::tessellations(), settled, "Prepare frames with nothing changed reuse every mesh");
}

fn ms(since: Instant) -> f32 {
    since.elapsed().as_secs_f32() * 1000.0
}

fn timed_step(f: &mut Fixture, events: Vec<egui::Event>) -> f32 {
    f.input_mut().events.extend(events);
    let clock = Instant::now();
    f.step();
    ms(clock)
}

/// Where the canvas draws inch `(0, 0)`: a Text tool click puts a text's top-left under the pointer. The
/// text is dropped again.
fn canvas_origin(f: &mut Fixture) -> Pos2 {
    let design = f.state().design.clone();
    // Probe on a blank canvas. Spec 51's Text tool places a text on a shape's edge when the click lands
    // within 6 px of one, which in the heavy design would attach the probe text and move it off the click.
    let mut blank = crate::design::Design::default();
    blank.set_snapping(false);
    f.state_mut().design = blank;
    f.state_mut().tool = Tool::Text;
    let probe = pos2(700.0, 500.0);
    click_at(f, probe);
    let text = f.state().design.shapes.last().unwrap().clone();
    let px = 72.0 * f.state().view.zoom;
    let s = f.state_mut();
    (s.doc.design, s.doc.tool, s.doc.view.text_edit) = (design, Tool::Select, None);
    s.doc.selection.clear();
    f.step();
    probe - vec2(text.x_in, text.y_in) * px
}

/// How long a fixed workload that uses none of Kerf's code takes on this machine, in milliseconds: a map of
/// 20 000 owned strings, then a sort of its keys. A ceiling in multiples of it holds on a runner of any speed
/// and still fails when Kerf's own work grows.
pub(super) fn reference_sample_ms() -> f32 {
    let clock = Instant::now();
    let map: std::collections::HashMap<String, usize> = (0..20_000).map(|i| (format!("key-{}", i * 7919 % 20_011), i)).collect();
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    std::hint::black_box(keys.len());
    ms(clock)
}

pub(super) fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// The reference on a quiet moment of this machine: the median of 15 samples.
fn reference_ms() -> f32 {
    median((0..15).map(|_| reference_sample_ms()).collect())
}

/// Ceilings for median times in multiples of [`reference_ms`], about 2.2 times what the benchmark records
/// (docs/specs/38-benchmark.md), so a slowdown of that order fails and runner noise does not.
const IDLE_X: f32 = 0.55;
const DRAG_SHAPE_X: f32 = 0.7;
const DRAG_GROUP_X: f32 = 0.75;
const LOAD_X: f32 = 1.45;
const MAKE_X: f32 = 1.1;
const PREPARE_X: f32 = 2.5;

fn row(name: &str, ms: &[f32]) -> FrameStats {
    let stats = FrameStats::of(ms).unwrap_or_else(|| panic!("no {name} frames"));
    println!("{name:<20} {:>6} {:>9.2} {:>8.2} {:>8.2}", stats.frames, stats.median_ms, stats.p95_ms, stats.max_ms);
    stats
}

#[test]
#[ignore = "timing; CI runs it on the release build"]
fn heavy_design_frame_times_stay_under_their_ceilings() {
    let (mut f, _) = heavy();
    let reference_before = reference_ms();
    let text = std::fs::read_to_string(f.root.path().join("bench").join(bench::PROJECT_FILE)).unwrap();
    let load: Vec<f32> = (0..5)
        .map(|_| {
            let clock = Instant::now();
            let (project, _) = crate::project::parse(&text).unwrap();
            let mut design = project.design;
            design.settle(None);
            bench::evaluate(&design);
            ms(clock)
        })
        .collect();

    let mut frames: Vec<(Phase, Vec<f32>)> = Vec::new();
    for _ in 0..3 {
        let mut script = Script::new(&f.state().design, 60, Duration::ZERO);
        while let Some(phase) = script.step(f.state_mut()) {
            let ms = timed_step(&mut f, Vec::new());
            match frames.last_mut() {
                Some((p, times)) if *p == phase => times.push(ms),
                _ => frames.push((phase, vec![ms])),
            }
        }
    }
    let all = |phase: Phase| frames.iter().filter(|(p, _)| *p == phase).flat_map(|(_, t)| t.iter().copied()).collect::<Vec<f32>>();
    let firsts = |phase: Phase| frames.iter().filter(|(p, _)| *p == phase).map(|(_, t)| t[0]).collect::<Vec<f32>>();
    let undo: Vec<f32> = frames.iter().filter(|(p, _)| *p == Phase::UndoRedo).flat_map(|(_, t)| t.iter().step_by(2).copied()).collect();

    f.state_mut().view.zoom = 0.5;
    f.step();
    let origin = canvas_origin(&mut f);
    let at = |(x, y): (f32, f32)| origin + vec2(x, y) * 36.0;
    let (from, to) = (at((-0.15, -0.15)), at((14.0, 14.0)));
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let mut marquee = Vec::new();
    for _ in 0..5 {
        timed_step(&mut f, vec![egui::Event::PointerMoved(from), button(from, true)]);
        for k in 1..=6 {
            let p = from + (to - from) * (k as f32 / 6.0);
            marquee.push(timed_step(&mut f, vec![egui::Event::PointerMoved(p)]));
        }
        marquee.push(timed_step(&mut f, vec![button(to, false)]));
        let s = f.state();
        let missed = s.design.shapes.iter().filter(|x| x.parent.is_none() && !s.selection.contains(&x.id)).count();
        assert_eq!(missed, 0, "the marquee selects every top-level object");
    }

    let reference = (reference_before + reference_ms()) / 2.0;
    println!("\nspec 38 heavy design, kittest steps on the {} build, reference workload {reference:.2} ms", if cfg!(debug_assertions) { "debug" } else { "release" });
    println!("{:<20} {:>6} {:>9} {:>8} {:>8}", "measure", "frames", "median ms", "p95 ms", "max ms");
    let load = row("load", &load);
    let idle = row("idle frame", &all(Phase::Idle));
    let drag_shape = row("drag one shape", &all(Phase::DragShape));
    let drag_group = row("drag a group", &all(Phase::DragGroup));
    row("marquee select-all", &marquee);
    row("zoom step", &all(Phase::Zoom));
    row("undo", &undo);
    let make = row("Make entry", &firsts(Phase::MakePrepare));
    let prepare = row("Prepare frame", &all(Phase::MakePrepare));
    row("cut preview open", &firsts(Phase::CutPreview));
    let measured = [
        ("idle frame", idle, IDLE_X),
        ("drag one shape", drag_shape, DRAG_SHAPE_X),
        ("drag a group", drag_group, DRAG_GROUP_X),
        ("load", load, LOAD_X),
        ("Make entry", make, MAKE_X),
        ("Prepare frame", prepare, PREPARE_X),
    ];
    println!("\n{:<20} {:>9} {:>9}", "measure", "x ref", "ceiling");
    for (name, stats, ceiling) in measured {
        println!("{name:<20} {:>9.2} {:>9.2}", stats.median_ms / reference, ceiling);
    }

    if cfg!(debug_assertions) {
        return;
    }
    for (name, stats, ceiling) in measured {
        assert!(stats.median_ms < ceiling * reference, "{name} {stats:?} is over {ceiling} x the {reference:.2} ms reference");
    }
    assert!(load.median_ms < 500.0, "load {load:?}");
    assert!(make.median_ms < 250.0, "Make entry {make:?}");
    // The 16 ms frame budget is for a GPU. Under a software renderer (such as Mesa's lavapipe) every frame
    // pays for rasterising on the CPU, so only the ceilings above, which scale with the machine, apply there.
    if std::env::var_os(SOFTWARE_RENDERER).is_some() {
        println!("{SOFTWARE_RENDERER} is set: the 16 ms frame budgets are skipped on a software renderer");
        return;
    }
    assert!(idle.median_ms < 16.0, "idle {idle:?}");
    assert!(drag_shape.median_ms < 16.0, "drag one shape {drag_shape:?}");
    assert!(drag_group.median_ms < 16.0, "drag a group {drag_group:?}");
    assert!(prepare.median_ms < 16.0, "Prepare frame {prepare:?}");
}

/// Set by CI when the kittests draw through a software renderer, where a frame's time is mostly the CPU's
/// rasterising rather than Kerf's work.
const SOFTWARE_RENDERER: &str = "KERF_SOFTWARE_RENDERER";

#[test]
fn zooming_and_panning_the_canvas_triangulate_no_fill_again() {
    let (mut f, _) = heavy();
    f.step();
    let first = op_style::tessellations();
    for zoom_in in [true, true, true, true, false, false, false, false, false, false, true, true, true] {
        let before = f.state().view.zoom;
        f.state_mut().scripted = Some(Command::App(if zoom_in { AppCommand::ZoomIn } else { AppCommand::ZoomOut }));
        f.step();
        assert_ne!(f.state().view.zoom, before, "zooming in {zoom_in} changed the zoom");
    }
    let screen = egui::Rect::from_min_size(Pos2::ZERO, WINDOW);
    let artboard = f.state().view.artboard_rect(screen);
    for _ in 0..10 {
        let wheel = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(-60.0, -40.0), modifiers: egui::Modifiers::NONE };
        f.input_mut().events.extend([egui::Event::PointerMoved(pos2(800.0, 500.0)), wheel]);
        f.step();
    }
    assert_ne!(f.state().view.artboard_rect(screen), artboard, "the wheel panned the canvas");
    assert_eq!(op_style::tessellations(), first, "every fill drawn on the first frame is reused at every zoom and pan");
}
