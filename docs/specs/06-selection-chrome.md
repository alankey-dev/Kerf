# 06 Selection chrome

**Wave 1. Depends on 00, 01 and 02. Size M.**

Reference: `reference/selection-shape.png`, `reference/selection-text.png`.

## Problem

A selection shows small square handles and nothing else. Kerf should add three things that make direct manipulation easier: larger, shaped handles, a size badge, and a floating action bar under the selection. A multi-selection or a group also cannot be resized or rotated as one.

## Goal

The selection looks and behaves like the reference, in Kerf colours, for one shape, a multi-selection and a link.

## Frame and handles

- One frame around the selection. For one unrotated shape it is the shape's box. For one rotated shape it is the rotated box, as today. For several shapes or a link it is the axis-aligned union of their world bounds.
- Corner handles are circles, 10px across, `BG_PANEL` fill, 2px `ACCENT_PRIMARY` stroke.
- Edge handles are pills: 20 x 8px on the top and bottom edges, 8 x 20px on the left and right.
- A faint cross marks the frame centre, 1px `TEXT_MUTED` at 40 % opacity, 24px across.
- The rotate handle stays as it is.
- Hit targets stay at least 18px across, larger than the drawn handle.

## Size badge

A pill filled with `ACCENT_PRIMARY` showing the frame size, `Units::size(w, h)` (spec 00), in white semibold 13. It sits 8px below the frame, centred. It updates live while resizing.

## Action bar

A floating bar of 32px icon buttons in a white rounded card (12px radius, `SHADOW`), 8px below the size badge and centred on the frame:

| Button | Shown when | Action | Tooltip shortcut |
|---|---|---|---|
| Delete | always | remove the selection | Delete |
| Duplicate | always | `Design::duplicate` | Cmd+D |
| Attach or Detach | 2 or more units, or the selection is attached | spec 02 | Option+A |
| Group or Ungroup | 2 or more units, or the selection is grouped | spec 02 | Cmd+G |
| More | always | opens a menu: Move to Front, Move to Back, Flip Horizontal, Flip Vertical, Hide | none |

Placement rules:

- If the badge and bar would fall below the visible canvas, put them above the frame instead, bar on top.
- Clamp the bar horizontally inside the canvas with an 8px margin.
- Hide the badge and bar while a move, resize or rotate drag is in progress and show them again on release.
- Show them only with the Select tool active.

## Multi-selection transform

Add `transform::resize_many` and `transform::rotate_many` in `src/transform.rs`:

- Resizing scales every member's position and size about the anchor opposite the dragged handle. Shift keeps the frame's aspect ratio, as for one shape.
- Rotating turns every member about the frame centre and adds the angle to each member's `rotation_deg`. Shift snaps to 15 degrees.
- A rotated member inside a non-uniform resize keeps its rotation and scales its own box. Exact skew preservation is out of scope.

## Files

- **Owns.** `src/ui/selection.rs` (new: frame, handles, badge, action bar painting and hit-testing), `src/transform.rs`.
- **Shared edits.** `src/ui/canvas.rs` (call into `selection.rs` instead of `paint_selection`, route grabs on the multi-frame to the new transforms).

## Acceptance

- [ ] Unit tests: `resize_many` on two shapes keeps their relative layout scaled, and `rotate_many` by 90 degrees twice equals 180 degrees.
- [ ] Kittest test: select two shapes, drag the bottom-right handle, and both grow. The size badge text matches the new union size.
- [ ] Kittest test: the action bar's Duplicate adds shapes, and More then Flip Horizontal flips the selection.
- [ ] Kittest test: during a drag the action bar is not in the accessibility tree, and after release it is.
- [ ] A shape near the bottom of the canvas shows its badge and bar above the frame.
- [ ] Snapshots `06-single.png`, `06-multi.png` and `06-text.png`.

Node editing (`src/ui/node_edit.rs`) replaces the selection chrome while a path's nodes are being edited, through the `view.nodes` match in `canvas::show`. Keep that branch when changing `paint_selection`.
