# Windows Runtime Smoke Installed-Path Fix

## Context

- Native macOS release lane is green.
- Native Windows release lane failed on GitHub Actions run `23338178351` at **Run packaged runtime smoke**.
- Downloaded runtime-smoke artifacts show:
  - process output reached `VOICEWIN_RUNTIME_SMOKE_STAGE stage=build_service`
  - process output then emitted `VOICEWIN_RUNTIME_SMOKE_FAIL ... reason=build_service`
  - Notepad target file stayed empty
- The Windows runtime-smoke script currently launches the raw release executable from `target/x86_64-pc-windows-msvc/release/...exe`.
- The Tauri app resolves bundled resources from the Windows resource directory, which is tied to the installed/bundled executable layout, not the raw cargo release output.

## Root Cause

The Windows runtime smoke is labeled "packaged" but currently executes the raw release executable. That path is good enough for startup smoke, but it is not sufficient for runtime smoke because the real service path needs bundled resources (bootstrap model). The raw release executable does not reliably run with the packaged resource layout, so `build_service` fails before insertion.

A second gap surfaced at the same time: the runtime-smoke PowerShell script trusted process exit code first, but the artifact logs prove the app can emit an explicit runtime-smoke failure marker before PowerShell observes a failing exit code.

## Fix

1. Keep startup smoke as the fast raw-exe preflight.
2. Upgrade **Windows runtime smoke** to prefer the built NSIS installer, install it silently into a controlled temp directory, and run the installed executable from that packaged layout.
3. Keep backward-compatible fallback support for direct executable input when an installer path is explicitly unavailable.
4. Harden smoke diagnostics:
   - capture installer layout evidence
   - copy app log candidates when available
   - fail immediately on explicit `VOICEWIN_RUNTIME_SMOKE_FAIL` markers instead of waiting for downstream Notepad mismatch
5. Harden smoke-mode process exit in Tauri so smoke failures cannot silently collapse to exit code `0` on Windows.

## Verification

- Local:
  - `cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`
  - `cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml`
  - `bash scripts/ci/run-pr-checks.sh`
- Native:
  - rerun `.github/workflows/voicewin-windows-portable.yml`
  - require **Run packaged runtime smoke** to pass on the installed-path flow
