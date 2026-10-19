# 56 Swatches, recent colours and the eyedropper

**Wave 13. Depends on 22, 23, 44. Size M.** Affinity and Illustrator keep a swatch panel per document, show recent colours, and have an eyedropper that lifts a colour and style from one object onto another. Kerf's colour swatch opens egui's picker with a hex read-out and nothing is remembered.

## Problem

- Matching a new shape to a colour used earlier means reading a hex from one swatch and typing it in another, or in the Pen case scrolling the pen list.
- The hex is shown but not typed; there is no field to paste `#D0202E` into.
- Materials are bought in colours. A project for one vinyl set needs its palette within reach, and the mats compose by colour, so a near-miss colour makes an extra mat.

## Goal

### The picker

- `src/ui/colour_picker.rs` replaces `egui::color_picker` everywhere a colour is chosen (inspector swatch, Layers row swatches, the pattern panel's colourways, instance overrides). It shows, top to bottom: a hex field that accepts `#RGB`, `#RRGGBB`, `RRGGBB` and CSS names on Enter; the hue, saturation and value area; **Project colours**, every distinct colour in the design as swatches with a count on hover ("Red, 7 shapes"), so picking one makes no new mat; **Recent**, the last 12 colours chosen in any project, kept in `settings.json`; and **Saved**, the user's own swatches.
- Saved swatches have a + to add the current colour, drag to reorder, right-click to Rename or Delete, and live in `settings.json` as `swatches: Vec<Swatch { name, colour }>`. A project can also carry its own in `Design::swatches` for a palette that belongs to one job; the picker shows them under the project name when present, and **Save palette to project** and **Copy palette to settings** move them. Six bundled palettes (Vinyl, Iron-On, Cardstock basics, Pastel, Earth, Mono) sit in a dropdown to add from, as colour lists in `src/palettes.rs` with no artwork.
- Clicking a swatch applies it to every selected shape in one undo step, as the picker does today. On a Pen layer the picker shows pens as today with Recent and Saved filtered to pens.

### Eyedropper

- **Eyedropper** on the tool rail (key I). Click any shape to lift its colour, operation, pen, pattern and style settings into the eyedropper; Shift-click to apply what was lifted to the clicked shape; with a selection, clicking a shape applies that shape's style to the selection in one undo step, which is the Illustrator default. The lifted colour is also set as the picker's current colour. Escape or another tool ends it.
- While the eyedropper is active the pointer shows the dropper and a small swatch of the colour under it, read from the shape under the pointer, or from the canvas pixels (the decoded image) when hovering a Bitmap or a pattern, so a colour can be lifted from a photo to cut a matching layer.
- Option toggles between lifting style and lifting colour only, shown in the tooltip.

### Replace colour

- **Replace colour...** (Edit menu, palette) lists the project colours with counts and lets one be replaced by another across every shape that has it, in one undo step. This is what Select Same Colour then a swatch click does (spec 50), packaged for the common case of "make all the dark blue navy".

## Files

- **Owns.** `src/palettes.rs` (new: bundled palettes, hex parsing and CSS names, project colour counts; pure), `src/ui/colour_picker.rs` (new), `src/ui/eyedropper.rs` (new), `src/ui/replace_colour.rs` (new), `src/ui/tests_spec56.rs` (new).
- **Shared edits.** `src/settings.rs` (`recent_colours`, `swatches`), `src/design.rs` (`swatches`), `src/ui/inspector.rs`, `src/ui/layers_panel.rs`, `src/ui/pattern_panel.rs`, `src/ui/instance_overrides.rs` (call the new picker), `src/ui/canvas.rs` (`Tool::Eyedropper`), `src/ui/icons.rs`, `src/edit.rs` (`Edit::ApplyStyle`, `Edit::ReplaceColour`), `src/ui/commands.rs`, `src/ui/chrome.rs`.

## Acceptance

- [ ] Unit test: `#d0202e`, `D0202E`, `#f00` and `rebeccapurple` parse; `#12345` and `blurple` fail with a message.
- [ ] Unit test: project colours of a design with 7 red, 3 blue and 1 red pen list red 7 (fill) and the pen separately, in count order.
- [ ] Unit test: recent colours cap at 12, newest first, with no duplicates; saved swatches round-trip through `settings.json`.
- [ ] Unit test: Replace colour maps every shape with the old fill, leaves pens and patterns alone, and is one edit.
- [ ] Unit test: the eyedropper's lifted style applied to a Basic Cut shape sets colour, operation and pen; with Option it sets colour only.
- [ ] Kittest: open the picker, type `#7048E8`, Enter; the selected shape is violet and Recent shows it first.
- [ ] Kittest: click a Project colour; the shape takes it and Make's mat count does not grow.
- [ ] Kittest: select a blue shape, Eyedropper, click a red Score shape; the blue shape becomes red Score, one Undo restores it.
- [ ] Kittest: Replace colour red with navy; every red shape is navy.
- [ ] Snapshots `56-picker.png`, `56-eyedropper.png` (the pointer swatch over a photo), `56-replace-colour.png`.
- [ ] `docs/guide.md` describes the picker's sections, saved palettes, the eyedropper and Replace colour, and the Shapes panel paragraph's colour picker sentence points to it.
