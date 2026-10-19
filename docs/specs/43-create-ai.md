# 43 Create AI: generate cut-ready designs from a prompt

**Status: Implemented.** Checked against `main` on 2026-10-19: the feature is in the app and its tests in `src/ui/tests_spec43.rs` pass.

**Wave 9. Depends on 22, 23, 25, 28. Size XL.** Requested directly by the user.

## Background

AI design generators let a user type a prompt, such as a dinosaur with a unicorn horn riding a skateboard, and return a single-layer, black and white image that is cut-ready and easy to weed. They save generated images for reuse, block trademarked, licensed and inappropriate content, and typically run as an online subscription service whose images are for personal use only.

## What Kerf does differently

Kerf is offline and has no account, so it cannot host a model. The user brings their own Anthropic API key. Claude writes the design directly as SVG vector paths, which Kerf validates and imports. A cut-ready vector is what such a raster image becomes after tracing anyway, so Kerf skips the raster step.

Kerf sends only the prompt and the chosen style. It never sends the design, project names or files.

## User flow

1. A **Create AI** item on the rail opens the Create AI panel. Its tooltip reads "Create AI: describe a design and Kerf draws it for cutting."
2. With no key set, the panel shows one sentence on what is sent and to whom, a key field and a link to the Anthropic console. Saving the key stores it in the OS keychain through the `keyring` crate. Kerf never writes the key to `settings.json`, a project or the debug log. `ANTHROPIC_API_KEY` in the environment takes precedence, and the panel says when it is in use.
3. With a key, the panel shows:
   - a prompt box (up to 400 characters);
   - **Style** chips: Silhouette (solid, one piece), Line art (outlines with a minimum stroke width), Stencil (bridged islands, for stencil film), Sticker (solid with an offset border);
   - **Detail**: Simple, Medium, Detailed. This sets the minimum feature size and the path budget below;
   - **Generate**, which asks for four candidates.
4. While a request runs, the panel shows four placeholder tiles and a Cancel button. The UI never blocks.
5. Results appear as four tiles. Clicking one selects it. **Insert** adds it at the centre of the view at 4 in wide, as one Basic Cut shape in black, named from the prompt (for example "Dinosaur skateboard"), as one undo step. **More like this** generates four variations of the selected tile. **Regenerate** runs the same prompt again.
6. **History**, a tab in the panel, lists past generations with their prompt and thumbnail. They are stored in the projects directory under `ai/`, so a design can be reused offline.

## The request

`src/ai/anthropic.rs` calls the Messages API over raw HTTPS (`POST https://api.anthropic.com/v1/messages`, headers `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`), because Rust has no official Anthropic SDK. Use `ureq` with rustls on a worker thread, in the same shape as `nesting/worker.rs`. Results return over a channel, and Cancel drops the request.

- **Model.** `claude-opus-5-5` by default, changeable in Settings. Do not hard-code the model anywhere else.
- **Output shape.** Use structured outputs (`output_config.format` with a JSON schema): `{ "candidates": [ { "title": string, "svg": string } ] }` with exactly four items. Do not use forced `tool_choice`, which current models reject.
- **Thinking and effort.** Use adaptive thinking and `output_config.effort: "medium"`, and stream the response so long outputs do not hit HTTP timeouts. Set the client timeout to 120 s.
- **Refusals.** Check `stop_reason` before reading `content`. On `"refusal"`, show the `stop_details.explanation` in the panel ("Kerf couldn't make this: ...") and keep the prompt so the user can rephrase. Enable the server-side fallback (`anthropic-beta: server-side-fallback-2026-07-01`, `fallbacks: "default"`).
- **Errors.** 401 shows "Your API key was rejected." 429 and 529 show a retry message with the `retry-after` time. Network failures show "Kerf couldn't reach Anthropic." None of these clear the prompt.
- **System prompt.** It lives in `src/ai/prompt.md`, included at build time. It asks for a single-colour SVG design for a cutting machine: one `viewBox`, filled closed paths only, no strokes, text, images, gradients or transforms; every island at least the detail's minimum size at 4 in wide; at most the detail's path budget. It also says to decline trademarked characters, logos and brands, real people, and anything unsuitable for a family craft project. Before writing the prompt, read the current Claude API docs for prompt guidance on the chosen model, and verify the request shape against them.

The implementing agent must check the exact request and response fields above against the live API docs before coding, because they change faster than this spec.

## Validation

Model output is untrusted input. `src/ai/validate.rs` is pure, and every candidate passes through it before a tile shows:

1. Parse with `shape_library::import_svg`, then run `Design::sanitise`.
2. Reject anything left with `<text>`, `<image>`, scripts or external references.
3. Union all fills into one compound path, scaled to 4 in wide.
4. Weedability: drop islands smaller than the detail's minimum size, and reject the candidate if that removed more than 5 % of its area. Reject it if its path count or node count is over budget.
5. For Stencil, check that every hole is bridged to the outside.

When a candidate fails, Kerf asks once more for that candidate in a follow-up message that quotes the validator's reasons. If it fails again, its tile shows "This one didn't come out cut-ready" and a reason. The other tiles are unaffected.

## Model

- `src/ai/mod.rs` holds `Provider`, a trait with one method: `generate(&GenerateRequest) -> Result<Vec<Candidate>, AiError>`. `AnthropicProvider` implements it. Tests use `FakeProvider`, which returns canned SVGs from `src/ai/fixtures/`. No test touches the network.
- `GenerateRequest { prompt, style, detail, variations_of: Option<Candidate> }`. `Candidate { title, svg, piece: Option<Shape>, problem: Option<String> }`.
- `Edit::Insert(Insert::Generated { title, outline })` adds the shape. `Design::check` blocks an empty outline.
- History entries are `{ id, prompt, style, detail, created, svg }`, written through `store/` with its atomic writes.

## Files

- **Owns.** `src/ai/` (mod, `anthropic.rs`, `validate.rs`, `history.rs`, `prompt.md`, `fixtures/`), `src/ui/create_ai_panel.rs`, `src/ui/tests_spec43.rs`.
- **Shared edits.** `Cargo.toml` (`ureq` with rustls, `keyring`), `src/edit.rs` (one `Insert` variant), `src/settings.rs` (model name), `src/ui/chrome.rs` (one rail item), `src/ui/mod.rs` (open the panel), `src/debug_log.rs` (redact the key), `deny.toml` if the new crates need a licence entry, `docs/guide.md` (a Create AI section), one Features line in `README.md`.

## Acceptance

- [ ] Unit tests in `validate.rs`: a clean silhouette passes; text, an image or a stroke-only path is rejected; tiny islands are removed and an over-budget path count is rejected with its reason; a Stencil design with an unbridged hole is rejected.
- [ ] Kittest with `FakeProvider`: type a prompt, Generate, see four tiles, Insert one. The design gains one shape named from the prompt, and one Undo removes it.
- [ ] Kittest: a fake refusal shows its explanation and keeps the prompt. A fake 401 shows the key message.
- [ ] Kittest: Cancel during a request returns the panel to idle, and a late result is dropped.
- [ ] The key never appears in `settings.json`, the debug log or any project file. A test writes a key, runs a fake request with logging on, and greps the log and the files.
- [ ] History survives a restart and inserts offline.
- [ ] One live run with a real key on a release build: paste the prompt, the four tiles' snapshot and the time it took into the PR.
- [ ] Snapshots: `target/snapshots/43-no-key.png`, `43-results.png` and `43-refusal.png`.
