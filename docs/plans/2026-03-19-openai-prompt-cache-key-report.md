# 2026-03-19 OpenAI Prompt Cache Key Report

## Goal

Investigate the next latency lever after capability-aware screenshot gating:

- determine whether repeated OpenAI Responses requests on the real gateway show warm-path gains
- add the lowest-risk cache/routing hint supported by the official API
- validate that the change works end to end against the configured gateway

## Official API Basis

OpenAI prompt-caching docs:

- exact-prefix cache hits require static content at the beginning and dynamic content at the end
- requests are routed based on the initial prompt prefix hash
- `prompt_cache_key` is combined with that prefix hash to influence routing and improve cache-hit rates
- caching is available for prompts with `1024` tokens or more
- `cached_tokens` is still reported even when requests are below that threshold

Relevant sources:

- https://developers.openai.com/api/docs/guides/prompt-caching
- https://developers.openai.com/api/docs/guides/conversation-state

Important conversation-state note:

- `previous_response_id` works in HTTP mode too
- but disabling storage with `store=false` disables the stored-response path we would need for a safe HTTP warmup chain
- so a `prompt_cache_key` hint is lower risk than adding a `store=true` warmup flow right now

## Implementation

Added an automatic `prompt_cache_key` for OpenAI Responses requests in:

- `voicewin-providers/src/openai_responses.rs`

Current strategy:

- only emit a key when the request has stable `instructions`
- derive the key from:
  - model
  - normalized instructions
  - whether an image is attached
- intentionally ignore transcript text so repeated cleanup requests with the same static prompt prefix can share a routing key

This keeps the optimization tightly scoped to VoiceWin's enhancement path without changing API surface or user configuration.

## Tests

Added/updated provider tests:

- Responses request includes `prompt_cache_key` when stable instructions are present
- transcript changes do not change the cache key
- image usage does change the cache key bucket

Verified:

- `cargo test -q -p voicewin-providers`
- full workspace `cargo check -q`
- full workspace `cargo test -q`
- `cargo check -q` in `voicewin-tauri/src-tauri`
- `cargo test -q` in `voicewin-tauri/src-tauri`

All passed.

## Live Validation

Validation date: 2026-03-19

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API: `responses_sse`

The gateway accepted the new field with no compatibility issue.

### A. OpenAI text-only repeated preview, before `prompt_cache_key`

Three identical rounds:

- round 1: `6269 ms`, first token `5368 ms`
- round 2: `1338 ms`, first token `999 ms`
- round 3: `1620 ms`, first token `1270 ms`
- average: `3077 ms`

### B. OpenAI text-only repeated preview, after `prompt_cache_key`

Three identical rounds:

- round 1: `2177 ms`, first token `1844 ms`
- round 2: `2957 ms`, first token `2614 ms`
- round 3: `1402 ms`, first token `1087 ms`
- average: `2180 ms`

Observed result:

- average improved by about `29%`
- cold first-round latency improved by about `65%`

### C. OpenAI multimodal repeated preview, real screenshot, before `prompt_cache_key`

Three identical rounds:

- round 1: `1811 ms`, first token `1440 ms`
- round 2: `7175 ms`, first token `6892 ms`
- round 3: `5632 ms`, first token `5344 ms`
- average: `4874 ms`

### D. OpenAI multimodal repeated preview, real screenshot, after `prompt_cache_key`

Three identical rounds:

- round 1: `8051 ms`, first token `7780 ms`
- round 2: `5958 ms`, first token `5681 ms`
- round 3: `4017 ms`, first token `3730 ms`
- average: `6010 ms`

Observed result:

- the real-screenshot multimodal path remained highly variable on this gateway
- warm rounds improved relative to the slowest prior multimodal run
- the cold round regressed badly
- conclusion: no clean multimodal win from `prompt_cache_key` alone on the full-size screenshot path

### E. OpenAI multimodal repeated preview, 640 px screenshot, after `prompt_cache_key`

Three identical rounds:

- round 1: `2007 ms`, first token `1663 ms`
- round 2: `1770 ms`, first token `1424 ms`
- round 3: `1736 ms`, first token `1415 ms`
- average: `1839 ms`

Observed result:

- much better and much more stable latency
- but output quality changed from `VoiceWin` to `VOICEWIN`
- so reducing screenshot size looks promising for latency, but it is not yet safe to ship as a default without broader quality evaluation

### F. Gemini repeated preview reference

For the same transcript/prompt on the same gateway:

- text-only average: `2624 ms`
- screenshot average: `2843 ms`

Gemini was already stable and did not show the same dramatic warm-path behavior as OpenAI text-only.

## Decision

Keep the `prompt_cache_key` change.

Reasons:

- it is officially supported
- it is low-risk
- it materially improved OpenAI text-only latency on the real gateway
- it does not require changing `store=false`
- it does not require introducing stateful warmup/session chaining yet

Do **not** change the screenshot resize default yet.

Reasons:

- smaller screenshots can dramatically improve OpenAI multimodal latency
- but the limited live sample showed a real quality/casing change
- that tradeoff needs a broader quality benchmark before productizing

## Next Useful Slice

If we continue on this branch, the next high-value step is:

1. add `cached_tokens` telemetry extraction from OpenAI Responses completion events
2. benchmark whether VoiceWin prompts actually cross the `1024` token threshold often enough to trigger true prompt-cache hits
3. only then decide whether to add:
   - a real prompt-warmup preflight mode
   - configurable screenshot max edge
   - or provider-specific multimodal defaults
