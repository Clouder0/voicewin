# Testing Foundation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a practical testing foundation that turns PR validation into a daily guardrail, strengthens batch-provider coverage, adds opt-in live smoke coverage, and closes a major UI-testing gap.

**Architecture:** Keep the fast path simple and deterministic: one shared PR script plus focused offline integration tests. Add only one new production seam, an ElevenLabs STT URL override, so provider integration tests can hit a local mock server without introducing broad test-only abstractions. Keep live-provider coverage opt-in and secret-driven, and treat desktop packaged-app E2E as the next phase after this foundation is in place.

**Tech Stack:** Rust, Tokio, Wiremock, Hound, Bun, Vitest/RTL, GitHub Actions, shell scripts.

---

## Progress

- [x] Task 0: Set up a safe `.worktree` branch and verify the clean baseline
- [x] Task 1: Add provider contract tests and the minimal ElevenLabs URL override seam
- [x] Task 2: Add live-provider smoke scaffolding with curated audio fixture support
- [x] Task 3: Add missing `ModelsPage` UI coverage and frontend verification scripts
- [x] Task 4: Add PR and live-smoke GitHub Actions workflows
- [x] Task 5: Run verification, update this plan with results, and finish the branch

## Execution Notes

- Working tree: `.worktree/testing-foundation`
- Baseline already verified in the worktree before implementation:
  - root `cargo test`
  - `cargo test` in `voicewin-tauri/src-tauri/`
  - `bun run test && bunx tsc --noEmit` in `voicewin-tauri/`
- The user supplied a live provider key in chat; it will not be written into repo files or shell history. Live tests must consume secrets from environment variables only.

## Verification Notes

- 2026-03-09: Clean worktree baseline established in `.worktree/testing-foundation`.
- 2026-03-09: `cargo test -p voicewin-providers` passed after adding the ElevenLabs URL override seam and test isolation cleanup.
- 2026-03-09: `cargo test -p voicewin-runtime --test elevenlabs_stt_integration` passed (`3` tests).
- 2026-03-09: `cargo test -p voicewin-runtime --test live_provider_smoke -- --list` passed and registered the ignored live smoke test plus the fixture-loader check.
- 2026-03-09: `bash scripts/ci/run-live-provider-smoke.sh` skipped cleanly without `VOICEWIN_LIVE_PROVIDER_TESTS=1`.
- 2026-03-09: `bun run test -- ModelsPage.test.tsx` passed (`3` tests).
- 2026-03-09: `bun run typecheck` passed after adding a shared `typecheck` script.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed end-to-end: root `cargo test`, `cargo test` in `voicewin-tauri/src-tauri/`, `bun install --frozen-lockfile`, `bun run test`, `bun run typecheck`, and `bun run build`.
- 2026-03-09: Live-provider key handling remains environment-only by design; the chat-provided secret was not written to repo files or shell commands.
- 2026-03-09: coverage expanded further in the follow-up plan at `docs/plans/2026-03-09-realistic-stt-coverage-implementation.md`, including deterministic HTTP executor tests plus live batch and realtime smoke coverage.
- 2026-03-09: added `voicewin-appcore` regression coverage proving insert failure still merges caller warnings and persists recovered text to History; `cargo test -p voicewin-appcore` passed with `6` tests and the broader PR verification lane stayed green.

---

### Task 1: Add Provider Contract Tests And The Minimal ElevenLabs URL Override Seam

**Files:**
- Modify: `voicewin-providers/src/elevenlabs.rs`
- Create: `voicewin-runtime/tests/elevenlabs_stt_integration.rs`
- Modify: `voicewin-runtime/Cargo.toml`

**Step 1: Write the failing tests**

Add provider tests that prove the batch STT path hits the expected endpoint and handles core failure cases:

```rust
#[tokio::test]
async fn transcribe_normalizes_realtime_model_and_hits_mock_server() { /* ... */ }

#[tokio::test]
async fn transcribe_surfaces_non_success_status_with_response_body() { /* ... */ }

#[tokio::test]
async fn transcribe_rejects_non_16khz_audio_before_network() { /* ... */ }
```

Also add a unit test in `voicewin-providers/src/elevenlabs.rs` proving the request URL honors `VOICEWIN_ELEVENLABS_STT_URL` when present.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p voicewin-runtime --test elevenlabs_stt_integration`

Expected: FAIL because the integration test and/or URL override seam do not exist yet.

**Step 3: Write the minimal implementation**

- Add a tiny `elevenlabs_stt_url()` helper in `voicewin-providers/src/elevenlabs.rs`.
- Default to `https://api.elevenlabs.io/v1/speech-to-text`.
- If `VOICEWIN_ELEVENLABS_STT_URL` is present and non-empty, use it instead.
- Add only the dev-dependencies needed for the tests (`wiremock`, plus anything required by the integration harness).

**Step 4: Run tests to verify they pass**

