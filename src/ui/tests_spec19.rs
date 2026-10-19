//! Spec 19: operation rendering, the operation legend and Show as made, driven through the real app.

use egui::Color32;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::op_style::Mode;
use super::test_harness::{assert_snapshot, bare, bare_frame, click_at, drag_along, move_to, pixel, rgb, snapshot_harness, two_squares_apart};
use crate::design::{Design, FoilWeight, Operation, ShapeKind};

/// Multiples of the reference workload; see `docs/specs/38-benchmark.md`.
const CEILING_200_X: f32 = 0.45;

const TEAL: Color32 = Color32::from_rgb(0x1f, 0x8a, 0x8a);

/// A 1 inch square per operation in a row from (1, 1), 1.5 inches apart, and the inch-to-screen map.
fn squares(harness: &mut Harness<'_, Kerf>, ops: &[Operation]) -> impl Fn((f32, f32)) -> egui::Pos2 + use<> {
    let to_screen = two_squares_apart(harness);
    let mut design = Design::default();
    for (i, &op) in ops.iter().enumerate() {
        let id = design.add_shape(ShapeKind::Square);
        let shape = design.shape_mut(id).unwrap();
        (shape.x_in, shape.y_in, shape.width_in, shape.height_in) = (1.0 + 1.5 * (i % 6) as f32, 1.0 + 2.0 * (i / 6) as f32, 1.0, 1.0);
        shape.fill = TEAL;
        shape.set_operation(op);
    }
    harness.state_mut().design = design;
    harness.run();
    to_screen
}

fn teal() -> [u8; 3] {
    [TEAL.r(), TEAL.g(), TEAL.b()]
}

#[test]
fn a_square_switched_from_basic_cut_to_score_has_no_fill_inside() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = squares(&mut harness, &[Operation::BasicCut]);
    let centre = to_screen((1.5, 1.5));
    assert_eq!(pixel(&mut harness, centre), teal(), "Basic Cut fills the square");

    harness.state_mut().design.shapes[0].set_operation(Operation::Score);
    harness.run();
    let artboard = bare(&mut harness, centre);
    assert_eq!(pixel(&mut harness, centre), artboard, "Score draws the outline only, so the artboard shows through");
}

#[test]
fn clicking_score_in_the_legend_selects_exactly_the_score_shapes() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = squares(&mut harness, &[Operation::BasicCut, Operation::Score, Operation::Pen, Operation::Score]);
    let scores: Vec<u64> = harness.state().design.shapes.iter().filter(|s| s.operation == Operation::Score).map(|s| s.id).collect();
    assert_eq!(scores.len(), 2);
    harness.get_by_label("Score shapes").click();
    harness.run();
    assert_eq!(harness.state().selection, scores);
}

#[test]
fn show_as_made_draws_a_basic_cut_square_as_its_outline() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = squares(&mut harness, &[Operation::BasicCut]);
    let centre = to_screen((1.5, 1.5));
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label("Show as made").click();
    harness.run();
    assert_eq!(harness.state().view.mode, Mode::AsMade);
    click_at(&mut harness, to_screen((11.5, 11.5)));
    let artboard = bare(&mut harness, centre);
    assert_eq!(pixel(&mut harness, centre), artboard, "no fill inside");
    let edge = to_screen((1.3, 1.0));
    assert_ne!(pixel(&mut harness, edge), bare(&mut harness, edge), "the cut line is drawn");
}

const EVERY: [Operation; 12] = [
    Operation::BasicCut,
    Operation::WavyCut,
    Operation::Perforate,
    Operation::Pen,
    Operation::Foil(FoilWeight::Fine),
    Operation::Foil(FoilWeight::Bold),
    Operation::Score,
    Operation::Deboss,
    Operation::Engrave,
    Operation::PrintThenCut,
    Operation::Guide,
    Operation::Foil(FoilWeight::Medium),
];

