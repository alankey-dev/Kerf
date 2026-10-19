# 03 Align grid

**Wave 1. Depends on 01. Size S.**

Reference: `reference/align.png`.

## Problem

Align is a plain text list of eight items. It should be a two-column grid of icon buttons with an extra Centre option, and a separate Distribute section.

## Goal

The Align popover matches the reference layout, adds Centre, and treats each group or attach set as one unit.

## Layout

Title "Align", then a two-column grid of `option_button`s (spec 01), in this order:

| Left column | Right column |
|---|---|
| Top | Left |
| Vertical | Horizontal |
| Bottom | Right |
| Centre | |

Then a 1px divider, the title "Distribute", and one row with Vertical and Horizontal.

Each button's tooltip says exactly what moves, because "Vertical" alone is ambiguous:

| Button | `Align` value | Tooltip |
|---|---|---|
| Top | `Top` | Line up top edges. |
| Vertical | `VCentre` | Line up middles on one horizontal line. |
| Bottom | `Bottom` | Line up bottom edges. |
| Left | `Left` | Line up left edges. |
| Horizontal | `HCentre` | Line up centres on one vertical line. |
| Right | `Right` | Line up right edges. |
| Centre | `Centre` (new) | Stack centres on one point. |
| Distribute Vertical | `DistributeV` | Space evenly from top to bottom. |
| Distribute Horizontal | `DistributeH` | Space evenly from left to right. |

## Rules

- Align needs 2 or more units. Distribute needs 3 or more units. Disabled buttons stay visible, with a tooltip naming the count needed.
- A unit is a single unlinked shape or a whole link from spec 02. `Design::align` measures a link by the union of its members' world bounds and moves every member by the same delta.
- `Align::Centre` is `HCentre` followed by `VCentre`, as one undo step.

## Files

- **Owns.** `src/ui/toolbar/align.rs`.
- **Shared edits.** `src/design.rs` (`Align::Centre`, grouping boxes by link inside `align` and `distribute`).

## Acceptance

- [ ] Unit test: Centre on three shapes leaves all three centres equal.
- [ ] Unit test: a two-member group aligned Left against a lone shape keeps the group's internal offset.
- [ ] Kittest test: with two shapes selected, Distribute buttons are disabled and Align buttons work.
- [ ] Snapshot `03-align.png` of the open popover matches the reference layout.
