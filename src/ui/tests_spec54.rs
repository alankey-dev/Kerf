//! Spec 54: the Export dialog gains PNG and JPEG with raster options, and Copy as PNG puts a picture of
//! the selection on the clipboard.

use egui::Color32;
use egui_kittest::kittest::Queryable;
use image::RgbaImage;

use super::test_harness::{Fixture, assert_snapshot, pick, snapshot_harness};
use crate::design::{Design, Operation, ShapeKind};
use crate::export::{self, Format};

const SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);

/// A single red square, with the app set to write exports into a temp folder.
fn one_square(fixture: &mut Fixture) -> tempfile::TempDir {
    let mut d = Design::default();
    let id = d.add_shape(ShapeKind::Square);
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.fill, s.operation) = (1.0, 1.0, Color32::RED, Operation::BasicCut);
    let out = tempfile::tempdir().unwrap();
    let app = fixture.state_mut();
    app.design = d;
    app.export_dir = Some(out.path().to_owned());
    fixture.run();
    out
}

#[test]
fn export_as_png_shows_the_pixel_size_and_writes_a_file_that_decodes_to_it() {
    let mut fixture = snapshot_harness(SIZE);
    let out = one_square(&mut fixture);
    fixture.get_by_label("File").click();
    fixture.run();
    pick(&mut fixture, "Export...");
    fixture.get_by_label("PNG").click();
    fixture.run();
    fixture.get_by_label("150").click();
    fixture.run();

    let request = fixture.state().export.as_ref().unwrap().request;
    assert_eq!(request.format, Format::Png);
    assert_eq!(request.raster.dpi, 150);
    assert_snapshot(&mut fixture, "54-export-raster");

    // The file list's size comes straight from the plan the dialog shows, so proving the plan proves
    // the list; the written file must then decode to the same size.
    let files = export::plan(&fixture.state().design, &request, "Untitled-1", fixture.state().mat);
    let size = files[0].raster_size().expect("a PNG has a pixel size");
    assert!(size.0 > 0 && size.1 > 0);

    pick(&mut fixture, "Export");
    assert!(fixture.state().export.is_none(), "Export closes the dialog");
    let png = std::fs::read(out.path().join("Untitled-1.png")).expect("the PNG is written");
    let decoded = image::load_from_memory(&png).expect("valid PNG").into_rgba8();
    assert_eq!(decoded.dimensions(), size, "the written file decodes to the size the list showed");
}

fn copied_image(fixture: &Fixture) -> RgbaImage {
    let colour = fixture
        .output()
        .platform_output
        .commands
        .iter()
        .rev()
        .find_map(|command| match command {
            egui::OutputCommand::CopyImage(image) => Some(image.clone()),
            _ => None,
        })
        .expect("an image was copied to the clipboard");
    let [w, h] = colour.size;
    RgbaImage::from_fn(w as u32, h as u32, |x, y| {
        let [r, g, b, a] = colour[(x as usize, y as usize)].to_srgba_unmultiplied();
        image::Rgba([r, g, b, a])
    })
}

#[test]
fn copy_as_png_puts_a_transparent_cornered_picture_on_the_clipboard() {
    let mut fixture = snapshot_harness(SIZE);
    let id = fixture.state_mut().design.add_shape(ShapeKind::Heart);
    let shape = fixture.state_mut().design.shape_mut(id).unwrap();
    (shape.x_in, shape.y_in, shape.fill) = (4.0, 3.0, Color32::RED);
    fixture.state_mut().selection = vec![id];
    fixture.run();

    fixture.get_by_label("Edit").click();
    fixture.run();
    // One step so the frame that dispatches the command, and emits the clipboard image, is the last.
    fixture.get_all_by_label("Copy as PNG").last().expect("the Copy as PNG menu row").click();
    fixture.step();

    let image = copied_image(&fixture);
    assert!(image.width() > 10 && image.height() > 10, "a real picture: {:?}", image.dimensions());
    assert_eq!(image.get_pixel(0, 0).0[3], 0, "the corner outside the heart is transparent");
    assert!(image.pixels().any(|p| p.0[3] == 255), "the heart itself is opaque");
}
