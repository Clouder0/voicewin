# 2026-03-20 Provider Benchmark UI Report

## Goal

Turn the existing one-shot provider probe into a more decision-useful Settings surface for latency-sensitive users.

The branch already had:

- a one-shot smoke probe
- a one-shot screenshot probe
- live examples and reports for repeated latency measurement

What it did **not** have in the shipped UI was a quick way for users to measure repeated latency on
their current draft without dropping to a CLI harness.

## Product Decision

Add repeated benchmark actions in Settings, but keep them explicitly scoped:

- `Run 3-Round Benchmark`
- `Run 3-Round Screenshot Benchmark`

Important product framing:

- these buttons reuse the current Settings draft
- they reuse the same app/runtime client path across rounds
- they are useful for rough provider/model/gateway comparison
- they do **not** claim to represent full stop-path latency after recording

That distinction matters because VoiceWin's stop path also includes session preparation,
recording overlap, and context capture policy.

## Implementation

Updated `voicewin-tauri/src/ui/SettingsPage.tsx`:

- added benchmark-only summary state
- added `runProviderBenchmark(...)`
  - sequentially invokes the existing `probe_llm_provider` command `3` times
  - reuses the same Tauri/runtime process state
- added local summary helpers for:
  - min / avg / max total latency
  - min / avg / max first-token latency
  - min / avg / max cached-token telemetry
  - warning count
  - mismatch count
  - output-variant count
- added two new buttons:
  - `Run 3-Round Benchmark`
  - `Run 3-Round Screenshot Benchmark`
- benchmark results now render a compact summary line plus:
  - sample warning text when present
  - expected-output reminder when mismatches occur

Also tightened the existing unknown-model recovery affordance so it now triggers from benchmark
errors too, not only from one-shot probe errors.

## Regression Coverage

Updated `voicewin-tauri/src/ui/SettingsPage.test.tsx`:

- added coverage for the new 3-round benchmark flow
- verifies:
  - three repeated `probe_llm_provider` calls are issued with the current draft
  - the benchmark summary renders min / avg / max latency
  - warning aggregation is surfaced

## Live Validation

Validation date:

- `2026-03-20`

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`

Model choices:

- OpenAI-compatible: `gpt-5.4`
- Gemini: `gemini-3-flash-preview`

The shipped benchmark UI runs repeated one-shot probes, so I validated with the existing repeated
probe harness:

- `voicewin-appcore/examples/live_provider_probe.rs`

### OpenAI-compatible smoke benchmark reference

- probe kind: `smoke`
- rounds: `3`

Observed:

- elapsed min/avg/max: `1909 / 3064 / 4485 ms`
- first-token min/avg/max: `1723 / 2861 / 4169 ms`
- cached-input-tokens min/avg/max: `0 / 0 / 0`
- final output: `VoiceWin provider probe ok.`
- warning: none

### OpenAI-compatible screenshot benchmark reference

- probe kind: `screenshot_product_name`
- rounds: `3`

Observed:

- elapsed min/avg/max: `1235 / 2937 / 5141 ms`
- first-token min/avg/max: `1107 / 2876 / 5087 ms`
- cached-input-tokens min/avg/max: `0 / 0 / 0`
- final output: `VoiceWin`
- warning: none

### Gemini screenshot benchmark reference

- probe kind: `screenshot_product_name`
- rounds: `3`

Observed:

- elapsed min/avg/max: `2320 / 2820 / 3758 ms`
- first-token min/avg/max: `2319 / 2820 / 3758 ms`
- cached-input-tokens min/avg/max: `0 / 0 / 0`
- final output: `VoiceWin`
- warning: none

## Interpretation

The fresh rerun reinforced two important points:

1. provider behavior on this gateway is still variable enough that users benefit from measuring,
   not guessing
2. a one-shot probe is not enough to judge a stack fairly

It also reinforced why the UI copy should stay careful:

- repeated probe latency can differ materially from full stop-path latency
- the relative winner can move across reruns

So the benchmark buttons are intentionally presented as a rough comparison tool, not as a
definitive latency oracle.

## Verification

Passed after the final UI changes:

- `npm test -- SettingsPage.test.tsx`
- `npm run typecheck` in `voicewin-tauri`
- full workspace verification rerun afterward:
  - `cargo check -q`
  - `cargo test -q`
  - `cargo test -q` in `voicewin-tauri/src-tauri`
  - `npm run typecheck` in `voicewin-tauri`
  - `npm test` in `voicewin-tauri`

## Outcome

VoiceWin now exposes a practical benchmark surface directly in Settings:

- users can compare repeated smoke latency without leaving the app
- users can compare repeated screenshot latency on multimodal stacks
- warning and mismatch aggregation are surfaced instead of hidden in single-run noise
- the feature stays honest about what it measures and what it does not
