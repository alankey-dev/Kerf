# 30 Nesting: Optimise layout on mats

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec30.rs` pass.

**Wave 7. Depends on 22, 23, 09, 18, 21. Size L.** Kerf packs pieces in a fixed grid and leaves the user to drag them closer on each mat. Users ask for true nesting so expensive material (vinyl, foil, leather) is not wasted.

## Problem

`composition::compose` packs units left to right and top to bottom by their bounding boxes, a quarter inch apart. A ring, an L shape or a set of letters wastes most of its bounding box. Nothing rotates, so a long strip that would fit sideways spills onto another mat.

## Goal

**Optimise layout** in Prepare packs every mat's pieces as tightly as their real outlines allow, shows how much material it saves, and is one undo step.

### Units and constraints

- A nesting **piece** is a composition unit as spec 09 and spec 18 define it (`containers::placement_key`): an Attach link, a cut container with its children, or a single shape. A piece moves and rotates rigidly; its members keep their relative positions.
- Geometry is the evaluated outline from spec 21 (offsets and stickers included), flattened with `geometry.rs`. Pen, Score and Foil members of a piece count as part of it, so drawing stays on the piece it belongs to.
- **Rotation**: a Prepare setting, None, 90 degree steps (default) or Free (a step chosen from 5, 10, 15 or 45 degrees).
- **Grain lock** per shape: `Shape.grain_lock: bool` (`#[serde(default)]`), set from the inspector with a small lock-grain toggle. A piece with any grain-locked member may only turn 0 or 180 degrees. Use it for wood veneer, faux leather and directional glitter.
- **Spacing**: minimum gap between pieces, default 0.125 in, through `units.rs`. The mat edges keep the existing 18 pt padding.
- **Use holes** (default on): small pieces may be placed in the holes of larger ones, such as letters inside a frame.
- Mats stay keyed by colour and material as spec 09 decides; nesting never moves a piece to a mat of another colour. It may reduce the number of mats of one colour.

### Algorithm: raster bottom-left fill

Use a raster nesting first; leave no-fit polygons for a later spec if the raster result is not good enough.

- Each piece, in each allowed rotation, is rasterised with `tiny-skia` (already a dependency) onto a bit grid at a cell size of `spacing / 2`, clamped to 0.02 to 0.05 in, after dilating its outline by `spacing / 2` with `geometry::offset` (round). Two dilated pieces that do not overlap are therefore at least `spacing` apart.
- The mat is a bit grid of its cutting area less padding. Rows are `u64` words, so a collision test is a shifted AND per row.
- Placement is **bottom-left fill** in mat space (top-left on screen, the mat origin): for each piece, for each rotation, take the first free position scanning rows then columns; keep the rotation with the smallest resulting used height, then smallest x. Pieces go in order of decreasing area.
- **Improvement**: within a time budget (1.5 s by default) try further orders made by swapping neighbours of the greedy order, using a seeded RNG so a given design always nests the same way. Keep the best by (mats used, used length of the last mat, total used length).
- Results are snapped back to exact geometry: placement offsets are in inches and the rotation exact, so the cut paths themselves are never rasterised.

Why raster over no-fit polygons: holes, concave shapes and multi-contour pieces need no special cases, spacing is a dilation, Use holes comes free, and the code is small and fast enough with bit rows. No-fit polygons (as in SVGnest, MIT) give exact contact and better results on tight curves, but computing them robustly for concave shapes with holes on top of `i_overlay` is a large job. The raster loses at most one cell (0.05 in or less) per contact, which is under the default spacing. If a measured case shows a real loss, a later spec can swap the placement core for no-fit polygons behind the same `nest()` function.

### Pure interface

`src/nesting.rs`: `nest(pieces: &[Piece], mat: MatArea, settings: &NestSettings, budget: Duration, cancel: &AtomicBool) -> NestResult`, where `NestResult` lists per mat the placement `(piece key, x_in, y_in, rotation_deg)` and the used length. No UI types.

### Prepare UI

- Prepare's mat list gains **Optimise layout** above the mats, with a small settings popover (Rotation, Spacing, Use holes).
- Running it shows a progress bar and Cancel; nesting runs on a worker thread and never blocks the UI.
- When it finishes, a summary card reads "Material: 24 in to 15.5 in, 35 % less" (lengths through `units.rs`), plus mats before and after when the count changes. Prepare shows the nested mats immediately.
- **Undo optimise** on the card, and Cmd+Z, restore the previous layout in one step.

### Storage and validity

- The result is stored on the design, `Design.nesting: Option<NestedLayout { fingerprint, settings, mats }>` (`#[serde(default)]`), so it saves with the project, survives reopening and is covered by the normal undo history. Spec 23's command path applies it as one undo step.
- `fingerprint` hashes every piece's evaluated geometry, members, operation, colour and the mat size. `compose` uses the stored placements only when the fingerprint matches; otherwise it falls back to today's packing and Prepare shows "The design changed since the layout was optimised" with **Optimise again**.
- `bridge.rs` already places paths with a `PbMatrix`; placements with rotation fill in its rotation terms. Mirror applies after nesting.

## Files

- **Owns.** `src/nesting.rs` (new: rasterising, bit grids, placement, improvement loop; pure), `src/nesting/worker.rs` (new), `src/ui/prepare_nest.rs` (new: button, settings, progress and summary card), `src/ui/tests_spec30.rs` (new).
- **Shared edits.** `src/composition.rs` (use a matching `NestedLayout`; expose pieces and the fingerprint), `src/design.rs` (`Design.nesting`, `Shape.grain_lock`), `src/bridge.rs` (rotation in the placement matrix), `src/ui/prepare.rs` (mount the section), `src/ui/inspector.rs` (grain lock toggle), `src/ui/mod.rs` (`mod` lines). Spec 09's files need the most review here.

## Acceptance

- [ ] Unit test: six 1 in squares and one upright 1 x 4 in strip on a 12 x 12 mat nest into a used length of no more than 2.5 in with 0.125 in spacing, against more with today's packing. (A flat 4 x 1 strip packs into one row either way.)
- [ ] Unit test: no two placed pieces are closer than the spacing, measured on exact geometry (minimum ring distance), across a fixture of 30 mixed shapes.
- [ ] Unit test: a 12 x 1 in strip, wider than a 12 x 24 mat's 11.5 in cutting width, overflows with Rotation None and is placed at 90 or 270 degrees when rotation allows 90 degree steps.
- [ ] Unit test: a grain-locked strip only takes 0 or 180 degrees.
- [ ] Unit test: with Use holes on, a 1 in circle lands inside a 6 in square frame's hole; off, it does not.
- [ ] Unit test: an Attach link and a cut container with children nest as rigid units; members keep their offsets within 0.001 in.
- [ ] Unit test: nesting the same design twice with the same seed gives identical placements; cancel returns the best result so far.
- [ ] Unit test: changing a shape after optimising changes the fingerprint and `compose` falls back to packing.
- [ ] Unit test: an old project loads with no nesting and no grain lock.
- [ ] Unit test: a rotated placement's payload matrix rotates the path as the preview shows.
- [ ] Kittest: build a design of mixed shapes, Make, Optimise layout; the summary shows a smaller length; Undo optimise restores the old layout.
- [ ] Kittest: optimise, return to the canvas, move a shape, Make; Prepare shows the "design changed" notice.
- [ ] Snapshots `30-before.png` and `30-after.png` (the same mat before and after), `30-summary.png` (summary card).
- [ ] README describes Optimise layout, rotation, spacing, grain lock and Use holes.
