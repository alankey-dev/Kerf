# 48 Zoom to fit, zoom to selection and view navigation

**Wave 11. Depends on 22, 23. Size S.** Every editor has Fit (Cmd+0), Zoom to selection and 100 % a key away, and a hand tool for panning. Kerf has Zoom In, Zoom Out and Reset Zoom, and pans by scroll only.

## Problem

- Reaching a small shape on a 24 in mat means several Cmd+= presses and a scroll hunt.
- `AppCommand::ResetZoom` shows actual size, which on a 12 x 12 mat at a laptop window shows a quarter of the board. There is no "show me the whole mat".
- There is no keyboard pan, no hand tool and no Space-to-pan, so the Pen tool cannot reach past the viewport without losing the path in progress.
- The zoom pill shows a percentage but offers no menu.

## Goal

### Commands

| Command | Key | Does |
|---|---|---|
| Zoom to fit | Cmd+0 | fits the artboard plus a 24 px margin, as `View::fit` does at open; ResetZoom moves to Cmd+1 and is renamed Actual size |
| Zoom to selection | Cmd+2 | fits the selection's frame plus a margin, capped at 800 %; disabled with nothing selected, reason "Select something to zoom to" |
| Zoom to all | Cmd+Shift+0 | fits the union of every shape's frame, or the artboard when the design is empty |
| Zoom in and out | Cmd+= and Cmd+- | as today, but about the pointer when it is over the canvas and about the view centre otherwise |
| Zoom to | none | the zoom pill opens a menu of 25, 50, 100, 200, 400 and 800 % plus the three fits |

- Every row is an `AppCommand` and runs through `edit::run`, so the palette lists them and the Help menu's Keyboard shortcuts shows them. Zoom changes are not undo steps, as today.
- Pinch and Cmd+scroll zoom about the pointer, as the trace panel already does.

### Panning

- Holding Space turns the pointer into a hand over the canvas, for every tool, and dragging pans. Releasing Space returns the tool with any Pen path in progress intact. While a text is being edited Space types a space, so the hand needs the canvas to have the keyboard.
- Middle-button drag pans. Two-finger scroll pans as today.
- The arrow keys pan when nothing is selected and no tool has the keyboard; with a selection they nudge, as today.
- `View::pan_pt` clamps so the artboard can never leave the viewport entirely; at least 48 px of it stays visible on each axis.

### Zoom pill

- The pill shows the percentage and opens the Zoom to menu on click. Its tooltip names Cmd+0, Cmd+1 and Cmd+2. The magnet and legend pills stay beside it.
- Each zoom and fit animates over 120 ms, interpolating the zoom and pan together, with no animation under `egui_kittest` so snapshots are deterministic. The animation is skipped when the user prefers reduced motion where the platform reports it.

## Files

- **Owns.** `src/ui/view_nav.rs` (new: fit calculations, pointer-anchored zoom, the Space hand, the pan clamp, the animation; the maths pure and unit tested), `src/ui/zoom_menu.rs` (new), `src/ui/tests_spec48.rs` (new).
- **Shared edits.** `src/ui/canvas.rs` (`View` gains `fit_to(rect)` and the pan clamp; the scroll and pinch handlers anchor on the pointer; around 20 lines), `src/edit.rs` (`AppCommand::ZoomToFit`, `ZoomToSelection`, `ZoomToAll`, `ZoomTo(pct)`), `src/ui/commands.rs`, `src/ui/chrome.rs` (View rows and the key changes), `src/ui/op_legend.rs` or wherever the zoom pill lives (the menu).

## Acceptance

- [ ] Unit test: Zoom to selection on a 1 x 1 in shape in a 1000 x 800 px viewport gives 800 % (the cap) and centres the shape; on a 10 x 2 in shape the zoom fits the width with the margin.
- [ ] Unit test: a pointer-anchored zoom keeps the world point under the pointer fixed within 0.5 px.
- [ ] Unit test: the pan clamp keeps 48 px of the artboard visible at every zoom step.
- [ ] Kittest: zoom in three times, Cmd+0; the whole artboard is visible with the margin.
- [ ] Kittest: select a small shape, Cmd+2; the shape fills the viewport and the pill shows the new percentage.
- [ ] Kittest: Pen tool, place two anchors, hold Space and drag, release Space, place a third anchor; the path has three anchors.
- [ ] Kittest: click the zoom pill, choose 200 %; the pill shows 200 %.
- [ ] Snapshot `48-zoom-menu.png`.
- [ ] `docs/guide.md` describes the fits, their keys, Space to pan and the zoom menu, and the Editor section's Keyboard sentence lists Cmd+0, Cmd+1 and Cmd+2.
