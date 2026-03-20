# 2026-03-20 OCR Source Telemetry Report

## Scope

This pass completed the unfinished OCR provenance telemetry work for VoiceWin's LLM post-processing path and then hardened OCR-aware post-processing again after live gateway validation exposed new short-form failure modes.

Goals:

- persist whether screen OCR was produced `inline` on the stop/preview path or reused from `prepared` recording-start overlap
- surface that provenance in History, Prompt Preview, replay benchmarks, and live benchmark examples
- validate the new labels against the real `gpt-5.4` `/v1/responses` SSE path on `https://cc2.caaa.tech/v1`
- keep OCR fallback behavior correct when the gateway emits short generic or question-style miscorrections

## Implementation

### Runtime telemetry

Added `screen_ocr_source` to `voicewin_core::llm::VisualContextRuntime`:

- `inline`
- `prepared`

The value is now assigned in both runtime paths:

- `voicewin-engine/src/engine.rs`
  - prepared context OCR reuse -> `prepared`
  - stop-time OCR extraction -> `inline`
- `voicewin-appcore/src/service.rs`
  - preview snapshot OCR reuse -> `prepared`
  - preview-time OCR extraction -> `inline`

### UI / benchmark surfaces

Updated:

- `voicewin-tauri/src/ui/HistoryPage.tsx`
- `voicewin-tauri/src/ui/PromptsPage.tsx`
- `voicewin-tauri/src/ui/HistoryPage.test.tsx`
- `voicewin-tauri/src/ui/PromptsPage.test.tsx`

Changes:

- latency rows now show `OCR prepared` or `OCR inline`
- context rows now show `ocr:prepared` or `ocr:inline`
- replay/preview runtime summaries now include OCR provenance
- benchmark visual labels now include OCR provenance, e.g.:
  - `auto->ocr / foreground-window / ocr-prepared`
  - `auto->ocr / foreground-window / ocr-inline`
- benchmark `visual_variant_count` now distinguishes prepared-vs-inline OCR runs automatically

### Live example output

Updated:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- `voicewin-appcore/examples/live_replay_preview.rs`
- `voicewin-appcore/examples/live_session_stop_latency.rs`

Changes:

- per-round output now prints `screen_ocr_source=prepared|inline`
- summary output now prints:
  - `screen_ocr_source_variant_count`
  - `screen_ocr_source`
- visual labels in example output now include OCR provenance:
  - `ocr/display/ocr_prepared`
  - `ocr/display/ocr_inline`

### OCR fallback hardening

Live gateway validation exposed new short-form failure patterns that were not yet covered by tests:

- `Generate text in Wen/文言文 style`
- salvaged question-style answers such as `When is voice available?`

Hardened `voicewin-core/src/enhancement.rs` by:

- expanding generic assistant-option detection to include `generate`
- applying OCR clarification/question fallback logic to salvaged enhancer candidates, not just the raw cleaned output

Added regressions:

- `enhancer_post_process_prefers_screen_ocr_over_voice_when_question`
- `enhancer_post_process_prefers_screen_ocr_over_generate_text_in_wen_style`
- `enhancer_post_process_prefers_screen_ocr_over_salvaged_voice_question`

## Verification

Executed successfully:

- `cargo fmt --all`
- `cargo test -q -p voicewin-core`
- `cargo test -q -p voicewin-core -p voicewin-engine -p voicewin-appcore`
- `cargo test -q`
- `cargo check -q --examples -p voicewin-appcore`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

## Live Validation

Date validated: 2026-03-20

Gateway:

- base URL: `https://cc2.caaa.tech/v1`
- provider kind: `openai_compatible`
- API kind: `responses_sse`
- model: `gpt-5.4`

Scenario:

- transcript: `voice wen`
- screenshot fixture: `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`
- visual mode: `ocr`
- expected corrected output: `VoiceWin`

### Prepared OCR overlap

Command shape:

- `VOICEWIN_LIVE_PREPARED=true`
- `VOICEWIN_LIVE_PREPARED_CONCURRENT=true`
- `VOICEWIN_LIVE_RECORDING_SLEEP_MS=5000`

Observed:

- `final_text=VoiceWin`
- `warning=LLM output looked conversational; VoiceWin used screen OCR text as the final correction.`
- `visual_dispatch=ocr/display/ocr_prepared`
- `screen_ocr_source=prepared`
- `screen_ocr_source_variant_count=1`
- `prepare_wall_ms=2516`
- `stop_wall_ms=3221`
- `enhancement_ms=3215`
- `first_token_ms=2117`
- `screen_ocr_elapsed_ms=2516`

### Inline OCR baseline

Command shape:

- no prepared overlap
- `VOICEWIN_LIVE_RECORDING_SLEEP_MS=5000`

Observed:

- `final_text=VoiceWin`
- `warning=LLM output looked conversational; VoiceWin used screen OCR text as the final correction.`
- `visual_dispatch=ocr/display/ocr_inline`
- `screen_ocr_source=inline`
- `screen_ocr_source_variant_count=1`
- `stop_wall_ms=5000`
- `enhancement_ms=2524`
- `first_token_ms=1205`
- `screen_ocr_elapsed_ms=2470`

### Measured stop-path delta

Single-run comparison on 2026-03-20:

- prepared concurrent stop wall: `3221 ms`
- inline stop wall: `5000 ms`
- improvement: `1779 ms`

This confirms the new provenance labels are emitted correctly and remain aligned with the actual latency behavior of prepared OCR overlap versus inline OCR.

## Notes

- During validation, the gateway still occasionally produced conversational or generic OCR-mode responses. The added hardening now reliably falls back to the concise OCR text in the observed failure patterns and preserves the explicit warning trail.
- The Linux validation here covers telemetry, prompt construction, post-processing, and provider integration. Native OS-specific foreground-window capture behavior still requires macOS/Windows runtime validation on those hosts.