#[test]
fn every_operation_side_by_side() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = squares(&mut harness, &EVERY);
    let design = &mut harness.state_mut().design;
    for (i, op) in EVERY.iter().enumerate() {
        let (x, y) = (1.0 + 1.5 * (i % 6) as f32, 2.15 + 2.0 * (i / 6) as f32);
        let id = design.add_text(op.label().to_owned(), crate::text::FontRef::Inter, 0.18, Color32::from_gray(0x40), x, y);
        design.shape_mut(id).unwrap().set_operation(Operation::BasicCut);
    }
    move_to(&mut harness, egui::pos2(700.0, 650.0));
    let bare = bare_frame(&mut harness);
    let corner = |i: usize| (1.0 + 1.5 * (i % 6) as f32, 1.0 + 2.0 * (i / 6) as f32);
    let centre = |i: usize| to_screen((corner(i).0 + 0.5, corner(i).1 + 0.5));
    let top_edge = |i: usize| {
        let (x, y) = corner(i);
        egui::Rect::from_min_max(to_screen((x + 0.1, y)) - egui::vec2(0.0, 2.0), to_screen((x + 0.9, y)) + egui::vec2(0.0, 2.0))
    };
    let edge_drawn = |image: &image::RgbaImage, i: usize| {
        let r = top_edge(i);
        (r.top() as u32..r.bottom() as u32).any(|y| (r.left() as u32..r.right() as u32).any(|x| image.get_pixel(x, y) != bare.get_pixel(x, y)))
    };

    let design = assert_snapshot(&mut harness, "19-operations");
    for (i, op) in EVERY.iter().enumerate() {
        let filled = matches!(op, Operation::BasicCut | Operation::WavyCut | Operation::PrintThenCut);
        let expected = if filled { teal() } else { rgb(&bare, centre(i)) };
        assert_eq!(rgb(&design, centre(i)), expected, "{op:?}: filled is {filled}");
        assert!(edge_drawn(&design, i), "{op:?} draws its edge");
    }

    harness.state_mut().view.mode = Mode::AsMade;
    let made = assert_snapshot(&mut harness, "19-operations-as-made");
    for (i, op) in EVERY.iter().enumerate() {
        let expected = if *op == Operation::PrintThenCut { teal() } else { rgb(&bare, centre(i)) };
        assert_eq!(rgb(&made, centre(i)), expected, "{op:?} as made: only Print Then Cut is printed");
        assert_eq!(edge_drawn(&made, i), *op != Operation::Guide, "{op:?} as made: only a guide leaves no mark");
    }
}

#[test]
fn hovering_a_shape_names_its_operation() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = squares(&mut harness, &[Operation::Score]);
    harness.state_mut().view.material = Some("Cardstock");
    move_to(&mut harness, to_screen((1.5, 1.5)));
    harness.get_by_label("Score, Cardstock");
}

#[test]
fn a_marquee_passes_over_guides_but_a_click_still_selects_one() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let to_screen = squares(&mut harness, &[Operation::Guide, Operation::BasicCut]);
    let ids: Vec<u64> = harness.state().design.shapes.iter().map(|s| s.id).collect();
    drag_along(&mut harness, &[to_screen((0.5, 0.5)), to_screen((2.0, 1.5)), to_screen((3.0, 2.5))]);
    assert_eq!(harness.state().selection, [ids[1]], "the marquee picks the cut, not the guide");
    click_at(&mut harness, to_screen((1.5, 1.5)));
    assert_eq!(harness.state().selection, [ids[0]]);
}

/// Run with `cargo test --release -- --ignored a_frame_of_200`. The gate is the CPU half of the frame (layout,
/// painting and tessellation), as a multiple of a reference workload sampled between the frames, so a slow or
/// busy runner moves both. The GPU render is printed and not gated: it is the runner's driver, not Kerf's code.
#[test]
#[ignore = "timing; CI runs it on the release build"]
fn a_frame_of_200_shapes_stays_within_its_ceiling() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let mut design = Design::default();
    for i in 0..200 {
        let id = design.add_shape(if i % 3 == 0 { ShapeKind::Star } else { ShapeKind::Heart });
        let shape = design.shape_mut(id).unwrap();
        (shape.x_in, shape.y_in, shape.width_in, shape.height_in) = ((i % 20) as f32 * 0.55, (i / 20) as f32 * 1.1, 0.5, 0.5);
        shape.set_operation(EVERY[i % EVERY.len()]);
    }
    harness.state_mut().design = design;
    harness.run();
    let (mut reference, mut cpu, mut gpu) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..61 {
        reference.push(super::tests_spec38::reference_sample_ms());
        let start = std::time::Instant::now();
        harness.run();
        cpu.push(start.elapsed().as_secs_f32() * 1000.0);
        let start = std::time::Instant::now();
        harness.render().expect("wgpu render");
        gpu.push(start.elapsed().as_secs_f32() * 1000.0);
    }
    let (reference, cpu, gpu) = (super::tests_spec38::median(reference), super::tests_spec38::median(cpu), super::tests_spec38::median(gpu));
    eprintln!("200 shapes: CPU frame {cpu:.2} ms, {:.2} x the {reference:.2} ms reference; GPU render {gpu:.2} ms", cpu / reference);
    assert!(cpu < CEILING_200_X * reference, "CPU frame {cpu:.2} ms is over {CEILING_200_X} x the {reference:.2} ms reference");
}
