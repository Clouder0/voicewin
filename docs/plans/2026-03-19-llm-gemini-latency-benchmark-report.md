# Gemini Native Latency Benchmark Report

Date: 2026-03-19

## Goal

Measure whether Gemini-native `GET /models` preflight is a worthwhile stop-path latency optimization
on the user-provided gateway, instead of assuming that the OpenAI-compatible results carry over.

Endpoint under test:

- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- request shape:
  - `POST /models/{model}:streamGenerateContent?alt=sse`
  - `system_instruction`
  - `contents`
  - `Accept: text/event-stream`

Tool used:

- `voicewin-providers/examples/live_gemini_latency_bench.rs`

This benchmark uses the same `reqwest`-backed `HttpExecutor` that VoiceWin uses in production.

## Scenarios

### 1. Cold

Per run:

1. create a fresh `HttpExecutor`
2. send Gemini native SSE generation

### 2. Warmed

Per run:

1. create a fresh `HttpExecutor`
2. send authenticated `GET /models`
3. wait for an optional dwell delay
4. send Gemini native SSE generation on the same client

### 3. Persistent

Per run:

1. reuse one persistent `HttpExecutor`
2. send Gemini native SSE generation

## Results

### A. Immediate warmup, no dwell delay

Command shape:

- `VOICEWIN_GEMINI_ROUNDS=3`
- `VOICEWIN_GEMINI_WARMUP_DELAY_MS=0`

Observed medians:

- cold generation: `1944 ms`
- warmed generation after `/models`: `1273 ms`
- warmed `/models` call itself: `713 ms`
- persistent direct generation: `1776 ms`

Conclusion:

- unlike the OpenAI-compatible gateway path, Gemini-native warmup was already positive even with no
  dwell delay,
- persistent-client reuse alone was not the best path here.

### B. Delayed warmup, 2000 ms dwell

Command shape:

- `VOICEWIN_GEMINI_ROUNDS=3`
- `VOICEWIN_GEMINI_WARMUP_DELAY_MS=2000`

Observed medians:

- cold generation: `1775 ms`
- warmed generation after `/models`: `1180 ms`
- warmed `/models` call itself: `669 ms`
- persistent direct generation: `1781 ms`

Conclusion:

- the warmup advantage remained under a more realistic recording-length dwell window,
- the warmed path improved stop-time generation latency by about `595 ms` median versus cold.

## Decision

Gemini should keep the same user-facing preflight control as the OpenAI-compatible path, but the
evidence on this gateway is different:

- Gemini native `http_connect` is a real latency win on the tested gateway
- persistent-client reuse alone is not enough to match the warmed path

VoiceWin should still leave preflight **off by default globally** for now because the default path
must be conservative across provider families, but Gemini users on this gateway have a stronger case
for opting into `http_connect`.

## Important Caveat

The same gateway still returns `{"models":[]}` for `GET /v1beta/models`, so the endpoint is useful
for connection warmup but not trustworthy as a model-discovery UX source.

