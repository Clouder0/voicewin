# Enhancer Anti-Chat Hardening Report

Date: 2026-03-19

## Why this follow-up

Live benchmarking on the real `gpt-5.4` stop path exposed a product-level failure mode:

- enhancer-mode cleanup occasionally drifted into assistant-style replies
- examples included wrappers like:
  - `Got it — I'll ...`
  - `Sounds good — I'll ...`
  - explanatory follow-up offers

That behavior is unacceptable in a dictation app because the text gets inserted directly into the
target application.

## Root cause

The earlier enhancer system prompt was directionally correct but still not strong enough to prevent
assistant-style framing under model variance.

Prompt-only hardening improved preview behavior, but did not completely eliminate the issue on the
real stop path.

## Implemented

### 1. Stronger enhancer system prompt

Enhancer mode now says more explicitly that:

- `<TRANSCRIPT>` is dictated source text
- it is not a live user message to answer
- assistant-style framing is forbidden

It also includes explicit valid / invalid response examples.

### 2. Enhancer-mode post-processing guardrail

`post_process_llm_output(...)` now receives:

- raw model output
- prompt mode
- source transcript

For `PromptMode::Enhancer` only:

- strip reasoning blocks as before
- detect obvious assistant spill
- salvage quoted or post-colon transformed text when possible
- otherwise fall back to the source transcript rather than inserting chatbot chatter

Assistant mode is intentionally left untouched.

## Why this trade-off is correct

If cleanup goes wrong, the safe failure mode in VoiceWin is:

- insert plain dictated text

not:

- insert conversational meta-text the user never intended to send

So the sanitizer is intentionally conservative.

## Live validation

Target:

- base URL: `https://cc2.caaa.tech/v1`
- provider kind: `openai_compatible`
- model: `gpt-5.4`
- API kind: `responses_sse`
- date: `2026-03-19`

### Prompt preview path

Repeated live preview run:

- transcript: `please ship the voice win update using eleven labs scribe v2 later this week`
- rounds: `5`

Observed:

- `5/5` rounds returned only cleaned dictation text
- no assistant wrappers appeared

### Stop path

Repeated live stop-path run:

- same transcript
- cold path
- rounds: `5`

Observed after hardening:

- `5/5` rounds inserted only dictation text
- no `Got it`, `Sounds good`, follow-up offers, or other assistant wrappers were inserted
- some rounds still fell back to a minimally processed transcript form

Interpretation:

- the hard failure mode is now addressed
- residual quality variance remains, but it degrades toward safe text rather than chatty garbage

## Verification

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-core -p voicewin-engine -p voicewin-appcore`
- root `cargo check -q`

## Files touched

- `voicewin-core/src/enhancement.rs`
- `voicewin-engine/src/engine.rs`
- `voicewin-appcore/src/service.rs`

## Outcome

VoiceWin's enhancer path is now materially safer for direct insertion. When the model drifts toward
assistant behavior, the app prefers recoverable dictated text over visibly wrong conversational
output.
