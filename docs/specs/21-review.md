# 21 Live effects: design review

**Status: Companion to spec 21, which is implemented.** Kept as the design record.

Adversarial review of `docs/specs/21-design.md` against `docs/specs/21-live-effects.md`, run with the interrogate skill. Three reviewers on different models (Opus, Fable, Sonnet) read the same brief and attacked seven angles: the data model, migration, the evaluation cache, undo, interactions with links, containers, node editing, text and warp, composition and the bridge, and whether phase 1 ships alone. The lead re-traced every finding kept below against the code.

Main was at 1a9a6d9 when the review started. Spec 18 merged during it (0d3903f, "Merge spec 18"), so `Shape.parent`, `settle` and `src/containers.rs` are now on main. The design's risk "Spec 18 is in flight" no longer applies, and its line numbers (grounded at 33667ae) are a few lines stale throughout. Line references below are to main at 0d3903f.

## Verdict

The design's core is sound. Reviewers checked these claims and found them correct.

- One stack per shape.
- One parent field.
- Styled pieces.
- Exact-key memoisation instead of a revision counter.
- The box rule for leading Warps.

Seven problems would ship wrong cuts, lost work or broken gestures, and all of them sit at the edges the design did not trace. These are the Compound's pose, destructive operations on a Compound, text fonts in the cache key, Make reading shape style instead of piece style, live panel editing, node editing under decision 7, and the size of phase 1. Each needs a design change before implementation starts. None needs the architecture replaced.

## Act on

### A1. A Compound cannot be rotated or resized through the existing gestures

Raised by all three reviewers. The lead traced it.

The design's `refit_compounds` writes each Compound's box as the bounds of its children with rotation 0 on every `settle`. The design also extends `carry` to Compounds for resize and flip. The rotate gesture writes an absolute pose from the drag-start originals every frame (`src/ui/canvas.rs:605-608`, `transform::rotate_many`). `carry` diffs against `before`, which is the design at the start of the current frame (`src/ui/mod.rs:490`, `:516`). `selection::write_back` replaces the selected shapes wholesale (`src/ui/selection.rs:65`).

**Failure scenario.**

1. Select two shapes and apply a welded Offset, which makes a phase 1 effect group G.
2. Drag G's rotate handle. Frame 1 writes G as (B0, δ1). `carry` turns the children by δ1. `refit_compounds` then rewrites G as (AABB of the turned children, rotation 0).
3. Frame 2 writes G as (B0, δ2) from the originals. Against `before` (AABB1, 0) this is a resize plus a turn of δ2. The design's extended `carry` remaps the children by both, so they are now at δ1 + δ2 and also scaled from AABB1 to B0.
4. After n frames the children have turned roughly n(n+1)/2 times the step and drifted in size. The frame always shows rotation 0.

With spec 18's unextended `carry` (`src/containers.rs:704-729`, which skips non-rigid changes), the rotation freezes after frame 1 instead. Either way the gesture is broken. Typing a rotation in the inspector rotates once and then reads 0.

Resize has the same root cause. Children's Offset rings are absolute inches and do not scale, so the refit box after a remap is not the box the user dragged. The next frame then remaps from the wrong box, and the handle trails the pointer. A typed width of 4 in comes back as something else.

The same order problem hits a Compound inside a Hug container (Fable). `relayout` sizes the container from the Compound's stale box, then `refit_compounds` grows the Compound past the container's edge. `settle` returns early on the next frame because `before == self`, so the container stays wrong until an unrelated edit.

**Design change.**

- Give a Compound a user-owned pose, as containers have. Keep `rotation_deg` and the flips. Refit only the centre and size, measured in the Compound's own rotated frame (children through `to_local`).
- Refit from the children's source boxes, not their evaluated pieces, so the box a resize writes is the box the refit reads back.
- Fold the Compound refit into `relayout`'s reverse-z pass, so a Compound inside a Hug container is refitted before its container measures it.
- Add kittests for a rotate drag in steps to 90 degrees, a typed W on a Compound, and a Compound inside a Hug row with an idempotence check.

