# Linux Platform Fallback Refinement Report

Date: 2026-03-20

## Scope

Refined the Linux runtime path so VoiceWin no longer uses the synthetic test provider in the Tauri app on non-Windows/non-macOS hosts.

This batch also made visual-context degradation explicit when a screenshot was requested but no screenshot or precomputed OCR artifact existed.

## Changes

### 1. Replaced the Linux Tauri fallback provider

Updated the Linux Tauri wiring to use:

- `voicewin_platform::linux::LinuxContextProvider`
- `voicewin_platform::linux::LinuxInserter`

instead of:

- `TestContextProvider`
- `StdoutInserter`

### 2. Added a real Linux platform module

Implemented:

- clipboard-backed context capture
- clipboard-backed insertion
- explicit logs for unsupported Linux platform capabilities

Current Linux behavior:

- clipboard context: best-effort via `arboard`
- insertion: copies final text to the clipboard and logs that paste is still manual
- selected text capture: not implemented
- foreground app lookup: not implemented
- window context capture: not implemented
- screenshot capture: not implemented

This is intentionally honest. Linux now degrades explicitly instead of silently behaving like a test harness.

### 3. Added explicit visual capture unavailability warnings

Added a shared helper:

- `visual_context_capture_unavailable_warning(...)`

Used in:

- `voicewin-engine`
- `voicewin-appcore`

Behavior:

- if visual dispatch resolved to `screenshot` or `ocr`
- and no screenshot / precomputed OCR input was actually available
- VoiceWin now surfaces a user-visible warning instead of only logging the miss

Example warning:

- `Visual context capture did not produce a screenshot (dispatch=screenshot capture_scope=foreground_window); continuing without visual context.`

### 4. Added Linux UI guidance

Updated `OverviewPage` so Linux users now see:

- clipboard-only insert guidance (`Ctrl+V`)
- a note that profile matching / visual context remain best-effort on Linux for now

## Verification

Passed locally:

- `cargo fmt --all`
- `cargo test -q -p voicewin-platform -p voicewin-core -p voicewin-engine -p voicewin-appcore`
- `npm test -- OverviewPage.linux.test.tsx OverviewPage.accessibility.test.tsx OverviewPage.hotkey.test.tsx OverviewPage.microphone.test.tsx`
- `npm run typecheck` in `voicewin-tauri`
- `env CARGO_HOME=/tmp/voicewin-cargo-home cargo check -q` in `voicewin-tauri/src-tauri`

Note:

- the Tauri Rust check needed a temporary writable `CARGO_HOME` in this sandbox because the shared Cargo registry cache is read-only here.

## Remaining Linux constraints

- no native Linux screenshot capture yet
- no native Linux selected-text capture yet
- no native Linux active-window identity yet
- Linux insertion is still clipboard-only rather than synthetic paste

Those are now explicit product constraints rather than hidden behavior.
