# 38 Heavy-design benchmark

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec38.rs` pass.

**Wave 8. Depends on everything merged. Size M.** Requested directly by the user: "can we add a test for both? to load in 100+ shapes / layers / etc".

## Goal

One repeatable Kerf-only benchmark that loads the same heavy design, reports its frame times and process statistics, and gates regressions in CI.

## The scenario (one source of truth)

`src/bench.rs` builds a deterministic heavy `Design` from a seed and a size (default 150 top-level objects, about 400 layers in total):
- 60 library shapes in mixed colours and operations (Basic Cut, Pen, Score, Perforate, Print Then Cut), some rotated and mirrored;
- 20 text objects (bundled, system-font fallback-safe and one writing font), some curved, one multi-line;
- 10 groups and 6 attach sets of 3 to 5 members;
- 4 containers, two with Row/Grid auto layout, 5 children each;
- 15 shapes with live effects (Offset, Inset, Sticker, Warp);
- 6 live boolean groups (Weld, Subtract, Slice) of 2 to 3 children;
- 2 components with 8 instances each, some with overrides;
- 2 traced-image Print Then Cut bitmaps from a committed fixture PNG and 2 pattern fills.

It writes three artefacts to a chosen folder: `heavy.kerf.json` (a Kerf project), `heavy.svg` (spec 25 whole-design SVG, filled, inches) and `heavy-mats/` (spec 25 Prepare for Design Space files).

## Kerf, automated

1. **Headless gate (CI).** A `#[ignore = "timing; run on the release build"]` kittest in `src/ui/tests_spec38.rs` loads the scenario and records, as medians over repeated frames: load (parse + sanitise + first full evaluation), idle frame, drag of one shape, drag of a group, marquee select-all, zoom step, undo, Make entry (compose all mats), cut preview open. It prints a table and asserts ceilings: idle and drag under 16 ms, load under 500 ms, Make entry under 250 ms on the release build, and each median under a multiple of a reference workload timed in the same run (see Timing gate in CI). A non-ignored smoke test asserts the scenario's counts (objects, layers) so the generator cannot silently shrink.
2. **Real app.** Add a CLI: `kerf --open <project.json>` opens a project in a tab; `kerf --bench <project.json> --bench-out <metrics.json>` opens it, runs an in-app scripted sequence (no OS input events: the app drives its own state through `edit::run` and view changes over a fixed number of frames: idle 120 frames, drag a shape 120 frames, drag a group, zoom in and out, select all, undo/redo, open Make Prepare, cut preview), measures per-phase frame times from egui's frame clock, then writes the metrics JSON and quits. Frames must actually render (vsync off for the run).
3. **Metrics.** `kerf --bench-generate <dir>` writes the scenario, and `kerf --bench` runs it against `/Applications/Kerf.app` (or `target/release/kerf`) and writes a per-phase metrics JSON (load, idle, drag, zoom, select, undo, Make entry, cut preview).

## Files

- **Owns.** `src/bench.rs`, `src/ui/tests_spec38.rs`, the CLI argument parsing in `src/main.rs`, a small `bench` mode in `src/ui/` (new file `src/ui/bench_mode.rs`), `.gitignore` entry for `bench-results/`.
- **Shared edits.** `src/main.rs` (args), `src/ui/mod.rs` (hook the bench mode; keep it tiny).

## Acceptance

- [ ] Unit test: the scenario has at least 150 top-level objects and 400 layers, every feature listed above is present, and two builds with the same seed are equal.
- [ ] The scenario round-trips through save and load unchanged, and its SVG re-imports with the same number of paths.
- [ ] Ignored timing kittest prints the table; run it once on the release build and paste the numbers in the report.
- [ ] `kerf --bench` on the release build writes a metrics JSON with every phase, and quits by itself. Run it once and paste the output.

## Timing gate in CI

The `timing` job in `.github/workflows/ci.yml` builds release and runs the two timing tests, `heavy_design_frame_times_stay_under_their_ceilings` and `a_frame_of_200_shapes_stays_within_its_ceiling`, with `--nocapture`, so every push prints the table. They stay `#[ignore]`d so the main job and a plain `cargo test` do not run them on a debug build.

