# 28 Image trace: upload a bitmap and cut it

**Wave 6. Depends on 22, 23, 02, 09, 19. Size XL; run as two phases (asset store and Bitmap shape first, then the trace panel).** Kerf imports SVG only. A PNG or JPG cannot be turned into a cut image with background removal and a single-layer or Print Then Cut result.

## Problem

Upload (`shape_library::import_svg`, the rail item in `src/ui/chrome.rs`) accepts SVG only. There is no bitmap shape, so a photo or a scanned drawing cannot be cut, drawn or printed. The `image` crate is compiled with PNG only, for thumbnails.

Kerf's trace should avoid the common weaknesses. It should not be modal or slow, it should offer more than one tolerance slider and an eraser, it should split an image into colour layers, and it should preview the cut line throughout, not only at the end.

## Goal

Upload a PNG, JPG or WebP, clean it up, and get cut paths, in one panel with a live preview of the cut line over the image.

### Bitmap assets (data, phase 1)

- `History::observe` snapshots the whole `Design` every frame, so pixels must never live inside `Design`. Add a content-addressed **asset store**: `src/assets.rs` writes each imported image once to `<projects dir>/assets/<blake3 hex>.png` (re-encoded as PNG, alpha kept) and hands back an `AssetId` (the hash, a newtype, not a bare `String`). Decoded textures are cached in memory by `AssetId`.
- A new `ShapeKind::Bitmap { asset: AssetId, px: (u32, u32), clip: Vec<Contour> }`. `clip` is the cut line in the bitmap's own frame; empty means the image rectangle. It defaults to Print Then Cut. The canvas draws the texture clipped to `clip` (spec 19's Print Then Cut style: print, thin cut line, faint shadow).
- Saving a project keeps assets in the shared `assets/` folder; loading resolves them there. A missing asset renders as a grey hatched box named "Missing image" and still loads, like a missing font (spec 15). Garbage collection of unreferenced assets belongs to spec 33's pruning.
- `#[serde(default)]` on everything new; a test loads a project saved before this spec.

### Decode and limits

- Turn on the `image` crate's `jpeg` and `webp` features (pure-Rust decoders, MIT or Apache-2.0).
- Read the header first (`ImageReader::into_dimensions`). Refuse anything over 50 megapixels with a clear message naming the size, before allocating. Set `image::Limits` so a lying header cannot allocate more.
- Images over 4096 px on the long side are stored at full size but traced on a copy downscaled to 4096 px. The live preview traces a 768 px copy.
- EXIF orientation is applied on import so phone photos stand upright.

### The trace panel (phase 2)

Upload of a bitmap opens a full-height panel over the canvas (`src/ui/trace_panel.rs`), not a separate wizard. Left: the image with the proposed cut lines drawn over it in `ACCENT_PRIMARY`, zoomable and pannable like the canvas. Right: three steps as collapsible sections, all editable in any order, each change re-running the preview.

1. **Background.**
   - **Remove colour**: click the image to remove the region of that colour. A Tolerance slider (0 to 100, colour distance in CIE Lab, so it matches what the eye sees) and a Contiguous toggle (on: flood fill from the click; off: every pixel within tolerance anywhere). Each click is added to a list of removals that can be removed again one by one.
   - **Auto remove**: floods from every border pixel using the dominant border colour and an automatic tolerance from the border histogram. One button, undoable inside the panel.
   - **Edges**: optional Sobel edge mask (computed in `src/trace/raster.rs`, no new crate) that stops a flood fill at strong edges, so a white subject on a white-ish background survives.
   - **Erase** and **Restore** brushes with a size slider, for the cases tolerance cannot fix.
   - The removed area shows as a checkerboard.
2. **Colours.** Mode: **Threshold** (one layer; slider 0 to 255 on luminance, with Invert) or **Posterise** (colour count 2 to 16). Posterise shows the palette as swatches; clicking a swatch merges it into its nearest neighbour.
3. **Clean-up.** **Speckle** (drop regions under N px, shown in the current units as an area), **Smoothing** (0 to 100) and **Corner threshold** (degrees; below it a join stays sharp). Each maps to one vtracer parameter, listed below.

The preview runs on a 768 px copy and refreshes within 150 ms of the last change; a small spinner shows while it is stale. **Apply** runs the full-resolution trace with a progress bar and Cancel.

### Output

A segmented control chooses what Apply makes:

- **Cut layers** (default for Posterise): one `ShapeKind::Imported` shape per colour, each filled with its palette colour, Basic Cut, gathered in one Group (spec 02) at the image's position and size. A **Layering** toggle picks **Stacked** (each layer includes everything above it, for layered cardstock) or **Side by side** (layers do not overlap, for vinyl). These are vtracer's `Hierarchical::Stacked` and `Hierarchical::Cutout`.
- **Single cut**: the union of every kept region as one Basic Cut shape, the silhouette.
- **Print Then Cut**: one `ShapeKind::Bitmap` with the cleaned image (removed background made transparent) and `clip` set to the silhouette. The bitmap is kept; only the outline is cut.

The whole Apply is one undo step and selects the result.

