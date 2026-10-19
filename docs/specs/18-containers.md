# 18 Containers

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec18.rs` pass.

**Wave 3. Depends on 02, 03, 06, 09, 11, 13. Size L.** Requested directly by the user: "can we have containers? so distribution etc works with objects within the containers?"

## Problem

Align and Distribute (spec 03) only work relative to the selection's own bounds. There is no way to say "centre these three hearts on this card" or "space these letters evenly across this 6 in strip" except by hand. Links (spec 02) are flat (one level, Group or Attach) and have no bounds of their own.

## Goal

A **container** is a rectangular frame object that owns child objects. Align and Distribute can use the container as their reference, and a container can optionally lay its children out automatically.

### Model (revised: any shape can be a container)

The user chose this model: **a container is a role any shape can take**, not a separate kind of object. A heart, a circle, a card rectangle, a text object or a guide rectangle can each become a container and hold children.

- Add an optional container role to `Shape`, for example `#[serde(default)] container: Option<ContainerRole { padding_in, layout }>`, plus one source of truth for parenthood (a `parent: Option<u64>` on children, or a child list on the role; architect chooses, no duplication). A child may itself be a Group or Attach link (the whole link moves in as one unit). Nesting: a child may itself be a container; state the depth rule chosen.
- The container's reference box is its shape's world bounds (the spec 06 frame for a single shape), inset by padding. For a non-rectangular container (circle, heart) Align and Distribute still use that bounding box; Centre therefore centres on the bounding box centre.
- The container's operation is untouched: whatever the shape is (Basic Cut, Pen, Guide, Print Then Cut) it stays. A Guide shape acting as a container is the "layout frame" case; a cut card acting as a container is the "base piece" case.
- Moving, rotating or deleting a container moves, rotates or deletes its children. **Remove container** clears the role and keeps both the shape and the children where they are.
- Back-compat: `#[serde(default)]` on every new field; a test loads a project saved before this spec.
- Canvas: a shape with the container role shows a small container badge on its Layers row and, while selected or while something is dragged over it, a 1px dashed `ACCENT_PRIMARY` inner-padding outline.

### Creating and filling

- **Make container** (context menu, Edit menu, Cmd+Option+G) on **one** selected shape gives that shape the container role.
- **Wrap in container** on **two or more** selected objects creates a new rectangle fitted to their bounds plus the default padding (0.25 in), with the Guide operation, gives it the container role, and makes the selection its children.
- Dragging an object onto any container (pointer inside its bounds on release) adds it as a child, with a highlight on the target while hovering; dragging a child out past the bounds removes it. Holding Cmd while dragging suppresses reparenting. When containers overlap, the front-most one under the pointer wins.
- Layers (spec 02 parent rows): a container is a parent row with its children indented under it. Dragging rows in or out of containers belongs to spec 20 (Layers drag and drop); expose a reparent function it can call.

### Align and Distribute with containers

- When every selected unit is a child of the same container, the Align popover (spec 03) shows an **Align to** toggle: **Selection** or **Container** (default Container). With Container, Left aligns to the container's inner left edge (frame minus padding), Centre to its centre, and so on. This works with a single selected child (e.g. centre one heart on the card).
- **Distribute** with Align to Container spaces the selected units evenly between the container's inner edges (the first and last units touch the padding edges; equal gaps between), and is enabled from **2** units, not 3.
- Selecting the container itself and choosing Align or Distribute acts on all its children against the container.
- Each action is one undo step.

### Auto layout (optional per container)

A container's inspector section has **Layout**: None (default), Row, Column, or Grid, with Gap (in), Padding (in), alignment on the cross axis (start, centre, end), and Grid columns. When Layout is not None:
- Children are positioned by the layout every time the container or its children change (adding, removing, resizing a child, reordering in Layers, changing gap). Users cannot drag a child's position (dragging reorders it within the layout instead); they can still resize and rotate it.
- **Hug** or **Fixed** size: Hug resizes the container to fit its laid-out children plus padding; Fixed keeps the container size and distributes the children within it (space between).
- Layout is pure and unit tested: `layout(container, children) -> Vec<(id, x_in, y_in)>`.

### Make

Children are composed onto mats like any objects (spec 09). A container is made according to its own operation: a Guide container contributes nothing; a cut container is cut like any shape. Children of a cut container go on the same mat as it, keeping their relative positions (as an Attach link does), so a card base and its decorations cut together.

## Files

- **Owns.** `src/containers.rs` (new: container role, reparenting, pure layout), `src/ui/container_panel.rs` (new: inspector Layout section), `src/ui/tests_spec18.rs` (new).
- **Shared edits.** `src/design.rs` (the container role and parent field on `Shape`, every `Shape` literal; align/distribute reference), `src/ui/toolbar/align.rs` (Align to toggle), `src/ui/layers_panel.rs` (container parent rows, drag in/out), `src/ui/canvas.rs` (drop-into highlight and reparent on drag end, dashed frame painting; keep small), `src/ui/context_menu.rs` (Make container, Wrap in container, Remove container), `src/ui/chrome.rs` (Edit menu item and shortcut), `src/composition.rs` (skip Guide containers), `src/ui/inspector.rs` (mount the Layout section).

## Acceptance

- [ ] Unit test: Align Left to Container puts each child's left edge on the container's inner left edge.
- [ ] Unit test: Distribute horizontally to Container with three 1 in children in a 6 in container with 0.5 in padding gives gaps of 1.0 in ((6 - 2 x 0.5 - 3 x 1) / 2) and touches both padding edges.
- [ ] Unit test: Row layout with gap 0.25 in and Hug sizes the container to children plus gaps plus padding; Fixed with space-between spreads them.
- [ ] Unit test: a grouped pair inside a container aligns and lays out as one unit and keeps its internal offset.
- [ ] Unit test: moving and rotating a container moves and rotates its children; Remove container keeps children in place.
- [ ] Unit test: a Guide container contributes nothing to `compose`; a Basic Cut container (for example a circle) is cut and its children share its mat at their relative offsets.
- [ ] Unit test: an old project loads with no containers.
- [ ] Kittest: right-click a heart, Make container, drag a small star onto it, Align to Container Centre centres the star on the heart; Remove container keeps both shapes.
- [ ] Kittest: select three shapes, Wrap in container, then Align to Container Centre centres them in it; one Undo reverts the align, a second removes the container.
- [ ] Kittest: drag a shape onto a container, it becomes a child (Layers shows it indented); drag it out, it is no longer a child.
- [ ] Kittest: set Layout to Row and add a fourth child; it is placed after the others without manual positioning.
- [ ] Snapshots `18-container.png` (dashed frame with children and the Align to toggle open) and `18-layout-row.png`.
- [ ] README describes containers, Align to Container, and auto layout.
