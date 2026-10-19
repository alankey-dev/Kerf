# 21 Live, editable effects on layers

**Wave 4. Depends on 04, 05, 12, 17, 18, 19, 20, and on 22 and 35 (phase 1a) and 23 (phase 1b). Size XL; run as three phases.** Design: `docs/specs/21-design.md`. Review: `docs/specs/21-review.md`. Also absorbs AUDIT-007, 015 and 021 from `docs/audit/2026-10-12-audit.md`. Requested directly by the user: "any modifying action (such as effects) should be visible on the layer so they can be edited after the fact".

## Problem

Some modifying actions are already non-destructive (Warp is a removable `Shape.warp`, text Curve is `curve_in`, Contour hiding is `contour_hidden`, Flatten has Unflatten), but nothing in Layers shows them, and the rest bake their result immediately:

| Action | Today |
|---|---|
| Offset | Adds a new plain shape behind; no link back to the source; changing the source does not update it. |
| Sticker | Flattens the selection with a backing; border and corner style cannot be changed later. |
| Combine (Weld, Unite, Subtract, Intersect, Exclude) | Replaces the inputs with one baked path; the originals are gone except through Undo. |
| Slice | Replaces two shapes with baked pieces. |
| Warp, Curve | Stored, but only visible by opening the Effects panel on that exact shape. |
| Contour | Stored, but invisible in Layers. |

## Goal

Every modifying action is a **live effect**: visible on its layer, editable after the fact with its original controls, toggleable, removable, and only baked when the user explicitly chooses **Expand**. This is how Illustrator's Appearance panel and Figma's boolean groups work.

### Model

One source of truth for "what was done to this layer". The design (`21-design.md`) replaces the sketch this spec first gave; the points below are what it settles.

