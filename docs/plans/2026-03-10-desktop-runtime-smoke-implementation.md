# Desktop Runtime Smoke Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a deterministic packaged runtime-session smoke lane for macOS and Windows that reuses transcript override, inserts into controlled target apps, and upgrades the native workflows into deeper post-startup gates.

**Architecture:** Add one narrow env-gated runtime-smoke seam in Tauri that runs a real packaged session via `AppService::run_session_with_hook(...)` using `RunSessionRequest.transcript`. Pair that seam with explicit TextEdit/Notepad smoke scripts and workflow steps, while keeping live audio/STT and full permission automation out of the first runtime batch.

**Tech Stack:** Rust, Tauri 2, Bash, PowerShell, GitHub Actions, macOS TextEdit automation via `osascript`, Windows Notepad automation via PowerShell/WScript shell activation.

---

## Progress

- [ ] Task 1: Add a pure runtime-smoke helper and unit tests
- [ ] Task 2: Wire runtime smoke into the packaged Tauri entrypoint
- [ ] Task 3: Add macOS and Windows packaged runtime-smoke scripts
- [ ] Task 4: Upgrade native workflows to run runtime smoke after startup smoke
- [ ] Task 5: Update docs and verification notes for the new runtime lane

## Execution Notes

- Work in the isolated branch/worktree: `desktop-e2e-lane` at `.worktree/desktop-e2e-lane`.
- Keep the current startup-smoke path intact; runtime smoke is an additional packaged lane.
- Keep the first runtime batch deterministic: default `Paste` mode, controlled target app, transcript override, and explicit process-output markers.
- Do not add a broad desktop-testing framework.

---

### Task 1: Add A Pure Runtime-Smoke Helper And Unit Tests

**Files:**
- Create: `voicewin-tauri/src-tauri/src/runtime_smoke.rs`
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Test: `voicewin-tauri/src-tauri/src/runtime_smoke.rs`

**Step 1: Write the failing tests**

Add Linux-safe unit tests for the helper API, for example:

```rust
#[test]
fn runtime_smoke_is_disabled_by_default() {
    assert_eq!(runtime_smoke_mode(None, None, None, "0.1.4", "abc123"), Ok(None));
}

#[test]
fn runtime_smoke_requires_exact_enable_flag_and_non_empty_transcript() {
    assert_eq!(runtime_smoke_mode(Some("1 "), Some("hello"), None, "0.1.4", "abc123"), Ok(None));
    assert!(runtime_smoke_mode(Some("1"), Some("   "), None, "0.1.4", "abc123").is_err());
}

#[test]
fn runtime_smoke_builds_expected_markers() {
    let mode = runtime_smoke_mode(Some("1"), Some("hello world"), Some("TextEdit"), "0.1.4", "abc123")
        .unwrap()
        .unwrap();
    assert!(mode.success_marker.contains("VOICEWIN_RUNTIME_SMOKE_OK"));
}
```

Include one test for expected-process matching logic as a pure helper.

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`

Expected: FAIL because the helper module and tests do not exist yet.

**Step 3: Write the minimal implementation**

In `voicewin-tauri/src-tauri/src/runtime_smoke.rs`:

- Add a small pure helper that parses env-style inputs.
- Require exact enable flag `"1"`.
- Require a non-empty transcript.
- Accept an optional expected foreground process.
- Provide compact marker/log strings for:
  - runtime-smoke start
  - stage progress
  - success
  - failure

Keep it string/data only; no Tauri or app-service logic in this file.

**Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-tauri/src-tauri/src/runtime_smoke.rs voicewin-tauri/src-tauri/src/main.rs
git commit -m "test(tauri): add runtime smoke helper"
```

### Task 2: Wire Runtime Smoke Into The Packaged Tauri Entrypoint

**Files:**
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Modify: `voicewin-tauri/src-tauri/src/runtime_smoke.rs`
- Test: `voicewin-tauri/src-tauri/src/runtime_smoke.rs`

