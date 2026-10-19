//! Spec 23: one command model. Every surface's entries come from the command table and the model's
//! check, toolbar Flip mirrors the whole selection, and Undo during a drag ends it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::commands::{Command, Cx, Entry, info};
use super::test_harness::{drag_along, move_to, press_at, snapshot_harness, two_squares_apart};
use super::{Kerf, chrome, context_menu, layers_actions, selection, toolbar};
use crate::containers::AlignTo;
use crate::design::{Design, ShapeKind};
use crate::edit::Edit;

/// Every entry each surface shows for `on`, named by surface.
fn surfaces(d: &Design, on: &[u64]) -> Vec<(&'static str, Entry)> {
    let cx = Cx::new(d, on, AlignTo::Container);
    let mut out = Vec::new();
    let history = crate::design::History::default();
    for command in chrome::EDIT_MENU.iter().flat_map(|g| g.iter()).cloned().chain(chrome::FILE_MENU.map(Command::App)) {
        let e = match &command {
            Command::Edit(e) => cx.entry(e.clone()),
            Command::App(a) => cx.app(a.clone(), &history),
            Command::Clipboard(action) => cx.clipboard(*action),
        };
        out.push(("top bar", e));
    }
    for rows in [context_menu::SHAPE_MENU, context_menu::EMPTY_MENU] {
        for row in rows {
            match row {
                context_menu::Row::Sub(_, items) => out.extend(items.iter().map(|e| ("context submenu", context_menu::resolve(&cx, e)))),
                row => out.extend(context_menu::shown(row, &cx).map(|e| ("context menu", e))),
            }
        }
    }
    out.extend(selection::entries(&cx).into_iter().map(|e| ("action bar", e)));
    out.extend(selection::MORE_MENU.map(|e| ("action bar More", cx.entry(e))));
    out.extend(toolbar::entries(&cx).into_iter().map(|e| ("toolbar", e)));
    for slot in &layers_actions::BAR {
        match slot {
            layers_actions::Slot::One(e) => out.push(("Layers bar", cx.entry(e.clone()))),
            layers_actions::Slot::Either(a, b) => out.push(("Layers bar", cx.either(a.clone(), b.clone()))),
            layers_actions::Slot::Combine | layers_actions::Slot::Contour => {}
        }
    }
    out
}

/// Squares 1 and 2, a container 3 made of a circle, and a text 4.
fn mixed() -> Design {
    let mut d = Design::default();
    d.add_shape(ShapeKind::Square);
    d.add_shape(ShapeKind::Square);
    let c = d.add_shape(ShapeKind::Circle);
    d.contain(&[c]);
    d.add_text("Hi".into(), crate::text::FontRef::Inter, 1.0, egui::Color32::BLACK, 1.0, 6.0);
    d
}

#[test]
fn every_surface_reads_labels_shortcuts_and_enablement_from_the_table_and_check() {
    let d = mixed();
    for on in [&[][..], &[1], &[1, 2], &[1, 3], &[3], &[4], &[1, 2, 3, 4]] {
        let entries = surfaces(&d, on);
        assert!(entries.len() > 40, "every surface is enumerated");
        for (surface, e) in entries {
            let row = info(&e.command);
            assert_eq!(e.info, row, "{surface}: {:?} carries its table row", e.command);
            assert_eq!(e.info.shortcut, row.shortcut, "{surface}: {:?} shortcut", e.command);
            let live = match &e.command {
                Command::Edit(Edit::Contain) => ["Make container", "Wrap in container"].contains(&e.label),
                _ => e.label == row.label,
            };
            assert!(live, "{surface}: {:?} is labelled {} not {}", e.command, e.label, row.label);
            if let Command::Edit(edit) = &e.command {
                assert_eq!(e.blocked, d.check(edit, on).err(), "{surface}: {edit:?} on {on:?} is enabled by the model's check");
            } else if let Command::Clipboard(action) = &e.command {
                let needs_selection = matches!(
                    action,
                    super::clipboard::Action::Cut | super::clipboard::Action::Copy | super::clipboard::Action::CopyAsImage | super::clipboard::Action::PasteStyle
                );
                assert_eq!(e.blocked.is_some(), needs_selection && on.is_empty(), "{surface}: {action:?} on {on:?}");
            }
        }
    }
}

