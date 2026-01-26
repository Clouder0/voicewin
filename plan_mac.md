
macOS Parity Plan (VoiceWin)

This plan extends VoiceWin to support macOS with capabilities comparable to Windows.
Windows remains the first priority platform (primary UX + fastest fixes). macOS is the
second priority platform, but must be production-grade: the core user flow must work
end-to-end (hotkey -> record -> transcribe -> insert) with clear permission UX.

Scope (repo paths):
- App/UI: `voicewin/voicewin-tauri`
- Core service: `voicewin/voicewin-appcore`
- Audio capture: `voicewin/voicewin-audio`
- Platform integrations (insertion/context): `voicewin/voicewin-platform`

---

## 0) Decisions / Defaults (Locked)

- Windows default hotkey: `Ctrl+Space`.
- macOS default hotkey: `Option+Z`.
  - Stored as `Alt+Z` in shortcut strings (Tauri uses `Alt`).
  - UI displays `Option` on macOS.
- macOS insertion requires Accessibility permission (acceptable and expected).
- Local STT uses Whisper GGUF (same pipeline as Windows).

---

## 1) Current Status (As Implemented)

Already implemented (core macOS parity work is landed; macOS runtime verification is still required):

- Packaging metadata:
  - `voicewin/voicewin-tauri/src-tauri/tauri.conf.json` includes `bundle.targets: ["nsis", "dmg"]`.
  - `voicewin/voicewin-tauri/src-tauri/tauri.conf.json` sets `bundle.macos.infoPlist: "Info.plist"`.
  - `voicewin/voicewin-tauri/src-tauri/Info.plist` contains `NSMicrophoneUsageDescription`.
  - `voicewin/voicewin-tauri/src-tauri/icons/icon.icns` exists and is referenced by config.

- Hotkey parity:
  - `voicewin/voicewin-tauri/src-tauri/src/main.rs` enables `get_toggle_hotkey` + `set_toggle_hotkey` on macOS.
  - Persisted hotkey is registered at startup on macOS.
  - Default hotkey on macOS is `Alt+Z` (Option+Z).
  - `voicewin/voicewin-tauri/src/ui/OverviewPage.tsx` displays `Alt` as `Option` and `Super` as `Cmd` on macOS.
  - Note: Tauri v2 does not expose `@tauri-apps/api/os`, so we use a UA sniff for macOS label display.

- Audio capture parity (Windows + macOS only):
  - `voicewin/voicewin-audio/src/recorder.rs` provides a CPAL-based recorder for Windows + macOS.
  - `voicewin/voicewin-appcore/src/service.rs` enables recording APIs on macOS.
  - `voicewin/voicewin-tauri/src-tauri/src/main.rs` exposes `list_microphones` on macOS.

- Session pipeline + overlay progress:
  - `voicewin/voicewin-tauri/src-tauri/src/session_controller.rs` uses engine stage hooks to drive the overlay.
  - Mic-level events are emitted on Windows + macOS so the visualizer can work.
  - Processing now runs in a background task so Cancel can abort in-flight work.

- Permission UX (partial):
  - `voicewin/voicewin-tauri/src-tauri/src/main.rs` has macOS-only commands:
    - `open_macos_accessibility_settings`
    - `open_macos_microphone_settings`
  - `voicewin/voicewin-tauri/src/ui/Overlay.tsx` shows buttons to open these settings when error text indicates missing permissions.

Known remaining gaps / risks:

- macOS runtime verification is still required (we cannot run a macOS build from this Linux environment).
- The generated `voicewin/voicewin-tauri/src-tauri/icons/icon.icns` should be verified on macOS (it may need regeneration with macOS icon tooling).

---

## 2) Concrete Work Plan (Follow In Order)

### Step A — Fix Build Blockers (Required)

Goal: Linux checks and macOS builds both compile cleanly.

Work:
- Fix `voicewin/voicewin-audio/src/lib.rs` to only declare `resample` once.
  - Keep the public API the same: `AudioRecorder`, `AudioCaptureError`, `CapturedAudio` are only available on Windows + macOS.

Done when:
- `cargo check` succeeds for the workspace on Linux.

Status: Done (Linux `cargo test` + `cargo check` pass).

### Step B — Finish SessionController macOS Parity (Required)

Goal: Cancel behaves correctly on macOS (stop recording via tray, and cancel processing via overlay).

Work:
- In `voicewin/voicewin-tauri/src-tauri/src/session_controller.rs`:
  - Widen the `cancel_recording` recorder-stop path from `#[cfg(windows)]` to `#[cfg(any(windows, target_os = "macos"))]`.
  - Run the session pipeline in a background task after stopping recording, so Cancel can abort processing.
  - Allow starting a new recording from the Success state.

Done when:
- On macOS, pressing Cancel during Recording (tray) stops the recorder and returns the overlay to Cancelled.
- On macOS, pressing Cancel during Transcribing/Enhancing aborts the in-flight pipeline.
- Linux build still works.

Status: Done (verified on Linux builds; macOS runtime verification still pending).

### Step C — UI Polish + Clear Messaging (Nice-to-have, Low Risk)

Work:
- Update stale comments / small UI text where it no longer matches behavior (e.g. mic levels on macOS).

Done when:
- No misleading comments remain in `voicewin/voicewin-tauri/src/ui/Overlay.tsx` about platform behavior.

Status: Done.

### Step D — Linux Verification (Required)

Run in this environment:
- `cargo test` (workspace)
- `cargo check` in `voicewin/voicewin-tauri/src-tauri`
- `bun run test` in `voicewin/voicewin-tauri`
- `bun run build` in `voicewin/voicewin-tauri`

Done when:
- All commands pass.

Status: Done.

### Step E — macOS Manual Verification Checklist (Required for Release)

Run on a macOS machine:

Build:
- `cargo tauri build` (or `bunx tauri build`, depending on local setup)

Functional checks:
- Hotkey: default `Option+Z` toggles recording.
- Microphone permission prompt appears on first recording attempt (requires `NSMicrophoneUsageDescription`).
- Recording produces text (transcription works).
- Insertion:
  - Without Accessibility permission: overlay shows error + offers an "Accessibility" button to settings.
  - After granting Accessibility: insertion works into the foreground app.
- Overlay:
  - Shows Recording -> Thinking... -> Inserted.
  - Stop works; Cancel works during Transcribing/Enhancing.
- History:
  - On insertion failure, output is still recoverable via History.

---

## 3) Acceptance Criteria (macOS "Production-Grade")

- Core flow works end-to-end on macOS: hotkey -> record -> transcribe -> insert.
- Permission failures are actionable (settings deep-links are present).
- No "Windows-only" behavior surprises in the UI (copy + comments match reality).
- Linux dev checks still pass (we do not regress existing workflows).
