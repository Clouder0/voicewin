
## Context

The first testing-foundation phase is now strong where this Linux environment is strong: fast PR checks, provider/runtime contract coverage, opt-in live-provider smoke, frontend regression coverage, session-controller orchestration coverage, and service-layer persistence/error-path checks. The next remaining risk is release-only behavior on real desktop targets. The repository already has native build workflows for unsigned macOS bundles and Windows installers, but they are still mostly artifact-build lanes rather than true smoke gates.

## Goals

- Turn the existing macOS and Windows build workflows into meaningful desktop smoke gates.
- Verify that packaged/native VoiceWin artifacts can build, launch, emit startup provenance, and exit cleanly in a controlled smoke mode.
- Keep the production seam narrow and reversible.
- Leave full permission/insertion E2E as a documented second step rather than mixing it into the first native lane.

## Non-Goals

- Full GUI automation across arbitrary target apps in this first native batch.
- Automating macOS TCC permission dialogs on GitHub-hosted runners.
- Notarization/signing rollout, release publishing, or store/distribution plumbing.
- Replacing the current Linux PR lane.

## Recommended Architecture

Use a staged native release lane:

1. **Native build + bundle verification**: keep the existing macOS and Windows workflows, but treat them as the base of a release-gate lane rather than tag-only artifact builders.
2. **Packaged startup smoke**: add one small startup-smoke seam in `voicewin-tauri/src-tauri/src/main.rs` (or a tiny helper module it calls) so the bundled app can be launched with an environment variable such as `VOICEWIN_SMOKE_TEST=1`, log version/git SHA and a clear success marker, verify bootstrap/resource paths, then exit successfully before recorder/insertion flows begin.
3. **Workflow-level smoke scripts**: add platform-specific CI scripts that locate the built `.app` / `.exe`, launch it in smoke mode, capture stdout/stderr or redirected logs, and fail if the smoke marker never appears.
4. **Manual/native follow-up lane**: document a separate permission/insertion matrix for TextEdit/Notepad-style target apps. Keep this outside the first smoke batch so the initial native lane stays deterministic and reviewable.

This keeps the functional core unchanged and adds only one thin imperative seam at process startup, which is the right boundary for packaged-app verification.

## Files In Scope

- Create: `docs/plans/2026-03-09-desktop-release-lane-implementation.md`
- Create: `scripts/ci/run-macos-bundle-smoke.sh`
- Create: `scripts/ci/run-windows-bundle-smoke.ps1`
- Create: `docs/testing/desktop-release-smoke-matrix.md`
- Modify: `.github/workflows/voicewin-macos-unsigned.yml`
- Modify: `.github/workflows/voicewin-windows-portable.yml`
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Optional create/modify: `voicewin-tauri/src-tauri/src/startup_smoke.rs`

## Verification Strategy

- Linux-safe unit checks for the startup-smoke helper logic in `voicewin-tauri/src-tauri/`.
- Native workflow smoke on GitHub-hosted runners:
  - macOS: build unsigned `.app` / `.dmg`, launch packaged app in smoke mode, assert startup marker and git SHA/version marker.
  - Windows: build installer/binary, launch packaged executable in smoke mode, assert startup marker and clean exit.
- Keep the existing baseline lane (`bash scripts/ci/run-pr-checks.sh`) unchanged and green.

## Recommended Batch Breakdown

Phase 2A should be **native build/startup smoke** only.

- Why first: it is deterministic, CI-friendly, and directly addresses release-only regressions.
- Why not permission/insertion yet: those flows are platform- and environment-dependent, especially on macOS, and deserve a separate test matrix plus likely partial manual QA.

Phase 2B should be **permission/insertion E2E and controlled target-app verification**.

- macOS: TextEdit/System Settings permission path + paste verification.
- Windows: Notepad target-app insertion + recorder/stop/cancel event ordering.

## Open Follow-Up After This Batch

- Add controlled target-app insertion automation where practical.
- Add permission failure assertions for packaged builds.
- Decide whether native smoke becomes nightly, release-only, or required before tagging.
