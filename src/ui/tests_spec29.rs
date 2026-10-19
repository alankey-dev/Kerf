//! Spec 29: single-line writing fonts, driven through the app.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::test_harness::{REFERENCE_CSS, assert_snapshot, click_at, key, snapshot_harness};
use super::{Kerf, canvas};
use crate::design::{Operation, ShapeKind};
use crate::pens::{PenPick, PenSize};
use crate::stroke_font::StrokeFontId;
use crate::text::FontRef;

const SIZE: egui::Vec2 = egui::vec2(1400.0, 900.0);

fn writing(id: &str) -> FontRef {
    FontRef::Stroke(StrokeFontId(id.into()))
}

/// Places "Text" with the Text tool and leaves the editor that placing opens.
fn place_text(harness: &mut Harness<'_, Kerf>) -> u64 {
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Text;
    click_at(harness, egui::pos2(700.0, 400.0));
    harness.press_key(egui::Key::Escape);
    harness.run();
    harness.state().selection[0]
}

/// Adds `content` in `font` with a Fine pen and selects it.
fn add_writing(harness: &mut Harness<'_, Kerf>, content: &str, font: FontRef, at: (f32, f32)) -> u64 {
    let design = &mut harness.state_mut().design;
    let id = design.add_text(content.into(), font, 0.6, egui::Color32::BLACK, at.0, at.1);
    design.shape_mut(id).unwrap().pick_pen(PenPick::Size(PenSize::Fine));
    harness.state_mut().selection = vec![id];
    harness.run();
    id
}

fn font_and_operation(harness: &Harness<'_, Kerf>, id: u64) -> (FontRef, Operation) {
    let shape = harness.state().design.shapes.iter().find(|s| s.id == id).unwrap();
    let ShapeKind::Text { font, .. } = &shape.kind else { panic!("not text") };
    (font.clone(), shape.operation)
}

fn hover_long(harness: &mut Harness<'_, Kerf>, label: &str) {
    harness.get_by_label(label).hover();
    for _ in 0..60 {
        harness.step();
    }
    harness.run();
}

#[test]
fn picking_a_writing_font_turns_a_cut_into_a_pen_with_a_notice_that_one_undo_reverses() {
    let mut harness = snapshot_harness(REFERENCE_CSS);
    let id = place_text(&mut harness);
    harness.get_by_role_and_label(Role::ComboBox, "Pen").click();
    harness.run();
    harness.get_by_label("Basic").click();
    harness.run();
    assert_eq!(font_and_operation(&harness, id), (FontRef::Inter, Operation::BasicCut), "the text starts as a cut");

    harness.get_by_label("Font").click();
    harness.run();
    assert!(harness.query_by_label("Hershey Simplex Roman").is_some(), "the menu lists writing fonts");
    harness.get_by_label("Relief SingleLine").click();
    harness.run();
    assert_eq!(font_and_operation(&harness, id), (writing("relief-singleline"), Operation::Pen), "the font and the operation change together");
    assert!(harness.query_by_label("Writing fonts draw with a pen").is_some(), "the notice explains the switch");
    assert!(harness.state().view.notice.as_ref().is_some_and(|n| n.message() == "Writing fonts draw with a pen"));

    harness.get_by_label("Undo").click();
    harness.run();
    harness.run();
    assert_eq!(font_and_operation(&harness, id), (FontRef::Inter, Operation::BasicCut), "one undo restores both");
    assert!(harness.query_by_label("Writing fonts draw with a pen").is_none(), "the notice goes with the change it describes");
}

#[test]
fn writing_text_offers_regular_and_italic_with_bold_greyed() {
    let mut harness = snapshot_harness(SIZE);
    add_writing(&mut harness, "Hi", writing("hershey-simplex"), (2.0, 2.0));
    harness.get_by_label("Font style").click();
    harness.run();
    assert!(harness.query_by_label("Regular").is_some() && harness.query_by_label("Italic").is_some());
    assert!(harness.query_by_label("Medium").is_none(), "one weight");
    assert!(harness.get_by_label("Bold").is_disabled(), "Bold is greyed");
    hover_long(&mut harness, "Bold");
    assert!(harness.query_by_label("Writing fonts have one weight; the pen size sets the line weight.").is_some(), "and says why");
}

