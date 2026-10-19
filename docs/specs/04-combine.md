# 04 Combine: Slice and Flatten

**Wave 1. Depends on 01. Size M.**

Reference: `reference/combine.png`.

## Problem

Combine is a text list of Weld, Unite, Subtract, Intersect and Exclude. It should show icon tiles and add two more operations: Slice and Flatten (with Unflatten).

## Goal

The Combine popover is a tile grid with Slice and Flatten added, each with a clear enabled rule.

## Layout

A `tile_grid` (spec 01) in two rows:

| Row | Tiles |
|---|---|
| 1 | Slice, Weld, Flatten (Unflatten when the selection is one flattened shape) |
| 2 | Unite, Subtract, Intersect, Exclude |

## Operations

| Operation | Enabled when | Result |
|---|---|---|
| Slice | exactly 2 units selected | Up to three new shapes: A minus B, B minus A, and A intersect B. Empty results are skipped. Each piece keeps the fill and operation of its source. The intersection takes the front shape's style. The inputs are removed. |
| Weld | 2 or more | Unchanged. |
| Unite, Subtract, Intersect, Exclude | 2 or more | Unchanged. |
| Flatten | 2 or more, or 1 group | One Print Then Cut shape. See below. |
| Unflatten | 1 flattened shape | Restores the original shapes. |

A unit is a shape or a whole link from spec 02. Slice on a two-member group counts as two units.

### Flatten

Flatten turns layers into one printable image whose outline is cut. Kerf models it without a raster:

```rust
ShapeKind::Flattened { parts: Vec<Shape>, view_box: Pt }
```

- `parts` are the input shapes, stored in the flattened shape's local frame.
- The canvas draws every part with its own fill, so it still looks like the original artwork.
- The cut outline is the weld of all parts.
- The operation is forced to `PrintThenCut`.
- Unflatten maps `parts` back to world space and inserts them where the flattened shape was.

Make treats a flattened shape like any Print Then Cut shape. Printing itself is out of scope.

## Files

- **Owns.** `src/ui/toolbar/combine.rs`, `src/flatten.rs` (new, the `Flattened` geometry and drawing helpers).
- **Shared edits.** `src/design.rs` (the `Combine::Slice` and `ShapeKind::Flattened` variants and their match arms), `src/geometry.rs` (a `slice` helper next to `boolean`), `src/ui/canvas.rs` (draw `Flattened` parts in `paint_shapes`).

## Acceptance

- [ ] Unit test: slicing two overlapping squares gives three shapes whose areas sum to the union's area.
- [ ] Unit test: slicing two disjoint squares gives two shapes (the intersection is empty).
- [ ] Unit test: flatten then unflatten returns shapes equal to the inputs within 0.001 in.
- [ ] Kittest test: Slice is disabled with three shapes selected and enabled with two.
- [ ] A flattened shape shows Print Then Cut in the inspector and draws its parts in their own colours. Snapshot `04-flatten.png`.
- [ ] Snapshot `04-combine.png` of the open popover matches the reference layout.