- **Per-shape effect stack.** `Shape.effects` is an ordered stack of `Warp`, `Offset { distance_in, corners, style }` and `Sticker { border_in, corners }` entries, each with a stable `EffectId` from a monotonic counter that is never reused. The existing `warp` field migrates into the stack on load. Curve and contour hiding stay source settings with their own models (`Text.curve: Option<Curve { diameter_in, enabled }>`, `Shape.contours: ContourMask { hidden, enabled }`) and show as pinned rows with an eye. Offset's `weld` is a choice made when the effect is created, not a stored field.
- **Offset as an effect** draws its outline behind the source as part of the same layer, cut as its own piece with its own style (defaulting to the layer's, live). Moving, resizing, editing nodes or text of the source updates it live. Expand turns it into a separate plain shape, as today.
- **Sticker as an effect** on a shape keeps the input editable underneath and the backing live. Expand produces today's `Flattened` shape, which stays as the baked Print Then Cut kind.
- **Groups (phase 2).** A `Compound` kind under spec 18's `parent` holds multi-shape effects (a welded Offset, a Sticker on several shapes) and boolean groups (Combine and Slice). Children stay intact and editable; selecting a child (Layers or Cmd-click) and editing it updates the result live. Changing the op is a dropdown on the parent. Expand bakes to today's output. A Compound owns its rotation and flips; its centre and size follow its children's source boxes.
- **Evaluation** is pure (`src/effects/`). `Design::geometry(&Shape)` returns styled pieces, memoised by exact keys in one cache shared across clones of a design. The key includes the system font catalogue generation (spec 22) and is built so that a new `Shape` field cannot be left out. Make, composition (spec 09), `make_flow`, hit-testing, the selection frame (spec 06), op rendering (spec 19) and export all use the evaluated geometry, and Make reads each piece's own operation, pen and colour.
- **Edits** are `Edit` variants (spec 23): `SetWarp`, `AddEffect`, `EditEffect` and `Expand`, with every refusal in `Design::check`. No new dispatch path.
- **Destructive operations** consume what Delete would delete: `Design::consume` expands through `with_descendants`, and every destructive operation (Combine, Slice, Flatten, Unflatten, Expand, Split Letters) calls it.
- **Undo.** Adding, editing (one step per slider drag or committed field), toggling, reordering, removing and expanding are each one undo step. History is capped at 200 entries.

### Live panel editing

- A panel targets either the selection (`New`) or one entry (`Entry { shape, row }`). Nothing a panel does on a frame appends an entry. The toolbar Warp edits the first Warp entry or inserts one before the first Offset or Sticker; Remove Warp removes that entry. Offset and Sticker add one entry on Apply, then edit it by id.
- `Replace` cannot change an entry's kind. A panel whose entry is gone closes.
- Typed fields in effect panels commit on Enter or focus loss, not per keystroke.

### Node editing

Entering node editing writes nothing; a notice says "Editing nodes applies the warp". The first real node edit bakes the leading Warps in the same undo step. A Warp after an Offset or Sticker refuses node editing and offers Expand.

### Text split

Split Letters and Split Lines refuse text with any effects ("Expand or remove effects to split") and text that is a container. Split letters start from a clean shape (no container role, no link, no stack) and take the source's parent and z slot.

### Layers UI

- Each row shows small effect chips after its name (spec 01 icons: Warp, Curve, Offset, Sticker, Contour), at most three then "+N". Hovering a chip shows a tooltip with its parameters ("Offset 0.25 in, round").
- A disclosure on the row (or clicking a chip) expands an **effect list** under the row: one line per effect with an eye toggle, its name and summary, an Edit button, and a remove (x). Lines can be dragged to reorder the stack (reuse spec 20's drag machinery and insertion line).
- **Edit** opens that effect's existing panel (spec 05 Effects popover, spec 12 Curve, the Contour panel) pre-filled with the stored values, anchored beside the Layers row; changes apply live.
- Boolean group parent rows show the op icon and a small op dropdown; their children are indented under them and can be dragged in or out (spec 20) to add or remove inputs.
- The context menu (spec 13) gains Edit effect, Disable or Enable effect, Remove effect, and Expand (bakes all effects of the selection, or the boolean group).
- The inspector shows the same effect list for the primary selection, so it is reachable without Layers.

### Expand

**Expand** (context menu, Layers row menu, Object menu) bakes the evaluated geometry into plain shapes exactly as today's destructive actions produced, and removes the effects or the boolean parent. One undo step.

## Phases

- **Phase 1a (behaviour identical, merge alone).** Lands after specs 22 and 35. `Shape::new` with a codemod, the history cap, the extended timing baseline, the warp-only stack and its migration, `Design::geometry` and its cache, and every consumer moved to evaluated geometry (composition, `make_flow` and the bridge on pieces). Output is identical to today, proven by the existing tests, snapshots and spec 35 goldens.
- **Phase 1b (merge alone).** Lands after spec 23. `Design::consume` and the text split policy; live single-shape Offset and Sticker; Curve and Contour models with eyes; the panel editing model; Layers chips and effect list with Edit, eye, remove and reorder; the inspector list and context menu items; Expand. A welded Offset or a Sticker on several shapes still bakes as today. Combine, Slice and Flatten refuse inputs with a live Offset or Sticker ("Expand effects first").
- **Phase 2.** Compounds on one mechanism: effect groups for multi-shape Offset and Sticker, and boolean groups for Combine and Slice, with live children, op dropdown, drag in and out, Cmd-click, and Expand. May land as two merges sharing the Compound machinery.

## Files

- **Owns.** `src/effects/` (new: model, rows, evaluation, cache, ops, upgrade), `src/ui/effects_list.rs` (new: Layers and inspector effect list), `src/ui/tests_spec21.rs` (new).
- **Shared edits.** `src/design.rs` (the stack on `Shape`, `consume`, `History` cap, every `Shape` literal), `src/edit.rs` (effect `Edit` variants and their checks), `src/containers.rs` (phase 2 Compounds in `settle`), `src/warp.rs` and `src/text.rs` (read effects and `curve` from the model), `src/text_ops.rs` (split policy), `src/flatten.rs` (parts through evaluation, `consume`), `src/path_edit.rs` and `src/ui/node_edit.rs` (bake at first edit), `src/ui/toolbar/effects.rs` and `combine.rs` (create effects, not baked shapes), `src/ui/inspector.rs` (effect list, `length_drag` commit mode), `src/ui/layers_panel.rs` (chips, effect list, group parent rows), `src/ui/context_menu.rs`, `src/composition.rs`, `src/make_flow.rs` and `src/bridge.rs` (pieces and piece style), `src/project.rs` (`format_version` bump, thumbnail), `src/ui/canvas.rs` (draw evaluated geometry; keep small). This is a deep change: rebase onto the latest main before starting each phase.

## Acceptance

Phase 1a:
- [ ] Unit test: a project saved before this spec with a warp, a text curve and a hidden contour loads with identical outlines (1e-6 in), and its warp is the first stack entry.
- [ ] Unit test: a pre-spec sticker whose parts include a warped word loads and renders identically.
- [ ] Unit test: a file with duplicate effect ids loads with distinct ids and `next` above them.
- [ ] Unit test: system-font text laid out before the catalogue is ready changes geometry once the catalogue reports ready (new generation).
- [ ] Unit test: every piece id in every geometry the suite builds is distinct.
- [ ] Unit test: `History` keeps at most 200 past entries.
- [ ] Every existing test, snapshot and spec 35 golden payload passes unchanged.
- [ ] Performance: the extended timing test (200 shapes plus 50 text shapes, a large import, and a drag frame) is no slower than the baseline recorded before the cache.

Phase 1b:
- [ ] Unit test: Offset effect on a square, then resizing the square, yields an offset outline that tracks the new size; disabling the effect removes it from evaluation and from Make.
- [ ] Unit test: Expand on an Offset produces what today's Offset produces (1e-3 in on rotated shapes).
- [ ] Unit test: reordering Warp above Offset changes the evaluated geometry as expected (offset of warped vs warp of offset).
- [ ] Unit test: an Offset at distance 0 adds no piece; a negative Offset that consumes the shape adds no piece; offsetting a body plus its own offset ring equals offsetting the ring alone.
- [ ] Unit test (Make): a Sticker on a Basic Cut square goes on a Print Then Cut mat with a blade load and a fiducial; an Offset with its own style cuts with that operation and colour.
- [ ] Unit tests (subtrees): Weld, Slice, Flatten, multi-shape Sticker, Split Letters and Unflatten on a container with children leave no orphan, no unexpected `container`, and each cut path once; `group` succeeds after Split.
- [ ] Unit test: Split Letters is refused on text with an Offset and on container text.
- [ ] Kittest: apply Offset, then the Layers row shows an Offset chip; click it, change the distance in the reopened panel, and the outline updates; one Undo restores the previous distance.
- [ ] Kittest: reopen an Offset, type "0.375" and press Enter; one Undo restores the previous value.
- [ ] Kittest: drag the Warp Bend slider over many frames; the stack holds one Warp and one Undo removes the drag.
- [ ] Kittest: Remove effect from the Layers effect list removes the warp and restores the original size; a panel open on that entry closes.
- [ ] Kittest: enter node editing on a warped star and press Escape; the shape still has its Warp and history has no new step.
- [ ] Unit test: the cut of `[Warp, Offset]` is the same before and after the first node edit bakes the Warp; a Warp after an Offset refuses node editing.
- [ ] Performance: 200 shapes with one Offset each evaluate and render under 16 ms per frame in release, reported for cold, warm, drag and resize frames (timed test behind `#[ignore]`).

Phase 2:
- [ ] Unit test: Weld as a boolean group, then moving one child, changes the welded outline; Expand matches today's Weld output.
- [ ] Unit test: changing a boolean group from Unite to Subtract re-evaluates.
- [ ] Unit test: a Compound inside a Compound evaluates without a borrow panic.
- [ ] Unit test (Make): two children each with an Offset, under a group with its own Offset, cut three distinct offsets once each; an attached Pen label inside a welded Offset lands on its cut's mat.
- [ ] Unit tests: Weld, Flatten and Expand on a Compound selection leave no orphans and no duplicate cut paths.
- [ ] Kittest: rotate a Compound in steps to 90 degrees; it reads 90 and its children turned by 90.
- [ ] Kittest: type W = 4 in on a Compound; it reads back 4 in.
- [ ] Kittest: a Compound inside a Hug row settles to the same design twice.
- [ ] Kittest: Combine > Subtract, then Cmd-click the child and drag it; the hole moves live.
- [ ] Kittest: drag a third shape's Layers row into a Weld group; it joins the weld.
- [ ] Performance: the timing test adds Compound drag, undo and Make entry.

Both:
- [ ] Snapshots `21-chips.png` (Layers rows with chips), `21-effect-list.png` (expanded list with an effect being edited), `21-boolean.png` (a Subtract group with its children shown).
- [ ] README describes live effects, the effect list, boolean groups and Expand.
