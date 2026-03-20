# macOS Clipboard Context Capture Report

Date: 2026-03-19

## Goal

Close one of the remaining phase-2 context gaps from the LLM post-processing research:

- Windows already captured clipboard context.
- macOS still captured only window context.

For VoiceWin's post-processing feature, clipboard context is immediately useful and materially lower
risk than selected-text capture or OCR. This batch adds best-effort clipboard text capture to the
macOS context provider so profiles/prompts that enable clipboard context behave more consistently
across platforms.

## Implementation

Updated:

- `voicewin-platform/src/macos_insert.rs`
- `voicewin-platform/src/macos.rs`

### New helper

Added:

- `read_clipboard_text() -> Option<String>`

Behavior:

- runs on the main queue using the same `run_on_main_queue_sync(...)` mechanism already used for
  other AppKit-sensitive operations,
- reads `NSPasteboard::generalPasteboard().stringForType(NSPasteboardTypeString)`,
- returns `None` for missing or whitespace-only text.

### Context provider change

`MacosContextProvider.snapshot_context()` now returns:

- `window_context`
- `clipboard`

instead of window context alone.

## Verification

### Verified on this host

- `cargo fmt --all -- voicewin-platform/src/macos.rs voicewin-platform/src/macos_insert.rs`
- `cargo check -q -p voicewin-platform`
- `cargo test -q -p voicewin-platform`

All passed on the Linux development host.

### Additional macOS-target verification attempt

I also installed the Rust target:

- `rustup target add x86_64-apple-darwin`

Then attempted:

- `cargo check -q -p voicewin-platform --target x86_64-apple-darwin`

That did **not** complete because the Linux host lacks a macOS cross-compilation SDK/toolchain for
C dependencies (`ring` failed looking for `TargetConditionals.h`).

I retried once with:

- `CC=clang cargo check -q -p voicewin-platform --target x86_64-apple-darwin`

but the same host-boundary remained: no macOS SDK headers.

## Confidence

This change is still grounded rather than guessed:

1. it reuses the same AppKit/objc2 bindings already present in the repo,
2. the exact binding signatures were checked from the installed `objc2-app-kit` crate sources,
3. the helper follows the existing main-thread discipline already established in
   `macos_insert.rs`.

## Product impact

This improves VoiceWin's LLM post-processing behavior on macOS immediately:

- prompts using clipboard context now get real clipboard text on macOS too,
- global/profile context toggles behave more consistently across macOS and Windows,
- this narrows a concrete product gap without pretending OCR or selected-text editing is finished.

## Remaining context gaps

Still deferred:

- selected-text capture on macOS
- selected-text capture on Windows
- screenshot / OCR / multimodal context
- custom vocabulary source plumbing

Those remain separate, larger batches.
