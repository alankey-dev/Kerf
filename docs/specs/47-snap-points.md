# 47 Snap points

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec47.rs` pass.

**Wave 11. Depends on 45, 46, 17, 34. Size M.** Affinity's snapping manager and Illustrator's Smart Guides snap to nodes, midpoints, intersections and angles, and let you choose which. Kerf snaps a moving box's edges and centre to other boxes, the grid and the mat centre. Nothing snaps to a path's corner, a segment's middle or a 45 degree line.

## Problem

- `snap::snap_move` sees only bounding boxes, so a rotated shape, a star or a drawn path snaps by a frame that touches none of its points.
- `snap::snap_point` (Pen, Pencil, node drags) and `snap::snap_to_feature` (Measure) each have their own target list and threshold. Three functions, three behaviours.
- Shift during a drag frees or keeps the aspect; nothing constrains a move to an axis or a 45 degree line, and the Pen has no angle constraint.
- There is one magnet toggle. A user who wants guides but not shape edges has no choice.

## Goal

### One snapper

- `snap.rs` gains `Targets`, built once per drag from the design, the guides (spec 45) and the grid (spec 46), and one function `snap(candidate: &Candidate, targets: &Targets, options: &SnapOptions) -> Snapped`. A `Candidate` is the thing moving, which is a set of points with roles (edges and centre of a frame, a path's nodes, a single pointer) and the function finds the best correction so one candidate point meets one target within the threshold, preferring the smaller correction and, on a tie, node over midpoint over edge over grid. `snap_move`, `snap_point` and `snap_to_feature` become thin callers and then go, per the migrate-then-delete rule.
- Targets are the other shapes' bounding-box edges and centres (today's), their path nodes and segment midpoints (from `geometry::flatten` on the evaluated outline, cached per shape while it does not change), the intersections of straight segments between two shapes within the viewport, ruler guides, grid lines and the mat's edges, centre and corners. Point targets are gathered per frame only for shapes whose box lies within the viewport plus the threshold, so a 2000 shape design does not pay for off-screen points.
- The candidate for a moving selection is its frame's edges and centre plus the nodes of every path and text in it, so the corner of a rotated rectangle meets the corner of another. Resizing a frame offers the dragged edge or corner only.
- `Snapped` reports what met what, so the canvas draws a small cross at a point snap, today's line at an edge or guide snap, and a short line between the two when a distance was equalised.
- Equal spacing. When a moving shape sits between two others on an axis, the gap to each is a candidate and the target is the other gap, so dragging a third tag into a row snaps to the same gap as the first two, with the gap drawn as two tick marks. Affinity calls this spacing snaps.

### Angle constraints

- Shift while moving constrains the move to the nearer of horizontal, vertical or 45 degrees from the start point, shown as a faint ray. Shift while resizing keeps today's aspect behaviour. Shift while rotating keeps 15 degree steps.
- Shift while placing a Pen anchor constrains the segment to 45 degree steps from the last anchor. Shift while dragging a node constrains along its own axis or 45 degrees; dragging a handle with Shift snaps the handle angle to 45 degree steps.
- `SnapOptions::angle_step_deg` is 45 by default and set from the Snapping popover (15, 30, 45, 90).

### Snapping popover

- The magnet button's click still toggles everything. A chevron beside it, and View > Snapping..., opens a popover with a toggle per target class (Grid, Guides, Shape edges and centres, Nodes and midpoints, Intersections, Equal spacing, Mat) and the angle step. The choices live on `Design::snap: SnapOptions`, `#[serde(default)]` to today's behaviour plus nodes and guides on, so the project remembers them and a kittest can set them.
- Every toggle has a `widget_info` label and the popover's rows are commands in the registry, so the palette finds "snap nodes".

## Files

- **Owns.** `src/snap.rs` (rewritten around `Targets`, `Candidate` and `snap`; pure, with the point cache in `src/snap/cache.rs`), `src/ui/snap_panel.rs` (new), `src/ui/snap_draw.rs` (new: crosses, rays, gap ticks), `src/ui/tests_spec47.rs` (new).
- **Shared edits.** `src/design.rs` (`snap: SnapOptions`), `src/ui/canvas.rs` (the drag path calls `snap` and draws through `snap_draw`; the Pen and node tools pass Shift), `src/ui/node_edit.rs` and `src/ui/measure_tool.rs` (call `snap`), `src/edit.rs` (`Edit::Snap(SnapOptions)`), `src/ui/commands.rs`, `src/ui/chrome.rs`.

## Acceptance

- [ ] Unit test: a rotated square's corner 0.01 in from another path's node snaps corner to node, and the report names both as nodes.
- [ ] Unit test: a node snap beats a grid snap at equal distance; a nearer grid snap beats a farther node snap.
- [ ] Unit test: a shape between two others 1 in apart, dragged to 1.02 in from one, snaps to 1 in and reports equal spacing.
- [ ] Unit test: Shift constrains a (0.9, 1.0) move to (0, 1.0) and a (1.0, 0.9) move to (1.0, 1.0) scaled to 45 degrees.
- [ ] Unit test: with Nodes off, the rotated square case snaps by its frame as today.
- [ ] Unit test: target gathering with 2000 shapes and a viewport over 20 of them collects points from at most those 20 plus the threshold margin.
- [ ] Unit test: the point cache is reused while a shape is unchanged and rebuilt after a node edit.
- [ ] Kittest: drag a star until its top point nears a heart's node; a cross appears at the meeting point and the positions coincide within 1e-3 in.
- [ ] Kittest: Shift-drag a shape diagonally; it moves on the 45 degree ray and the ray is drawn.
- [ ] Kittest: Pen tool, click, Shift-click at a shallow angle; the segment is horizontal.
- [ ] Kittest: open the Snapping popover, turn off Guides; a drag past a guide no longer snaps to it.
- [ ] Snapshots `47-snap-node.png` (cross at a node snap), `47-snap-spacing.png` (gap ticks), `47-snap-panel.png`.
- [ ] `docs/guide.md` replaces the snapping sentence under Editor with a Snapping paragraph naming the targets, Shift constraints and the popover.
