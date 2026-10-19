# 01 UI kit: popovers, tiles and icons

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its module unit tests in `src/` pass.

**Wave 0b. Depends on 00. Size M.**

## Problem

Wave 1 needs the same few building blocks: an icon tile grid (Combine, Align), an icon menu (Effects), a titled popover (Format, pen colours) and many new icons. Today each popover is hand-built inside one 450-line `src/ui/edit_toolbar.rs`, so four parallel agents would all edit that file.

## Goal

Shared popover components exist, the edit toolbar is split into one file per menu, and the icons wave 1 needs are registered. No behaviour changes in this spec. Existing tests must pass unchanged.

## Components

Put these in `src/ui/popover.rs`.

- **`popover(ui, anchor: &Response, id, width, body) -> Option<R>`.** Opens below `anchor` on click, closes on click-away or when `body` returns `Some`. Uses the existing `menu_frame` look (white, 4px radius, `SHADOW`). This replaces `menu_popover` and the hand-rolled area in `effects_popover`.
- **`titled(ui, title)`.** The popover title row: semibold 14, 16px below.
- **`tile_grid(ui, columns, tiles: &[Tile]) -> Option<usize>`.** Square icon tiles with a caption under each, as in `reference/combine.png`. Tile is 72 x 72 with a 1px `BORDER_SUBTLE` outline and 12px radius, icon 24px centred, caption regular 13 `TEXT_MUTED` below. Hover fills `ACCENT_HOVER`. A disabled tile draws at `DISABLED_TEXT` and still shows its tooltip, which says why it is disabled. `Tile { icon, label, enabled, tip }`.
- **`icon_row(ui, icon, label) -> Response`.** A 40px menu row with a 24px icon and a 14px label, as in `reference/effects.png`.
- **`option_button(ui, icon, label) -> Response`.** A bordered button with an icon left of the label, used in a two-column grid, as in `reference/align.png`.

## Anchored tooltips

`ui::tooltip::tip` currently shows egui's tooltip under the pointer, where it covers neighbouring icons. Anchor it to the widget instead, as in `reference/attach-tooltip.png`: a card beside the widget with a small arrow pointing at it. The side is chosen by where the widget sits. Rail items get it on the right, toolbar items below, and bottom-bar items above. Keep the 0.5 s delay and the title, body and shortcut content. Every existing `tip` call keeps working unchanged.

## Split the edit toolbar

Move `src/ui/edit_toolbar.rs` to `src/ui/toolbar/`:

- `mod.rs` keeps `show`, `tool`, `divider` and the operation dropdown.
- `flip.rs`, `arrange.rs`, `align.rs`, `effects.rs` and `combine.rs` each hold one menu and return the picked action.

`show` keeps calling them in the same order. Wave 1 specs then own one file each.

## Icons

Add `Icon` variants and draw them in `paint_drawn` in `src/ui/icons.rs` (the asset folder has none of these). Each is a simple 1.5px line drawing in the unit square, readable at 20 and 24px:

`Slice, Weld, Flatten, Unflatten, Unite, Subtract, Intersect, Exclude, AlignTop, AlignVCentre, AlignBottom, AlignLeft, AlignHCentre, AlignRight, AlignCentre, DistributeV, DistributeH, Offset, Sticker, Warp, Curve, Attach, Detach, Group, Ungroup, Duplicate, More, Format, TextAlignLeft, TextAlignCentre, TextAlignRight, LetterSpace, LineSpace, Lock, Unlock`.

Base their shapes on the glyphs in `reference/combine.png`, `reference/effects.png`, `reference/align.png`, `reference/selection-text.png` and `reference/format-text.png`. They must be original drawings, not traced artwork.

Add a snapshot test that paints every icon in a labelled grid to `target/snapshots/01-icons.png`, so reviewers can check them in one image.

## Files

- **Owns.** `src/ui/popover.rs` (new), `src/ui/toolbar/` (new, replaces `edit_toolbar.rs`), `src/ui/icons.rs`, `src/ui/tooltip.rs`.
- **Shared edits.** `src/ui/mod.rs` (module names, the call to `toolbar::show`).

## Acceptance

- [ ] Every existing test passes with no assertion changed.
- [ ] Flip, Arrange, Align, Effects and Combine open and act exactly as before.
- [ ] `01-icons.png` shows every new icon legibly at 20 and 24px.
- [ ] Hovering the Pencil rail item shows its tooltip to the right of the rail with the arrow on the icon. Snapshot `01-tooltip.png`.
- [ ] A standalone `Harness::new_ui` test renders a `tile_grid` with one disabled tile and asserts that clicking it returns `None` and clicking an enabled tile returns its index.