**Step 1: Write the failing tests first**

Add focused helper tests for the new orchestration boundary, for example:

```rust
#[test]
fn expected_process_match_is_case_insensitive() {
    assert!(foreground_process_matches(Some("notepad.exe"), Some("NOTEPAD.EXE")));
}

#[test]
fn runtime_smoke_process_output_orders_provenance_before_success_marker() {
    let mode = runtime_smoke_mode(Some("1"), Some("hello"), None, "0.1.4", "abc123").unwrap().unwrap();
    let output = runtime_smoke_success_output(&mode, "VoiceWin startup: version=0.1.4 git_sha=abc123");
    assert!(output.starts_with("VoiceWin startup:"));
    assert!(output.contains("VOICEWIN_RUNTIME_SMOKE_OK"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke_process_output_orders_provenance_before_success_marker`

Expected: FAIL because the new pure helper(s) do not exist yet.

**Step 3: Write the minimal implementation**

In `voicewin-tauri/src-tauri/src/main.rs`:

- Detect runtime-smoke mode early, alongside startup smoke.
- Skip `tauri_plugin_single_instance` when either smoke mode is active.
- During `.setup(...)`, keep startup provenance logging.
- If runtime smoke is enabled:
  - hide the main window if possible,
  - spawn an async task instead of exiting immediately,
  - wait a short fixed delay so the target app can be re-focused,
  - build the real `AppService`,
  - capture/validate the foreground process if configured,
  - run `AppService::run_session_with_hook(...)` with:
    - `RunSessionRequest { transcript: <env transcript>, warning: None }`
    - a minimal dummy `AudioInput`
  - write runtime-smoke markers to process output,
  - exit `0` on success and nonzero on failure.

Keep the seam narrow and env-gated; do not add a broad new test command surface.

**Step 4: Run verification**

