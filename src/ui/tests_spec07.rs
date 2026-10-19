//! Spec 07: the pen picker and nib-width strokes, driven through the real app.

use egui_kittest::kittest::Queryable;

use super::test_harness::{assert_snapshot, snapshot_harness};
use crate::design::{Contour, Operation, Seg};
use crate::pens::{Pen, PenSize};

#[test]
fn the_pen_picker_sets_size_and_colour_on_a_pen_layer() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    harness.get_by_label("Shapes").click();
    harness.run();
    harness.get_by_label("Basic Cut").click();
    harness.run();
    harness.get_all_by_label("Pen").last().unwrap().click();
    harness.run();
    assert_eq!(harness.state().design.shapes[0].operation, Operation::Pen);

    harness.get_by_role_and_label(egui::accesskit::Role::Button, "Colour").click();
    harness.run();
    harness.get_by_label("Pen size").click();
    harness.run();
    harness.get_by_label(PenSize::Medium.label()).click();
    harness.run();
    assert_eq!(harness.state().design.shapes[0].pen.size, PenSize::Medium);

    harness.get_by_label("Wine").click();
    harness.run();
    let shape = &harness.state().design.shapes[0];
    let wine = Pen::named("Wine", PenSize::Medium);
    assert_eq!(shape.pen, wine);
    assert_eq!(shape.fill, wine.rgb());
    assert!(harness.query_by_label("Pen size").is_none(), "picking a colour closes the picker");

    let line = |y: f32| Contour { start: (4.0, y), segs: vec![Seg::Line((8.0, y))], closed: false };
    for (y, size) in [(1.0, PenSize::ExtraFine), (1.5, PenSize::Fine), (2.0, PenSize::Medium), (2.5, PenSize::Bold)] {
        let id = harness.state_mut().design.add_path(line(y), egui::Color32::BLACK, Operation::Pen).unwrap();
        harness.state_mut().design.shape_mut(id).unwrap().pen.size = size;
    }
    assert_snapshot(&mut harness, "07-pen-nibs");
}
