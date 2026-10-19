# 21 Live effects: type and module design

Design for `docs/specs/21-live-effects.md`, produced with the architect skill and revised after the adversarial review in `docs/specs/21-review.md` and the audit in `docs/audit/2026-10-12-audit.md` (section 10, "Then: spec 21"). No code was written.

Grounded against `main` at 70492ab, whose `src/` matches 0d3903f ("Merge spec 18"). Spec 18's `Shape.parent`, `settle` and `src/containers.rs` are on main. The design also builds on three specs in progress, and names the interface it needs from each.

- **Spec 22** supplies an injectable `FontCatalogue` with `ready()` and `generation()`, and a project `format_version`.
- **Spec 23** supplies `src/edit.rs`: one `Edit` enum with model-owned `Design::check` and `Design::apply`. Effects are `Edit` variants, never a new dispatch path.
- **Spec 35** supplies golden bridge payload tests (rotated, flipped, mirrored, multi-copy).

Line references are to function names where possible. Re-ground any line number before implementing.

## Summary

- **One stack per shape.** `Shape.effects` is an ordered stack of `Warp`, `Offset` and `Sticker` entries, each with a stable id. `Shape.warp` moves into it on load. Curve and contour hiding stay source settings with their own small models (`Text.curve`, `Shape.contours`), shown as pinned rows with an eye.
- **One geometry entry point.** `Design::geometry(&Shape) -> Rc<Geometry>` returns styled pieces. It is memoised by exact keys in one cache shared by every clone of a design, keyed by `(id, key)`, and the key includes the font catalogue generation. Every consumer that paints, hit-tests, frames, aligns, composes, exports or builds the bridge payload reads it. Make reads each piece's own style.
- **One parent mechanism, later.** Multi-shape live effects and boolean groups need a `Compound` kind under spec 18's `parent`. They come in phase 2, with the pose, subtree and piece-identity fixes the review asked for. Phase 1 has no Compounds.
- **Three phases.** Phase 1a changes no behaviour: a warp-only stack, `Design::geometry`, and every consumer moved. Phase 1b adds live single-shape Offset and Sticker, the Layers effect list, the panel editing model and Expand. Phase 2 adds Compounds: effect groups and boolean groups on one mechanism.
- **Undo needs nothing new beyond spec 23.** Every effect edit is one `Edit` applied before `History::observe`, panels write one entry by stable id, and typed fields commit once.

## Problem

Warp, Curve and contour hiding are already non-destructive, but each lives in its own field and feeds geometry its own way. `Shape::all_contours` applies `warp`, `contours()` filters `contour_hidden`, and text layout reads `curve_in`. Offset, Sticker, Combine and Slice bake at once. Seven facts shape the design.

- **Warp owns the box.** `warp::apply` warps and stretches the result back onto `0..size`, and `Shape::set_warp` resizes the box about its centre so the spec 06 frame hugs the bent outline. Removing a warp restores the size exactly.
- **Effects carry style.** An Offset has its own operation and colour. Slice yields up to three pieces in two styles. So evaluation yields styled pieces, and Make places pieces, not shapes, because mats are keyed by `(is Print Then Cut, fill)`. Make must also take the operation, tool, colour and fiducial from the piece (`make_flow::tool_loads`, `bridge.rs` around `art_type` and `path_color`).
- **`Design.shapes` is public and mutated everywhere.** A revision counter would be a second source of truth that one missed bump turns into a stale cut.
- **Text layout reads a process-wide font catalogue that changes after start-up.** Today this heals because text lays out every frame. A cache must see the change (review A3, audit AUDIT-008).
- **Destructive operations disagree about subtrees.** `Design::replace` removes only the listed ids, `remove_many` and `duplicate` expand through `with_descendants`, and `ungroup_text` builds letters with `..source.clone()` (AUDIT-007, review A2 and A8).
- **Panels write every frame.** The Warp panel returns a live edit on each frame its controls change, and inspector `length_drag` uses `DragValue`, which by default changes on every keystroke (review A5).
- **Geometry is read raw, uncached, at about 40 sites.** Each editor frame clones the design, and history keeps every snapshot (AUDIT-015, AUDIT-021).

## Review and audit resolution

| Item | Resolution | Phase | Section |
|---|---|---|---|
| A1 Compound pose | A Compound owns its rotation and flips. Refit runs only when a child changed and the Compound did not, from children's source boxes in the Compound's own frame, inside `relayout`'s reverse-z pass | 2 | Phase 2: Compounds |
| A2 orphaned children | `Design::consume` expands through `with_descendants`; every destructive operation calls it; per-operation tests | 1b (primitive), 2 (Compound inputs) | Consuming shapes |
| A3 font catalogue | Text leaf keys carry the catalogue generation; every non-`Shape` input is listed; the key is built by an exhaustive destructure of `Shape` | 1a | Evaluation and caching |
| A4 piece style and identity | `MatItem` names a piece; `tool_loads`, operations, `art_type`, `path_color` and `fiducial_id` read `piece.style`; `PieceId { owner, local }`; Attach grouping inside a Compound | 1a (style, ids), 2 (Compound grouping) | Make on pieces |
| A5 panel editing | Panel targets, stable monotonic ids, `Replace` keeps its variant, toolbar Warp edits the first Warp, typed fields commit once | 1a (Warp), 1b (Offset, Sticker, Layers) | Panel editing model |
| A6 decision 7 | Notice on entry, bake leading Warps at the first real node edit, refuse a non-leading Warp | 1a (identical), 1b (refusal) | Node editing |
| A7 phase size | Phases 1a, 1b and 2; `Flattened` kept as the permanent baked kind; Combine refuses live Offset and Sticker inputs until phase 2 | all | Phases |
| A8 text split | `can_split` refuses any non-empty stack and any container text; letters start from a clean shape | 1b | Consuming shapes |
| AUDIT-007 | Same as A2 and A8. `ungroup_text` gets its own fix, since it does not call `replace` | 1b | Consuming shapes |
| AUDIT-015 | Exact-key cache with local-frame evaluation, `(id, key)` slots, one cache shared across clones, no borrow held across evaluation. The timing test is extended before the cache lands | 1a | Evaluation and caching |
| AUDIT-021 | History capped at 200 entries before the cache lands | 1a | Undo |
| Format version | Each phase that changes the file bumps spec 22's `format_version` | 1a, 1b, 2 | Persistence |

The review's Consider items are taken as follows. Decision 1 gets a model (Curve and contour hiding). Cache cost and re-entrancy are covered in caching. The format version is taken. The upgrade moves to the `Shape` level. Mixed orientation, zero and empty offsets are covered in evaluation. The Compound's own style, welded Offset z order and Compounds in links are settled in phase 2.

## Usage (caller's view)

### Effects popover (`src/ui/toolbar/effects.rs`)

```rust
// Phase 1a: the toolbar Warp edits the first Warp entry of each root, or inserts one before the first
// Offset or Sticker. Every live frame sends the same Edit, so a Bend drag writes one entry, not sixty.
dispatch(Edit::SetWarp(Some(w)));          // a live frame or a type click
dispatch(Edit::SetWarp(None));             // Remove Warp

// Phase 1b: Apply creates the entry once; the panel then edits it by id.
match panel.target {
    Target::New => if applied {
        dispatch(Edit::AddEffect { effect: Effect::Offset(o), reach });   // returns the new ids per root
        panel.target = Target::Entry(EntryRef { shape, row: Row::Entry(new_id) }); // one root only
    },
    Target::Entry(at) => if committed {
        dispatch(Edit::EditEffect { at, op: EffectEdit::Replace(Effect::Offset(o)) });
    },
}
```

