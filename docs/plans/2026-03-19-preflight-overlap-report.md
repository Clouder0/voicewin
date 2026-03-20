# Preflight Overlap Report

Date: 2026-03-19

## Summary

VoiceWin's `http_connect` preflight is still an expert-only optimization and remains **off by default**.

However, there was a real orchestration flaw in the prepared-session path:

- `prepare_session_context()` resolved app/profile/config,
- then captured context,
- only after that did it start `GET /models` preflight.

That meant slow optional context work, especially screenshot capture, could consume the overlap window that preflight was supposed to use.

This is now fixed:

- preflight starts immediately after effective config is known,
- screenshot/context capture happens after the preflight task is launched,
- the preflight task can now overlap with optional multimodal context work.

## Code Change

Adjusted `voicewin-appcore::AppService::prepare_session_context()` so `http_connect` preflight is spawned before policy-aware context capture.

This improves the implementation of the existing expert option without changing the default product behavior.

## Benchmark Harness Upgrade

Extended `voicewin-appcore/examples/live_session_stop_latency.rs` with:

- `VOICEWIN_LIVE_PREPARED_CONCURRENT`
- `VOICEWIN_LIVE_SNAPSHOT_DELAY_MS`
- `VOICEWIN_LIVE_CAPTURE_DELAY_MS`

Why this matters:

- the original harness awaited `prepare_session_context()` before sleeping, which was acceptable for fast local preparation but did not model delayed optional context work well,
- the new concurrent mode matches the real session controller more closely:
  - start prepare task,
  - simulate recording time,
  - at stop, await any remaining prepared work,
  - then run the final enhancement path.

This makes it possible to test how preflight overlap behaves when screenshot capture is slow.

## Live Benchmark Setup

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`
- OpenAI model: `gpt-5.4`
- Gemini model: `gemini-3-flash-preview`

Benchmark conditions:

- prepared mode: `true`
- prepared concurrent: `true`
- simulated recording overlap: `1000 ms`
- artificial screenshot capture delay: `1500 ms`
- screenshot enabled
- OCR enabled
- transcript: `voice wen`

Important:

- the artificial delay is there to model slow optional multimodal capture,
- it is not claiming the real platform capture necessarily costs `1500 ms`,
- it is a controlled way to evaluate whether preflight overlap is being used correctly.

## Results

### OpenAI Responses SSE

#### Preflight off

Rounds: `2`

Observed:

- prepare wall avg: `1500 ms`
- stop wall avg: `3248 ms`
- enhancement avg: `2745 ms`
- first token avg: `2624 ms`

#### Preflight on

Rounds: `2`

Observed:

- prepare wall avg: `1500 ms`
- stop wall avg: `2201 ms`
- enhancement avg: `1698 ms`
- first token avg: `1697 ms`

#### Delta

Compared with preflight off:

- stop wall improved by about `1047 ms`
- enhancement improved by about `1047 ms`
- first token improved by about `927 ms`

Interpretation:

- when optional screenshot work is slow, starting preflight earlier materially improves the useful overlap window,
- the implementation change is therefore justified on latency grounds.

### Gemini Native SSE

#### Preflight off

Rounds: `1`

Observed:

- prepare wall: `1500 ms`
- stop wall: `4385 ms`
- enhancement: `3879 ms`
- first token: `3879 ms`

#### Preflight on

Rounds: `1`

Observed:

- prepare wall: `1501 ms`
- stop wall: `2637 ms`
- enhancement: `2131 ms`
- first token: `2130 ms`

#### Delta

Compared with preflight off:

- stop wall improved by about `1748 ms`
- enhancement improved by about `1748 ms`
- first token improved by about `1749 ms`

Interpretation:

- the overlap fix helps Gemini too,
- which is consistent with the earlier latency findings that Gemini benefits more reliably from `http_connect` on this gateway.

## Product Conclusion

The product decision remains:

- keep `llm_preflight_mode = off` by default

But the implementation of the expert path is now better:

- if the user opts into `http_connect`,
- and optional screenshot/multimodal context takes real time,
- VoiceWin now overlaps that work correctly instead of wasting the preflight window.

## Outcome

This was not just a cleanup.

It removed a concrete latency bug in the preflight path:

- before: optional context capture could delay the start of warmup,
- after: warmup begins as soon as provider/model/auth are known.

That is the correct architecture for a latency-sensitive ASR post-processing app.
