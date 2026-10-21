//! Hostile and broken SVGs through the real Upload entry point and the clipboard decoder: none reads a
//! local file through `<image href>`, each finishes promptly, and a failure shows the import banner.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

use super::clipboard;
use super::test_harness::{Fixture, snapshot_harness};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/malformed/");

fn upload(fixture: &mut Fixture, name: &str) {
    fixture.state_mut().upload_file = Some(PathBuf::from(DIR).join(name));
    fixture.get_by_label("Upload").click();
    fixture.step();
    fixture.state_mut().wait_svg_answered();
    fixture.step();
}

#[test]
fn an_undecodable_file_shows_the_import_banner_not_a_font_error() {
    let mut fixture = snapshot_harness(vec2(1000.0, 700.0));
    upload(&mut fixture, "undecodable.svg");
    fixture.run();
    fixture.get_by_label_contains("Couldn't import the SVG");
    assert!(fixture.state().design.shapes.is_empty());
}

#[test]
fn image_hrefs_import_the_drawing_without_a_banner() {
    for name in ["href-dev-zero.svg", "href-unc.svg", "href-relative.svg"] {
        let mut fixture = snapshot_harness(vec2(1000.0, 700.0));
        upload(&mut fixture, name);
        fixture.run();
        assert_eq!(fixture.state().design.shapes.len(), 1, "{name} imports its square");
        assert_eq!(fixture.state().error, None, "{name}");
    }
}

#[test]
fn a_compressed_svg_imports_instead_of_failing_as_a_font() {
    let mut fixture = snapshot_harness(vec2(1000.0, 700.0));
    upload(&mut fixture, "compressed.svg");
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), 1);
    assert_eq!(fixture.state().error, None);
}

#[test]
fn paste_of_hostile_svg_text_finishes_and_ignores_the_href() {
    for name in ["href-dev-zero.svg", "href-unc.svg", "href-relative.svg"] {
        let text = std::fs::read_to_string(format!("{DIR}{name}")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || drop(tx.send(clipboard::parse(&text).map(|p| matches!(p, clipboard::Payload::Svg(i) if i.len() == 1)))));
        let parsed = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_else(|_| panic!("{name} pasted promptly"));
        assert_eq!(parsed, Ok(true), "{name}");
    }
}