### Layers effect list and inspector (`src/ui/effects_list.rs`, phase 1b)

```rust
for row in effects::rows(shape) {                  // Curve, Contour (pinned), then the stack in order
    chip(row.icon, &row.summary);                  // "Offset 0.25 in, round"
    if eye(ui, row.enabled).clicked() { dispatch(Edit::EditEffect { at: row.at(shape.id), op: EffectEdit::Enable(!row.enabled) }) }
    if edit_button(ui).clicked() { open_panel_beside_row(Target::for_row(shape.id, row.key)) }
    if remove_button(ui).clicked() { dispatch(Edit::EditEffect { at: row.at(shape.id), op: EffectEdit::Remove }) }
}
dispatch(Edit::EditEffect { at, op: EffectEdit::MoveTo(index) });     // spec 20 drag machinery
dispatch(Edit::Expand);                                                // bakes the selection
```

### Consumers (canvas, op_style, Make)

```rust
// src/ui/canvas.rs paint_shapes
for shape in design.drawn().filter(|s| !s.hidden) {
    for piece in design.geometry(shape).pieces() {
        op_style::paint_piece(painter, piece, &to_screen, px_per_in, mode);
    }
}
// src/composition.rs: MatItem { piece: PieceId, offset_in }, mat key from piece.style
// src/make_flow.rs tool_loads: items resolve to pieces; filter on piece.style.operation
// src/bridge.rs: art_type, art_tool, path_color and fiducial_id from piece.style
```

## Shape

### Model changes on `Shape` and `ShapeKind` (`src/design.rs`)

```rust
pub struct Shape {
    pub id: u64,
    pub kind: ShapeKind,
    /// The source box: the frame `kind.outline(w, h)` fills. Leading Warp entries refit it (see "Box rule").
    pub x_in: f32, pub y_in: f32, pub width_in: f32, pub height_in: f32,
    pub rotation_deg: f32, pub flip_x: bool, pub flip_y: bool,
    #[serde(with = "color_serde")] pub fill: Color32,
    pub operation: Operation,
    pub hidden: bool,
    /// Phase 1b replaces `contour_hidden: Vec<bool>` (see "Curve and contour hiding").
    pub contours: ContourMask,
    #[serde(default)] pub link: Option<crate::links::Link>,
    #[serde(default)] pub pen: crate::pens::Pen,
    /// What was done to this layer, in order. Changed only through the effect `Edit` variants.
    #[serde(default, skip_serializing_if = "Effects::is_empty")]
    pub effects: crate::effects::Effects,
    #[serde(default)] pub container: Option<crate::containers::ContainerRole>,   // spec 18
    #[serde(default)] pub parent: Option<u64>,                                   // spec 18: the only parent record
    // DELETED: warp: Option<Warp>
}

pub enum ShapeKind {
    // ...presets, Builtin, Imported, Path { contours, view_box, node_kinds } (spec 17)...
    Text { /* .. */ curve: Option<Curve>, /* replaces curve_in in 1b */ },
    /// The baked Print Then Cut kind: Sticker's Expand and today's Flatten produce it. Kept in every phase.
    Flattened { parts: Vec<Shape>, view_box: Pt },
    /// Phase 2. No outline of its own. Its children (`parent == Some(self.id)`) supply its geometry.
    Compound(CompoundOp),
}

impl Shape {
    /// One constructor so later field changes touch one line instead of every literal.
    pub fn new(id: u64, kind: ShapeKind) -> Shape;
    // KEPT: centre_in, to_world, to_local (placement only).
    // DELETED in 1a once callers move: all_contours, contours, contour_count, world_rings, world_polylines,
    //   world_contours, world_bounds, contains_in, set_warp.
}
```

`Flattened` stays. It is the baked form of a Sticker, the output of today's Flatten, and what every saved sticker already is. Retiring it would make Expand produce something phase 2 then rewrites into a group again (review A7). Its parts are full `Shape`s, so after the upgrade a part may carry a warp-only stack. `flatten::outline` and `flatten::paint_layers` evaluate each part with `effects::evaluate_leaf` directly. That is uncached below the top shape, which is fine because the top shape's own geometry is cached.

