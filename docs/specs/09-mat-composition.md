# 09 Mat composition

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec09.rs` pass.

**Wave 2. Depends on 00, 02 and 07. Size L.**

Reference: `reference/project-preview.png`.

## Problem

Today Kerf makes one mat per operation and keeps every shape where it sits on the canvas. Instead it should sort a project into mats by material colour, keep attached shapes together on one mat, pack everything else, and label each mat with all the operations it contains ("Score, Pen, Perforate"). The mat thumbnail is filled with the layer colour (the black mat in the reference).

## Goal

One pure function decides the mats. Project Preview, Prepare and the bridge payload all read from it, so what you see is what is sent.

## Data shape

New module `src/composition.rs`:

```rust
pub struct Mat {
    pub print_then_cut: bool,
    pub colour: Color32,                 // the material colour, the mat thumbnail fill
    pub items: Vec<MatItem>,
    pub operations: Vec<Operation>,      // deduped, in plot order
}

pub struct MatItem {
    pub shape_id: u64,
    pub offset_in: Pt,                   // position on the mat's cutting area
}

pub fn compose(design: &Design, mat: MatSize) -> Vec<Mat>;
```

## Rules

These follow the inferred rule in `docs/evidence-ui.md` section 6.

1. **Eligible layers.** Visible shapes whose operation is not Guide.
2. **Units of placement.** An Attach link (spec 02) is one unit and keeps its members' relative positions. Every other shape is its own unit. Groups do not stay together on the mat.
3. **Mat key.** A unit's key is (Print Then Cut or not, colour). For a cut unit the colour is its fill. Draw, Score, Foil, Deboss and Engrave shapes take the colour of the cut shape they are attached to. A non-cut unit with no attached cut shape goes on a mat keyed by its own fill.
4. **Order.** Print Then Cut mats first, then by colour in first-use order on the canvas.
5. **Packing.** Within a mat, place units left to right, top to bottom, inside the cutting area less `MAT_PADDING_PT`, with 0.25 in between. A unit too large for the mat goes on its own overflow mat with a warning flag.
6. **Operation label.** Dedupe the mat's operations and sort Print Then Cut first, then plot order: Score, Pen, Deboss, Engrave, Foil, Perforate, Wavy, Cut. Join labels with ", ".

## Consumers

- **Project Preview** (`src/ui/project_preview.rs`). One card per `Mat`. The thumbnail's cutting area is filled with `Mat::colour` and shows the packed items. The card lines are "Load N", the joined operation label, the mat size name in the current units (spec 00), and "On Mat". The Mat Details header collapses and expands on click (minus and plus icon), and the info icon shows the mat tooltip text from the evidence.
- **Prepare** (`src/ui/prepare.rs`). The mat list and preview use the same `compose` output.
- **Bridge** (`src/bridge.rs`). `mat_placements` takes a `Mat` and uses `MatItem::offset_in` instead of the canvas position. Mirror and copies keep working on top.
- **Make flow** (`src/make_flow.rs`). Today one pass cuts the whole design. `MakeFlow` gains `mats: Vec<Mat>` and `current: usize`. Set, Load and Go run for the current mat, and the step header reads "Mat 1 of 3". After the mat is unloaded on the Done screen, Next Mat advances to Set for the following mat. The flow finishes after the last mat. Tool loads are computed per mat.

## Files

- **Owns.** `src/composition.rs` (new), `src/ui/project_preview.rs`, `src/ui/prepare.rs`.
- **Shared edits.** `src/bridge.rs` (`mat_placements` signature), `src/make_flow.rs` (iterate mats), `src/ui/set_load_go.rs` (the "Mat N of M" header and Next Mat), `src/design.rs` (remove `operations` and `only` once nothing calls them).

## Acceptance

- [ ] Unit test: a black cut square and a red cut square give two mats, ordered by canvas order.
- [ ] Unit test: a pen star attached to a black cut square gives one mat labelled "Pen, Basic Cut".
- [ ] Unit test: an unattached pen star gives its own mat.
- [ ] Unit test: an attached pair keeps its relative offset on the mat, and an unattached pair is packed to the top left.
- [ ] Unit test: a Print Then Cut shape's mat comes first.
- [ ] Unit test: the existing bridge payload tests pass after updating their expected transforms for packing.
- [ ] Kittest test: the Make flow drives two loads to completion for a two-colour design.
- [ ] Snapshot `09-preview.png` with a coloured mat thumbnail and a joined operation label.
