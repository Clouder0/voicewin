# 2026-03-19 OpenAI Cached-Token Telemetry Report

## Goal

Finish the `cached_input_tokens` telemetry slice for OpenAI Responses SSE and determine whether the
real gateway used by VoiceWin actually exposes useful prompt-cache-hit telemetry.

The key question for this batch was not "does warm latency improve?" because earlier live runs had
already shown that it does. The key question was:

- is VoiceWin parsing the right field from the SSE completion event?
- or is the gateway simply returning `cached_tokens = 0` even when warm-path latency improves?

## Official API Basis

Relevant OpenAI docs:

- prompt caching is automatic for prompts `1024` tokens and larger
- the Responses API response object exposes cache-hit telemetry at
  `usage.input_tokens_details.cached_tokens`
- `prompt_cache_key` is a supported routing hint for better cache locality

Sources:

- https://developers.openai.com/api/docs/guides/prompt-caching
- https://platform.openai.com/docs/api-reference/responses/object
- https://platform.openai.com/docs/api-reference/responses/create

That field path matches the parser VoiceWin now uses:

- `response.usage.input_tokens_details.cached_tokens`

## Implementation

Completed the end-to-end telemetry path already staged in this branch:

- `voicewin-providers/src/sse.rs`
  - `SseTextEvent` now carries `cached_input_tokens`
- `voicewin-providers/src/openai_responses.rs`
  - extract `usage.input_tokens_details.cached_tokens` from `response.completed` and related
    terminal events
- `voicewin-providers/src/runtime.rs`
  - preserve `cached_input_tokens` while aggregating SSE deltas/full-text
- `voicewin-engine/src/traits.rs`
  - `EnhancedText` now carries `cached_input_tokens`
- `voicewin-runtime/src/llm.rs`
  - propagate provider telemetry into runtime enhancement results
- `voicewin-runtime/src/history.rs`
  - persist `enhancement_cached_input_tokens`
- `voicewin-runtime/src/ipc.rs`
  - expose cached-token telemetry to app/UI callers
- `voicewin-appcore/src/service.rs`
  - include cached-token telemetry in prompt preview, provider probe, and history writes
- UI/examples
  - prompt preview, settings probe, history page, replay tools, and live latency harnesses now
    display the metric when available

## Tests

Parser/runtime regression coverage now includes:

- Responses completion event with `usage.input_tokens_details.cached_tokens`
- SSE executor metric aggregation preserving cached-token telemetry through stream completion

Important unit checks:

- `voicewin-providers/src/openai_responses.rs`
- `voicewin-providers/src/runtime.rs`

## Live Validation

Validation date: 2026-03-19

Gateway:

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`

### A. Raw SSE payload inspection

Ran `voicewin-providers/examples/live_responses_sse_probe.rs` with a deliberately long static
instruction prefix so the prompt comfortably exceeded the prompt-caching threshold.

Observed terminal event:

- `type = response.completed`
- `usage.input_tokens = 3670`
- `usage.input_tokens_details.cached_tokens = 0`

This is the decisive result for the parser question:

- the gateway does emit the exact field VoiceWin parses
- the field value on this live run was explicitly `0`
- so the parser path is correct

Additional gateway-specific observation from the same run:

- the response object echoed a shortened `prompt_cache_key` value rather than the full
  client-supplied string

That does not affect VoiceWin's parser, but it is worth remembering when debugging gateway-side
cache routing.

### B. Repeated live prompt preview on current branch

Ran `voicewin-appcore/examples/live_prompt_preview.rs` against the same gateway with the same long
static prompt prefix.

Successful rerun with `rounds=2` and `sleep_ms=1000`:

- round 1: `2215 ms`, first token `1966 ms`, cached `0`
- round 2: `1329 ms`, first token `984 ms`, cached `0`

Observed result:

- warm-path latency improved materially
- cached-token telemetry still remained `0`

### C. Variance note

A separate tighter back-to-back rerun (`rounds=3`, no sleep) timed out on round 2.

Interpretation:

- this looks like gateway/runtime variance on a long SSE prompt, not a telemetry parser defect
- the parser was already proven correct by the raw SSE event inspection
- the next rerun with a short delay succeeded and still showed the same `cached=0` result

## Conclusion

Ship the telemetry implementation as-is.

Reason:

- the code is parsing the documented field correctly
- the real gateway returns that field
- on this gateway/model pair the field is currently `0` even when repeated requests get much
  faster

So for `cc2.caaa.tech` we can currently trust:

- `elapsed_ms`
- `first_token_ms`

But we cannot use `cached_input_tokens > 0` as the sole indicator that provider-side warm-path
reuse occurred.

The most defensible interpretation is:

- VoiceWin is seeing real warm-path latency improvement
- but this gateway is not surfacing that improvement as positive `cached_tokens` telemetry for the
  tested `gpt-5.4` Responses SSE path

## Decision

Keep all cached-token plumbing in the product.

Reasons:

- it is low-risk
- it is correct against the documented field path
- it already works for any gateway that reports non-zero cached tokens
- it gives VoiceWin a better observability surface for future provider comparisons

Do not add new product behavior based on `cached_input_tokens` alone yet.

For this gateway, treat it as advisory telemetry rather than a control signal.
