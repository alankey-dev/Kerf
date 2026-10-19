# 15 System fonts for text

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec15.rs` pass.

**Wave 2b. Depends on 08, 10. Size M.** Requested directly by the user: "text should also include system text", meaning the fonts installed on the machine.

## Problem

`text::FontChoice` is a two-value enum (Inter, Space Grotesk), both bundled. Kerf should list the user's installed system fonts alongside its own, and today it cannot use any font the user already has.

## Goal

The text toolbar's font menu (spec 10) offers every installed system font family as well as the bundled ones, and text in a system font outlines, cuts, draws, saves and reloads exactly like bundled text.

### Data shape

- Replace the `FontChoice` enum with a font reference that is either a bundled font or a system family name, for example `enum FontRef { Inter, SpaceGrotesk, System(String) }` (architect may choose a better shape). Text keeps its existing `FontStyle` (Regular, Bold, Italic, Bold Italic); for a system family, style resolves to the closest installed face (weight 400/700, italic or oblique), falling back to synthetic bold or the spec 08 italic shear when the family lacks that face.
- **Back-compat.** Projects saved with `"Inter"` or `"SpaceGrotesk"` still load as the same bundled fonts. Add a test that loads a project JSON saved before this change.
- **Missing font.** A project that names a system family not installed on this machine loads, renders with Inter, and keeps the original family name in the file so it round-trips unchanged. The font menu shows the missing name in `TEXT_MUTED` with "(missing)".

### Font discovery

- Enumerate installed families once, off the UI thread or lazily on first open of the font menu, so start-up and the first frame do not block. `fontdb` is the natural choice (it is the library `usvg` uses); add it as a direct dependency. Cache loaded face bytes by (family, style) so outlining a string does not reload the file.
- Use `ttf-parser` (already a dependency) for outlines and kerning, as spec 08 does for the bundled fonts. Text layout must not care whether the face is bundled or system.
- Skip faces `ttf-parser` cannot outline (bitmap-only colour emoji fonts, for example).

### Font menu

- The font dropdown shows "Kerf fonts" (the bundled ones) first, then "System fonts" sorted alphabetically.
- A search field at the top of the menu filters by family name; the list scrolls; typing narrows instantly with hundreds of families.
- Each family name is drawn in its own face where egui can load it cheaply; otherwise in the UI font. Loading previews must not stall the frame (load lazily for visible rows, or fall back to the UI font).

## Files

- **Owns.** `src/system_fonts.rs` (new: discovery, cache, face lookup), `src/ui/tests_spec15.rs` (new).
- **Shared edits.** `src/text.rs` (font reference type and face resolution), `src/design.rs` (the field type on `ShapeKind::Text`, `add_text` signature), `src/ui/toolbar/text.rs` (the font menu), `src/ui/fonts.rs` (preview faces), `Cargo.toml`. Update every `FontChoice` call site.

## Acceptance

- [ ] Unit test: an old project with `"font": "Inter"` and one with `"SpaceGrotesk"` load as the bundled fonts.
- [ ] Unit test: a project naming a non-existent system family loads, outlines with Inter, and serialises back with the original family name.
- [ ] Unit test (skipped gracefully if the machine has no such font): text in a common macOS system family (for example Helvetica or Arial) produces outlines that differ from Inter's for the same string.
- [ ] Kittest: open the font menu, search for a system family present on the machine, pick it, and the selected text's font is that system family and its outline changes. Skip with a clear message if no system fonts are found.
- [ ] The app's first frame is not delayed by font discovery (measure: discovery is not on the start-up path, or a test asserts the menu opens before discovery completes with a "Loading fonts..." row).
- [ ] Snapshot `15-font-menu.png` with the System fonts section and search.
- [ ] README Text section describes system fonts and the missing-font behaviour.
