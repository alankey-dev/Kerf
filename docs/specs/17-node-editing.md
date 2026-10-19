# 17 Node editing: add, remove, convert and smooth

**Wave 3. Depends on 01, 06. Size L.** Requested directly by the user: "pen should be able to add in control points or remove control points, switch between straight, sym, curve, etc and smooth options".

## Problem

Node editing (`src/path_edit.rs`, `src/ui/node_edit.rs`) can drag anchors and handles and delete one anchor with Delete. It cannot:
- add a node on an existing segment;
- remove a node any way other than click-then-Delete, or remove several at once;
- say what kind a node is. "Smooth" is only inferred when the two handles happen to be collinear (`smooth()` within 3 degrees), so there is no way to make a node straight, symmetric or a cusp on purpose;
- smooth a node or a whole path (for example a jittery Pencil stroke).

The Pen tool while drawing also has only click (corner) and click-drag (symmetric).

## Goal

Illustrator-style node editing, on both drawn paths and any shape converted to a path.

### Node kinds (data)

An explicit per-node kind, stored with the path so it survives save, undo and reload:

| Kind | Handles | Dragging one handle |
|---|---|---|
| **Straight** (corner, no handles) | none; both adjacent segments are lines at this node | n/a |
| **Cusp** (curve, broken) | independent | moves only that handle |
| **Smooth** | collinear, lengths independent | the other handle rotates to stay collinear, keeps its length |
| **Symmetric** | collinear, equal length | the other handle mirrors exactly |

- Store the kinds alongside the contours (for example `ShapeKind::Path { contours, view_box, #[serde(default)] node_kinds }`; architect chooses). When kinds are missing (old projects, freshly imported or flattened shapes) infer them from geometry: no handles is Straight, collinear equal is Symmetric, collinear unequal is Smooth, else Cusp. Add a test that loads a project saved before the change.
- `move_part` enforces the kind instead of inferring smoothness from the angle.

### Node selection

- Click a node to select it; Shift-click to add or remove; drag a marquee over empty canvas (in node edit mode) to select the nodes inside; Cmd+A selects every node of the path.
- Selected nodes draw filled with `ACCENT_PRIMARY`; unselected nodes white with an accent stroke (spec 06 handle style). Handles show for selected nodes and their neighbours.
- Dragging any selected anchor moves all selected anchors together. Arrow keys nudge the selected nodes (Shift for larger steps, same distances as shape nudge).

### Adding and removing nodes

- **Add.** Hovering a segment in node edit mode shows a pen-plus cursor and a ghost node at the nearest point; clicking inserts a node there by splitting the segment with de Casteljau, so the shape does not change. The new node is Smooth on a curve and Straight on a line.
- **Remove.** Delete or Backspace removes the selected nodes. Alt-click (Option-click) on a node removes it directly. Removing re-fits the joined segment so the outline stays as close as possible (fit one cubic through the removed span's samples, least squares) rather than just joining control points. Respect `min_nodes`; if a removal would go below it, disable the action and say why in its tooltip.

### Node toolbar

While node editing, the edit toolbar (spec 10's `Tool` enum, `src/ui/toolbar/mod.rs`) is replaced by a node toolbar in the same style (spec 01 `popover`, icons, tooltips, accessibility labels):

- **Straight**, **Cusp**, **Smooth**, **Symmetric**: convert every selected node. The current kind of the selection is highlighted (`ACCENT_HOVER`), mixed shows none. Converting to Straight drops handles and makes adjacent segments lines; converting from Straight to a curved kind creates handles at one third of each neighbouring segment along the bisector tangent (Catmull-Rom style), so the result is immediately smooth.
- **Add node** (inserts at the midpoint of each segment between two selected adjacent nodes) and **Delete node**.
- **Open path at node** and **Close path** (close joins the two end nodes with a line; open splits a closed contour at the selected node).
- **Smooth**: a popover with a strength slider (0 to 100) and Apply. With nodes selected, smooths just those (recomputes their handles from neighbours, kinds become Smooth); with none selected, smooths the whole path: simplify with Ramer-Douglas-Peucker at a tolerance scaled by strength, then refit Catmull-Rom cubics, as the Pencil does. Live preview while dragging the slider; one undo step on Apply.
- **Done** leaves node editing (as Escape does).

Each toolbar action is one undo step, applied before `History::observe`.

### Pen tool while drawing

- Click: Straight node. Click-drag: Symmetric node. Holding Option (Alt) while dragging a handle breaks it into a Cusp (only the outgoing handle follows).
- Clicking on a segment of the path being drawn does not add a node mid-path (that would be surprising); node insertion is a node-edit feature.
- The kinds the Pen creates are stored, so editing later shows the right kind.

### Keyboard and help

Shortcuts (shown in tooltips and the node toolbar): A for the direct-select of nodes if not already used, Delete removes, Option-click removes, Cmd+A selects all nodes, Escape or Enter leaves node edit. Do not clash with existing shortcuts (check `chrome.rs` and spec 13's context menu); a test lists node-edit shortcuts and asserts no duplicates.

A right-click on a node (spec 13 context menu) offers the same convert, delete and open/close items.

## Files

- **Owns.** `src/path_edit.rs` (node kinds, insert, remove with refit, convert, smooth, open/close; pure and unit tested), `src/ui/node_edit.rs` (selection, marquee, hover-add, Option-click), `src/ui/toolbar/node.rs` (new node toolbar), `src/ui/tests_spec17.rs` (new).
- **Shared edits.** `src/design.rs` (the node-kinds field on `ShapeKind::Path`), `src/ui/toolbar/mod.rs` (show the node toolbar while node editing), `src/ui/canvas.rs` (Pen tool Option-drag and recording kinds; keep small), `src/ui/context_menu.rs` (node items), `src/ui/mod.rs` (`mod` lines).

## Acceptance

- [ ] Unit test: inserting a node on a cubic at t leaves the flattened outline unchanged within 0.001 in.
- [ ] Unit test: removing a node from a sampled circle-like path and refitting stays within 0.02 in of the original outline.
- [ ] Unit tests: for each kind, dragging one handle produces the documented behaviour for the other handle.
- [ ] Unit test: Straight to Smooth creates collinear handles; Smooth to Straight removes handles and the adjacent segments become lines.
- [ ] Unit test: Smooth at strength 100 on a 200-point Pencil stroke reduces the node count and stays within its tolerance of the input.
- [ ] Unit test: an old project with no node kinds loads and infers kinds from geometry.
- [ ] Kittest: double-click a drawn path, click a segment, and the node count goes up by one with the outline unchanged; Undo removes it in one step.
- [ ] Kittest: marquee-select two nodes, click Symmetric, and both nodes' handles become equal and opposite.
- [ ] Kittest: Option-click a node removes it.
- [ ] Kittest: Pen tool, click, Option-drag, click, Enter; the middle node is a Cusp.
- [ ] Snapshots `17-node-toolbar.png` (node toolbar with a mixed selection), `17-add-ghost.png` (hover ghost node on a segment), `17-smooth.png` (before and after a Pencil stroke smooth).
- [ ] README describes node kinds, adding and removing nodes, the node toolbar and Smooth.
