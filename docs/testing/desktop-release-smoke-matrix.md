# Desktop Release Smoke Matrix

## Automated CI Smoke

- **macOS unsigned bundle:** build `voicewin-tauri/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/VoiceWin.app`, run `bash scripts/ci/run-macos-bundle-smoke.sh`, and expect `VOICEWIN_SMOKE_OK version=<version> git_sha=<sha>` in `voicewin-tauri/src-tauri/target/ci-smoke/macos/stdout.log`, copied into `voicewin-tauri/src-tauri/target/ci-smoke/macos/voicewin.log`.
- **Windows portable/bundle build:** build `voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/VoiceWin.exe` or `voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/voicewin-tauri.exe`, run `pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-bundle-smoke.ps1`, and expect `VOICEWIN_SMOKE_OK version=<version> git_sha=<sha>` in `voicewin-tauri/src-tauri/target/ci-smoke/windows/stdout.log` or `voicewin-tauri/src-tauri/target/ci-smoke/windows/stderr.log`, copied into `voicewin-tauri/src-tauri/target/ci-smoke/windows/voicewin.log`.
- **Startup provenance marker:** both native lanes should emit `VoiceWin startup: version=<version> git_sha=<sha>` before the smoke success marker in process output.
- **Expected process behavior:** the packaged app exits with code `0` in smoke mode and does not continue into recorder, tray, hotkey, or normal background runtime startup.

## Bundle Checks

- **macOS artifact presence:** `.app` and `.dmg` artifacts upload from `.github/workflows/voicewin-macos-unsigned.yml`.
- **Windows artifact presence:** NSIS installer uploads from `.github/workflows/voicewin-windows-portable.yml` and the raw release executable remains available for startup smoke.
- **Bootstrap/resource presence:** this batch proves packaged startup and smoke-output wiring. A later native pass should add an explicit bundled bootstrap-model/resource assertion if release regressions point at packaged resource loading.

## Manual Follow-Up

- **macOS launch path:** open the unsigned `.app`, confirm the app opens normally outside smoke mode, and verify logs are reachable from the in-app log folder action.
- **macOS permissions:** verify Accessibility and Microphone prompts/settings flow against System Settings, then run a controlled insertion test against TextEdit.
- **Windows launch path:** launch the packaged app outside smoke mode from the built installer or release binary and confirm normal startup.
- **Windows insertion path:** verify dictation into Notepad, then exercise stop/cancel flows to check packaged event ordering.
- **Cross-platform target:** confirm the packaged app can still reach the bundled bootstrap model and complete a real transcription session after the smoke-only startup path.
