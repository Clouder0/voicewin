# LLM Reasoning Latency Report

Date: 2026-03-19

## Goal

Measure the latency impact of the newly exposed `reasoning_effort` control on the real user-provided
gateway instead of treating it as a purely semantic knob.

This pass was run after the initial Responses/Gemini integration work was already green. The only
tooling change needed for this report was adding `VOICEWIN_LIVE_REASONING_EFFORT` support to the
OpenAI Responses latency bench so both provider families can be measured symmetrically.

Gateway under test:

- OpenAI-compatible Responses:
  - base URL: `https://cc2.caaa.tech/v1`
  - model: `gpt-5.4`
- Gemini native:
  - base URL: `https://cc2.caaa.tech/v1beta`
  - model: `gemini-3-flash-preview`

Common benchmark shape:

- `rounds = 3`
- `warmup_delay_ms = 2000`
- cold client
- warmed client after `GET /models`
- persistent client reuse

Tools used:

- `voicewin-providers/examples/live_responses_latency_bench.rs`
- `voicewin-providers/examples/live_gemini_latency_bench.rs`
- `voicewin-appcore/examples/live_prompt_preview.rs`

## Live Preview Sanity Check

Prompt preview was rerun first to confirm the appcore path still works before deeper latency study.

Observed successful runs:

- OpenAI Responses, defaults scope, `reasoning_effort=low`
  - `provider_elapsed_ms=3626`
  - output:
    - `Turn this into a polished sentence: Hello, Voicewin world.`
- OpenAI Responses, forced profile scope, `reasoning_effort=low`
  - `provider_elapsed_ms=1772`
  - matched profile:
    - `Live Profile`
  - output:
    - `Turn this into a polished sentence: Hello, VoiceWin world.`
- Gemini native, defaults scope, `reasoning_effort=low`
  - `provider_elapsed_ms=1325`
  - output:
    - `Hello, VoiceWin world!`

Interpretation:

- the end-to-end preview path still works after the latest completion work,
- Gemini remained materially faster and more obedient to the cleanup-only prompt on this probe,
- OpenAI-compatible Responses still works, but quality/latency variance remains higher.

## Provider-Layer Benchmark Results

### 1. OpenAI Responses, no reasoning

Command shape:

- `VOICEWIN_LIVE_MODEL=gpt-5.4`
- `VOICEWIN_LIVE_WARMUP_DELAY_MS=2000`
- `VOICEWIN_LIVE_ROUNDS=3`
- `VOICEWIN_LIVE_REASONING_EFFORT` unset

Observed medians:

- cold: `1530 ms`
- warmed: `5255 ms`
- warmed `/models`: `707 ms`
- persistent: `1576 ms`

Important detail:

- warmed requests were consistently worse than cold or persistent in this rerun,
- one warmed `/models` call itself also spiked to `2294 ms`.

### 2. OpenAI Responses, `reasoning_effort=low`

Command shape:

- `VOICEWIN_LIVE_MODEL=gpt-5.4`
- `VOICEWIN_LIVE_WARMUP_DELAY_MS=2000`
- `VOICEWIN_LIVE_ROUNDS=3`
- `VOICEWIN_LIVE_REASONING_EFFORT=low`

Observed medians:

- cold: `2149 ms`
- warmed: `2814 ms`
- warmed `/models`: `652 ms`
- persistent: `1646 ms`

Observed delta vs no reasoning:

- cold median: about `+619 ms`
- warmed median: about `-2441 ms`, but still slower than persistent and too noisy to treat as a win
- persistent median: about `+70 ms`

Interpretation:

- `reasoning_effort=low` is transport-valid on this gateway and still returns usable text,
- but OpenAI Responses latency remains noisy enough that we should not promise a stable reasoning
  overhead number from this gateway alone,
- the practical product decision still stands:
  - keep reasoning configurable,
  - keep it unset by default,
  - keep OpenAI `http_connect` preflight off by default.

### 3. Gemini native, no reasoning

Command shape:

- `VOICEWIN_GEMINI_MODEL=gemini-3-flash-preview`
- `VOICEWIN_GEMINI_WARMUP_DELAY_MS=2000`
- `VOICEWIN_GEMINI_ROUNDS=3`
- `VOICEWIN_GEMINI_REASONING_EFFORT` unset

Observed medians:

- cold: `1592 ms`
- warmed: `1176 ms`
- warmed `/models`: `747 ms`
- persistent: `1568 ms`

Important detail:

- one cold run spiked to `22215 ms`,
- warmed runs stayed tightly grouped between `1125 ms` and `1403 ms`.

### 4. Gemini native, `reasoning_effort=low`

Command shape:

- `VOICEWIN_GEMINI_MODEL=gemini-3-flash-preview`
- `VOICEWIN_GEMINI_WARMUP_DELAY_MS=2000`
- `VOICEWIN_GEMINI_ROUNDS=3`
- `VOICEWIN_GEMINI_REASONING_EFFORT=low`

Observed medians:

- cold: `1635 ms`
- warmed: `1352 ms`
- warmed `/models`: `730 ms`
- persistent: `1679 ms`

Observed delta vs no reasoning:

- cold median: about `+43 ms`
- warmed median: about `+176 ms`
- persistent median: about `+111 ms`

Interpretation:

- Gemini native remained the more predictable low-latency provider family on this gateway,
- `http_connect` warmup still helps Gemini stop-path latency,
- `reasoning_effort=low` adds only a modest cost in these runs.

## Decision

Keep the existing product defaults:

1. `llm_preflight_mode = off` globally
2. `llm_reasoning_effort = unset` by default
3. Gemini native remains a first-class path for users who care most about low latency and stable
   cleanup behavior on this gateway
4. OpenAI Responses remains supported, but VoiceWin should continue treating both latency and prompt
   obedience on this gateway as environment-specific rather than universally reliable

## Follow-up

If we revisit latency defaults later, the next safe experiments are:

1. benchmark longer dwell windows with controller-managed warmup instead of fixed synthetic delays,
2. collect first-token latency in addition to full-response latency,
3. only consider prompt-cache or chained-response optimizations after provider-specific validation,
   not by assumption.
