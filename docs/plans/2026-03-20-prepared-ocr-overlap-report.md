# 2026-03-20 Prepared OCR Overlap Report

## Goal

Reduce OCR-mode stop latency in the real recording flow.

Before this pass, VoiceWin's prepared-session architecture overlapped:

- app/profile resolution,
- context snapshotting,
- optional screenshot capture,
- optional preflight warmup.

But OCR itself still ran only after the user stopped recording.

That meant OCR mode paid the full OCR sidecar cost on the stop path even when the screenshot had
already been captured at recording start.

## Implementation

### Prepared-session OCR precompute

`voicewin_appcore::AppService::prepare_session_context()` now:

1. resolves effective config,
2. starts preflight as before,
3. captures context/screenshot as before,
4. if the effective visual dispatch is `ocr`, runs the OCR sidecar immediately during the
   prepared-session task,
5. stores the OCR result into the prepared context snapshot.

This means the OCR sidecar can complete while recording is still in progress.

### Snapshot reuse

Added `voicewin_engine::traits::PreparedScreenOcr` and extended `ContextSnapshot` with:

- `precomputed_screen_ocr: Option<PreparedScreenOcr>`

`VoicewinEngine` now reuses that precomputed OCR result when present instead of re-running OCR on
the stop path.

The engine still falls back safely:

- if prepared OCR is absent, it performs the existing stop-time OCR path,
- if prepared OCR failed, the stop path can still retry OCR from the screenshot.

### Additional OCR fallback hardening

The overlap work exposed a remaining post-processing weakness on short recordings:

- GPT-5.4 sometimes returned short transcript-ish variants such as:
  - `Transcribe voice`
  - `“Wen” in voice.`

Those are not valid corrected product names even though OCR text already resolved the answer.

So `voicewin-core/src/enhancement.rs` was tightened again:

- very short verb-led assistant options now fall back to OCR,
- short transcript-permutation outputs now prefer OCR when a concise OCR correction is available.

## Files

- `voicewin-engine/src/traits.rs`
- `voicewin-engine/src/engine.rs`
- `voicewin-engine/tests/engine_integration.rs`
- `voicewin-appcore/src/service.rs`
- `voicewin-core/src/enhancement.rs`

## Tests

Added/updated coverage for:

- engine reuse of precomputed OCR from context snapshots
- prepared-session OCR precompute on text-only main APIs
- no duplicate OCR request on the prepared stop path
- new enhancer regressions for:
  - `transcribe voice`
  - `“Wen” in voice.`

Verified on March 20, 2026:

- `cargo fmt --all`
- `cargo test -q -p voicewin-core`
- `cargo test -q -p voicewin-engine`
- `cargo test -q -p voicewin-appcore`
- `cargo test -q`
- `cargo check -q --examples -p voicewin-appcore`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

All passed.

## Live Validation

Gateway under test:

- base: `https://cc2.caaa.tech/v1`
- provider: `openai_compatible`
- api: `responses_sse`
- model: `gpt-5.4`
- visual mode: `ocr`
- screenshot fixture: `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`
- transcript: `voice wen`
- prompt: `Correct the dictated product name using OCR text when helpful. Return only the corrected product name as plain text. No explanation.`

Tool:

- `voicewin-appcore/examples/live_session_stop_latency.rs`

### A. Baseline: no prepared session, 5s recording window

Config:

- `prepared=false`
- `recording_sleep_ms=5000`
- `rounds=3`

Observed:

- final output: `VoiceWin`
- stop wall min/avg/max: `3191 / 4978 / 6115 ms`
- enhancement min/avg/max: `1528 / 2383 / 3627 ms`
- first-token min/avg/max: `1413 / 1836 / 2198 ms`
- OCR elapsed min/avg/max: `1662 / 2592 / 3628 ms`

One round still needed OCR fallback warning, but all rounds ended with the correct final text.

### B. Prepared OCR overlap: prepared concurrent, 5s recording window

Config:

- `prepared=true`
- `prepared_concurrent=true`
- `recording_sleep_ms=5000`
- `rounds=3`

Observed:

- final output: `VoiceWin`
- stop wall min/avg/max: `1326 / 2711 / 4870 ms`
- enhancement min/avg/max: `1321 / 2709 / 4870 ms`
- first-token min/avg/max: `1177 / 2574 / 4725 ms`
- prepared OCR elapsed min/avg/max: `1988 / 2448 / 3038 ms`

Delta vs baseline:

- stop wall improved by about `2267 ms` on average

Interpretation:

- the OCR sidecar is now materially overlapped with recording time when the recording window is
  long enough,
- this is a real stop-path win, not just a provider-only microbenchmark improvement.

### C. Short-window spot check before final hardening

Config:

- `prepared=true`
- `prepared_concurrent=true`
- `recording_sleep_ms=1000`
- `rounds=2`

Observed before the final post-process fix:

- bad final outputs appeared:
  - `Transcribe voice`
  - `“Wen” in voice.`

Raw history confirmed these came from model outputs, not telemetry bugs.

### D. Short-window spot check after final hardening

Config:

- `prepared=true`
- `prepared_concurrent=true`
- `recording_sleep_ms=1000`
- `rounds=2`

Observed after the fix:

- both `2/2` rounds returned `VoiceWin`
- stop wall min/avg/max: `3260 / 3873 / 4487 ms`
- enhancement min/avg/max: `3221 / 3224 / 3228 ms`
- first-token min/avg/max: `1974 / 2537 / 3101 ms`
- prepared OCR elapsed min/avg/max: `1040 / 1647 / 2255 ms`

Reference baseline for the same 1s window without prepared overlap:

- `prepared=false`
- `rounds=2`
- stop wall min/avg/max: `4954 / 5596 / 6238 ms`

Short-window average delta:

- stop wall improved by about `1723 ms`

## Product Conclusion

This should ship.

Why:

- it improves the real stop path for OCR mode without changing screenshot mode,
- it keeps the existing fallback behavior when prepared OCR is absent,
- it is test-covered,
- it is live-validated on the real gateway.

Current recommendation remains:

- prefer `auto` or `screenshot` for multimodal-capable models when lowest latency matters most
- keep `ocr` available for non-multimodal models and explicit user preference
- rely on prepared-session overlap so OCR mode is no longer guaranteed to pay the full OCR cost
  after stop

## Outcome

VoiceWin's OCR mode is now closer to the intended latency architecture:

- capture and OCR can happen during recording,
- the stop path reuses that work,
- short OCR-correctable product-name outputs are hardened against transcript-ish model drift.
