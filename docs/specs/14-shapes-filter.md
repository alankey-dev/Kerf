# 14 Shapes panel header: search and filter

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec14.rs` pass.

**Wave 2b. Depends on 01. Size S.** Requested directly by the user (screenshot: the filter icon floats past the panel's right edge, above the grid, and clicking it does nothing).

## Problem

`src/ui/shapes_panel.rs` paints `Icon::Filter` at `full.right() - 6.0`, which sits outside the 3-column grid's content box and misaligned with the tiles below. It is a bare painted icon: no response, no tooltip, no accessibility label, no behaviour. With about 45 shapes in the panel there is also no way to find one quickly.

## Goal

A real panel header that lines up with the grid and makes the Shapes list filterable.

### Header row (40px, aligned to the grid's left and right edges)

- Left: "Shapes" title (semibold 14, `TEXT_PRIMARY`).
- Right: a search field (placeholder "Search shapes", magnifier icon, clear button when non-empty) and the Filter icon button. The Filter button's right edge aligns exactly with the right edge of the third tile column. The icon is vertically centred with the title and the field.
- The Filter button is a proper widget: hover wash `ACCENT_HOVER`, a tooltip (`ui::tooltip::tip`, "Filter shapes"), `widget_info` label "Filter shapes", and an accent dot or tint when any filter is active.

### Categories

- Give every built-in shape a category in `shape_library.rs` (data, not UI): Basic, Polygons, Stars and Bursts, Arrows, Frames and Outlines, Speech and Labels, Lines. Every free shape and every generated shape gets exactly one; a unit test asserts none is missing.
- The Filter button opens a `popover` (spec 01) titled "Filter" with one toggle row per category showing its count, plus "Clear filters". Multiple categories can be on at once (union). Toggling does not close the popover.

### Filtering

- The grid shows shapes that match the search text (case-insensitive substring of the name) AND any enabled category (all categories when none are enabled).
- When nothing matches, show "No shapes match" in `TEXT_MUTED` and a "Clear" text button that resets search and filters.
- Search text and filters persist while the panel is collapsed and reopened within a session (keep them in app state, not egui temp memory that resets). They do not need to persist across launches.
- Inserting a shape is unchanged.

## Files

- **Owns.** `src/ui/shapes_panel.rs`, `src/ui/tests_spec14.rs` (new).
- **Shared edits.** `src/shape_library.rs` (category field and assignments), `src/ui/mod.rs` (state field for the filter, `mod` line, pass state into `shapes_panel::show`), `src/design.rs` only if `ShapeKind::free()` needs to expose categories.

## Acceptance

- [ ] Unit test: every built-in shape has a category; each category has at least one shape.
- [ ] Unit test: filtering by "arrow" returns only names containing "arrow"; Arrows category plus search "block" returns Block Arrow only.
- [ ] Kittest: the Filter button exists by label "Filter shapes", lies fully inside the panel, and its right edge is within 1px of the third tile column's right edge.
- [ ] Kittest: open Filter, enable Stars and Bursts, and every visible tile is a star or burst; Clear filters restores the full grid.
- [ ] Kittest: typing a nonsense search shows "No shapes match", and Clear restores the grid.
- [ ] Snapshots `14-header.png` (header aligned over the grid) and `14-filter-open.png`.
- [ ] README describes search and filter in the Shapes section.