### `src/effects/model.rs`: the stack

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Effects {
    entries: Vec<Entry>,
    /// Monotonic. An id is never reused within one stack, even after its entry is removed.
    #[serde(default)]
    next: u16,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectId(u16);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: EffectId,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub enabled: bool,
    pub effect: Effect,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Effect {
    /// 1a. Bends every piece so far inside the current frame and stretches the result back onto that frame.
    Warp(crate::warp::Warp),
    /// 1b. A closed outline `distance_in` outside (negative inside) the weld of the cuttable pieces so far,
    /// behind them.
    Offset(Offset),
    /// 1b. Replaces every piece so far with one Print Then Cut piece: a white backing `border_in` outside
    /// their weld, printed with the pieces in their own fills.
    Sticker(Sticker),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners { Round, Sharp }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    pub distance_in: f32,
    pub corners: Corners,
    /// `None` follows the layer's own fill, operation and pen live.
    #[serde(default)]
    pub style: Option<Style>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Sticker { pub border_in: f32, pub corners: Corners }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Style { #[serde(with = "crate::design::color_serde")] pub fill: Color32, pub operation: Operation, pub pen: Pen }

impl Effects {
    pub fn is_empty(&self) -> bool;
    pub fn entries(&self) -> &[Entry];
    pub fn get(&self, id: EffectId) -> Option<&Entry>;
    pub fn enabled(&self) -> impl Iterator<Item = &Entry>;
    /// Entries before the first Offset or Sticker that are Warps. Only these refit the box and only these
    /// bake on a node edit.
    pub fn leading_warps(&self) -> impl Iterator<Item = &Entry>;
    /// Whether an enabled Warp follows an Offset or Sticker.
    pub fn has_trailing_warp(&self) -> bool;
    pub(in crate::effects) fn push(&mut self, effect: Effect) -> EffectId;
    /// The toolbar Warp: edits the first Warp entry, or inserts one before the first Offset or Sticker.
    /// `None` removes that entry. Idempotent for a repeated value.
    pub(in crate::effects) fn set_warp(&mut self, warp: Option<Warp>);
    /// Refuses (returns false) when `effect` is a different variant from the entry's.
    pub(in crate::effects) fn replace(&mut self, id: EffectId, effect: Effect) -> bool;
    pub(in crate::effects) fn set_enabled(&mut self, id: EffectId, on: bool);
    pub(in crate::effects) fn remove(&mut self, id: EffectId) -> Option<Entry>;
    pub(in crate::effects) fn move_to(&mut self, id: EffectId, index: usize);
    /// Load-time repair: renumber duplicate ids, raise `next` above every id, drop entries `kind` cannot take.
    pub(crate) fn validated(self, kind: &ShapeKind, container: bool) -> Effects;
}
```

The spec's `Offset { weld, placement }` fields are dropped. Weld only means something across several shapes, so it is a choice made when the effect is created (`Reach`), and "weld on one shape" is not representable. `placement` has no UI and no defined meaning today.

In phase 1a `Effect` has only the `Warp` variant. Phase 1b adds `Offset` and `Sticker`.

### Curve and contour hiding (decision 1, phase 1b)

Both stay source settings, because neither is an ordered stage, but each gets a model in place of a second flag beside the old field.

```rust
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Curve { pub diameter_in: f32 /* signed: negative curves down */, pub enabled: bool }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ContourMask { pub hidden: Vec<bool>, #[serde(default = "yes")] pub enabled: bool }

impl ShapeKind { pub fn active_curve(&self) -> Option<f32>; }   // the one reader of `curve`
impl ContourMask { pub fn active(&self) -> &[bool]; }           // empty when disabled
```

- `curve: None` is straight text. "Diameter 0 with the eye off" is not representable. Every former reader of `curve_in` (layout, `default_size_in`, `curve_range`, the Curve panel) goes through `active_curve`.
- The Curve eye changes the natural size, so toggling it goes through the text refit, as a diameter change does. A straight layout is never scaled into the arc's box.
- The Contour row shows only when `hidden` has a true entry. Its eye shows all contours without losing the mask.
- Both are part of the cache key automatically (see "Key construction").
- The upgrade maps `curve_in: 0` to `None`, any other value to `Some(Curve { diameter_in, enabled: true })`, and `contour_hidden: v` to `ContourMask { hidden: v, enabled: true }`.

### `src/effects/rows.rs`: what Layers and the inspector list (phase 1b)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row { Curve, Contour, Entry(EffectId) }

pub struct RowView {
    pub key: Row,
    pub icon: Icon,
    pub name: &'static str,
    pub summary: String,          // through units.rs, for example "Offset 0.25 in, round"
    pub enabled: bool,            // every row has an eye
    pub draggable: bool,          // false for Curve and Contour
}

/// Curve (text with a curve), then Contour (any hidden), then the stack in order.
pub fn rows(shape: &Shape) -> Vec<RowView>;
```

Chips are the first three `rows` then "+N". This is a derived view, so nothing is stored twice.

### `src/effects/eval.rs`: what evaluation yields

```rust
pub type Bounds = crate::snap::Aabb;          // spec 23 replaces the anonymous 4-tuple
pub type Lines = Vec<(Vec<Pt>, bool)>;

pub struct Geometry {
    pieces: Vec<Piece>,            // back to front
    placement: Placement,          // flip then rotate about the centre
    bounds: Option<Bounds>,        // world AABB of all pieces
    grab: Option<[Pt; 4]>,         // Text: the whole box is grabbable, as today
}

pub struct Piece {
    pub id: PieceId,
    pub style: Style,
    pub contours: Vec<Contour>,    // in the placement's local frame, beziers kept: what Make sends and export writes
    pub lines: Lines,              // world, flattened: what the canvas paints and hit-tests
    pub paint: Paint,
    pub bounds: Bounds,            // world; a piece with no contours is dropped, never stored
}

/// Unique within any geometry, including under phase 2 nesting: `owner` is the shape that produced the piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PieceId { pub owner: u64, pub local: Local }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Local { Body, Effect(EffectId), Slice(u8) }

pub enum Paint {
    Fill,
    /// Print Then Cut artwork (Sticker, Flattened): world lines with their own fills, painted inside the cut.
    Print(Vec<(Lines, Color32)>),
}

pub struct Placement { pub centre: Pt, pub half: Pt, pub rotation_deg: f32, pub flip_x: bool, pub flip_y: bool }

impl Geometry {
    pub fn pieces(&self) -> &[Piece];
    pub fn piece(&self, id: PieceId) -> Option<&Piece>;
    pub fn bounds(&self) -> Option<Bounds>;
    pub fn placement(&self) -> &Placement;
    pub fn contains(&self, p: Pt) -> bool;   // grab box, else closed rings even-odd, else open lines within LINE_HIT_IN
    pub fn rings(&self, tol: f32) -> Vec<Ring>;        // world closed rings of every cuttable piece
    pub fn body_rings(&self, tol: f32) -> Vec<Ring>;   // world closed rings of Body pieces only
}

impl Design {
    /// The one geometry entry point. Memoised; calling it many times per frame is cheap.
    pub fn geometry(&self, shape: &Shape) -> Rc<Geometry>;
    /// Shapes with no Compound ancestor: what is painted, hit at the top level, composed and exported.
    /// Every shape in phase 1; phase 2 adds the filter, so no consumer moves twice.
    pub fn drawn(&self) -> impl Iterator<Item = &Shape>;
}

/// The body contours before hiding: what the Contour panel lists and indexes.
pub fn body_contours(shape: &Shape) -> Vec<Contour>;
```

Leaf evaluation is pure and runs in the shape's local frame, so moving, rotating or flipping never re-runs it.

```rust
pub(crate) fn evaluate_leaf(src: &LeafKey) -> LocalPieces {
    todo!()
    // let base = src.kind.outline(src.size.0, src.size.1);  // Text lays out with active_curve and the catalogue
    // let mut frame = Rect::new((0.0, 0.0), src.size);
    // let mut pieces = vec![LocalPiece::body(filter(base, src.mask.active()), src.style)];
    // for entry in src.effects.enabled() { match entry.effect {
    //   Warp(w)    => warp every piece (contours and Print lines) through warp::apply_in(w, .., frame), using a
    //                 fit measured on the UNFILTERED body when the body is the only piece, else on all pieces;
    //                 with body only and frame = 0..size this is bit for bit today's warp::apply then filter
    //   Offset(o)  => if o.distance_in.abs() < EPS_IN { continue }        // no duplicate body piece
    //                 let input = geometry::boolean(cuttable rings per piece as separate groups, Weld);
    //                 let ring = geometry::offset(input, o.distance_in, round);   // input orientation normalised
    //                 if ring.is_empty() { continue }                     // a deep negative offset vanishes
    //                 pieces.insert(0, LocalPiece::effect(entry.id, ring, o.style.unwrap_or(src.style)));
    //                 frame = frame.union(bounds(ring));
    //   Sticker(s) => backing = geometry::offset(weld(rings per piece), s.border_in, round);
    //                 pieces = vec![LocalPiece { local: Effect(entry.id), contours: backing,
    //                     style: Style { fill: src.style.fill, operation: PrintThenCut, pen: src.style.pen },
    //                     paint: Print([(backing, WHITE)] ++ every piece's paint) }];
    //                 frame = frame.union(bounds(backing));
    // }}
}
```

Each piece is a separate operand to the weld before an Offset or Sticker. `geometry::boolean` with one group never calls `overlay`, and `to_io` does not normalise orientation, so a body ring and an offset ring of opposite winding in one operand could cancel under `NonZero`. Separate operands go through `overlay` and come out normalised. A unit test asserts that offsetting a body plus its own offset ring equals offsetting the ring alone.

### Box rule (Warp and Offset together)

`width_in` and `height_in` keep today's meaning: the frame of the body after its leading Warp entries. Offset and Sticker grow outside the box and never change it, because their distances are absolute inches and must not scale when the frame is resized.

- The effect `Edit` variants are the only doors into a stack. Each refits the box about its centre from `box_extent(kind, before)` to `box_extent(kind, after)`, measured at 1 x 1 through the enabled leading Warps. This generalises `set_warp`. Removing or disabling a leading Warp restores the size exactly. An edit that leaves the leading Warps unchanged leaves the box alone, so the refit is idempotent.
- `refit_text` becomes the same refit with the natural-size ratio folded in, and the Curve eye goes through it.
- `[Warp, Offset]` warps the body in the box, then offsets it. `[Offset, Warp]` offsets first, which grows the frame, then warps body and ring together inside the grown frame and fits them back onto it. The box does not refit for a Warp that follows an Offset. The toolbar never creates that order (`set_warp` inserts before the first Offset or Sticker); only a reorder in Layers does.
- Contour hiding filters the body before the stack, and the warp fit for a body-only frame is measured on the unfiltered body. That is today's order exactly.

### Evaluation and caching (`src/effects/cache.rs`)

```rust
/// One cache shared by every clone of a design (History snapshots, drag `before`, MakeFlow). Not part of
/// the design's value: PartialEq is always true, Clone shares the Rc, serde skips it. Sharing is safe
/// because every hit is an exact key comparison.
#[derive(Default, Clone)]
pub(crate) struct Cache(Rc<RefCell<Slots>>);
impl PartialEq for Cache { fn eq(&self, _: &Cache) -> bool { true } }

struct Slots {
    leaves: HashMap<u64, SmallVec<[LeafSlot; 2]>>,       // (id, key): live shape and drag original both hit
    compounds: HashMap<u64, SmallVec<[CompoundSlot; 2]>>, // phase 2
}
struct LeafSlot { key: LeafKey, local: Rc<LocalPieces>, placed: Option<(Placement, Rc<Geometry>)> }

#[derive(PartialEq, Clone)]
pub(crate) struct LeafKey {
    kind: ShapeKind, size: Pt, mask: ContourMask, effects: Effects, style: Style,
    /// `Some(generation)` when `kind` is Text with a system font (or Flattened with such a part), else None.
    fonts: Option<u64>,
}
```

**Key construction.** `LeafKey::of(shape, fonts)` destructures `Shape` exhaustively with no `..`, assigning each field to the key, to the placement, or to an explicit ignore list (`id`, `hidden`, `link`, `container`, `parent`). A field added to `Shape` later is a compile error there until someone classifies it. This meets the review's "group the inputs so none can be left out" without moving every field into a sub-struct, which would touch every `s.kind` read in the crate.

**Inputs that are not on `Shape`.**

| Input | Read by | Invalidation |
|---|---|---|
| System font catalogue (spec 22) | Text layout, Flattened text parts | `generation()` is in the Text leaf key; a scan landing bumps it, so the next lookup misses |
| Bundled fonts | Text layout | Compiled in; never change in a run |
| Shape library builtins | `ShapeKind::outline` | Compiled in; spec 22 makes the persisted reference a stable name |
| Flatten and offset tolerances | evaluation | Constants |
| Children of a Compound | phase 2 Compound evaluation | Each child's evaluated `Rc<LocalPieces>` and relative placement are in the Compound key |

Evaluation reads the catalogue through the handle spec 22 gives text layout. If spec 22 passes the handle as a parameter rather than a process-wide accessor, the cache holds a clone of it, set when the app opens or creates a design, and shared with every clone.

**Rules.**

- **Exact keys, no counter, no hash.** A hit is one `PartialEq` over the evaluation's inputs. It cannot go stale under direct field writes, undo, redo, load or duplicate ids, and cannot collide.
- **Two levels for leaves.** Local pieces depend on `LeafKey`. The placed `Geometry` also depends on `Placement`. Dragging 200 offset shapes re-places points but never re-runs `geometry::offset` or text layout.
- **Slots keyed by `(id, key)`.** Each id keeps its two most recently used slots, so a caller passing drag originals or a node-edit preview under the same id does not evict the live shape. Ids absent from the live design are swept when the map grows past twice `shapes.len()`.
- **No borrow across evaluation.** `geometry` borrows, looks up, clones the `Rc` on a hit, and drops the borrow. On a miss it evaluates with no borrow held, then borrows mutably to insert. Phase 2 Compound evaluation recurses into `geometry`, so a held borrow would panic with `BorrowMutError`. A test evaluates a Compound inside a Compound.
- **Shared across clones.** Undo, redo and Make entry hit warm entries. `Design` is not `Send` because of the `Rc`; no `Design` crosses a thread today (review, Dismissed).
- **Budget.** Steady state for 200 shapes is 200 key comparisons plus painting. A cold load is 200 evaluations, once. The `#[ignore]` timing test (`tests_spec19.rs`, `a_frame_of_200`) is first extended, before the cache lands, with 50 text shapes, a large imported SVG and a drag frame, and its numbers are recorded as the baseline. Phase 1b adds 200 offset shapes (cold, warm, drag, resize). Phase 2 adds Compound drag, undo and Make entry.

The per-frame `self.design.clone()` in `ui/mod.rs` stays. Replacing it with a revision counter is AUDIT-015's third step and is out of scope. The shared cache means that clone no longer discards cached geometry.

### Call sites that switch to evaluated geometry (phase 1a)

| Site | Today | After |
|---|---|---|
| `ui/canvas.rs` `paint_shapes` | `op_style::paint_shape(shape)` | `design.drawn()` pieces through `op_style::paint_piece` |
| `ui/op_style.rs` `paint_shape`, `paint_placed` | `shape.world_polylines()`, `shape.contours()`, the shape's own op and fill | `paint_piece` and `paint_placed_piece`; `look` per piece style; `Paint::Print` replaces the `Flattened` match |
| `ui/canvas.rs` hit tests, `ui/context_menu.rs` | `design.hit_test` | `hit_test` over `drawn()` using `Geometry::contains` |
| `ui/canvas.rs` snap AABB, marquee | `world_bounds` | `geometry(s).bounds()` |
| `transform.rs` `Frame::of`, `ui/selection.rs` | `world_bounds` for several shapes, the box for one | several shapes: union of `geometry().bounds()`, supplied by the caller; one shape: the source box (decision 2) |
| `design.rs` `align`, `align_units` (spec 23 `units_by`) | `world_bounds` | `geometry().bounds()` |
| `containers.rs` `units_of`, `layout`, `drop_target` | `world_bounds` | `geometry().bounds()` |
| `composition.rs` `units`, `add_unit`, `MatItem` | one item per shape, mat key from the shape | one item per cuttable piece: `MatItem { piece: PieceId, offset_in }`; mat key from `piece.style`; see "Make on pieces" |
| `make_flow.rs` `tool_loads`, mat operations | joins `MatItem.shape_id` to shapes, filters `s.operation == op` | resolves `MatItem.piece` through `design.geometry`, filters `piece.style.operation == op` |
| `bridge.rs` `mat_placements`, `build_mat_path_data` | `world_bounds`, `to_svg_path_d(shape)`, `p.shape.operation`, `p.shape.pen` | `Geometry::placement()`, `to_svg_path_d(&piece.contours)`, `art_type`, `art_tool`, `path_color` and `fiducial_id` from `piece.style` |
| `ui/prepare.rs`, `ui/project_preview.rs` | `p.shape.contours()` | `paint_placed_piece` on the placed piece |
| `design.rs` `to_svg_document`, `bounds_in`; `project.rs` thumbnail | per shape | per drawn shape, per piece |
| `flatten.rs` `outline`, `paint_layers`, `flattened` | `p.world_rings`, `p.world_polylines` on each part | `evaluate_leaf` on each part, placed by the part's own placement |
| `ui/contour_panel.rs`, `ui/layers_actions.rs`, `ui/toolbar/mod.rs` | `all_contours`, `contour_count` | `effects::body_contours(shape)` |
| `path_edit.rs` `read`, `write` | reads warped `all_contours`, then `warp = None` | reads the body through the leading Warps; on write, removes the enabled leading Warps (see "Node editing") |
| `ui/node_edit.rs` overlay | `world_polylines` | `geometry().pieces()` lines |
| `text_ops.rs` `can_split`, `design.rs` `refit_text` | `shape.warp` | the stack; the generalised refit |
| `design.rs` `combine`, `slice` | `world_rings` of each input | `geometry().body_rings()` (warped body, as today); phase 1b refuses inputs with an enabled Offset or Sticker |
| `warp.rs` `apply`, `set_warp` | box-relative apply; box refit on the shape | `apply_in(warp, contours, frame)` (equal to `apply` when the frame is `0..size`); `set_warp` deleted |
| `ui/toolbar/effects.rs` Warp | `shape.set_warp` per frame | `Edit::SetWarp` |

`geometry.rs`, `pens.rs`, `system_fonts.rs`, `shapes_panel.rs` and `ShapeKind::outline` keep raw access. Only `effects::eval` calls `outline` for placed geometry, and `flatten.rs` calls `evaluate_leaf` for parts.

### Make on pieces (review A4)

- `MatItem { piece: PieceId, offset_in }` replaces `shape_id`. The owning shape is `piece.owner`.
- `tool_loads`, the mat's operation list, `art_type`, `art_tool`, `path_color` and `fiducial_id` read `piece.style`. A Sticker piece on a Basic Cut layer is Print Then Cut, so its mat shows a blade load and its payload carries a fiducial.
- **Path and image ids.** A Body piece keeps today's strings exactly: `path_id = "{shape}-{copy}"` and `image_id = "{shape}"`. Any other piece appends the same suffix to both, `"-e{effect}"` or `"-s{n}"`, and under phase 2 nesting `"-c{owner}"` before it. Plain-shape payloads stay byte for byte identical, so spec 35's goldens do not move in phase 1a. The bridge evidence tests pin neither string.
- **Packing.** Pieces of one shape that share a mat key pack as one unit, so an Offset stays registered around its source (decision 3). Pieces with different mat keys split. Attach and container units keep today's rule. Phase 2 adds the Compound rule (below).
- Every piece id in a geometry is distinct. A unit test asserts it for every geometry the suite builds.

### Consuming shapes (AUDIT-007, review A2 and A8, phase 1b)

```rust
pub struct Consumed { pub at: usize, pub parent: Option<u64>, pub subtree: Vec<Shape> }

impl Design {
    /// Removes `ids` and all their descendants. Returns the z slot of the back-most removed root, its
    /// parent, and the removed shapes back to front. Every destructive operation calls it.
    pub(crate) fn consume(&mut self, ids: &[u64]) -> Consumed;
    /// Inserts `shapes` at `at` under `parent`. Returns their ids. Replaces `replace`.
    pub(crate) fn insert_at(&mut self, at: usize, parent: Option<u64>, shapes: Vec<Shape>) -> Vec<u64>;
}
```

The policy is one sentence: an operation consumes what Delete would delete. Combine, Slice, Flatten, the multi-shape Offset and Sticker bakes, Unflatten, Expand and Split Letters all call `consume`, then `insert_at`. `replace` is deleted in the same change.

- A consumed container's children are consumed with it. Their geometry is part of the operation's input, because the user saw them inside it.
- Shapes built from a source (`text_ops::place`, `flatten::placed_part`) start from `container: None, parent: None, link: None` and an empty stack. `insert_at` sets the parent. So `unflatten` restores a container part as a plain shape, and split letters are never containers.
- **Text split policy (A8).** The split-text `Edit`'s `check` refuses text whose stack is non-empty, with the reason "Expand or remove effects to split". It also refuses text that is a container ("Move its contents out to split"), because N letters cannot share one set of children. Wrapping the letters in an effect group that carries the stack waits for phase 2 and is not planned. `ungroup_text` calls `consume` and `insert_at`, then `group`.
- **Baking inputs in phase 1b.** Combine, Slice, Flatten and the multi-shape Offset and Sticker bakes refuse an input whose stack holds an enabled Offset or Sticker, with the reason "Expand effects first". Enabled Warps are honoured, as today. Flatten parts keep their stacks, and `evaluate_leaf` draws them. Phase 2 lifts the refusal by making these operations create groups.
- **Effects on containers.** Offset and Sticker on a container root are refused in phase 1b ("Effects on a container come with groups"). A leaf stack sees only the container's own frame, which is not what the user means. SetWarp keeps today's eligibility.

Tests (phase 1b), one per operation: Weld, Slice, Flatten, multi-shape Sticker, Split Letters and Unflatten on a container with children. Each asserts that no result carries `container` unexpectedly, that no shape has a dangling `parent`, that `group` succeeds after Split, and that composing the result produces each cut path once.

### Panel editing model (review A5)

```rust
// src/ui/toolbar/effects.rs (and the Curve and Contour panels)
pub enum Target {
    /// The toolbar on the current selection. Warp sends `Edit::SetWarp`; Offset and Sticker hold a draft and
    /// send `Edit::AddEffect` once, on Apply.
    New,
    /// One entry, opened from Layers, the inspector list or the context menu, or reached after Apply on a
    /// single root. Every committed change sends `Edit::EditEffect { at, Replace }`.
    Entry(EntryRef),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EntryRef { pub shape: u64, pub row: Row }
```

- **No per-frame pushes.** Nothing a panel does on a frame appends an entry. Warp's live frames send `SetWarp`, which edits one entry in place. Offset and Sticker in `New` mode edit a draft until Apply, as today; Apply sends `AddEffect` once and, for a single root, switches to `Entry`. In `Entry` mode a frame either sends nothing or replaces the one entry.
- **Stable ids.** `EffectId`s come from the stack's monotonic `next` and are never reused. `Replace` with a different variant is refused by `check` (`Blocked::WrongEffect`), so a stale panel cannot turn an Offset into a Warp. Each frame the panel checks that `design.shape(at.shape)` still has that row; when it does not (removed, undone, expanded, shape deleted), the panel closes.
- **One undo step per committed edit.**
  - A slider drag replaces the entry every frame while the pointer is down, and `History::observe` folds the gesture into one step, as today.
  - A typed field commits once. Effect panel `DragValue`s use `update_while_editing(false)`, so the value lands on Enter or focus loss. `length_drag` gains that as a parameter rather than a second function.
  - A click (Corners, Warp type, eye, remove) is one change.
  - Spec 23 resets `gesture_open` on Undo and Redo (AUDIT-018), which this relies on.
- **Toolbar Warp on a stack with an Offset.** `set_warp` edits the first Warp entry or inserts before the first Offset or Sticker, so "warp this shape" warps the body and the box refits. It never appends `[Offset, Warp]`. Remove Warp removes that entry.
- **Kittests.** Drag Bend for 60 frames and assert one Warp entry and one undo step. Reopen an Offset from Layers, type "0.375" and Enter, undo once, and assert the previous distance. Remove the entry while its panel is open and assert the panel closes. Remove the last entry, add another, and assert its id is new.

### Node editing (review A6, replacing decision 7)

- Entering node editing writes nothing. When the stack has enabled leading Warps, a one-line notice says "Editing nodes applies the warp". Pressing Escape before any node changes leaves the design and history untouched.
- `NodePath::read` reads the body through the enabled leading Warps, as today's `read` applies `warp`.
- The first real edit (`NodePath::write`) removes the enabled leading Warp entries in the same mutation as the node change, so bake and edit are one undo step. Disabled leading Warps stay in the stack. Later Offset and Sticker entries stay live and track the edit.
- When `has_trailing_warp()` is true (an enabled Warp after an Offset or Sticker), node editing is refused with the notice "Expand to edit nodes of a warped offset", and the notice offers Expand. Baking such a Warp into the body alone cannot reproduce the outline.
- Undo while node editing is open behaves as today, because entering wrote nothing.
- Spec 17 node edits that add or remove a contour remap `contours.hidden`, or clear it when the remap is ambiguous.
- Tests: Escape after entry leaves `History` with no new step; the cut of `[Warp, Offset]` before and after the first node edit (with no geometric change to the nodes) is equal to 1e-3 in; a trailing Warp refuses entry.

In phase 1a every stack is warp-only, so every Warp is leading and behaviour equals today's.

### Effect edits (`src/effects/ops.rs`, `Edit` variants in `src/edit.rs`)

```rust
// src/edit.rs (spec 23): variants spec 21 adds. Spec 23's baked Offset and Sticker variants are replaced.
pub enum Edit {
    // ...spec 23...
    /// 1a. Toolbar Warp on each root of the selection.
    SetWarp(Option<Warp>),
    /// 1b. One live entry per root (`Reach::EachShape`). In phase 1b `Reach::Together` (welded Offset or
    /// Sticker over several roots) bakes as today; phase 2 makes it an effect group.
    AddEffect { effect: Effect, reach: Reach },
    /// 1b. The only door into an existing entry or pinned row.
    EditEffect { at: EntryRef, op: EffectEdit },
    /// 1b. Bake the selection's enabled effects (and phase 2 groups).
    Expand,
}

pub enum Reach { EachShape, Together }
pub enum EffectEdit { Enable(bool), Remove, MoveTo(usize), Replace(Effect) }   // MoveTo and Replace refused on pinned rows

// src/effects/ops.rs: the arms `Design::apply` calls. `Design::check` holds every refusal named in this design.
impl Design {
    pub(crate) fn set_warp(&mut self, ids: &[u64], warp: Option<Warp>);
    pub(crate) fn add_effect(&mut self, ids: &[u64], effect: Effect, reach: Reach) -> Vec<u64>;
    pub(crate) fn edit_effect(&mut self, at: EntryRef, op: EffectEdit);
    /// Expand. Each piece of each root becomes a plain shape, as today's destructive actions made them:
    /// the Body becomes a Path (or keeps its kind when no Warp is baked), box unchanged; each Offset piece
    /// becomes a `shape_from_rings` shape behind the source in the piece's style; a Sticker becomes today's
    /// `Flattened` whose parts are the backing and the source with its stack cut before the Sticker. Disabled
    /// entries are dropped. Named `bake` because `Design::expand` is spec 02's link expansion.
    pub(crate) fn bake(&mut self, ids: &[u64]) -> Vec<u64>;
}
```

Refusals in `Design::check`, each with a `Blocked` reason: Warp on `Flattened` (today) and on a Compound (phase 2); Offset and Sticker on a container (1b); `Replace` with a different variant; `MoveTo` or `Replace` on a pinned row; Combine, Slice, Flatten and the together-bakes on inputs with an enabled Offset or Sticker (1b); text split per the policy above; node editing with a trailing Warp.

### Persistence and the upgrade (`src/effects/upgrade.rs`)

The upgrade runs at the `Shape` level, not on a `serde_json::Value` tree.

```rust
#[derive(Serialize, Clone, Debug, PartialEq)]
#[derive(Deserialize)] #[serde(from = "ShapeRepr")]
pub struct Shape { /* as above */ }

/// What any saved shape may contain: today's fields plus the legacy ones. `From<ShapeRepr> for Shape` is
/// an exhaustive struct literal, so a field added to `Shape` without one here is a compile error.
#[derive(Deserialize)]
struct ShapeRepr {
    /* every Shape field, each #[serde(default)] where today's is */
    #[serde(default)] warp: Option<Warp>,                 // legacy: becomes the first stack entry
    #[serde(default)] contour_hidden: Option<Vec<bool>>,  // legacy (1b): becomes `contours`
}
```

- `"warp": W` becomes `effects: [{ id: 0, effect: Warp(W) }]` with `next: 1`. A current file has no `warp` key and passes through, so the upgrade is idempotent.
- Text `curve_in` becomes `curve` (1b), handled inside `ShapeKind`'s deserialiser the same way.
- Every stack goes through `Effects::validated`: duplicate ids are renumbered, `next` is raised above every id, and entries the kind cannot take (Warp on `Flattened`, any effect on a Compound that it cannot carry) are dropped.
- Because the upgrade is on `Shape`, it reaches `Flattened.parts` and direct `Shape` parsing for free, and Home's project list does not build a `Value` tree for large imported paths.
- **Format version.** Phase 1a bumps spec 22's `format_version` (the `warp` key moves), phase 1b bumps it again (new effect variants, `curve`, `contours`), and phase 2 bumps it for the `Compound` kind. Spec 22 refuses a newer file with a message instead of hiding it.
- **Load tests (1a).** A pre-spec project with a warp, a curve and hidden contours loads with an equal outline to 1e-6 in. A pre-spec sticker whose parts include a warped word loads and renders identically. A file with duplicate effect ids loads with distinct ids.

### Undo

- Each effect `Edit` is one command applied before `History::observe`, so add, replace, enable, remove, move, set-warp and Expand are each one step. The panel model above gives one step per committed edit.
- `settle` runs before `observe`, so phase 2's carry and Compound refit land in the step that caused them. `settle` and the upgrade are idempotent, so neither makes a phantom step.
- The cache is outside the compared value, and shared across snapshots, so an undone design hits warm entries.
- **History cap (AUDIT-021).** `History.past` becomes a `VecDeque` capped at 200 entries, dropping from the front, with a unit test of the cap. It lands in phase 1a before the cache.

### Module map

```
src/effects/
  mod.rs      re-exports: Effects, Entry, EffectId, Effect, Offset, Sticker, Corners, Style, Curve, ContourMask,
              Row, RowView, rows, Geometry, Piece, PieceId, Local, Paint, Placement, Reach, EffectEdit,
              EntryRef, body_contours
  model.rs    the stack, its crate-private mutators, validation, summaries and icons
  rows.rs     rows(): the Layers and inspector view (1b)
  eval.rs     evaluate_leaf, place, box_extent, refit; evaluate_compound (phase 2)
  cache.rs    Cache, LeafKey::of, Design::geometry, Design::drawn
  ops.rs      impl Design: set_warp, add_effect, edit_effect, bake, consume, insert_at
  upgrade.rs  ShapeRepr and From<ShapeRepr> for Shape
src/edit.rs              (spec 23) SetWarp, AddEffect, EditEffect, Expand; check rules
src/ui/effects_list.rs   chips (three then "+N", tooltips), the expanded list, the inspector list, Edit anchoring
src/ui/tests_spec21.rs
```

A reader answers any geometry question in three files: `cache.rs`, `eval.rs`, `model.rs`.

## Phases

Each phase merges alone, ends green, and leaves no compatibility path for a later phase to remove.

### Phase 1a: evaluated geometry, behaviour identical

Depends on spec 22 (catalogue generation, `format_version`) and spec 35 (golden payloads) being merged. It does not need spec 23: `SetWarp` replaces the existing call to `Shape::set_warp`, which spec 23 will have moved behind `Edit` if it lands first.

1. Subtract first. Introduce `Shape::new` and convert every `Shape { .. }` literal with a codemod. Cap `History` (AUDIT-021). Extend the timing test with text, a large import and a drag frame, and record the baseline (AUDIT-015).
2. `src/effects/` model (Warp only), `upgrade.rs`, `eval.rs` and `cache.rs`. `Shape.warp` becomes `Shape.effects`. `warp.rs` gains `apply_in` and loses `set_warp`. The load tests go green before any consumer moves.
3. Every call site in the table moves to `design.geometry`, including composition, `make_flow` and the bridge on pieces with piece style. The `Shape` geometry methods are deleted in the same change.

Exit: every existing test, snapshot and spec 35 golden passes unchanged; geometry equals today's pipeline to 1e-6 in; the font generation test passes (lay out system-font text, flip the catalogue to ready, assert the geometry changed); the timing test is no slower than the baseline.

### Phase 1b: live single-shape Offset and Sticker

Depends on spec 23 (`Edit`, `check`, `apply`).

1. `Design::consume` and `insert_at`; `replace` deleted; the per-operation subtree tests; the text split policy.
2. `Effect::Offset` and `Effect::Sticker`, `Reach::EachShape` live, `Reach::Together` baking as today through `consume`. `Design::offset` and `Design::sticker` stay only as those bakes until phase 2.
3. `Curve` and `ContourMask` with eyes; `rows`.
4. The panel editing model, the Layers chips and effect list (Edit, eye, remove, reorder), the inspector list, the context menu items (Edit effect, Disable or Enable, Remove, Expand), and `bake`.
5. The node editing refusal for trailing Warps.

### Phase 2: Compounds (effect groups and boolean groups)

One mechanism for multi-shape live effects and live Combine and Slice. Phase 2 may land as two merges (effect groups, then boolean groups), but both go through the same Compound machinery below.

```rust
pub enum ShapeKind { /* .. */ Compound(CompoundOp) }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompoundOp {
    /// Children draw as themselves; the group's own stack sees all of them together.
    Effects,
    /// Children are the inputs of the boolean and draw only through its result.
    Boolean(Combine),        // Combine gains Serialize and Deserialize
}

impl Shape {
    /// A container role or a Compound kind. The single predicate spec 18's `repair` accepts as a parent.
    pub fn holds_children(&self) -> bool;
}

impl Design {
    pub fn hit_test_deep(&self, p: Pt) -> Option<u64>;   // Cmd-click into a Compound
    pub fn set_compound_op(&mut self, id: u64, op: CompoundOp);
}
```

**Pose (review A1).** A Compound owns its `rotation_deg` and flips, as a container does. Its centre and size are derived.

- `settle` keeps spec 18's order (repair, normalise_z, carry, relayout). The Compound refit moves into `relayout`'s reverse-z pass, so a Compound inside a Hug container is refitted before the container measures it.
- **Refit** writes only the centre and size, as the AABB of the children's source boxes (each box's corners through the child's `to_world`, then the Compound's `to_local`). Source boxes, not evaluated pieces, so an Offset ring that does not scale cannot make the refit disagree with a resize.
- **Refit runs only when a descendant changed and the Compound itself did not**, compared against `before`. When the Compound's own box or pose changed, `carry` moves its children and the refit does not run. So a rotate drag writes the Compound's pose from the drag originals every frame, `carry` turns the children by the frame's delta, and nothing rewrites the pose. No spin, no freeze.
- **Carry** extends to Compounds for move, turn, resize and flip. A resize maps each child's centre from the old local box to the new one and scales its size by the factors along the child's own axes. That is exact for children aligned with the Compound and approximate for others; the Compound keeps the box the user dragged until a child changes. A flip mirrors children about the centre.
- Tests: a rotate drag in steps to 90 degrees ends at 90 with children turned by 90; a typed W of 4 in reads back 4 in; a Compound inside a Hug row settles to the same design twice (idempotence).

**Subtrees (review A2).** A Compound is consumed with its subtree through `consume`. Each destructive operation on a Compound input uses its evaluated rings. Tests per operation on a Compound selection assert no orphans and no duplicate cut paths. A Compound dissolves when it has no children, not when it has no visible children.

**Evaluation.** A Compound evaluates in its own local frame, with the same placement level as leaves. Its key is its own `LeafKey` plus, per child in z order, the child's cached `Rc<LocalPieces>` and its placement relative to the Compound. Holding the `Rc` in the key keeps the pointer alive, so pointer equality is exact. Dragging a Compound re-places; it does not re-run the weld or the offset.

```rust
fn evaluate_compound(design: &Design, shape: &Shape, op: CompoundOp) -> LocalPieces {
    todo!()
    // children = design.children(Some(shape.id)).filter(|c| !c.hidden), each through design.geometry (no borrow held)
    // Effects                 => every child's pieces in z order, PieceId.owner left as the child's id
    // Boolean(Unite)          => one piece: all child rings, the Compound's own style, owner = Compound
    // Boolean(Slice)          => exactly two units: geometry::slice's pieces as Slice(0..3) in styles
    //                            (back, front, front); any other count shows the children unchanged, flagged
    // Boolean(op)             => one piece: geometry::boolean(per-child ring groups, op), own style
    // then the Compound's own enabled Offset and Sticker entries, as in evaluate_leaf. Warp is refused.
}
```

**Make (review A4).** Piece ids stay unique under nesting because `owner` is the producing shape. Inside a Compound, composition groups pieces by the child's placement key first (Attach link or container), then applies today's anchor rule within each group, so an attached Pen label keeps registration with its cut. Tests: a Compound whose two children each carry an Offset under a group Offset cuts three distinct offsets once each; an attached Pen label inside a welded Offset lands on its cut's mat.

**Settled choices.**

- **Own style.** An effect group takes the back-most root's fill, operation and pen at creation, as today's welded Offset does. Inspector colour and operation edits on a Compound change that own style, which its Offset (with `style: None`) and Sticker follow. Children keep theirs.
- **Z order.** Wrapping non-adjacent roots puts the group at the back-most root's slot, as Weld does today. A test pins it.
- **Links.** A Compound (not a container) may be a link member; spec 18 needs only that link members are siblings. A link inside a Compound moves in as one unit.
- **Lifted refusals.** Combine and Slice create boolean groups; welded Offset and multi-shape Sticker create effect groups; the phase 1b "Expand effects first" refusals and `Design::offset` and `Design::sticker` are deleted.
- `containers.rs`: `s.container.is_some()` becomes `s.holds_children()` where it asks "can this be a parent"; `can_contain` refuses Compounds; spec 20's `DropTarget::Into(id)` accepts Compounds.

## Synthesis decision

Three runners produced candidates on different models and lenses. Runner A started from "per-shape stack plus a compound kind under spec 18's `parent`". Runner B started from "every multi-input modifier is a compound node". Runner C started from "smallest change; keep evaluation behind `Shape` methods".

**Base: candidate A.** It had the most complete call-site table, the only coherent rule for Warp and Offset together, the two-level leaf memo, and the honest `CompoundOp` split between an effect group and a boolean.

**Grafted from B.** The cache lives inside `Design` with a no-op `PartialEq`. `Frame::of` takes bounds from its caller. A lone shape's pieces split by their own mat key.

**Grafted from C.** Contour hiding stays a source setting. The single-shape selection frame is the source box. The pre-spec round-trip test comes first.

**Changed by the review.** The cache is shared across clones and keyed by `(id, key)` rather than one slot per id with an empty `Clone`. Compounds move to phase 2, which takes C's "multi-shape effects bake in phase 1" for one phase only. `Flattened` stays permanently, and `Effect::Flatten` is dropped. The upgrade moves from the `Design` value tree to `Shape`.

**Rejected.** C's evaluation behind unchanged `Shape` methods. B's SipHash fingerprint key, because a collision is a silently wrong cut.

## Tradeoffs accepted

- We accept a Compound box that is derived on child edits but user-owned under its own gestures, in exchange for every transform path working on Compounds with no feedback loop. Spec 18's Hug containers set the precedent for a derived box.
- We accept a cache field whose `PartialEq` is always true, shared by `Rc` across clones, in exchange for no cache parameter on about 15 pure functions and warm undo and Make. Stale reads are impossible because keys are compared exactly.
- We accept that phase 1b refuses Combine, Slice and Flatten on shapes with live Offset or Sticker, in exchange for never silently welding an offset ring into a body. Phase 2 lifts it.
- We accept that split letters cannot carry effects, in exchange for no N-fold offsets and no lost stickers.
- We accept that a Warp after an Offset does not refit the box and cannot be node-edited without Expand.
- We accept Offset computed in the local frame, so Expand of an Offset on a rotated shape matches today's world-space offset to about 1e-3 in rather than bit for bit.
- We accept approximate child resizing inside a Compound for children rotated relative to it.

## Alternatives considered

- **Box as the unwarped source box, with Warp not stretched back.** Old projects would need a geometric conversion and "identical outlines" on load would fail.
- **A boolean role beside `container`.** A Square kind with a boolean role would claim an outline it never draws.
- **Multi-shape effects stored on a spec 02 link.** Links have no row, no z slot and no style.
- **Store the evaluated result in the model, refreshed by `settle`.** Saved files and every snapshot would carry derived geometry.
- **Cache in the app, passed down.** `compose`, `mat_placements`, `align`, `containers` and their tests would all grow a parameter.
- **A `source: Source` sub-struct on `Shape` for the key.** Meets A3 too, but touches every field read in the crate. The exhaustive destructure gets the same compile-time guarantee in one function.
- **Splitting text into an effect group carrying the stack.** Needs Compounds, and N letters under one Offset is rarely what "split" means.
- **Compound refit every settle, rotation 0.** The original design. It spins or freezes a rotate drag (A1).

## Risks

- **Spec 22's catalogue API.** The design assumes `generation()` is readable wherever text lays out. If spec 22 threads the handle as a parameter, the cache holds the handle; confirm before phase 1a step 2.
- **Spec 23's `Edit` lands late.** Phase 1b needs it. Phase 1a does not.
- **Resize of many offset shapes.** Resizing 200 offset shapes re-runs 200 offsets per frame. If the timing test shows this over 16 ms, evaluate Offset at a coarser tolerance while the pointer is down.
- **Expand equality.** Holds to about 1e-3 in on rotated shapes. Tests compare with that tolerance.
- **Hidden cache state.** A shared field that compares equal regardless of content can surprise a reader. Its doc comment states the rule, and a `#[cfg(test)]` hit and miss counter lets tests assert that moving one shape does not re-evaluate another.
- **Literal and signature churn.** `op_style` changes signature to pieces, and every `Shape` literal changes again. Both are mechanical once `Shape::new` exists.

## Next implementation step

After specs 22 and 35 merge: add `Shape::new` and convert every literal, cap `History`, and extend the timing test. Then write `model.rs` (Warp only), `upgrade.rs` and the load tests with `Design::geometry` returning today's pipeline output, before any consumer moves.

## Orchestrator decisions (2026-10-12, revised after review)

1. Curve and Contour rows get an eye toggle too, so every row in the effect list behaves the same. **Revised:** each through a model, not a flag beside the field: `Text.curve: Option<Curve { diameter_in, enabled }>` and `Shape.contours: ContourMask { hidden, enabled }`. The Curve eye goes through the text refit.
2. Single-shape selection frame stays the source box; multi-selection, marquee, align, snap and hit-test use evaluated bounds. Unchanged.
3. An Offset on the same mat as its source packs with it as one unit. Unchanged.
4. **Revised:** Expand on a Sticker produces `Flattened` in every phase. `Flattened` is kept as the baked Print Then Cut kind and is not retired; there is no `Effect::Flatten` and no upgrade rewrite.
5. A boolean group left with one child is kept (the user can Expand or remove it). **Added:** a Compound with no children dissolves; one whose children are all hidden does not.
6. Warp on a Compound is refused in both phases, with a tooltip saying why. Unchanged.
7. **Replaced:** entering node editing writes nothing and shows "Editing nodes applies the warp". The first real node edit bakes the enabled leading Warps, in the same undo step. A Warp after an Offset or Sticker refuses node editing and offers Expand.
8. **Added:** the phases are 1a (behaviour identical), 1b (live single-shape Offset and Sticker, Layers list, Expand) and 2 (Compounds: effect groups and boolean groups).
9. **Added:** splitting text refuses a non-empty stack and a container text.
10. **Added:** in phase 1b, Combine, Slice, Flatten and the multi-shape bakes refuse inputs with an enabled Offset or Sticker, and Offset and Sticker on a container are refused.

## Orchestrator decisions on the revision (2026-10-12)

- Phase 1b refusals accepted, except Offset on a container: keep today's behaviour (offset the container's own frame) by treating the container as a single shape for Offset in 1b.
- Text split refuses when effects are present (as designed).
- Compound resize of rotated children stays approximate in phase 2; document it.
- Catalogue API: coordinate with spec 22's merged `FontCatalogue` before 1a step 2; adapt the cache key to whatever 22 ships.
- Weld-off Offset over several shapes: verify today's behaviour in 1a and preserve it.
