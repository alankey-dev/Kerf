# 55 Corner tool

**Wave 13. Depends on 17, 21, 49. Size M.** Illustrator's Live Corners and Affinity's Corner tool round, chamfer or scoop any corner of any path by dragging a widget or typing a radius, and the rounding stays editable. Kerf rounds nothing after the fact; Offset's Round join is the only curve a corner can get, and it moves the whole outline.

## Problem

- A traced logo with a sharp spike, a box net with square tabs or a drawn tag needs its corners softened. The route today is Offset out then Offset in, which loses the original and rounds every corner equally.
- Spec 49 gives rectangles and polygons a corner radius, but a `Path` from the Pen, a trace or an import has only Straight, Cusp, Smooth and Symmetric nodes.
- Small radii on cardstock corners keep them from catching and tearing, so this is a cutting need as much as a design one.

## Goal

### Model

- `ShapeKind::Path` gains `corners: Vec<Vec<Corner>>`, one per node matching `node_kinds`, `#[serde(default)]` to none, where `Corner { radius_in: f32, style: Round | Chamfer | Inverted }`. A corner applies to a Straight or Cusp node only, and a node with a corner draws its arc live every time the outline is evaluated, so the underlying node stays sharp and the radius can be changed or removed later. Bump `FORMAT_VERSION`.
- `path_edit::with_corners(contour, corners) -> Contour` is the pure evaluation. Round fits a circular arc tangent to both edges with the same cubic approximation spec 49 uses; Chamfer cuts a straight line between the tangent points; Inverted scoops a concave arc. The radius is limited to half the shorter adjacent edge less any neighbouring corner's take, and the clamp is reported so the UI can show the effective value.
- `Shape::effects`, Combine, Offset, Warp, Make and export see only the evaluated outline, so none change.

### Tool

- **Corner** joins the tool rail (key C, a new 1.5 px icon). Hovering a Straight or Cusp node shows a small circle widget; dragging it inward rounds the corner with the radius shown in the read-out, outward reduces it, and Option-click cycles Round, Chamfer, Inverted. Dragging a widget with several nodes selected rounds them all to the same radius. Each drag is one undo step.
- Dragging over a path with no node selected, or clicking a path, selects it, and a marquee selects nodes as node editing does, so the tool reuses `node_edit`'s selection and drawing.
- With the Corner tool active the edit toolbar shows Radius (an expression field applying to the selected nodes), Style and Round all corners, which rounds every eligible node of the selected shapes to the Radius. Zero removes a corner.
- The Select tool shows no corner widgets; node editing shows a dot under a cornered node and Straight, Cusp, Smooth and Symmetric leave corners alone unless the node becomes Smooth or Symmetric, which removes its corner with a notice.
- Parametric shapes (spec 49) hand their corner radius through this model when converted to a path, so pressing A on a rounded rectangle gives a path with four Round corners of the same radius rather than baked arcs.

## Files

- **Owns.** `src/corners.rs` (new: `Corner`, the clamp, `with_corners`; pure), `src/ui/corner_tool.rs` (new), `src/ui/toolbar/corner.rs` (new), `src/ui/tests_spec55.rs` (new).
- **Shared edits.** `src/design.rs` (`corners` on `Path`), `src/path_edit.rs` (evaluate corners before returning geometry; keep corner vectors in step when nodes are added or removed), `src/ui/canvas.rs` (`Tool::Corner` dispatch; a few lines), `src/ui/node_edit.rs` (the dot and the Smooth notice), `src/parametric.rs` (convert radii to corners), `src/ui/icons.rs`, `src/project.rs` (the format bump), `src/edit.rs` (`Edit::Corners`).

## Acceptance

- [ ] Unit test: a 0.2 in Round corner on a right angle replaces the node with an arc tangent to both edges, and the arc's midpoint lies 0.2 x (sqrt 2 - 1) in inside the corner.
- [ ] Unit test: Chamfer gives a straight segment between the tangent points; Inverted gives an arc on the far side.
- [ ] Unit test: a radius larger than half the shorter edge clamps and reports the effective radius; two corners on a short edge share it.
- [ ] Unit test: adding a node to a cornered path keeps every other node's corner; removing a cornered node drops its corner only.
- [ ] Unit test: converting a rounded rectangle from spec 49 to a path gives four Round corners equal to its radius and the same outline within 0.01 in.
- [ ] Unit test: a project saved before corners loads with none.
- [ ] Kittest: Corner tool, drag a square's corner widget inward; the corner rounds, the read-out shows the radius, one Undo squares it.
- [ ] Kittest: select two nodes, type `3mm` in Radius; both round to 3 mm.
- [ ] Kittest: Round all corners on a star; every point rounds and Make's preview cuts the rounded outline.
- [ ] Snapshots `55-corner-widget.png`, `55-three-styles.png`.
- [ ] `docs/guide.md` adds the Corner tool to Drawing tools and the key C to the tool list.
