# Prompt Preview Sample Context Report

Date: 2026-03-19

## Goal

Make the prompt preview surface usable for context-heavy prompts without requiring the user to set
up a real live foreground app selection every time.

This especially matters now that VoiceWin ships:

- selected-text capture
- selection-edit built-in prompts
- richer prompt library workflows

Without sample context overrides, prompt preview remains awkward for the exact flows we just added.

## Implemented behavior

### IPC / backend

`PromptPreviewRequest` now supports optional `context_override` with:

- `clipboard`
- `selected_text`
- `window_context`

When present, the Tauri backend:

1. captures the live app identity
2. captures the live context snapshot
3. overlays any provided sample context fields
4. runs preview through the existing appcore preview path

This keeps preview behavior realistic while letting the user override only the specific context they
want to test.

### Appcore

Added `AppService::capture_context_snapshot()` as a small public seam for preview orchestration.

### Frontend

The Prompt Library preview panel now exposes:

- `Sample selected text`
- `Sample clipboard text`
- `Sample window context`

These fields:

- affect preview only
- do not mutate saved config
- do not affect runtime dictation sessions

## Why this design

I intentionally kept the override surface narrow:

- no new saved config schema
- no full fake app identity editor
- no runtime behavior changes

This is a prompt-lab affordance, not a second configuration system.

## Tests

Added a frontend test ensuring preview requests include manual sample context overrides.

Passed:

- `cargo fmt --all`
- `cargo test -q -p voicewin-runtime`
- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`
- `cargo check -q && cargo test -q` in `voicewin-tauri/src-tauri`
- `npm test` in `voicewin-tauri`
- `npm run typecheck` in `voicewin-tauri`

## Product outcome

The prompt library is now much more useful for iterating on sophisticated post-processing prompts:

- selection-edit prompts can be tested directly
- clipboard-aware prompts can be tested directly
- window-context prompts can be tested directly

That reduces friction for exactly the workflows VoiceWin is now starting to support seriously.

## Files touched

- `voicewin-runtime/src/ipc.rs`
- `voicewin-appcore/src/service.rs`
- `voicewin-tauri/src-tauri/src/main.rs`
- `voicewin-tauri/src/lib/types.ts`
- `voicewin-tauri/src/ui/PromptsPage.tsx`
- `voicewin-tauri/src/ui/PromptsPage.test.tsx`
