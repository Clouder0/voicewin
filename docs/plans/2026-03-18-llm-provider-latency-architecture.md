# LLM Provider + Latency Architecture

Date: 2026-03-18

## Goal

Define how `voicewin` should structure LLM post-processing so it can support:

- low-latency one-shot cleanup after ASR,
- profile-specific prompting and context policy,
- optional multimodal screenshot context,
- multiple provider families,
- and future live/assistant modes,

without overcomplicating the next implementation batch.

This note is intentionally architecture-first. The productization work around prompt CRUD, prompt lab, and richer history is covered in the earlier research note and remains required.

## Short Recommendation

Keep the current `transcribe -> optional enhance -> insert` pipeline.

Do not turn the MVP into a generic agent framework. Instead:

1. keep enhancement as a single post-STT rewrite step,
2. introduce a capability-based enhancement provider layer,
3. split session preparation from session finalization so context capture and transport prewarm can happen while the user is still recording,
4. treat multimodal screenshot input as a first-class optional context artifact,
5. implement first-class providers in this order:
   - OpenAI Responses API
   - Gemini native API
   - OpenAI-compatible `chat/completions`
6. defer WebSocket/live transports to phase 2, but design the provider/session seam now so prewarm and persistent sessions fit cleanly later.

## The Most Important Structural Gap

The current `VoicewinEngine::run_session(...)` starts only after audio is already available. That means:

- app/profile resolution happens too late,
- context is captured too late,
- screenshot/OCR work cannot overlap with recording time,
- transport prewarm cannot overlap with recording time,
- and switching windows during recording can change the context unexpectedly.

This is the core reason the current engine seam is not sufficient for a serious low-latency enhancement system.

The engine needs a two-phase session lifecycle:

1. `prepare_enhancement_session(...)` at recording start
2. `finish_session_with_audio(...)` or `finish_session_with_transcript(...)` at recording stop

The current `run_session(...)` can remain as a compatibility wrapper that internally does both phases back-to-back.

## Terminology Correction

The user-facing label should not be "OpenAI completions API" unless we truly mean legacy `/v1/completions`.

For this project, "OpenAI-compatible" should mean:

- `POST /chat/completions`
- messages-based request format
- optional `/models` discovery endpoint

Legacy `/v1/completions` is a separate, older API shape. It should not be a first-class target unless real demand appears.

Recommended naming in `voicewin`:

- `OpenAI Responses`
- `Gemini`
- `OpenAI-compatible Chat`

## Product Model: Profiles Need One More Layer

The app already has per-app `PowerModeProfile`s. That is the right activation mechanism, but it is not the right unit for every enhancement setting.

To support "one profile for multiple applications" cleanly, `voicewin` should add a reusable enhancement preset layer.

Suggested model:

- `ApplicationProfile`
  - matches apps/windows/processes
  - controls STT, insertion, and which enhancement preset to use
- `EnhancementPreset`
  - prompt
  - provider
  - model
  - context policy
  - multimodal policy
  - latency/reasoning policy

This gives both standard designs:

- one app profile with custom overrides,
- or many app profiles pointing to the same enhancement preset.

This also avoids duplicating prompt/provider/context settings across many profiles.

## Proposed Provider Architecture

### Design principle

Separate three concerns that are currently collapsed into one stringly-typed path:

1. provider request shape
2. provider capabilities
3. transport/session behavior

The current runtime only has one path:

- build two messages,
- `POST /chat/completions`,
- wait for full JSON,
- parse `choices[0].message.content`.

That is too narrow for Responses, Gemini native multimodal input, or prewarmed sessions.

### Capability model

Each provider should declare capabilities up front.

Suggested capability set:

- `text_input`
- `image_input`
- `http_unary`
- `http_sse_streaming`
- `persistent_session`
- `reasoning_control`
- `model_discovery`
- `openai_compatible`

Suggested provider matrix for `voicewin`:

