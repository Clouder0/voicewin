# Gemini Native Validation Report

Date: 2026-03-19

## Goal

Validate first-class Gemini native support in `voicewin` instead of only supporting Gemini through
OpenAI-compatible shims.

Implemented path:

- provider kind: `gemini`
- API kind: `stream_generate_content_sse`
- transport: `models/{model}:streamGenerateContent?alt=sse`
- auth header: `x-goog-api-key`

## Implementation Notes

VoiceWin now supports a first-class Gemini provider through:

- native request builder in `voicewin-providers/src/gemini.rs`
- generic SSE text collection in `voicewin-providers/src/runtime.rs`
- provider-aware runtime dispatch in `voicewin-runtime/src/llm.rs`
- provider-aware secret storage and Tauri settings plumbing

The current implementation targets Gemini 3-style reasoning control with:

- `generationConfig.thinkingConfig.thinkingLevel`

This matches the native Gemini 3 path we validated live below.

## Live Validation

### 1. Public Google Gemini endpoint

Probe:

- `GET https://generativelanguage.googleapis.com/v1beta/models`
- header:
  - `x-goog-api-key: $GOOGLE_API_KEY`

Observed result:

- HTTP `400`
- body:
  - `API key not valid`

Conclusion:

- the provided key is not a valid public Google Gemini API key,
- so successful native Gemini validation had to use the user-provided gateway instead.

### 2. User gateway Gemini-native discovery surface

Probe:

- `GET https://cc2.caaa.tech/v1beta/models`
- header:
  - `x-goog-api-key: <provided key>`

Observed result:

- HTTP `200`
- body:
  - `{"models":[]}`

Conclusion:

- this gateway exposes a Gemini-native path,
- but its model listing is not reliable enough to drive VoiceWin UX by itself.

### 3. Successful native Gemini SSE call on the user gateway

Probe:

- `POST https://cc2.caaa.tech/v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse`
- headers:
  - `Content-Type: application/json`
  - `Accept: text/event-stream`
  - `x-goog-api-key: <provided key>`
- prompt:
  - system: `Return exactly the single word BANANA.`
  - user: `hello voicewin`

Observed result:

- HTTP `200 OK`
- `content-type: text/event-stream`
- final collected text:
  - `BANANA`

Conclusion:

- VoiceWin's native Gemini path works end-to-end against the provided gateway when the model is:
  - `gemini-3-flash-preview`

### 4. Gemini reasoning control on the user gateway

Probe:

- same as above
- plus:
  - `generationConfig.thinkingConfig.thinkingLevel = "low"`

Observed result:

- HTTP `200 OK`
- final collected text:
  - `BANANA`

Conclusion:

- the native Gemini reasoning-effort plumbing works for the validated model/path.

### 5. Requested model name that does not work on the gateway

Probe:

- `POST https://cc2.caaa.tech/v1beta/models/gemini-3.1-flash-preview:streamGenerateContent?alt=sse`

Observed result:

- HTTP `503 Service Unavailable`
- gateway error:
  - unknown model

Conclusion:

- on 2026-03-19, the provided gateway accepts native Gemini requests but does **not** accept:
  - `gemini-3.1-flash-preview`
- the model that worked live was:
  - `gemini-3-flash-preview`

## Product Decision

Ship Gemini native support with:

- provider kind:
  - `gemini`
- API kind:
  - `stream_generate_content_sse`
- suggested/default model for the Gemini UI path:
  - `gemini-3-flash-preview`

Do not default the Gemini model to `gemini-3.1-flash-preview` because it failed live on the
provided gateway during validation on 2026-03-19.
