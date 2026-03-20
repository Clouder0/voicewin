# 2026-03-20 Screenshot Prompt Hardening Report

## Goal

Continue hardening multimodal enhancer behavior on the OpenAI-compatible Responses path after the
earlier screenshot-wrapper salvage work.

Two concrete issues were observed live on `2026-03-20`:

- screenshot-attached prompts still allowed image-description wrappers like
  - `The screenshot appears to contain ...`
- when the model echoed the transcript first and only later produced a cleaned version, VoiceWin
  could salvage the transcript echo too early instead of the better cleaned text

## Implementation

### 1. Screenshot-specific prompt guidance

Updated [enhancement.rs](/home/clouder/GitRepos/voicewin/voicewin/voicewin-core/src/enhancement.rs) so that when a screenshot is attached, enhancer mode now adds stronger screenshot output rules:

- do not describe the screenshot unless explicitly asked
- do not explain that an image is attached
- if the screenshot helps recover exact words/names/casing, return only the final corrected text

Added screenshot-specific examples:

- valid: `VoiceWin`
- invalid: `The screenshot contains the word VoiceWin.`
- invalid: `I see an image with the text VoiceWin.`

### 2. Better salvage candidate prioritization

Also updated [enhancement.rs](/home/clouder/GitRepos/voicewin/voicewin/voicewin-core/src/enhancement.rs) so transcript-equivalent salvage candidates are treated as low-priority fallbacks.

This matters for outputs shaped like:

- transcript echo first
- cleaned version later

VoiceWin now prefers the later non-transcript candidate when available instead of locking onto the
earlier echo.

### 3. Regression anchoring in appcore

Updated [service.rs](/home/clouder/GitRepos/voicewin/voicewin/voicewin-appcore/src/service.rs) preview tests so screenshot-enabled previews assert that the stronger screenshot prompt block is present.

## Regression Coverage

Added / updated tests for:

- screenshot-specific prompt rules are rendered when an image is attached
- screenshot wrapper variants still salvage to `VoiceWin`
- transcript-echo plus later cleaned-version output now resolves to the cleaned version rather than the echoed transcript
- appcore preview keeps the stricter screenshot block on the rendered system prompt

## Verification

Passed locally:

- `cargo fmt --all`
- `cargo check -q`
- `cargo test -q`

Focused passes also succeeded during development:

- `cargo test -q -p voicewin-core enhancement::tests::`
- `cargo test -q -p voicewin-appcore preview_prompt_`

## Live Validation

Gateway:

- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- API kind: `responses_sse`
- date: `2026-03-20`

### Screenshot-only probe

Prompt:

- `Read the screenshot and return only the visible word. Preserve casing. Output only the word.`

Transcript:

- `read the screenshot`

Observed after prompt hardening:

- the stricter screenshot block was present in the actual system prompt
- the gateway still produced a wrapper on the final run:
  - `The screenshot appears to contain just one visible word in the center:`
  - `**VoiceWin**`
- final output was still reduced correctly to:
  - `VoiceWin`
- warning:
  - `LLM output looked conversational; VoiceWin stripped assistant framing from the model output.`

Interpretation:

- prompt hardening is directionally correct but not sufficient by itself on this gateway
- the sanitizer remains necessary

### Realistic cleanup probe

Prompt:

- `Fix grammar, punctuation, and capitalization. Use the attached screenshot only to recover the exact product name when helpful. Return only the cleaned dictation.`

Transcript:

- `please ship the voice win update later this week`

Observed final rerun after the prioritization fix:

- raw output:
  - `Please ship the VoiceWin update later this week.`
- final output:
  - `Please ship the VoiceWin update later this week.`
- warning:
  - none

This is the important outcome for product behavior: the stricter screenshot guidance plus improved
salvage ordering did not regress the normal screenshot-assisted cleanup path and recovered the
desired `VoiceWin` spelling.

## Decision

Keep both changes:

- stronger screenshot-specific enhancer prompt rules
- transcript-echo candidate deprioritization during salvage

Reasoning:

1. the prompt guidance is cheap, explicit, and aligned with product intent
2. the gateway still needs sanitizer backup, so prompt-only hardening is not enough
3. better candidate prioritization fixes a real quality bug where VoiceWin could previously stop at a transcript echo and miss a later cleaned answer

## Outcome

VoiceWin now handles screenshot-attached enhancer requests more coherently:

- the prompt more clearly forbids image-description wrappers
- the sanitizer still rescues wrapper-style outputs when the gateway ignores that instruction
- transcript echoes no longer outrank a later cleaned multimodal answer

That leaves one known residual provider risk:

- if the OpenAI-compatible gateway fully deflects and never includes the intended answer anywhere,
  VoiceWin still cannot reconstruct missing information from that response alone

But for the real wrapper and transcript-echo shapes seen on `2026-03-20`, the branch is now
materially better.
