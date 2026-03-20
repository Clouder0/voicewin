# File-Backed Custom Vocabulary Source Report

Date: 2026-03-19

## Goal

Finish the next low-risk phase-2 LLM post-processing improvement without adding more config
schema churn: make custom vocabulary practical by loading it from a file beside the existing
VoiceWin app data files.

## Decision

Use a file-backed source:

- file name: `custom_vocabulary.txt`
- location: same directory as `config.json`, `history.json`, and `secrets.json`
- UX shape: no new picker, no new storage schema, no extra migration surface

This keeps the product surface simple while still unlocking the most common "teach the model my
product names / team names / jargon" workflow.

## Implemented Behavior

### Source loading

`voicewin-appcore::AppService` now loads `custom_vocabulary.txt` from the app data directory.

### Normalization

Loaded vocabulary is normalized before use:

- trim each line
- drop empty lines
- dedupe while preserving first-seen order
- cap final payload at `32 KiB`

### Merge behavior

The file-backed vocabulary is merged into `ContextSnapshot.custom_vocabulary`.

Order is:

1. already-present snapshot vocabulary
2. file-backed vocabulary

That means existing in-memory/context-provider vocabulary stays first, and file terms are appended
only when they add something new.

### Failure mode

Unreadable or malformed file access does **not** fail preview or session execution. VoiceWin logs a
warning and continues without the file-backed overlay. Missing file is treated as normal.

### Coverage in product flows

The overlay is now applied in all relevant paths:

- prompt preview
- prepared session capture
- live engine session execution

So the same vocabulary source reaches both preview UX and real runtime enhancement.

## Supporting Tooling

`voicewin-appcore/examples/live_prompt_preview.rs` now accepts:

- `VOICEWIN_LIVE_CUSTOM_VOCABULARY`

When set, the example writes `custom_vocabulary.txt` beside the generated config and exercises the
real appcore preview path end-to-end.

## Tests Added

Added appcore coverage for:

- preview includes file-backed custom vocabulary when enabled
- prepared session context merges file-backed vocabulary
- normalization trims and dedupes
- normalization respects the max-size cap

## Verification

### Local verification

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-appcore`
- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`
- `cargo check -q && cargo test -q` in `voicewin-tauri/src-tauri`
- `npm test` in `voicewin-tauri`
- `npm run typecheck` in `voicewin-tauri`

### Live end-to-end preview validation

Provider:

- OpenAI-compatible Responses SSE
- base URL: provided gateway
- model: `gpt-5.4`
- reasoning effort: `low`

#### Run A: custom vocabulary enabled

- `custom_vocabulary_enabled=true`
- custom vocabulary:
  - `VoiceWin`
  - `ElevenLabs`
  - `Scribe v2`
- transcript:
  - `voice win uses eleven labs scribe v2 for dictation cleanup`

Observed:

- total preview elapsed: `4906 ms`
- first token: `4782 ms`
- system prompt contained `<CUSTOM_VOCABULARY>`
- final output:
  - `VoiceWin uses ElevenLabs Scribe v2 for dictation cleanup.`

#### Run B: custom vocabulary disabled

Same provider/model, same transcript, same preview flow, but no file-backed vocabulary.

Observed:

- total preview elapsed: `6138 ms`
- first token: `6076 ms`
- final output:
  - `VoiceWin uses ElevenLabs Scribe v2 for dictation cleanup.`

## Latency Takeaway

The new local work is just:

- reading at most `32 KiB`
- trimming/deduping lines
- merging a short string into the context snapshot

Live runs showed no sign of added end-to-end latency; the custom-vocabulary-enabled run was
actually faster than the control run, which indicates network/model variance dominates this path.
So this feature is below the practical latency noise floor relative to multi-second cloud
inference.

## Files Touched

- `voicewin-appcore/src/service.rs`
- `voicewin-appcore/examples/live_prompt_preview.rs`
- `voicewin-tauri/src/ui/SettingsPage.tsx`
- `voicewin-tauri/src/ui/ProfilesPage.tsx`

## Conclusion

File-backed custom vocabulary is now productized enough for the current phase:

- simple UX
- no schema churn
- works in preview and real sessions
- tested locally
- validated live against the real gateway