#[test]
fn weld_is_disabled_for_single_line_text_and_says_why() {
    let mut harness = snapshot_harness(SIZE);
    let square = harness.state_mut().design.add_shape(ShapeKind::Square);
    let text = add_writing(&mut harness, "Hi", writing("hershey-simplex"), (5.0, 2.0));
    harness.state_mut().selection = vec![square, text];
    harness.run();
    harness.get_by_label("Combine").click();
    harness.run();
    assert!(harness.get_by_label("Weld").is_disabled(), "Weld is disabled");
    assert!(!harness.get_by_label("Flatten").is_disabled(), "the popover is open for the pair");
    hover_long(&mut harness, "Weld");
    assert!(harness.query_by_label("Single-line text has no area to combine.").is_some(), "the tooltip names the reason");

    harness.state_mut().selection = vec![text];
    harness.run();
    harness.get_by_label("Effects").click();
    harness.run();
    assert!(harness.get_by_label("Offset").is_disabled(), "Offset is disabled");
    assert!(harness.get_by_label("Sticker").is_disabled(), "Sticker is disabled");
    assert!(!harness.get_by_label("Convert to Paths").is_disabled());
}

#[test]
fn ungroup_to_letters_keeps_the_writing_font_on_every_letter() {
    let mut harness = snapshot_harness(SIZE);
    let font = writing("hershey-script-simplex");
    add_writing(&mut harness, "Pen", font.clone(), (3.0, 3.0));
    harness.get_by_label("Format").click();
    harness.run();
    harness.get_by_label("Letters").click();
    harness.run();
    let shapes = &harness.state().design.shapes;
    let letters: Vec<String> = shapes
        .iter()
        .map(|s| match &s.kind {
            ShapeKind::Text { content, font: f, .. } if *f == font => content.clone(),
            other => panic!("{other:?} lost the writing font"),
        })
        .collect();
    assert_eq!(letters, ["P", "e", "n"], "one text per letter");
}

#[test]
fn the_font_menu_shows_writing_fonts_and_filters_to_them() {
    let mut harness = snapshot_harness(REFERENCE_CSS);
    add_writing(&mut harness, "Hello", writing("hershey-script-complex"), (3.0, 3.0));
    harness.get_by_label("Font").click();
    harness.run();
    assert!(harness.get_by_label("Hershey Complex Script").toggled() == Some(egui::accesskit::Toggled::True), "the current writing font is ticked");
    assert_snapshot(&mut harness, "29-font-menu");

    harness.get_by_label("Writing only").click();
    harness.run();
    assert!(harness.query_by_label("Inter").is_none() && harness.query_by_label("System fonts").is_none(), "only writing fonts");
    assert!(harness.query_by_label("Relief SingleLine").is_some());
    harness.get_by_label("Search fonts").type_text("relief");
    harness.run();
    assert!(harness.query_by_label("Hershey Simplex Roman").is_none() && harness.query_by_label("Relief SingleLine").is_some(), "search covers Writing");
}

#[test]
fn relief_singleline_draws_with_a_fine_pen_beside_the_same_words_in_inter() {
    let mut harness = snapshot_harness(SIZE);
    add_writing(&mut harness, "Happy birthday", writing("relief-singleline"), (1.0, 2.0));
    add_writing(&mut harness, "Happy birthday", FontRef::Inter, (1.0, 3.5));
    harness.state_mut().selection.clear();
    harness.run();
    let shapes = &harness.state().design.shapes;
    assert!(shapes.iter().all(|s| s.operation == Operation::Pen && s.pen.size == PenSize::Fine));
    assert!(shapes[0].kind.outline(shapes[0].width_in, shapes[0].height_in).iter().all(|c| !c.closed), "Relief draws open strokes");
    assert!(shapes[1].kind.outline(shapes[1].width_in, shapes[1].height_in).iter().all(|c| c.closed), "Inter traces closed outlines");
    assert_snapshot(&mut harness, "29-writing");
}

#[test]
fn a_writing_font_is_found_in_the_command_palette_and_switches_a_cut_to_pen() {
    let mut harness = snapshot_harness(SIZE);
    let id = add_writing(&mut harness, "Hi", FontRef::Inter, (2.0, 2.0));
    harness.state_mut().design.shape_mut(id).unwrap().set_operation(Operation::BasicCut);
    harness.run();
    key(&mut harness, egui::Key::K, egui::Modifiers::COMMAND);
    assert!(harness.state().palette.is_open());
    harness.input_mut().events.push(egui::Event::Text("relief singleline".into()));
    harness.run();
    assert!(harness.query_by_label("Relief SingleLine").is_some(), "the palette lists the writing font");
    key(&mut harness, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(font_and_operation(&harness, id), (writing("relief-singleline"), Operation::Pen), "picking it sets the font and the pen");
    assert!(harness.query_by_label("Writing fonts draw with a pen").is_some(), "with the same notice as the menu");
}
