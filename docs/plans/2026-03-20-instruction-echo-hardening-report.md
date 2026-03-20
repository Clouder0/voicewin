# 2026-03-20 Instruction-Echo Hardening Report

## Goal

Close a remaining quality gap in enhancer mode:

- when the dictated transcript itself looks like an editing command,
- some providers on the tested gateway still echoed that command wrapper back,
- so VoiceWin could return output like:
  - `Turn this into a polished sentence: Hello, VoiceWin world.`

That is better than chatbot spill, but still not the intended cleanup-only result.

## Problem

This was already visible in earlier live reports:

- `docs/plans/2026-03-19-llm-prompt-history-preview-completion.md`
- `docs/plans/2026-03-19-llm-responses-sse-validation-report.md`
- `docs/plans/2026-03-19-llm-reasoning-latency-report.md`

The key pattern was:

- transcript:
  - `turn this into a polished sentence: hello voicewin world`
- imperfect model output:
  - `Turn this into a polished sentence: Hello, VoiceWin world.`

This is not assistant chatter, so the earlier anti-chat sanitizer did not catch it.

## Product Decision

Fix this in two layers:

1. strengthen the enhancer prompt with an explicit valid/invalid example for command-shaped
   transcripts
2. add a narrowly-scoped post-process salvage path for instruction-echo wrappers

The post-process heuristic is intentionally conservative:

- it only activates for known edit-style prefixes
- it requires the output to preserve the same instruction prefix
- it only strips the wrapper when the suffix was actually transformed

So it is not a generic colon-stripper.

## Implementation

Updated `voicewin-core/src/enhancement.rs`:

### Prompt hardening

Added an explicit example to enhancer-mode response rules:

- valid:
  - `Hello, VoiceWin world.`
- invalid:
  - `Turn this into a polished sentence: Hello, VoiceWin world.`

### Post-process hardening

Added:

- `ENHANCER_WARNING_STRIPPED_INSTRUCTION_ECHO`
- `extract_instruction_echo_enhancer_text(...)`

Behavior:

- detect edit-style instruction wrappers such as:
  - `turn this into ...`
  - `rewrite this ...`
  - `polish this ...`
  - `clean this up ...`
  - `rephrase this ...`
- compare transcript prefix vs output prefix
- if the wrapper is echoed but the suffix was improved, strip the wrapper and keep the rewritten text

## Regression Coverage

Added / updated tests in `voicewin-core`:

- enhancer prompt includes the new anti-instruction-echo example
- post-process strips the echoed wrapper when the suffix was rewritten
- post-process does not strip when the suffix is unchanged

Added end-to-end appcore tests in `voicewin-appcore/src/service.rs`:

- preview path strips the instruction wrapper and surfaces the warning
- runtime session path strips the instruction wrapper, inserts the cleaned text, and persists the warning

## Live Validation

Validation date:

- `2026-03-20`

Gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`

Transcript under test:

- `turn this into a polished sentence: hello voicewin world`

### Baseline before this slice

Fresh reruns earlier in the same work session, before the fix:

- OpenAI-compatible preview final output:
  - `Turn this into a polished sentence: “Hello, VoiceWin world.”`
- Gemini preview final output:
  - `Turn this into a polished sentence: Hello, VoiceWin world.`

### OpenAI-compatible preview after the fix

One-round spot check:

- provider elapsed: `2970 ms`
- first token: `2876 ms`
- raw output:
  - `Hello, Voicewin World.`
- final output:
  - `Hello, Voicewin World.`
- warning: none

Three-round rerun:

- elapsed min/avg/max: `1327 / 2371 / 3114 ms`
- first-token min/avg/max: `1181 / 2253 / 3010 ms`
- final-round raw output:
  - `Hello, VoiceWin world.`
- final-round final output:
  - `Hello, VoiceWin world.`
- final-round warning: none

Interpretation:

- the bad instruction wrapper disappeared on the live OpenAI-compatible path
- the provider now followed the stronger prompt directly in these reruns
- I did **not** treat the faster rerun as a prompt-latency win claim; gateway variance is still too high

### Gemini preview after the fix

First rerun:

- timed out on the gateway SSE request

Second rerun:

- provider elapsed: `4091 ms`
- first token: `4090 ms`
- raw output:
  - `Hello, VoiceWin world.`
- final output:
  - `Hello, VoiceWin world.`
- warning: none

Interpretation:

- the successful Gemini rerun also removed the bad instruction wrapper
- gateway variance/timeouts remain a real operational characteristic and should not be hidden

### OpenAI-compatible runtime stop-path sanity check

One live stop-path rerun after the fix:

- provider: OpenAI-compatible `gpt-5.4`
- API: `responses_sse`
- prepared: `false`
- stop wall: `3737 ms`
- first token: `3416 ms`
- inserted text:
  - `Hello, Voicewin world.`
- warning: none

Interpretation:

- the runtime session path also stopped echoing the transcript instruction wrapper
- remaining casing differences still depend on provider/context quality, which is a separate issue

## Verification

Passed after the final changes:

- `cargo test -q -p voicewin-core`
- `cargo test -q -p voicewin-appcore instruction_echo_wrapper`
- full workspace rerun:
  - `cargo check -q`
  - `cargo test -q`
  - `cargo test -q` in `voicewin-tauri/src-tauri`
  - `npm run typecheck` in `voicewin-tauri`
  - `npm test` in `voicewin-tauri`

## Outcome

VoiceWin's enhancer path is now more robust on command-shaped dictated transcripts:

- the prompt is clearer about the desired behavior
- the sanitizer can recover when a provider still echoes the instruction frame
- preview and runtime both keep the cleaned text instead of the echoed wrapper

This closes a real remaining quality edge case without changing the latency architecture or
provider surface.
