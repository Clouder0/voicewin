# Testing Foundation Design

## Context

VoiceWin already has useful unit coverage, but the riskiest regressions still sit outside the daily guardrail: batch-provider wiring, the separate Tauri Rust crate, event-driven UI flows, and release-only workflows. The design goal for this batch is not to build the final testing system in one jump; it is to install a safe default path that every PR can run quickly, while also adding opt-in live-provider coverage and a concrete path toward desktop release gates.

## Goals

- Make normal development run a single, repeatable PR-grade test lane.
- Add real offline coverage for the ElevenLabs batch STT request/response path.
- Add live-provider smoke scaffolding that is secret-driven, opt-in, and cheap to run.
- Expand UI coverage in a high-risk event-driven surface (`ModelsPage`).
- Track the work in plan docs so implementation stays spec-driven.

## Non-Goals

- Full native desktop E2E in this Linux environment.
- Exact-transcript assertions against live providers.
- Broad framework churn such as adopting a brand-new repo-wide lint stack.

## Recommended Architecture

Use four lanes, but only fully implement the first three in this change:

1. Fast PR lane: a single script that runs root Rust tests, Tauri Rust tests, frontend Vitest, frontend typecheck, and frontend build.
2. Provider contract lane: offline Rust integration tests that redirect ElevenLabs batch requests to a local mock server through an explicit environment override.
3. Live-provider smoke lane: ignored Rust integration tests plus a manual/nightly GitHub Actions workflow that reads `ELEVENLABS_SCRIBE_V2_API_KEY` and uses a tiny speech fixture.
4. Desktop release lane: documented follow-up work for packaged-app smoke tests on macOS and Windows.

The important design choice is to keep testability at the provider boundary. Instead of introducing a broad dependency-injection framework, add one narrow boundary hook in `voicewin-providers/src/elevenlabs.rs` so batch STT can target either the real ElevenLabs endpoint or a local mock server. That keeps the functional core simple while giving integration tests realistic request coverage.

## Files In Scope

- Create: `docs/plans/2026-03-09-testing-foundation-implementation.md`
- Create: `scripts/ci/run-pr-checks.sh`
- Create: `scripts/ci/run-live-provider-smoke.sh`
- Create: `.github/workflows/voicewin-pr.yml`
- Create: `.github/workflows/voicewin-live-provider-smoke.yml`
- Create: `voicewin-runtime/tests/elevenlabs_stt_integration.rs`
- Create: `voicewin-runtime/tests/live_provider_smoke.rs`
- Create: `voicewin-tauri/src/ui/ModelsPage.test.tsx`
- Create: `testdata/audio/README.md`
- Create: `testdata/audio/english.wav`
- Modify: `voicewin-providers/src/elevenlabs.rs`
- Modify: `voicewin-runtime/Cargo.toml`
- Modify: `voicewin-tauri/package.json`
- Modify: `voicewin-tauri/src/ui/ModelsPage.tsx`

## Verification Strategy

- Local deterministic verification:
  - `cargo test -p voicewin-providers`
  - `cargo test -p voicewin-runtime --test elevenlabs_stt_integration`
  - `cargo test -p voicewin-runtime`
  - `cargo test` in `voicewin-tauri/src-tauri/`
  - `bun run test`
  - `bun run typecheck`
  - `bun run build`
  - `bash scripts/ci/run-pr-checks.sh`
- Live verification:
  - compile and register the ignored smoke test locally
  - run it only when the secret is injected outside the transcript

## Open Follow-Up After This Batch

- Add true packaged-app smoke tests on macOS and Windows.
- Add permission/insertion E2E against a controlled target app.
- Expand runtime coverage around session-controller fallback orchestration.
