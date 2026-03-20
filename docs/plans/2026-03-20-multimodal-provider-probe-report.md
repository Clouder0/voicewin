# 2026-03-20 Multimodal Provider Probe Report

## Goal

Extend the existing lightweight provider probe so VoiceWin can validate multimodal image-input
support, not just text-only connectivity.

The text-only smoke probe was already useful for:

- API key validation
- base URL / model / API mode smoke checks

But it could not answer the question that now matters more for screenshot-assisted cleanup:

- does this provider/model/API stack actually handle attached images well enough on the real gateway?

## Product Decision

Keep the original probe and add a second probe kind:

- `smoke`
  - exact-output text-only connectivity probe
- `screenshot_product_name`
  - built-in synthetic `VoiceWin` screenshot probe for multimodal image-input validation

The screenshot probe is intentionally still bounded and reproducible:

- built-in 640 px synthetic `VoiceWin` image
- dictated transcript: `voice wen`
- expected output: `VoiceWin`

This keeps it representative enough to validate screenshot-assisted cleanup while still small enough
for a Settings-page smoke tool.

## Implementation

### Backend / IPC

Updated:

- `voicewin-runtime/src/ipc.rs`
  - added `ProviderProbeKind`
  - added `probe_kind` to `ProviderProbeRequest`
  - added `probe_kind`, `expected_output`, and optional `warning` to `ProviderProbeResponse`

- `voicewin-appcore/src/service.rs`
  - `probe_llm_provider(...)` now accepts a probe kind
  - `smoke` keeps the old exact-output text-only request
  - `screenshot_product_name` now:
    - uses a built-in synthetic screenshot asset
    - builds a real enhancer-style multimodal prompt
    - runs through the same screenshot-aware sanitizer path
    - returns warnings when wrapper stripping was needed

- `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`
  - checked-in deterministic screenshot artifact used by the probe

- `voicewin-appcore/examples/live_provider_probe.rs`
  - now supports `VOICEWIN_LIVE_PROBE_KIND`

### Tauri + frontend

Updated:

- `voicewin-tauri/src-tauri/src/main.rs`
  - passes `probe_kind` through the command layer

- `voicewin-tauri/src/lib/types.ts`
  - added typed `ProviderProbeKind`
  - updated request / response types

- `voicewin-tauri/src/ui/SettingsPage.tsx`
  - keeps `Run Probe` for the text smoke probe
  - adds `Run Screenshot Probe` when the current API supports attached images
  - renders probe kind in the result line
  - renders expected-output mismatch and warning data

## Regression Coverage

Added / updated tests for:

- appcore text probe still returns exact output
- appcore screenshot probe:
  - attaches `input_image`
  - recovers `VoiceWin`
  - surfaces wrapper-strip warning when needed
- Settings page smoke probe payload now includes `probe_kind: smoke`
- Settings page screenshot probe:
  - invokes with `probe_kind: screenshot_product_name`
  - renders screenshot-probe result
  - renders probe warning text

## Live Validation

Date:

- `2026-03-20`

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`
- API key: user-provided

Probe kind:

- `screenshot_product_name`

Expected output:

- `VoiceWin`

### OpenAI-compatible `gpt-5.4` via `responses_sse`

Rounds: `3`

Observed:

- elapsed min/avg/max: `2785 / 5545 / 7580 ms`
- first-token min/avg/max: `2634 / 5401 / 7437 ms`
- final output: `VoiceWin`
- warning:
  - `LLM output looked conversational; VoiceWin stripped assistant framing from the model output.`

Interpretation:

- the multimodal probe works end to end on the real OpenAI-compatible gateway
- wrapper-style image-description behavior still appears on this provider path
- VoiceWin now exposes that fact as structured probe output instead of hiding it

### Gemini `gemini-3-flash-preview` via native SSE

Rounds: `3`

Observed:

- elapsed min/avg/max: `2142 / 9729 / 24577 ms`
- first-token min/avg/max: `2142 / 9728 / 24576 ms`
- final output: `VoiceWin`
- warning: none

Interpretation:

- the multimodal probe works end to end on Gemini too
- Gemini returned the target output cleanly in this sample
- latency still showed very large provider-side variance, so the probe remains a smoke/compatibility tool, not a benchmark substitute

## Verification

Passed:

- `cargo fmt --all`
- `cargo check -q`
- `cargo test -q`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

Focused passes also succeeded during development:

- `cargo test -q -p voicewin-appcore probe_llm_provider`
- `cargo check -q -p voicewin-tauri`
- `npm test -- SettingsPage.test.tsx`

## Outcome

VoiceWin now has a practical multimodal provider smoke tool in Settings.

That matters because the real gateway behavior is provider-specific:

- OpenAI-compatible multimodal can succeed but still require sanitizer rescue
- Gemini multimodal looked cleaner in this probe

Users can now validate that directly against their configured stack instead of inferring it from a
text-only probe or from static capability labels.
