# 46 Editable grid

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec46.rs` pass.

**Wave 10. Depends on 22, 23, 34. Size S.** The grid is fixed at 1 in with eight subdivisions (10 cm with ten in metric) and shapes snap to the 1/8 in minor line. A quilter works in 1/4 in seams, a laser user in 5 mm, a sticker maker in 1/2 in columns. None of them can set it.

## Problem

- `canvas::major_pt`, `ticks` and `multiplier` hard-code the pitch and subdivisions, and `snap::best` hard-codes the snap grid as the minor pitch.
- View offers Full grid, Partial grid and nothing else. There is no grid origin, so a design that starts 0.5 in from the mat edge never lines up with the lines.
- Snap to grid and snap to shapes share one magnet toggle, so turning off the grid snap also loses the smart guides.

## Goal

### Model

- `Design::grid: GridSettings`, `#[serde(default)]`, with `major_in: f32` (the pitch between major lines, stored in inches), `subdivisions: u32` (1 to 16), `origin_in: Pt` (where the lines start, default the artboard's top-left) and `visible: GridVisibility` (Off, Major, Full). The default reproduces today's grid in each unit system, so an untouched project draws exactly as before. Bump `FORMAT_VERSION`.
- The snap grid is `major_in / subdivisions`. `snap::best` takes it as a parameter it already has (`grid_in`), so the only change there is the caller.
- `View::snapping` splits into `snap_grid` and `snap_objects` on `Design` rather than the view, so the choice travels with the project. The magnet button keeps toggling both; spec 47 adds the popover that sets each.

### Grid panel

- The ruler corner, which already toggles units, opens a **Grid** popover on right-click, and View gains Grid settings... (Cmd+Option+G is taken; use Cmd+').
- The popover holds Spacing (an expression field, so `1/4in` and `5mm` both work), Subdivisions (a count field), Origin X and Y (expression fields, with a Reset to mat corner button and a Use selection button that puts the origin on the selected shape's top-left), Show (Off, Major lines, Full) and Snap to grid. Presets below fill the fields in one click: 1/8 in, 1/4 in, 1/2 in, 1 in, 1 mm, 5 mm, 1 cm. Each change is one undo step.
- Switching units in View leaves the stored pitch alone, so a 1 in grid shows as 2.54 cm; only the presets and the field display change. Today's unit switch silently changes the pitch, which stops.

### Drawing

- `paint_grid` and `paint_rulers` read `major_in`, `subdivisions` and `origin_in`. The ruler's labelled interval and tick pattern follow the subdivision count (every subdivision a tick, the half way one longer when the count is even). `multiplier` keeps doubling the pitch as the zoom halves, so a 1 mm grid at 25 % does not become a grey wash; the subdivision lines drop out first, then the major lines thin, as now.
- Lines stop being drawn when they would fall closer than 4 px, and the ruler says so with a faint "grid hidden at this zoom" in its corner tooltip.

## Files

- **Owns.** `src/grid.rs` (new: `GridSettings`, defaults per unit, the pitch and label rules, the zoom thinning; pure), `src/ui/grid_panel.rs` (new), `src/ui/tests_spec46.rs` (new).
- **Shared edits.** `src/design.rs` (`grid`, `snap_grid`, `snap_objects`), `src/ui/canvas.rs` (`paint_grid`, `paint_rulers`, `major_pt`, `ticks` and the snap threshold read the settings; around 30 lines change), `src/snap.rs` (callers pass the grid pitch), `src/edit.rs` (`Edit::Grid(GridSettings)`), `src/ui/commands.rs`, `src/ui/chrome.rs`, `src/project.rs` (format bump).

## Acceptance

- [ ] Unit test: the default `GridSettings` draws the same line positions as today at 100 % for both unit systems.
- [ ] Unit test: a 5 mm grid with origin at 0.5 in snaps a shape edge at 0.5 in + 5.1 mm to 0.5 in + 5 mm.
- [ ] Unit test: at 10 % zoom a 1 mm grid draws no subdivision lines and major lines at least 4 px apart.
- [ ] Unit test: a project saved before this spec loads with the default grid; one saved with a 1/4 in grid round-trips.
- [ ] Kittest: open the Grid popover, click the 1/4 in preset; the canvas draws four subdivisions per inch and a dragged shape snaps at 0.25 in.
- [ ] Kittest: turn Snap to grid off and leave objects on; a drag no longer snaps to lines but still snaps to another shape's edge.
- [ ] Kittest: switch units to centimetres; the grid lines do not move.
- [ ] Snapshots `46-grid-panel.png`, `46-grid-5mm.png` (a 5 mm grid with its ruler).
- [ ] `docs/guide.md` describes the grid settings, presets, origin and the two snap toggles, and the Units section's grid sentence is updated.
