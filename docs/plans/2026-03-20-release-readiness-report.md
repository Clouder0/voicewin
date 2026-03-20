# Release Readiness Report

Date: 2026-03-20

## Scope

Assessed whether the current VoiceWin tree is ready to publish and pushed the release-readiness surface as far as possible from this environment.

This pass covered:

- full local release-gate execution
- release-lane workflow dispatch viability
- exact remaining blockers to a real publish decision

## Findings

### 1. The full local release gate is green

`scripts/ci/run-pr-checks.sh` now passes locally.

Verified:

- `cargo test --locked`
- `cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml`
- `bun install --frozen-lockfile`
- `bun run test`
- `bun run typecheck`
- `bun run build`

### 2. The local gate needed one environment-hardening fix

In this sandbox, the default Cargo registry cache under `~/.cargo` is read-only.

Updated:

- `scripts/ci/run-pr-checks.sh`

Behavior:

- when `CARGO_HOME` is unset
- and the default Cargo cache probe is not writable
- the script now falls back to `CARGO_HOME=${TMPDIR:-/tmp}/voicewin-cargo-home`

This is an execution-environment hardening change, not a product change.

### 3. Native release workflows are dispatchable from this repo

Confirmed:

- `gh` is installed
- GitHub auth is valid with `workflow` scope
- repo is `Clouder0/voicewin`
- default branch is `main`

Dispatched on remote `main`:

- macOS unsigned workflow:
  - run `23338178411`
  - <https://github.com/Clouder0/voicewin/actions/runs/23338178411>
- Windows installer workflow:
  - run `23338178351`
  - <https://github.com/Clouder0/voicewin/actions/runs/23338178351>

Outcome:

- macOS unsigned workflow `23338178411`: **success**
- Windows installer workflow `23338178351`: **failure**

Windows failed specifically at **Run packaged runtime smoke**.

Downloaded runtime-smoke evidence showed:

- startup provenance emitted correctly
- runtime smoke reached `VOICEWIN_RUNTIME_SMOKE_STAGE stage=build_service`
- runtime smoke then emitted `VOICEWIN_RUNTIME_SMOKE_FAIL ... reason=build_service`
- the Notepad target file stayed empty

Root cause: the Windows runtime-smoke script was still launching the raw release executable instead of a real installed/package layout, so bundled bootstrap-model resources were not reliably available for the runtime service path.

Local fix prepared:

- `scripts/ci/run-windows-runtime-smoke.ps1` now prefers the NSIS installer, installs silently into a controlled temp directory, runs the installed executable, captures installer/app-log evidence, and fails immediately on explicit runtime-smoke failure markers
- `voicewin-tauri/src-tauri/src/main.rs` now hardens smoke-mode process exit so Windows smoke failures cannot silently collapse to exit code `0`
- `docs/plans/2026-03-20-windows-runtime-smoke-installed-path-fix.md` records the evidence, root cause, and fix

Local verification after the fix:

- `cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`
- `cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml`
- `bash scripts/ci/run-pr-checks.sh`

All passed locally.

## Hard Blocker

The current workspace is **not** a clean release candidate snapshot.

Current git state:

- branch: `main`
- upstream: `origin/main`
- local tree contains a large uncommitted feature batch

Current diff shape:

- 61 tracked files changed in `git diff --stat`
- about `15138` insertions
- about `850` deletions
- many additional untracked files under `docs/plans/`, provider modules, platform modules, and UI files

That means native CI cannot validate the exact code that exists in this local tree until it is frozen into a clean commit and pushed to a dedicated remote ref.

## Publish Decision

Current answer: **not ready to publish this exact tree yet**.

Reasons:

- local gates are green, which is good
- release workflows exist and are runnable, which is good
- macOS native lane is green
- Windows now has a concrete fix staged locally for the packaged runtime-smoke blocker
- but the exact code intended for release is still only a large dirty local tree
- the Windows fix has not yet been validated on native CI for this exact snapshot
- native macOS/Windows evidence for this exact local snapshot still does not exist yet

## Required Next Step

Before a publish decision:

1. freeze this work into a clean release-candidate commit or branch
2. push that exact ref
3. run the native macOS and Windows workflows against that exact ref
4. review the smoke artifacts/logs from those runs
5. only then decide whether to tag/publish
