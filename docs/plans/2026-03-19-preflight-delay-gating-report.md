# 2026-03-19 Preflight Delay Gating Report

## Goal

Improve VoiceWin's expert-only `http_connect` preflight path so it better matches the latency
evidence already gathered on this branch.

The earlier reports established two things:

1. immediate `GET /models` warmup can hurt short recordings,
2. a meaningful dwell window before warmup can materially help.

But the product still had a blunt implementation:

- if `llm_preflight_mode = http_connect`, preflight started immediately.

That left VoiceWin with no way to encode the benchmark result into the product behavior.

## Problem

A naive fix would be:

- sleep for `N` ms, then send `/models`.

That is not safe enough on its own.

If the user stops recording before the delay expires, the delayed warmup can wake up in the middle
of the real `/responses` request and create exactly the kind of overlap we want to avoid.

So this batch needed two properties together:

1. delay-aware preflight start,
2. clean cancellation before the final enhancement request begins.

## Implementation

### New config field

Added a new global config field:

- `llm_preflight_delay_ms: u64`

Current default:

- `1500`

Scope:

- global defaults / effective config
- settings UI
- runtime config validation
- live benchmark harness

This is intentionally global for now. It is an expert tuning control, not a new profile-surface
explosion.

### Why `1500 ms`

This value is grounded in the earlier benchmark sweep plus a new midpoint rerun:

- earlier reports showed:
  - `0 ms` dwell: harmful
  - `1000 ms` dwell: roughly neutral / slightly positive
  - `2000 ms` dwell: clearly positive
- this batch reran the provider benchmark at `1500 ms`

That makes `1500 ms` the most defensible midpoint to ship as the default delay when users opt into
`http_connect`.

### Abortable delayed preflight

Changed `voicewin-appcore::AppService::prepare_session_context()` so it now:

1. resolves config/app/effective config,
2. decides whether preflight is applicable,
3. spawns a detached preflight task that:
   - waits `llm_preflight_delay_ms`,
   - then sends authenticated `GET /models`,
4. keeps an abort handle for that task,
5. continues context/screenshot capture as before.

Added guard behavior so:

- if the prepare task is cancelled before it returns, the delayed preflight task is aborted,
- if the prepared context reaches the stop path, `run_session_with_prepared_with_hook(...)`
  aborts any still-pending preflight task before running the real enhancement request.

This is the key correctness improvement of the batch.

### UI / validation

Settings now exposes:

- `Preflight`
- `Preflight delay`

The delay field:

- is numeric,
- is clamped to `0..=60000`,
- is disabled when preflight mode is `off`.

The recommended-stack reset path also restores:

- `llm_preflight_mode = off`
- `llm_preflight_delay_ms = 1500`

## Tests

Added focused appcore coverage for the new behavior:

- delayed preflight does not send `/models` before the configured delay expires
- prepared context can abort the delayed preflight before it fires

Relevant file:

- `voicewin-appcore/src/service.rs`

Also updated:

- core defaults / config-store tests
- settings-page test coverage for saving `llm_preflight_delay_ms`
- Tauri runtime smoke fixture

## Live Validation

Validation date: 2026-03-19

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`

### A. Provider benchmark at `1500 ms` dwell

Tool:

- `voicewin-providers/examples/live_responses_latency_bench.rs`

Config:

- rounds: `3`
- warmup delay: `1500 ms`

Observed medians:

- cold total: `3217 ms`
- cold first token: `3050 ms`
- warmed total: `1049 ms`
- warmed first token: `854 ms`
- warmed `/models`: `610 ms`
- persistent total: `1447 ms`
- persistent first token: `1254 ms`

Interpretation:

- `1500 ms` dwell is clearly in the "useful warmup" regime on this gateway,
- it materially outperformed the cold path in this rerun,
- it also outperformed the persistent-client baseline median.

### B. Stop-path spot checks

Tool:

- `voicewin-appcore/examples/live_session_stop_latency.rs`

Spot checks were run for:

- `recording_sleep_ms=1000`
- `recording_sleep_ms=5000`
- preflight `off`
- preflight `http_connect` with `delay=0`
- preflight `http_connect` with `delay=1500`

Result:

- the real stop-path samples were highly noisy in small `2`-round runs,
- some rounds also triggered output-hardening warnings unrelated to transport timing,
- so these spot checks were useful as smoke validation but not clean enough to use as the primary
  tuning signal.

Decision:

- use the provider benchmark plus the new cancellation tests as the primary basis for the shipped
  `1500 ms` delay default,
- treat the stop-path spot checks as sanity checks only.

## Verification

Passed:

- root `cargo check -q`
- `cargo test -q -p voicewin-appcore -p voicewin-runtime -p voicewin-core`
- Tauri `cargo check -q`
- Tauri `cargo test -q`
- frontend `npm run typecheck`
- frontend `npm test`

## Product Decision

Keep:

- `llm_preflight_mode = off` by default

When users opt into `http_connect`, use:

- `llm_preflight_delay_ms = 1500`

Why:

- immediate warmup is too risky,
- `1500 ms` is now benchmark-backed on the real gateway,
- cancellation prevents the delayed warmup from colliding with the real request when recording ends
  early.

## Outcome

VoiceWin's expert preflight path is now materially better aligned with the evidence:

- not immediate by default when enabled,
- delay-aware,
- cancellable,
- test-covered,
- benchmark-backed.
