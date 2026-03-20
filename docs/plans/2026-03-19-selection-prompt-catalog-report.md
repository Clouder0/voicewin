# Selection Prompt Catalog Report

Date: 2026-03-19

## Goal

Now that selected-text context is a real runtime source, the shipped prompt catalog needs to make
that capability usable. The previous built-in prompts were strong for cleanup/rewrite/email/chat,
but weak for selection-edit flows.

## Problem found

Just adding more prompts to `default_prompt_templates()` would not be enough because existing users
would not receive them. The previous backfill behavior only populated prompts when the catalog was
completely empty.

## Implemented change

### New built-in prompts

Added these built-in prompts:

- `Edit Selection`
- `Shorten Selection`
- `Explain Selection`

All three use `PromptMode::Assistant`.

They explicitly say:

- `CURRENTLY_SELECTED_TEXT` is the source text
- the transcript is the instruction
- the model must not ask for the text again

That wording was chosen based on live failures and live retries against the real provider, not just
intuition.

### Backfill migration

Updated `backfill_default_prompts()` so it now:

- still initializes the full built-in catalog when empty
- also appends any missing built-in prompts to non-empty existing catalogs
- avoids duplicating already-present built-ins by title

This means existing VoiceWin users will actually get the new selection-edit prompts.

## Validation cycle

### First live attempt

An earlier `Shorten Selection` wording was too soft. Against the real `gpt-5.4` provider, it
drifted into explanation mode and asked for the text again.

That failure was useful: it showed the prompt needed to state more explicitly that:

- the transcript is **not** the source text
- `CURRENTLY_SELECTED_TEXT` **is** the source text

### Final live validation

Provider:

- OpenAI-compatible Responses SSE
- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- reasoning effort: `low`

Prompt text used:

- the final shipped `Shorten Selection` wording

Inputs:

- transcript: `make it feel more decisive`
- selected text:
  - `Please ship the VoiceWin beta update using ElevenLabs Scribe v2 later this week if possible.`
- custom vocabulary:
  - `VoiceWin`
  - `ElevenLabs`
  - `Scribe v2`

Observed:

- total preview latency: `7033 ms`
- first token latency: `6985 ms`
- final output:
  - `Please ship the VoiceWin beta update with ElevenLabs Scribe v2 later this week.`

This is the behavior we want: the model edited the selected text directly instead of explaining the
task or asking for the text again.

## Tests

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-runtime`
- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`
- `cargo check -q && cargo test -q` in `voicewin-tauri/src-tauri`

## Files touched

- `voicewin-runtime/src/defaults.rs`

## Product outcome

VoiceWin now ships a more coherent prompt catalog for post-processing:

- cleanup/rewrite/email/chat for transcript-first flows
- selection-edit prompts for edit/assistant flows

And the migration path means this improvement reaches existing users instead of only fresh installs.