### A2. Destructive operations on a Compound orphan its children, so they are cut twice

Raised by Opus and Sonnet. The lead confirmed `replace`.

`Design::replace` (`src/design.rs:923-934`) removes only the listed ids. It is the shared tail of `combine`, `slice`, `sticker`, `flatten`, `unflatten` and `ungroup_text`. `remove_many` (`src/design.rs:719-723`) and `duplicate` already go through `with_descendants`, but `replace` does not. The design makes a Compound selectable and has phase 1 Combine read `geometry().rings()`, which already includes the children.

**Failure scenario.**

1. Welded Offset on A and B makes group G.
2. Select G and C and choose Weld.
3. `replace([G, C], ..)` removes G and C and inserts the welded result, which already contains A, B, the ring and C.
4. A and B still have `parent == G`. `repair` clears the dangling parent and they become top-level shapes.
5. Make cuts A and B twice, once inside the weld and once on their own.

`flatten` on G clones G alone as a part, so the part has no geometry and A and B reappear the same way.

**Design change.** `replace` expands its ids through `with_descendants`, so every caller inherits it. State what each destructive operation does with a Compound input (use its evaluated rings and remove its subtree). Add a phase 1 test per operation on a Compound selection that asserts no orphans and no duplicate cut paths.

### A3. The cache key omits the system font catalogue, so text keeps its Inter fallback

Raised by all three reviewers. The lead confirmed the code path.

`FontRef::System(family)` falls back to Inter while the background scan is running (`src/text.rs:85`; `src/system_fonts.rs:67`, `CATALOGUE.get()?`). The scan runs on its own thread from start-up. Today this heals itself because text lays out every frame. The design's `LeafKey` holds only shape content, and the design states that per-frame layout disappears, so the first (Inter) evaluation stays cached.

**Failure scenario.**

1. Launch and open a project whose text uses Georgia before the scan finishes.
2. The canvas, hit-testing, snapping and Align use Inter outlines. Align writes `x_in` from Inter bounds.
3. The scan finishes and nothing in the key changes. The canvas still shows Inter.
4. Expand bakes Inter glyphs into the file permanently. Make clones the design with an empty cache and cuts Georgia, so the cut no longer matches the canvas.

Decision 1 creates the same class of bug (Opus). A new "show all contours" flag beside `contour_hidden` is a `Shape` field that the hand-picked `LeafKey` does not list. Toggling the Contour eye then returns the cached geometry.

**Design change.**

- Make `system_fonts` keep a generation counter, bumped when the catalogue lands, and include it in the key of every Text leaf and every Compound.
- List every non-`Shape` input to evaluation in the design (the font catalogue, the shape library, bundled fonts) and say how each invalidates.
- Group the geometry inputs on `Shape` into one `source` struct and key on it, so a field added later cannot be left out of the key.
- Add a test that lays out system-font text, completes the scan, and asserts the geometry changed.

### A4. Make, the bridge and composition read the shape's style, not the piece's

Raised by Opus and Sonnet. Opus alone raised piece identity and Attach. The lead traced both.

The design moves Make onto pieces but lists only `to_svg_path_d` and the placement as bridge changes. Three things are missing.

