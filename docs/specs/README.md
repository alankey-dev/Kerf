# Feature specs

These specs built out Kerf's own canvas, using the layouts collected in `reference/`. Each spec was one unit of work for one agent. The waves below set the order. Specs in the same wave could run in parallel because they owned different files.

**Every spec here is implemented on `main`** (checked 2026-10-19, and shipped in v0.0.1). Each spec file carries a `Status` line under its title. Numbers 36 and 40 were never written. The specs are now a design record. New bugs, features and follow-up work go in [GitHub issues](https://github.com/alankey-dev/Kerf/issues), not in new spec files.

## Waves

| Wave | Spec | Depends on | Size |
|---|---|---|---|
| 0 | [00 Units everywhere](00-units.md) | none | S |
| 0 | [02 Groups and Attach](02-groups-attach.md) | none | L |
| 0b | [01 UI kit: popovers, tiles, icons, tooltips](01-ui-kit.md) | 00 | M |
| 1 | [03 Align grid](03-align-grid.md) | 01 | S |
| 1 | [04 Combine: Slice and Flatten](04-combine.md) | 01 | M |
| 1 | [06 Selection chrome](06-selection-chrome.md) | 00, 01, 02 | M |
| 1 | [07 Pen sizes and colours](07-pen-tools.md) | 01 | M |
| 1 | [08 Text model](08-text-model.md) | 00 | M |
| 2 | [05 Effects: Sticker and Warp](05-effects.md) | 01, 04 | L |
| 2 | [09 Mat composition](09-mat-composition.md) | 00, 02, 07 | L |
| 2 | [10 Text toolbar and Format](10-text-toolbar.md) | 01, 08 | M |
| 2 | [11 Layers panel actions](11-layers-actions.md) | 01, 02, 04 | S |
| 2 | [13 Context menus and Layers selection](13-context-menus-layer-select.md) | 01, 02, 03, 04 | M |
| 2 | [14 Shapes panel search and filter](14-shapes-filter.md) | 01 | S |
| 2 | [15 System fonts](15-system-fonts.md) | 08, 10 | M |
| 3 | [16 Make flow matches the canvas identity](16-make-flow-identity.md) | 09 | M |
| 3 | [17 Node editing: add, remove, convert, smooth](17-node-editing.md) | 01, 06 | L |
| 3 | [18 Containers](18-containers.md) | 02, 03, 06, 09, 11, 13 | L |
| 3 | [19 Operation-specific canvas rendering](19-operation-rendering.md) | 06, 07 | M |
| 3 | [20 Layers drag and drop](20-layers-drag-drop.md) | 02, 11, 13 (18) | M |
| 4 | [21 Live, editable effects on layers](21-live-effects.md) | 04, 05, 12, 17, 18, 19, 20 | XL |
| 8 | [38 Heavy-design benchmark](38-benchmark.md) | all | M |
| 8 | [39 Interactive tutorial, and starting on Home](39-tutorial-and-home-start.md) | all | L |
| 9 | [41 Build and release on Windows, macOS and Linux](41-cross-platform-release.md) | all | L |
| 9 | [42 Monogram Maker](42-monogram-maker.md) | 08, 10, 12, 15, 18, 21, 29, 31 | L |
| 9 | [43 Create AI](43-create-ai.md) | 22, 23, 25, 28 | XL |
| 10 | [44 Clipboard: Cut, Copy, Paste](44-clipboard.md) | 22, 23, 33 | M |
| 10 | [45 Ruler guides](45-ruler-guides.md) | 22, 23, 34 | M |
| 10 | [46 Editable grid](46-editable-grid.md) | 22, 23, 34 | S |
| 11 | [47 Snap points](47-snap-points.md) | 45, 46, 17, 34 | M |
| 11 | [48 Zoom to fit, zoom to selection and view navigation](48-zoom-and-view.md) | 22, 23 | S |
| 11 | [49 Live shape parameters](49-live-shape-parameters.md) | 22, 23, 17, 21, 34 | L |
| 11 | [50 Select Same, Select Inverse and lasso](50-select-same-and-lasso.md) | 22, 23, 19, 34 | M |
| 12 | [51 Text on a path](51-text-on-path.md) | 12, 17, 21, 29, 47 | L |
| 12 | [52 Step and repeat, rotate copies, skew and pivot](52-repeat-and-transform.md) | 18, 22, 23, 31, 34 | M |
| 12 | [53 SVG import fidelity](53-svg-import-fidelity.md) | 22, 23, 02, 18, 19, 28, 44 | L |
| 12 | [54 Raster export: PNG and JPEG](54-raster-export.md) | 25, 37, 44 | S |
| 13 | [55 Corner tool](55-corner-tool.md) | 17, 21, 49 | M |
| 13 | [56 Swatches, recent colours and the eyedropper](56-swatches-and-eyedropper.md) | 22, 23, 44 | M |
| 13 | [57 Shape Builder and Knife](57-shape-builder-and-knife.md) | 04, 17, 21, 47 | L |
| 14 | [58 USB machine detection](58-usb-machine-detection.md) | 24 | M |
| 3 | [12 Text: ungroup, curve, edit in place](12-text-advanced.md) | 02, 05, 08, 10 | L |
| 5 | [22 Engineering foundation](22-foundation.md) | 00-20 | XL |
| 5 | [23 One command model](23-one-command-model.md) | 00-20 | L |
| 5 | [35 Make flow hardening](35-make-flow-hardening.md) | 09, 16 | M |
| 5 | [24 Open cutters: HPGL and G-code devices](24-open-cutters.md) | 09, 16, 18 (22, 23) | XL |
| 5 | [25 Export: SVG, DXF, PDF and Design Space handoff](25-export.md) | 09, 18, 19 | L |
| 6 | [26 Cut path preview and estimates](26-cut-preview.md) | 09, 16, 24 | M |
| 6 | [27 Print Then Cut](27-print-then-cut.md) | 09, 25 | L |
| 6 | [28 Image tracing](28-image-trace.md) | 22, 23, 02, 09, 19 | XL |
| 6 | [29 Single-line writing fonts](29-single-line-fonts.md) | 22, 23, 07, 08, 10, 12, 15 | L |
| 6 | [32 Command palette](32-command-palette.md) | 22, 23, 14, 15 | M |
| 6 | [33 Autosave, versions, crash recovery, tabs](33-autosave-versions-tabs.md) | 22, 23 | XL |
| 7 | [37 Images print, thumbnail and flatten](37-print-images.md) | 25, 27, 28 | S |
| 7 | [30 Nesting](30-nesting.md) | 22, 23, 09, 18, 21 | L |
| 7 | [31 Components](31-components.md) | 22, 23, 02, 18, 21 | XL |
| 7 | [34 Precision tools](34-precision.md) | 22, 23, 21, 28 | XL |

- **New logic goes in a new module.** A spec adds its geometry, layout or UI in its own file (for example `src/warp.rs` or `src/ui/toolbar/align.rs`) and touches shared files only to add an enum variant, a field or a dispatch line.
- **Shared files take small edits only.** `src/design.rs`, `src/ui/mod.rs` and `src/ui/canvas.rs` are shared. Keep each edit to a few lines in a clearly separate place, so a rebase is trivial.

## Conventions every spec inherits

- **Identity.** Kerf keeps its own look from `src/ui/tokens.rs`: white panels, 1px `BORDER_SUBTLE` lines and the violet `ACCENT_PRIMARY`. The reference screenshots use a green accent. Follow their layout and behaviour, never their colours.
- **Units.** After spec 00, every length shown or typed goes through `src/units.rs`. Do not format a length with a literal `"in"` or `"cm"` anywhere else. A test enforces this.
- **Tooltips.** Every icon-only control gets `ui::tooltip::tip` with a name, one line on what it does and its shortcut if it has one.
- **Accessibility labels.** Every interactive widget sets `widget_info` with a label, so `egui_kittest` tests can find it with `get_by_label`.
- **Undo.** Every design change must be one undo step. `History::observe` in `src/ui/mod.rs` already snapshots the design each frame, so do not mutate the design across several frames for one user action.
- **Project files.** New `Shape` or `Design` fields use `#[serde(default)]` so existing project JSON still loads. Add a test that loads a project saved before the change.

## Definition of done

1. `cargo test` passes and `cargo clippy --all-targets` adds no warnings.
2. New behaviour has tests. Pure logic gets unit tests next to the code. UI behaviour gets an `egui_kittest` test in the spec's own `src/ui/tests_specNN.rs` that drives the real app, using the `click_at`, `drag_along` and `snapshot_harness` helpers in `src/ui/test_harness.rs`.
3. Each visible change is pinned by a committed baseline (`assert_snapshot`, written with `UPDATE_SNAPSHOTS=1`) or by a pixel assertion. Look at the new baseline before calling the work done.
4. `docs/guide.md` describes the new behaviour in the matching section. `README.md` stays short: touch it only to add a line to Features.
5. The work reaches `main` through a pull request on GitHub (`alankey-dev/Kerf`), never a local merge. The PR links its issue with `Closes #N`, its description checks off the issue's acceptance list, and the PR carries the `docs/guide.md` and `README.md` changes from item 4.
