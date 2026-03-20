# Gemini Model Compatibility Refresh

Date: 2026-03-19

## Why this follow-up

The user explicitly asked for Gemini support with `gemini-3.1-flash-preview`, but the current
VoiceWin defaults had already drifted to `gemini-3-flash-preview` based on earlier live testing.

That is the kind of detail that must be re-verified, not assumed.

## Live re-validation

Endpoint tested:

- base URL: `https://cc2.caaa.tech/v1beta`
- API kind: `stream_generate_content_sse`
- date rechecked: `2026-03-19`

### `gemini-3.1-flash-preview`

Observed result:

- request failed
- HTTP status: `503`
- gateway error body included:
  - `未知模型，请检查模型名称是否正确`

Interpretation:

- this gateway still does not currently expose `gemini-3.1-flash-preview`
- so it is not safe to make that the shipped Gemini default for VoiceWin

### `gemini-3-flash-preview`

Observed result:

- request succeeded
- final output: `VoiceWin provider probe ok.`
- elapsed: `1540 ms`
- first token: `1539 ms`

## Product decision

Keep the Gemini default model as:

- `gemini-3-flash-preview`

Do not silently switch defaults to `gemini-3.1-flash-preview` until live validation on the real
gateway succeeds.

## UX improvement shipped with this note

Settings now recognizes "unknown model" style probe failures and offers a one-click reset back to
the provider default model. This keeps the product smooth even when a gateway advertises or
accepts only a narrower model subset than expected.
