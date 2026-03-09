# Desktop Release Lane Implementation Plan

**Goal:** Add a practical phase-2 native release lane that proves VoiceWin can build and launch as a packaged desktop app on macOS and Windows, with a deterministic startup smoke mode and workflow-level artifact checks.

**Architecture:** Reuse the existing native build workflows, but add one narrow startup-smoke seam at the packaged-app boundary instead of introducing broad GUI automation. Keep the first native batch deterministic: build the bundle, launch it in smoke mode, assert startup provenance/log markers, and exit cleanly. Leave permission/insertion E2E for a later batch with a controlled target app.

**Tech Stack:** Rust, Tauri 2, Bun/Vite, GitHub Actions, Bash, PowerShell, macOS `.app` bundles, Windows `.exe` / NSIS artifacts.

---

## Progress

- [x] Task 1: Add a pure startup-smoke helper and unit tests
- [x] Task 2: Wire startup smoke into the Tauri entrypoint
- [x] Task 3: Add macOS and Windows packaged-app smoke scripts
- [x] Task 4: Upgrade native workflows into smoke gates
- [x] Task 5: Add a manual desktop smoke matrix and verification notes

## Execution Notes

- Work continues in the isolated worktree branch `desktop-release-lane` under `.worktree/desktop-release-lane`.
- The startup smoke flag is `VOICEWIN_SMOKE_TEST=1`.
- In smoke mode, the Tauri entrypoint still logs the startup provenance line and also writes both `VoiceWin startup: version=<version> git_sha=<sha>` and `VOICEWIN_SMOKE_OK version=<version> git_sha=<sha>` to process output before exiting early.
- The native smoke scripts write deterministic CI artifacts under `voicewin-tauri/src-tauri/target/ci-smoke/macos/` and `voicewin-tauri/src-tauri/target/ci-smoke/windows/` by capturing packaged-app process output.
- The Linux PR lane remains unchanged; native startup smoke is layered on top of the existing workflows rather than replacing them.

## Verification Notes

- **Verified locally on Linux:**
  - `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`
  - `bash scripts/ci/run-pr-checks.sh`
  - `bash -n scripts/ci/run-macos-bundle-smoke.sh`
  - synthetic macOS bundle checks verified explicit `VoiceWin.app` candidate selection, executable discovery under `Contents/MacOS/`, and smoke-marker capture from process output
  - `bash scripts/ci/run-macos-bundle-smoke.sh` fails cleanly when no macOS `VoiceWin.app` bundle is present
- **Not available in this Linux environment:**
  - `pwsh` is not installed here, so the Windows smoke script could not be parsed or executed locally
  - native macOS/Windows packaged-app startup could not be exercised from this container
- **Expected from native CI runners:**
  - `.github/workflows/voicewin-macos-unsigned.yml` builds bundle artifacts, runs `bash scripts/ci/run-macos-bundle-smoke.sh`, and uploads `voicewin-tauri/src-tauri/target/ci-smoke/macos/*`
  - `.github/workflows/voicewin-windows-portable.yml` builds native artifacts, runs `pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-bundle-smoke.ps1`, and uploads `voicewin-tauri/src-tauri/target/ci-smoke/windows/*`
  - both native runners should capture the startup provenance line and smoke marker from packaged-app process output before the app exits successfully
- **Still manual / native follow-up:**
  - macOS Accessibility and Microphone permission flows
  - TextEdit insertion verification on macOS
  - Notepad insertion verification on Windows
  - confirmation that packaged resource/bootstrap-model availability should get an explicit native assertion if future failures point there

## Final Verification

Run locally:

- `bash scripts/ci/run-pr-checks.sh`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`

Then run on GitHub-hosted native runners via workflow dispatch:

- `.github/workflows/voicewin-macos-unsigned.yml`
- `.github/workflows/voicewin-windows-portable.yml`

Expected native evidence:

- bundle artifacts exist
- startup smoke marker appears
- app exits successfully in smoke mode
- smoke logs upload with artifacts
