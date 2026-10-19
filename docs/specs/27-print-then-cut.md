# 27 Print Then Cut

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec27.rs` pass.

**Wave 6. Depends on 09, 25 (PDF writer). Size L.** Runs in parallel with 26.

## Problem

`composition::compose` (spec 09) keys every placement unit by `(print_then_cut, colour)`. A Print Then Cut unit's key is `(true, anchor.fill)` in `composition::units`, so a red Print Then Cut sticker and a blue one land on two separate mats. For cut material that is right, because each colour is a different sheet. For Print Then Cut it is wrong. Both images are printed on the same sheet of paper, so colour says nothing about which sheet they belong on.

Print Then Cut images need their own page-composition rules: page size, a printable area smaller than the page, how many images fit on one page, and the order they are placed in. Kerf does not have those rules yet.

The Make flow is also missing a Print step. Its steps are Set, Load and Go, so there is nowhere to print the sheet before the cut.

## Goal

Print Then Cut mats in Kerf are composed by Kerf's own page rules, each rule stated here and pinned by a test. The Make flow gains the Print step. It renders each Print Then Cut page to a PDF of the artwork only, which the user can print.

The print sheet carries no registration marks. A machine that lines a cut up by reading marks off the sheet cannot cut it directly, and the Print step says so plainly.

## Step 1: decide the page rules

Settle each question below before writing code, and record the answer and its reasoning in a new section "Print Then Cut composition" in `docs/evidence-webapp.md`.

1. **Page sizes.** Which paper sizes are offered (Letter, A4, Tabloid, others), and do they depend on the machine family?
2. **Printable area.** What is the maximum Print Then Cut image area per page and per machine family? Is it a single rectangle or margins per side?
3. **Bleed.** Is bleed on by default, how wide is it, and does it count against the printable area?
4. **Grouping key.** Does colour play any part, or only the Print Then Cut flag and the material? How are attached and flattened layers treated? Does a Print Then Cut image ever share a page with a plain cut?
5. **Fit and spill.** How are images placed on a page (packing direction, gap between images), and when does a new page start?
6. **Order.** In what order are pages and images placed (canvas order, size, area), and where do Print Then Cut pages sit relative to cut mats?
7. **Oversize.** What happens to an image larger than the printable area?
8. **Print step.** What does the Print step show and require before Set (print dialog, "Use system dialog", bleed toggle)?

Where a question has no clear answer yet, say so and record what was considered. Do not fill the gap with a guess. The default is that Kerf ignores colour for Print Then Cut and packs images onto pages by size. The tests fix the behaviour.

## Step 2: implement to match

### Data shape

```rust
// src/print_then_cut.rs (new)
pub struct PaperSize { pub name: &'static str, pub width_in: f32, pub height_in: f32 }
pub struct PrintArea { pub paper: PaperSize, pub max_image_in: (f32, f32), pub bleed_in: f32 }

pub fn print_area(family: MachineFamily, paper: PaperSize) -> PrintArea;   // the recorded table
pub fn pages(units: &[Unit], area: &PrintArea) -> Vec<Mat>;  // composition's Print Then Cut units; the recorded fit, spill and order rules
```

`Mat` gains `page: Option<PrintArea>`, set only on Print Then Cut mats. `Mat` is never serialised, so no project format changes. `compose` keeps building cut mats exactly as today. It hands Print Then Cut units to `print_then_cut::pages` and puts the result where the rules say.

### Rules

1. **Composition.** Print Then Cut units no longer key on colour, unless the recorded rule says they do. Everything else in `compose` stays: Attach and container units, cut-mat keys, packing and the operation label. `Mat::colour` for a Print Then Cut page is white, so the thumbnail reads as paper.
2. **Tables.** Paper sizes and printable areas live in one `const` table in `src/print_then_cut.rs`, each row with a comment giving its source.
3. **Prepare.** For a Print Then Cut mat, the sidebar shows a Paper Size dropdown (from the table, through `src/units.rs`) and a Bleed toggle. The mat preview shows the printable area as a dashed rectangle inside the page.
4. **Print step.** `MakeStep` gains `Print`, numbered 0. It comes before Set for a Print Then Cut mat and is skipped for every other mat. It shows the page, **Save print sheet (PDF)** and **Next**.
5. **Print sheet PDF.** `src/print_sheet.rs` renders the mat's images at their page offsets through spec 25's `export::pdf::page`, on a page the size of the chosen paper.
    - Images are drawn filled in their own colours, with bleed if the recorded rule adds it by default. The bleed is an outward offset of the outline (`geometry::offset`), filled in the image colour.
    - The PDF holds artwork only. No registration marks and no sensor box.
    - The file name is `"{project} - Print 1.pdf"`.
    - The Print step says plainly: "This sheet has no registration marks, so a machine that lines a cut up by reading marks off the sheet cannot cut it directly."
6. **Bridge.** `bridge::build_mat_path_data` and the simulator keep working for Print Then Cut mats. The simulator still reaches Done, with the page offsets as transforms.
7. **Open devices.** Spec 24 skips Print Then Cut mats for open devices. That stays.

## Files

- **Owns.** `src/print_then_cut.rs` (new), `src/print_sheet.rs` (new), `src/ui/print_step.rs` (new), `src/ui/tests_spec27.rs` (new).
- **Shared edits.**
  - `src/composition.rs`: make `Unit` `pub(crate)`, route Print Then Cut units to `print_then_cut::pages`, and add `Mat::page`.
  - `src/make_flow.rs`: add `MakeStep::Print` and the step order.
  - `src/ui/set_load_go.rs`: dispatch to `print_step`.
  - `src/ui/prepare.rs`: add Paper Size, Bleed and the printable area outline.
  - `src/ui/project_preview.rs`: the white paper thumbnail.
  - `src/main.rs`: add the modules.
  - `docs/evidence-webapp.md`: the new section.
  - `README.md`: update the Mats section, which today says Print Then Cut mats are keyed by colour.

## Acceptance

- [ ] `docs/evidence-webapp.md` has a "Print Then Cut composition" section that answers questions 1 to 8, each with its reasoning or an explicit "not settled yet" note.
- [ ] Unit test: a red and a blue 2 in Print Then Cut square land on one page, or on two if the recorded rule splits them. The test cites the rule in its message.
- [ ] Unit test: Print Then Cut images fill a page up to the recorded printable area, and the next image that does not fit starts a new page in the recorded order.
- [ ] Unit test: an image larger than the printable area gets the recorded oversize treatment.
- [ ] Unit test: Print Then Cut pages sit in the recorded position relative to cut mats, and cut mats still key by colour as in spec 09.
- [ ] Unit test: every row of the paper and printable area table matches the recorded values.
- [ ] Unit test: `print_sheet` for two images gives a one-page PDF with a MediaBox of the chosen paper in points and two filled paths at the composed offsets. The page holds no path outside the images and their bleed.
- [ ] Unit test: with bleed on, each filled path's bounds are the image bounds grown by the recorded bleed width.
- [ ] Unit test: the existing spec 09 composition tests pass, apart from Print Then Cut cases updated with the recorded rule.
- [ ] Kittest: a design with two Print Then Cut stickers and one cut square goes Prepare, Print (Save print sheet writes a PDF to an injected temp path), Set, Load, Go and Done for the page, then Next Mat runs the cut mat without a Print step.
- [ ] Snapshots `27-prepare-print-page.png` (white page, printable area dashed, two stickers) and `27-print-step.png`.
- [ ] README's Mats section describes the Print Then Cut page rules and the print sheet, and says it carries no registration marks.

## Open question: registration marks for open cutters

Without marks, a printed sheet cannot be cut in register. Some open machines can register: vinyl cutters with an optical sensor read their own mark formats, and a GRBL laser with a camera can align with LightBurn-style targets. Should Kerf draw marks of its own design, or a documented open format per device profile (spec 24), on the print sheet for those machines? That needs a separate spec. It must use an open or original mark format, and each device's sensor needs its own mark geometry and a test against a real machine.
