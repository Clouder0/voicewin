# Desktop Release Lane Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a practical phase-2 native release lane that proves VoiceWin can build and launch as a packaged desktop app on macOS and Windows, with a deterministic startup smoke mode and workflow-level artifact checks.

**Architecture:** Reuse the existing native build workflows, but add one narrow startup-smoke seam at the packaged-app boundary instead of introducing broad GUI automation. Keep the first native batch deterministic: build the bundle, launch it in smoke mode, assert startup provenance/log markers, and exit cleanly. Leave permission/insertion E2E for a later batch with a controlled target app.

**Tech Stack:** Rust, Tauri 2, Bun/Vite, GitHub Actions, Bash, PowerShell, macOS `.app` bundles, Windows `.exe` / NSIS artifacts.

---

## Progress

- [ ] Task 1: Add a pure startup-smoke helper and unit tests
- [ ] Task 2: Wire startup smoke into the Tauri entrypoint
- [ ] Task 3: Add macOS and Windows packaged-app smoke scripts
- [ ] Task 4: Upgrade native workflows into smoke gates
- [ ] Task 5: Add a manual desktop smoke matrix and verification notes

## Execution Notes

- Start from the current local `main` checkpoint in a fresh worktree/branch; do not pile this work directly onto the merged testing-foundation milestone.
- Keep the Linux PR lane (`bash scripts/ci/run-pr-checks.sh`) unchanged unless a narrow smoke-related addition is necessary.
- Treat CI-hosted native smoke as startup/build validation, not full permission/insertion E2E.

---

### Task 1: Add A Pure Startup-Smoke Helper And Unit Tests

**Files:**
- Create: `voicewin-tauri/src-tauri/src/startup_smoke.rs`
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Test: `voicewin-tauri/src-tauri/src/startup_smoke.rs`

**Step 1: Write the failing tests**

Add Linux-safe unit tests for a tiny helper API, for example:

```rust
#[test]
fn smoke_mode_is_disabled_by_default() {
    assert_eq!(startup_smoke_mode(None, "0.1.4", "abc123"), None);
}

#[test]
fn smoke_mode_builds_expected_marker_when_enabled() {
    let mode = startup_smoke_mode(Some("1"), "0.1.4", "abc123").unwrap();
    assert_eq!(mode.marker, "VOICEWIN_SMOKE_OK version=0.1.4 git_sha=abc123");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`

Expected: FAIL because the helper module and tests do not exist yet.

**Step 3: Write the minimal implementation**

- Add a small pure helper in `voicewin-tauri/src-tauri/src/startup_smoke.rs`.
- Keep it string/env driven only.
- Return a small plain-data struct with the exact smoke marker text to emit.

**Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-tauri/src-tauri/src/startup_smoke.rs voicewin-tauri/src-tauri/src/main.rs
git commit -m "test(tauri): add startup smoke helper"
```

### Task 2: Wire Startup Smoke Into The Tauri Entrypoint

**Files:**
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Test: `voicewin-tauri/src-tauri/src/startup_smoke.rs`

**Step 1: Write the failing behavior test first**

Add or extend a unit test so the helper proves the entrypoint contract explicitly:

```rust
#[test]
fn smoke_mode_requires_exact_enable_flag() {
    assert!(startup_smoke_mode(Some("0"), "0.1.4", "abc123").is_none());
    assert!(startup_smoke_mode(Some("1"), "0.1.4", "abc123").is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke_requires_exact_enable_flag`

Expected: FAIL if the helper is not strict enough yet.

**Step 3: Write the minimal implementation**

In `voicewin-tauri/src-tauri/src/main.rs`:

- Evaluate the helper early in startup.
- Emit the existing startup provenance log plus a deterministic smoke marker.
- Exit cleanly before recorder/hotkey/tray/background runtime paths begin if smoke mode is enabled.

Keep the seam narrow: no extra config files, no test-only commands, no new UI path.

**Step 4: Run verification**

Run:
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml startup_smoke`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-tauri/src-tauri/src/main.rs voicewin-tauri/src-tauri/src/startup_smoke.rs
git commit -m "feat(tauri): add packaged startup smoke mode"
```

### Task 3: Add macOS And Windows Packaged-App Smoke Scripts

**Files:**
- Create: `scripts/ci/run-macos-bundle-smoke.sh`
- Create: `scripts/ci/run-windows-bundle-smoke.ps1`

**Step 1: Write the failing script calls**

Run the missing scripts directly:

```bash
bash scripts/ci/run-macos-bundle-smoke.sh
pwsh -File scripts/ci/run-windows-bundle-smoke.ps1
```

Expected: FAIL because the scripts do not exist yet.

**Step 2: Write the minimal implementation**

`scripts/ci/run-macos-bundle-smoke.sh` should:

- locate the built `.app`
- choose the executable under `Contents/MacOS/`
- run it with `VOICEWIN_SMOKE_TEST=1`
- capture stdout/stderr to a log file
- assert the smoke marker appears

`scripts/ci/run-windows-bundle-smoke.ps1` should:

- locate the built release `.exe`
- run it with `VOICEWIN_SMOKE_TEST=1`
- capture stdout/stderr
- assert the same smoke marker appears

Keep both scripts small and explicit; fail loudly if the artifact path is missing.

**Step 3: Verify locally where possible**

On Linux, only syntax/basic file checks are practical. Use native CI runners for the real smoke execution.

**Step 4: Commit**

```bash
git add scripts/ci/run-macos-bundle-smoke.sh scripts/ci/run-windows-bundle-smoke.ps1
git commit -m "test(ci): add native bundle smoke scripts"
```

### Task 4: Upgrade Native Workflows Into Smoke Gates

**Files:**
- Modify: `.github/workflows/voicewin-macos-unsigned.yml`
- Modify: `.github/workflows/voicewin-windows-portable.yml`

**Step 1: Add the failing workflow step references**

Update each workflow to call the new smoke script after the bundle is built, then run the workflow locally through review (it will not execute fully on Linux, but the referenced paths/commands should be valid in diff review).

**Step 2: Implement the minimal workflow changes**

- Keep existing artifact checks.
- Add the smoke-script step after bundle creation.
- Upload smoke logs as artifacts alongside the built app artifacts.
- Consider adding `schedule` only if it stays cheap and useful; otherwise keep `workflow_dispatch` + tag flow.

**Step 3: Verify referenced commands and paths**

Run:
- `bash scripts/ci/run-pr-checks.sh`

Then inspect workflow references carefully in diff review.

Expected: local baseline still PASS; native smoke execution remains for GitHub-hosted macOS/Windows runners.

**Step 4: Commit**

```bash
git add .github/workflows/voicewin-macos-unsigned.yml .github/workflows/voicewin-windows-portable.yml
git commit -m "test(ci): gate native bundles with startup smoke"
```

### Task 5: Add A Manual Desktop Smoke Matrix And Verification Notes

**Files:**
- Create: `docs/testing/desktop-release-smoke-matrix.md`
- Modify: `docs/plans/2026-03-09-desktop-release-lane-implementation.md`

**Step 1: Write the documentation stub first**

Create a short checklist covering:

- macOS unsigned `.app` launch
- Windows installer/binary launch
- startup provenance marker visible
- bootstrap model/resource presence
- manual permission/insertion follow-up matrix for TextEdit / Notepad

**Step 2: Verify the doc is concrete enough**

Read it back and ensure it names exact artifacts, log markers, and manual targets.

**Step 3: Update implementation notes**

- Record what Linux verified locally.
- Record what native CI is expected to verify.
- Record what still requires real-user/manual/native hardware validation.

**Step 4: Commit**

```bash
git add docs/testing/desktop-release-smoke-matrix.md docs/plans/2026-03-09-desktop-release-lane-implementation.md
git commit -m "docs: add desktop smoke verification matrix"
```

---

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
