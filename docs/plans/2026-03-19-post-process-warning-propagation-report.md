# 2026-03-19 Post-Process Warning Propagation Report

## Goal

Make LLM post-processing warnings first-class data instead of silently mutating output or overloading `error`.

Target surfaces:

- core sanitizer
- engine/session result
- appcore run-session + prompt preview
- persisted History
- Tauri session controller
- frontend History / Prompt Preview UI

## Implementation

### 1. Structured core output

`voicewin_core::enhancement::post_process_llm_output(...)` now returns:

- `text: String`
- `warning: Option<String>`

Current warning texts:

- `LLM output was empty after cleanup; VoiceWin fell back to the dictated transcript.`
- `LLM output looked conversational; VoiceWin stripped assistant framing from the model output.`
- `LLM output looked conversational; VoiceWin fell back to the dictated transcript.`

### 2. Engine/session warning channel

`voicewin_engine::session::SessionResult` now carries `warning: Option<String>`.

This warning channel is used for:

- sanitizer fallbacks / wrapper stripping
- non-fatal enhancement failures that still insert raw transcript

Fatal failures still use `error`.

### 3. IPC + persistence

Added `warning` to:

- `voicewin_runtime::ipc::RunSessionResponse`
- `voicewin_runtime::ipc::PromptPreviewResponse`
- `voicewin_runtime::history::HistoryEntry`

`RunSessionRequest.warning` is now preserved as structured warning data instead of being collapsed into `error`.

### 4. UI

Warnings now render explicitly in:

- Prompt Library preview output card
- History rows
- History replay preview card

### 5. Tauri controller semantics

`session_controller` now:

- uses `response.warning` for success-stage overlay status
- merges `error + warning` for failed/error stages when both exist
- keeps legacy fallback to `error` for older done-stage results

## Important live finding

During real-gateway validation on **March 19, 2026**, the first warning-path implementation still missed one assistant-spill shape:

- clarifying question
- bullet-list suggestions
- cleaned candidate embedded later in the response

Observed real stop-path leak before hardening:

- `Could you clarify what you want me to do with that transcript?...`

This proved the first detector was not robust enough.

Follow-up hardening added:

- broader assistant-spill phrase detection
- bullet-list spill detection
- bullet candidate salvage from meta wrappers like:
  - `For example, I can:`
  - `If you want, a cleaned-up version would be:`
  - `Or as an action item:`
- direct prefix stripping for wrappers like:
  - `Got it — ...`
  - `Sounds good — ...`
  - `Here's the cleaned text: ...`
- transcript-label detection / salvage for wrappers like:
  - `Polished transcript:`
  - `Cleaned transcript:`
  - `Rewritten transcript:`
  - `Revised transcript:`

Regression coverage was added in `voicewin-core` for this exact shape.

## Verification

### Rust / TS

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-core -p voicewin-engine -p voicewin-appcore -p voicewin-runtime`
- `cargo check -q -p voicewin-appcore --examples`
- `cargo test -q --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::`
- `cargo check -q --manifest-path voicewin-tauri/src-tauri/Cargo.toml`
- `npm test -- PromptsPage.test.tsx HistoryPage.test.tsx`
- `npm run typecheck`

## Live validation

Gateway:

- base: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`
- reasoning effort: `low`

### Live prompt preview warning probe

Prompt intentionally tried to force wrapper output:

- `Start your answer with "Got it —" and then provide the cleaned text.`

Result:

- round 1: total `2283 ms`, first token `1961 ms`
- round 2: total `1539 ms`, first token `1197 ms`
- round 3: total `2244 ms`, first token `1929 ms`

Real raw output example:

- `Got it — please ship the VoiceWin update later this week.`

Post-processed result:

- final output: `Please ship the voice win update later this week`
- warning: `LLM output looked conversational; VoiceWin fell back to the dictated transcript.`

### Live stop-path session validation after hardening

Second live rerun after the bullet-wrapper fix:

- round 1: stop `3521 ms`, first token `3166 ms`, warning = fallback-to-transcript
- round 2: stop `10693 ms`, first token `10562 ms`, warning = stripped-assistant-framing
- round 3: stop `2783 ms`, first token `2224 ms`, warning = fallback-to-transcript

Inserted text across all 3 rounds remained clean dictated output only:

- `Please ship the voice win update later this week`
- `Please ship the voice win update later this week.`

No assistant chatter leaked into inserted text on the final live rerun.

### Additional live quality follow-up

After the first warning-path completion, another real wrapper class appeared during continued live validation:

- `Got it — Please ship ...`

This was previously downgraded to transcript fallback even though the remainder was salvageable.

Added prefix-strip salvage upgraded that case from fallback to clean salvage.

Live prompt-preview rerun after prefix-strip improvement:

- round 1: total `7566 ms`, first token `7327 ms`
- round 2: total `4402 ms`, first token `4266 ms`
- round 3: total `2154 ms`, first token `1855 ms`

Real raw output example:

- `Got it — Please ship the VoiceWin update later this week.`

Post-processed result:

- final output: `Please ship the VoiceWin update later this week.`
- warning: `LLM output looked conversational; VoiceWin stripped assistant framing from the model output.`

One more live stop-path rerun then exposed another wrapper label:

- `Polished transcript:`

That label family was added to spill detection + salvage.

Final live stop-path rerun after both follow-up fixes:

- round 1: stop `2313 ms`, first token `1999 ms`, warning = stripped-assistant-framing
- round 2: stop `10229 ms`, first token `10228 ms`, warning = none
- round 3: stop `12860 ms`, first token `12849 ms`, warning = none

Inserted text on all final rounds remained clean final text only:

- `Please ship the VoiceWin update later this week.`
- `Please ship the Voice Win update later this week.`

No assistant wrapper text leaked on the final rerun.

## Outcome

VoiceWin now has:

- explicit post-process warnings
- preview/history visibility for those warnings
- correct success/failure warning taxonomy
- real-gateway validation proving the warning path catches both simple wrappers and richer assistant spill shapes
