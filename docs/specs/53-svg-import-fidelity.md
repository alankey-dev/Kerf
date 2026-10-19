# 53 SVG import fidelity

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec53b.rs`, `src/ui/tests_spec53.rs` pass.

**Wave 12. Depends on 22, 23, 02, 18, 19, 28, 44. Size L.** Affinity and Illustrator open an SVG with its strokes, groups, layer names, clip paths, text and images intact. `shape_library::import_svg` keeps each path's geometry and solid fill and nothing else, so a stroked line drawing arrives filled black and a layered file arrives as a flat list.

## Problem

- A stroke-only path (a laser engraving line, a pen sketch, a fold line exported from Inkscape) has no fill, so it imports as a black filled shape. The stroke colour and width are dropped.
- `<g>` elements, Inkscape layers and `id` attributes are flattened to one list of unnamed paths, so a hundred path file is a hundred rows called Imported.
- `<clipPath>`, `<mask>`, `<text>`, `<image>` and `<use>` are dropped or, for text, depend on usvg's font fallback with no fonts loaded.
- Gradients and opacity become black. The user is told nothing about what was lost.
- The same converter serves Upload and, after spec 44, Paste, so every gap is paid twice.

## Goal

### Strokes become operations

- A path with a stroke and no fill imports as an open or closed `Path` in the Pen operation with the stroke colour as its pen, nearest of the 24 pens by Lab distance, and the pen size nearest the stroke width in inches. A stroke under 0.01 in, or a `stroke="#000"` with `fill="none"` on a closed path, is a cut line by Inkscape and laser convention, so a **Treat hairlines as** choice in the import notice offers Cut (default when every stroke is a hairline), Pen or Score, applied to all of them in one step after import.
- A path with both fill and stroke imports as the filled shape only, as today, and the notice counts the dropped strokes. An **Outline strokes** option expands each stroke to a filled ring with `geometry::offset`, for artwork that relies on thick outlines.
- Dash arrays are dropped and counted. Line caps and joins pass to the outline when strokes are outlined.

### Structure survives

- Each `<g>` becomes a Kerf Group (a link) of its children, nested as the file nests, and an Inkscape layer (`inkscape:groupmode="layer"`) or a `<g>` with an `id` or `inkscape:label` names its Group through `Design::link_names`. Paths with an `id` are named by it. A group of one path collapses to the path, named by whichever name exists.
- `<use>` instances resolve through usvg as today, and when the same `<symbol>` or referenced element is used three or more times the importer makes a Kerf component (spec 31) with an instance per use, named by the symbol's id. Below three it makes plain copies.
- `<clipPath>` on a group intersects each child's outline with the clip region through `geometry` booleans, so a clipped photo frame arrives cut to shape. A clip the booleans cannot express (a clip containing a bitmap) is dropped and counted. `<mask>` is always dropped and counted.
- `<text>` is outlined by usvg using the installed system fonts from spec 15's scan (an `fontdb` the importer is handed), so an SVG with live text imports as filled paths in the right face, named by the text's content. A font that is not installed falls back to Inter and is counted. The notice offers **Keep as editable text** when the whole `<text>` is one line in one font Kerf has, which makes a Kerf Text instead.
- `<image>` with an embedded PNG, JPEG or WebP data URI becomes a `Bitmap` in the asset store at its placed size, as Upload makes one, cut to its own clip when it has one, else to its rectangle. A linked external image is dropped and counted.

### Colour, opacity and size

- A gradient fill imports as the gradient's mean colour and is counted; a `pattern` fill the same. `fill-opacity` and `opacity` under 1 are dropped and counted, so a 20 % black still cuts as black. Shapes with `fill="none"` and no stroke are dropped and counted (they were invisible).
- Size follows `width`, `height` and `viewBox` as today. A file with no `width` uses the viewBox in px at 96 per inch, and the notice names the scale it chose with a **Resize** link to the inspector.
- Everything imported arrives selected, grouped as the file is, placed at the mat's top-left as today, and the whole import is one undo step.

### The import notice

- After an import the notice bar says what came in and what was lost: "Imported 42 paths in 3 groups. Dropped: 2 gradients (mean colour used), 1 mask, 3 strokes on filled shapes." with the Treat hairlines as and Outline strokes choices where they apply. The counts come from an `ImportReport` the pure converter returns beside the shapes, so the UI invents none of it, and the report is what the tests check.

## Files

- **Owns.** `src/svg_import.rs` (new: the converter returning `(Vec<Shape>, Vec<Component>, link_names, ImportReport)`, with strokes, groups, clips, text, images and the hairline rule; pure given a `fontdb::Database` and an asset sink), `src/ui/import_notice.rs` (new), `src/ui/tests_spec53.rs` (new), `tests/fixtures/svg/` (new: a stroked line drawing, an Inkscape layered file, a clipped group, a symbol used five times, a gradient, a text file, an embedded image).
- **Shared edits.** `src/shape_library.rs` (`import_svg` becomes a call into `svg_import` and keeps serving the bundled shapes), `src/ui/upload.rs` and `src/ui/clipboard.rs` (take the report), `src/system_fonts.rs` (expose the `fontdb` for the importer), `src/assets.rs` (the sink), `src/edit.rs` (`Edit::Place` takes groups and components).

## Acceptance

- [ ] Unit test: a stroke-only red 0.4 mm path imports as a Pen path with the Red Fine pen; a 0.001 in black stroke counts as a hairline and the default choice is Cut.
- [ ] Unit test: Outline strokes on a 0.1 in stroked square gives a closed ring whose outer edge is 0.05 in outside the path.
- [ ] Unit test: an Inkscape file with two layers and a nested group imports as two named Groups, one holding a Group, with every `id` kept as a name.
- [ ] Unit test: a symbol used five times becomes one component with five instances; used twice it makes two plain paths.
- [ ] Unit test: a clipped group's children are intersected with the clip and the report counts no drop; a clip over a bitmap is counted.
- [ ] Unit test: `<text>` in an installed font outlines to a path named by its content; in a missing font it falls back and is counted; a single-line text in Inter with Keep as editable text makes a Kerf Text whose content matches.
- [ ] Unit test: an embedded PNG becomes a Bitmap at the right size with its asset stored once across two imports.
- [ ] Unit test: a gradient fill imports as its mean colour and the report says so; an invisible path is dropped and counted.
- [ ] Unit test: every fixture imports with the same shape count and names on a second run (determinism).
- [ ] Kittest: Upload the stroked fixture; the notice names the hairlines, choosing Pen switches the shapes' operation in one undo step.
- [ ] Kittest: Upload the layered fixture; Layers shows the file's layer names as Group rows.
- [ ] Snapshots `53-import-notice.png`, `53-layered-import.png`.
- [ ] `docs/guide.md` rewrites the Upload sentence under Editor into an SVG import paragraph naming what is kept, what is converted and what is dropped.
