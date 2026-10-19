# 19 Operation-specific canvas rendering

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec19.rs` pass.

**Wave 3. Depends on 06, 07. Size M.** Requested directly by the user: "different operations should show different borders on the canvas, can we add that?" The product's goal is to make each operation read clearly on the canvas and the canvas easy to use.

## Problem

Kerf paints every shape the same way (filled outline), except that Pen strokes use the nib width (spec 07). Nothing tells you at a glance what will be cut, drawn, scored, perforated or printed. Each operation should read differently on the canvas instead.

## Step 1: decide the rendering (do this first, write it down)

Decide how Kerf renders each operation on its canvas, and write it down before coding:
- One row per operation, covering stroke style, dash arrays, fill, opacity, colours, any pattern or texture, and the selected vs unselected state.
- Cover every operation by its `lineType` (CUT, WAVE, PERF_BASIC, DRAW, MIDAS_*, SCORE, DEBOSS_FINE, ENGRAVE, PRINTCUT, GUIDE).

Append the decisions as a new section of `docs/evidence-ui.md` ("Operation rendering on the canvas"), one row per operation. Where a choice is still open, say so rather than guessing.

## Step 2: rendering in Kerf

Implement one rendering per `Operation`, in Kerf's tokens. The expected shape (confirm or refine from Step 1):

| Operation | Canvas rendering |
|---|---|
| Basic Cut | Solid fill in the layer colour (as now). |
| Wavy Cut | Fill, with the outline drawn as a small sine wave so the edge reads as wavy. |
| Perforate | Fill, with a dashed outline (short dash, short gap) in a darker tone of the fill. |
| Pen | No fill, stroke at the nib width in the pen colour (spec 07). Closed shapes draw their outline only. |
| Foil (Fine, Medium, Bold) | No fill, stroke at the foil weight with a subtle metallic gradient or sheen. |
| Score | No fill, dashed stroke (a short score dash). |
| Deboss | Fill lighter than the layer colour with an inner shadow suggesting a pressed line. |
| Engrave | Fill with a fine hatch or engraved-line texture at low contrast. |
| Print Then Cut | The printed artwork, plus a thin cut-line outline offset by the print bleed, and a faint drop shadow (it will be a sticker-like printed piece). |
| Guide | No fill, 1px dashed stroke in `TEXT_MUTED` at reduced opacity; never selected through by default (clicking it still selects it). |

Rules:
- Rendering is a pure function from (operation, shape, colour, zoom) to paint instructions in a new module, unit tested; `canvas.rs` only calls it. Dash lengths and wave amplitudes are in screen points so they look the same at every zoom.
- Rendering also applies in the Project Preview mat thumbnails and the Prepare mat preview (spec 09), so the mats look like the canvas.
- Hidden contours (Contour panel) and flattened parts (spec 04) still render correctly.
- Performance: a design with 200 shapes still renders a frame under 16 ms on the release build (add a quick benchmark test or timing assertion behind `#[ignore]`, and run it once).

## Step 3: going further

- **Operation legend.** A small pill in the canvas corner (next to the zoom pill) lists the operations present in the design with their swatch style ("Cut, Pen, Score"); clicking one selects every shape with that operation.
- **Layers.** Each Layers row shows the operation as a small icon (spec 01 operation icons) next to the swatch.
- **Hover hint.** Hovering a shape for 0.5 s shows a tooltip "Basic Cut, Cardstock" style line (operation plus, once a material is chosen in the flow, the material), using `ui::tooltip`.
- **Make preview toggle.** View menu: "Show as made" toggles a mode that renders everything exactly as it will come out (cuts as outlines only, pens as lines, scores dashed, print as print), so users can check before Make.

## Files

- **Owns.** `src/ui/op_style.rs` (new: pure rendering per operation), `src/ui/op_legend.rs` (new), `src/ui/tests_spec19.rs` (new), the new section in `docs/evidence-ui.md`.
- **Shared edits.** `src/ui/canvas.rs` (`paint_shapes` calls `op_style`; keep small), `src/ui/project_preview.rs` and `src/ui/prepare.rs` (thumbnails use `op_style`), `src/ui/layers_panel.rs` (operation icon), `src/ui/chrome.rs` (View menu toggle), `src/ui/mod.rs` (`mod` lines, state for the toggle). Spec 16 is restyling prepare.rs and spec 18 touches layers_panel.rs and canvas.rs; expect a rebase.

## Acceptance

- [ ] `docs/evidence-ui.md` has the operation rendering section with file:line evidence for each operation, or an explicit "no evidence" note.
- [ ] Unit tests: Score and Perforate produce dashed strokes with the documented dash lengths independent of zoom; Pen produces no fill; Guide produces no fill and reduced opacity; Wavy produces a stroke whose points deviate from the outline by the documented amplitude.
- [ ] Kittest: a square switched from Basic Cut to Score renders no fill pixels inside it (sample the centre pixel of the snapshot).
- [ ] Kittest: clicking "Score" in the operation legend selects exactly the Score shapes.
- [ ] Kittest: View > Show as made renders a Basic Cut square as an outline only.
- [ ] Snapshot `19-operations.png`: one shape of every operation side by side, labelled, and reviewed so each operation reads distinctly.
- [ ] README describes operation rendering, the legend and Show as made.
