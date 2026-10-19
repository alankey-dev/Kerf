# 10 Text toolbar and Format

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec10.rs` pass.

**Wave 2. Depends on 01 and 08. Size M.**

Reference: `reference/text-toolbar.png`, `reference/format-text.png`.

## Problem

Text is edited in a floating card with a string field, two font buttons and a size in inches. Kerf should put text controls in the edit toolbar when text is selected, size text in points, and add a Format popover for spacing, wrapping and alignment.

## Goal

Selecting text adds a text group to the left of the edit toolbar. The floating card goes away.

## Text group

Shown when every selected shape is text. In order, separated by the toolbar's dividers:

| Control | Behaviour |
|---|---|
| Font | A dropdown listing each `FontChoice`, each name drawn in its own font. |
| Size | A number field in points (`size_in * 72`), 1 to 1440. Typing commits on Enter or blur. |
| Style | A dropdown of the styles the font supports (spec 08), with an "A" glyph icon. |
| Alignment | One icon button that cycles Left, Centre, Right, showing the current alignment's icon. |
| Format | Opens the Format popover. |

With several texts selected, a control shows the shared value or is blank when they differ, and a change applies to all of them.

## Format popover

Title "Format Text", then:

| Row | Control |
|---|---|
| Letter Space | slider -200 to 1000 plus a number field with the `LetterSpace` icon |
| Line Space | slider -500 to 2000 plus a number field with the `LineSpace` icon |
| Text wrapping | two toggle buttons, No wrap and Wrap in box |
| Text Box Align | three toggle buttons, Top, Middle, Bottom. Enabled only with Wrap in box. |
| Select Ungrouping | three buttons, Letters, Lines, Layers. Disabled with a "Coming soon" tooltip until spec 12. |

Turning Wrap in box on sets `wrap_in` to the current box width. While wrapping, dragging a side handle changes `wrap_in` and reflows instead of stretching the glyphs.

## Toolbar overflow

With the text group added, the toolbar can be wider than the window. Items that do not fit move into a "N More" button at the right, as in `docs/evidence-ui.md` section 5. The More menu lists the hidden items, and each opens its usual popover.

## Content editing

Remove the floating card from `src/ui/mod.rs`. Until spec 12 adds editing on the canvas, the inspector shows a multi-line "Text" field above Properties when one text shape is selected.

## Files

- **Owns.** `src/ui/toolbar/text.rs` (new), `src/ui/toolbar/format.rs` (new).
- **Shared edits.** `src/ui/toolbar/mod.rs` (text group and overflow), `src/ui/mod.rs` (remove the floating editor), `src/ui/inspector.rs` (the Text field), `src/ui/canvas.rs` (side-handle reflow for wrapped text).

## Acceptance

- [ ] Kittest test: place text, set Size to 36, and `size_in` is 0.5.
- [ ] Kittest test: pick Bold, and the outlines change.
- [ ] Kittest test: set Letter Space to 200 in Format, and the box gets wider.
- [ ] Kittest test: turn on Wrap in box, drag the right handle left, and the text gains a line.
- [ ] Kittest test: at a 900px window width the toolbar shows a More button that lists Contour.
- [ ] The floating text card no longer exists.
- [ ] Snapshots `10-text-toolbar.png` and `10-format.png`.
