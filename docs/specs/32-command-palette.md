# 32 Command palette (Cmd+K)

**Wave 6. Depends on 22, 23, 14, 15. Size M.** Kerf hides actions in toolbars that change with the selection, and has no keyboard route to most of them. A command palette makes every action one keystroke and a few letters away.

## Problem

Commands are spread across the File, Edit and View menus (`src/ui/chrome.rs`), the edit toolbar (`src/ui/toolbar/`), the selection action bar, the Layers action bar, the context menu table (`src/ui/context_menu.rs`), the Effects menu, the Shapes panel and the Font menu. Each surface has its own enable rule (`context_menu::Action::enabled`, `combine::Pick::enabled`, `layers_actions` and so on). A palette written against those surfaces would be one more copy of every rule.

## Goal

Cmd+K opens a palette that finds and runs anything, by keyboard, using spec 23's single command model for both the list and the action.

### Sits on spec 23

Spec 23 gives every user action one `Command` definition: an id, a label, a shortcut, an `enabled(ctx) -> Result<(), Reason>` rule and one dispatch path that applies before `History::observe`. The palette:
- lists commands from that registry; it defines no command of its own except "Open palette";
- asks the same `enabled` rule, so a command greyed in the toolbar is greyed here with the same reason;
- runs a pick by pushing the command onto the same queue the menus use. A palette run is therefore one undo step and behaves exactly like the menu item.

If a needed action is not yet in the registry, the fix is to register it in spec 23's model, not to special-case it here.

### Sources

| Section | Entries | Running one |
|---|---|---|
| Commands | every registered command: menu items, toolbar and action-bar actions, Combine, Align, Arrange, Flip, effects (Offset, Sticker, Warp, Curve), view toggles | runs it |
| Shapes | every Shapes panel shape by name and category (spec 14 data) | inserts it at the view centre, selected |
| Fonts | Kerf, Writing (spec 29) and system fonts (spec 15) | with text selected, sets the font; otherwise places a text in that font |
| Recent projects | the 10 most recently opened projects | opens it (in a tab after spec 33) |

Shapes, fonts and projects are parameterised commands in spec 23's model (for example `InsertShape(kind)`, `SetFont(FontRef)`, `OpenProject(id)`), so the palette still has one run path.

### Matching and ranking

- Fuzzy subsequence matching over the label and a few aliases per command (for example "weld" also matches "unite", "delete" matches "remove", "colour" matches "color"). Scoring rewards consecutive matches, word starts and a match at the start of the label. Case and diacritics are ignored.
- Write the matcher in `src/palette/fuzzy.rs` (around 100 lines, pure, unit tested) rather than add a crate: it is small, deterministic and has no licence question. If it proves too slow on the system font list, `fuzzy-matcher` (MIT) is the fallback.
- With an empty query the palette shows **Recent** (the last 8 commands run, most recent first), then **Suggested** (enabled commands for the current selection, for example Weld, Attach and Align with two shapes selected).
- With a query, recently used entries get a ranking boost that decays with age, so a frequently used command rises without hiding a better match.
- Recent entries persist in `settings.json` (`palette_recent`, capped at 50, `#[serde(default)]`).

### Interaction

- Cmd+K opens it centred near the top of the canvas, over everything, with the search field focused. Cmd+K again, Escape or a click outside closes it.
- Up and Down move, Enter runs, Cmd+Enter runs and keeps the palette open (useful for nudges and toggles). Page Up and Page Down jump by section. Typing never needs the mouse.
- Each row: icon (from `icons.rs`), label, section, and the shortcut right-aligned in muted text, read from the command's definition. Disabled rows stay visible in muted text with the reason as a second line, so the palette teaches why something cannot run. Enter on a disabled row does nothing and flashes the reason.
- Results are capped at 50 rendered rows; the list scrolls.
- Every row has a `widget_info` label equal to its command label.
- The palette works on Home (projects, New, Settings) as well as the canvas; commands that need the canvas are hidden there, not greyed.

## Files

- **Owns.** `src/palette/mod.rs` (new: sources, ranking, recents; pure), `src/palette/fuzzy.rs` (new), `src/ui/palette.rs` (new: overlay, keyboard handling), `src/ui/tests_spec32.rs` (new).
- **Shared edits.** spec 23's command registry (aliases field, parameterised commands for shapes, fonts and projects if 23 did not add them), `src/settings.rs` (`palette_recent`), `src/ui/chrome.rs` (Cmd+K shortcut and a "Command palette" item in the View menu), `src/ui/mod.rs` (`mod` lines and the overlay mount).

## Acceptance

- [ ] Unit test: "wld" ranks Weld first; "al c" ranks Align Centre above Align Left; "colour" finds the colour command.
- [ ] Unit test: a command used five times today outranks an equally good match never used; an exact prefix match still beats a weak match used often.
- [ ] Unit test: every registered command appears in the palette's source list (walks spec 23's registry), so a new command can never be missing.
- [ ] Unit test: the palette's enabled state for every command equals the registry's `enabled` for the same context (the palette has no enable logic of its own).
- [ ] Unit test: recents persist through `settings.json` in a tempdir and cap at 50.
- [ ] Kittest: select two shapes, Cmd+K, type "weld", Enter; the shapes weld and one Undo restores them.
- [ ] Kittest: with one shape selected, Cmd+K, type "weld"; the row is disabled and shows the reason the toolbar tooltip gives.
- [ ] Kittest: Cmd+K, type "heart", Enter; a heart is inserted and selected.
- [ ] Kittest: select a text, Cmd+K, type a font name, Enter; the text's font changes.
- [ ] Kittest: Cmd+K with an empty query lists the last command run first.
- [ ] Snapshots `32-palette-empty.png` (Recent and Suggested), `32-palette-query.png` (a query with a disabled row and shortcuts).
- [ ] README describes the palette, its sources, keys and ranking.