- **Operation and pen.** `make_flow.rs` `tool_loads` (about line 125-140) joins `MatItem.shape_id` back to shapes and filters on `s.operation == op`. `bridge.rs` (about lines 360-375) builds `art_type`, `art_tool`, `path_color` and `fiducial_id` from `p.shape.operation` and `p.shape.pen`. A Sticker piece is Print Then Cut on a layer that may be Basic Cut. An Offset with `style: Some(..)` and each Slice piece also carry their own style. `make_flow.rs` is not in the call-site table.
- **Piece identity.** `evaluate_compound` rewrites only bodies to `Child(kid id)`. Each child's own Offset keeps `Effect(EffectId)`, and ids are unique only within one stack. Two children with an Offset each, under a group with its own Offset, give three pieces called `Effect(1)`. `MatItem { shape_id: group, piece: Effect(1) }` then resolves all three to the first match.
- **Attach inside a Compound.** Composition groups units by `placement_key` over `design.shapes` and takes the mat key from the first cut member (`src/composition.rs:74-92`). The design composes over `drawn()`, which hides a Compound's children, and splits a shape's pieces by each piece's own key. An Attach between children is no longer seen.

**Failure scenarios.**

- Sticker on a Basic Cut square. The mat is Print Then Cut, but no shape has that operation, so `tool_loads` shows no blade load. The payload says Cut with no fiducial.
- A and B each carry a 0.1 in Offset, then a welded 0.25 in Offset groups them. Make cuts one of the three offsets three times and never cuts the other two.
- A black Pen label attached to a red square, then both given a welded Offset. Make puts the label on a black mat, out of registration with its cut.

**Design change.**

- Add `make_flow.rs` to the table. `MatItem` resolves to a piece, and `tool_loads`, `operations`, `art_type`, `path_color` and `fiducial_id` read `piece.style`.
- Make `PieceId` a path that stays unique under nesting, for example `{ owner: u64, local: Body | Effect(EffectId) | Slice(u8) }`. Add a test that every piece id in a geometry is distinct.
- Inside a Compound, group pieces by the child's placement key first (Attach link or container) and apply today's anchor rule within each group.
- Decide `image_id` explicitly. It would be identical for every piece of one shape. The bridge evidence tests do not pin `path_id` or `image_id`, and the proto treats both as free strings, so a suffix on both breaks no pinned value.
- Add a Make test for a Sticker piece, an Offset with its own style, and a Compound with nested Offsets.

### A5. Live panel editing is unspecified, so Warp stacks up and ids get reused

Raised by all three reviewers in parts. The lead confirmed the egui default.

The design's usage has `Picked::Warp(w) => add_effect(..)`, and says a reopened panel edits in place. The Warp panel returns a live edit on every frame its controls change, and it offers Remove Warp (`src/ui/toolbar/effects.rs`, the `live` return). Five gaps follow.

- **Stacking.** Nothing switches the panel from adding to editing after the first push, so a one-second Bend drag adds about 60 Warp entries. Remove Warp has no target.
- **Order.** `add_effect` appends, so warping a shape that already has an Offset gives `[Offset, Warp]`. That warps the ring too and does not refit the box, which is not what "warp this shape" means.
- **Id reuse.** `EffectId` is `max + 1`. Remove the last entry and add another, and the new one reuses the id. A panel still open on `(shape, 2)` then sends `Replace(Offset)` into what is now a Warp. Nothing stops `Replace` changing the variant.
- **Duplicate ids.** Deserialised stacks are not validated, so a duplicate id maps two entries to one `PieceId`.
- **Typed fields.** "A committed field is one change" is false. `length_drag` (`src/ui/inspector.rs:294-303`) uses `DragValue`, whose default in egui 0.31.1 is `update_while_editing(true)`. With the pointer up, `observe` records one step per keystroke. Today the Offset and Sticker panels only write on Apply, so this never mattered.

**Failure scenario.** Reopen an Offset from Layers and type "0.375". The design records about four undo steps and evaluates intermediate offsets such as 0.3.

**Design change.**

- Specify the panel state. `New` becomes `Editing(EffectId)` after the first add, and later frames use `edit_effect(Replace)`.
- The toolbar Warp edits the first Warp entry, or inserts one before the first Offset or Sticker. Remove Warp is `Edit::Remove` on that entry.
- Store a monotonic `next_id` on `Effects`. `Replace` refuses a different variant, and the panel closes when its entry is gone.
- `upgrade` rejects or renumbers duplicate ids, and drops entries the kind cannot take (Warp on a Compound or Flattened).
- Effect panel fields use `update_while_editing(false)`, or keep the gesture open while one has focus. Add a kittest that types a value and undoes once.

