# LLM Latency Benchmark Report

Date: 2026-03-19

## Goal

Measure whether `GET /models` preflight actually reduces post-recording latency for the current
OpenAI Responses gateway instead of assuming that connection warmup is helpful.

Endpoint under test:

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- request shape:
  - `instructions: string`
  - `input: string`
  - `stream: true`
  - `store: false`

Tool used:

- `voicewin-providers/examples/live_responses_latency_bench.rs`

This benchmark uses the same `reqwest`-backed `HttpExecutor` that VoiceWin uses in production.

## Scenarios

### 1. Cold

Per run:

1. create a fresh `HttpExecutor`
2. send `POST /responses`

### 2. Warmed

Per run:

1. create a fresh `HttpExecutor`
2. send authenticated `GET /models`
3. wait for an optional dwell delay
4. send `POST /responses` on the same client

### 3. Persistent

Per run:

1. reuse one persistent `HttpExecutor`
2. send `POST /responses`

## Results

### A. Immediate warmup, no dwell delay

Command shape:

- `VOICEWIN_LIVE_WARMUP_DELAY_MS=0`
- `VOICEWIN_LIVE_ROUNDS=5`

Observed medians:

- cold `/responses`: `1471 ms`
- warmed `/responses` after `/models`: `3643 ms`
- warmed `/models` call itself: `544 ms`
- persistent direct `/responses`: `1696 ms`

Conclusion:

- immediate `/models` warmup made the following `/responses` slower on this gateway,
- so enabling `http_connect` by default would be a regression.

### B. Delayed warmup, 1000 ms dwell

Command shape:

- `VOICEWIN_LIVE_WARMUP_DELAY_MS=1000`
- `VOICEWIN_LIVE_ROUNDS=3`

Observed medians:

- cold `/responses`: `1512 ms`
- warmed `/responses` after `/models`: `1434 ms`
- warmed `/models` call itself: `544 ms`

Conclusion:

- around one second of dwell time, warmup becomes roughly neutral to slightly positive,
- but the variance is still high.

### C. Delayed warmup, 2000 ms dwell

Command shape:

- `VOICEWIN_LIVE_WARMUP_DELAY_MS=2000`
- `VOICEWIN_LIVE_ROUNDS=3`

Observed medians:

- cold `/responses`: `1513 ms`
- warmed `/responses` after `/models`: `1075 ms`
- warmed `/models` call itself: `540 ms`

Conclusion:

- with a more realistic recording-length gap, `/models` warmup improved stop-path latency by
  about `438 ms` median on this gateway,
- but only after enough time elapsed between warmup and the final Responses request.

### D. Final spot-check after preflight integration

Command shape:

- `VOICEWIN_LIVE_WARMUP_DELAY_MS=2000`
- `VOICEWIN_LIVE_ROUNDS=3`

Observed medians:

- cold `/responses`: `3165 ms`
- warmed `/responses` after `/models`: `2223 ms`
- warmed `/models` call itself: `550 ms`
- persistent direct `/responses`: `1861 ms`

Important detail:

- one cold run spiked to `15305 ms`, while the persistent client stayed between
  `1534 ms` and `1926 ms`.

Conclusion:

- the repo-level changes that now matter most are the ones already implemented:
  - persistent `reqwest` client reuse in `AppService`
  - session-start preparation so warmup can happen during recording
- `/models` warmup remains a conditional optimization, not a safe universal default.

## Decision

Ship the feature, but keep it **off by default**:

- `llm_preflight_mode = off`

Rationale:

1. back-to-back warmup is measurably worse,
2. short dictation sessions are common,
3. VoiceWin must not ship a latency feature that regresses the default path,
4. advanced users can still opt into `http_connect` after measuring their own network/provider.

## Follow-up

If we want this enabled more aggressively in the future, the next safe iteration is:

1. track preflight as a first-class task in the session controller,
2. avoid overlapping `/models` with the final `/responses` request,
3. consider delaying warmup until the recording has stayed active for a minimum dwell window,
4. benchmark again before changing the default.
