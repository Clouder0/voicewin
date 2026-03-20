# 2026-03-20 Screenshot Context UI Completion Report

## Goal

Close the last obvious product-surface gap in the LLM post-processing branch:

- expose screenshot-aware multimodal context in the Tauri UI,
- stop referring to the feature as deferred OCR,
- warn clearly when the chosen provider/API cannot attach images,
- keep history terminology aligned with the runtime behavior.

## Implementation

### Settings

Updated `voicewin-tauri/src/ui/SettingsPage.tsx` to:

- add a global `Screenshot context` toggle backed by `defaults.context.use_ocr`,
- replace the outdated deferred-OCR subtitle with screenshot/multimodal wording,
- show a positive note when screenshot context is enabled on a supported API,
- show a blocking warning when screenshot context is enabled on an unsupported API such as `chat_completions`.

### Profiles

Updated `voicewin-tauri/src/ui/ProfilesPage.tsx` to:

- add `Screenshot context` to the per-profile `inherit / on / off` override set,
- update the context override subtitle to reflect actual multimodal support,
- compute the effective profile provider/API stack and surface a warning when screenshot context resolves to a text-only API.

### Shared frontend capability helper

Updated `voicewin-tauri/src/ui/llmConfig.ts` to mirror the backend capability gate:

- `llmSupportsAttachedImages(...)`
- `screenshotContextWarning(...)`

This keeps the UI support matrix aligned with the runtime policy:

- OpenAI-compatible: `responses_sse`
- Gemini: `stream_generate_content_sse`

### History wording

Updated `voicewin-tauri/src/ui/HistoryPage.tsx` so history rows now label `use_ocr` as `screenshot` instead of `ocr`.

## Regression Coverage

Updated frontend tests:

- `voicewin-tauri/src/ui/SettingsPage.test.tsx`
  - saves `use_ocr`
  - warns on `chat_completions + screenshot context`
  - clears the warning after switching to `responses_sse`
- `voicewin-tauri/src/ui/ProfilesPage.test.tsx`
  - saves per-profile `use_ocr`
  - warns when effective profile config resolves to a text-only API
- `voicewin-tauri/src/ui/HistoryPage.test.tsx`
  - verifies history now renders `screenshot` context labeling

## Verification

Passed locally on 2026-03-20:

- `cargo check -q`
- `cargo test -q`
- `cargo check -q` in `voicewin-tauri/src-tauri`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

## Live Validation

Fresh live rerun against the real gateway on 2026-03-20:

- provider: `openai_compatible`
- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`
- prepared: `true`
- prepared concurrent: `true`
- preflight: `http_connect`
- preflight delay: `1500 ms`
- rounds: `2`

Observed:

- stop wall avg: `3317 ms`
- enhancement avg: `3314 ms`
- first token avg: `2954 ms`
- cached input tokens avg: `0`
- final output: `Please ship the VoiceWin update using ElevenLabs Scribe v2 later this week.`

Round detail:

- round 1: stop `3859 ms`, first token `3508 ms`, warning = conversational fallback to transcript
- round 2: stop `2776 ms`, first token `2401 ms`, warning = none

## Outcome

The backend multimodal work is now represented accurately in the shipped UI:

- users can enable screenshot context globally or per profile,
- unsupported API choices are called out before save,
- history terminology matches the actual runtime artifact,
- the branch remains locally green and live-valid on the real OpenAI-compatible gateway.
