# History Replay Preview Report

Date: 2026-03-19

## Why this batch

After finishing file-backed custom vocabulary, the next gaps were:

- selected-text capture
- history re-run / enhancement replay

Selected-text capture is still valuable, but it is platform-specific and cannot be fully
end-to-end validated from this Linux host without a real macOS/Windows runtime lane. History replay
preview is the smaller fully verifiable batch that directly improves prompt iteration and UX now.

## Goal

Let users take an existing history row and run the LLM enhancement path again without dictating
again or inserting text.

This should feel like:

- recover transcript from history
- reuse the saved prompt when possible
- show a fresh preview inline
- do not write new history or insert anything

## Implemented behavior

### Backend

Added `preview_history_entry` Tauri command.

Replay resolution behavior:

1. load the history entry by stable id
2. choose transcript source in this order:
   - `raw_transcript`
   - `text`
   - `enhanced_text`
3. choose prompt in this order:
   - exact `prompt_id` match in current config
   - exact `prompt_title` match in current config
   - current default prompt
   - first prompt in the catalog
4. reconstruct app identity from saved history metadata
5. reconstruct window context from saved app/window metadata
6. run the normal preview path against the current provider configuration

### Important replay boundary

History replay is intentionally **not** a byte-for-byte time machine.

It reuses:

- saved transcript
- saved prompt when available
- saved app/window identity
- current file-backed custom vocabulary

It does **not** replay ephemeral text context that was never persisted:

- clipboard text
- selected text

The UI states this explicitly.

### Appcore support

Added `AppService::preview_prompt_with_app_snapshot(...)` so replay flows can use a synthetic
app/context source instead of only the live foreground app.

This also gives us a clean seam for future flows that need deterministic or saved context.

### Frontend

History rows now expose `Enhance Again`.

On click, VoiceWin:

- calls `preview_history_entry`
- renders the replayed output inline
- shows provider/model + latency
- surfaces replay errors inline when needed

## Tests added

### Rust

- `preview_prompt_with_app_snapshot_uses_supplied_app_and_context`

### Frontend

- History page replay button invokes backend replay command
- inline replay preview is rendered with latency and explanatory note

## Verification

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-appcore`
- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`
- `cargo check -q && cargo test -q` in `voicewin-tauri/src-tauri`
- `npm test` in `voicewin-tauri`
- `npm run typecheck` in `voicewin-tauri`

## Live validation

Added reusable example:

- `voicewin-appcore/examples/live_replay_preview.rs`

Live run against the provided gateway:

- provider: OpenAI-compatible Responses SSE
- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- reasoning effort: `low`

Input:

- transcript: `please ship the voice win update using eleven labs scribe v2 today`
- app: `code`
- window: `VoiceWin notes`
- custom vocabulary:
  - `VoiceWin`
  - `ElevenLabs`
  - `Scribe v2`

Observed:

- total preview latency: `1974 ms`
- first token latency: `1633 ms`
- system prompt included reconstructed window context
- system prompt included file-backed custom vocabulary
- final output:
  - `Please ship the VoiceWin update using ElevenLabs Scribe v2 today.`

## Latency takeaways

Replay preview adds no meaningful local cost beyond:

- history row lookup
- prompt resolution
- synthetic app/window reconstruction

Those costs are negligible relative to remote inference latency. The replay path remains dominated by
the provider round-trip.

## Files touched

- `voicewin-appcore/src/service.rs`
- `voicewin-appcore/examples/live_replay_preview.rs`
- `voicewin-tauri/src-tauri/src/main.rs`
- `voicewin-tauri/src/ui/HistoryPage.tsx`
- `voicewin-tauri/src/ui/HistoryPage.test.tsx`

## Conclusion

History is now more than a dead log:

- users can recover text
- inspect prompt/provider metadata
- and immediately re-run enhancement from a saved row

This materially improves prompt iteration and makes the LLM post-processing surface feel more
complete without adding risky platform-specific capture work in the same batch.
