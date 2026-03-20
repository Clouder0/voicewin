# 2026-03-19 Multimodal Capability Gating Report

## Goal

Close the remaining correctness and latency gap in LLM post-processing:

- If `use_ocr = true` but the selected provider/API cannot accept attached images, VoiceWin must:
  - skip screenshot capture entirely
  - avoid adding screenshot prompt context
  - avoid sending image payloads
  - surface a non-fatal warning
  - continue the session without paying screenshot latency

This specifically targeted `openai_compatible + chat_completions`, which is a text-only path in the current architecture.

## Implementation

Added provider/API capability helpers in `voicewin-core/src/llm.rs`:

- `llm_supports_attached_images(provider_kind, api_kind)`
- `screenshot_context_warning(provider_kind, api_kind)`

Wired the capability gate through:

- `voicewin-engine/src/engine.rs`
  - runtime session snapshot policy
  - enhancement prompt context construction
  - warning propagation on unsupported OCR/image combinations
- `voicewin-appcore/src/service.rs`
  - preview snapshot policy
  - prepared-session snapshot policy
  - preview warning propagation

The runtime provider safety check in `voicewin-runtime/src/llm.rs` remains intact. Unsupported image input still fails fast there if it somehow leaks through.

## Regression Coverage

Added/updated tests for the unsupported-image branch:

- `voicewin-engine/tests/engine_integration.rs`
  - `transcript_override_skips_screenshot_capture_when_api_cannot_attach_images`
- `voicewin-appcore/src/service.rs`
  - `preview_prompt_skips_screenshot_for_unsupported_api_and_surfaces_warning`
  - `runtime_session_skips_screenshot_for_unsupported_api_and_persists_warning`
  - `prepare_session_context_skips_captured_screenshot_when_api_is_unsupported`

Verified locally:

- `cargo check -q`
- `cargo test -q`
- `cargo check -q` in `voicewin-tauri/src-tauri`
- `cargo test -q` in `voicewin-tauri/src-tauri`

Everything passed.

## Live Validation

Validation date: 2026-03-19

### OpenAI Responses SSE with screenshot

Provider:

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`

Result:

- success
- warning: none
- output: `VoiceWin uses ElevenLabs Scribe v2 for dictation cleanup.`

Measured:

- preview elapsed: `7812 ms`
- first token: `7552 ms`

### Gemini native SSE with screenshot

Provider:

- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- API kind: `stream_generate_content_sse`

Result:

- success
- warning: none
- output: `VoiceWin uses ElevenLabs Scribe v2 for dictation cleanup.`

Measured:

- preview elapsed: `2852 ms`
- first token: `2851 ms`

### Text-only comparison for screenshot overhead

OpenAI Responses SSE, same prompt/transcript, `use_ocr = false`:

- preview elapsed: `5372 ms`
- first token: `5167 ms`
- output: `Voicewin uses ElevenLabs Scribe v2 for dictation cleanup.`

Observed delta vs screenshot-enabled run:

- elapsed: `+2440 ms`
- first token: `+2385 ms`
- benefit: recovered `VoiceWin` casing from context

Gemini native SSE, same prompt/transcript, `use_ocr = false`:

- preview elapsed: `2293 ms`
- first token: `2263 ms`

Observed delta vs screenshot-enabled run:

- elapsed: `+559 ms`
- first token: `+588 ms`

### Unsupported API path: `chat_completions + use_ocr = true`

Current gateway state on 2026-03-19:

- `gpt-5.4` on `chat_completions` returned `503 no_available_providers`
- `glm-5` on `chat_completions` also returned `503`
- `/v1/models` exposed `glm-5`, `gpt-5.2`, `gpt-5.2-codex`, `gpt-5.3-codex`, `gpt-5.4`, `zai-org/GLM-5`

So the gateway did not provide a healthy live `chat_completions` model for a clean preview success path during this validation window.

Still validated end-to-end at runtime using the live stop-path harness with the real gateway:

- provider/API: `openai_compatible + chat_completions`
- model: `gpt-5.4`
- configured screenshot artifact present
- injected screenshot capture delay: `1500 ms`

Observed behavior:

- session still completed successfully
- raw transcript was inserted
- warning included:
  - screenshot capability warning
  - enhancement failure warning from the upstream `503`

Latency proof that screenshot capture was skipped:

- `use_ocr = 1`, capture delay `1500 ms`: stop avg `382 ms`
- `use_ocr = 0`, capture delay `1500 ms`: stop avg `370 ms`

Prepared-session validation:

- `use_ocr = 1`, capture delay `1500 ms`, prepared mode on
- `prepare_wall_ms = 0`

This is the critical result: the configured `1500 ms` screenshot delay did not appear in either the prepared path or the stop path for unsupported APIs.

## Notes

- A tiny synthetic inline PNG was not reliable for live multimodal validation on this gateway. Reusing the earlier real screenshot artifact succeeded for both supported providers.
- `gemini-3.1-flash-preview` remained non-viable on this gateway. `gemini-3-flash-preview` is still the working choice.

## Outcome

The capability-aware multimodal gate is now closed:

- supported multimodal APIs still attach screenshots and work live
- unsupported APIs no longer waste screenshot latency
- warnings propagate to preview/runtime/history
- provider-side image rejection remains as a defensive backstop
