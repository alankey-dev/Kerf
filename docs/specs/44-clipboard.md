# 44 Clipboard: Cut, Copy, Paste

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec44.rs` pass.

**Wave 10. Depends on 22, 23, 33. Size M.** Every vector editor has Cmd+C and Cmd+V. Kerf has Duplicate only, so moving a shape between tabs, or bringing a path in from Affinity or Illustrator, means export and Upload. The guide says it plainly: "Kerf has no clipboard yet, so the menus have no Cut, Copy or Paste."

## Problem

- Nothing can be copied out of a tab or into one. Two open projects cannot share a shape.
- Paths drawn in another editor reach Kerf only through a saved SVG file and the Upload dialog.
- The context menu, Edit menu and command palette have a gap where every user expects three rows.
- Cmd+C already copies the Measure tool's label, so the key has one owner today and needs a rule.

## Goal

### The payload

`src/clipboard.rs` (new, pure) turns a selection into a serialisable `Package` and a `Package` back into shapes.

- `Package { shapes: Vec<Shape>, components: Vec<Component>, link_names, origin_in: Pt }` closes the selection over everything it needs: selected shapes, children of selected containers and live groups, component masters referenced by selected instances, and names for links wholly inside the package. It includes hidden children but nothing outside the selection.
- `Package::place` adds shapes with fresh ids, remaps `parent`, links and component ids, and reuses equal components when the same instance is pasted more than once. `Placement::InPlace` keeps stored positions; `Placement::Offset` moves the copy 0.25 in right and down, as Duplicate does; `Placement::At(Pt)` centres the package on a point. Pasted roots go to the top of the z-order and become selected.
- The current egui clipboard path carries one `text/plain` value, an SVG in Filled style. A `<metadata id="kerf-clipboard">` element holds the versioned JSON package, so Kerf pastes the native shapes and other editors see editable SVG paths. A plain SVG has no Kerf metadata and imports through `shape_library::import_svg`. The adapter reads clipboard text on every paste, so two Kerf processes interoperate without keeping clipboard data in app state.
- Paste of plain text that is not SVG makes a Text shape in the default font at the view centre. Paste of an image (PNG bytes, where the platform gives them) opens the trace panel as Upload does.
- The current eframe 0.31 desktop clipboard path exposes text input only. Image clipboard paste is deferred until the platform adapter exposes image bytes; image files still use Upload.

### Commands

| Command | Key | Enable rule |
|---|---|---|
| Cut | Cmd+X | selection not empty; a locked layer is skipped with the reason "Locked layers stay" |
| Copy | Cmd+C | selection not empty |
| Paste | Cmd+V | available on the canvas; reads SVG, text or a Kerf package from the clipboard |
| Paste in Place | Cmd+Shift+V | available on the canvas; preserves native package positions |
| Paste Style | Cmd+Option+V | selection is not empty; applies the first packaged shape's operation, colour, pen and pattern |

- Cut, Copy, Paste, Paste in Place and Paste Style share the command table, labels, shortcuts, enablement and command surfaces. Clipboard reads and writes stay in the UI adapter. Copy does not change the design; Cut writes the clipboard then runs `Edit::Delete`; Paste and Paste Style run as design edits through `edit::run`.
- Cmd+C while the Measure tool shows a label copies the label, as today; otherwise it copies the selection. While a text is being edited, or node editing has the keyboard, the keys belong to the text editor or do nothing, as the other shortcuts already do.
- Paste into a different tab pastes in place when the clip came from a project of the same mat size, else centres on the view. The clip is read from the system clipboard every time, so nothing is held in app state and two Kerf processes interoperate.
- Paste into an open container drops the shapes into it when the pointer, or the view centre, lies inside the container's padded box, following the drag rule in `container_canvas.rs`.
- Pasting into a component master is allowed unless the clip contains an instance of that component or one that contains it; refuse with "A component can't contain itself."

### Sanitise

Every paste validates and sanitises a temporary incoming design, so malformed metadata cannot mutate the destination and non-finite or oversized shapes cannot reach the canvas. Dropped or clamped shapes are reported in the notice bar.

## Files

- **Owns.** `src/clipboard.rs` (`Package`, closure, placement, validation and id remapping; pure), `src/ui/clipboard.rs` (egui clipboard events, metadata wrapper and text parsing), `src/ui/tests_spec44.rs` (new).
- **Shared edits.** `src/edit.rs` (`Paste` and `PasteStyle` design edits), `src/ui/commands.rs` (clipboard action rows and keys), `src/ui/chrome.rs` (Edit menu rows), `src/ui/context_menu.rs` (three rows after Duplicate), `src/ui/mod.rs` (clipboard action and event dispatch).

## Acceptance

- [ ] Unit test: copying a container copies its children and a paste re-parents them to the new container id; copying one child alone pastes it with no parent.
- [ ] Unit test: copying an instance into a design without its component adds the component once, and a second paste adds no second component.
- [ ] Unit test: a Clip round-trips through the SVG wrapper byte-for-byte; an SVG without the metadata element imports as paths.
- [ ] Unit test: Paste Style on three shapes sets operation, colour, pen and pattern from the clip's first shape and leaves geometry alone.
- [ ] Unit test: a clip holding a shape 400 in wide is clamped by sanitise and reported.
- [ ] Kittest: select a heart, Cmd+C, Cmd+V; a second heart appears 0.25 in right and down, selected, and one Undo removes it.
- [ ] Kittest: Cmd+X removes the shape and Cmd+Shift+V puts it back at the same X and Y.
- [ ] Kittest: copy in one tab, Cmd+T, Cmd+V; the shape appears in the new tab.
- [ ] Kittest: with the Measure label showing, Cmd+C copies the label text, not the selection.
- [ ] Kittest: paste an SVG string set on the clipboard by the test; a path appears at true scale.
- [ ] Snapshot `44-paste.png` (the Edit menu with the five rows and a pasted copy beside its source).
- [ ] `docs/guide.md` describes the clipboard, the two representations and the Paste rules, and the sentence saying Kerf has no clipboard is removed.
