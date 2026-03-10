# Desktop Release Smoke Matrix

## Automated CI Smoke

- **Phase 2A startup smoke, macOS:** build `voicewin-tauri/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/VoiceWin.app`, run `bash scripts/ci/run-macos-bundle-smoke.sh`, and expect `VOICEWIN_SMOKE_OK version=<version> git_sha=<sha>` in `voicewin-tauri/src-tauri/target/ci-smoke/macos/stdout.log`, copied into `voicewin-tauri/src-tauri/target/ci-smoke/macos/voicewin.log`.
- **Phase 2A startup smoke, Windows:** build `voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/VoiceWin.exe` or `voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/voicewin-tauri.exe`, run `pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-bundle-smoke.ps1`, and expect `VOICEWIN_SMOKE_OK version=<version> git_sha=<sha>` in `voicewin-tauri/src-tauri/target/ci-smoke/windows/stdout.log` or `voicewin-tauri/src-tauri/target/ci-smoke/windows/stderr.log`, copied into `voicewin-tauri/src-tauri/target/ci-smoke/windows/voicewin.log`.
- **Phase 2B1 packaged runtime smoke, macOS:** run `bash scripts/ci/run-macos-runtime-smoke.sh`, expect `VoiceWin startup: version=<version> git_sha=<sha>`, `VOICEWIN_RUNTIME_SMOKE_START version=<version> git_sha=<sha>`, and `VOICEWIN_RUNTIME_SMOKE_OK version=<version> git_sha=<sha>` in `voicewin-tauri/src-tauri/target/ci-smoke/macos-runtime/stdout.log`, and require `voicewin-tauri/src-tauri/target/ci-smoke/macos-runtime/textedit-runtime-target.txt` to equal the scripted transcript after TextEdit is re-focused.
- **Phase 2B1 packaged runtime smoke, Windows:** run `pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-runtime-smoke.ps1`, expect `VoiceWin startup: version=<version> git_sha=<sha>`, `VOICEWIN_RUNTIME_SMOKE_START version=<version> git_sha=<sha>`, and `VOICEWIN_RUNTIME_SMOKE_OK version=<version> git_sha=<sha>` in `voicewin-tauri/src-tauri/target/ci-smoke/windows-runtime/stdout.log` or `voicewin-tauri/src-tauri/target/ci-smoke/windows-runtime/stderr.log`, and require `voicewin-tauri/src-tauri/target/ci-smoke/windows-runtime/notepad-runtime-target.txt` to equal the scripted transcript after Notepad is re-focused and saved.
- **Expected process behavior:** startup smoke still exits before normal runtime startup, while packaged runtime smoke continues far enough to build the real service, emit runtime markers, and exit `0` only after the controlled target app receives the transcript.

## Bundle Checks

- **macOS artifact presence:** `.app` and `.dmg` artifacts upload from `.github/workflows/voicewin-macos-unsigned.yml`.
- **Windows artifact presence:** NSIS installer uploads from `.github/workflows/voicewin-windows-portable.yml` and the raw release executable remains available for startup smoke.
- **Bootstrap/resource presence:** Phase 2B1 now proves packaged startup plus deterministic runtime-session initialization. A later native pass should add an explicit bundled bootstrap-model/resource assertion only if release regressions point at packaged resource loading.

## Manual Follow-Up

- **Phase 2B2 macOS permissions:** verify Accessibility and Microphone prompts/settings flow against System Settings, then repeat insertion coverage against TextEdit outside the deterministic transcript override path.
- **Phase 2B2 Windows insertion variants:** verify dictation into Notepad with non-default insert modes (`PasteAndEnter`, `ShiftInsert`) and then exercise stop/cancel flows to check packaged event ordering.
- **macOS launch path:** open the unsigned `.app` outside smoke mode and verify logs remain reachable from the in-app log folder action.
- **Windows launch path:** launch the packaged app outside smoke mode from the built installer or release binary and confirm normal startup from the installed path as well as the raw release executable.
- **Cross-platform follow-up:** confirm future failures that point at packaged resources add a native assertion for bootstrap-model availability instead of relying only on the smoke-lane evidence.
