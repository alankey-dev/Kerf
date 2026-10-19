//! Spec 51: putting text on a path and taking it off, driven through the app. Attach runs from the
//! Cmd+Option+T shortcut; Side, Offset and Remove run as the Path panel and canvas handles raise them.

use egui::{Color32, Key, Modifiers};

use super::test_harness::{Fixture, assert_snapshot, key, snapshot_harness};
use crate::design::{Design, History, ShapeKind};
use crate::edit::{Edit, TextPathEdit};
use crate::text::FontRef;
use crate::text_path::Side;

const SIZE: egui::Vec2 = egui::vec2(1000.0, 700.0);
const CMD_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

/// A text and a heart, the heart to the text's right, both selected, on a fresh canvas.
fn text_and_heart() -> (Fixture, u64, u64) {
    let mut fx = snapshot_harness(SIZE);
    let mut design = Design::default();
    let text = design.add_text("Spectrum".into(), FontRef::Inter, 0.6, Color32::BLACK, 1.5, 3.0);
    let heart = design.add_shape(ShapeKind::Heart);
    if let Some(s) = design.shape_mut(heart) {
        (s.x_in, s.y_in, s.width_in, s.height_in) = (4.0, 2.0, 3.5, 3.5);
        s.set_operation(crate::design::Operation::Guide);
    }
    design.set_snapping(false);
    fx.state_mut().design = design;
    fx.state_mut().selection = vec![text, heart];
    fx.state_mut().history = History::default();
    fx.run();
    (fx, text, heart)
}

fn has_path(fx: &Fixture, id: u64) -> bool {
    matches!(fx.state().design.shapes.iter().find(|s| s.id == id).map(|s| &s.kind), Some(ShapeKind::Text { path: Some(_), .. }))
}

fn run(fx: &mut Fixture, edit: Edit) {
    let ctx = fx.harness.ctx.clone();
    fx.harness.state_mut().run(super::commands::Command::Edit(edit), &ctx);
    fx.harness.run();
}

#[test]
fn cmd_option_t_runs_the_text_along_the_heart_and_undo_restores_both() {
    let (mut fx, text, heart) = text_and_heart();
    key(&mut fx.harness, Key::T, CMD_ALT);

    assert!(has_path(&fx, text), "the text now follows a path");
    assert_eq!(fx.state().selection, vec![text], "the text is left selected");
    assert!(fx.state().design.shapes.iter().any(|s| s.id == heart), "the heart is kept");
    assert_snapshot(&mut fx.harness, "51-text-on-heart");

    key(&mut fx.harness, Key::Z, Modifiers::COMMAND);
    assert!(!has_path(&fx, text), "one undo takes the text off the path");
    assert!(fx.state().design.shapes.iter().any(|s| s.id == heart), "the heart is still there");
}

#[test]
fn the_path_panel_edits_flip_the_side_slide_the_offset_and_straighten() {
    let (mut fx, text, _) = text_and_heart();
    key(&mut fx.harness, Key::T, CMD_ALT);
    let path = |fx: &Fixture| match &fx.state().design.shapes.iter().find(|s| s.id == text).unwrap().kind {
        ShapeKind::Text { path: Some(p), .. } => p.clone(),
        _ => panic!("the text is on a path"),
    };

    run(&mut fx, Edit::TextPath(TextPathEdit::Side(Side::Below)));
    assert_eq!(path(&fx).side, Side::Below, "the panel flips the text below the path");

    run(&mut fx, Edit::TextPath(TextPathEdit::Offset(0.4)));
    assert!((path(&fx).offset_in - 0.4).abs() < 1e-5, "the offset slides the run");

    run(&mut fx, Edit::TextPath(TextPathEdit::Remove));
    assert!(!has_path(&fx, text), "Remove from path straightens the text");
}