### A6. Decision 7 destroys a warp on Escape and changes the `[Offset, Warp]` outline

Raised by all three reviewers.

Today `NodePath::read` applies the warp for display and `write` clears it only on a real edit (`src/path_edit.rs` around `read` and `write`; `src/ui/node_edit.rs`, `begin` does not touch the design). Decision 7 bakes every Warp on entry. That causes three problems.

- **Escape loses the warp.** Double-click a warped star to look at its nodes and press Escape. The star is now a Path with no Warp row, and the change is an undo step the user did not intend.
- **Undo desynchronises.** Undoing the bake while node editing is open leaves a live warp that is drawn but not read, so the nodes sit off the visible outline.
- **The outline changes.** With `[Offset, Warp]` the Warp bends body and ring together inside the grown frame. Baking it into the body alone leaves `[Offset]`, which is an offset of the warped body. That is a different outline, and "box unchanged" cannot reproduce a body that was stretched onto the union frame.

**Design change.** Show the notice on entry but bake at the first `write`, as today, so entering and leaving stays free. Bake only leading Warps. For a Warp after an Offset or Sticker, refuse node editing with the notice and offer Expand. Test that the cut is identical before and after the bake.

### A7. Phase 1 is too large to ship as one unit and leaves work for phase 2 to undo

Raised by all three reviewers in parts.

Phase 1 contains the persisted `Compound` kind, `settle` changes, extended `carry`, `refit_compounds` and `hit_test_deep`. It also contains the cache, the upgrade, about 40 consumer moves, composition and the bridge on pieces, and the Layers UI. Findings A1, A2 and A4 show that the risk sits in the Compound machinery, which phase 1 needs only for multi-shape Offset and Sticker. Three further problems make the cut incoherent.

- **Flattened parts.** `Flattened` survives phase 1, and its outline is computed inside `ShapeKind::outline` through `flatten::outline` calling `p.world_rings` and `world_polylines` on each part (`src/flatten.rs`). Those methods are deleted in the same wave. After the upgrade, a part can carry `effects: [Warp]`, and `outline` has no `Design` to evaluate it. Either the methods survive (a dual path) or old stickers containing a warped word render unwarped, which breaks "old projects render identically".
- **Expand round trip.** A Sticker's Expand produces `Flattened` in phase 1 (decision 4), and phase 2's upgrade rewrites every `Flattened` into an Effects group with `Flatten`. Something the user expanded becomes a group again, and phase 1 `Flattened` shapes carrying their own stacks have no mapping in that rewrite.
- **Combine reads live rings.** Combine reading `geometry().rings()` welds an Offset ring into the body and then drops the stack. Today the offset is a separate shape the user chooses to include. Subtract and Intersect with an offset shape give surprising cuts.

**Design change.** Split phase 1.

1. **1a.** `Shape::new`, `Effects` with Warp only and its migration, `Design::geometry`, and every consumer moved. The output must be identical to today, proven by the existing tests and pinned payloads.
2. **1b.** Single-shape live Offset and Sticker, the Layers list, the eyes, and Expand. Multi-shape Offset and Sticker still bake.
3. **1c, or with phase 2.** Compounds, with A1, A2 and A4 fixed, landing Effects groups and boolean groups on one mechanism.

Also in the design:

- `flatten::outline` and `paint_layers` call `evaluate_leaf` on each part directly, uncached below the top shape.
- Combine uses body pieces only (or bakes first).
- Decide now whether `Flattened` stays a legacy kind or is retired. If retired, phase 1's Sticker Expand should not produce it.
- Add a pre-spec sticker containing a warped word to the load test.

### A8. Splitting text copies or drops its stack