| Provider kind | Text | Image | HTTP unary | HTTP stream | Persistent session | Best role in Voicewin |
| --- | --- | --- | --- | --- | --- | --- |
| OpenAI Responses | yes | yes | yes | SSE | WebSocket | first-class OpenAI, future-proof multimodal path |
| Gemini native | yes | yes | yes | streamGenerateContent | Live API | first-class Gemini, strong multimodal path |
| OpenAI-compatible Chat | yes | baseline yes | yes | usually SSE | not standardized | compatibility path for OpenRouter, local gateways, Gemini OpenAI-compatible, etc. |
| Legacy Completions | yes | no | yes | sometimes | no | not recommended as a primary target |

Important: for the compatibility layer, image support must be treated as optional and provider-specific. Do not assume every OpenAI-compatible endpoint accepts image parts the same way.

### Suggested Rust seam

The provider layer should move from a single stateless call:

```rust
async fn enhance(base_url, api_key, model, system_message, user_message) -> EnhancedText
```

to a session-oriented interface closer to:

```rust
struct EnhancementTarget {
    provider_id: String,
    model: String,
    transport_preference: TransportPreference,
}

struct ProviderCapabilities {
    image_input: bool,
    sse_streaming: bool,
    persistent_session: bool,
    reasoning_control: bool,
    model_discovery: bool,
}

struct FrozenEnhancementContext {
    captured_at_unix_ms: u64,
    app: AppIdentity,
    selected_text: Option<String>,
    clipboard: Option<String>,
    window_text: Option<String>,
    custom_vocabulary: Option<String>,
    screenshot: Option<ImageArtifact>,
    ocr_text: Option<String>,
    warnings: Vec<String>,
}

#[async_trait::async_trait]
trait EnhancementProvider {
    fn capabilities(&self) -> ProviderCapabilities;

    async fn begin_session(
        &self,
        request: BeginEnhancementSessionRequest,
    ) -> anyhow::Result<Box<dyn EnhancementSession>>;
}

#[async_trait::async_trait]
trait EnhancementSession {
    async fn prewarm(&mut self) -> anyhow::Result<()>;

    async fn enhance(
        &mut self,
        request: EnhanceRequest,
    ) -> anyhow::Result<EnhancedText>;
}
```

This is intentionally small. It does not require every provider to use WebSockets. A unary HTTP provider can still implement `begin_session` by returning a lightweight session that simply holds a reusable HTTP client and frozen config.

## Context Model: Freeze Early, Separate Text From Visual

### Freeze context at recording start

At recording start:

- resolve the foreground app,
- resolve the effective application profile,
- resolve the enhancement preset,
- decide which context sources are allowed,
- start capturing those sources immediately,
- freeze the session to those choices.

This is better than resolving at stop time because:

- the context matches the moment of dictation,
- prewarm can use the final provider/model/prompt choice,
- and live transports like Gemini Live want session config fixed up front.

### Split text context from visual context

Current `voicewin` context is text-only:

- selected text
- clipboard
- window context
- custom vocabulary

That is still useful, but multimodal providers need a richer model:

- `AppIdentity` metadata
- text context
- visual context
- derived OCR text

Recommended structure:

- `AppMetadata`
  - process name
  - exe path
  - window title
  - browser URL later if available
- `TextContext`
  - selected text
  - clipboard
  - window text
  - custom vocabulary
- `VisualContext`
  - screenshot bytes or file handle
  - mime type
  - optional OCR sidecar

Do not overload `current_window_context` with both app metadata and OCR text forever. That will become messy once multimodal input is added.

### OCR is a fallback, not the primary design

If the selected provider/model supports image input:

- send the screenshot directly

If it does not:

- use OCR text only if it is ready within the latency budget

This is a critical design rule. Screenshot capture is the source artifact. OCR is a derived compatibility layer.

### Latency-aware context deadlines

Some context is cheap:

- foreground app identity
- clipboard text
- selected text

Some context is potentially slow:

- screenshot capture
- image encoding
- OCR

So the session should use deadlines:

- required context: block
- optional context: wait only within a small budget, then continue without it

If screenshot capture or OCR misses the deadline, enhancement should still run with the text context already available.

## Transport Strategy

### 1. Unary HTTP

Use unary HTTP as the baseline for all providers.

It is enough for:

- the first OpenAI Responses implementation,
- the first Gemini native implementation,
- the OpenAI-compatible chat path,
- connectivity tests,
- prompt lab execution.

Unary is not the enemy. It is the simplest path to correctness.

### 2. SSE streaming

SSE is useful when we want:

- partial output events,
- a visible "enhancing..." progress stream,
- improved cancellation behavior,
- a common foundation for Responses-style event handling.

But for one-shot rewrite after dictation, SSE is not automatically the main latency win. The user still needs the final text before insertion.

So SSE should be treated as:

- useful infrastructure,
- not the primary latency lever.

### 3. WebSocket / live sessions

Persistent WebSocket/live sessions become compelling when one of these is true:

- we prewarm while the user is recording,
- we expect multiple requests in one session,
- we want future assistant/live behavior,
- or we want to reuse provider-side session state.

For `voicewin` post-processing, the biggest reason to add WebSockets is not streaming itself. It is prewarming and connection reuse.

This is why the provider layer should support sessions now even if phase 1 still uses mostly unary HTTP.

## Latency Strategy: What Actually Matters

### The biggest practical win: overlap work with recording time

While the user is speaking:

- resolve the active app/profile/preset,
- capture clipboard/selected text,
- capture screenshot or start OCR if enabled,
- create or prewarm the provider session if supported,
- and prepare any request-shape-specific context objects.

Then, at stop time, the hot path becomes mostly:

- finish STT,
- finalize prompt,
- send transcript,
- receive rewrite,
- insert.

This hides connection and context setup behind recording time.

### Preflight patterns that are actually supported

There are three distinct latency ideas here, and they should not be conflated:

1. transport preconnect
2. prompt/context cache reuse
3. stateful session continuation

They overlap, but they are not the same mechanism.

#### OpenAI: exact-prefix prompt caching

OpenAI Prompt Caching is automatic for prompts of 1024 tokens or more, and cache hits require exact prefix matches. The docs also expose:

- `prompt_cache_key`
- `prompt_cache_retention`

Implications for `voicewin`:

- put static enhancement policy first,
- keep volatile session data at the end,
- do not put timestamps/session IDs near the front of the prompt,
- and keep images/tool declarations identical if they are meant to participate in the cached prefix.

This is the lowest-complexity way to reduce prefill cost across similar enhancement requests.

#### OpenAI: stateful continuation over Responses WebSocket mode

OpenAI also documents an official warmup path in Responses WebSocket mode:

- send `response.create` with `generate: false`
- include the tools/instructions/messages you already know
- receive a response ID
- later send the real turn with `previous_response_id` and only the new input items

This is the closest match to a true "preflight" request for `voicewin`.

Important caveats:

- this is best done at recording start, not after transcription finishes,
- the connection-local low-latency continuation path only applies to the active socket and only retains the most recent previous response in memory,
- and even when using `previous_response_id`, previous input tokens are still billed as input tokens.

So this is primarily a latency optimization, not a billing optimization.

Also note a subtle but important OpenAI rule: `instructions` are request-scoped and are not carried forward automatically via `previous_response_id`. For preflight chaining, stable enhancement behavior should live in carried input messages or prompt templates, not only in the request-level `instructions` field.

#### Gemini: implicit caching + explicit caching + stateful interactions

Gemini provides three relevant mechanisms:

- implicit caching for repeated large prefixes,
- explicit context caching via cached content objects,
- stateful continuation via `previous_interaction_id`.

For `voicewin`, these map to different use cases:

- implicit caching: cheapest and easiest, but opportunistic
- explicit caching: best when the static prefix is large and reused enough to justify creating a cache object
- `previous_interaction_id`: best when we want a stateful two-step flow where the server remembers prior context and we only send new input on the second turn

Gemini docs explicitly say that using `previous_interaction_id` can improve performance and reduce costs by making it easier for the system to utilize implicit caching for conversation history.

#### Practical recommendation for `voicewin`

Treat preflight as an optional optimization policy selected by provider capability:

- `none`
- `preconnect_only`
- `warmup_turn`
- `explicit_cache`

Recommended first implementation:

- OpenAI Responses:
  - HTTP/SSE normal path
  - optional WebSocket preconnect
  - optional WebSocket `generate:false` warmup in phase 2
- Gemini:
  - normal `generateContent` / `streamGenerateContent` path first
  - add `previous_interaction_id` stateful continuation next
  - use explicit cache only when the static prefix is large enough

This avoids overusing preflight when the prefix is too small to matter.

### Concrete low-hanging fruit in the current code

`voicewin-providers/src/runtime.rs` currently builds a fresh `reqwest::Client` for every request.

That should change regardless of SSE/WebSocket work.

Provider instances should own long-lived HTTP clients so `voicewin` can reuse:

- DNS resolution,
- TLS sessions,
- keep-alive connections,
- connection pooling.

This is a simpler and more immediate latency improvement than adding streaming everywhere.

### Other high-value latency levers

For cleanup-focused enhancement:

- use smaller/faster models first,
- keep prompts short and structured,
- limit output size,
- prefer low/disabled reasoning on rewrite paths,
- avoid extra round trips,
- and do not wait indefinitely for optional context.

For Gemini specifically, the "thinking budget" control on Flash-family models is relevant for latency-sensitive cleanup modes.

For OpenAI/Gemini assistant-like prompts, higher reasoning can remain available as an explicit preset choice rather than the default.

### A separate but interesting optimization: predicted output

For cleanup-style enhancement, the final output is often very close to the raw transcript. That makes prediction-based decode optimization conceptually attractive.

OpenAI currently documents Predicted Outputs for Chat Completions, where you provide the expected output text via the `prediction` parameter and the model can accept matching tokens faster.

This could fit rewrite presets where:

- the expected output is "mostly the same text with small corrections",
- we use a supported Chat Completions model,
- and we are willing to accept the extra complexity and rejected-prediction billing tradeoff.

However, this is not the right primary design center for `voicewin` today because:

- it is a Chat Completions-specific optimization, not the first-class Responses path,
- it is output-side optimization, not context/session optimization,
- and bad predictions still incur cost.

So it is worth keeping in mind for the OpenAI-compatible chat path, but it should remain a later experiment rather than the core architecture.

## Recommended Session Lifecycle

### Recording start

1. read current foreground app
2. resolve matching application profile
3. resolve enhancement preset
4. freeze provider/model/prompt/context policy
5. start async context capture tasks
6. create provider session
7. if supported, prewarm provider transport/session

### Recording stop

1. finish STT
2. finalize any optional context that is ready within budget
3. build provider-specific request from transcript + frozen context
4. run enhancement
5. post-process model output
6. insert
7. persist artifacts

### Failure rules

- if prewarm fails, continue with a cold request
- if screenshot is unavailable, continue text-only
- if OCR is slow, drop it
- if enhancement fails, insert raw transcript
- if insertion fails, history still stores the best available output

## Recommended Provider Order

### Phase 1A: foundation

- add provider registry + capability model
- add reusable HTTP client ownership in providers
- add session preparation seam at recording start
- extend history/session artifacts to store provider, model, prompt, raw transcript, enhanced text, timings, and context metadata

### Phase 1B: first useful providers

- OpenAI Responses over HTTP first
- Gemini native `generateContent` / `streamGenerateContent`
- OpenAI-compatible `chat/completions`

This gives:

- first-class OpenAI future path,
- first-class Gemini future path,
- compatibility for many third-party endpoints.

### Phase 1C: product surface required for usability

- prompt CRUD
- shipped prompt catalog
- enhancement preset management
- per-app profile override UI
- connectivity tests
- model discovery where available
- prompt lab with sample input

Without this, provider work is technically impressive but not user-real.

### Phase 2: latency + multimodal upgrades

- prewarm during recording
- screenshot context capture as a real context source
- direct image upload to multimodal providers
- OCR fallback for text-only providers
- OpenAI Responses WebSocket mode
- Gemini Live API

### Phase 3: optional expansion

- richer assistant mode
- live dictation assistant behaviors
- provider-specific advanced controls
- rerun enhancement from history

## Provider-Specific Notes

### OpenAI Responses

OpenAI Responses is the correct first-class OpenAI path because it aligns with the future needs:

- multimodal input,
- SSE streaming,
- WebSocket mode,
- and a cleaner long-term OpenAI story than staying on chat completions forever.

Use HTTP first. Add SSE support in the same provider family. Keep WebSocket mode for phase 2 when prewarm/session reuse is implemented.

### Gemini native

Gemini native should be first-class rather than hidden behind the OpenAI-compatible shim.

Reasons:

- native multimodal model features fit better,
- native streaming/live support is clearer,
- native latency controls are clearer,
- and the request model is not identical to OpenAI anyway.

Use `generateContent` and `streamGenerateContent` first. Add Live API later.

### OpenAI-compatible chat

This should remain important, but as a compatibility layer:

- local gateways,
- OpenRouter,
- LM Studio-style servers,
- and any providers exposing the common `/chat/completions` contract.

This provider should assume only the common denominator. Do not let compatibility constraints drag down the first-class OpenAI Responses or Gemini designs.

## Suggested Data Model Changes

### Config

Current defaults/profiles store:

- `llm_base_url`
- `llm_model`

That is too weak for multiple provider families.

Suggested evolution:

- `enhancement_provider_id`
- `enhancement_model`
- `enhancement_preset_id`
- provider-specific settings stored in a provider registry/config area

### History

Current history is too lossy for provider work.

Required additions:

- raw transcript
- enhanced text
- effective prompt id/title
- provider id
- model id
- enhancement duration
- context metadata
- warnings
- optionally request preview/debug payloads with secrets redacted

### Session result

Add enough structure to support:

- rerun enhancement
- compare prompt outputs
- provider troubleshooting
- latency breakdown

At minimum, timings should expand to include:

- context_capture_ms
- provider_prewarm_ms
- transcription_ms
- enhancement_ms
- insertion_ms

## What Not To Do

- Do not make screenshot OCR a hard prerequisite for context-aware enhancement.
- Do not force every provider into one identical request schema.
- Do not add WebSocket complexity before the engine can actually prepare sessions during recording.
- Do not call legacy `/completions` the main "OpenAI-compatible" target.
- Do not block the hot path on slow optional context capture.
- Do not keep provider selection as only `base_url + model`; that will become unmanageable quickly.

## Recommended Next Implementation Batch

If only one architecture batch is funded next, it should be:

1. split session prep from session finalization
2. add provider registry + capability model
3. ship reusable enhancement presets
4. implement OpenAI Responses HTTP
5. implement Gemini native HTTP
6. keep OpenAI-compatible chat as the compatibility adapter
7. add richer history artifacts and prompt lab support

That gives `voicewin` the right backbone for:

- profiles,
- prompt/product UX,
- multimodal context,
- and future low-latency transports,

without prematurely turning the app into a generic assistant runtime.

## Sources Checked

Official docs:

- OpenAI Streaming Responses: https://developers.openai.com/api/docs/guides/streaming-responses
- OpenAI WebSocket Mode: https://developers.openai.com/api/docs/guides/websocket-mode
- OpenAI Latency Optimization: https://developers.openai.com/api/docs/guides/latency-optimization
- OpenAI model capability references:
  - https://developers.openai.com/api/docs/models/gpt-5.4
  - https://developers.openai.com/api/docs/models/gpt-4.1-mini
- Gemini text generation + streaming: https://ai.google.dev/gemini-api/docs/text-generation
- Gemini Live API: https://ai.google.dev/api/live
- Gemini thinking controls: https://ai.google.dev/gemini-api/docs/thinking
- Gemini OpenAI compatibility: https://ai.google.dev/gemini-api/docs/openai

Local code references:

- `voicewin-providers/src/runtime.rs`
- `voicewin-providers/src/openai_compatible.rs`
- `voicewin-runtime/src/llm.rs`
- `voicewin-engine/src/engine.rs`
- `voicewin-engine/src/traits.rs`
- `voicewin-core/src/enhancement.rs`
- `voicewin-core/src/context.rs`
- `voicewin-core/src/power_mode.rs`
- sibling `codex` Responses transport code under `codex-rs/codex-api/src/endpoint/`
