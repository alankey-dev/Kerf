# 25 Export and Design Space handoff

**Wave 5. Depends on 09, 18, 19. Size L.** Runs in parallel with 24.

## Problem

Kerf has one export. File > Export SVG and the top bar's Export button both raise `MenuCommand::Export`. `Kerf::export_svg` in `src/ui/mod.rs` writes `design::to_svg_document`, which puts the whole visible design in one SVG at 72 points per inch. It writes every shape as a bare `<path>` with only its fill or stroke colour. Several things are missing:

- The operation is lost. A Score line and a Basic Cut look the same to Design Space or LightBurn, apart from colour.
- The mat layout from `composition::compose` is lost. The user cannot export what each mat will cut.
- There is no DXF for laser software and no PDF.
- Design Space has no path from a Kerf project. A user who still owns a Cricut has no clean way to cut a Kerf design with it through Design Space's own SVG upload.

## Goal

**File > Export** opens one dialog with three formats and a scope:

- **SVG.** The whole design, or one file per mat at true scale.
- **DXF.** R12 polylines and arcs, for laser software.
- **PDF.** Vector, one page per mat.

Every format keeps the operation as a layer or group name ("Score - Heart") and a colour, so the file stays meaningful after import. **Prepare for Design Space** exports SVGs sized and named the way Design Space's upload expects, with a short guide for reassigning operations after upload.

## Data shape

New module tree `src/export/`. One intermediate shape feeds all three writers, so names, colours and geometry cannot drift between formats.

```rust
// src/export/mod.rs
pub enum Scope { Design, PerMat }
pub enum Format { Svg, Dxf, Pdf }
pub enum Style { Filled, Outlines }           // Filled for Design Space, Outlines (hairline strokes) for laser software

pub struct Sheet {                             // one file, or one PDF page
    pub name: String,                          // "Mat 1 - Black - Basic Cut"
    pub size_in: (f32, f32),                   // the design bounds plus 0.25 in, or the mat size
    pub layers: Vec<Layer>,
}

pub struct Layer {
    pub name: String,                          // "Score - Heart", from Operation::label and ShapeKind::label
    pub operation: Operation,
    pub colour: Color32,                       // the shape's fill, or its pen colour for Pen
    pub contours: Vec<Contour>,                // world inches, on the sheet
}

pub fn sheets(design: &Design, scope: Scope, setup: &MatSetup) -> Vec<Sheet>;
```

`sheets` with `Scope::PerMat` reads `composition::compose` and `bridge::mat_placements`, so a per-mat file matches the Prepare preview. Mirror is off unless the user ticks it in the dialog. Iron-On users want it on. With `Scope::Design` it keeps canvas positions. Hidden shapes, hidden contours and Guide shapes are left out, as `compose` leaves them out.

## Rules

1. **Dialog.** `src/ui/export_dialog.rs` is a modal built from `ui::flow_kit`. It offers Format (SVG, DXF, PDF), Scope (Whole design, One per mat; PDF is always one page per mat), Style (Filled, Outlines), Units (follows the units setting, changeable here), Mirror, and a list of the files it will write. **Export** asks for a folder for several files, or a file name for one. File > Export, the top bar's Export button and Cmd+E open it.
2. **Delete the old path.** `design::to_svg_document`, `Kerf::export_svg` and the "Export SVG" menu item go once the dialog replaces them. The `design::tests` that cover the old function move into `export::svg` tests. `design::to_svg_path_d` stays, because `bridge.rs` uses it.
3. **Layer names.** One layer per (operation, shape) in z order, named `"{operation} - {shape}"` from `Operation::label()` and `ShapeKind::label()`, for example "Score - Heart" or "Pen - Text". The members of an Attach link go in a parent group named "Attached - {cut shape}". A container's children go in a group named after the container. Names are unique within a sheet. A repeat gets " 2", " 3".
4. **SVG.**
    - `width` and `height` are in the chosen unit (`in` or `mm`), with a `viewBox` in points, so the file opens at true scale anywhere that honours units.
    - Each layer is a `<g>` with `id` (slugged name) and `inkscape:label` (the readable name) under `inkscape:groupmode="layer"`.
    - Filled style puts closed contours in a filled `<path>` with `fill-rule="evenodd"`. Open contours and Pen, Score, Deboss, Engrave and Foil layers become strokes of 0.01 in in the layer colour.
    - Outlines style writes every contour as a 0.001 in hairline stroke with `fill="none"`, coloured per layer.
    - Write `export::svg::document(&Sheet, Units, Style) -> String`.
5. **DXF.**
    - ASCII R12 (`$ACADVER AC1009`). Coordinates are in the chosen unit, Y up, origin at the sheet's bottom left.
    - Write `$INSUNITS` (1 for inches, 4 for millimetres) for readers that honour it. The dialog states the unit, because strict R12 readers ignore it.
    - Each layer is a DXF `LAYER` table entry. R12 names allow only letters, digits, `$`, `-` and `_` up to 31 characters, so "Score - Heart" becomes `Score-Heart`. The colour is the nearest AutoCAD Colour Index entry.
    - Lines become `POLYLINE` vertices. Cubic segments become arcs: `export::biarc::fit` approximates each cubic with biarcs within 0.001 in and writes them as vertex bulges (group 42). A full circle becomes one `CIRCLE`. Closed contours set the polyline closed flag (group 70 bit 1).
