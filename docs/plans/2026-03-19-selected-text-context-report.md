# Selected Text Context Report

Date: 2026-03-19

## Goal

Move selected-text context from "supported by the prompt builder but not really captured" into a
real best-effort runtime source.

This matters because selected text is the bridge from simple dictation cleanup to real edit flows:

- shorten this paragraph
- rewrite the selected sentence
- adjust tone of the selected text

Without selected-text capture, those flows are structurally weaker.

## Scope decision

I kept this batch bounded:

- implement best-effort platform capture where we can do it with low additional risk
- validate the prompt path end-to-end with the real provider
- do not expand into OCR or multimodal work here

## Implemented behavior

### macOS

Added best-effort selected-text capture via Accessibility:

- query the focused UI element from the system-wide accessibility tree
- read `AXSelectedText`
- return `None` when:
  - Accessibility trust is unavailable
  - there is no focused text element
  - there is no non-empty selection

Important properties:

- no clipboard mutation
- no synthetic copy shortcut
- no extra user-visible side effect

### Windows

Added a conservative best-effort selected-text path for classic editable controls:

- inspect the focused GUI control
- restrict to likely text hosts:
  - `Edit`
  - `RichEdit*`
  - `Scintilla`
- read selection range with `EM_GETSEL`
- read text with `WM_GETTEXT`
- slice the selected UTF-16 range

This is intentionally conservative. It will not cover every modern app surface, but it gives
VoiceWin a real selected-text source in standard native editing environments without pulling in a
larger UI Automation/COM stack in the same batch.

## Wiring

`ContextSnapshot.selected_text` is now populated by:

- `MacosContextProvider`
- `WindowsContextProvider`

The rest of the stack was already ready for this field, so no schema churn was needed.

## Prompt-path validation work

Added appcore test coverage for selected-text preview behavior and extended the live preview example
to accept:

- `VOICEWIN_LIVE_SELECTED_TEXT`

This lets us validate the full enhancement prompt path against the real provider even from a Linux
host, independent of local macOS/Windows capture mechanics.

## Tests and verification

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-appcore`
- `cargo check -q -p voicewin-platform`
- `cargo test -q -p voicewin-platform`
- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`

### Cross-target compile verification

I attempted:

- `cargo check -q -p voicewin-platform --target x86_64-pc-windows-msvc`
- `cargo check -q -p voicewin-platform --target x86_64-apple-darwin`

Both were blocked before meaningful Rust validation by missing foreign C toolchain support in a
transitive `ring` build step:

- Windows: missing `lib.exe`
- macOS: host `cc` cannot handle `-arch` / `-mmacosx-version-min`

So the Rust source is formatted and host-verified, but full target-lane compile validation still
needs a proper Windows/macOS cross toolchain or native runner.

## Live end-to-end validation

Provider:

- OpenAI-compatible Responses SSE
- base URL: `https://cc2.caaa.tech/v1`
- model: `gpt-5.4`
- reasoning effort: `low`

### Run A: selected text enabled

Prompt instruction:

- explicitly told the model to rewrite `CURRENTLY_SELECTED_TEXT`
- transcript acts as the edit instruction

Inputs:

- transcript: `make it shorter and crisper`
- selected text:
  - `Please ship the VoiceWin beta update using ElevenLabs Scribe v2 later this week if possible.`
- custom vocabulary:
  - `VoiceWin`
  - `ElevenLabs`
  - `Scribe v2`

Observed:

- total preview latency: `7450 ms`
- first token latency: `7052 ms`
- system prompt contained `<CURRENTLY_SELECTED_TEXT>`
- final output:
  - `Ship the VoiceWin beta update with ElevenLabs Scribe v2 later this week.`

### Run B: selected text disabled control

Same prompt and transcript, but no selected text provided.

Observed:

- total preview latency: `12954 ms`
- first token latency: `12936 ms`
- model responded with a fallback asking for the missing text instead of performing the edit

This is exactly the product signal we needed:

- selected text materially changes capability, not just prompt size
- without it, edit-style prompts degrade badly

### Prompt-shape finding

An earlier weaker prompt caused `gpt-5.4` to ignore the selected-text context and ask for the text
again even when the selection was present.

After tightening the instruction to:

- explicitly rewrite `CURRENTLY_SELECTED_TEXT`
- treat transcript as edit instruction
- never ask for the text again

the model behaved correctly.

Implication:

- selected-text flows should ship with stronger prebuilt prompt templates than generic cleanup flows

## Files touched

- `voicewin-platform/src/macos.rs`
- `voicewin-platform/src/macos_insert.rs`
- `voicewin-platform/src/windows.rs`
- `voicewin-appcore/src/service.rs`
- `voicewin-appcore/examples/live_prompt_preview.rs`

## Conclusion

Selected text is now a real runtime context source instead of dead schema.

The batch is useful even with the current best-effort limits because:

- the prompt path is fully wired
- the real provider behavior was measured
- the capability difference is concrete and large

The next quality step for this area would be:

- stronger built-in selection-edit prompt templates
- broader Windows coverage via UI Automation if needed
