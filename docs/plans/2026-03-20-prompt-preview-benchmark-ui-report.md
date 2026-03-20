# 2026-03-20 Prompt Preview Benchmark UI Report

## Goal

Upgrade Prompt Library from a one-shot prompt tester into a more practical latency lab.

The branch already had:

- editable prompt templates
- live preview against the effective provider/profile stack
- sample transcript and sample context overrides
- elapsed / first-token / cached-token telemetry on one preview run

What was still missing in the shipped UI was a quick way to measure repeated preview behavior for a
real prompt, not just a synthetic provider probe.

## Product Decision

Add a repeated preview benchmark directly in `PromptsPage`.

New action:

- `Run 3-Round Preview Benchmark`

The benchmark intentionally reuses the exact same prompt draft, preview scope, transcript, and
sample context override three times in a row.

This makes it useful for:

- rough latency comparison while iterating on prompts
- checking warning frequency
- spotting output instability across repeated runs

The UI copy explicitly keeps the scope honest:

- it benchmarks repeated prompt preview calls
- it does **not** claim to represent full recording stop-path latency

## Implementation

Updated `voicewin-tauri/src/ui/PromptsPage.tsx`:

- added benchmark state and aggregation helpers
- added `runPreviewBenchmark()`
  - calls the existing `preview_prompt` Tauri command `3` times sequentially
- benchmark summary now includes:
  - total latency min / avg / max
  - first-token latency min / avg / max
  - cached-token telemetry min / avg / max
  - warning count
  - final-output variant count
  - raw-output variant count
  - sample warning
  - last output
- the last successful round is still rendered in the normal preview detail panels, so the user can
  inspect:
  - final output
  - raw output
  - rendered system message
  - rendered user message

## Regression Coverage

Updated `voicewin-tauri/src/ui/PromptsPage.test.tsx`:

- verifies the new 3-round benchmark issues three repeated preview calls
- verifies benchmark summary rendering
- verifies warning aggregation rendering
- verifies benchmark error handling reports the failed round index

## Live Validation

Validation date:

- `2026-03-20`

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`

Prompt under test:

- built-in cleanup-style prompt

Transcript under test:

- `turn this into a polished sentence: hello voicewin world`

This is a useful real-world benchmark input because it was also the instruction-echo quality edge
case on this gateway.

### OpenAI-compatible repeated prompt preview

Command path:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- rounds: `3`
- sleep between rounds: `1000 ms`

Observed:

- total elapsed min/avg/max: `1327 / 2371 / 3114 ms`
- provider elapsed per round: `2669`, `3114`, `1326 ms`
- first-token per round: `2569`, `3010`, `1181 ms`
- cached tokens per round: `0`, `0`, `0`
- final-round output:
  - `Hello, VoiceWin world.`
- warning: none

Interpretation:

- repeated preview latency is materially variable even on the same prompt
- warm-path improvement can show up strongly on later rounds
- the prompt-preview benchmark is therefore more informative than a one-shot preview

### Gemini repeated prompt preview

First attempt with `3` rounds:

- timed out on the gateway during round `1`

Follow-up rerun with `2` rounds and the same `1000 ms` sleep:

- total elapsed min/avg/max: `10512 / 11684 / 12857 ms`
- provider elapsed per round: `12853`, `10512 ms`
- first-token per round: `12852`, `10511 ms`
- cached tokens per round: `0`, `0`
- final output:
  - `Hello, VoiceWin world.`
- warning: none

Interpretation:

- the benchmark error path is not theoretical; this gateway really can time out on repeated preview runs
- surfacing benchmark failure by round number in the UI is useful, not just defensive polish

## Verification

Passed after the final frontend changes:

- `npm test -- PromptsPage.test.tsx`
- `npm run typecheck` in `voicewin-tauri`

Full rerun afterward also passed:

- `cargo check -q`
- `cargo test -q`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

## Outcome

VoiceWin's Prompt Library is now a better prompt-development surface:

- users can measure repeated preview latency in-app
- users can see when warnings or outputs vary across runs
- the benchmark remains tied to the real prompt and real preview scope they are editing
- timeout/error handling is visible and actionable instead of silent
