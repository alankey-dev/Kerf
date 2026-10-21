//! The surfaces each spec promises for a command (a menu row, a palette entry, a shortcut) against the
//! tables the app draws them from, and the rows that were once missing (issue 32): effect rows in the
//! shape menu, Reset this part in a Layers part row's menu, Step and repeat, Rotate copies, Select Same
//! and Transform Again in the palette and the Edit menu.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::commands::{self, Command};
use super::context_menu::{SHAPE_MENU, labels};
use super::test_harness::{Fixture, pick, right_click_at, snapshot_harness, two_squares_apart};
use crate::design::ShapeKind;
use crate::edit::{Edit, TextPathEdit};
use crate::effects::{Corners, Effect, Reach, Row, Sticker};

/// What one spec line promises, and where.
struct Promise {
    /// The spec file and the surface it names, as written there.
    cited: &'static str,
    /// Labels the shape context menu shows.
    context: &'static [&'static str],
    /// Palette titles.
    palette: &'static [&'static str],
    /// Palette titles that also carry a shortcut, so Help > Keyboard shortcuts lists them.
    keyed: &'static [&'static str],
}

const SELECT_SAME: &[&str] = &[
    "Select Same: Colour",
    "Select Same: Operation",
    "Select Same: Pen",
    "Select Same: Font",
    "Select Same: Shape kind",
    "Select Same: Effects",
    "Select Same: Everything alike",
];

/// Spec 51's "Text on path" in the context menu is tracked with the pick mode in issue 8.
const PROMISES: &[Promise] = &[
    Promise {
        cited: "21-live-effects: context menu gains Edit effect, Disable or Enable effect, Remove effect, Expand",
        context: &["Edit effect", "Disable effect", "Enable effect", "Remove effect", "Expand"],
        palette: &["Expand"],
        keyed: &[],
    },
    Promise {
        cited: "50-select-same-and-lasso: Select Same in the context menu and the palette",
        context: &["Select Same", "Select Inverse"],
        palette: SELECT_SAME,
        keyed: &[],
    },
    Promise {
        cited: "51-text-on-path: Remove from path in the context menu and the palette",
        context: &["Remove from path"],
        palette: &["Remove from path", "Text on path"],
        keyed: &["Text on path"],
    },
    Promise {
        cited: "52-repeat-and-transform: Step and repeat, Rotate copies in the context menu and palette",
        context: &["Step and repeat...", "Rotate copies..."],
        palette: &["Step and repeat...", "Rotate copies..."],
        keyed: &[],
    },
    Promise { cited: "52-repeat-and-transform: Transform Again with its shortcut", context: &[], palette: &["Transform Again"], keyed: &["Transform Again"] },
];

#[test]
fn every_spec_promise_has_its_surface_in_the_menu_and_registry_tables() {
    let context: Vec<&str> = SHAPE_MENU.iter().flat_map(labels).collect();
    let registry = commands::registry((12.0, 12.0));
    let titles: Vec<String> = registry.iter().map(|c| commands::title(c).into_owned()).collect();
    let keyed: Vec<String> = registry.iter().filter(|c| commands::info(c).shortcut.is_some()).map(|c| commands::title(c).into_owned()).collect();
    let mut missing = Vec::new();
    for promise in PROMISES {
        let Promise { cited, context: want_context, palette, keyed: want_keyed } = promise;
        missing.extend(want_context.iter().filter(|l| !context.contains(l)).map(|l| format!("{cited}: context menu has no {l}")));
        missing.extend(palette.iter().filter(|l| !titles.iter().any(|t| t == *l)).map(|l| format!("{cited}: palette has no {l}")));
        missing.extend(want_keyed.iter().filter(|l| !keyed.iter().any(|t| t == *l)).map(|l| format!("{cited}: shortcut list has no {l}")));
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn the_edit_menu_lists_select_same_step_and_repeat_rotate_copies_and_transform_again() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    fixture.get_by_label("Edit").click();
    fixture.run();
    for label in ["Select Same", "Step and repeat...", "Rotate copies...", "Transform Again"] {
        assert!(fixture.query_by_label(label).is_some(), "the Edit menu lists {label}");
    }
}

fn sticker_on_first(fixture: &mut Fixture) -> impl Fn((f32, f32)) -> egui::Pos2 + use<> {
    let at = two_squares_apart(fixture);
    let effect = Effect::Sticker(Sticker { border_in: 0.125, corners: Corners::Round });
    let design = &mut fixture.state_mut().design;
    design.apply(&Edit::AddEffect { effect, reach: Reach::EachShape }, &[1]).unwrap();
    fixture.run();
    at
}

fn first_effect_enabled(fixture: &Fixture) -> bool {
    fixture.state().design.shapes[0].effects.entries()[0].enabled
}

#[test]
fn the_shape_menu_disables_enables_edits_and_removes_the_selected_shapes_effect() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = sticker_on_first(&mut fixture);
    right_click_at(&mut fixture, at((2.0, 2.0)));
    pick(&mut fixture, "Disable effect");
    assert!(!first_effect_enabled(&fixture), "Disable effect turned the sticker off");
    assert_eq!(fixture.state().selection, vec![1], "the selection is untouched");

    right_click_at(&mut fixture, at((2.0, 2.0)));
    pick(&mut fixture, "Enable effect");
    assert!(first_effect_enabled(&fixture), "Enable effect turned it back on");

    right_click_at(&mut fixture, at((2.0, 2.0)));
    pick(&mut fixture, "Edit effect");
    assert!(matches!(super::effects_list::editing(&fixture.ctx), Some(e) if e.shape == 1 && matches!(e.row, Row::Entry(_))), "Edit effect opens the sticker's panel");

    right_click_at(&mut fixture, at((2.0, 2.0)));
    pick(&mut fixture, "Remove effect");
    assert!(fixture.state().design.shapes[0].effects.is_empty(), "Remove effect took it off");
}

