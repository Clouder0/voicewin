# LLM Responses SSE Implementation Spec

Date: 2026-03-19

## Goal

Upgrade `voicewin` enhancement from a single OpenAI-compatible chat-completions path to a configurable LLM layer that supports:

- OpenAI Responses over HTTP SSE,
- the existing chat-completions path as a compatibility fallback,
- configurable provider kind,
- configurable model,
- configurable reasoning effort,
- and live integration testing against a real endpoint.

This batch is intentionally scoped to the current MVP architecture:

- keep enhancement as a single post-STT step,
- do not implement prewarm/session reuse yet,
- do not implement prompt lab/history redesign yet,
- do not implement multimodal image upload yet.

## User Constraints For This Batch

Requested by user:

- base URL: `https://cc2.caaa.tech`
- target models:
  - `gpt-5.4`
  - `gemini-3.1-flash-preview`
- use `/v1/responses`
- use HTTP SSE
- use `stream: true`
- use `store: false`
- reasoning should be configurable
- "many things should be configurable in general"
- live end-to-end integration testing against the real endpoint is required

## Live Endpoint Findings Before Implementation

As of 2026-03-19, direct probes against the provided endpoint showed:

1. `/v1/models` responds successfully, but currently lists only:
   - `glm-5`
   - `zai-org/GLM-5`
2. `/v1/chat/completions` with both `gpt-5.4` and `glm-5` returns:
   - HTTP 503
   - error type `no_available_providers`
   - many provider filters with `format_type_mismatch`
3. `/v1/responses` with:
   - `stream: true`
   - `store: false`
   - several official request-shape variants
   - optional reasoning block
   also returns the same upstream 503 / `no_available_providers` / `format_type_mismatch`

This means:

- the key is accepted at least well enough to access `/v1/models`,
- the endpoint is reachable,
- but the currently available provider routing for `gpt-5.4` is not usable from the request shapes tested so far.

This is an upstream/runtime issue we must preserve in notes and expose clearly during integration verification.

## Scope

### In scope

- add configurable LLM API kind:
  - `responses_sse`
  - `chat_completions`
- add configurable reasoning effort:
  - `none`
  - `minimal`
  - `low`
  - `medium`
  - `high`
- implement Responses request builder
- implement SSE event aggregation for final output text
- route enhancement calls by configured API kind
- extend config resolution for new fields
- expose new settings controls in Tauri UI
- add tests for config resolution, request building, SSE parsing, engine behavior
- add a live integration harness/CLI test path that can hit real endpoints

### Out of scope

- WebSocket mode
- provider prewarm/session reuse
- Gemini native API implementation
- multimodal image input
- prompt CRUD/preset system
- richer history persistence redesign

## Configuration Design

Current fields:

- `llm_base_url: String`
- `llm_model: String`

Add:

- `llm_api_kind: String`
  - allowed values:
    - `responses_sse`
    - `chat_completions`
- `llm_reasoning_effort: Option<String>`
  - `None` means disabled
  - allowed values when present:
    - `minimal`
    - `low`
    - `medium`
    - `high`

These fields must exist in:

- `GlobalDefaults`
- `PowerModeOverrides`
- `EffectiveConfig`
- Tauri frontend types
- default config generation
- config resolution logic

## Runtime Design

### Provider abstraction

Keep the existing `voicewin_engine::traits::LlmProvider` for this batch to minimize blast radius.

Extend the method signature from:

```rust
async fn enhance(
    &self,
    base_url: &str,
    api_key: &str,
    model: &str,
    system_message: &str,
    user_message: &str,
) -> anyhow::Result<EnhancedText>;
```

to:

```rust
async fn enhance(
    &self,
    api_kind: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    reasoning_effort: Option<&str>,
    system_message: &str,
    user_message: &str,
) -> anyhow::Result<EnhancedText>;
```

This keeps the engine orchestration stable while still allowing the runtime provider to dispatch between:

