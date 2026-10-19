# stroke_font

Converts single-line font data to ODS stroke font JSON (`*.odsstroke.json`), the one stroke format Kerf reads. Run it by hand when a bundled font changes; the app build never runs it.

It reads two inputs:

- **Hershey `.jhf` files.** The first 95 records become ASCII 32 to 126. Coordinates flip to y up with the baseline at 0, units per em is 32, and the ascender and descender are the highest and lowest points in the font.
- **SVG fonts.** Converted by `StrokeFont::from_svg_font` in `src/stroke_font.rs`, the same code that reads a user's uploaded SVG font. The tool prints how many subpaths stayed open, collapsed to a centreline, or stayed closed loops.

## Run

```sh
cargo run --example stroke_font -- hershey <in.jhf> <out.json> --name "<Name>"
cargo run --example stroke_font -- svg <in.svg> <out.json> [--name "<Name>"]
```

`--name` sets the family name the font menu shows. For an SVG font it defaults to the `font-family` of `<font-face>`.

## Sources

The bundled fonts, their upstream commits, licences and the exact commands used are in `assets/PROVENANCE.md`.
