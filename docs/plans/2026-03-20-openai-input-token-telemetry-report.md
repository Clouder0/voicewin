# 2026-03-20 OpenAI Input-Token Telemetry Report

## Goal

Close the next observability gap in VoiceWin's latency/caching work:

- we already surface `first_token_ms`
- we already surface `cached_input_tokens`
- but we still could not answer, from inside the product, whether a request likely crossed the
  prompt-caching eligibility threshold at all

For the OpenAI Responses path, that requires surfacing total `input_tokens`, not just cached
tokens.

## Why this matters

The earlier cache reports already showed:

- warm-path latency can improve even when `cached_input_tokens = 0`
- so `cached_input_tokens` alone is not enough to reason about prompt-cache eligibility or gateway
  behavior

Adding total input-token telemetry gives VoiceWin a better answer to:

- was this request tiny and definitely below threshold?
- or was it large enough that prompt-cache behavior should at least be considered?

## Implementation

### Provider/runtime

Updated OpenAI Responses SSE parsing:

- `voicewin-providers/src/openai_responses.rs`
  - now extracts `response.usage.input_tokens`
  - keeps extracting `response.usage.input_tokens_details.cached_tokens`
- `voicewin-providers/src/sse.rs`
  - `SseTextEvent` now carries `input_tokens`
- `voicewin-providers/src/runtime.rs`
  - `SseTextResponse` now carries `input_tokens`
  - SSE aggregation preserves the last seen input-token usage value through stream completion

### Engine/runtime/appcore

Threaded the new metric through:

- `voicewin-engine/src/traits.rs`
  - `EnhancedText.input_tokens`
- `voicewin-runtime/src/llm.rs`
- `voicewin-runtime/src/ipc.rs`
  - `PromptPreviewResponse.input_tokens`
  - `ProviderProbeResponse.input_tokens`
- `voicewin-runtime/src/history.rs`
  - `HistoryEntry.enhancement_input_tokens`
- `voicewin-appcore/src/service.rs`
  - provider probe
  - prompt preview
  - persisted history

### UI

Surfaced the metric in the main LLM product surfaces:

- `PromptsPage`
  - one-shot preview latency line now shows `input tok` when present
  - 3-round benchmark summary now includes `input` min/avg/max
- `SettingsPage`
  - provider probe line now shows `input tok` when present
  - provider benchmark summary now includes `input` min/avg/max
- `HistoryPage`
  - history metadata now shows `LLM input ... tok`
  - replay preview line now shows `input tok`
  - replay benchmark summary now includes `input` min/avg/max

### Live harnesses

Updated:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- `voicewin-appcore/examples/live_provider_probe.rs`
- `voicewin-appcore/examples/live_replay_preview.rs`

They now print:

- per-round `provider_input_tokens`
- aggregate `input_tokens_min/avg/max`

## Regression Coverage

Added/updated tests for:

- OpenAI Responses SSE parser extracting both `input_tokens` and `cached_tokens`
- SSE runtime preserving both through completion
- Prompt preview UI showing input-token latency details
- Settings provider probe/benchmark showing input tokens
- History metadata/replay/replay benchmark showing input tokens

## Verification

Passed:

- `cargo fmt --all`
- `cargo check -q`
- `cargo test -q`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

Latest frontend result:

- `12` test files passed
- `60` tests passed

## Live Validation

Validation date:

- `2026-03-20`

Gateway:

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`

Used the real appcore preview path with a deliberately long static prompt prefix:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- rounds: `2`
- sleep: `1000 ms`
- transcript: `voice win works well`
- prompt text: repeated cleanup instruction block long enough to produce a very large rendered
  system prompt

Observed:

- round 1:
  - elapsed: `10267 ms`
  - first token: `10146 ms`
  - input tokens: `34`
  - cached input tokens: `0`
- round 2:
  - elapsed: `5638 ms`
  - first token: `5468 ms`
  - input tokens: `2812`
  - cached input tokens: `0`

Rendered system prompt in the final run clearly contained the repeated long instruction block, so
the request really was large.

Interpretation:

1. the new plumbing is working end to end because VoiceWin now surfaces non-zero total input-token
   usage on the real gateway
2. the same gateway can still report highly variable token telemetry across repeated runs of the
   same logical workload
3. therefore `input_tokens`, like `cached_input_tokens`, should be treated as observability
   telemetry, not a hard control signal, on this gateway

## Product Conclusion

Ship the telemetry.

Why:

- it is low risk
- it materially improves cache/latency debugging
- it helps explain whether a request is even in the regime where prompt caching could matter
- it gives VoiceWin better evidence for future provider-specific latency work

But do **not** automate product behavior from `input_tokens` alone yet.

On `cc2.caaa.tech`, the field is useful, but not stable enough to drive policy by itself.

## Outcome

VoiceWin now has a better observability story for the OpenAI Responses path:

- total latency
- first-token latency
- input tokens
- cached tokens

That is a much better foundation for future cache and prompt-size decisions than latency alone.
