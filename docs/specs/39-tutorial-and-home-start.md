# 39 Interactive tutorial, and starting on Home

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec39.rs` pass.

**Wave 8. Depends on everything merged. Size L.** Requested directly by the user: "a tutorial that runs through everything in an interactive form (with popups/tooltips, that also darken the screen and focus themselves); tutorial should be run only when asked for in the menu. Also when loading, starting in the home screen rather than the canvas."

## Part A: start on Home

- Every launch opens on Home. Session restore (spec 33) still reopens the previous tabs in order, but in the tab strip, not on screen; Home is shown first and the tab strip is visible (Home always shows it). Clicking a tab or a card goes to the canvas.
- Crash recovery (spec 33) still offers its dialog at launch, over Home.
- `kerf --open <file>` and `kerf --bench` (spec 38) still go straight to the canvas.
- Kittests that assume the app starts on the canvas get an explicit "open a blank tab" step in the harness (one helper, not per-test edits where avoidable). The user-visible default changes; the harness keeps tests readable.

## Part B: interactive tutorial

### Entry and exit

- **Help** menu (new, in the top bar after View) with "Interactive tutorial", "Keyboard shortcuts" (opens the command palette filtered to commands with shortcuts) and "About Kerf". Also a palette command "Start tutorial".
- Never starts on its own. No first-run prompt.
- Esc, a Close button on every step, or "Skip tutorial" ends it at once and restores everything. Back and Next step through it; a progress line "Step 7 of 32" and a section name.
- The tutorial runs in its own tab, "Tutorial", with a sample design it builds (a card with a heart, text and a star). It never touches the user's other tabs or saved projects; closing the tutorial closes that tab without saving or autosaving it (the tab is marked transient).

### Spotlight overlay

- A full-window dim layer (black at about 60 % alpha, a token in `tokens.rs`) drawn above all panels, with a rounded cut-out (8 px padding, 8 px radius) around the step's target, a 2 px `ACCENT_PRIMARY` ring, and a short fade (150 ms) between steps.
- A popover card beside the target (spec 01 `menu_frame` style, 320 px wide) with an arrow toward the target, auto-placed to stay on screen (right, left, below, above, in that order), holding the title, one or two sentences, the shortcut where relevant, and the buttons.
- Clicks inside the cut-out reach the app (the user can do the step); clicks on the dim area are swallowed. Keyboard focus stays in the app so shortcuts work during "do it" steps.
- Steps with no single target (an overview) show a centred card with no cut-out.

### Targets

- A small anchor registry: `ui::anchors::mark(ui, Anchor::X, rect)` records a widget's rect for the frame; the tutorial looks targets up there. Add one `mark` line at each target (rail buttons, Shapes panel, toolbar groups, inspector sections, Layers, Components tab, tab strip, top bar menus and buttons, Make flow controls). No per-step coordinates.
- If a target is not on screen this frame (panel closed, wrong screen), the step's `prepare` action opens it (for example opens the Shapes panel or selects the heart) through `edit::run`/`AppCommand`, never by synthetic input.

### Steps

Data-driven: `tutorial::STEPS: &[Step { section, anchor, title, body, prepare, advance }]` where `advance` is `Next` (button) or `When(predicate)` that watches app state (for example "a shape was added", "selection is the heart", "Make flow is on Prepare"). "Do it" steps also show "Show me" which performs the action for the user through the command path.

Cover everything, in this order (about 30 to 40 steps; each one or two sentences):
1. Home: projects, New, version history entry point, tabs.
2. Canvas basics: rail tools (Select, Pen, Pencil, Text, Measure), Shapes panel and its search and filter, Upload (images and SVG fonts), zoom, rulers, snapping.
3. Selecting and editing: selection frame, size badge, action bar, right-click menu, typed maths in fields (`*2`, `2in + 5mm`), locks.
4. Operations: the operation dropdown and how each draws (Cut, Pen, Score, Print Then Cut, Guide), the legend, Show as made.
5. Text: font menu (system and writing fonts), Format, curve, edit in place, split into letters.
6. Arrange: Align and Distribute, groups and attach, containers and auto layout.
7. Effects: Offset and Inset, Sticker, Warp, the Layers effect chips and list, Expand.
8. Combine: live Weld/Subtract/Slice groups, editing a child, the op dropdown.
9. Nodes: double-click a path, node types, add and remove, smooth.
10. Components: Make component, the Components tab, editing the master, overrides, Detach.
11. Images and patterns: trace panel overview, pattern fill on Print Then Cut.
12. Command palette (Cmd+K).
13. Make: Prepare (mats, paper size, Optimise layout), cut preview, the machine picker (the Kerf simulator and open cutters), Set/Load/Go, Print Then Cut's print step.
14. Export and prepare for other editors.
15. Saving: autosave, version history, crash recovery, tabs.
16. Finish: "You can rerun this from Help > Interactive tutorial."

### Logging

Tutorial start, step changes, skips and finish log at a `tutorial` target in the debug log.

## Files

- **Owns.** `src/ui/tutorial.rs` (steps, state machine), `src/ui/tutorial_overlay.rs` (dim layer, cut-out, popover), `src/ui/anchors.rs`, `src/ui/tests_spec39.rs`.
- **Shared edits.** `src/ui/mod.rs` (start on Home; mount the overlay last; transient tab), `src/ui/chrome.rs` (Help menu), `src/edit.rs` and `src/ui/commands.rs` (AppCommand::Tutorial, ::Shortcuts, ::About; registry), one `anchors::mark` line in each target widget's file, `src/ui/tokens.rs` (dim colour), `src/ui/test_harness.rs` (blank-tab helper), README.

## Acceptance

- [ ] Kittest: a fresh launch shows Home; with a saved session the tabs are in the strip and Home is still shown; `--open` goes to the canvas.
- [ ] Kittest: the tutorial does not start on launch; Help > Interactive tutorial opens it in a Tutorial tab; Esc ends it and the tab is gone; no project file or autosave was written for it.
- [ ] Unit test: every step's anchor is marked by some widget (a test renders each step's screen and asserts the anchor rect exists), and every `When` predicate becomes true after its "Show me" action.
- [ ] Kittest: on the "add a shape" step, clicking inside the cut-out on the Shapes panel adds a shape and advances; clicking on the dim area does nothing.
- [ ] Kittest: walking the whole tutorial with Next and Show me reaches the finish step with no panic, and Back from step 10 returns to step 9 with its prepare re-run.
- [ ] Pixel test: on a spotlight step, a pixel far from the target is darkened by the dim token and a pixel inside the cut-out is not.
- [ ] Snapshots `39-home-start.png`, `39-tutorial-rail.png`, `39-tutorial-inspector.png`, `39-tutorial-make.png`.
- [ ] README describes starting on Home and the Help menu.