- Responses SSE
- chat completions

### Request strategies

#### `responses_sse`

Request:

- `POST {base_url}/responses`
- `Accept: text/event-stream`
- JSON body:
  - `model`
  - `input`
  - `stream: true`
  - `store: false`
  - optional `reasoning: { effort }` when configured

Prompt mapping for the implemented batch:

- send the system prompt via request-level `instructions`
- send the user transcript via string `input`
- keep the prompt builder in `voicewin-core` unchanged

Reason for this implementation pivot:

- live validation against `https://cc2.caaa.tech/v1/responses` showed that the
  message-array `input` shape returned upstream `400 Bad Request`,
- while `instructions + input:string` succeeded end-to-end on `gpt-5.4`.

Output strategy:

- aggregate text from SSE events
- prefer semantic text-delta events
- stop on completion
- if streaming yields no deltas, attempt to recover text from terminal events before failing

#### `chat_completions`

Keep the existing path:

- `POST {base_url}/chat/completions`
- messages:
  - system
  - user
- optional reasoning parameter when configured

## Responses SSE Parsing Requirements

The parser must:

- ignore non-`data:` lines
- ignore empty keepalive chunks
- stop on `[DONE]`
- parse JSON payloads from `data: ...`
- accumulate text from OpenAI-style text delta events
- tolerate unknown event types
- return a clear error when the stream completes without any text

The implementation should not depend on one brittle event name only.

We should support at least:

- output text deltas
- output item done messages with text content
- final response completed envelope fallback

## UI / Settings Requirements

Settings page should expose:

- enhancement on/off
- LLM API kind selector
- base URL
- model
- reasoning selector
- existing API key controls

Constraints:

- reasoning selector must include an explicit disabled state
- if `responses_sse` is selected, UI copy should mention:
  - HTTP SSE
  - `stream: true`
  - `store: false`
- keep the existing secret-storage flow; do not add another secret slot in this batch

## Tests

### Unit / integration tests to add first

1. config resolution preserves new LLM fields through defaults and profile overrides
2. chat-completions request includes reasoning parameter when configured
3. responses request builder emits:
   - `/responses`
   - SSE accept header
   - `stream: true`
   - `store: false`
   - optional reasoning object
4. SSE parser aggregates output text deltas into final text
5. SSE parser can recover final text from message/item-done style events
6. runtime provider dispatches to chat vs responses by `llm_api_kind`
7. engine passes `llm_api_kind` and reasoning through to provider

### Live integration verification

Need a real probe that:

- uses base URL + key from env, not committed config
- hits `/v1/responses` using HTTP SSE
- logs headers/status
- logs the first N SSE events
- exits non-zero on API errors

This harness is required even if the upstream endpoint currently fails, because it becomes the repeatable verification artifact.

## Acceptance Criteria

The batch is done when:

1. `voicewin` can be configured to use either:
   - `responses_sse`
   - `chat_completions`
2. reasoning effort is configurable and flows into runtime requests
3. unit/integration tests pass
4. the app code can perform a real `/v1/responses` SSE call end-to-end
5. the live integration harness proves the exact outcome against the provided endpoint
6. all behavior is documented in the spec and architecture notes

## Expected Risks

- the provided endpoint may still reject `gpt-5.4` upstream even after our client is correct
- Gemini model support may not actually exist on this endpoint despite the requested model name
- SSE event shapes may vary slightly across routers; parser must stay tolerant
- changing `LlmProvider` signature touches tests and runtime wiring across crates

## Implementation Order

1. update spec and memory files
2. add failing tests for config + request builders + SSE parsing + provider dispatch
3. implement new config fields
4. implement Responses request builder + SSE parser
5. update runtime provider dispatch
6. update settings UI
7. run cargo/bun test suite
8. run live integration harness against provided endpoint
9. record final results and remaining upstream blockers explicitly