**Ceilings are in multiples of a reference workload, not milliseconds.** A fixed ceiling cannot be both loose enough for a slow runner and tight enough to catch a doubling: the frame is about 1.5 ms and a runner may be 2x slower. `reference_ms` times a workload that uses none of Kerf's code (20 000 owned strings into a map, then a sort of the keys; median of 15 runs, about 1.8 ms here) in the same process, and each ceiling is a multiple of it. A faster or slower machine moves the reference and the frame together. Ratios held within 3% across three runs while the machine was at load average 27. The 16 ms, 500 ms and 250 ms ceilings stay beside them as the product budget. The 16 ms frame budgets assume a GPU: with `KERF_SOFTWARE_RENDERER` set, where a software renderer such as Mesa's lavapipe rasterises on the CPU, they are skipped and only the relative ceilings and the load and Make entry budgets apply.

Recorded on an Apple-silicon Mac, release build, median over 180 frames (5 for load, 3 for Make entry), reference 1.79 ms:

| Measure | Median ms | x reference | Ceiling x | Headroom |
|:---|---:|---:|---:|---:|
| load (parse, sanitise, evaluate) | 1.22 | 0.68 | 2.0 | 2.9x |
| idle frame | 1.45 | 0.81 | 1.5 | 1.9x |
| drag one shape | 1.55 | 0.87 | 1.6 | 1.8x |
| drag a group | 1.63 | 0.91 | 1.7 | 1.9x |
| marquee select-all | 3.67 | 2.05 | not gated | |
| zoom step | 1.55 | 0.87 | not gated | |
| undo | 1.46 | 0.82 | not gated | |
| Make entry | 1.88 | 1.05 | 2.0 | 1.9x |
| Prepare frame | 2.07 | 1.16 | 2.2 | 1.9x |
| cut preview open | 4.79 | 2.68 | not gated | |
| 200 shapes, run and render | 8.9 | 4.9 | 8.5 | 1.7x |

Each ceiling is about 1.8 times the recorded ratio: a doubling fails and noise does not. The first run, made while other builds were running, gave idle 2.10, drag one shape 2.25, drag a group 2.36, Make entry 2.72 ms with ratios within 3% of these. A deliberate slowdown (1.5 ms of busy work at the top of every frame, about 2x) failed the idle, drag one shape and drag a group ceilings. The ceilings come from this machine, because the CI runner has not run the job yet: the first CI table shows its ratios, and a ceiling there closer than 1.3x to a recorded ratio should be raised once, with the new numbers written here. The 200-shape test includes a GPU render and is the one most likely to differ on a runner.

### A flake, and what the gate does now

On PR 4 the same commit passed the 200-shape test in one run and failed it in another: 23.22 ms against a 2.71 ms reference, 8.58 x, over the 8.5 x ceiling. The ceiling had been set from the 4.9 x this Mac recorded, but that frame was dominated by the GPU render, a readback of the 1400 by 900 window through the runner's virtual Metal device, which is not Kerf's code and does not scale with the CPU reference. On this Mac the render is 10 ms of a 10.6 ms frame.

Changes, all in the tests:

* The 200-shape test times `harness.run()` (layout, painting, tessellation) and `harness.render()` separately, and gates only the first. The GPU render is printed.
* The reference is sampled once per frame, interleaved with the frames, and the gate is the median frame over the median reference of 61 pairs, so a burst of contention on the runner moves both. The heavy-design test takes its reference before and after its frames and uses their mean.
* The Layers panel fix (above) cut the frames by more than half, which left the original ceilings at 4x to 6x slack and able to hide a real regression. Ceilings are reset to about 2.2 times the new recorded ratios.

Recorded after the change, 20 runs of the heavy test and 25 of the 200-shape test in a loop on this Mac (release, reference 2.5 to 2.6 ms, machine at load average 11 to 15 from other sessions), no failure in either:

| Measure | x reference, min to max | Median | Ceiling x |
|:---|---:|---:|---:|
| idle frame | 0.25 to 0.25 | 0.25 | 0.55 |
| drag one shape | 0.30 to 0.31 | 0.31 | 0.70 |
| drag a group | 0.34 to 0.35 | 0.35 | 0.75 |
| load | 0.64 to 0.71 | 0.66 | 1.45 |
| Make entry | 0.47 to 0.50 | 0.49 | 1.10 |
| Prepare frame | 1.13 to 1.19 | 1.16 | 2.50 |
| 200 shapes, CPU frame | 0.20 to 0.25 | 0.21 | 0.45 |

The 200-shape GPU render is 9.9 to 10.1 ms here and was not gated. A deliberate 1 ms of busy work per frame (about 2.5 times the idle frame) failed the idle ceiling and the 200-shape ceiling. A slowdown of 2x or less now passes: the smallest ceiling that stays clear of the shared-runner jitter seen so far is 2.2 times, and the first CI tables should be read the same way as before, raising a ceiling once if one sits closer than 1.3x to a runner's ratio.

## Where a drag frame goes

Sampled with `sample(1)` over 25 s of drag frames on the heavy scenario (release, 9 700 samples on the test thread) and checked with a micro-timing of each suspect on the same design:

| Part of the frame | Share |
|:---|---:|
| Layers panel (400 rows, its accessibility nodes included) | about 50% |
| Canvas | 18% |
| `Design::settle` after a moved shape | 3.5% |
| Every whole-design `clone`, in all callers | 1.7% |
| The three whole-design comparisons (`==`) | 0.7% |
| `History::observe`, comparison included | 0.2% |

The per-frame `before` clone costs 0.009 ms and one comparison of equal designs 0.004 ms, against a 1.5 ms frame, because shapes share their geometry through the cache. AUDIT-015's residual is therefore not significant at this size and the clone is unchanged: a revision counter or copy-on-first-write snapshot would add state to keep correct for under 2% of a frame. Revisit if `clone` or `==` passes 10% of the sampled frame, which the gate will show first as a rise in idle and drag ratios.

The cost that is large is the Layers panel, about half of every frame at 400 layers. The next section is the follow-up that fixed it.

## The Layers list lays out only what is in view

`src/ui/layers_panel/outline.rs` flattens the layer tree into one row per visible line, each with where it starts (a row's chips and an open effect list lengthen it, so heights are not fixed). A frame builds that outline, which needs no layout, then lays out only the rows that reach into the scroll area's clip rectangle. The drag hit-test, Shift-click ranges and the rename bookkeeping read the whole outline, so a drop on a row scrolled in by the auto-scroll, a range across rows off screen and a rename left on a row that scrolled away all behave as before. A selection made on the canvas, or a rename begun by F2 on an off-screen row, scrolls that row's line into view.

Two costs the old list paid per row went with it: `effects_list::any` evaluated each shape's geometry just to learn whether it listed anything (it now counts the rows, `effects::count`), and each row looked its shape up by id in the whole design.

`the_layers_list_costs_what_is_in_view_not_what_is_in_the_design` (`src/ui/layers_panel/scrolling.rs`, ignored, release) times the heavy scenario's idle frame on the Layers tab and on the Components tab, which lists nothing; the difference is the list. Median of 240 frames, middle of three alternations, release build, Apple silicon, machine busy with other builds:

| | Layers tab | Components tab | Layers list |
|:---|---:|---:|---:|
| before | 4.13 ms | 0.91 ms | 3.22 ms |
| after | 1.07 ms | 0.88 ms | 0.19 ms (0.31 ms on a second run) |

The gate's idle frame went from 2.13 ms to 1.07 ms, drag one shape from 2.39 ms to 1.33 ms and drag a group from 2.46 ms to 1.49 ms, against reference workloads of 2.59 ms and 4.16 ms: the later run was on the slower machine state and still faster. What is left of the list is the outline, linear in the layers but about a quarter of it, and the roughly 20 rows in view, so no cache is kept. The structural guard is `only_the_rows_in_view_are_laid_out`, which fails if the list builds every row again.