6. **PDF.**
    - One page per mat, sized to the mat. A whole-design export gets one page sized to the design.
    - Paths are vector, in the layer colour, filled or stroked by Style, as in SVG.
    - Each layer is an optional content group named like the SVG layer, so Acrobat and Illustrator show the operations as layers.
    - Use the `pdf-writer` crate.
    - `src/export/pdf.rs` exposes `page(&mut PdfBuilder, &Sheet, Style)`, so spec 27 can reuse it for the print sheet.
7. **Prepare for Design Space.** A second entry, **File > Prepare for Design Space**, writes one SVG per mat in Filled style with no prompts beyond the folder. The SVGs are sized so Design Space's image upload reads them at true scale. Two questions decide the sizing. Does the upload honour `width` and `height` units, and what DPI does it assume for unitless files? Write whatever the upload reads at true scale.
    - File names follow the mat: `"{project} - Mat 1 - Black - Basic Cut.svg"`.
    - Every shape on a mat goes in one file, so it uploads as one image whose layers keep their names and colours.
    - The folder also gets `README - Design Space.txt` from a template in `src/export/design_space_guide.txt`. It says how to upload each file, that every layer arrives as Basic Cut, and how to set each layer back by its name ("Score - Heart": change the operation to Score, then Attach it to its cut layer). It also says to check the size against the mat's ruler before Make.
    - Nothing here talks to Design Space or Cricut's servers. It writes files only.
8. **Units.** Every length shown in the dialog goes through `src/units.rs`. File content uses the unit chosen in the dialog.

## Files

- **Owns.** `src/export/mod.rs`, `src/export/svg.rs`, `src/export/dxf.rs`, `src/export/biarc.rs`, `src/export/pdf.rs`, `src/export/design_space_guide.txt` (all new), `src/ui/export_dialog.rs` (new), `src/ui/tests_spec25.rs` (new).
- **Shared edits.**
  - `Cargo.toml`: add `pdf-writer`.
  - `src/main.rs`: add `mod export`.
  - `src/design.rs`: delete `to_svg_document` and move its tests.
  - `src/ui/mod.rs`: delete `export_svg`, open the dialog on `MenuCommand::Export`, and handle `MenuCommand::PrepareForDesignSpace`.
  - `src/ui/chrome.rs`: rename the File item to "Export...", add "Prepare for Design Space" and add the Cmd+E shortcut.
  - `README.md`: a new "Export" section and rows in the module table.

## Acceptance

- [ ] Round-trip unit test: export a design with a square, a heart, a ring and a text object to SVG in inches. Re-import it with `shape_library::import_svg`. Every import's bounds and every contour's points match the source `world_contours` within 0.001 in.
- [ ] Round-trip unit test: the same in millimetres gives the same geometry within 0.001 in.
- [ ] Round-trip unit test: a per-mat SVG of a two-colour design gives two files. Each re-imports to the offsets that `compose` gave within 0.001 in.
- [ ] Unit test: an SVG layer for a Score heart is a `<g>` with `inkscape:label="Score - Heart"` and the heart's colour, and two Score hearts get "Score - Heart" and "Score - Heart 2".
- [ ] Unit test: an attached pen star and cut square sit in a group named "Attached - Square".
- [ ] DXF unit test: a small test-only parser (group code and value pairs into `POLYLINE`, `VERTEX`, `CIRCLE` and `LAYER` records) reads the exported file. Expanding bulges back to arcs and sampling gives points within 0.001 in of the source contours.
- [ ] DXF unit test: layer names are R12-safe (`Score-Heart`), and the header has `AC1009`.
- [ ] Unit test: `biarc::fit` on a quarter-circle cubic gives arcs whose radius is within 0.001 in of the true radius, and on a straight cubic gives a zero bulge.
- [ ] PDF unit test: a two-mat export has two pages, each with a MediaBox equal to the mat size in points, and the content streams contain the optional content group names.
- [ ] Unit test: Prepare for Design Space writes `"Untitled-1 - Mat 1 - Black - Basic Cut.svg"` and the guide text file. The SVG's width and height follow the recorded upload rule.
- [ ] Unit test: an old project with a hidden shape and a Guide shape exports neither.
- [ ] Kittest: File > Export..., choose DXF and One per mat, Export to a temp folder (the dialog takes an injected path in tests), and the folder holds one `.dxf` per mat.
- [ ] Kittest: the top bar Export button opens the same dialog.
- [ ] Snapshots `25-export-dialog.png` and `25-export-dialog-dxf.png`.
- [ ] The SVG size rule for the Design Space upload is documented, and the written files open at true scale.
- [ ] README describes the formats, layer naming, the per-mat scope and the Design Space handoff.
