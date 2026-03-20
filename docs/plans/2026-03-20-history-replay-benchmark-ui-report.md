# 2026-03-20 History Replay Benchmark UI Report

## Goal

Close the last obvious benchmarking gap in the new LLM post-processing surface:

- Settings could benchmark repeated provider probes
- Prompt Library could benchmark repeated live previews
- History could replay one saved row once, but could not benchmark repeated replay latency

For a latency-sensitive ASR app, replaying a real saved workload is more decision-useful than a
synthetic smoke probe.

## Product Decision

Add a repeated replay benchmark directly to each History row.

New action:

- `Benchmark Again`

Scope and framing:

- runs the existing `preview_history_entry` path `3` times sequentially
- keeps the benchmark tied to a real saved transcript/prompt/app context
- updates the inline replay preview to the last successful round
- reports rough repeated replay latency and stability
- does **not** claim to represent full recording stop-path latency

That last point matters because history replay skips live capture work and only exercises the
post-processing replay path.

## Implementation

### Frontend

Updated `voicewin-tauri/src/ui/HistoryPage.tsx`:

- added per-row replay benchmark state
- added per-row replay benchmark error state
- added per-row `Benchmark Again` loading state
- added `runReplayBenchmark(...)`
  - calls `preview_history_entry` three times in sequence
  - stores the last successful replay preview inline
  - aggregates:
    - total latency min / avg / max
    - first-token latency min / avg / max
    - cached-input-token min / avg / max
    - warning count
    - final-output variant count
    - raw-output variant count
    - sample warning
    - last output
- clears stale benchmark state when a new benchmark starts
- clears stale benchmark errors when a one-shot replay is run

### Shared helper cleanup

Added `voicewin-tauri/src/ui/llmBenchmark.ts` and moved the shared numeric benchmark helpers there:

- `summarizeNumbers(...)`
- `formatBenchmarkLatency(...)`
- `formatBenchmarkCountRange(...)`

This removes the third copy of the same benchmark-formatting logic from the UI.

### Live harness

Updated `voicewin-appcore/examples/live_replay_preview.rs`:

- added `VOICEWIN_LIVE_ROUNDS`
- added `VOICEWIN_LIVE_SLEEP_MS`
- prints per-round elapsed / provider elapsed / first-token / cached-token metrics
- prints aggregated min / avg / max elapsed summary

That makes the history replay path directly benchmarkable against the real gateway.

## Regression Coverage

Updated `voicewin-tauri/src/ui/HistoryPage.test.tsx`:

- keeps existing delete coverage
- keeps existing one-shot replay coverage
- adds 3-round replay benchmark coverage
- adds replay benchmark mid-run failure coverage with failed round index

## Verification

Passed after the change:

- `cargo fmt --all`
- `cargo test -q -p voicewin-appcore`
- `cargo check -q`
- `cargo test -q`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

Latest frontend result:

- `12` test files passed
- `55` tests passed

## Live Validation

Validation date:

- `2026-03-20`

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`

Prompt/replay workload:

- transcript: `please ship the voice win update using eleven labs scribe v2 today`
- app: `code`
- window: `VoiceWin notes`
- file-backed custom vocabulary:
  - `VoiceWin`
  - `ElevenLabs`
  - `Scribe v2`

### OpenAI-compatible replay benchmark — run 1

- provider: `openai_compatible`
- api: `responses_sse`
- model: `gpt-5.4`
- reasoning effort: `low`
- rounds: `3`
- sleep between rounds: `1000 ms`

Observed per-round elapsed:

- round 1: `2617 ms`
- round 2: `12302 ms`
- round 3: `2152 ms`

Observed first-token:

- round 1: `2256 ms`
- round 2: `5202 ms`
- round 3: `1269 ms`

Aggregated:

- total min / avg / max: `2152 / 5690 / 12302 ms`
- first-token min / avg / max: `1269 / 2909 / 5202 ms`
- cached-input-tokens min / avg / max: `0 / 0 / 0`

Final output:

- `Please ship the VoiceWin update using ElevenLabs Scribe v2 today.`

### OpenAI-compatible replay benchmark — run 2

Same config, immediate rerun:

- total per-round: `3157 / 1475 / 1453 ms`
- first-token per-round: `2816 / 1147 / 1119 ms`
- total min / avg / max: `1453 / 2028 / 3157 ms`
- first-token min / avg / max: `1119 / 1694 / 2816 ms`
- cached-input-tokens min / avg / max: `0 / 0 / 0`

Final output stayed correct:

- `Please ship the VoiceWin update using ElevenLabs Scribe v2 today.`

Interpretation:

- repeated replay latency on this gateway is materially variable
- later rounds often get faster
- but the improvement is not stable enough to assume

### Gemini replay benchmark

First attempt with `3` rounds:

- timed out on round `1`
- failure:
  - `error sending request for url (https://cc2.caaa.tech/v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse)`
  - `operation timed out`

Follow-up retry with `2` rounds:

- round 1 elapsed: `3513 ms`
- round 2 elapsed: `3147 ms`
- round 1 first-token: `3508 ms`
- round 2 first-token: `3114 ms`
- total min / avg / max: `3147 / 3330 / 3513 ms`
- first-token min / avg / max: `3114 / 3311 / 3508 ms`
- cached-input-tokens min / avg / max: `0 / 0 / 0`

Final output:

- `Please ship the VoiceWin update using ElevenLabs Scribe v2 today.`

Interpretation:

- the replay benchmark error path is real, not theoretical
- Gemini can produce correct replay outputs on this gateway
- but repeated replay can still hit timeout variability

## Latency Takeaways

1. The new History benchmark is worth shipping because one-shot replay latency is not predictive.
2. Reusing the same VoiceWin process and same replay workload did **not** surface any cached-token
   evidence on this gateway:
   - OpenAI-compatible: always `0`
   - Gemini: always `0`
3. That means simple repeated replay currently demonstrates connection/process warm-path effects,
   not proven provider-side prompt-cache hits.
4. The benchmark UI should stay framed as a rough comparison tool, not a hard latency oracle.

## Files Touched

- `voicewin-appcore/examples/live_replay_preview.rs`
- `voicewin-tauri/src/ui/HistoryPage.tsx`
- `voicewin-tauri/src/ui/HistoryPage.test.tsx`
- `voicewin-tauri/src/ui/PromptsPage.tsx`
- `voicewin-tauri/src/ui/SettingsPage.tsx`
- `voicewin-tauri/src/ui/llmBenchmark.ts`

## Outcome

History now matches the rest of the new LLM tooling surface better:

- users can re-run a saved row once
- users can benchmark that saved row repeatedly
- users can see latency spread, warning frequency, and output instability
- the live replay harness now measures the same path against the real gateway