#[test]
fn effect_rows_are_greyed_on_a_shape_with_no_effects() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut fixture);
    right_click_at(&mut fixture, at((2.0, 2.0)));
    for label in ["Edit effect", "Disable effect", "Remove effect"] {
        let row = fixture.get_all_by_label(label).last().unwrap_or_else(|| panic!("no {label} row"));
        assert!(row.is_disabled(), "{label} is greyed with nothing to act on");
    }
}

#[test]
fn remove_from_path_is_a_row_of_the_shape_menu() {
    let rows: Vec<&str> = SHAPE_MENU.iter().flat_map(labels).collect();
    let remove = super::commands::info(&Command::Edit(Edit::TextPath(TextPathEdit::Remove))).label;
    assert!(rows.contains(&remove));
}

#[test]
fn a_layers_part_rows_menu_offers_reset_this_part_and_runs_it() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut fixture);
    fixture.state_mut().selection = vec![1];
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND.plus(Modifiers::ALT), Key::K);
    fixture.run();
    let instance = fixture.state().selection[0];
    let master = {
        let design = &fixture.state().design;
        design.components[0].shapes[0].id
    };
    fixture
        .state_mut()
        .design
        .apply(&Edit::Override { part: master, change: crate::components::PartChange::Operation(crate::design::Operation::Pen) }, &[instance])
        .unwrap();
    fixture.run();
    let overridden = |fixture: &Fixture| matches!(&fixture.state().design.shapes.iter().find(|s| s.id == instance).unwrap().kind, ShapeKind::Instance { overrides, .. } if !overrides.is_empty());
    assert!(overridden(&fixture));
    let row = fixture.get_all_by_label("Part Square").last().expect("a part row in Layers").bounding_box().unwrap();
    right_click_at(&mut fixture, egui::pos2(row.x0 as f32 + 60.0, ((row.y0 + row.y1) / 2.0) as f32));
    assert!(fixture.query_by_label("Duplicate").is_none(), "a part row opens its own menu, not the shape menu");
    pick(&mut fixture, "Reset this part");
    assert!(!overridden(&fixture), "Reset this part cleared the override");
}

#[test]
fn shift_adds_select_sames_matches_to_the_selection() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let (square, text) = {
        let design = &mut fixture.state_mut().design;
        let square = design.add_shape(ShapeKind::Square);
        (square, design.add_text("Hi".into(), crate::text::FontRef::Inter, 1.0, egui::Color32::BLACK, 6.0, 6.0))
    };
    let all_text = Command::Edit(Edit::SelectSame(crate::select::Match::Text));
    fixture.state_mut().selection = vec![square];
    fixture.run();
    let ctx = fixture.ctx.clone();
    fixture.state_mut().run(all_text.clone(), &ctx);
    assert_eq!(fixture.state().selection, vec![text], "Select Same replaces the selection");

    fixture.state_mut().selection = vec![square];
    fixture.input_mut().modifiers = Modifiers::SHIFT;
    fixture.step();
    let ctx = fixture.ctx.clone();
    fixture.state_mut().run(all_text, &ctx);
    assert_eq!(fixture.state().selection, vec![square, text], "with Shift it adds to the selection");
}

#[test]
fn transform_again_from_the_edit_menu_repeats_the_last_step() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let id = fixture.state_mut().design.add_shape(ShapeKind::Square);
    {
        let s = fixture.state_mut().design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (1.0, 3.0, 1.0, 1.0);
    }
    fixture.state_mut().selection = vec![id];
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::D);
    fixture.run();
    let copy = fixture.state().selection[0];
    fixture.state_mut().design.shape_mut(copy).unwrap().x_in = 2.0;
    fixture.run();
    fixture.press_key_modifiers(Modifiers::COMMAND, Key::D);
    fixture.run();
    let before = fixture.state().design.shapes.len();
    let last = fixture.state().selection[0];
    let x = fixture.state().design.shapes.iter().find(|s| s.id == last).unwrap().x_in;
    fixture.get_by_label("Edit").click();
    fixture.run();
    pick(&mut fixture, "Transform Again");
    let moved = fixture.state().design.shapes.iter().find(|s| s.id == last).unwrap().x_in;
    assert_eq!(fixture.state().design.shapes.len(), before, "no copy");
    assert!((moved - (x + 1.0)).abs() < 1e-3, "moved one pitch on, from {x} to {moved}");
}

#[test]
fn transform_again_with_nothing_remembered_says_so_and_changes_nothing() {
    let mut fixture = snapshot_harness(egui::vec2(1400.0, 900.0));
    let id = fixture.state_mut().design.add_shape(ShapeKind::Square);
    fixture.state_mut().selection = vec![id];
    fixture.run();
    let before = fixture.state().design.clone();
    let ctx = fixture.ctx.clone();
    fixture.state_mut().run(Command::Edit(commands::TRANSFORM_AGAIN), &ctx);
    assert_eq!(fixture.state().design, before);
    assert!(fixture.state().doc.view.notice.is_some(), "a notice explains");
}
