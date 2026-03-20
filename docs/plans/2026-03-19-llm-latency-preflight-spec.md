# LLM Latency Preflight Spec

Date: 2026-03-19

## Goal

Reduce post-recording enhancement latency in `voicewin` without depending on unsafe or semantically unstable gateway behavior.

This batch focuses on two safe latency wins:

1. freeze app/context/effective-config at recording start,
2. pre-connect the persistent HTTP client during recording so the later enhancement request can reuse the same client/connection pool.

## Why This Shape

Live validation against `https://cc2.caaa.tech/v1/responses` showed:

- `instructions + input:string` is the working Responses payload shape,
- explicit `reasoning.effort != none` is transport-valid but semantically unreliable on this gateway,
- explicit `prompt_cache_key` is not safe to enable by default because requests using it produced gateway-side prompt corruption during probing,
- `prompt_cache_key` and `/v1/models` behavior from this gateway is inconsistent enough that remote prompt-cache warmup is not yet trustworthy.

So this batch must **not** depend on:

- `prompt_cache_key`,
- `previous_response_id`,
- or a dummy `/responses` warmup request.

Instead, use only safe primitives:

- local prep,
- frozen context,
- persistent `reqwest` client reuse,
- best-effort `GET /models` connection warmup.

## Scope

### In scope

- persistent OpenAI-compatible provider instance owned by `AppService`
- prepare session context at recording start
- reuse prepared app/context/effective config at stop
- configurable LLM preflight mode:
  - `off`
  - `http_connect`
- best-effort `/models` warm connection on recording start
- tests for prepared-session execution and preflight config propagation
- live probe validating that the app still works end-to-end after preflight changes

### Out of scope

- prompt-cache-key activation
- previous-response-id chaining
- dummy `/responses` warmup
- websocket warmup
- provider registry refactor
- Gemini native provider

## Design

### Prepared session context

Introduce a `PreparedSessionContext` carrying:

- foreground app identity captured at recording start
- context snapshot captured at recording start
- effective config resolved at recording start

This prevents:

- profile drift if the user switches windows before stop,
- context capture latency from landing entirely on the stop path,
- future OCR/context work from blocking the critical post-recording step.

### Provider lifetime

Today the LLM provider is rebuilt per run. That throws away:

- TCP/TLS connection reuse,
- HTTP/2 session reuse,
- any provider-local warm state.

Change:

- `AppService` owns one persistent `OpenAiCompatibleLlmProvider`
- each engine run reuses that provider instance

### Preflight mode

Add config field:

- `llm_preflight_mode: String`

Allowed values:

- `off`
- `http_connect`

Default:

- `off`

Behavior:

- on recording start, if enhancement is possible and preflight mode is `http_connect`,
  run a best-effort authenticated `GET {base_url}/models` using the persistent provider client
- ignore failures except for debug logging
- never block stop on this warmup

Revision after live benchmarking on 2026-03-19:

- keep `http_connect` implemented and user-configurable,
- but do **not** enable it by default on this gateway,
- because back-to-back `/models` then `/responses` on `https://cc2.caaa.tech/v1`
  was slower than a cold direct `/responses` call,
- and only became net-positive after a meaningful delay between warmup and stop.

So the shipped default should be conservative:

- `llm_preflight_mode = off`

Users can still opt into `http_connect` manually for environments where they have measured a win.

This is intended to pre-connect:

- DNS
- TCP
- TLS
- HTTP/2 session / connection pool state

without risking prompt corruption or additional model-side work.

## Execution flow

### Recording start

1. recorder starts
2. session controller spawns a preparation task
3. task loads config
4. task captures foreground app
5. task captures context snapshot
6. task resolves effective config
7. task optionally triggers detached `http_connect` warmup through persistent provider
8. task stores `PreparedSessionContext`

### Recording stop

1. stop recorder
2. await prepared-session task result if available
3. run engine using prepared app/context/config
4. enhancement request reuses the persistent provider client

## Validation requirements

1. prepared context uses recording-start app/config rather than re-reading at stop
2. `llm_preflight_mode` round-trips through config/UI
3. provider warmup path is best-effort and non-fatal
4. end-to-end Responses SSE still succeeds live on `gpt-5.4`

## Acceptance criteria

1. recording-start preparation is implemented
2. stop-time engine execution can consume prepared context
3. LLM provider instance persists across sessions in `AppService`
4. config/UI expose `llm_preflight_mode`
5. tests pass for touched crates
6. live `/v1/responses` SSE still works on the provided endpoint after the refactor
