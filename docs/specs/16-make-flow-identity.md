# 16 Make flow matches the canvas identity

**Wave 3. Depends on 09 (which owns prepare.rs and edits set_load_go.rs). Size M.** Requested directly by the user: "fix the next steps (mac) because the ui does not match the canvas".

## Problem

The canvas uses the Kerf identity from `src/ui/tokens.rs`: white panels, 1px `BORDER_SUBTLE` dividers, the light `BG_CANVAS_VOID` behind a floating white artboard, Inter, and one violet `ACCENT_PRIMARY`. The Make flow screens after it use none of it (`prepare.rs`, `set_load_go.rs` and `machine_modal.rs` reference zero tokens). They render default egui widgets:

- **Prepare.** The top bar reads "Kerf  Prepare" in plain text instead of the canvas top bar with the Kerf pill. The sidebar has default radio buttons and a grey ComboBox. Cancel and Continue are small grey default buttons. The mat preview is a pale-blue grid with grey lines, not the canvas mat look. The Payload panel uses default collapsing headers.
- **Set, Load, Go.** Default grey group frames, egui's blue selection highlight on the pressure segmented control, the active step title in egui's link blue, an orange warning in a default colour, and small default buttons.
- **Machine modal.** Default window chrome and widgets.

See `target/snapshots/make-prepare.png` and `target/snapshots/07-load-pens.png` for the current state.

## Goal

Every Make flow screen looks like it belongs to the same app as the canvas. Lay out the flow from `docs/evidence-ui.md` and the reference images, and take every colour, radius, font and spacing from `tokens.rs`.

### Shared flow widgets (new module)

Add `src/ui/flow_kit.rs` with the small set of styled widgets the flow needs, each built on the tokens:
- `primary_button` (accent fill, `ON_PRIMARY` text, `PRIMARY_HOVER` on hover, `BUTTON_H`, min `BUTTON_MIN_W`, radius as the canvas Make button) and `secondary_button` (white, 1px `BORDER_SUBTLE`, `TEXT_PRIMARY`). If the canvas header already has a Make/Save button helper, reuse or move it here; do not create a second look.
- `segmented` (Default / More / Less, Horizontal / Vertical style): selected segment uses `ACCENT_HOVER` fill and `ACCENT_PRIMARY` text, never egui's default blue.
- `radio_row` and `toggle_row` in the inspector's row style.
- `dropdown` styled like the inspector's operation dropdown (spec 01 `popover` + `text_row`), replacing the default ComboBox. Keep the existing accessibility labels so current tests still find widgets ("Material Size" etc.).
- `section_header` matching the inspector's Properties/Layers headers.
- `step_card(number, title, state)` for Set/Load/Go: white card, 1px border, number in a 24px circle (accent when current, `TEXT_MUTED` outline when upcoming, a check when done), title semibold 14 in `TEXT_PRIMARY` (muted when upcoming).
- `notice(kind, text)` for the Important warning: a tokenised warning style (add one `WARNING` text/background pair to `tokens.rs` if none exists).

Install a flow-wide `egui::Visuals` override (selection colour, widget rounding, stroke) from tokens so any remaining default widget also stops showing egui blue.

### Screens

- **Top bar.** All flow screens use the same 40px top bar as the canvas (Kerf pill and menus, `TOP_BAR_H`, `TRAFFIC_LIGHT_PAD` on macOS) with the step name as a breadcrumb ("Canvas > Prepare"), not a separate plain-text header.
- **Prepare.** Sidebar in the inspector style (240px `INSPECTOR_W`, section headers, 1px dividers). Mat preview on `BG_CANVAS_VOID` with the mat drawn like the canvas artboard (white or mat-grey surface, `ARTBOARD_SHADOW`, `GRID_MAJOR`/`GRID_MINOR`). Footer with Cancel as `secondary_button` and Continue as `primary_button`, aligned to the right of a 1px top border, with 16px padding.
- **Payload panel.** Same panel chrome as the inspector; monospace JSON on `BG_CANVAS_VOID`; section headers via `section_header`.
- **Machine modal.** Token colours, `menu_frame`-style card, primary/secondary buttons, rows like Layers rows with `ACCENT_HOVER` selection.
- **Set, Load, Go, Cutting, Done.** `step_card`s, `segmented` for pressure, `primary_button` for the main action of each step (Next, Load/Unload, Go, Looks good, Unload mat) and `secondary_button` for the rest (Pause, Cancel, Cut more). The checklist uses an accent check when done. The cutting progress bar uses `ACCENT_PRIMARY` on `BORDER_SUBTLE`.

Behaviour, labels and the bridge traffic do not change. Existing flow tests (including the full click-through in `ui::tests`) must pass with no assertion changed.

## Files

- **Owns.** `src/ui/flow_kit.rs` (new), `src/ui/prepare.rs`, `src/ui/set_load_go.rs`, `src/ui/machine_modal.rs`, `src/ui/tests_spec16.rs` (new).
- **Shared edits.** `src/ui/tokens.rs` (only new tokens if needed), `src/ui/mod.rs` (top bar during the flow, payload panel chrome, `mod` lines), `src/ui/chrome.rs` (breadcrumb in the top bar), `src/ui/project_preview.rs` (only if its card needs restyling to match).

## Acceptance

- [ ] A test scans `prepare.rs`, `set_load_go.rs`, `machine_modal.rs` and `flow_kit.rs` and fails on any `Color32::from_rgb`/`from_gray`/hex literal or default `egui::Button::new`/`ComboBox` use (colours must come from `tokens.rs`, widgets from `flow_kit`).
- [ ] Kittest: on Prepare, Continue renders with `ACCENT_PRIMARY` fill (sample the snapshot pixel at the button centre) and Cancel with a white fill.
- [ ] Kittest: on Set, selecting More shows the selected segment in `ACCENT_HOVER`, not egui's default selection blue (sample pixels).
- [ ] The full Make flow click-through test passes unchanged.
- [ ] Snapshots `16-prepare.png`, `16-machine.png`, `16-set.png`, `16-load.png`, `16-go.png`, `16-cutting.png`, `16-done.png`, each looked at next to `canvas.png` for consistency.
- [ ] README Screens section notes that the Make flow shares the canvas identity.
