# 2026-03-20 Screenshot Max Edge Config Report

## Goal

Turn screenshot sizing from a hidden implementation constant into an explicit product knob:

- keep the shipped default conservative at `1280 px`,
- let latency-sensitive users choose a smaller upload size,
- preserve end-to-end screenshot-context behavior and test coverage.

## Implementation

This slice adds a global screenshot sizing control and threads it through the runtime path.

### Config and policy

- added `screenshot_max_edge_px` to global defaults / effective config
- default remains `1280`
- normalized at the platform boundary to `256..3840`
- screenshot capture options are only built when screenshot context is actually allowed

### Capture seam

- `AppContextProvider::capture_screenshot(...)` now receives `ScreenshotCaptureOptions`
- `snapshot_context_for_policy(...)` now takes optional screenshot capture options
- engine and appcore now pass the configured `max_edge_px` through to screenshot capture

### Platform resizing

Shared screenshot encoding now:

- resizes to the configured longest edge before PNG encode
- preserves aspect ratio
- keeps the existing fast PNG path

### UI

Added `Screenshot max edge` to global settings:

- numeric input
- disabled unless screenshot context is enabled
- copy explains the trade-off clearly: smaller images can reduce multimodal latency but may affect quality

## Regression Coverage

Updated tests cover:

- screenshot size default loading and serde round-trip
- platform helper clamp + resize behavior
- engine propagation of `ScreenshotCaptureOptions`
- appcore prepared-session propagation
- Tauri settings save behavior for `screenshot_max_edge_px`

Because this Linux host cannot execute the real macOS / Windows screenshot capture implementations, the live benchmark below isolates the provider-side payload-size trade-off by sending already-sized screenshot artifacts through the same `AppService` preview path. The config-to-capture propagation itself is covered by Rust tests.

## Live Benchmark Setup

Date:

- `2026-03-20`

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`
- API key: user-provided

Models / APIs:

- OpenAI-compatible: `gpt-5.4` via `responses_sse`
- Gemini: `gemini-3-flash-preview` via `stream_generate_content_sse`

Prompt probe used for the latency comparison:

- prompt: `Correct the dictated product name using the attached screenshot if needed. Return only the corrected product name as plain text. No explanation.`
- transcript: `voice wen`
- rounds: `5`
- dwell between rounds: `1000 ms`
- runs executed sequentially to avoid self-induced contention

Benchmark artifacts:

- `1280 px` image: `8547 B` PNG, `11418 B` data URL
- `640 px` image: `3513 B` PNG, `4706 B` data URL

For comparison, the most decision-useful number is the warm path, so the analysis below emphasizes rounds `2-5`.

## Results

### OpenAI-compatible `gpt-5.4`

Warm-path medians, rounds `2-5`:

- `1280 px`
  - elapsed: `2014 ms`
  - first token: `1833 ms`
- `640 px`
  - elapsed: `1769 ms`
  - first token: `1382 ms`

Observed warm-path improvement from `1280 -> 640`:

- elapsed median: `-245 ms` (`-12.2%`)
- first-token median: `-451 ms` (`-24.6%`)

Additional note:

- cached-token telemetry was inconsistent on this gateway
  - `1280 px` warm cached-input average: `1184`
  - `640 px` warm cached-input average: `0`
- despite that, the smaller image was still faster in this run

Quality caveat:

- OpenAI multimodal behavior on this gateway remained instruction-obedience unstable across repeated runs
- one repeated `1280 px` run drifted into screenshot-description output instead of plain `VoiceWin`
- a realistic cleanup probe also remained inconsistent on this provider path

Interpretation:

- the size reduction is still a real latency win on the OpenAI-compatible path,
- but this slice should not be interpreted as “smaller images fix OpenAI multimodal quality” on the tested gateway.

### Gemini native `gemini-3-flash-preview`

Warm-path medians, rounds `2-5`:

- `1280 px`
  - elapsed: `3445 ms`
  - first token: `3445 ms`
- `640 px`
  - elapsed: `2124 ms`
  - first token: `2123 ms`

Observed warm-path improvement from `1280 -> 640`:

- elapsed median: `-1321 ms` (`-38.3%`)
- first-token median: `-1322 ms` (`-38.4%`)

Quality spot-checks:

- realistic cleanup probe at `1280 px`:
  - `Please ship the VoiceWin update later this week.`
- realistic cleanup probe at `640 px`:
  - `Please ship the VoiceWin update later this week.`

Interpretation:

- on this gateway, Gemini gets a large latency benefit from the smaller screenshot
- the realistic cleanup quality spot-check stayed intact at `640 px`

## Decision

Keep the product decision already chosen for this slice:

- ship `screenshot_max_edge_px` as a global configurable setting
- keep the default at `1280 px`
- do not silently lower the default yet

Why this is the right lock-in:

1. `640 px` is a meaningful latency win on both tested provider paths
2. Gemini kept realistic cleanup quality in the live spot-check at `640 px`
3. OpenAI multimodal quality on this gateway is still unstable for reasons broader than screenshot size
4. exposing the knob is the correct product move now because it lets latency-sensitive users opt into the faster path without changing the default for everyone

Practical recommendation for this gateway:

- if multimodal latency matters most, `640 px` is a strong advanced setting, especially on Gemini
- keep the shipped default at `1280 px` until broader live evidence justifies lowering it globally

## Verification

Passed locally after this slice:

- `cargo fmt --all`
- `cargo check -q`
- `cargo test -q`
- `cargo check -q` in `voicewin-tauri/src-tauri`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

## Outcome

VoiceWin now has a properly surfaced screenshot-size control instead of a hardcoded hidden default.

The implementation is green locally, the config propagates through the runtime path, and the live gateway evidence shows that smaller screenshot uploads can materially reduce multimodal latency, with the strongest observed win on Gemini.