Raised by all three reviewers.

`text_ops::place` builds each letter with `..source.clone()` (`src/text_ops.rs:82`), and the design changes `can_split` to block only Warp. Text with a 0.25 in Offset, split to letters, becomes N overlapping per-letter offsets. A Sticker becomes N stickers with N backings. If the implementer clears `effects` instead, the offset or sticker disappears.

**Design change.** `can_split` refuses any non-empty enabled stack with a tooltip, or `ungroup_text` wraps the letters in an Effects Compound carrying the source's stack. Pick one in the design.

## Consider

- **Decision 1 needs a model, not a flag beside the field.** Raised by all three. `curve_off` beside `curve_in`, and "show all" beside `contour_hidden`, make two fields per fact. "Eye off with an empty mask" and "`curve_in == 0` with `curve_off`" become representable. The body still says pinned rows have no eye (`RowView.enabled: None`, and `Enable` is "ignored on pinned rows"). The Curve eye changes the natural size, so it must go through the text refit, or the straight layout is scaled into the arc's box and the cut is squashed. Prefer `Text.curve: Curve { diameter_in, enabled }` and `contour_hidden: Option<Vec<bool>>`, or make Contour a real stack entry. Route every `curve_in` reader through one accessor. This feeds A3.
- **Cache cost outside the budget.** Raised by all three. A Compound evaluates in world space, so every drag frame changes its subtree key and re-runs the weld and offset, and the `Vec<Shape>` key clones every descendant at every ancestor level. One slot per id thrashes when callers pass drag originals or node-edit previews with the same id. An empty `Clone` makes every undo and every Make entry evaluate cold. Evaluate Compounds in their local frame with the same placement level as leaves. Keying slots by `(id, key)` would stop the thrash. Share one cache across clones (`Rc<RefCell<..>>` cloned with the design), which exact keys make safe. Add Compound drag, undo and Make entry to the timing test.
- **RefCell re-entrancy.** Raised by all three. `evaluate_compound` calls `geometry` for each child. If the lookup borrow is held across evaluation, the first Compound miss panics with `BorrowMutError`. State the rule (borrow, drop, evaluate, insert) and test a Compound inside a Compound.
- **No format version.** Raised by all three. An older build ignores `effects`, so warps, offsets and stickers vanish, and a save from it loses them. A `Compound` kind fails to parse, and `project::list` hides the project silently (`filter_map(.. .ok())`, `src/project.rs:115`). Add a `format` field to `Project` and refuse newer files with a message, or at least state that phase 1 is a one-way change.
- **Upgrade at the Shape level.** Opus. A `#[serde(from = "ShapeRepr")]` on `Shape` with an optional typed `warp` reaches `Flattened.parts` and direct `Shape` parsing for free, avoids a `Value` tree for large imported paths on every Home visit, and avoids the recursion trap in `TryFrom<Value> for Design`. Only phase 2's id-minting hoist needs the Design level.
- **Mixed-orientation rings in one operand.** Fable. `geometry::boolean` with one group never calls `overlay` (`src/geometry.rs:153-171`), so `weld(rings)` of one shape is a pass-through. `to_io` does not normalise orientation, and every overlay uses `FillRule::NonZero`. Putting a font's body rings and an i_overlay offset ring into one operand can cancel winding where orientations differ, which would turn the body into a hole in a Sticker backing. Pass pieces as separate operands, or normalise orientation in `to_io`. Test that offsetting body plus its own offset ring equals offsetting the ring alone.
- **Offset at distance 0 and empty results.** Sonnet. `geometry::offset` returns its input unchanged at distance 0 (`src/geometry.rs:186`), so a live Offset dragged through 0 adds a duplicate body piece that Make cuts twice. A negative distance can return no rings, and `Piece.bounds` is not optional. Drop empty and zero pieces in evaluation.
- **The style of a Compound's own effects.** Sonnet. Today a weld Offset copies the back-most input's fill, operation and pen (`src/design.rs:937-953`). The design does not say where an Effects group's `own` style comes from, or what inspector colour and operation edits on a Compound do.
- **Welded Offset reorders z.** Sonnet. Wrapping non-adjacent roots into one group at the back-most slot moves the front root behind unrelated shapes in between. Today's Offset leaves the inputs where they are. Refuse, warn, or state and test the reordering.
- **Compounds banned from links.** Fable. Today a Sticker is a plain shape that can be attached to a pen label. Spec 18's invariants need only that link members are siblings. Allow a Compound (not a container) to be a link member, or state the regression.

