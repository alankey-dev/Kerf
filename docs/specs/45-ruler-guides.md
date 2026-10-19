# 45 Ruler guides

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec45.rs` pass.

**Wave 10. Depends on 22, 23, 34. Size M.** Affinity and Illustrator let you drag a guide line out of a ruler, snap to it, lock it and type its position. Kerf has rulers and smart guides that appear during a drag, but no guide you can place and keep.

## Problem

- Laying out a card's fold line, a safe margin or a row of tags means placing a hidden rectangle as a stand-in. Kerf's Guide operation helps, but a Guide shape is still a shape in Layers, goes through Make's composition and is easy to move by accident.
- The rulers (`canvas::paint_rulers`) are display only; dragging on them does nothing.
- `snap.rs` snaps to shapes, the grid and the mat centre. There is no user-placed target.

## Goal

### Model

- `Design::guides: Vec<RulerGuide>`, `#[serde(default)]`, where `RulerGuide { id: u64, axis: Axis, at_in: f32, locked: bool }` and `Axis` is `Vertical` (a world x) or `Horizontal` (a world y). Guides are part of the design so they save, autosave, version and undo like everything else, and they are per project. Bump `FORMAT_VERSION`; older projects load with none.
- Guides take no part in Make, composition, nesting, export, thumbnails, Flatten or Combine. `export::sheets` and `composition::compose` never see them. The Project Preview and Prepare mats do not draw them.
- `Design::guides_locked: bool` locks every guide at once, as View > Lock Guides does in Illustrator.

### Canvas

- Press on a ruler and drag onto the artboard to pull a new guide out; it follows the pointer, snapping to the grid and to shape edges through `snap.rs`, and lands on release. Releasing back over the ruler, or off the artboard, cancels. The guide spans the whole artboard as a 1 px line in the accent colour at 60 % and draws above shapes and below the selection frame.
- With the Select tool, hovering a guide within 4 px shows a resize cursor on its axis. Dragging moves it with the same snapping. Dragging it onto its ruler deletes it. A locked guide shows no cursor and is passed through to the shape beneath, so locked guides never steal a click. A guide never selects as a shape and never appears in Layers.
- Double-click a guide to type its position in an expression field (`units::length_field`) in a small popover, with a Lock toggle and Delete. Right-click offers the same three plus Lock all guides and Clear guides.
- The ruler corner's context menu gains Show guides, Lock guides and Clear guides. View gains the same three rows, with Show guides on Cmd+; and Lock guides on Cmd+Option+;. Clear guides is one undo step that removes every guide.
- A guide's position shows in the ruler as a small accent marker while it is hovered or dragged, and the drag read-out beside the pointer shows its position in the current units.

### Snapping

Guides become snap targets for everything `snap.rs` serves. Moving or resizing a shape snaps its edges and centre to a guide, and the smart guide line drawn is the guide itself lit at full accent for the frame. Pen and Pencil anchors, the Measure tool ends and node drags snap to guides too. Spec 47 gives the per-target toggles; until then a guide snaps whenever magnetic snap is on.

## Files

- **Owns.** `src/guides.rs` (new: `RulerGuide`, `Axis`, hit testing, the move and delete rules; pure), `src/ui/guides.rs` (new: drawing, ruler drag, the position popover and the menus), `src/ui/tests_spec45.rs` (new).
- **Shared edits.** `src/design.rs` (`guides`, `guides_locked`, the format bump), `src/snap.rs` (guide positions as targets in `snap_move`, `snap_point` and `snap_to_feature`), `src/ui/canvas.rs` (a hook before shape hit testing and one after `paint_grid`; keep each to a few lines), `src/edit.rs` (`Edit::Guide(GuideEdit)` with Add, Move, SetLocked, Delete, LockAll, Clear, Show), `src/ui/commands.rs`, `src/ui/chrome.rs` (View rows), `src/project.rs` (the format bump and the upgrade test).

## Acceptance

- [ ] Unit test: a guide at 3.0 in is hit at 3.02 in at 100 % zoom and not at 3.2 in; a locked guide is never hit.
- [ ] Unit test: `snap_move` of a box whose left edge is 0.01 in from a vertical guide snaps to the guide and reports it; the mat centre and shape edges still win when nearer.
- [ ] Unit test: a project saved at format 11 loads with no guides; one saved with guides round-trips them.
- [ ] Unit test: `composition::compose` and `export::sheets` give the same result with and without guides.
- [ ] Kittest: drag from the top ruler down to 2 in; a horizontal guide appears at 2 in and one Undo removes it.
- [ ] Kittest: drag a shape so its top edge nears the guide; it snaps and the guide lights.
- [ ] Kittest: double-click the guide, type `1in + 5mm`, Enter; the guide moves and the popover shows the evaluated value.
- [ ] Kittest: Lock guides, then click where the guide crosses a shape; the shape is selected and the guide does not move.
- [ ] Kittest: drag the guide back onto its ruler; it is deleted.
- [ ] Snapshots `45-guides.png` (two guides, one hovered with its ruler marker), `45-guide-popover.png`.
- [ ] `docs/guide.md` gains a Guides paragraph under Editor and the Units section notes that guide positions follow the units setting.