#[test]
fn group_and_attach_are_off_on_every_surface_for_a_container_and_a_shape() {
    let d = mixed();
    for (surface, e) in surfaces(&d, &[1, 3]) {
        if matches!(e.command, Command::Edit(Edit::Group | Edit::Attach)) {
            assert!(!e.enabled(), "{surface} offers {:?} for a container and a shape", e.command);
        }
    }
    assert!(
        !selection::entries(&Cx::new(&d, &[1, 3], AlignTo::Container)).iter().any(|e| matches!(e.command, Command::Edit(Edit::Group | Edit::Attach))),
        "the action bar leaves them out"
    );
}

#[test]
fn group_is_greyed_in_the_edit_menu_and_the_layers_bar_for_a_container_and_a_shape() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    let c = harness.state_mut().design.contain(&[2]).unwrap();
    harness.state_mut().selection = vec![1, c];
    harness.run();
    assert!(harness.get_by_label("Layers Attach").is_disabled());
    harness.get_by_label("Edit").click();
    harness.run();
    assert!(harness.get_by_label("Group").is_disabled(), "Group is greyed");
    assert!(harness.get_by_label("Attach").is_disabled(), "Attach is greyed");
}

/// Picks `item` from the edit toolbar's `tool` popover.
fn toolbar_pick(harness: &mut Harness<'_, Kerf>, tool: &str, item: &str) {
    harness.get_by_label(tool).click();
    harness.run();
    harness.get_all_by_label(item).last().unwrap_or_else(|| panic!("no {item}")).click();
    harness.run();
}

#[test]
fn toolbar_flip_mirrors_all_three_selected_shapes() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let _ = two_squares_apart(&mut harness);
    let id = harness.state_mut().design.add_shape(ShapeKind::Square);
    (harness.state_mut().design.shape_mut(id).unwrap().x_in) = 7.0;
    harness.state_mut().selection = vec![1, 2, id];
    harness.run();
    let xs = |h: &Harness<'_, Kerf>| h.state().design.shapes.iter().map(|s| s.x_in).collect::<Vec<_>>();
    let before = xs(&harness);
    toolbar_pick(&mut harness, "Flip", "Flip Horizontal");
    let after = xs(&harness);
    assert!(harness.state().design.shapes.iter().all(|s| s.flip_x), "every shape is mirrored");
    assert!((after[0] - before[2]).abs() < 1e-4 && (after[2] - before[0]).abs() < 1e-4, "the outer shapes swap places: {before:?} to {after:?}");
}

fn cmd_z(harness: &mut Harness<'_, Kerf>) {
    harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
}

#[test]
fn undo_during_a_drag_ends_it_and_the_next_drag_is_its_own_step() {
    let mut harness = snapshot_harness(egui::vec2(1400.0, 900.0));
    let at = two_squares_apart(&mut harness);
    let x = |h: &Harness<'_, Kerf>| h.state().design.shapes[0].x_in;
    let start = x(&harness);
    let grab = |x: f32| at((x + 0.5, 1.5));

    drag_along(&mut harness, &[grab(start), grab(start + 0.1), grab(start + 1.0)]);
    let first = x(&harness);
    assert!(first > start + 0.5, "the first drag moves the square");

    move_to(&mut harness, grab(first));
    press_at(&mut harness, grab(first), true);
    move_to(&mut harness, grab(first + 0.1));
    move_to(&mut harness, grab(first + 1.0));
    assert!(x(&harness) > first + 0.5, "the second drag is under way");
    cmd_z(&mut harness);
    assert_eq!(x(&harness), first, "Undo takes back the drag so far");
    move_to(&mut harness, grab(first + 2.0));
    assert_eq!(x(&harness), first, "the held pointer no longer drags");
    press_at(&mut harness, grab(first + 2.0), false);
    assert!(harness.state().history.can_redo(), "nothing was recorded after the Undo");

    drag_along(&mut harness, &[grab(first), grab(first + 0.1), grab(first + 1.0)]);
    assert!(x(&harness) > first + 0.5, "a new drag moves it again");
    cmd_z(&mut harness);
    assert_eq!(x(&harness), first, "the new drag was one step of its own");
    cmd_z(&mut harness);
    assert_eq!(x(&harness), start, "and the first drag was another");
}
