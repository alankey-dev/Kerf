# 00 Units everywhere

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its module unit tests in `src/` pass.

**Wave 0. Depends on nothing. Size S.**

## Problem

The units setting (inches or centimetres) only reaches the rulers and the inspector. Other surfaces ignore it:

| Surface | Today | File |
|---|---|---|
| Project Preview mat size | Always cm | `src/ui/project_preview.rs` (`mat_card`) |
| Prepare mat size dropdown | Always inches ("12 in x 12 in") | `src/ui/prepare.rs`, names in `src/machine.rs` |
| Offset distance | Always " in" | `src/ui/edit_toolbar.rs` (`effects_popover`) |
| Text size | Always " in" | `src/ui/mod.rs` (`text_editor`) |
| Unit maths | `CM_PER_IN` duplicated in two files | `inspector.rs`, `project_preview.rs` |

The setting also lives on the canvas `View`, so the Make flow screens never see it, and it resets every launch.

## Goal

One units setting drives every length the app shows or accepts. Switching units in any place (ruler corner or View menu) updates every surface on the next frame. The choice survives a restart.

## Data shape

A new pure module `src/units.rs` owns units. Move `Units` there from `src/ui/canvas.rs`.

```rust
pub enum Units { Imperial, Metric }

impl Units {
    pub const CM_PER_IN: f32 = 2.54;
    pub fn suffix(self) -> &'static str;            // "in" | "cm"
    pub fn from_in(self, inches: f32) -> f32;        // inches to display value
    pub fn to_in(self, shown: f32) -> f32;           // display value to inches
    pub fn len(self, inches: f32) -> String;         // "5.08 cm", "2 in", 2 dp, trailing zeros trimmed
    pub fn size(self, w_in: f32, h_in: f32) -> String; // "5.08 cm x 5.08 cm"
}
```

The ruler helpers (`major_pt`, `label_step`, `ticks`) stay in `canvas.rs` as free functions or an extension, since they are canvas drawing concerns.

`MatSize` in `src/machine.rs` gains `metric_name: &'static str`. Use the metric names where known (`30.5 cm x 30.5 cm`, `30.5 cm x 48.26 cm`, `30.5 cm x 61 cm`, from `docs/evidence-ui.md` section 6). Paper sizes (A4, Letter and so on) keep one name. Add `MatSize::name(units)`.

A text size is a font size, not a length. Spec 08 moves it to points. Until then, show it through `Units` like any other length.

## Where the setting lives

Move `units` from `canvas::View` to the app struct (`Kerf` in `src/ui/mod.rs`) and pass `Units` by value to every surface that shows a length: canvas, inspector, edit toolbar, text editor, Project Preview, Prepare. Persist it in a small `settings.json` beside the projects directory, written when it changes and read at start-up. Tests use the injectable projects directory, so they stay isolated.

## Enforcement

Add a unit test in `src/units.rs` that walks `src/` and fails if any file other than `units.rs` and `machine.rs` formats a unit by hand. It matches a string literal that is exactly `"in"` or `"cm"`, a `suffix(" in")` or `suffix(" cm")` call, or a format placeholder followed by a unit, using the pattern `\{[^}]*\} ?(in|cm)\b`. Ordinary words such as "Zoom in" do not match. This turns the convention into a check instead of a reminder.

## Files

- **Owns.** `src/units.rs` (new), `src/ui/project_preview.rs`, `src/ui/prepare.rs`, `src/ui/inspector.rs`, `src/machine.rs`.
- **Shared edits.** `src/ui/canvas.rs` (move the enum out, read units from a parameter), `src/ui/mod.rs` (hold and persist the setting, pass it down), `src/ui/edit_toolbar.rs` (offset suffix), `src/ui/chrome.rs` (View menu binds to the app setting), `src/main.rs` (`mod units`).

## Acceptance

- [ ] Switching to cm changes the rulers, inspector fields, Project Preview, Prepare dropdown, Offset distance and text size together.
- [ ] Typing `5.08` in the W field in cm makes a 2 in wide shape.
- [ ] The Offset distance in cm accepts and shows cm, and the stored offset is in inches.
- [ ] Quitting and relaunching keeps the chosen units.
- [ ] The literal-scan test passes and fails if you add `format!("{} in", x)` to any UI file.
- [ ] A kittest test switches units from the View menu and asserts the Project Preview label and the inspector suffix both read cm.
