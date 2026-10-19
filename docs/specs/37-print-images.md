# 37 Images print, thumbnail and flatten as pictures

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec37.rs` pass.

**Wave 7. Depends on 25, 27, 28. Size S.** Follow-up from spec 28.

## Problem

A Print Then Cut image (spec 28 `ShapeKind::Bitmap`) cuts its clip outline correctly, but:
- the print sheet PDF (spec 27, `src/print_sheet.rs` through spec 25's `src/export/pdf.rs`) paints nothing for `Paint::Image`, so the printed page is blank where the picture should be;
- project thumbnails on Home draw a Bitmap as its outline in the fill colour, not the picture;
- Flatten draws a Bitmap part the same way.

## Goal

- **Print sheet.** Embed each Bitmap's pixels in the PDF, clipped to its clip outline, at the placed position, size, rotation and mirror, honouring bleed (spec 27: the dilated outline filled with edge-extended pixels, then the art). Use `pdf-writer` image XObjects (Flate-compressed RGB plus an SMask for alpha); keep native resolution, capped at 300 dpi at placed size. The export writers stay pure: pass resolved pixel buffers in (an `ImageSource` trait or a prepared map from `AssetId` to decoded RGBA) rather than giving the writer asset access.
- **Thumbnails.** Home thumbnails render the picture clipped to its outline (tiny-skia).
- **Flatten.** A flattened part that is a Bitmap paints its picture on canvas and in thumbnails.
- **Export.** SVG export may embed the image as a data URI only when the user picks Filled style; DXF stays outline-only; update the export dialog note accordingly.

## Files

- **Owns.** `src/print_sheet.rs`, `src/export/pdf.rs` (image XObject), `src/ui/tests_spec37.rs`.
- **Shared edits.** `src/project.rs` (thumbnail render), `src/flatten.rs`/`src/ui/op_style.rs` (flattened image parts), `src/export/svg.rs` and the dialog note, `src/assets.rs` (a decode-for-export helper).

## Acceptance

- [ ] Unit test: a print sheet PDF for one Bitmap contains one image XObject with an SMask, placed so its bounds match the item's placement within 0.01 in, and a 180-degree-rotated or mirrored placement transforms it accordingly.
- [ ] Unit test: bleed on produces an image draw preceded by the dilated clip fill; bleed off does not.
- [ ] Unit test: a thumbnail of a project with a red square image has red pixels inside the clip and none outside.
- [ ] Unit test: SVG Filled export of a Bitmap contains one `<image href="data:image/png;base64,...">` clipped by the outline; Outlines style contains none.
- [ ] Kittest: Upload a fixture PNG as Print Then Cut, Make, Print step, Save print sheet to an injected folder; the PDF has an image XObject.
- [ ] Snapshot `37-thumbnail.png`.
- [ ] README Print Then Cut and Export sections updated.
