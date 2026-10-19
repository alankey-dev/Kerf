# 29 Single-line fonts for Pen and Foil

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec29.rs` pass.

**Wave 6. Depends on 22, 23, 07, 08, 10, 12, 15. Size L.** Writing fonts draw letters with one pen stroke. Every Kerf font is an outline font, so text drawn with a pen traces each letter twice, round both sides of the stroke.

## Problem

`src/text.rs` outlines every glyph with `ttf-parser` into closed contours. With the Pen or Foil operation (the default for text) a pen draws each letter's outline, which looks like bubble writing and takes twice as long. Users work round this with third-party "single line" fonts that fake strokes with zero-width closed paths, which then draw each stroke twice.

## Goal

Real stroke fonts: each glyph is a set of open paths, drawn once by the pen.

### Font data

- **Bundled.**
  - **Hershey** fonts (public domain, from the U.S. National Bureau of Standards data). Ship Simplex Roman, Duplex Roman, Complex Script, Script Simplex and Gothic English. Keep the Usenet distribution notice in `assets/fonts/hershey/NOTICE`.
  - **Relief SingleLine** (SIL OFL 1.1, from the isdat-type Relief-SingleLine repository): ship its SVG font build and its `OFL.txt`. Confirm the upstream licence file at import time and record its version in the NOTICE.
- **Format.** The app reads one stroke format, Kerf stroke font JSON (`*.odsstroke.json`): units per em, ascender, descender, and per glyph an advance and a list of polylines or cubic paths. A small converter in `tools/stroke_font/` (built and run by hand, not at app build time) turns Hershey `.jhf` data and SVG fonts into this format, so the app has one parser and the bundled files are reviewable diffs.
- **User fonts.** Upload also accepts an SVG font file (`<font>` with `<glyph d="...">`). Parse it with `roxmltree` and `svgtypes` (both MIT or Apache-2.0, already pulled in by `usvg`), convert in memory to the same model, and copy it into `<projects dir>/fonts/`. Paths in an SVG font are treated as strokes: a closed subpath whose two halves coincide within 0.5 % of the em (the "fake single line" trick) is collapsed to the open centreline, so popular single-line writing fonts draw once.

### Model

- `text::FontRef` gains `Stroke(StrokeFontId)`. Projects save it as a plain string, `"stroke:hershey-simplex"`, like the existing font names. A missing stroke font falls back to Hershey Simplex and the menu lists it as "Name (missing)", as spec 15 does.
- `src/stroke_font.rs` loads and caches stroke fonts and returns per-glyph open contours in em units. `text.rs` gets a glyph source split: outline glyphs from `ttf-parser`, stroke glyphs from `stroke_font.rs`. Layout (spec 08: lines, alignment, letter and line spacing, wrapping) is unchanged and shared; only the glyph geometry differs. Kerning: none for Hershey; SVG fonts use `<hkern>` when present.
- Styles: Regular only, plus the shared 12 degree italic shear. Bold is disabled for stroke fonts with a tooltip saying why; the pen size (spec 07) is the weight.

### Behaviour

- **Font menu (spec 15).** A **Writing** section between "Kerf fonts" and "System fonts", each name drawn in its own strokes, with a small pen icon. Search covers it. A filter chip "Writing only" shows just these.
- **Operation.** Choosing a writing font for a text whose operation is a cut switches it to Pen (one undo step with the font change) and shows a one-line notice "Writing fonts draw with a pen" with an Undo link. Choosing a non-writing font leaves the operation alone. Foil is allowed.
- **Output.** The text's contours are open (`closed: false`), so `to_svg_path_d` emits no `Z`, the pen draws each stroke once, and spec 19 draws them at the nib width.
- **Stroke order.** Within one text, strokes are ordered for the pen: in reading order by glyph, and each glyph's strokes kept in the font's order, with the shortest pen-up move between glyphs. This ordering is pure and tested.
- **Toolbar (spec 10).** Font, Size, Alignment and Format work. Style shows Regular and Italic only.
- **Ungroup and Curve (spec 12).** Letters and Lines ungrouping and Curve work unchanged because they place laid-out glyphs; their results keep the stroke font.
- **Combine.** Weld, Unite, Subtract, Intersect, Exclude and Slice are disabled for a selection containing single-line text, with a tooltip "Single-line text has no area to combine". Offset and Sticker are disabled the same way. Attach works, so writing on a card cuts and draws on one mat.
- **Convert to paths** (Effects menu, and spec 21's Expand) turns the text into an open `ShapeKind::Path`.

## Files

- **Owns.** `src/stroke_font.rs` (new: model, loaders, cache, fake-single-line collapse; pure), `assets/fonts/hershey/` and `assets/fonts/relief-singleline/` (new data and licences), `tools/stroke_font/` (new converter), `src/ui/tests_spec29.rs` (new).
- **Shared edits.** `src/text.rs` (`FontRef::Stroke`, glyph source split, open contours), `src/ui/toolbar/font_menu.rs` (Writing section and chip), `src/ui/toolbar/text.rs` (style options), `src/ui/toolbar/combine.rs` and `src/ui/toolbar/effects.rs` (enable rules, through spec 23's command model), `src/ui/chrome.rs` (Upload accepts SVG fonts), `src/ui/mod.rs` (`mod` lines).

## Acceptance

- [ ] Unit test: Hershey Simplex "l" is one open contour; "o" is one closed loop drawn once (no doubled segments).
- [ ] Unit test: the fake-single-line collapse turns a zero-width closed "l" from an SVG font into one open contour within 0.5 % of the em of its centreline.
- [ ] Unit test: laying out "Hello" in a stroke font produces only open or once-drawn contours, and the box matches the font's ascender to descender.
- [ ] Unit test: the stroke order for "ab" visits all of "a" before "b", and the pen-up distance is no greater than drawing in font order.
- [ ] Unit test: `to_svg_path_d` for single-line text contains no `Z`.
- [ ] Unit test: a project saved with `"stroke:hershey-simplex"` round-trips; one naming an unknown stroke font loads and renders in Hershey Simplex; an old project loads unchanged.
- [ ] Unit test: `compose` puts Pen single-line text attached to a cut card on the card's mat.
- [ ] Kittest: type text, open the Font menu, pick a Writing font; the operation becomes Pen, the notice shows, and one Undo restores both font and operation.
- [ ] Kittest: with single-line text selected, Weld is disabled and its tooltip names the reason.
- [ ] Kittest: Ungroup to Letters on single-line text gives one text per letter, each still in the stroke font.
- [ ] Snapshots `29-font-menu.png` (Writing section), `29-writing.png` (a line of Relief SingleLine with a Fine pen next to the same text in Inter).
- [ ] README describes writing fonts, the bundled families with their licences, SVG font upload and the operation switch.
