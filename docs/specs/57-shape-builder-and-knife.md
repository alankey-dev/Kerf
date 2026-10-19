# 57 Shape Builder and Knife

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec57.rs` pass.

**Wave 13. Depends on 04, 17, 21, 47. Size L.** Illustrator's Shape Builder merges and deletes the regions where shapes overlap by dragging across them, and Affinity's Knife and Illustrator's Scissors cut a path where you draw. Kerf's Combine tiles do the booleans on whole shapes and Slice cuts two shapes by each other; neither takes a gesture, and nothing cuts a single path.

## Problem

- Making a cloud from six circles is six Unites; keeping the overlap of two and removing the rest is three operations and a Slice. Each is a trip to the Combine popover.
- Slice needs exactly two units and gives three pieces. Cutting one path along a line, or snipping an open path in two at a point, cannot be done.
- Shape Builder is the tool most often named when people describe what they miss moving from Illustrator to a cutter app.

## Goal

### Shape Builder

- **Shape Builder** on the tool rail (key Shift+M, Illustrator's key). With two or more shapes selected, it computes the planar map of their evaluated outlines once per selection change through `geometry` (every region bounded by the shapes' edges, with which shapes cover it) and shows the region under the pointer with a hatched accent fill.
- Dragging across regions unites them into one shape in the front-most input's colour and operation; a click on one region makes it its own shape. Option-drag or Option-click deletes regions. Each release is one undo step, and what is left of the inputs is split into shapes region by region, so Shape Builder is a destructive operation like Expand, not a live group. The planar map is rebuilt after each step.
- Open paths, text and bitmaps take part through their evaluated outlines, which Kerf already flattens; a single-line text has no area and is refused with the message spec 29 uses.
- Escape or another tool leaves it. With fewer than two shapes selected the tool shows the reason in its tooltip and does nothing.
- `geometry::planar_map(outlines) -> Vec<Region { ring, covered_by: Vec<usize> }>` is the pure piece and the one to test hard. Build it from `i_overlay`'s overlay graph if version 9 exposes it, else from pairwise booleans over the power set, which is fine up to the eight inputs the tool caps at (the tooltip says "Select up to 8 shapes").

### Knife

- **Knife** on the tool rail (key K). Drag a freehand line, or Shift-drag a straight one, across one or more selected shapes, or across everything under the line when nothing is selected. On release every closed shape the line crosses is split along it into two or more shapes, each keeping the source's colour, operation, effects and name with " 2", " 3" appended, and selected. An open path is cut at each crossing into separate open paths. A line that crosses nothing does nothing. One undo step.
- The cut uses `geometry` booleans with the line thickened to a hairline band, so a cut edge lands within 0.001 in of the drawn line and the two halves share it. Effects on the source are expanded first with the same notice Flatten gives, since a live Offset cannot follow half a shape.
- **Scissors** is the Knife with a click instead of a drag: click a point on a path's edge to open a closed path there, or cut an open one in two, snapping to nodes through spec 47. It is the same tool with no drag, so it needs no second rail item.
- A live group or a container is cut as its evaluated outline and replaced by the pieces; an instance is detached first with a notice.

## Files

- **Owns.** `src/geometry/planar.rs` (new: `planar_map`; pure), `src/knife.rs` (new: the band, the split, the open-path cut; pure), `src/ui/shape_builder.rs` (new), `src/ui/knife_tool.rs` (new), `src/ui/tests_spec57.rs` (new).
- **Shared edits.** `src/geometry.rs` (becomes `src/geometry/mod.rs` re-exporting as before), `src/ui/canvas.rs` (two `Tool` variants), `src/ui/icons.rs`, `src/edit.rs` (`Edit::BuildShape`, `Edit::Knife`), `src/ui/commands.rs`, `src/ui/chrome.rs`.

## Acceptance

- [ ] Unit test: the planar map of two overlapping circles has three regions, covered by {0}, {1} and {0, 1}, with areas summing to the union's area within 1e-4 in².
- [ ] Unit test: the planar map of three circles in a triangle has seven regions; of two disjoint squares two.
- [ ] Unit test: uniting the three regions of two circles gives the same outline as Unite within 0.01 in.
- [ ] Unit test: deleting the lens region of two circles leaves two crescents whose areas equal each circle less the lens.
- [ ] Unit test: a straight knife line across a square gives two rectangles whose shared edge lies on the line within 0.001 in; a wavy line gives two pieces whose areas sum to the square.
- [ ] Unit test: a knife across an open three-segment path at two points gives three open paths with the original's nodes.
- [ ] Unit test: a scissors click on a closed path opens it at the nearest node and the result has `closed: false`.
- [ ] Unit test: a knife over a shape with a live Offset expands it first and the pieces carry no effects.
- [ ] Kittest: select two overlapping circles, Shape Builder, drag across all three regions; one shape results and Make shows one piece. One Undo restores two circles.
- [ ] Kittest: Option-click the lens; two crescents remain.
- [ ] Kittest: Knife, Shift-drag across a heart; two shapes named Heart and Heart 2 are selected.
- [ ] Snapshots `57-shape-builder-hover.png` (hatched region), `57-knife.png` (mid-drag line and the result).
- [ ] `docs/guide.md` adds Shape Builder and Knife to Drawing tools with their keys and the eight shape cap.