Run:
- `cargo test -p voicewin-providers`
- `cargo test -p voicewin-runtime --test elevenlabs_stt_integration`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-providers/src/elevenlabs.rs voicewin-runtime/Cargo.toml voicewin-runtime/tests/elevenlabs_stt_integration.rs
git commit -m "test: cover ElevenLabs batch provider wiring"
```

### Task 2: Add Live-Provider Smoke Scaffolding With Curated Audio Fixture Support

**Files:**
- Create: `voicewin-runtime/tests/live_provider_smoke.rs`
- Create: `testdata/audio/README.md`
- Create: `testdata/audio/english.wav`
- Modify: `voicewin-runtime/Cargo.toml`
- Create: `scripts/ci/run-live-provider-smoke.sh`

**Step 1: Write the failing smoke test**

Add an ignored integration test that:

- loads `testdata/audio/english.wav`
- reads `ELEVENLABS_SCRIBE_V2_API_KEY`
- submits the fixture through `ElevenLabsSttProvider`
- asserts the returned transcript contains non-empty alphabetic multi-word output rather than exact phrasing

**Step 2: Run the test to verify the harness is wired correctly**

Run: `cargo test -p voicewin-runtime --test live_provider_smoke -- --list`

Expected: PASS with the ignored test listed.

**Step 3: Write the minimal implementation**

- Add the fixture README with provenance and usage.
- Add the tiny WAV fixture.
- Add a small WAV-loading helper in the test using `hound`.
- Add `scripts/ci/run-live-provider-smoke.sh` that exits early unless `VOICEWIN_LIVE_PROVIDER_TESTS=1` and `ELEVENLABS_SCRIBE_V2_API_KEY` are set.

**Step 4: Run verification**

Run:
- `cargo test -p voicewin-runtime --test live_provider_smoke -- --list`
- `bash scripts/ci/run-live-provider-smoke.sh` (without the env flag, expect skip)

Expected: smoke harness registers cleanly; script skips cleanly when env is absent.

**Step 5: Commit**

```bash
git add voicewin-runtime/Cargo.toml voicewin-runtime/tests/live_provider_smoke.rs testdata/audio/README.md testdata/audio/english.wav scripts/ci/run-live-provider-smoke.sh
git commit -m "test: add opt-in live provider smoke coverage"
```

### Task 3: Add Missing `ModelsPage` UI Coverage And Frontend Verification Scripts

**Files:**
- Create: `voicewin-tauri/src/ui/ModelsPage.test.tsx`
- Modify: `voicewin-tauri/package.json`
- Modify: `voicewin-tauri/src/ui/ModelsPage.tsx`

**Step 1: Write the failing tests**

Add `ModelsPage` tests that cover the highest-risk behaviors now missing:

```tsx
it('shows cloud-provider warning and confirms before switching to local')
it('updates visible download progress from Tauri events')
it('surfaces download failures and clears optimistic progress state')
```

**Step 2: Run tests to verify they fail**

Run: `bun run test -- ModelsPage.test.tsx`

Expected: FAIL because the test file does not exist yet.

**Step 3: Write the minimal implementation**

- Add the new test file.
- Add a `typecheck` script to `voicewin-tauri/package.json` so CI and local verification call the same command.
- Preserve download errors in `voicewin-tauri/src/ui/ModelsPage.tsx` after the refresh path clears optimistic state.

**Step 4: Run tests to verify they pass**

Run:
- `bun run test -- ModelsPage.test.tsx`
- `bun run typecheck`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-tauri/package.json voicewin-tauri/src/ui/ModelsPage.tsx voicewin-tauri/src/ui/ModelsPage.test.tsx
git commit -m "test(ui): cover model download and provider switch flows"
```

### Task 4: Add PR And Live-Smoke GitHub Actions Workflows

**Files:**
- Create: `scripts/ci/run-pr-checks.sh`
- Create: `.github/workflows/voicewin-pr.yml`
- Create: `.github/workflows/voicewin-live-provider-smoke.yml`

**Step 1: Write the failing verification target**

Plan to verify that one shared PR script drives both local and CI checks.

**Step 2: Run the script path before implementation**

Run: `bash scripts/ci/run-pr-checks.sh`

Expected: FAIL because the script does not exist yet.

**Step 3: Write the minimal implementation**

- `scripts/ci/run-pr-checks.sh` should run:
  - `cargo test`
  - `cargo test` in `voicewin-tauri/src-tauri/`
  - `bun install --frozen-lockfile`
  - `bun run test`
  - `bun run typecheck`
  - `bun run build`
- `voicewin-pr.yml` should run on `pull_request` and on branch pushes, but ignore tags.
- `voicewin-live-provider-smoke.yml` should run on `workflow_dispatch` and nightly schedule, and only execute smoke tests when the environment secret is available.

**Step 4: Run verification**

Run: `bash scripts/ci/run-pr-checks.sh`

Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/ci/run-pr-checks.sh .github/workflows/voicewin-pr.yml .github/workflows/voicewin-live-provider-smoke.yml
git commit -m "ci: add daily test and provider smoke workflows"
```

### Task 5: Run Verification, Update This Plan With Results, And Finish The Branch

**Files:**
- Modify: `docs/plans/2026-03-09-testing-foundation-implementation.md`

**Step 1: Run the full verification set**

Run:
- `cargo test`
- `cargo test` in `voicewin-tauri/src-tauri/`
- `cargo test -p voicewin-runtime --test live_provider_smoke -- --list`
- `bun run test`
- `bun run typecheck`
- `bun run build`
- `bash scripts/ci/run-pr-checks.sh`

**Step 2: Update progress and verification notes**

- Mark completed tasks in the progress section.
- Record exact commands run and noteworthy constraints.
- Note that the live-provider secret was intentionally not written to disk or shell history from chat.

**Step 3: Final branch hygiene**

- Run `git status`.
- If requested by the user, create focused commits and optionally a PR.
