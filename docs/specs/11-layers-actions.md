# 11 Layers panel actions

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec11.rs` pass.

**Wave 2. Depends on 01, 02 and 04. Size S.**

Reference: `reference/attach-tooltip.png`.

## Problem

Kerf should keep the most used layer operations in a bar at the bottom of the Layers panel: Slice, Combine, Attach, Flatten and Contour. Today they are only in the top toolbar, far from the layer list where you pick the objects.

## Goal

A fixed action bar sits at the bottom of the inspector, under the Layers list, and runs the same operations as the toolbar.

## Layout

A 56px bar with a 1px top border. Five evenly spaced buttons, each a 24px icon over a 12px label:

| Button | Enabled when | Action |
|---|---|---|
| Slice | exactly 2 units | spec 04 |
| Combine | 2 or more units | opens the spec 04 Combine popover above the button, with a small caret on the icon |
| Attach or Detach | 2 or more units, or the selection is attached | spec 02. The label and icon switch to Detach for an attached selection. |
| Flatten or Unflatten | as in spec 04 | spec 04 |
| Contour | one shape with more than one contour | opens the existing Contour panel |

Disabled buttons draw at `DISABLED_TEXT` and keep their tooltips. Each tooltip has a title, one sentence and the shortcut, shown above the button (spec 01 anchored tooltips). The Attach tooltip reads "Keeps objects in place so your design cuts exactly the way it is arranged on the canvas." with Option+A.

The Layers list scrolls above the bar, so the bar is always visible.

## Files

- **Owns.** `src/ui/layers_actions.rs` (new).
- **Shared edits.** `src/ui/inspector.rs` (reserve the bar at the bottom, make the Layers list scroll).

## Acceptance

- [ ] Kittest test: select two shapes, click Attach in the bar, and they share an Attach link. The button now reads Detach.
- [ ] Kittest test: with one shape selected, Slice and Combine are disabled.
- [ ] Kittest test: with 30 shapes the Layers list scrolls and the bar stays on screen.
- [ ] Snapshot `11-layers-actions.png` with the Attach tooltip open.
