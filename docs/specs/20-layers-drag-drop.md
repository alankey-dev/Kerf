# 20 Layers drag and drop

**Wave 3. Depends on 02, 11, 13; integrates with 18. Size M.** Requested directly by the user: "dragging and dropping in the layers panel should also work".

## Problem

The Layers panel (`src/ui/layers_panel.rs`) can select (spec 13), hide, collapse and delete, but rows cannot be dragged. Reordering the z-order means the Arrange menu; moving an object into or out of a group means ungrouping and regrouping.

## Goal

Direct-manipulation drag and drop in Layers, as in Figma and Illustrator.

### Behaviour

- **Start.** Press on a row and move more than 4 px to start dragging. A plain click still selects (spec 13). Dragging a row that is part of the selection drags the whole selection (in its current relative order); dragging an unselected row selects it first and drags just it. Dragging a linked parent row drags the whole link.
- **Feedback.** A semi-transparent ghost of the dragged rows follows the pointer (row count badge when more than one). A 2px `ACCENT_PRIMARY` insertion line shows where they will land between rows; hovering the middle third of a parent row (Group, Attach, or a container from spec 18) highlights that row with `ACCENT_HOVER` instead, meaning "drop inside". The list auto-scrolls when the pointer nears its top or bottom edge.
- **Drop between rows** reorders: the dropped objects take that z position, keeping their relative order. Dropping between members of a link adds them to that link; dropping between top-level rows takes them out of any link they were in (unless the whole link was dragged, which moves as a block).
- **Drop on a parent row** adds the dragged objects to that link (or container), at the top of its members.
- **Invalid drops** (a container into itself or its own descendant; a drop that would leave a link with fewer than two members, which instead dissolves it as Ungroup does) show a not-allowed cursor and do nothing, or apply the documented rule; never corrupt links.
- **Escape** while dragging cancels with no change. Releasing outside the list cancels.
- The canvas reflects the new z-order and links in the same frame. The whole drop is **one undo step** (apply before `History::observe`).

### Model

- All of this is pure design operations in `src/layers_dnd.rs` (new): `plan_drop(design, dragged_ids, target) -> DropPlan` and `apply_drop(design, plan)`, where `target` is `Between { above: Option<id>, below: Option<id> }` or `Into(parent)`. The UI only computes the target from the pointer and calls these. Unit test every rule.
- Reuse existing primitives: z-order through the same code Arrange uses, link membership through `links.rs` (`group`/`attach`/`unlink`/`prune_links`). Do not add a second way to change links.
- **Containers (spec 18).** If spec 18 is merged when you start, Into(container) and Between rows inside a container reparent through spec 18's reparent function, and dragging a child out of its container removes it. If spec 18 is not merged yet, build the link and z-order behaviour, keep `DropTarget::Into` generic, and leave a clearly marked TODO-free extension point (a match arm the containers merge fills in); the orchestrator will wire it on merge.

## Files

- **Owns.** `src/layers_dnd.rs` (new, pure), `src/ui/tests_spec20.rs` (new).
- **Shared edits.** `src/ui/layers_panel.rs` (drag sensing, ghost, insertion line, auto-scroll; keep the row painting intact), `src/ui/mod.rs` (`mod` lines). Spec 18 is also editing `layers_panel.rs` (container parent rows); keep your edits in the drag path and expect a rebase.

## Acceptance

- [ ] Unit tests for `plan_drop`/`apply_drop`: reorder one object above another; reorder a multi-selection keeping its relative order; drop into a group adds membership; drop a group member to top level removes it from the link; dragging out the second-to-last member dissolves the link; dragging a whole link moves it as a block; a no-op drop leaves the design unchanged.
- [ ] Kittest: three shapes; drag the bottom Layers row to the top; the canvas z-order changes and one Undo restores it.
- [ ] Kittest: two grouped shapes and one loose shape; drag the loose shape's row onto the Group parent row; all three share the Group link.
- [ ] Kittest: drag a group member's row out to the top level; it no longer has the link.
- [ ] Kittest: start a drag and press Escape; nothing changes.
- [ ] Kittest: with 30 shapes, dragging to the bottom edge auto-scrolls the list.
- [ ] Snapshot `20-drag.png` mid-drag with the ghost and insertion line, and `20-drop-into.png` with a parent row highlighted.
- [ ] README describes Layers drag and drop.