### Tracing approach: vtracer

> **As built.** The `vtracer` crate fails `cargo deny`, so `src/trace/` ports vtracer's pipeline onto the `visioncortex` crate directly and does not depend on `vtracer`. Read "the vtracer crate" below as that port; the README names vtracer as the origin of the approach.

Use the `vtracer` crate (MIT, by visioncortex, built on the MIT `visioncortex` crate). Reasons:
- It is pure Rust, so it builds on every platform with no C toolchain, like the rest of Kerf.
- It does colour clustering, hierarchical stacking, speckle filtering, corner detection and spline fitting in one pass. Its parameters (`filter_speckle`, `color_precision`, `layer_difference`, `corner_threshold`, `length_threshold`, `splice_threshold`, `mode: Spline`) map one to one onto the panel's controls, so Kerf writes almost no curve-fitting code.
- Potrace is GPL-2.0, which Kerf will not link. A potrace-style marching squares plus curve fitter would be Kerf code to write, test and tune, and it handles one colour at a time; posterise would need a separate clustering step.

The adapter `src/trace/mod.rs` is the only file that calls vtracer. It takes a cleaned RGBA buffer and `TraceSettings` and returns `Vec<TracedLayer { colour, contours: Vec<Contour> }>` in pixel space; scaling to inches happens once, at the boundary. If the vtracer API only returns SVG strings, parse the `d` data with `svgtypes` (MIT or Apache-2.0, already a `usvg` dependency) rather than adding a parser. Threshold mode pre-binarises the buffer and runs vtracer in binary mode.

### Threading

Decoding, background removal, preview traces and the full trace all run on one worker thread (`src/trace/worker.rs`), fed by an `mpsc` channel like the simulator. Each job carries a generation number; the UI drops any result older than the latest request, and the worker skips queued jobs that are already superseded. The full trace checks a cancel flag between stages. The UI thread never decodes or traces, and asks for a repaint while a job is in flight.

## Files

- **Owns.** `src/assets.rs` (new: asset store, `AssetId`, texture cache), `src/trace/mod.rs` (new: vtracer adapter and settings), `src/trace/raster.rs` (new: Lab distance, flood fill, auto background, Sobel, brushes; pure), `src/trace/worker.rs` (new), `src/ui/trace_panel.rs` (new), `src/ui/tests_spec28.rs` (new), test images under `tests/fixtures/trace/`.
- **Shared edits.** `Cargo.toml` (`vtracer`, `blake3`, `image` features `jpeg` and `webp`), `src/design.rs` (`ShapeKind::Bitmap`, its outline and bounds), `src/ui/op_style.rs` (paint a clipped texture), `src/ui/chrome.rs` (Upload accepts bitmaps and opens the panel), `src/composition.rs` (a Bitmap is a Print Then Cut unit), `src/bridge.rs` (Bitmap cut path is `clip`), `src/ui/mod.rs` (`mod` lines and the panel mount). Use spec 23's command path for Upload and Apply.

`blake3` is CC0-1.0 or Apache-2.0.

## Acceptance

- [ ] Unit test: Lab flood fill at tolerance 10 on a fixture with a white background and a red disc removes the background and keeps every disc pixel; Contiguous off also removes a detached white island.
- [ ] Unit test: Auto remove on the same fixture gives the same mask within 1 % of pixels.
- [ ] Unit test: Threshold trace of a black 2 x 2 in square on white, imported at 100 px per inch, gives one closed contour within 0.02 in of the square.
- [ ] Unit test: Posterise with 3 colours on a three-colour fixture gives three layers whose fills match the source colours within a Lab distance of 3.
- [ ] Unit test: Stacked layers nest (each contains the one above); Side by side layers do not overlap (pairwise intersection area under 0.1 %).
- [ ] Unit test: Speckle at 20 px drops a 3 px dot and keeps a 40 px dot.
- [ ] Unit test: a 60 megapixel header is refused before decode with a message naming the size.
- [ ] Unit test: the worker drops a stale preview result when a newer generation was requested.
- [ ] Unit test: saving a Bitmap shape writes one asset file, a second import of the same image reuses it, and a project referencing a missing asset loads.
- [ ] Unit test: an old project loads with no Bitmap shapes.
- [ ] Unit test: `compose` puts a Bitmap on a Print Then Cut mat and its payload path is the clip outline.
- [ ] Kittest: Upload a fixture PNG, click the background, choose Single cut, Apply; one cut shape appears and one Undo removes it.
- [ ] Kittest: Upload, Posterise 3, Cut layers, Apply; Layers shows a Group of three coloured shapes.
- [ ] Kittest: Upload, Print Then Cut, Apply; the shape's operation is Print Then Cut and Make shows a Print Then Cut mat.
- [ ] Snapshots `28-trace-panel.png` (preview with cut lines over a posterised image), `28-background.png` (checkerboard after a removal), `28-ptc.png` (the Bitmap on the canvas).
- [ ] README describes image upload, the three steps, the output choices and the size limits, and lists visioncortex (the vtracer port's base), blake3 and the extra image features with their licences.
