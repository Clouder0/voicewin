# 2026-03-20 Gemini Screenshot Recommendation Report

## Goal

Surface a grounded, low-friction Settings recommendation for the specific gateway the branch has been validating:

- when screenshot context is enabled,
- when the user is on the `cc2.caaa.tech` OpenAI-compatible multimodal path,
- show that Gemini has been the cleaner screenshot-assisted option on this gateway,
- and let the user switch to the tested Gemini stack in one click.

This is intentionally a gateway-scoped recommendation, not a universal provider ranking.

## Product Decision

Keep the recommendation narrow and evidence-based.

The callout only appears when all of these are true:

- screenshot context is enabled
- provider is `openai_compatible`
- API mode supports attached images
- base URL host is `cc2.caaa.tech`

The one-click action applies the validated Gemini screenshot stack for this gateway:

- provider: `gemini`
- API mode: `stream_generate_content_sse`
- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- preflight mode: `off`
- preflight delay: `1500 ms`
- reasoning effort: disabled

The copy explicitly says this is a **quality** recommendation, not a speed guarantee.

## Implementation

### Frontend recommendation helpers

Updated `voicewin-tauri/src/ui/llmConfig.ts`:

- `shouldRecommendGeminiForScreenshotContext(...)`
  - gates the recommendation to the validated `cc2.caaa.tech` screenshot path
- `recommendedGeminiBaseUrlForCurrentGateway(...)`
  - converts the current `cc2.caaa.tech` origin to `/v1beta`
  - falls back to the public Gemini default for other hosts

### Settings UX

Updated `voicewin-tauri/src/ui/SettingsPage.tsx`:

- added the gateway-specific recommendation callout
- appended probe-aware copy when the last screenshot probe on the OpenAI-compatible path needed cleanup
- added `Switch to Gemini`
  - applies the validated Gemini stack in one click
  - clears prior probe state to avoid stale probe interpretation after the provider swap

### Regression coverage

Updated `voicewin-tauri/src/ui/SettingsPage.test.tsx`:

- verifies the callout appears for:
  - screenshot context enabled
  - `openai_compatible`
  - `responses_sse`
  - `https://cc2.caaa.tech/v1`
- verifies `Switch to Gemini` updates:
  - provider
  - API mode
  - base URL
  - model

Added `voicewin-tauri/src/ui/llmConfig.test.ts`:

- recommendation helper returns `true` only on the validated screenshot-capable `cc2` OpenAI-compatible path
- recommendation helper stays `false` when screenshot context is off, the host differs, or the API is text-only
- Gemini base URL rewrite preserves the current origin and switches to `/v1beta`
- non-`cc2` inputs fall back to the public Gemini default

## Live Evidence

This Settings recommendation is grounded in prior real-gateway validation, not guesswork.

Primary prior reports:

- `docs/plans/2026-03-19-multimodal-preview-validation-report.md`
- `docs/plans/2026-03-20-multimodal-provider-probe-report.md`

Those earlier live runs on `cc2.caaa.tech` showed:

- OpenAI-compatible `gpt-5.4` screenshot probe succeeded but often needed wrapper stripping
- Gemini `gemini-3-flash-preview` screenshot probe returned `VoiceWin` cleanly with no warning

### Fresh rerun on 2026-03-20

I reran the real multimodal screenshot probe after finishing this slice.

#### OpenAI-compatible

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API mode: `responses_sse`
- probe kind: `screenshot_product_name`
- rounds: `2`

Observed:

- elapsed min/avg/max: `3355 / 5071 / 6787 ms`
- first-token min/avg/max: `3352 / 5069 / 6786 ms`
- final output: `VoiceWin`
- final-round warning: none

#### Gemini

- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- API mode: `stream_generate_content_sse`
- probe kind: `screenshot_product_name`
- rounds: `2`

Observed:

- elapsed min/avg/max: `2157 / 11210 / 20263 ms`
- first-token min/avg/max: `2156 / 11209 / 20263 ms`
- final output: `VoiceWin`
- final-round warning: none

### Interpretation

The recommendation still stands, but it should be read correctly:

- it is about **multimodal cleanup quality history on this gateway**
- it is **not** claiming Gemini is always faster

The fresh rerun showed:

- OpenAI-compatible can also return a clean screenshot probe result
- Gemini still has very large latency variance on this gateway

So the Settings callout remains intentionally phrased as:

- gateway-specific
- quality-oriented
- non-absolute

## Verification

Passed after the final frontend changes:

- `cargo check -q`
- `cargo test -q`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

## Outcome

VoiceWin now gives users a smoother decision path for screenshot-assisted cleanup on the tested `cc2` gateway:

- the UI surfaces the accumulated live evidence where it matters
- the recommendation does not overclaim on latency
- switching to the validated Gemini stack is one click
- the slice is fully tested and revalidated against the real API
