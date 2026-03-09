# Realistic STT Coverage Design

## Context

The first testing-foundation batch installed a daily PR lane, offline ElevenLabs batch coverage, a minimal live batch smoke, and missing `ModelsPage` coverage. The next gap is realism: the repository still proves only a narrow happy path for live provider behavior, while the real app depends on both batch HTTP and realtime WebSocket flows, plus stop-time fallback decisions that users actually feel when dictation goes wrong.

## Goals

- Expand deterministic coverage for provider/runtime behavior that frequently breaks in production-like usage.
- Add explicit live smoke coverage for both batch and realtime provider paths.
- Fix the small review findings so CI enforces locked dependencies and tests do not leak environment overrides.
- Keep the change scoped to layers that are practical to verify from this Linux worktree.

## Constraints

- The full recording/orchestration path in `voicewin-tauri/src-tauri/src/session_controller.rs` is heavily gated to macOS/Windows code paths, so Linux cannot execute end-to-end recorder tests directly.
- Live provider tests must stay opt-in and consume secrets from environment only.
- Assertions for live tests should be loose and robust; they should prove usable speech output rather than exact transcript strings.

## Design

Use a three-layer expansion:

1. **Deterministic provider shell tests** in `voicewin-providers/src/runtime.rs` to prove HTTP method dispatch, JSON/multipart forwarding, and boundary validation.
2. **Deterministic runtime/provider tests** in `voicewin-runtime/src/stt.rs` and `voicewin-runtime/tests/elevenlabs_stt_integration.rs` to cover blank keys, sample-rate rejection, model normalization, trimmed endpoint overrides, and error surfacing.
3. **Opt-in live provider matrix** in `voicewin-runtime/tests/live_provider_smoke.rs` to exercise both the ElevenLabs batch API and the realtime WebSocket path against a real fixture.

For the app-level orchestration gap, extract only the smallest pure helper needed from `session_controller` if it materially improves Linux-testable fallback logic. Do not force a broad refactor of the recorder path in this batch.

## Files In Scope

- Create: `docs/plans/2026-03-09-realistic-stt-coverage-implementation.md`
- Modify: `scripts/ci/run-pr-checks.sh`
- Modify: `scripts/ci/run-live-provider-smoke.sh`
- Modify: `voicewin-providers/Cargo.toml`
- Modify: `voicewin-providers/src/elevenlabs.rs`
- Modify: `voicewin-providers/src/runtime.rs`
- Modify: `voicewin-runtime/tests/elevenlabs_stt_integration.rs`
- Modify: `voicewin-runtime/tests/live_provider_smoke.rs`
- Modify: `docs/plans/2026-03-09-testing-foundation-implementation.md`

## Verification Strategy

- Focused deterministic checks:
  - `cargo test -p voicewin-providers`
  - `cargo test -p voicewin-runtime --test elevenlabs_stt_integration`
  - `cargo test -p voicewin-runtime --test live_provider_smoke -- --list`
- Live checks:
  - `VOICEWIN_LIVE_PROVIDER_TESTS=1 ELEVENLABS_SCRIBE_V2_API_KEY=... bash scripts/ci/run-live-provider-smoke.sh`
- Full regression:
  - `bash scripts/ci/run-pr-checks.sh`

## Non-Goals For This Batch

- Native packaged-app E2E.
- Permission/insertion automation.
- Large fixture corpus or visual regression tooling.
