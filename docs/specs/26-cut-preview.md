# 26 Cut preview

**Wave 6. Depends on 24 (toolpath and ordering), 09, 16. Size M.**

## Problem

Prepare (`src/ui/prepare.rs`) shows where each shape sits on the mat, but not how the machine will move. Spec 24 decides a real machine order: draws before cuts, inside-out, nearest-neighbour travel and blade offset at corners. The user cannot see that order, how much the head travels between cuts, or how long a mat will take. Nor can they see how much of the material a mat uses. That matters when a user is about to cut a sheet of vinyl on a cutter they own.

## Goal

An animated preview of the tool path for each mat, in machine order:

- Cut and draw moves are drawn solid, in the operation's style.
- Travel moves are dashed.
- A scrubber, Play and Pause, and a playback speed control the animation.
- Each mat shows an estimated time and its material usage.

The preview is available from Prepare and from the canvas View menu. The estimator is pure and tested.

## Data shape

```rust
// src/output/estimate.rs (pure, beside spec 24's toolpath)
pub struct Motion {                         // one straight move in machine order, in mm
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub kind: MoveKind,                     // Travel, Settle (tool lowering, in place), or Work(Operation)
    pub start_s: f32,                       // cumulative time at the start of this move
    pub duration_s: f32,
}

pub struct Rates {
    pub work_mm_s: f32,                     // from the profile and material, spec 24 rule 8
    pub travel_mm_s: f32,
    pub tool_change_s: f32,                 // pen or knife up/down, per stroke
}                                           // passes are already strokes: toolpath::with_passes repeats them

pub struct Estimate { pub motions: Vec<Motion>, pub total_s: f32, pub work_mm: f32, pub travel_mm: f32 }
pub struct Usage { pub bounds_in: (f32, f32, f32, f32), pub area_in2: f32, pub mat_fraction: f32 }

pub fn estimate(path: &Toolpath, rates: &Rates) -> Estimate;
pub fn usage(design: &Design, mat: &Mat, size: MatSize) -> Usage;
pub fn position_at(est: &Estimate, t_s: f32) -> ((f32, f32), usize);   // head position and current motion index
```

`estimate` takes the output of `output::mat_toolpath` (spec 24's `order`, `blade::compensate` and `with_passes`, the same pipeline `output::job` runs), so the preview shows exactly the order that would be sent. Travel joins each stroke's end to the next stroke's start, from and back to the origin.

## Rules

1. **Rates.** An open device (spec 24) takes `Rates` from its profile and the selected material: the mapped work speed, `speed_mm_s.1` for travel, and `settle_ms` for tool changes. The simulated cutter has no published speeds, so it uses a fixed table in `estimate.rs`: work speed per `Material::cut_speed` step, plus a nominal travel speed and tool change time. The preview labels the time "Estimate" for the simulator. Before Set has run, it uses the first material in the list and says so.
2. **Multi-pass.** Cut strokes count `passes` times. Draw strokes count once. The animation plays each pass in order.
3. **Usage.** `bounds_in` is the union of the world bounds of the mat's items at their packed offsets. `area_in2` is that rectangle's area. `mat_fraction` is that area over the mat's cutting area (the mat size less `MAT_PADDING_PT` on each side). The panel shows "Material used 4.5 x 3.2 in (12 % of mat)", formatted through `src/units.rs`.
4. **Drawing.** `src/ui/cut_preview.rs` paints the mat with `flow_kit::mat_sheet`. Completed work moves are solid strokes in their operation style from `ui::op_style`. Remaining work moves are a faint 1px line. Travel moves use `egui::Shape::dashed_line` in `TEXT_MUTED`. The head is a small `ACCENT_PRIMARY` circle at `position_at`. With a drag knife, the compensated path is drawn. A toggle **Show original path** overlays the uncompensated outline at 40 % opacity.
5. **Controls.** Play and Pause, a scrubber across the whole time (dragging it sets `t` directly), speed choices 1x, 4x, 16x and 64x, Restart, and a mat picker ("Mat 1 of 3") when there is more than one mat. The readout shows elapsed and total time ("0:42 of 3:15"), work length and travel length. Every control sets `widget_info` and gets a tooltip.
6. **Frame loop.** Playback advances `t` by frame time times speed and requests a repaint only while playing. It never blocks. Playback state (`t`, playing, speed, mat) lives in a small struct on the preview, not in `MakeFlow` or `Design`, so it adds no undo steps.
7. **Entry points.** Prepare gains a **Preview cut** button under the mat preview, which opens the preview on the selected mat. The canvas View menu gains **Cut preview**, which composes with the current device and preferred mat and opens the same view as a modal overlay, like `project_preview::overlay`. Esc closes it.
8. **Per-mat totals.** Prepare's mat list shows each mat's estimated time beside its label, from the same `estimate`.

## Files

- **Owns.** `src/output/estimate.rs` (new), `src/ui/cut_preview.rs` (new), `src/ui/tests_spec26.rs` (new).
- **Shared edits.**
  - `src/output/mod.rs`: add `pub mod estimate`.
  - `src/ui/prepare.rs`: add the Preview cut button and the per-mat time.
  - `src/ui/chrome.rs`: add the View menu item.
  - `src/ui/mod.rs`: add a `cut_preview_open` flag and its overlay call, as `preview_open` does.
  - `README.md`: a new "Cut preview" section.

## Acceptance

- [ ] Unit test: a 10 mm square at (10, 10) with work 10 mm/s, travel 100 mm/s and no tool change gives 4.0 s of work. Travel is 2 x 14.142 mm from and back to the origin, about 0.283 s. The total is within 0.001 s.
- [ ] Unit test: tool change time adds once per stroke, and three passes triple only the cut strokes' time.
- [ ] Unit test: `estimate` keeps spec 24's order. The motion list's work moves follow `order`'s stroke order exactly, and a Pen stroke's motions come before the Basic Cut it is attached to.
- [ ] Unit test: `position_at` at 0 is the origin, at `total_s` is the origin again, and halfway along a single work move is that move's midpoint.
- [ ] Unit test: `usage` for one 2 x 3 in shape on a 12 x 12 mat gives a 6 in² area and the fraction over the padded cutting area.
- [ ] Unit test: the simulator rate table gives a longer time for Cardstock (`cut_speed` 7) than for Vinyl (`cut_speed` 10) on the same path.
- [ ] Kittest: from Prepare, Preview cut opens the preview. Play advances the readout over a few frames, and Pause stops it.
- [ ] Kittest: dragging the scrubber to the end shows the total time as elapsed and every work move as complete.
- [ ] Kittest: View > Cut preview on the canvas opens the overlay, and Esc closes it.
- [ ] Snapshots `26-preview-midway.png` (dashed travel, half the moves done, head visible) and `26-prepare-times.png` (per-mat times in the mat list).
- [ ] README describes the preview, the estimate and the usage figure, and says the simulator's time is a nominal estimate.
