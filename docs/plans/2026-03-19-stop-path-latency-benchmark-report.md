# Stop-Path Latency Benchmark Report

Date: 2026-03-19

## Why this follow-up

VoiceWin already had:

- provider probe latency
- prompt preview latency
- architecture notes for prepared sessions and preflight

But it still lacked a reproducible benchmark for the actual stop path that users feel after
recording stops.

That matters because the product decision is not just "can preflight help in theory?" but:

- does `prepare_session_context()` materially change stop latency?
- when does `http_connect` help?
- when does it hurt?

## Implemented

Added a new live harness:

- `voicewin-appcore/examples/live_session_stop_latency.rs`

It exercises the real session stop path using:

- transcript override mode
- normal enhancement + insertion
- persisted history timings
- optional prepared session
- optional `http_connect` preflight
- configurable simulated recording duration

The harness prints:

- prepare wall time
- stop-path wall time
- persisted enhancement time
- persisted first-token time
- min/avg/max summaries

## Live validation target

Real gateway:

- provider kind: `openai_compatible`
- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`
- reasoning: disabled

## Results

### Cold path, no prepared session, 2.5s simulated recording

Config:

- `prepared=false`
- `preflight=off`
- `recording_sleep_ms=2500`
- rounds: `3`

Observed:

- stop wall min/avg/max: `1305 / 2490 / 4218 ms`
- enhancement min/avg/max: `1305 / 2488 / 4218 ms`
- first token min/avg/max: `930 / 1677 / 2563 ms`

### Prepared session, preflight off, 2.5s simulated recording

Config:

- `prepared=true`
- `preflight=off`
- `recording_sleep_ms=2500`
- rounds: `3`

Observed:

- stop wall min/avg/max: `1649 / 2793 / 4887 ms`
- enhancement min/avg/max: `1643 / 2791 / 4887 ms`
- first token min/avg/max: `1266 / 2182 / 3812 ms`

Interpretation:

- freezing app/context/config at recording start is still the correct architecture
- but on this gateway the stop-path latency remains dominated by remote inference variability
- local preparation cost is negligible; the harness measured it below 1 ms
- prepared/off alone did not produce a reliable stop-latency win in this short-window run

### Prepared session, `http_connect`, 2.5s simulated recording

Config:

- `prepared=true`
- `preflight=http_connect`
- `recording_sleep_ms=2500`
- rounds: `3`

Observed:

- stop wall min/avg/max: `2347 / 3380 / 4176 ms`
- enhancement min/avg/max: `2347 / 3379 / 4176 ms`
- first token min/avg/max: `1952 / 2897 / 3804 ms`

Interpretation:

- at a 2.5-second overlap window, `http_connect` was worse than leaving preflight off
- this confirms we should not enable it by default

### Prepared session, preflight off, 5s simulated recording

Config:

- `prepared=true`
- `preflight=off`
- `recording_sleep_ms=5000`
- rounds: `2`

Observed:

- stop wall min/avg/max: `1659 / 1711 / 1764 ms`
- enhancement min/avg/max: `1659 / 1708 / 1758 ms`
- first token min/avg/max: `1276 / 1329 / 1383 ms`

### Prepared session, `http_connect`, 5s simulated recording

Config:

- `prepared=true`
- `preflight=http_connect`
- `recording_sleep_ms=5000`
- rounds: `2`

Observed:

- stop wall min/avg/max: `1352 / 1381 / 1411 ms`
- enhancement min/avg/max: `1346 / 1378 / 1410 ms`
- first token min/avg/max: `954 / 985 / 1017 ms`

Interpretation:

- with a longer overlap window, `http_connect` became net positive on this gateway
- compared with prepared/off at the same 5-second window:
  - stop wall improved by about `330 ms` on average
  - first-token latency improved by about `344 ms` on average

## Product conclusion

The existing product stance remains correct:

- keep prepared-session capture as core architecture
- keep `llm_preflight_mode=off` as the default
- retain `http_connect` as an expert option for users who have measured a win in their own setup

More specific takeaway:

- short recordings do not reliably benefit from `http_connect`
- longer recordings may benefit materially because the connection warmup has time to complete

## Verification

Passed:

- `cargo fmt --all`
- `cargo check -q -p voicewin-appcore --example live_session_stop_latency`
- `cargo test -q -p voicewin-appcore`

## Outcome

VoiceWin now has a reproducible benchmark for the latency users actually feel at stop time, not
just provider-only smoke timings.