## Noted

- An Effects group whose children are all deleted keeps its stack and stays an empty row. A Boolean group with no visible children dissolves, so hiding both children destroys the group. Dissolve on no children, not on no visible children (Opus).
- `Combine` (`src/design.rs:94-101`) has no serde derives, which `CompoundOp::Boolean(Combine)` needs (Opus).
- Illegal entries in a loaded file (Warp on a Compound, Offset on a Guide container) are guarded only at `add_effect`. Folded into A5's upgrade validation (Fable).
- Design line references are stale against main. Re-ground before implementation (Sonnet).

## Dismissed

- **Threads.** All three checked. No `Design` crosses a thread today (only the device simulator, the font scan and font previews spawn), so `Rc` and `RefCell` inside `Design` are safe. It rules out a background save, which nobody has asked for.
- **Home previews sharing a cache.** Fable. Previews are PNG thumbnails rendered at save, so the design's colliding-ids argument for owning the cache in `Design` is moot, though harmless.
- **Spec 18 rebase risk.** Spec 18 merged during the review.

## Verified correct

- `History::observe` compares the whole `Design` and folds while the pointer is down. `settle` runs before `observe` (`src/ui/mod.rs:516-517`). An always-equal cache field causes no phantom steps, and add, enable, remove, move and slider drags are one step each.
- `warp::apply` fits on all unfiltered contours and returns one contour per input, then `contour_hidden` filters. `apply_in` over `0..size` equals today's `apply`, so old warps load bit for bit and the box needs no conversion.
- Every project load goes through `Project` and so through `Design`'s deserialiser. There is no clipboard JSON path. Only tests parse `Shape` directly.
- The bridge evidence tests pin `path_data`, transform, operation, tool and colour, not `path_id` or `image_id`. Plain shapes keep identical payloads through `Placement`. Local-frame contours that exceed the box (offset rings) place correctly through the box-centre transform.
- Warp is already refused on `Flattened` (`src/ui/toolbar/effects.rs:143-145`).
- Spec 18's `parent`, `children`, `with_descendants`, `roots` and `settle` order (repair, normalise_z, carry, relayout, with an early return) match the design. `holds_children` is the right seam.

## Agreement map

All three reviewers independently found these.

- The Compound rotate failure (A1).
- The font cache staleness (A3).
- Decision 7 (A6).
- Text split (A8).
- `EffectId` reuse (A5).
- Decision 1's second field.
- The cold cache and re-entrancy.
- The missing format version.

Two reviewers found the orphaned children (A2), the missing `make_flow.rs` and shape-style reads (A4), the Warp stacking (A5), and the phase 1 split (A7).

The disagreement on A1 is useful. Fable and Sonnet traced spec 18's current `carry` and predicted a freeze. Opus traced the design's extended `carry` and predicted a runaway spin. The lead's trace agrees with Opus for the design as written. Both reviewers agree the gesture breaks.

Lone findings with traced evidence went to Act on: piece id collisions and Attach inside a Compound (Opus), and keystroke undo steps (Opus, egui default confirmed). The rest went to Consider. The Fable reviewer leaned towards gesture and geometry correctness. The Sonnet reviewer leaned towards Make and edge cases. The Opus reviewer leaned towards performance and phase structure.
