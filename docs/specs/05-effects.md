# 05 Effects: Sticker and Warp

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec05.rs` pass.

**Wave 2. Depends on 01 and 04 (Sticker builds a flattened shape). Size L.**

Reference: `reference/effects.png`.

## Problem

Effects opens straight into the Offset form. It should open a menu of Offset, Sticker and Warp, plus Curve for text (spec 12).

## Goal

Effects opens an icon menu. Offset keeps working. Sticker and Warp are new.

## Menu

`icon_row`s (spec 01): Offset, Sticker, Warp. Picking one swaps the popover body for that effect's panel, with a back chevron in the title row. Each panel has an Apply button. Warp also updates live.

## Offset

Unchanged behaviour, moved into the panel. Distance goes through `src/units.rs` (spec 00).

## Sticker

Turns the selection into one Print Then Cut sticker with a white border.

| Control | Default | Range |
|---|---|---|
| Border | 0.125 in | 0 to 1 in, shown in the current units |
| Corners | Round | Round or Sharp |

Apply does this as one undo step:

1. Weld the selection's outlines and offset the result by Border. This is the backing.
2. Build a `ShapeKind::Flattened` (spec 04) whose parts are a white backing shape followed by the original shapes.
3. Replace the selection with that shape. Its cut line is the backing outline.

## Warp

Bends a shape inside an envelope. The warp is a property of the shape, so it stays editable and removable.

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum WarpStyle { Arch, Bulge, Wave, Flag, Rise }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Warp {
    pub style: WarpStyle,
    pub bend: f32,        // -1.0 to 1.0, shown as -100 to 100
    pub vertical: bool,   // false bends along the width, true along the height
}

pub struct Shape {
    // existing fields
    #[serde(default)]
    pub warp: Option<Warp>,
}
```

The warp applies in `Shape::contours()`, after the base outline and before placement, so the canvas, hit-testing, Combine and Make all see the warped geometry. It works on the shape's local box, with `u` and `v` running 0 to 1 across the width and height:

| Style | Displacement of a point at (u, v), with `b = bend * height` |
|---|---|
| Arch | Every point moves up by `b * (1 - (2u - 1)^2)`. Top and bottom bend together. |
| Bulge | Points move away from the middle line by `b * (1 - (2u - 1)^2) * (2v - 1)`. Negative bend pinches. |
| Wave | Points move up by `b * 0.5 * sin(2 * pi * u)`. |
| Flag | As Wave, scaled by `u`, so the left edge stays fixed. |
| Rise | Points move up by `b * u`, a shear that keeps edges straight. |

For `vertical: true`, swap the roles of `u` and `v` and of x and y. Curves are flattened at `FLATTEN_TOL_IN` before warping and come out as line contours. Refit the shape's width and height to the warped bounds so the selection frame hugs it.

Panel controls: a row of style tiles, a Bend slider from -100 to 100 with a number field, a Horizontal or Vertical toggle, and Remove Warp when the shape already has one.

## Files

- **Owns.** `src/ui/toolbar/effects.rs`, `src/warp.rs` (new, pure envelope maths).
- **Shared edits.** `src/design.rs` (the `warp` field, the hook in `contours()`, a `sticker` method next to `offset`).

## Acceptance

- [ ] Unit tests in `src/warp.rs`: bend 0 is the identity for every style, Rise keeps straight lines straight, and Arch moves the top-centre point by exactly `bend * height`.
- [ ] Unit test: sticker on a 1 in square with a 0.125 in border gives a flattened shape with a 1.25 in square cut outline.
- [ ] Kittest test: apply Arch at 50, save, reload, and the warp is still there and editable.
- [ ] A project saved before this change loads with `warp: None`.
- [ ] Snapshots `05-effects-menu.png`, `05-sticker.png` and `05-warp-arch.png`.
