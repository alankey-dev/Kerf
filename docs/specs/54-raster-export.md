# 54 Raster export: PNG and JPEG

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec54.rs` pass.

**Wave 12. Depends on 25, 37, 44. Size S.** Every editor exports a PNG. Kerf exports SVG, DXF and PDF and renders PNG thumbnails for Home, but a maker who wants to show a design in a shop listing, a message or a forum has no picture to send.

## Problem

- The export dialog's Format has three vector choices. A quick preview image means a screenshot.
- `src/export/picture.rs` and the thumbnail path already rasterise a design with tiny-skia, at thumbnail size, with no dpi control, no transparency choice and no per-mat option.
- The clipboard (spec 44) copies vectors only; pasting a picture into a chat needs PNG bytes.

## Goal

### Formats

- Format gains **PNG** and **JPEG**. Both share the dialog's Scope (whole design, or one file per mat) and Mirror. Style is hidden for them; raster export always draws as the canvas does, operation styles included, through `op_style`, so a Score line is dashed and a Print Then Cut image shows its pixels.
- Raster options appear under Format. Resolution is 72, 150, 300 or 600 dpi, default 300, with the pixel size shown live ("3600 x 3600 px"). Background is Transparent (PNG only, default), White or Mat (the artboard with its grid and rulers off). Margin is an expression field, default 0.25 in as SVG uses. JPEG adds Quality (60 to 100, default 90) and has no transparent option.
- **Show as made** from View is a toggle here too, so the picture can show cut outlines only, which is what a listing of a cut file usually wants.
- The file list shows each file's pixel size and an estimated byte size, as it shows sizes for the vector formats.

### Rendering

- `export::raster::render(sheet, options, pixels) -> RgbaImage` draws a `Sheet` with tiny-skia at the dpi, reusing `picture.rs` and the canvas styles, so the export matches the Project Preview. Images are drawn from the full-size decode, not the 512 px thumbnail copy, so a 300 dpi export of a photo sticker is sharp. Text and patterns go through the same evaluation every other output uses.
- An export over 100 megapixels is refused in the dialog with the size it would be, as Upload refuses large images, so a 600 dpi 24 x 24 in mat (207 megapixels) says so instead of running out of memory.
- Rendering runs on the image worker with the progress bar the trace panel uses, and Cancel stops it. The dialog closes when the files are written.

### Copy as image

- **Copy as PNG** (Edit menu, context menu, palette) renders the selection at 300 dpi with a transparent background and puts the PNG on the system clipboard beside the vector clip of spec 44, where the platform clipboard allows an image, so a paste into a chat gets the picture and a paste into Kerf gets the shapes.

## Files

- **Owns.** `src/export/raster.rs` (new: options, size and memory check, `render`; pure given the pixels), `src/ui/tests_spec54.rs` (new).
- **Shared edits.** `src/export/mod.rs` (`Format::Png`, `Format::Jpeg`, `RasterOptions` on `Request`, the plan's pixel and byte sizes), `src/export/picture.rs` (take a dpi and background), `src/ui/export_dialog.rs` (the options block; Style hidden for raster), `src/ui/clipboard.rs` (the image write), `src/edit.rs` (`Edit::CopyAsImage`), `src/ui/commands.rs`, `src/ui/chrome.rs`, `src/ui/context_menu.rs`.

## Acceptance

- [ ] Unit test: a 2 x 3 in sheet at 300 dpi with a 0.25 in margin renders 750 x 1050 px; at 72 dpi 180 x 252.
- [ ] Unit test: a transparent PNG of a red square has alpha 0 in its margin and alpha 255 inside the square; the White background has no alpha 0 pixel.
- [ ] Unit test: Show as made renders a Basic Cut shape as an outline with a transparent interior.
- [ ] Unit test: a JPEG at quality 60 is smaller than one at quality 100 and decodes to the same size.
- [ ] Unit test: a 24 x 24 in sheet at 600 dpi is refused with a message naming 207 megapixels.
- [ ] Unit test: per-mat scope renders one image per mat with the mat's packed layout.
- [ ] Kittest: Export, choose PNG, 150 dpi; the file list shows the pixel size and the written file decodes to it.
- [ ] Kittest: select a heart, Copy as PNG; the clipboard holds PNG bytes that decode to an image with a transparent corner.
- [ ] Snapshot `54-export-raster.png` (the dialog with PNG options).
- [ ] `docs/guide.md` adds PNG and JPEG to the Export section with the options and the size limit, and Copy as PNG under the clipboard.
