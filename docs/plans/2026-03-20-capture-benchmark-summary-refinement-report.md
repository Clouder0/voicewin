# Capture Benchmark Summary Refinement Report

Date: 2026-03-20

## Scope

Refined the Tauri benchmark summaries for history replay and prompt preview so they no longer stop at OCR timing. The benchmark panels now surface screenshot capture behavior explicitly, which is required for latency work on visual context.

## Changes

- Added capture latency aggregation to both benchmark builders:
  - `capture_elapsed_min_ms`
  - `capture_elapsed_avg_ms`
  - `capture_elapsed_max_ms`
- Added capture fallback aggregation to both benchmark builders:
  - `capture_fallback_count`
  - `sample_capture_fallback_reason`
- Updated benchmark rendering to show:
  - `capture 15/17/19 ms min/avg/max`
  - `capture fallbacks 2/3`
  - `Sample capture fallback: foreground_window_not_implemented`
- Kept requested capture scope and actual capture scope separate. The benchmark label still reports the runtime visual label from the last round, including `actual-display` when a foreground-window request fell back to display capture.

## Why

Without capture telemetry in the benchmark summaries, latency review could only attribute cost to OCR, not to the screenshot stage that feeds OCR or multimodal dispatch. That obscures the true cost of visual context and makes OS-specific fallback behavior harder to spot.

## Validation

Executed in `voicewin-tauri`:

- `npm test -- HistoryPage.test.tsx PromptsPage.test.tsx`
- `npm run typecheck`

Both passed.
