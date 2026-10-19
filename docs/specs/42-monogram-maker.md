# 42 Monogram Maker

**Wave 9. Depends on 08, 10, 12, 15, 18, 21, 29, 31. Size L.** Requested directly by the user.

## What Monogram Maker does

Monogram Maker is a tool built into the Kerf canvas. It combines fonts, frame images and layout styles into hundreds of personalised monograms, in themes from classic to sports and botanicals. The result can be cut, drawn or printed then cut. Users may type any character, though not every font has every glyph.

This spec sets how many initials it takes, what the layouts are and what the output layers look like, all with Kerf's own choices.

Kerf has no paywall. Everyone can make, cut and save a monogram.

## User flow

1. A **Monogram** item on the rail, below Shapes, opens the Monogram panel in the same slot as the Shapes panel. Its tooltip reads "Monogram: build initials inside a frame."
2. The panel, top to bottom:
   - **Letters.** One field, 1 to 3 characters, any Unicode. The help line under it changes with the style, for example "First, Last, Middle" for Classic three.
   - **Theme** tiles: Classic, Modern, Script, Sports, Botanical. A theme is a preset of style, frame and fonts. Picking a theme sets those three. Changing any of them afterwards keeps the rest and shows the theme as "Custom".
   - **Style** tiles, each drawn with the current letters:

     | Style | Letters | Layout |
     |---|---|---|
     | Single | 1 | One large initial. |
     | Classic three | 3 | First and middle letters at 70 % beside a larger centre letter. The typed order is First, Last, Middle, as in traditional monograms, and the help line says so. |
     | Even three | 3 | Three letters at one size. |
     | Stacked | 2 to 3 | Letters in a column. |
     | Interlocking | 2 to 3 | Letters overlap by a set fraction of their width, ready to weld. |
     | Circle | 3 | Small first and middle letters on an arc around the centre letter, using spec 12 curved text. |

   - **Frame** tiles: None, Circle, Double circle, Square, Diamond, Scalloped circle, Laurel, Shield, Banner, Leaf wreath. Frames are original artwork drawn for Kerf, stored as SVG in `assets/monogram/frames/`.
   - **Fonts.** One picker per role (centre, sides). Bundled, system and writing fonts all appear, through the existing `FontCatalogue`. A font that lacks a typed character shows the fallback glyph and a notice: "Script Serif has no 'Ł'. Kerf used Inter for it."
   - **Adjust.** Letter spacing, centre size, frame padding and frame thickness, each a slider with a typed field through `units.rs`.
   - **Output.** "Layered" keeps the frame and letters as separate shapes. "Knockout" cuts the letters out of a solid frame. "Welded" joins letters and frame into one outline. All three are live boolean groups from spec 21, so they stay editable.
   - **Operation.** Basic Cut, Pen (switches every font to its writing font when one exists) or Print Then Cut.
3. A large live preview sits above the controls and redraws on every change, without touching the design.
4. **Insert** adds the monogram at the centre of the view as one undo step, and selects it.

## Editing it later

The inserted object is a Group named `Monogram ABC`. It stores its `MonogramSpec` (letters, theme, style, frame, fonts, adjustments, output, operation) in a new `#[serde(default)] monogram: Option<MonogramSpec>` field.

- Double-clicking the group, or choosing **Edit monogram** from its context menu or the command palette, reopens the panel with that spec. **Update** replaces the group's children in place, keeps its position, rotation and size, and is one undo step.
- Ungrouping, or editing a child directly, clears `monogram`. From then on the shapes are plain shapes, and Edit monogram is disabled with the reason "This group was edited by hand."
- Monograms can become components (spec 31) like any group.

## Model

- `src/monogram.rs` is pure. `layout(spec: &MonogramSpec, fonts: &FontCatalogue) -> MonogramLayout` returns the children as `Shape`s in inches, plus any missing-glyph notices. Text stays as live text objects, so it can still be edited, curved or converted to paths.
- `Edit::Insert(Insert::Monogram(MonogramSpec))` inserts a monogram. `Edit::Monogram { id, spec }` updates one. Both go through `Design::check`. `check` blocks an empty letters field with "Type at least one letter."
- Themes are data: `assets/monogram/themes.json` maps each theme to a style, a frame and font names. A missing font falls back to Inter and logs once.

## Bundled fonts

Classic and Script need serif and script faces that every platform has. Bundle three SIL OFL families and record each one's source and licence in `assets/PROVENANCE.md`: a high-contrast serif (for example Playfair Display), a formal script (for example Great Vibes) and a condensed athletic face (for example Oswald). Keep only the weights the themes use.

## Files

- **Owns.** `src/monogram.rs`, `src/ui/monogram_panel.rs`, `src/ui/tests_spec42.rs`, `assets/monogram/` (frames, `themes.json`), the new fonts under `assets/fonts/`.
- **Shared edits.** `src/edit.rs` (two variants and their check), `src/design.rs` (the `monogram` field), `src/ui/chrome.rs` (one rail item and `RailEvent::Monogram`), `src/ui/commands.rs` (Edit monogram), `src/ui/context_menu.rs` (one row), `src/ui/mod.rs` (open the panel), `assets/PROVENANCE.md`, `docs/guide.md` (a Monograms section under Editor), one Features line in `README.md`.

## Acceptance

- [ ] Unit tests in `monogram.rs`: each style gives the expected number of children and relative sizes. Classic three puts the second typed letter in the centre at full size. Interlocking letters overlap. Every layout fits inside its frame plus padding.
- [ ] A missing glyph produces the fallback and a notice, not a panic or a missing letter.
- [ ] Kittest: open the panel from the rail, type `ABC`, pick Botanical, Insert. The design gains one group named `Monogram ABC` with a frame and three text children, and one Undo removes it.
- [ ] Kittest: double-click the monogram, change the style to Circle, Update. The group keeps its id, position and rotation, and one Undo restores the old children.
- [ ] Kittest: ungroup the monogram. Edit monogram is then disabled with its reason.
- [ ] Welded output composes to one cut path per mat in `compose`, and Pen output uses writing fonts where the font has one.
- [ ] A project saved before this spec still loads, and a project with a monogram round-trips through save and load with its spec intact.
- [ ] Snapshots: `target/snapshots/42-panel.png` (the panel with a preview) and `42-styles.png` (every style with the Circle frame).
