# VoiceWin 0.1.4 macOS Insert Crash Hotfix Notes

## What this hotfix addresses

This `0.1.4` hotfix targets a macOS crash that could happen immediately after recording completed, during the text insertion step.

It supersedes the earlier `0.1.1` / `0.1.2` / `0.1.3` cuts after fixing the macOS CI linkage regression exposed by the new macOS-only insert tests.

The root cause was thread affinity: the session pipeline runs on a background Tokio task, but macOS pasteboard and related HIToolbox/AppKit calls must execute on the app's main-thread/main-queue boundary. The crash was reproducible from a stack ending in `dispatch_assert_queue` / `TSMGetInputSourceProperty` during the insert phase.

## What changed

- `voicewin-platform/src/macos_insert.rs`
  - Refactors insertion into short main-thread phases for pasteboard write and restore.
  - Routes background callers through `run_on_main_queue_sync()` instead of touching AppKit inline from the Tokio worker.
  - Keeps delays and CGEvent posting off the main thread to avoid a visible UI stall.
  - Adds debug assertions so main-thread-only operations fail loudly in debug builds.
  - Adds structured macOS insert-phase logging.
- `voicewin-tauri/src-tauri/build.rs`
  - Injects a best-effort git SHA into the build.
- `voicewin-tauri/src-tauri/src/main.rs`
  - Logs app startup with version and git SHA.

## Expected log markers

At startup:

```text
VoiceWin startup: version=<version> git_sha=<sha>
```

During a successful insert:

```text
macOS insert phase=start
macOS insert phase=pasteboard_written
macOS insert phase=layout_switch_attempt
macOS insert phase=paste_posted
macOS insert phase=restore outcome=<restored|skipped_changed|skipped_snapshot_unavailable>
```

For `PasteAndEnter`, expect an additional:

```text
macOS insert phase=enter_posted
```

## Verification completed in this environment

- `cargo test` at repo root
- `cargo test` in `voicewin-tauri/src-tauri`
- `cargo check` in `voicewin-tauri/src-tauri`
- `bun run test` in `voicewin-tauri`
- `bun run build` in `voicewin-tauri`

Note: the macOS-only `run_on_main_queue_sync_hops_from_worker_thread` regression test is intentionally ignored until it can run under a real macOS main-queue environment; it is not a safe Linux proof point.

## Verification still required on macOS before release

1. Build the app bundle on macOS.
2. Confirm startup logs show the expected git SHA.
3. Validate paste into a foreground app with:
   - default ABC layout
   - a non-US keyboard layout
   - `PasteAndEnter`
4. Validate the clipboard restore behavior when the clipboard changes during the restore window.
5. Validate the Accessibility-disabled path still shows the expected user guidance without crashing.
6. Run repeated record/stop cycles to confirm there is no post-recording crash regression.

## Release note text

Fixed a macOS crash that could happen right after recording finished, while VoiceWin pasted text back into the target app.

## Support checklist for first field reports

Collect:

- app version
- git SHA from startup log
- macOS version
- target app name
- keyboard layout in use
- whether Accessibility was enabled
- the last `macOS insert phase=...` log line before any failure
