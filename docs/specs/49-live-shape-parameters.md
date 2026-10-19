# 49 Live shape parameters

**Wave 11. Depends on 22, 23, 17, 21, 34. Size L.** In Affinity a star keeps its point count and inner radius as fields, a rectangle its corner radius per corner, a polygon its side count, after it is placed. In Kerf every Shapes panel shape is a fixed outline from `shape_library.rs`; a five point star stays five points for life and rounding a rectangle means Offset tricks.

## Problem

- `ShapeKind::Star`, `Pentagon`, `RoundedSquare`, `Arch`, `HalfCircle` and the `Builtin` shapes carry no parameters. `shape_library::star`, `rounded_rect`, `ring` and friends take the parameters at build time and throw them away.
- A six point star, a 3 mm corner radius, a 30 degree arc or a thicker ring each need a new library entry or a drawn path.
- The inspector shows X, Y, W, H and Rotation for every shape and nothing about the shape itself.

## Goal

### Model

- `ShapeKind::Parametric { kind: ParamKind, params: Params }` replaces the fixed polygon, star, rounded, arc and ring kinds. `ParamKind` is Rectangle (corner radii, one per corner, with a link toggle), Ellipse (none), Polygon (sides 3 to 64, corner radius), Star (points 3 to 64, inner radius as a percentage, corner radius for outer and inner points), Arc (start and sweep in degrees, inner radius for a donut sector, closed as a pie or a chord or open as a line), Ring (thickness), Line (none) and Plus (bar width). `Params` holds the values in the shape's local frame as fractions of its size, so resizing keeps the look and a 0.1 in radius on a 1 in square stays round when the square becomes 3 in only if the user asks (a Scale with shape toggle on corner radii, default off, so radii are stored in inches).
- `shape_library::outline(kind, params, size) -> Vec<Contour>` is the one generator, pure, and the existing `star`, `rounded_rect`, `ring` and `poly` helpers move into it. The fixed library entries that these replace (Star, StarThin, Pentagon, Hexagon, Octagon, Triangle, Diamond, Square, Rectangle, RoundedSquare, Circle, Oval, Pill, HalfCircle, Arch, Plus, Line and the builtin rings, frames, bursts and polygons) become `Parametric` presets, and the Shapes panel keeps its tiles, search and categories. Shapes with no parameters (Heart, Equals, the free builtins) keep `Builtin`.
- Projects saved with the old kinds upgrade on open to the `Parametric` preset that draws the same outline, within the 0.01 in flatten tolerance; a test walks every old kind. Bump `FORMAT_VERSION`.
- Corner rounding is a circular arc fitted with the same cubic as `geometry::offset` Round joins, limited to half the shorter adjacent edge so radii can never overlap.

### Inspector and canvas

- A **Shape** section appears under Size in the inspector when every selected shape has the same `ParamKind`, with one field per parameter (expression fields for lengths, count fields for sides and points, degree fields for angles). Mixed values show blank and a typed value applies to all, as the text toolbar does. Each commit is one undo step.
- On the canvas a selected parametric shape shows small diamond handles for its parameters: one at each rectangle corner for its radius (drag inward), one on a star's inner radius, one at each end of an arc. Dragging a handle shows the value beside the pointer and commits on release as one undo step. Handles hide with the other chrome during a drag and never show on a multi-selection.
- Double-clicking a parametric shape, or pressing A, offers node editing as today, which converts it to `ShapeKind::Path` in the same undo step after a notice that the shape's parameters will be lost, with an Undo link. Combine, Offset, Warp, Flatten and Make evaluate the outline and need no change.
- The command palette finds "star points" and "corner radius" as rows that focus the field.

## Files

- **Owns.** `src/parametric.rs` (new: `ParamKind`, `Params`, `outline`, the preset table, the upgrade from old kinds; pure, with the moved helpers), `src/ui/shape_panel.rs` (new: the inspector section), `src/ui/param_handles.rs` (new: the canvas handles), `src/ui/tests_spec49.rs` (new).
- **Shared edits.** `src/design.rs` (`ShapeKind::Parametric`, old variants removed once `project.rs` upgrades them), `src/shape_library.rs` (the panel's tiles read the preset table; the SVG presets stay), `src/project.rs` (the format bump and the upgrade), `src/ui/inspector.rs` (mount the section), `src/ui/selection.rs` (mount the handles), `src/ui/node_edit.rs` (the convert notice), `src/edit.rs` (`Edit::Params`).

## Acceptance

- [ ] Unit test: every old fixed kind upgrades to a preset whose outline matches the old outline within 0.01 in by Hausdorff distance.
- [ ] Unit test: a rectangle with a 0.2 in radius on a 1 x 0.3 in frame clamps the radius to 0.15 in.
- [ ] Unit test: a 7 point star at 40 % inner radius has 14 nodes with alternating radii.
- [ ] Unit test: an arc from 0 to 270 degrees as a pie has a node at the centre; as a chord it does not; open it has `closed: false`.
- [ ] Unit test: resizing a rectangle with Scale with shape off keeps its radius at 0.2 in; with it on, doubling the width doubles the radius.
- [ ] Kittest: insert a star, set Points to 8 in the inspector; the canvas shows eight points and one Undo shows five.
- [ ] Kittest: drag a rectangle's corner handle inward; the corners round and the read-out shows the radius.
- [ ] Kittest: press A on a star; the notice shows, the star becomes a path, and Undo makes it a star again with its Shape section back.
- [ ] Kittest: a project saved before this spec with a Pentagon opens and its inspector shows Sides 5.
- [ ] Snapshots `49-shape-section.png` (star fields), `49-corner-handles.png`, `49-arc.png` (pie, chord and open side by side).
- [ ] `docs/guide.md` gains a Shape parameters paragraph under Editor and the Shapes panel paragraph says which shapes are parametric.
