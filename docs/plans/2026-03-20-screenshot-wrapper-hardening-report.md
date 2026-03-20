# 2026-03-20 Screenshot Wrapper Hardening Report

## Goal

Harden VoiceWin's enhancer post-processing against a multimodal failure mode observed live on the
OpenAI-compatible Responses path:

- the model sometimes returned screenshot-description wrappers instead of the final text itself
- the inserted result should be the recovered answer when the wrapper still contains it
- VoiceWin should surface a warning instead of leaking the wrapper into user text

This is a follow-up slice after the earlier enhancer anti-chat hardening work.

## Failure Mode

Real gateway runs on `2026-03-20` produced raw outputs like:

- `The screenshot appears to contain a single word in the center:`
- followed by `**VoiceWin**`

Earlier sanitizer behavior:

- treated this as normal enhancer output
- left the descriptive wrapper untouched

That is wrong for a dictation app. The wrapper is model chatter; `VoiceWin` is the useful output.

## Implementation

Updated [enhancement.rs](/home/clouder/GitRepos/voicewin/voicewin/voicewin-core/src/enhancement.rs):

- expanded enhancer spill detection to cover screenshot-description phrasing
  - `the screenshot shows`
  - `the screenshot contains`
  - `the text in the image appears to be`
  - `i read the screenshot text as`
  - related descriptive fragments like `single visible word`, `in the center`
- added salvage from markdown-emphasized segments like `**VoiceWin**`
- added salvage from text lines that follow screenshot/meta labels
- normalized outer emphasis / quote wrappers before returning the final text

Updated [service.rs](/home/clouder/GitRepos/voicewin/voicewin/voicewin-appcore/src/service.rs):

- added an appcore preview regression test that exercises the full preview path with a screenshot-aware prompt and wrapped multimodal raw output

## Regression Coverage

Added core tests for these wrapper shapes:

- `The text in the image appears to be: **VoiceWin**`
- `I read the screenshot text as: **“VoiceWin”**.`
- `I also see an image ... appears to read **“VoiceWin”**`
- `The screenshot contains a single visible word in the center: **VoiceWin**`

Added appcore preview regression:

- wrapped multimodal provider output is reduced to `VoiceWin`
- warning is surfaced as:
  - `LLM output looked conversational; VoiceWin stripped assistant framing from the model output.`

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

Prompt used:

- `Read the screenshot and return only the visible word. Preserve casing. Output only the word.`

Transcript:

- `read the screenshot`

Screenshot:

- attached `1280 px` benchmark artifact with centered `VoiceWin`

Observed live result after hardening:

- round latencies: `3291 ms`, `2980 ms`, `3181 ms`
- raw output:
  - `The screenshot appears to contain a single word in the center:`
  - `**VoiceWin**`
- final output:
  - `VoiceWin`
- warning:
  - `LLM output looked conversational; VoiceWin stripped assistant framing from the model output.`

Interpretation:

- the gateway still produced the wrapper
- VoiceWin now recovers the intended answer instead of leaking the wrapper

## Outcome

VoiceWin's multimodal enhancer path is materially safer for screenshot-assisted correction on the
OpenAI-compatible gateway.

The model can still drift into wrapper-style image descriptions, but when the answer is embedded in
that wrapper, VoiceWin now extracts the useful text and preserves a warning trail instead of
inserting the verbose wrapper into the target application.
