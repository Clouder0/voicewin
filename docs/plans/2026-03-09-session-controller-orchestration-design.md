# Session Controller Orchestration Design

## Context

The provider/runtime layers now have strong deterministic coverage plus live batch and realtime smoke tests. The largest remaining regression surface is the Tauri session controller, especially the stop-time transition from realtime dictation into batch fallback, the mapping of engine stages onto overlay stages, and the behavior when realtime warnings or errors mean we should not trust the streamed transcript.

## Goal

Add Linux-testable orchestration coverage for the highest-risk session-controller decisions without requiring a live recorder, OS-specific hooks, or a full packaged-app environment.

## Approach

Extract the stop-time decision logic into small pure helpers that can compile and run on Linux. Keep the side effects where they are now: emitting overlay status, aborting tasks, showing windows, and calling `AppService`. The helpers should only decide:

- whether realtime output is trustworthy enough to use as a transcript override,
- what warning text should be propagated into History/UI,
- how engine stages map to overlay stages when a transcript override is in play.

This keeps the imperative shell in `session_controller.rs` thin while moving the risky policy into a functional core that can be tested thoroughly.

## First Batch Scope

- Extract and test warning aggregation.
- Extract and test realtime preflight decisions from dropped chunks, provider warnings, and provider errors.
- Extract and test stop-time finalize result handling (success, empty transcript, failure, timeout).
- Extract and test overlay stage mapping so override-driven sessions never regress from `Finalizing` back to `Transcribing`.

## Follow-Up

After this helper-first batch, add `tauri::test::mock_app()` event-order tests and then a cancel/late-success batch around stale-session guards.
