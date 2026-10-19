# 02 Groups and Attach

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its module unit tests in `src/` pass.

**Wave 0. Depends on nothing. Size L.**

Reference: `reference/attach-tooltip.png`, `reference/selection-text.png` (the paperclip in the action bar).

## Problem

Every shape is independent. Kerf needs two ways to tie shapes together, and later specs rely on both:

- **Group** moves, resizes and selects shapes together on the canvas. At Make time each member is placed on the mat on its own.
- **Attach** does everything Group does and also keeps the members' relative positions on the mat, so they cut exactly as arranged. A pen layer attached to a cut layer draws on that cut layer's mat (spec 09).

## Data shape

One level of linking, stored on the shape, so deleting a shape can never leave a dangling member list.

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind { Group, Attach }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Link { pub id: u64, pub kind: LinkKind }

pub struct Shape {
    // existing fields
    #[serde(default)]
    pub link: Option<Link>,
}
```

Invariants, enforced only by the `Design` methods below:

1. Every shape sharing a `link.id` has the same `kind`.
2. A link with fewer than two members is removed.
3. Link ids come from the same counter as shape ids, so they never collide.

## Design operations

Add these to `Design`, with the logic in a new `src/links.rs` (`impl Design` blocks may live in any module of the crate):

| Method | Behaviour |
|---|---|
| `group(ids)` | Needs 2 or more shapes. Members of existing links are pulled out of them. All `ids` get one new `Group` link. Returns the link id. |
| `attach(ids)` | As `group`, with `Attach`. |
| `unlink(ids)` | Removes the link from every shape that shares a link with any of `ids`. Ungroup and Detach both call this. |
| `expand(ids) -> Vec<u64>` | `ids` plus every shape that shares a link with one of them, in z-order. |
| `prune_links()` | Enforces invariant 2. Called at the end of `remove_many`, `combine` and `offset`. |

Existing operations follow these rules:

- **Duplicate** gives the copies a fresh link id of the same kind.
- **Combine** removes its inputs, so their links shrink. The result has no link.
- **Arrange** moves linked shapes as a block, keeping their order relative to each other.

## Canvas behaviour

Changes in `src/ui/canvas.rs`, kept to the selection entry points:

- A click, drag start or marquee hit on a linked shape selects `design.expand(...)`.
- Cmd+click on a linked shape selects just that member, for editing one part. The next plain click selects the whole link again.
- Moving a linked selection moves every member. Resizing and rotating a multi-shape selection is spec 06.

## Commands

| Command | Shortcut | Enabled when |
|---|---|---|
| Group | Cmd+G | 2 or more shapes selected |
| Ungroup | Cmd+Shift+G | the selection contains a Group link |
| Attach | Option+A | 2 or more shapes selected |
| Detach | Option+Shift+A | the selection contains an Attach link |

Add all four to the Edit menu in `src/ui/chrome.rs` with their shortcuts shown. Spec 06 adds them to the selection action bar and spec 11 to the Layers panel.

## Layers panel

A link shows as a parent row named "Group" or "Attached", with a paperclip icon for Attach and a group icon for Group, and its members indented beneath it. Clicking the parent selects the whole link. The parent row has the same eye and delete buttons as a shape row, acting on all members. Parent rows collapse and expand with a chevron.

## Files

- **Owns.** `src/links.rs` (new), `src/ui/layers_panel.rs`.
- **Shared edits.** `src/design.rs` (the `link` field, calls to `prune_links`, duplicate and arrange rules), `src/ui/canvas.rs` (selection expansion), `src/ui/chrome.rs` (Edit menu), `src/ui/mod.rs` (shortcuts and command dispatch).

Spec 01 adds the `Attach`, `Detach`, `Group` and `Ungroup` icons. If 01 has not merged yet, use text labels in the Layers panel and switch to the icons once it lands.

## Acceptance

- [ ] Unit tests in `src/links.rs` cover each invariant, regrouping members of an existing link, duplicate, delete down to one member, and combine.
- [ ] A project JSON saved before this change loads with `link: None` on every shape.
- [ ] Kittest test: select two shapes, press Cmd+G, click one, and both are selected. Drag one and both move by the same amount.
- [ ] Kittest test: Cmd+click a grouped shape selects only that shape.
- [ ] Kittest test: Option+A then Option+Shift+A leaves no links.
- [ ] The Layers panel shows an "Attached" parent row with indented members. Snapshot `02-layers-links.png`.
- [ ] Undo restores the previous links in one step after each command.
