# 50 Select Same, Select Inverse and lasso

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec50.rs` pass.

**Wave 11. Depends on 22, 23, 19, 34. Size M.** Illustrator's Select > Same and Affinity's Select Same pick every object that shares a fill, a stroke or a font, and a lasso picks the awkward ones a marquee cannot. Kerf has click, Shift-click, marquee, Select All and the operation legend.

## Problem

- Recolouring all the red pieces in a forty piece design means forty Shift-clicks or a hunt in Layers.
- A marquee is a rectangle. Picking the six shapes along a diagonal, without the two in between, cannot be done in one gesture.
- Select All picks everything; there is no Deselect the rest, Select Inverse or Select All in this container.
- The legend pill selects by operation, which is one case of a general need, written once in `op_legend.rs`.

## Goal

### Select Same

- A **Select Same** submenu in the Edit menu, the context menu and the palette, with rows Colour, Operation, Pen, Font, Shape kind, Effects and Everything alike (all of the above). Each takes the selection's shared value and selects every visible, unlocked shape with the same value, inside the current component master when one is open, else on the canvas. Rows whose value differs across the selection are greyed with "Selection has mixed colours". Nothing selected greys the whole submenu.
- Colour compares the fill, or the pen for Pen layers, or the pattern asset for pattern fills. Font compares family and style. Shape kind compares the `ShapeKind` variant, and for `Parametric` (spec 49) the `ParamKind`.
- The legend pill's click becomes Select Same > Operation for the clicked operation, so the rule lives once in `src/select.rs`.
- Shift with a Select Same row adds to the selection instead of replacing it.

### Select Inverse, Select in container, Select by type

- **Select Inverse** (Cmd+Shift+I) selects every selectable shape not in the selection.
- **Select All in container** (Cmd+A while the selection sits inside one container) selects the container's children and no more; Cmd+A again widens to everything, as Figma does.
- **Select All text** and **Select All paths** rows in the submenu, for the two kinds people recolour or refont together.
- Tab and Shift+Tab with one shape selected move the selection to the next or previous shape in Layers order, so a keyboard user can walk the design.

### Lasso

- Dragging a marquee with Option held draws a freehand lasso instead. The lasso closes on release and selects every shape whose evaluated outline has a node inside it, or, for a marquee as today, whose frame intersects. Holding Shift as well adds to the selection. The lasso draws as a 1 px dashed accent line with a faint fill.
- A lasso shorter than 8 px is a click. Escape cancels a lasso in progress.
- Containers and locked layers follow the marquee rules in `canvas.rs` unchanged.

## Files

- **Owns.** `src/select.rs` (new: the Same predicates, inverse, container scope, Tab order, point-in-lasso; pure), `src/ui/lasso.rs` (new: the gesture and drawing), `src/ui/tests_spec50.rs` (new).
- **Shared edits.** `src/edit.rs` (`Edit::SelectSame(Match)`, `SelectInverse`, `SelectNext`, `SelectPrevious`, and `SelectAll` learns the container scope), `src/ui/commands.rs`, `src/ui/chrome.rs`, `src/ui/context_menu.rs` (the submenu), `src/ui/op_legend.rs` (calls `select`), `src/ui/canvas.rs` (Option starts a lasso; a few lines).

## Acceptance

- [ ] Unit test: Select Same Colour with a red heart selected picks every red shape, skips hidden and locked ones, and treats a red pen layer's pen colour as its colour.
- [ ] Unit test: Select Same with two shapes of different colours reports mixed and selects nothing.
- [ ] Unit test: Select Inverse of {a, b} in {a, b, c, d} is {c, d}; locked d is left out.
- [ ] Unit test: Cmd+A inside a container selects its children; a second Cmd+A selects everything.
- [ ] Unit test: a lasso polygon selects a shape with a node inside it and skips one whose frame overlaps but whose nodes are all outside.
- [ ] Kittest: three red and two blue shapes; select a red one, Edit > Select Same > Colour; the three red are selected and the inspector's colour swatch shows red.
- [ ] Kittest: click the legend's Pen swatch; every Pen shape is selected (behaviour unchanged, now through `select`).
- [ ] Kittest: Option-drag a lasso around two of three shapes on a diagonal; those two are selected.
- [ ] Kittest: select one shape, press Tab; the next shape in Layers is selected.
- [ ] Snapshots `50-select-same-menu.png`, `50-lasso.png` (mid-drag).
- [ ] `docs/guide.md` describes Select Same, Select Inverse, the container scope, Tab and the lasso under Editor.
