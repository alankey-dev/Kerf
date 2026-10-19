# 23 One command model

**Wave 5. Depends on everything merged (00-20); parallel with 22. Size L.** Source: `docs/audit/2026-10-12-audit.md` AUDIT-005 (root), 006, 013, 018, 023, 026, and the UI rows of 016.

## Problem

About a dozen dispatch paths and action enums (`MenuCommand` design edits, `View.command`, `context_menu::Action`, `selection::Pick`/`Action`, `toolbar::flip::Flip`, `combine::Pick`, `effects::Effect`, `layers_actions::Action`, `RowAction`, TopBar capability flags, `units_in`) re-derive enable rules and unit counting per surface. Results: toolbar Flip only flips the first shape (006); Group and Attach look enabled but do nothing with a container selected; labels and shortcuts disagree across surfaces (023); domain rules live in UI modules (013).

## Goal

One model-side `Edit` command, copying the proven `path_edit::NodeAction` pattern:

- `enum Edit` covering every design edit (delete, duplicate, group, ungroup, attach, detach, contain, uncontain, arrange, flip, align with its target, distribute, combine ops, slice, flatten, unflatten, effects, operation, colour, hide, select-all and so on). App commands (New, Open, Save, Undo, Redo, Make, Preview, view toggles) stay a separate small `AppCommand`.
- `Design::check(&Edit, &Selection) -> Result<(), Blocked>` holds every enable rule once, including the model's own guards (no linking containers, no warping flattened shapes, text limits, position clamp). `Blocked::reason()` gives the disabled tooltip.
- `Design::apply(&Edit, &mut Selection)` performs it. One `units_by` replaces the five unit-partition loops; `snap::Aabb` replaces the bounds 4-tuples.
- One `CommandInfo` table: label, icon, tooltip body, shortcut per `Edit`/`AppCommand`. Every surface (menus, toolbar, selection bar, Layers action bar, context menu, node toolbar where applicable, and the future spec 32 palette) reads labels, shortcuts and enablement from it and dispatches `Edit` values through one function. Delete the per-surface enums, `View.command`, the TopBar capability flags and `units_in`.
- Edits apply directly within the frame; `History::observe` already makes each frame one undo step, so the deferral through `View.command` goes away. Fix undo during an open gesture (018): Undo ends the gesture first.
- Align To lives in `Edit::Align { op, target }`, not egui temp memory (026).

## Files

- **Owns.** `src/edit.rs` (new: `Edit`, `AppCommand`, `Blocked`, `CommandInfo` table), `src/ui/tests_spec23.rs` (new).
- **Shared edits.** `src/design.rs`, `src/containers.rs`, `src/links.rs` (check/apply, `units_by`), `src/ui/mod.rs` (one dispatch), `src/ui/chrome.rs`, `src/ui/toolbar/*`, `src/ui/selection.rs`, `src/ui/context_menu.rs`, `src/ui/layers_actions.rs`, `src/ui/layers_panel.rs`, `src/ui/canvas.rs`, `src/ui/inspector.rs`. Spec 22 runs in parallel touching persistence and the test harness in `ui/mod.rs`; keep away from those.

## Acceptance

- [ ] Unit tests: `check` for every `Edit` across selections of shapes, links, containers, flattened shapes and text, including the container-plus-shape Group case (blocked, with reason).
- [ ] Kittest: toolbar Flip with three shapes selected mirrors all three (006 regression).
- [ ] A test enumerates every surface's entries and asserts each label, shortcut and enabled state comes from `CommandInfo`/`check` (no surface-local rule).
- [ ] Kittest: Cmd+Z during a drag ends the gesture and the next drag is its own undo step.
- [ ] The deleted enums and fields are gone (`rg` finds no `View.command`, `selection::Pick`, `flip::Flip`, `units_in`).
- [ ] All existing tests pass unchanged except where they named deleted types.
- [ ] README module table and Editor sections describe the command model and current UI (AUDIT-016 UI rows).
