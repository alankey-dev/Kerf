# 13 Right-click context menus and Layers selection

**Wave 2b. Depends on 01, 02, 03, 04. Size M.** Requested directly by the user.

## Problem

Kerf has no right-click menus anywhere, so every operation means a trip to the toolbar or the menu bar. The Layers panel also does not behave as a selection surface: clicking rows should select exactly as clicking on the canvas does, including multi-select.

## Goal

1. **Layers selection.** First reproduce in the running app whether clicking a Layers row selects its shape; if it does not, find and fix the root cause. Then:
   - Click a row selects that object (a linked parent row selects the whole link; a member row selects that member, as spec 02 set up).
   - Shift-click selects the contiguous range of rows between the last clicked row and this one.
   - Cmd-click toggles the row in or out of the selection.
   - The selected rows are highlighted with `ACCENT_HOVER` and the canvas selection frame updates in the same frame.
   - Clicking a row's eye or colour swatch must not change the selection.
   - Selecting a shape on the canvas scrolls its row into view if the list scrolls.
2. **Context menus.** Right-click (secondary click, and Ctrl-click on macOS) opens a menu in the Kerf menu style (`popover::menu_frame`, `text_row`), with shortcuts right-aligned in `TEXT_MUTED`.
   - **On a shape on the canvas.** If the shape is not in the selection, select it first (whole link, as a left click would). Items, each disabled when it cannot apply:
     Duplicate (Cmd+D), Delete (Delete), separator,
     Group (Cmd+G) or Ungroup (Cmd+Shift+G), Attach (Option+A) or Detach (Option+Shift+A), separator,
     Combine submenu (Slice, Weld, Unite, Subtract, Intersect, Exclude), Flatten or Unflatten, separator,
     Arrange submenu (Bring to Front, Bring Forward, Send Backward, Send to Back),
     Flip submenu (Flip Horizontal, Flip Vertical),
     Align submenu (the spec 03 choices, with the same enable rules), separator,
     Hide, Select All (Cmd+A).
     Include Copy/Cut/Paste only if Kerf already has a clipboard; do not invent one in this spec.
   - **On empty canvas or the void.** Select All, Deselect, and Paste if a clipboard exists.
   - **On a Layers row.** The same menu as a shape on the canvas, acting on that row's object (selecting it first if needed), plus Rename only if layers already have editable names.
   - Every item runs through the existing command paths (`MenuCommand` / `Design` methods / toolbar `Pick::apply`), never duplicate logic. Each action is one undo step (apply before `History::observe`, as spec 02 established).
   - Escape or a click elsewhere closes the menu. Picking an item closes it.

## Files

- **Owns.** `src/ui/context_menu.rs` (new): the menu contents, built from a table of items and their enable rules, returning a typed action. `src/ui/tests_spec13.rs` (new): kittests.
- **Shared edits.** `src/ui/canvas.rs` (one hook on the canvas response for secondary click), `src/ui/layers_panel.rs` (row click modifiers, range anchor, context hook), `src/ui/mod.rs` (dispatch the returned action, `mod` lines). Keep each edit small and separate. Spec 06 is rewriting selection chrome in canvas.rs and spec 11 is changing inspector.rs, so expect a rebase.

## Acceptance

- [ ] Kittest: clicking a Layers row selects that shape; Shift-click on a third row selects the range; Cmd-click removes one.
- [ ] Kittest: clicking a row's eye toggles visibility without changing the selection.
- [ ] Kittest: right-click on an unselected shape selects it and opens the menu; choosing Duplicate adds one shape, and one Undo removes it.
- [ ] Kittest: right-click with two shapes selected, Group, and both share a Group link.
- [ ] Kittest: right-click on a Layers row and choose Delete removes that object.
- [ ] Kittest: right-click on empty canvas shows Select All, and choosing it selects every shape.
- [ ] Snapshots `13-context-shape.png` (menu open over a shape, with the Arrange submenu open) and `13-layers-multi.png` (three rows selected).
- [ ] README describes context menus and Layers selection.
