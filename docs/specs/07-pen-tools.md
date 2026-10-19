# 07 Pen sizes and colours

**Wave 1. Depends on 01. Size M.**

Reference: `reference/pen-colours.png`.

## Problem

A Pen layer has a free RGB colour and no pen size. In Kerf a pen layer's colour is a physical pen. You pick a pen size, then a named pen colour, and Make asks you to load that pen.

## Goal

Choosing the Pen operation turns the colour swatch into a pen picker. The pen choice reaches the canvas, the payload and the Load step.

## Data shape

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PenSize { ExtraFine, #[default] Fine, Medium, Bold }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Pen { pub size: PenSize, pub colour: usize }   // index into PEN_COLOURS

pub struct Shape {
    // existing fields
    #[serde(default)]
    pub pen: Pen,
}
```

`Operation` stays unchanged, so old projects load. `pen` only matters when `operation == Pen`. When a shape becomes a Pen layer, pick the pen colour nearest its current fill.

| Size | Label | Nib |
|---|---|---|
| ExtraFine | XF - Extra Fine Point (0.3mm) | 0.3 mm |
| Fine | F - Fine Point (0.4mm) | 0.4 mm |
| Medium | M - Medium Point (1.0mm) | 1.0 mm |
| Bold | B - Bold Point (2.0mm) | 2.0 mm |

`PEN_COLOURS` is a table of about 24 named colours in `src/pens.rs`, each `(name, hex)`, ordered dark to light then by hue. Start it with Black, Midnight, Armadillo, Moccasin, Tawny, Wine and Raspberry from the reference and fill out the hue wheel.

## Picker

When every selected shape is a Pen layer, the inspector's Colour swatch opens a popover (spec 01) instead of the RGB picker:

- A size dropdown at the top, as in the reference.
- A scrolling list of colours, each row a 20px rounded swatch and the name. The current colour has the `ACCENT_HOVER` row fill.
- Picking sets `pen` on every selected shape and sets `fill` to the pen's hex, so the canvas, Layers and Project Preview show the right colour.

## Canvas

Open Pen paths draw at the nib width (`nib_mm / 25.4` inches times the zoom), with a minimum of 1px, instead of a fixed hairline.

## Make

- `pathColor` in the payload is the pen colour's hex. It already comes from `fill`, so check it still matches.
- `MakeFlow::tool_loads` dedupes pens by size and colour, not just by tool (`docs/evidence-ui.md`, section 6).
- The Load step names each pen: "Load Black Fine Point Pen in Clamp A". With two pens it shows one line each, in plot order.

## Files

- **Owns.** `src/pens.rs` (new), the pen popover in `src/ui/toolbar/pen.rs` (new).
- **Shared edits.** `src/design.rs` (the `pen` field), `src/ui/inspector.rs` (open the pen popover for Pen layers), `src/ui/canvas.rs` (stroke width), `src/make_flow.rs` and `src/ui/set_load_go.rs` (pen loads), `src/bridge.rs` tests.

## Acceptance

- [ ] Unit test: two Pen shapes in Black Fine and Wine Fine give two pen loads; two in Black Fine give one.
- [ ] Unit test: a project saved before this change loads with the default pen.
- [ ] Kittest test: set a shape to Pen, open the swatch, pick Medium and Wine, and the shape's `pen` and `fill` both change.
- [ ] The Load step lists one line per pen. Snapshot `07-load-pens.png`.
- [ ] Snapshot `07-pen-picker.png` of the open picker.
