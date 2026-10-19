# 35 Make flow hardening

**Wave 5. Depends on 09, 16. Size M. Land before spec 21 phase 1.** Source: `docs/audit/2026-10-12-audit.md` AUDIT-010, 014, 020, 027.

## Goal

What reaches the machine is right in every state, and tested.

- **Copies stay on the mat (010).** Project copies flow through composition (spec 09) as real units, packed with the rest, spilling to extra mats when full, never placed past the mat edge or at negative x under Mirror.
- **Bridge tests (014).** Golden JSON payloads for rotated, flipped, mirrored and multi-copy designs with curved paths, compared structurally (parse and compare, not `contains`). Assert every path stays inside the mat's cutting area.
- **State machine (014, 020).** Tests for Cancel in Set, Load, Go and Cutting; Cut more; Rerun after Done and after Cancel; Next Mat after Cancel. Fix Rerun after Cancel in Set (it must resend material and tools and never strand in Load).
- **Transport errors (027).** `Transport::send` returns its error; a dead simulator thread or a remembered machine missing from the device list shows a visible error in the flow instead of waiting forever.

## Files

- **Owns.** `src/make_flow.rs`, `src/bridge.rs` tests, `src/device.rs`, `tests/golden/` (new).
- **Shared edits.** `src/composition.rs` (copies as units), `src/ui/set_load_go.rs` and `src/ui/machine_modal.rs` (error display).

## Acceptance

- [ ] Golden payload tests for the listed cases pass and fail on a one-point transform change.
- [ ] Unit test: 20 copies of a 3 in square on a 12 x 12 mat span several mats and every path lies inside its mat.
- [ ] State-machine tests for every listed transition; the Rerun-after-Cancel case resends `riMaterialSelected` and tools.
- [ ] Kittest: killing the simulator thread mid-flow shows an error with a Back to canvas action.
