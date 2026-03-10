# Desktop Runtime Smoke Design

## Context

The first testing-foundation phase is already strong where this Linux environment is strong: fast PR checks, provider/runtime coverage, frontend regression coverage, and Linux-safe orchestration coverage. Phase 2A also added packaged startup smoke for macOS and Windows, proving the bundled app can launch, emit provenance, and exit cleanly in a controlled startup-only mode.

The next remaining product risk is **runtime behavior after packaged startup**: can the packaged app initialize its real service/runtime path, resolve the bundled bootstrap model, target the real foreground app, and perform a deterministic insertion flow on native desktop targets without relying on live audio or provider networking.

## Goals

- Add a deterministic packaged **runtime session smoke** for macOS and Windows.
- Reuse the existing transcript-override seam so native runtime smoke does not depend on microphone capture or live STT.
- Verify insertion into one controlled native target app per platform:
  - macOS: TextEdit
  - Windows: Notepad
- Keep the production seam narrow, env-gated, and reversible.
- Preserve the current startup-smoke lane as the fast packaged preflight.

## Non-Goals

- Full microphone-permission automation on CI runners.
- Full GUI automation across arbitrary apps.
- Replacing the current startup-smoke scripts.
- Broad new desktop-testing infrastructure/frameworks.
- NSIS-installed-path validation in the first runtime batch.

## Recommended Architecture

Use a staged **Phase 2B1 deterministic packaged runtime smoke** lane.

1. **Add a narrow env-gated runtime-smoke mode in Tauri**
   - Parse a dedicated runtime-smoke env flag plus required transcript input.
   - Launch after normal packaged startup rather than taking the existing early-exit startup-smoke path.
   - Reuse `AppService::run_session_with_hook(...)` with `RunSessionRequest.transcript` so the packaged app runs its real post-startup session pipeline without relying on audio/STT.
   - Emit structured runtime-smoke process output markers for preflight, stage progress, success, and failure.

2. **Use a short target-app settle/focus window**
   - The packaged app may briefly become foreground when launched.
   - Runtime smoke should therefore wait a small fixed delay before capturing the foreground app and starting the session.
   - Support an optional expected-process env input so the app fails loudly if the wrong window is focused.

3. **Add platform-native runtime-smoke scripts**
   - macOS script prepares/focuses TextEdit, launches the packaged app in runtime-smoke mode, re-focuses TextEdit, then asserts both process-output markers and inserted document text.
   - Windows script prepares/focuses Notepad, launches the packaged app in runtime-smoke mode, re-focuses Notepad, then asserts both process-output markers and inserted file contents.
   - Keep both scripts explicit and artifact-path-driven, not framework-heavy.

4. **Upgrade the native workflows to run both startup smoke and runtime smoke**
   - Startup smoke remains the packaged preflight.
   - Runtime smoke becomes the first real packaged-session gate.
   - Upload dedicated runtime smoke logs/artifacts.

5. **Keep permissions and richer insert-mode coverage as the next layer**
   - Phase 2B1 proves packaged runtime initialization + insertion into a real target app.
   - Phase 2B2 remains manual/expanded coverage for:
     - macOS Accessibility + Microphone permission matrix
     - Windows alternate insert modes and installed-path validation
     - richer stop/cancel packaged timing scenarios if needed

This keeps the change concentrated at the right boundary: the packaged app process itself. The functional core stays unchanged; only a thin imperative shell is added to drive an already-existing transcript-override path under controlled native conditions.

## Files In Scope

- Create: `docs/plans/2026-03-10-desktop-runtime-smoke-implementation.md`
- Create: `scripts/ci/run-macos-runtime-smoke.sh`
- Create: `scripts/ci/run-windows-runtime-smoke.ps1`
- Create: `voicewin-tauri/src-tauri/src/runtime_smoke.rs`
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Modify: `.github/workflows/voicewin-macos-unsigned.yml`
- Modify: `.github/workflows/voicewin-windows-portable.yml`
- Modify: `docs/testing/desktop-release-smoke-matrix.md`
- Modify: `docs/plans/2026-03-09-desktop-release-lane-implementation.md`

## Verification Strategy

- Linux-safe unit tests for the new `runtime_smoke` helper logic.
- Existing local baseline stays green via `bash scripts/ci/run-pr-checks.sh`.
- Local bash syntax/basic checks for the macOS runtime smoke script.
- Static inspection / optional PowerShell parse check for the Windows runtime smoke script where `pwsh` is available.
- Native workflow verification on GitHub-hosted runners:
  - macOS: startup smoke + TextEdit runtime smoke
  - Windows: startup smoke + Notepad runtime smoke

## Recommended Batch Breakdown

### Phase 2B1 — Deterministic packaged runtime smoke

- Reuse transcript override.
- Reuse packaged artifacts from the native workflows.
- Validate one controlled target app per platform.
- Keep insert mode to the default `Paste` path for the first pass.

### Phase 2B2 — Manual/expanded permission and insert-mode coverage

- macOS Accessibility + Microphone follow-up matrix.
- Windows `PasteAndEnter` / `ShiftInsert` follow-up.
- NSIS-installed-path verification if raw release EXE coverage proves insufficient.

## Open Follow-Up After This Batch

- Decide whether runtime smoke becomes required on every native build or remains workflow-dispatch/release gated.
- Add explicit packaged bootstrap/resource assertions only if runtime smoke or field failures indicate a real packaged-resource gap.
- Expand from deterministic transcript smoke to broader packaged cancel/stop timing checks only if the current runtime lane exposes regressions.
