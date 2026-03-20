# LLM First-Token Telemetry Report

Date: 2026-03-19

## Goal

Improve VoiceWin's latency observability for streaming LLM providers.

Total enhancement time is useful, but for a real-time-feeling ASR post-processing product the more
important question is often:

- how long until the provider starts producing text at all?

This batch adds provider-level `first_token_ms` telemetry for SSE-based enhancement paths and then
surfaces it through:

- provider runtime metrics,
- engine session timings,
- persisted history,
- prompt preview responses,
- prompt preview UI,
- live preview and live latency harnesses.

## Implementation

### Provider/runtime

Added streaming response metrics to `voicewin-providers`:

- `SseTextResponse`
  - `text`
  - `first_text_ms`
  - `total_ms`
- new executor helpers:
  - `execute_sse_collect_text_metrics(...)`
  - `execute_sse_collect_text_with_metrics(...)`

The old text-only helpers remain and delegate to the metric-bearing versions.

### Engine/runtime

`EnhancedText` now carries:

- `first_token_ms: Option<u64>`

`SessionTimings` now carries:

- `enhancement_first_token_ms: Option<u64>`

`HistoryEntry` now persists:

- `enhancement_first_token_ms`

### Appcore / UI

`PromptPreviewResponse` now carries:

- `first_token_ms`

Prompt preview UI now renders:

- total provider latency
- first-token latency when available

History UI now renders:

- `STT ... ms`
- `LLM ... ms`
- `LLM first token ... ms`

### Tests

Added/updated coverage for:

- provider SSE metrics helper
- appcore preview response carrying first-token timing
- prompt preview UI rendering the new latency line
- history UI rendering first-token timing

## Automated Verification

All relevant suites passed after the telemetry change:

- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`
- `cargo build -q -p voicewin-providers --example live_responses_latency_bench --example live_gemini_latency_bench -p voicewin-appcore --example live_prompt_preview`
- `npm test`
- `npm run typecheck`
- `cargo check -q` in `voicewin-tauri/src-tauri`

## Live Verification

### Prompt preview

#### OpenAI-compatible Responses

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`
- reasoning: `low`

Observed:

- `provider_elapsed_ms=9067`
- `provider_first_token_ms=8866`

Output:

- `Turn this into a polished sentence: Hello, Voicewin world.`

Interpretation:

- the first-token telemetry is working,
- this gateway/model pair can spend most of the request budget before any visible text arrives.

#### Gemini native

- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- API kind: `stream_generate_content_sse`
- reasoning: `low`

Observed:

- `provider_elapsed_ms=1355`
- `provider_first_token_ms=1354`

Output:

- `Hello, Voicewin world!`

Interpretation:

- telemetry is working here too,
- on this prompt the gateway effectively emitted the useful text only at the end of generation.

### Provider latency benches

#### OpenAI Responses, no reasoning, `warmup_delay_ms=2000`, `rounds=3`

Observed medians:

- cold total: `3215 ms`
- cold first token: `3062 ms`
- warmed total: `2652 ms`
- warmed first token: `2472 ms`
- persistent total: `3104 ms`
- persistent first token: `2586 ms`

Interpretation:

- OpenAI first-token latency tracks total latency closely on this gateway,
- warmup helped in this specific rerun, but variance remains high enough that the global default
  should still stay conservative.

#### Gemini native, no reasoning, `warmup_delay_ms=2000`, `rounds=3`

Observed medians:

- cold total: `1618 ms`
- cold first token: `1618 ms`
- warmed total: `1150 ms`
- warmed first token: `1150 ms`
- persistent total: `1570 ms`
- persistent first token: `1570 ms`

Interpretation:

- for this benchmark prompt, Gemini first-token and total latency were effectively identical,
- warmed Gemini remained clearly faster than cold or persistent.

## Product Read

This telemetry does not change the shipped defaults, but it improves the quality of future latency
decisions:

1. VoiceWin can now distinguish "slow to start responding" from "slow to fully finish."
2. Prompt preview becomes a better prompt-lab because users can see whether a provider feels
   sluggish before the full answer completes.
3. History becomes more useful for comparing provider families and prompt variants over time.

## Decision

Keep current defaults:

- global `llm_preflight_mode = off`
- default `llm_reasoning_effort = unset`

But use the new first-token telemetry as a required signal for future latency work:

- prewarm policy
- dwell-window heuristics
- first-token vs total-latency regression tracking
- future multimodal/OCR context experiments
