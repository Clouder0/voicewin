# 2026-03-20 Compact Screenshot Recommendation Report

## Goal

Turn the earlier screenshot-size research into a smoother in-product recommendation for users who
care about multimodal latency.

The branch already had:

- configurable `screenshot_max_edge_px`
- live evidence that `640 px` can materially reduce screenshot latency
- a Gemini-on-cc2 screenshot quality recommendation

What it still lacked was a direct product nudge for the specific advanced setting that tested well.

## Product Decision

Add a targeted recommendation in Settings for the validated gateway/provider combination:

- only when screenshot context is enabled
- only when the active provider is Gemini
- only on `cc2.caaa.tech`
- only when the current screenshot edge is still above `640 px`

New action:

- `Use 640px`

Important framing:

- the shipped default remains `1280 px`
- the recommendation is an advanced latency hint, not a forced migration
- the recommendation is scoped narrowly to the gateway/provider combination we actually measured

## Why this is justified

From `2026-03-20-screenshot-max-edge-config-report.md`:

- Gemini `1280 -> 640 px` warm screenshot latency improved by about `38%`
- realistic cleanup quality stayed intact in the live spot-check at `640 px`

That is strong enough for an in-product recommendation, but not strong enough to silently change
the global default for everyone.

## Implementation

### Config helper

Updated `voicewin-tauri/src/ui/llmConfig.ts`:

- added `recommendedCompactScreenshotMaxEdgePxForCurrentGateway(...)`
- added `shouldRecommendCompactScreenshotForLatency(...)`

Current recommendation logic is intentionally narrow:

- provider must resolve to `gemini`
- gateway host must be `cc2.caaa.tech`
- screenshot context must be enabled
- current edge must be above `640`

### Settings UI

Updated `voicewin-tauri/src/ui/SettingsPage.tsx`:

- added a compact screenshot recommendation callout in the Context section
- added one-click action:
  - `Use 640px`
- the callout copy explicitly says:
  - `1280 px` remains the conservative default
  - `640 px` is the faster advanced setting for this validated stack

## Regression Coverage

Updated tests:

- `voicewin-tauri/src/ui/llmConfig.test.ts`
  - helper returns `640` only for Gemini on `cc2.caaa.tech`
  - helper does not recommend compact screenshots for other providers/gateways
  - helper does not recommend when already at `640`
- `voicewin-tauri/src/ui/SettingsPage.test.tsx`
  - compact screenshot recommendation appears for Gemini + screenshot context on `cc2`
  - clicking `Use 640px` updates the screenshot edge draft to `640`

## Verification

Passed:

- `npm test -- llmConfig.test.ts SettingsPage.test.tsx`
- `npm run typecheck` in `voicewin-tauri`
- full rerun:
  - `npm test` in `voicewin-tauri`
  - `cargo check -q`

Latest frontend result:

- `12` test files passed
- `60` tests passed

## Live Validation

Validation date:

- `2026-03-20`

Fresh rerun used the real appcore preview path with the checked-in `640 px` screenshot artifact:

- provider: `gemini`
- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- API kind: `stream_generate_content_sse`
- reasoning effort: `low`
- rounds: `3`
- dwell: `1000 ms`
- transcript: `voice wen`
- prompt:
  - `Correct the dictated product name using the attached screenshot if needed. Return only the corrected product name as plain text. No explanation.`

Observed:

- round 1 elapsed: `3254 ms`
- round 2 elapsed: `9171 ms`
- round 3 elapsed: `2318 ms`
- total min / avg / max: `2318 / 4914 / 9171 ms`
- first token min / avg / max: `2318 / 4912 / 9171 ms`
- cached-input tokens: `0 / 0 / 0`
- final output:
  - `VoiceWin`

Interpretation:

- the current branch still produces the correct screenshot-assisted correction on the real gateway
- latency remains variable, so the recommendation should stay framed as a practical hint, not a guarantee
- the recommendation is still directionally justified by the earlier broader benchmark report

## Outcome

VoiceWin now gives latency-sensitive Gemini screenshot users a cleaner next step:

- keep the safe default at `1280 px`
- expose `640 px` as the validated advanced setting
- make it one click instead of hidden knowledge in a report
