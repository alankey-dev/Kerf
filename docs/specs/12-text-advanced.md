# 12 Text: ungroup, curve and editing in place

**Wave 3. Depends on 02, 05, 08 and 10. Size L.**

Reference: `reference/format-text.png` (Select Ungrouping), `reference/selection-text.png`.

## Problem

Three text features remain after spec 10. Text cannot be split into letters or lines, it cannot follow a curve, and its string is edited in the inspector rather than on the canvas.

## Ungroup

The Format popover's Select Ungrouping buttons become live:

| Button | Result |
|---|---|
| Letters | One text shape per visible glyph, each holding its single character and the source's font, style and size, placed exactly where the glyph was. |
| Lines | One text shape per line, each placed where the line was. |
| Layers | Disabled, tooltip "This font has one layer." Multi-layer fonts are not bundled. |

The new shapes share one Group link (spec 02) and replace the source in its z position. The result looks identical to the source, which a snapshot comparison checks. Use the glyph and line indices from `TextLayout` (spec 08) for placement.

## Curve

A text-only Curve entry in the Effects menu (spec 05), shown only when every selected shape is text.

```rust
ShapeKind::Text {
    // existing fields
    #[serde(default)] curve_in: f32,   // signed circle diameter in inches; 0 is straight, positive puts text on top
}
```

The panel has a Diameter slider and number field, shown in the current units, from the smallest diameter that fits the text up to 10 times the text width, plus a Straighten button. Curve applies only to single-line text and is disabled otherwise.

In `text::layout`, a non-zero `curve_in` places each glyph along a circle of that diameter, spaced by its advance along the arc. The glyph's baseline centre sits on the arc and the glyph rotates to the tangent. Positive curve puts the text on top of the circle, negative underneath. The box refits to the curved bounds.

## Editing in place

- Double-clicking a text shape with the Select tool, or clicking one with the Text tool, enters edit mode for that shape.
- In edit mode, a multi-line `TextEdit` sits over the text box, sized to the box at the current zoom, on a white 90 % background, using the UI font closest to the text's font at the zoomed size. The canvas outlines update live as you type.
- Escape or a click outside commits and leaves edit mode. The whole session is one undo step.
- Remove the inspector Text field added in spec 10.

## Files

- **Owns.** `src/text_ops.rs` (new: ungroup and curve placement helpers), `src/ui/text_edit.rs` (new).
- **Shared edits.** `src/text.rs` (curve in `layout`), `src/design.rs` (`curve_in` field, `ungroup_text`), `src/ui/toolbar/format.rs` and `src/ui/toolbar/effects.rs` (enable the buttons), `src/ui/canvas.rs` (double-click entry), `src/ui/inspector.rs` (remove the Text field).

## Acceptance

- [ ] Unit test: ungrouping "Hi" to letters gives two text shapes in one Group link, at the glyphs' original positions within 0.001 in.
- [ ] Unit test: ungrouping "a\nb" to lines gives two shapes.
- [ ] Unit test: `curve_in` 0 gives the same layout as before this spec, and a positive curve places the middle glyph highest.
- [ ] Kittest test: double-click a text shape, type "!", press Escape, and the content ends with "!". One undo restores it.
- [ ] Snapshots `12-ungrouped.png` and `12-curve.png`.
