# Changelog

All notable changes to Kerf are listed here, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/). `tools/release/bump.sh` adds a section for the next version; the release workflow copies a version's section into its GitHub Release notes.

## [0.0.1]

The first release of Kerf, a fast, offline desktop editor for cutting machines.

### Added

- Design: shapes with live parameters, Pen and Pencil, node editing, the Corner tool, Shape Builder and Knife, Weld, Slice and Flatten, live effects (Offset, Inset, Sticker, Warp), groups, attach sets, containers with auto layout, and components.
- Text: system fonts, text on a path, curved text, and single-line writing fonts for pens.
- Layout: ruler guides, an editable grid, one unified snapper with angle constraints, step and repeat, Select Same and lasso selection, align and distribute, and zoom to fit, selection and actual size.
- Colour: a swatch picker with recent colours, saved per-project palettes, an eyedropper and Replace colour.
- Images: upload, trace to cut paths, pattern fills, and Print Then Cut.
- Make: mats composed by colour, Optimise layout, a cut path preview with time estimates, and the Set, Load, Go flow.
- Machines: plot to HPGL and G-code open cutters over serial, with USB machine detection.
- Export: SVG, DXF, PDF, raster PNG and JPEG, Copy as PNG, and a per-mat SVG handoff.
- Workflow: tabs, autosave, version history, crash recovery, a command palette, an interactive tutorial, and typed values in inches or centimetres.
- Installers for every desktop platform: a universal `.dmg` for macOS, `.msi` and portable `.zip` for Windows on x64 and arm64, and `.AppImage`, `.deb` and `.tar.gz` for Linux on x86_64 and aarch64, with a `SHA256SUMS` file.

[0.0.1]: https://github.com/alankey-dev/Kerf/releases/tag/v0.0.1
