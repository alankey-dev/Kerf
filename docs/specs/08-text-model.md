# 08 Text model

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its module unit tests in `src/` pass.

**Wave 1. Depends on 00. Size M.**

## Problem

Text is one line in one weight, laid out without kerning, and its box is the tight ink bounds, so the box jumps as letters with descenders come and go. Specs 10 and 12 need multi-line text, spacing, alignment, weights, wrapping and per-glyph geometry.

## Goal

`text.rs` lays out multi-line, styled, kerned text into contours with a stable box, and keeps the glyph-to-line mapping so text can later split into letters and lines. No new UI beyond what tests need. Spec 10 adds the controls.

## Data shape

Extend the existing struct variant. Every new field has a serde default, so saved projects still load.

```rust
ShapeKind::Text {
    content: String,                 // may contain '\n'
    font: FontChoice,
    size_in: f32,                    // em height; the UI shows points (size_in * 72)
    #[serde(default)] style: FontStyle,       // Regular | Medium | Bold | Italic | BoldItalic
    #[serde(default)] letter_space: f32,      // thousandths of an em added after each glyph, -200..=1000
    #[serde(default)] line_space: f32,        // thousandths of an em added between lines, -500..=2000
    #[serde(default)] align: TextAlign,       // Left | Centre | Right
    #[serde(default)] wrap_in: Option<f32>,   // Some(width) wraps words at that width
    #[serde(default)] v_align: VAlign,        // Top | Middle | Bottom, used when the box is taller than the text
}
```

## Layout

Replace `text_to_contours` with:

```rust
pub struct TextLayout {
    pub glyphs: Vec<GlyphOutline>,   // contours plus line index and char index, local inches
    pub size: Pt,                    // the text box
}
pub fn layout(spec: &TextSpec) -> Option<TextLayout>;
```

Rules:

- **Box.** Width is the widest line's advance width, or `wrap_in` when set. Height is the number of lines times the line height. Line height is `ascender - descender + line_gap` from the font, plus `line_space`. The box does not depend on which glyphs have ink.
- **Kerning.** Apply pair kerning from the font's `kern` table, and from GPOS pair adjustment when present (`ttf-parser` with `opentype-layout`). If neither exists, use advances alone.
- **Spacing.** `letter_space` adds `letter_space / 1000 * size_in` after every glyph except the last on each line.
- **Alignment.** Each line is offset inside the box width by its alignment.
- **Wrapping.** With `wrap_in`, break at spaces so no line exceeds the width. A single word wider than the box stays on its own line and overflows.
- **Styles.** Map styles to the bundled faces: Inter Regular, Medium and SemiBold (as Bold). Space Grotesk is a variable font, so set its `wght` axis (400, 500, 700). Italic and BoldItalic shear the outlines by 12 degrees about the baseline, because no italic faces are bundled.
- **Characters without a glyph** advance by the space width.

`ShapeKind::Text` returns `layout(...).glyphs` contours from `outline()`. `refit_text` sets the shape's size to `layout.size`, keeping the current scale factor between the shape's size and the natural size, so editing text that was resized does not snap it back.

## Files

- **Owns.** `src/text.rs`.
- **Shared edits.** `src/design.rs` (the variant's fields, `outline`, `default_size_in`, `refit_text`), the text editor in `src/ui/mod.rs` only where needed to compile.

## Acceptance

- [ ] Unit test: "AV" with kerning is narrower than the sum of the A and V advances, for Inter.
- [ ] Unit test: the box height of "ace" equals the box height of "gyp" at the same size.
- [ ] Unit test: "a\nb" has two lines and twice the single-line height plus `line_space`.
- [ ] Unit test: centred alignment of "a\nabc" puts the "a" glyph's centre at the box's centre x.
- [ ] Unit test: `wrap_in` of 1 in wraps "one two three" at 1 in size into three lines.
- [ ] Unit test: Bold Space Grotesk outlines differ from Regular.
- [ ] Unit test: a project with the old three-field text variant loads with the defaults.
- [ ] Unit test: each `GlyphOutline` reports the right line and char index for "ab\nc".
