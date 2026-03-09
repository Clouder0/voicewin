# Session Controller Orchestration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Linux-testable coverage for the riskiest session-controller orchestration decisions: realtime fallback, warning propagation, and overlay-stage mapping.

**Architecture:** Extract a small functional core from `session_controller.rs` for realtime stop-time decisions and stage mapping, then leave all Tauri/AppService side effects in the existing imperative shell. Test the helpers directly in `session_controller.rs` so CI covers the high-risk policy without requiring recorder/native runtime setup.

**Tech Stack:** Rust, Tokio tests, Tauri crate unit tests.

---

## Progress

- [x] Task 1: Extract pure stop-time decision helpers and add failing tests
- [x] Task 2: Wire the controller to use the helpers with minimal behavior changes
- [x] Task 3: Run targeted and broader verification, then update plan docs

## Verification Notes

- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::realtime_` failed first because the helper types/functions did not exist, then passed after extraction.
- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` passed with helper coverage for realtime preflight, finalize resolution, stage mapping, chunking, and transition gating.
- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml` passed (`8` tests total in the Tauri crate).
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed after the helper extraction, so the broader Rust/frontend verification lane still stays green.
- 2026-03-09: this batch deliberately covers the decision core only; it does not yet add native `mock_app()` event-order tests or recorder-driven platform orchestration.

### Task 1: Extract Pure Stop-Time Decision Helpers And Add Failing Tests

**Files:**
- Modify: `voicewin-tauri/src-tauri/src/session_controller.rs`

**Step 1: Write the failing tests**

Add tests for:

```rust
#[test]
fn realtime_preflight_drops_force_batch_and_warning() {}

#[test]
fn realtime_preflight_provider_backpressure_warning_forces_batch() {}

#[test]
fn realtime_finalize_resolution_uses_override_for_non_empty_text() {}

#[test]
fn realtime_finalize_resolution_empty_text_falls_back_with_warning() {}

#[test]
fn engine_stage_mapping_uses_finalizing_when_override_is_active() {}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::realtime_`

Expected: FAIL because the helpers/tests do not exist yet.

**Step 3: Write the minimal implementation**

- Extract `merge_warning` into a file-level helper.
- Add pure helper structs/enums for realtime preflight and finalize resolution.
- Add a pure helper for overlay stage mapping from engine stage labels.

**Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::realtime_`

Expected: PASS.

### Task 2: Wire The Controller To Use The Helpers With Minimal Behavior Changes

**Files:**
- Modify: `voicewin-tauri/src-tauri/src/session_controller.rs`

**Step 1: Keep the logic change minimal**

- Replace the inline local `merge_warning` function.
- Use the new preflight helper before tail/drain/finalize work.
- Use the new finalize-resolution helper after finalize returns.
- Use the new stage-mapping helper in the engine stage callback.

**Step 2: Run focused verification**

Run: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::`

Expected: PASS.

### Task 3: Run Targeted And Broader Verification, Then Update Plan Docs

**Files:**
- Modify: `docs/plans/2026-03-09-session-controller-orchestration-implementation.md`

**Step 1: Run verification**

Run:
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::`
- `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml`
- `bash scripts/ci/run-pr-checks.sh`

**Step 2: Update notes**

- Record commands and results.
- Note that this batch covers the decision core, not full native recorder E2E.

## Additional Progress

- 2026-03-09 follow-up: extracted a pure session-result resolver for stale-session ignore, success/warning handling, and failed/history fallback handling.
- 2026-03-09 follow-up: extracted a pure cancel-decision helper so recording vs processing cancellation policy stays unit-testable.
- 2026-03-09 follow-up: widened session-controller `AppHandle`-accepting methods over `tauri::Runtime` so `tauri::test::mock_app()` can drive Linux-safe controller tests.
- 2026-03-09 follow-up: fixed `stage = "error"` session-result handling so controller HUD errors preserve the real pipeline failure text.
- 2026-03-09 follow-up: moved stop-time result application into a controller `apply_session_result` path with session-conditional mutations, so late success/error application is guarded at mutation time instead of only at initial classification.
- 2026-03-09 follow-up: made `tokio` test features explicit in `voicewin-tauri/src-tauri/Cargo.toml` (`macros`, `rt`, `sync`, `time`) so the controller test suite does not rely on transitive feature unification.
- 2026-03-09 follow-up: tightened `apply_session_result` again so it now applies result state atomically under one lock via `apply_session_result_to_inner`, eliminating partial stale leaks when cancel/new-session interleaves between async mutation steps.
- 2026-03-09 follow-up: gated failed/error logging through the same atomic apply outcome so stale late results no longer leak error logs after being ignored for state/UI.
- 2026-03-09 follow-up: added a tiny test-only transition pause seam so `mock_app()` tests can deterministically force concurrent cancel/stop overlap instead of relying on scheduler timing.
- 2026-03-09 follow-up: added explicit finalize-error and finalize-timeout warning-order regression tests so the remaining late-finalize fallback policy is locked down in Linux-safe helper coverage.

## Additional Verification Notes

- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_result` passed after adding the session-result resolver and stale-session tests.
- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml cancel_` failed first because the cancel helper did not exist, then passed after extracting `resolve_cancel_decision`.
- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` passed with `14` session-controller tests, including stale-session, success/warning, failed/history fallback, and cancel-decision coverage.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed again after the session-result and cancel helper extraction.
- 2026-03-09: `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml cancel_` also now includes `mock_app()` controller coverage for processing cancellation and idle cancellation after widening `SessionController` methods over `tauri::Runtime`.
- 2026-03-09: review follow-up added `error_stage_session_result_preserves_error_message` and `stale_success_result_does_not_override_cancelled_session_state`, and `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` now passes with `18` controller tests listed.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed again after the review-driven `apply_session_result` and Tokio feature fixes.
- 2026-03-09: second review follow-up added `matching_success_result_applies_all_fields_atomically` and `stale_success_result_leaves_inner_unchanged_atomically`; `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` now passes with `20` controller tests.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed again after the atomic `apply_session_result_to_inner` refactor.
- 2026-03-09: third review follow-up added `stale_error_result_produces_no_log_or_state_changes` and `matching_failed_result_requests_log_after_atomic_apply`; `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` now passes with `22` focused controller tests and `23` total Tauri main tests in the full PR script.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed again after log gating moved behind the atomic session-result apply outcome.
- 2026-03-09: fourth follow-up added deterministic transition-race coverage via `cancel_holds_transition_and_concurrent_stop_reports_busy` and `stop_holds_transition_and_concurrent_cancel_reports_busy`; `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` now passes with `24` focused controller tests and `25` total Tauri main tests in the full PR script.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed again after the transition pause seam and public-method race tests.
- 2026-03-09: final helper follow-up added `realtime_finalize_error_merges_after_existing_warning` and `realtime_finalize_timeout_merges_after_existing_warning`; `cargo test --manifest-path voicewin-tauri/src-tauri/Cargo.toml session_controller::tests::` now passes with `26` focused controller tests and `27` total Tauri main tests in the full PR script.
- 2026-03-09: `bash scripts/ci/run-pr-checks.sh` passed again after the finalize warning-order regression tests.
