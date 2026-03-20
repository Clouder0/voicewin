# History Profile Traceability Report

Date: 2026-03-19

## Why this follow-up

The LLM post-processing surface now supports:

- per-app profile matching
- profile-specific prompt/provider/model overrides
- replay preview from history

That made one remaining UX gap obvious: history rows showed prompt/provider metadata, but not
which profile actually shaped the enhancement run.

Without that field, users cannot easily answer:

- "Which profile produced this text?"
- "Was this row using defaults or an app-specific override?"
- "Why did this replay resolve to a different model/provider?"

## Decision

Persist `matched_profile_name` into `HistoryEntry` as trace metadata only.

This is intentionally not used to force replay behavior later. Replay should continue to resolve
against the current config using the saved app/window identity, because that keeps the behavior
explicit and reflects what the app would do now.

## Implemented

- `voicewin-runtime::history::HistoryEntry` now stores `matched_profile_name`
- session history persistence writes `result.config.matched_profile_name`
- history UI renders saved profile metadata inline with the row
- replay preview UI also shows the currently resolved effective scope

## Compatibility

Backward compatible:

- the new history field is `#[serde(default)]`
- existing history files continue to load without migration

## Tests

Added/updated coverage for:

- persisted prepared-session history includes matched profile name
- History page renders saved profile metadata
- History replay preview renders resolved profile scope

## Product outcome

History now explains not just what happened, but which profile caused it. That makes profile-driven
LLM behavior auditable enough for real iteration instead of guesswork.
