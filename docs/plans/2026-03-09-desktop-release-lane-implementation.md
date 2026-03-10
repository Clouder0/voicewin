# Desktop Release Lane Implementation Plan

**Goal:** Add a practical phase-2 native release lane that proves VoiceWin can build and launch as a packaged desktop app on macOS and Windows, first with deterministic startup smoke and then with a deterministic packaged runtime-smoke insertion lane against controlled native target apps.

**Architecture:** Reuse the existing native build workflows, keep the narrow startup-smoke seam at the packaged-app boundary, and add a second env-gated packaged runtime-smoke seam that runs a real session through transcript override. Keep the native lane deterministic: assert startup provenance/log markers, verify runtime success markers, and require transcript insertion into TextEdit/Notepad without introducing broad GUI automation.

**Tech Stack:** Rust, Tauri 2, Bun/Vite, GitHub Actions, Bash, PowerShell, macOS `.app` bundles, Windows `.exe` / NSIS artifacts.

---

## Progress

- [x] Task 1: Add a pure startup-smoke helper and unit tests
- [x] Task 2: Wire startup smoke into the Tauri entrypoint
- [x] Task 3: Add macOS and Windows packaged-app smoke scripts
- [x] Task 4: Upgrade native workflows into smoke gates
- [x] Task 5: Add a manual desktop smoke matrix and verification notes

## Execution Notes

- Follow-up runtime-smoke work continues in the isolated worktree branch `desktop-e2e-lane` under `.worktree/desktop-e2e-lane`.
- The startup smoke flag is `VOICEWIN_SMOKE_TEST=1`.
- In smoke mode, the Tauri entrypoint still logs the startup provenance line and also writes both `VoiceWin startup: version=<version> git_sha=<sha>` and `VOICEWIN_SMOKE_OK version=<version> git_sha=<sha>` to process output before exiting early.
- The packaged runtime-smoke flag is `VOICEWIN_RUNTIME_SMOKE_TEST=1`; the packaged app also requires `VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT=<text>` and optionally validates `VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS=<process>`.
- The native smoke scripts now write deterministic CI artifacts under `voicewin-tauri/src-tauri/target/ci-smoke/macos/`, `voicewin-tauri/src-tauri/target/ci-smoke/windows/`, `voicewin-tauri/src-tauri/target/ci-smoke/macos-runtime/`, and `voicewin-tauri/src-tauri/target/ci-smoke/windows-runtime/` by capturing packaged-app process output plus controlled target-app text evidence.
- The Linux PR lane remains unchanged; native startup smoke is layered on top of the existing workflows rather than replacing them.

## Verification Notes

- **Verified locally on Linux:**
  - `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`
  - `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`
  - `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml`
  - `bash scripts/ci/run-pr-checks.sh`
  - `bash -n scripts/ci/run-macos-bundle-smoke.sh`
  - `bash -n scripts/ci/run-macos-runtime-smoke.sh`
  - synthetic macOS bundle checks verified explicit `VoiceWin.app` candidate selection, executable discovery under `Contents/MacOS/`, and smoke-marker capture from process output
  - `bash scripts/ci/run-macos-bundle-smoke.sh` fails cleanly when no macOS `VoiceWin.app` bundle is present
- **Not available in this Linux environment:**
  - `pwsh` is not installed here, so the Windows startup/runtime smoke scripts could not be parsed or executed locally
  - native macOS/Windows packaged-app startup could not be exercised from this container
- **Expected from native CI runners:**
  - `.github/workflows/voicewin-macos-unsigned.yml` builds bundle artifacts, runs both `bash scripts/ci/run-macos-bundle-smoke.sh` and `bash scripts/ci/run-macos-runtime-smoke.sh`, and uploads `voicewin-tauri/src-tauri/target/ci-smoke/macos/*` plus `voicewin-tauri/src-tauri/target/ci-smoke/macos-runtime/*`
  - `.github/workflows/voicewin-windows-portable.yml` builds native artifacts, runs both `pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-bundle-smoke.ps1` and `pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-runtime-smoke.ps1`, and uploads `voicewin-tauri/src-tauri/target/ci-smoke/windows/*` plus `voicewin-tauri/src-tauri/target/ci-smoke/windows-runtime/*`
  - both native runners should capture the startup provenance line before the startup/runtime success markers and should prove deterministic insertion into TextEdit/Notepad by matching the scripted transcript text
- **Still manual / native follow-up:**
  - macOS Accessibility and Microphone permission flows
  - macOS insertion coverage outside the transcript-override runtime smoke path
  - Windows non-default insert-mode coverage and installed-path verification
  - confirmation that packaged resource/bootstrap-model availability should get an explicit native assertion if future failures point there

## Final Verification

Run locally:

- `bash scripts/ci/run-pr-checks.sh`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`
- `bash -n scripts/ci/run-macos-runtime-smoke.sh`

Then run on GitHub-hosted native runners via workflow dispatch:

- `.github/workflows/voicewin-macos-unsigned.yml`
- `.github/workflows/voicewin-windows-portable.yml`

Expected native evidence:

- bundle artifacts exist
- startup smoke marker appears
- runtime smoke marker appears after the startup provenance line
- TextEdit/Notepad contain the scripted transcript after the packaged runtime-smoke run
- smoke logs upload with artifacts
