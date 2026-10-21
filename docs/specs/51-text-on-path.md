# 51 Text on a path

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec51b.rs`, `src/ui/tests_spec51.rs` pass.

**Wave 12. Depends on 12, 17, 21, 29, 47. Size L.** Illustrator's Type on a Path and Affinity's Path Text run text along any curve, keep it editable, and let you slide it, flip it to the other side and set where it starts. Kerf's Curve bends one line onto a circle and nothing else.

## Problem

- `text::curve` takes a signed circle diameter. A banner on a wave, a name along a heart's edge or a label around an arch cannot be made.
- The circle is implicit, so there is nothing to see or snap to; the user guesses the diameter.
- A curved text cannot be slid along its circle or started at a chosen angle.

## Goal

### Model

- `ShapeKind::Text` gains `path: Option<TextPath>`, `#[serde(default)]`, where `TextPath { contours: Vec<Contour>, view_box: Pt, offset_in: f32, side: Side, align: Along }`. The contours are the text's own copy of the guide path in the text's local frame, so the text stays self-contained when the source shape is deleted. `Side` is Above or Below the path, `Along` is Start, Centre or End, and `offset_in` slides the start along the path.
- `curve_in` becomes a `TextPath` made from a circle on upgrade, and the Curve panel stays as a quick way to make one. Bump `FORMAT_VERSION`.
- `text::layout` lays glyphs along the path by arc length. Each glyph's baseline centre sits at its advance midpoint along the arc, turned to the tangent, with the per-glyph spacing correction Illustrator applies on tight curves (outer glyphs spread, inner glyphs tighten, by the ratio of the glyph's baseline radius to its cap height). Text longer than the path overflows past the end on an open path and wraps round on a closed one, and the layout reports the overflow so the canvas can draw a small red plus at the end as Illustrator does.
- Only a single unwrapped line follows a path. Wrapped or multi-line text greys the controls, as Curve does today.

### Making one

- Select one text and one path shape (any kind; its evaluated outline is the path) and choose **Text on path** (Effects menu, context menu, palette, Cmd+Option+T). The text takes a copy of the path, the source shape is kept and becomes a Guide operation unless it already has a cut operation, which it keeps so a word can run round the edge of a tag that is still cut. The text is selected.
- With the Text tool, hovering a path's edge within 6 px shows a text-on-path cursor, and clicking there places a new text on that path at the click point.
- **Remove from path** straightens the text and keeps the glyphs' size and font, as Straighten does today.

### Editing

- A selected path text shows its path as a faint accent line and three handles on it. The start handle slides `offset_in`; the centre handle drags across the path to flip Side; the end handle, on an open path, clips the run so the text centres between start and end when `Along` is Centre. Each drag is one undo step with the read-out showing the offset in the current units.
- The Curve panel becomes **Path** when the text has a path. It shows Side, Align along, Offset (an expression field) and Remove from path. For a circle path it also shows Diameter, as today.
- Node editing the text's path (press A with a path text selected) edits the copy inside the text and reflows the letters live. Editing the source shape does not move the text, by design, since the copy is the text's own. A Refit to shape row was specified here and dropped (issue 32): it was never built, and re-copying the source's outline would overwrite node edits made to the text's own copy.
- Ungroup to letters places each letter at its tangent, as it does for curved text. Warp, Offset and Sticker apply to the laid-out glyphs as for any text.
- Writing fonts follow a path too, and single-line text on a path draws once with the pen.

## Files

- **Owns.** `src/text_path.rs` (new: `TextPath`, arc-length tables, the glyph placement and spacing correction, overflow; pure), `src/ui/text_path.rs` (new: handles, cursor, the Path panel), `src/ui/tests_spec51.rs` (new).
- **Shared edits.** `src/text.rs` (`layout` calls `text_path::place` where `path` is set; `curve` goes once the upgrade lands), `src/design.rs` (`path` on Text), `src/project.rs` (the format bump and the upgrade of `curve_in`), `src/edit.rs` (`Edit::TextPath(TextPathEdit)`), `src/ui/toolbar/effects.rs` or wherever Curve lives (rename and extend), `src/ui/canvas.rs` (the Text tool's path hover; a few lines), `src/text_ops.rs` (letters from a path text).

## Acceptance

- [ ] Unit test: a `curve_in` of 3.0 upgrades to a circle `TextPath` whose layout matches the old `curve` layout within 1e-3 in per glyph.
- [ ] Unit test: text along a straight 4 in line lays out identically to straight text.
- [ ] Unit test: text along a semicircle has every glyph's baseline centre on the arc within 1e-4 in and its turn equal to the tangent angle.
- [ ] Unit test: text longer than an open path reports overflow and the overflowing glyphs sit past the end on the tangent.
- [ ] Unit test: flipping Side mirrors the glyphs to the other side with the baseline still on the path.
- [ ] Unit test: Offset slides the first glyph by the given arc length, and Align End places the last glyph at the path's end.
- [ ] Kittest: select a text and a heart, Cmd+Option+T; the text runs along the heart and one Undo restores both.
- [ ] Kittest: drag the start handle; the text slides and the read-out shows the offset.
- [ ] Kittest: drag the centre handle across the path; the text flips below.
- [ ] Kittest: press A, move a node of the path; the letters reflow.
- [ ] Kittest: Text tool, click on a circle's edge, type "Hello"; a path text appears on the circle.
- [ ] Snapshots `51-text-on-heart.png`, `51-path-handles.png`, `51-overflow.png`.
- [ ] `docs/guide.md` replaces the Curve paragraph under Text with Text on a path, keeping the Diameter note for circles.
