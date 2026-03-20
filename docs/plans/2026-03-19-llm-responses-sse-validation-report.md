# LLM Responses SSE Validation Report

Date: 2026-03-19

## Summary

The VoiceWin Responses/SSE implementation is now validated end-to-end against the user-provided endpoint:

- base URL: `https://cc2.caaa.tech/v1`
- path: `POST /responses`
- transport: HTTP SSE
- required flags:
  - `stream: true`
  - `store: false`
- target model validated live:
  - `gpt-5.4`
- reasoning efforts validated live:
  - `none`
  - `low`

The critical wire-level finding is:

> For this endpoint, the working payload shape is `instructions + input:string`, not `input:[message,...]`.

This changed the implementation direction for the Responses request builder.

## Working Request Shape

Successful live requests used:

```json
{
  "model": "gpt-5.4",
  "instructions": "You are VoiceWin's post-processor. Return only the cleaned-up dictation.",
  "input": "turn this into a polished sentence: hello voicewin world",
  "stream": true,
  "store": false
}
```

Reasoning was also accepted live with:

```json
{
  "reasoning": { "effort": "low" }
}
```

added to the same request.

## Live Results

### 1. VoiceWin builder, `gpt-5.4`, no reasoning

Observed result:

- HTTP `200 OK`
- `content-type: text/event-stream`
- SSE emitted `response.created`, `response.in_progress`, `response.output_item.added`, `response.output_text.delta`, `response.completed`
- final text collected successfully:
  - `Hello, VoiceWin world.`

### 2. VoiceWin builder, `gpt-5.4`, `reasoning.effort = low`

Observed result:

- HTTP `200 OK`
- `content-type: text/event-stream`
- SSE completed normally
- final text was collected
- the stream included an explicit `reasoning` output item before the final answer

### 3. Strict control prompt reruns

Strict reruns were repeated again later on the same date.

Observed results:

- request:
  - `instructions = "Return exactly the single word BANANA."`
  - `input = "hello voicewin"`
  - `reasoning = none`
  - output:
    - `Hello! How can I help you today?`
- same request with:
  - `reasoning.effort = low`
  - output:
    - `Hello! What can I help you with today?`
- stricter self-referential request:
  - `instructions = "You are VoiceWin. Return only the single word BANANA and nothing else."`
  - `input = "Return only BANANA."`
  - output:
    - `BANANA`

Conclusion:

- the transport and SSE parsing path is correct,
- reasoning wiring is still accepted by the endpoint,
- but instruction obedience on this gateway is inconsistent when the user payload does not itself
  reinforce the requested format.

## Failed / Interesting Variants

### 1. `input` as array of message items

The prior VoiceWin request shape used:

- `input: [{ type: "message", role, content: [...] }, ...]`

Against `gpt-5.4`, this returned:

- HTTP `400 Bad Request`
- wrapped gateway error:
  - `Provider returned 400: Bad Request`

Conclusion:

- this endpoint does not reliably accept the message-array shape for our use case,
- so VoiceWin should keep the leaner `instructions + input:string` request shape for text-only rewrite.

### 2. `/v1/models` vs actual Responses behavior

Live `/v1/models` returned:

- `glm-5`
- `zai-org/GLM-5`

But live Responses behavior was inconsistent with that list:

- `gpt-5.4` via `/v1/responses` -> works end-to-end despite not appearing in `/v1/models`
- `gemini-3.1-flash-preview` via `/v1/responses` -> `503` unknown model
- `glm-5` via `/v1/responses` was not revalidated in the final pass because it is not a target model

Conclusion:

- model-list visibility on this gateway is not sufficient proof that a model is usable on the Responses SSE path,
- VoiceWin should not assume `/v1/models` implies operational compatibility.

### 3. Intermittent response echo inconsistency

On multiple successful `gpt-5.4` runs, the gateway echoed this in `response.created`:

- `instructions = "You are a coding agent running in the Codex CLI"`

even though VoiceWin sent:

- `instructions = "You are VoiceWin's post-processor. Return only the cleaned-up dictation."`

Later reruns still returned usable rewrite output, but strict prompt obedience remained inconsistent.

Conclusion:

- the final output path is usable,
- but echoed metadata from this gateway should not be treated as a strong source of truth for
  prompt-integrity or cache semantics,
- VoiceWin should validate this provider path with realistic dictation-cleanup probes instead of
  assuming a synthetic control prompt fully captures production behavior.

## SSE Event Compatibility Notes

The successful stream included these relevant event kinds:

- `response.created`
- `response.in_progress`
- `response.output_item.added`
- `response.content_part.added`
- `response.output_text.delta`
- `response.output_text.done`
- `response.output_item.done`
- `response.completed`

Current VoiceWin parsing is compatible with the important output-producing events:

- `response.output_text.delta`
- `response.output_text.done`
- `response.output_item.added`
- `response.output_item.done`
- `response.completed`

## Latency / Preflight Implications

Observed in live successful Responses streams:

- `prompt_cache_key` appeared in `response.created` / `response.completed`
- `previous_response_id` was present and `null`

This is a strong signal that future latency work should focus on:

1. keeping `instructions` stable across turns and profiles where possible,
2. keeping the non-user prefix minimal and deterministic,
3. later adding explicit support for:
   - `prompt_cache_key`
   - `previous_response_id`
   - connection/session reuse or prewarm

For the current batch, using:

- `instructions` for stable prompt prefix
- `input:string` for the user transcript

is also the best low-latency shape because it avoids the heavier message-array encoding and matched the live-compatible path.

Follow-up benchmark work on the same date showed:

- immediate authenticated `GET /models` warmup can make the next `/responses` call slower,
- but with a realistic delay between warmup and stop it can improve median `/responses` latency,
- persistent client reuse was more stable than a fresh-client cold path in the final spot-check,
- so VoiceWin should keep `llm_preflight_mode` implemented but **default it to `off`** until
  a later controller-managed preflight lifecycle avoids overlap with short recordings.

## Implementation Decision Locked In

For VoiceWin batch 1 Responses support:

- use `instructions` for system prompt
- use `input:string` for transcript/user payload
- use `stream:true`
- use `store:false`
- keep optional `reasoning.effort`
- keep chat completions as compatibility fallback

Do **not** use message-array Responses input for the default rewrite path in this endpoint family unless a future provider-specific compatibility layer proves it safe.
