# 22 Engineering foundation

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec22.rs` pass.

**Wave 5. Depends on everything merged (00-20). Size XL; land in the order below.** Source: `docs/audit/2026-10-12-audit.md`. Each item names its AUDIT finding, which holds the evidence, locations and validation; read the finding before working on its item.

## Goal

Make Kerf safe to use and safe to change: no lost work, no crash on bad input, a green and deterministic test suite enforced by CI, real snapshot regression checks, and a tree that can be published under an open licence.

## Items, in landing order

1. **Deterministic tests (AUDIT-008).** Make the system font catalogue injectable (a `FontCatalogue` value passed in, with `Ready { generation }` / `Scanning` state) instead of the process-global `OnceLock`/`Once` in `system_fonts.rs`; batch font-preview repaints so a face load never repaints per face. Kittests get a fixed catalogue. Expose the catalogue generation so spec 21 can key its cache on it, and make Make wait for (or warn about) a pending scan when the design uses a system font. Acceptance: `cargo test` passes 30 consecutive runs.
2. **Shared kittest harness (AUDIT-022).** Move the harness and helpers out of `ui/mod.rs` into `src/ui/test_harness.rs`; delete the copies in spec test files; replace sleeps and wall-clock waits with step-based waits.
3. **Snapshot comparison (AUDIT-011).** Commit baseline PNGs under `tests/snapshots/` (small, reviewed) and compare with a per-pixel tolerance using `egui_kittest`'s snapshot support; `UPDATE_SNAPSHOTS=1` regenerates. Give the six assertion-free tests real assertions.
4. **Persistence (AUDIT-001, 002, 017, 019).**
   - A dirty flag (design differs from last saved) and a confirm-before-discard dialog at New, Home Edit, Home Make and window close (Save, Don't save, Cancel).
   - Save writes atomically (temp file plus rename), keeps the project on failure, and shows a visible, dismissable error banner; `project::list` shows unreadable files as "Couldn't open" cards instead of hiding them.
   - A `format_version` in project JSON with a tested upgrade path.
   - Builtin shapes persist by stable name, not index; old index-based projects upgrade.
   - Thumbnails refresh in-session (cache-bust the URI) and are not re-read from disk every frame; an emptied design removes its stale thumbnail.
5. **Input bounds (AUDIT-003, 009).** `flatten_cubic` is total (depth and point caps, rejects non-finite); SVG import and project load go through one `Design::sanitise` that rejects or clamps non-finite and absurd coordinates (same 252 in limit as the inspector) and reports what it dropped. Project ids come from file stems; ids in JSON are ignored for paths. Include the malformed inputs from the audit as regression fixtures.
6. **Licence and assets: dropped (personal use only, user decision 2026-10-12).** Was: Delete `assets/ds/shapes/previews/`. Replace the 22 compiled-in copied icons with an openly licensed set (Lucide, ISC, or Phosphor, MIT) or original drawings, keeping each icon's role and size; delete the other 94 copied icon files. Add `LICENSE` (MIT OR Apache-2.0 unless the user says otherwise) and an `assets/PROVENANCE.md` stating the source and licence of every asset directory. Fix the "Shape Studio" font note. Do not delete `docs/specs/reference/` screenshots or `docs/` evidence files: the user decides those.
7. **CI and tooling (AUDIT-012, 025).** `rust-toolchain.toml` pinning the toolchain with `rustfmt` and `clippy`; `rust-version` in Cargo.toml; `rustfmt.toml`; a `[lints]` table matching current clippy cleanliness; `.github/workflows/ci.yml` on `macos-latest` running fmt check, clippy with `-D warnings`, tests, and `cargo-deny` (licences and advisories) with a `deny.toml`. Run `cargo fmt` once in its own commit. Note the `block` warning is dev-only (wgpu via kittest) and record it in `deny.toml` or a README note.
8. **README Tests section (AUDIT-016 part).** Describe how tests are organised, the harness, snapshots and how to update them.

## Files

- **Owns.** `src/system_fonts.rs`, `src/ui/font_preview.rs`, `src/ui/test_harness.rs` (new), `src/project.rs`, `src/geometry.rs` (`flatten_cubic`), `src/ui/home.rs`, `src/ui/icons.rs`, `assets/`, `tests/snapshots/`, `.github/`, `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`, `LICENSE`.
- **Shared edits.** `src/ui/mod.rs` (dirty flag, dialogs, error banner, harness move), `src/design.rs` (`sanitise`, `format_version`, builtin names), `src/shape_library.rs` (import bounds, builtin names), every `src/ui/tests_spec*.rs` (harness imports). Spec 23 runs in parallel and also edits `ui/mod.rs`; keep your edits to persistence and harness code.

## Acceptance

- [ ] 30 consecutive `cargo test` runs pass.
- [ ] Each AUDIT finding listed above meets its own Validation section.
- [ ] Kittests: New with unsaved changes shows the dialog; Cancel keeps the design; Don't save discards; Save then New succeeds.
- [ ] Unit test: a save to an unwritable directory leaves the old file intact, keeps the project id, and surfaces an error.
- [ ] Unit tests: the audit's malformed SVG and project fixtures load or are rejected without panic, hang or unbounded allocation (assert point counts).
- [ ] Snapshot comparisons run in `cargo test` and fail on a deliberate one-pixel-region change.
- [ ] CI workflow passes on a branch push (or `act`-free: the workflow file is valid and every command in it passes locally).
- [ ] No file under `assets/` lacks a provenance entry; no copied icon is compiled in.