Run:
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-tauri/src-tauri/src/main.rs voicewin-tauri/src-tauri/src/runtime_smoke.rs
git commit -m "feat(tauri): add packaged runtime smoke mode"
```

### Task 3: Add macOS And Windows Packaged Runtime-Smoke Scripts

**Files:**
- Create: `scripts/ci/run-macos-runtime-smoke.sh`
- Create: `scripts/ci/run-windows-runtime-smoke.ps1`

**Step 1: Write the failing script calls**

Run the missing scripts directly:

```bash
bash scripts/ci/run-macos-runtime-smoke.sh
pwsh -NoLogo -NoProfile -File scripts/ci/run-windows-runtime-smoke.ps1
```

Expected: FAIL because the scripts do not exist yet.

**Step 2: Write the minimal implementation**

`scripts/ci/run-macos-runtime-smoke.sh` should:

- locate the built `VoiceWin.app`
- prepare and focus TextEdit via `osascript`
- launch the packaged app with:
  - `VOICEWIN_RUNTIME_SMOKE_TEST=1`
  - `VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT=<known transcript>`
  - `VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS=TextEdit`
- re-focus TextEdit after launching VoiceWin
- capture process output
- assert startup provenance + runtime success markers
- read TextEdit document text and assert the transcript landed
- fail loudly on missing artifact, missing markers, or text mismatch

`scripts/ci/run-windows-runtime-smoke.ps1` should:

- locate the built Windows release executable via explicit candidate paths
- create/open an empty temp file in Notepad
- focus Notepad
- launch the packaged app with:
  - `VOICEWIN_RUNTIME_SMOKE_TEST=1`
  - `VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT=<known transcript>`
  - `VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS=notepad.exe`
- re-focus Notepad after launching VoiceWin
- capture stdout/stderr
- assert startup provenance + runtime success markers
- save the temp file and assert its contents match the transcript
- fail loudly if anything is missing or mismatched

Keep both scripts explicit and small; do not add framework-heavy automation.

**Step 3: Verify locally where possible**

On Linux, only syntax/basic checks are practical. Use native CI runners for the real runtime smoke execution.

Run at least:
- `bash -n scripts/ci/run-macos-runtime-smoke.sh`
- `command -v pwsh || true`

If `pwsh` exists, also run a PowerShell parse check. Otherwise document the blocker.

**Step 4: Commit**

```bash
git add scripts/ci/run-macos-runtime-smoke.sh scripts/ci/run-windows-runtime-smoke.ps1
git commit -m "test(ci): add packaged runtime smoke scripts"
```

### Task 4: Upgrade Native Workflows To Run Runtime Smoke After Startup Smoke

**Files:**
- Modify: `.github/workflows/voicewin-macos-unsigned.yml`
- Modify: `.github/workflows/voicewin-windows-portable.yml`

**Step 1: Add the failing workflow step references**

Update each workflow to reference the new runtime-smoke script after the existing startup-smoke step.

**Step 2: Implement the minimal workflow changes**

- Keep the current artifact checks and startup-smoke steps.
- Add the runtime-smoke step after startup smoke.
- Upload runtime smoke logs/artifacts alongside the current artifact uploads.
- Keep the native workflows as the real execution environment; do not try to make `run-pr-checks` build or execute native GUI runtime smoke locally.

**Step 3: Verify referenced commands and paths**

Run:
- `bash scripts/ci/run-pr-checks.sh`

Then inspect workflow references carefully in diff review.

Expected: local baseline still PASS; native runtime smoke remains for GitHub-hosted macOS/Windows runners.

**Step 4: Commit**

```bash
git add .github/workflows/voicewin-macos-unsigned.yml .github/workflows/voicewin-windows-portable.yml
git commit -m "test(ci): add packaged runtime smoke gates"
```

### Task 5: Update Docs And Verification Notes For The New Runtime Lane

**Files:**
- Modify: `docs/testing/desktop-release-smoke-matrix.md`
- Modify: `docs/plans/2026-03-09-desktop-release-lane-implementation.md`

**Step 1: Write the documentation updates**

Update the smoke matrix so it clearly distinguishes:

- startup smoke (Phase 2A)
- packaged runtime smoke (Phase 2B1)
- manual/expanded permission follow-up (Phase 2B2)

Document exact runtime-smoke evidence:

- startup provenance line
- runtime-smoke success marker
- controlled target app content match

**Step 2: Update implementation notes**

Record:

- what Linux verified locally
- what native CI is expected to verify for runtime smoke
- what still requires real-user/manual permission validation

**Step 3: Read the docs back for concreteness**

Make sure they name exact artifacts, target apps, and still-pending permission steps.

**Step 4: Commit**

```bash
git add docs/testing/desktop-release-smoke-matrix.md docs/plans/2026-03-09-desktop-release-lane-implementation.md
git commit -m "docs: add packaged runtime smoke verification"
```

---

## Final Verification

Run locally:

- `bash scripts/ci/run-pr-checks.sh`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml runtime_smoke`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`
- `bash -n scripts/ci/run-macos-runtime-smoke.sh`

If `pwsh` is available locally, also run:

- `pwsh -NoLogo -NoProfile -Command "[void][System.Management.Automation.Language.Parser]::ParseFile('scripts/ci/run-windows-runtime-smoke.ps1',[ref]$null,[ref]$errs); if ($errs) { $errs | ForEach-Object { $_.ToString() }; exit 1 }"`

Then run on GitHub-hosted native runners via workflow dispatch:

- `.github/workflows/voicewin-macos-unsigned.yml`
- `.github/workflows/voicewin-windows-portable.yml`

Expected native evidence:

- packaged app starts normally outside startup-only smoke
- bundled bootstrap model is available during real service initialization
- runtime-smoke success marker appears
- TextEdit/Notepad contain the expected transcript text
- runtime smoke logs upload with artifacts
