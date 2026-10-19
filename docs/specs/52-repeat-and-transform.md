# 52 Step and repeat, rotate copies, skew and pivot

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec52b.rs`, `src/ui/tests_spec52.rs` pass.

**Wave 12. Depends on 18, 22, 23, 31, 34. Size M.** Illustrator's Transform Each, Transform Again and Rotate with Copy, and Affinity's Power Duplicate, make forty tags in a grid or twelve petals round a centre in two actions. Kerf has Duplicate (0.25 in right and down), containers with Row, Column and Grid layouts, and no skew, no movable pivot and no way to repeat a transform.

## Problem

- Twelve petals round a centre is eleven duplicates, eleven rotations typed by hand and eleven moves.
- Duplicate's offset is fixed. Affinity remembers the offset and rotation of the last duplicate and applies it again on the next, which is the whole trick.
- Rotation is about the frame centre only. A hinge, a clock hand or a petal needs a pivot at an edge or outside the shape.
- There is no skew. An isometric box face or a slanted label needs a path drawn by hand.

## Goal

### Power Duplicate

- Cmd+D stays Duplicate. If the user then moves, rotates, resizes or flips the copy and presses Cmd+D again, the new copy repeats the same delta from the last copy, so Cmd+D, move, Cmd+D, Cmd+D, Cmd+D lays out a row at an even pitch. The delta is the transform between the last duplicate's placement and its position now, kept on the document as `DuplicateMemory { from: Transform, to: Transform, ids }` and cleared when the selection changes to something else. Each press is one undo step.
- **Transform Again** (Cmd+Shift+D) applies the last transform (move, rotate, resize, flip or skew) to the current selection without copying, as Illustrator does.

### Step and repeat

- **Step and repeat...** (Edit menu, context menu, palette) opens a popover with Copies, Offset X and Y (expression fields), Rotation step, Scale step (percent) and a Grid toggle that adds Columns and a second offset, with a live preview of the copies in faint accent outlines while the fields are edited. Apply makes the copies as plain shapes in one undo step, grouped with the source. Cancel or Escape makes nothing.
- **Rotate copies...** in the same menu takes Copies, Total angle (360 by default) and the pivot, previewing the ring. Apply makes a Group.
- Containers gain a **Radial** layout beside Row, Column and Grid (`containers.rs`), with Radius, Start angle, Sweep and Rotate items, for the live version where the children stay editable and a change to one child does not break the ring. Step and repeat with Grid on offers "Make a Grid container instead" for the same reason.

### Pivot

- With one object selected, the rotate handle's centre cross becomes draggable. Dropping it sets `Shape::pivot: Option<Pt>` in the shape's local frame, shown as a small crosshair; it snaps to the frame's corners, edge midpoints, centre and to spec 47's targets. Rotation on the canvas, typed Rotation in the inspector and Rotate copies turn about it. Double-click the crosshair resets it. A pivot saves with the shape and projects before it load with none. Multi-selections rotate about the frame centre or, when every member has the same pivot in world space, about that point.

### Skew

- The inspector's transform section gains Skew X and Skew Y in degrees, -89 to 89, as expression fields. Skew is stored as `Shape::skew_deg: (f32, f32)` and applied in `transform.rs` between scale and rotation, so the selection frame draws as a parallelogram with its handles on it and resizing still works along the skewed axes. Holding Cmd while dragging a side handle skews instead of resizing, as Affinity's shear handle does.
- Everything that evaluates an outline (`geometry`, Combine, Offset, Make, export) receives the skewed outline through the same matrix it uses for rotation today, so skew needs no change downstream. Text keeps its own 12 degree italic shear separate.
- Bitmaps and instances skew as rigid pictures; a warped shape skews after the warp.

## Files

- **Owns.** `src/repeat.rs` (new: `DuplicateMemory`, step and repeat, rotate copies, the radial layout maths; pure), `src/ui/repeat_panel.rs` (new: the two popovers and their previews), `src/ui/pivot.rs` (new: the crosshair drag), `src/ui/tests_spec52.rs` (new).
- **Shared edits.** `src/design.rs` (`pivot`, `skew_deg`), `src/transform.rs` (skew in the matrix, pivot in rotation, parallelogram frame), `src/containers.rs` (`Layout::Radial`), `src/ui/container_panel.rs` (the Radial fields), `src/edit.rs` (`Edit::Duplicate` reads the memory, `TransformAgain`, `StepRepeat`, `RotateCopies`, `Pivot`, `Skew`), `src/ui/inspector.rs` (Skew fields), `src/ui/selection.rs` (pivot handle, Cmd skew), `src/ui/commands.rs`, `src/ui/chrome.rs`, `src/ui/context_menu.rs`.

## Acceptance

- [ ] Unit test: Duplicate, move the copy by (1, 0), Duplicate twice; the four shapes sit at x = 0, 1, 2, 3.
- [ ] Unit test: Transform Again after a 30 degree rotation rotates the selection another 30 degrees about the same pivot.
- [ ] Unit test: Rotate copies with 12 copies over 360 degrees places centres on a circle at 30 degree steps within 1e-4 in.
- [ ] Unit test: a Radial container with four children of equal size places them at 0, 90, 180 and 270 degrees and rotates each to face outward when Rotate items is on.
- [ ] Unit test: a pivot at a frame's corner rotates the opposite corner by the expected arc; the matrix equals rotate-about-point.
- [ ] Unit test: a 30 degree Skew X on a square gives a parallelogram whose top edge is offset by height x tan 30 and whose area is unchanged; Combine of two skewed squares matches the boolean of the skewed outlines.
- [ ] Unit test: a project saved before this spec loads with no pivot and zero skew.
- [ ] Kittest: select a petal, Rotate copies, 8 copies, Apply; eight petals form a ring and one Undo removes them.
- [ ] Kittest: drag the centre cross to a corner, drag the rotate handle; the shape turns about that corner.
- [ ] Kittest: type 20 into Skew X; the frame leans and the W field reads back unchanged.
- [ ] Kittest: Step and repeat, 3 copies, Offset X `1in`; three copies appear at 1 in pitch, grouped with the source.
- [ ] Snapshots `52-rotate-copies.png` (the popover with its preview), `52-step-repeat.png`, `52-skew.png`, `52-pivot.png`.
- [ ] `docs/guide.md` describes Power Duplicate, Transform Again, Step and repeat, Rotate copies, the Radial layout, the pivot and skew.
