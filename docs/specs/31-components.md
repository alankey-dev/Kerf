# 31 Components: reusable masters and instances

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec31.rs` pass.

**Wave 7. Depends on 22, 23, 02, 18, 21. Size XL; run as two phases (model and Make first, then the panel and overrides).** Kerf has no reusable symbols; to change a repeated tag or a wedding place card you edit every copy. Figma components solve this, and Kerf can do the same.

## Problem

Duplicate (Cmd+D) copies shapes with no link back to the original. A crafter making 40 favour tags with the same frame must edit 40 frames when the frame changes. Links (spec 02) and containers (spec 18) group objects but do not share them.

## Goal

A **component** is a reusable design part. Its **master** lives in the project's Components panel; **instances** are placed from it. Editing the master updates every instance. Instances can override colour and operation and can be detached into plain shapes.

### Model (architect first)

Keep one source of truth for the geometry; instances never hold copies of the master's shapes.

- `Design.components: Vec<Component { id, name, shapes: Vec<Shape>, size_in }>` (`#[serde(default)]`). The master's shapes live in the component's own frame with its top-left at the origin. Shapes inside a master may use links, containers and live effects as anywhere else.
- An instance is a shape with `ShapeKind::Instance { component: u64, overrides: BTreeMap<u64, PartOverride> }`, keyed by the master shape id, where `PartOverride { fill: Option<Color32>, operation: Option<Operation>, pen: Option<PenId>, hidden: bool }`. Its frame (x, y, size, rotation, flip, warp and other effects) is the instance's own, like any shape.
- `src/components.rs` owns `expand(design, instance) -> Vec<Shape>`: the master's shapes, transformed by the instance frame, with overrides applied, ids derived deterministically from (instance id, master shape id). Everything that reads geometry for drawing, hit-testing, Combine inputs, `compose` and the payload goes through spec 21's evaluation, which calls `expand`. Nothing else reads master shapes directly.
- Instances are container-like parents in Layers (spec 18 style parent row with a component badge) and their parts show as read-only child rows. They are rigid: parts cannot be moved inside an instance.
- **Nesting**: a master may contain instances of other components. Making a component that would contain itself, directly or indirectly, is refused with a message; a test covers the cycle check.
- **Links (spec 02)**: an instance can be a Group or Attach member like any shape. Attach links inside a master stay rigid within each instance.

### Creating and placing

- **Make component** (context menu, Edit menu, Cmd+Option+K) on a selection: the selection's shapes move into a new master named "Component 1" (editable), and the selection is replaced by one instance at the same position. One undo step.
- **Components panel**: a tab beside Layers in the inspector, a grid of thumbnails with names. Drag a tile to the canvas, or click it, to place an instance at its master size. Right-click a tile for Rename, Edit master, Duplicate component and Delete.
- **Delete** a component with instances asks: "Detach N instances and delete", or Cancel. Never leave dangling instances; a project that has one anyway (hand-edited) loads and draws it as a "Missing component" box.

### Editing the master

- **Edit master** (double-click an instance, or from the panel) opens the master in an isolation view: the canvas shows only the master's shapes on a plain artboard, with a breadcrumb "Canvas > Component: Name" and Done. Other instances are visible in a small live preview strip so the user sees the effect.
- Every editing tool works in isolation (spec 17 nodes, spec 12 text, Combine, effects). Instances update every frame, because they expand from the master.
- Changes made in isolation are part of the same undo history as the canvas; Done is not an undo step. Undo after Done reverts master edits too.
- If a master shape that has overrides is deleted, its overrides are dropped from every instance.

### Overrides

- With an instance selected, the inspector lists its parts. Each part shows its colour swatch and operation and can override them (and pen, for Pen parts). Overridden values show a small dot.
- **Reset overrides** on the instance clears all of them; right-click a part row for **Reset this part**.
- Size, rotation, flip and effects on the instance itself are not overrides; they are the instance's own frame.

### Detach

**Detach instance** (context menu, Cmd+Option+Shift+K) replaces the instance with the result of `expand`: plain shapes with overrides baked in, in one Group at the instance's z position, selected. One undo step.

### Save, load and Make

- Components and instances serialise with the design. A project saved before this spec loads with none.
- `compose` places each instance as one rigid unit (as an Attach link), with its parts' operations and colours after overrides deciding their mats, so a two-colour component spreads over two mats as normal shapes would.
- Thumbnails (`project::render_thumbnail`) draw instances expanded.

## Files

- **Owns.** `src/components.rs` (new: model, `expand`, cycle check, make and detach; pure), `src/ui/components_panel.rs` (new), `src/ui/component_isolation.rs` (new), `src/ui/tests_spec31.rs` (new).
- **Shared edits.** `src/design.rs` (`Design.components`, `ShapeKind::Instance`, literals), spec 21's evaluation module (call `expand`), `src/ui/layers_panel.rs` (instance parent rows), `src/ui/inspector.rs` (panel tab and overrides section), `src/ui/context_menu.rs` and `src/ui/chrome.rs` (commands and shortcuts, registered through spec 23), `src/ui/canvas.rs` (double-click to edit master; keep small), `src/composition.rs` (instance as a unit), `src/ui/mod.rs` (`mod` lines).

## Acceptance

- [ ] Unit test: `expand` of an instance scaled 2x and rotated 90 degrees places each master shape correctly within 0.001 in.
- [ ] Unit test: changing a master shape's size changes every instance's expanded geometry; instances' own frames are unchanged.
- [ ] Unit test: a fill override on one instance changes only that instance's part; Reset overrides restores the master fill.
- [ ] Unit test: deleting a master shape drops its overrides from every instance.
- [ ] Unit test: Detach produces plain shapes equal to `expand`, grouped, and the design has no instance left.
- [ ] Unit test: making a component that contains an instance of itself is refused.
- [ ] Unit test: `compose` treats an instance as one rigid unit; a two-colour component yields parts on two mats.
- [ ] Unit test: a project with components and overridden instances round-trips; an old project loads; an instance of a missing component loads.
- [ ] Kittest: select three shapes, Make component; the selection becomes one instance and the panel shows one tile; one Undo restores the three shapes.
- [ ] Kittest: drag a tile onto the canvas twice, double-click one instance, recolour a part in isolation, Done; both instances show the new colour.
- [ ] Kittest: override one instance's part to Pen, then Reset overrides; it returns to Basic Cut.
- [ ] Kittest: Detach instance; Layers shows a Group of plain shapes.
- [ ] Snapshots `31-panel.png` (Components panel with tiles), `31-isolation.png` (editing a master with the preview strip), `31-overrides.png` (inspector parts list with an override dot).
- [ ] README describes components, instances, editing masters, overrides and detach.
