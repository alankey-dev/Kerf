//! Spec 56 deferred: the Eyedropper tool, the per-project palette commands, and the eyedropper cursor
//! over an image, driven through the real app.

use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers, Pos2, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use image::{Rgba, RgbaImage};

use super::Kerf;
use super::canvas::Tool;
use super::eyedropper;
use super::test_harness::{assert_snapshot, click_at, move_to, snapshot_harness};
use crate::assets;
use crate::design::{Design, History, Operation, Pt, ShapeKind};
use crate::pens::{Pen, PenSize};
use crate::settings::Swatch;

const SIZE: Vec2 = vec2(1400.0, 900.0);
const RED: Color32 = Color32::from_rgb(0xD0, 0x20, 0x2E);
const BLUE: Color32 = Color32::from_rgb(0x1F, 0x5F, 0xBF);

/// A map from world inches to screen points, calibrated by placing a probe text and reading it back.
fn grid_map(harness: &mut Harness<'_, Kerf>) -> impl Fn(Pt) -> Pos2 + use<> {
    harness.state_mut().tool = Tool::Text;
    let probe = pos2(640.0, 420.0);
    click_at(harness, probe);
    let text = harness.state().design.shapes.last().unwrap().clone();
    let px = 72.0 * harness.state().view.zoom;
    let origin = probe - vec2(text.x_in, text.y_in) * px;
    harness.state_mut().design.remove_many(&[text.id]);
    harness.state_mut().tool = Tool::Select;
    harness.run();
    move |(x, y)| origin + vec2(x, y) * px
}

fn shape(harness: &Harness<'_, Kerf>, id: u64) -> crate::design::Shape {
    harness.state().design.shapes.iter().find(|s| s.id == id).unwrap().clone()
}

#[test]
fn eyedropper_applies_a_clicked_shapes_style_to_the_selection_in_one_undo_step() {
    let mut harness = snapshot_harness(SIZE);
    let map = grid_map(&mut harness);

    let mut design = Design::default();
    let blue = design.add_shape(ShapeKind::Square);
    let red = design.add_shape(ShapeKind::Square);
    for (id, x, fill, op) in [(blue, 1.0_f32, BLUE, Operation::BasicCut), (red, 3.0, RED, Operation::Score)] {
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (x, 1.0, 1.0, 1.0);
        s.fill = fill;
        s.set_operation(op);
    }
    design.set_snapping(false);
    harness.state_mut().design = design;
    harness.state_mut().selection = vec![blue];
    harness.state_mut().history = History::default();
    harness.run();

    harness.get_by_role_and_label(Role::Button, "Eyedropper").click();
    harness.run();
    click_at(&mut harness, map((3.5, 1.5)));
    harness.run();

    assert_eq!(shape(&harness, blue).fill, RED, "the blue shape takes the red shape's fill");
    assert_eq!(shape(&harness, blue).operation, Operation::Score, "and its operation");

    harness.press_key_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(shape(&harness, blue).fill, BLUE, "one undo restores the fill");
    assert_eq!(shape(&harness, blue).operation, Operation::BasicCut, "and the operation");
}

#[test]
fn sample_lifts_a_basic_cut_shapes_colour_operation_and_pen() {
    let mut design = Design::default();
    let id = design.add_shape(ShapeKind::Square);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (0.0, 0.0, 2.0, 2.0);
    s.set_operation(Operation::BasicCut);
    s.set_pen(Pen::named("Green", PenSize::Fine));
    s.fill = RED;

    let lift = eyedropper::sample(&design, &assets::Pixels::new(), (1.0, 1.0)).expect("a shape under the point");
    assert_eq!(lift.colour, RED, "a cut shape lifts its fill");
    assert_eq!(lift.style.operation, Operation::BasicCut);
    assert_eq!(lift.style.fill, RED);
    assert_eq!(lift.style.pen, Pen::named("Green", PenSize::Fine));

    // Option lifts colour only: applying `Edit::Colour` sets the fill without touching the operation.
    let target = design.add_shape(ShapeKind::Circle);
    design.shape_mut(target).unwrap().set_operation(Operation::Score);
    crate::edit::run(&mut design, &mut vec![target], &crate::edit::Edit::Colour(lift.colour), crate::edit::Subject::Ids(&[target])).unwrap();
    let after = design.shapes.iter().find(|s| s.id == target).unwrap();
    assert_eq!(after.fill, RED, "colour only recolours");
    assert_eq!(after.operation, Operation::Score, "and leaves the operation alone");
}

#[test]
fn the_project_palette_commands_move_swatches_between_project_and_settings() {
    let mut harness = snapshot_harness(SIZE);
    let swatches =
        vec![Swatch { name: "Navy".into(), colour: Color32::from_rgb(0x1C, 0x2E, 0x6B) }, Swatch { name: "Gold".into(), colour: Color32::from_rgb(0xC9, 0xA2, 0x3A) }];
    harness.state_mut().swatches = swatches.clone();
    harness.run();

    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Save palette to project").click();
    harness.run();
    assert_eq!(harness.state().design.swatches, swatches, "the project takes the user's swatches");

    harness.state_mut().swatches.clear();
    harness.run();
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Copy palette to settings").click();
    harness.run();
    assert_eq!(harness.state().swatches, swatches, "the settings palette is restored from the project");
}

#[test]
fn the_eyedropper_cursor_shows_the_pixel_under_a_bitmap() {
    let mut harness = snapshot_harness(SIZE);
    let map = grid_map(&mut harness);

    let dir = assets::dir(&harness.projects);
    let asset = assets::store(&dir, &RgbaImage::from_pixel(8, 8, Rgba([0xE8, 0x7A, 0x1F, 0xFF]))).unwrap();

    let mut design = Design::default();
    let id = design.add_shape(ShapeKind::Bitmap { asset: asset.clone(), px: (8, 8), clip: Vec::new() });
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (2.0, 2.0, 2.0, 2.0);
    design.set_snapping(false);
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.run();
    // The canvas paints the bitmap from an async texture, so settle the loader thread before the
    // snapshot or the image races its grey placeholder from one run to the next.
    let textures = assets::Textures::of(&harness.ctx).expect("installed by Kerf::new");
    assert!(matches!(textures.wait_loaded(&asset, assets::Wrap::Clamp), assets::Texture::Ready(_)));
    harness.run();

    harness.get_by_role_and_label(Role::Button, "Eyedropper").click();
    harness.run();
    move_to(&mut harness, map((3.0, 3.0)));
    harness.run();

    assert_snapshot(&mut harness, "56-eyedropper");
}
