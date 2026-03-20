# 2026-03-20 Visual Context Runtime Telemetry Report

## Goal

Close the remaining observability gap in VoiceWin visual context so History and prompt preview show:

- configured visual mode and capture scope,
- actual visual dispatch used at runtime,
- OCR sidecar timing and text-size telemetry when OCR runs.

This specifically fixes the ambiguity around `auto`, screenshot fallback, and OCR sidecar latency.

## What changed

### Runtime model

Added `voicewin_core::llm::VisualContextRuntime`:

- `mode`
- `capture_scope`
- `dispatch`
- `screen_ocr_elapsed_ms`
- `screen_ocr_first_token_ms`
- `screen_ocr_text_chars`

`dispatch` is now the **actual** main-request visual path after screenshot availability and OCR success are known:

- `screenshot` only when an image is actually attached
- `ocr` only when OCR text is actually injected
- `off` when the configured visual path degraded away at runtime

### Backend plumbing

Added runtime telemetry to:

- engine session results
- persisted history entries
- prompt preview IPC responses

Both engine and appcore preview paths now:

- record OCR elapsed / first-token / char-count metrics,
- compute final dispatch after capture / OCR outcome,
- emit a final visual-context debug log with requested vs actual dispatch.

OCR failure logs now include elapsed time.

### UI

History now shows:

- `visual:auto->ocr`
- `visual:screenshot->off`
- OCR latency and OCR text-size metrics in the latency line

Prompt preview / replay preview now show a dedicated runtime visual line, for example:

- `Visual auto -> ocr • capture foreground-window • OCR 64 ms • OCR first token 50 ms • OCR text 8 chars`

Benchmark panels now also aggregate visual/OCR runtime telemetry across rounds:

- stable visual route label when present, for example:
  - `visual auto->ocr / foreground-window`
- OCR sidecar elapsed min/avg/max
- OCR first-token min/avg/max
- OCR text-size min/avg/max
- visual variant count when runtime dispatch differs across rounds

### Live tooling

Updated live preview/replay examples to print:

- `visual_dispatch`
- `screen_ocr_elapsed_ms`
- `screen_ocr_first_token_ms`
- `screen_ocr_text_chars`

### OCR stop-path hardening

Refined `voicewin-core/src/enhancement.rs` so the enhancer no longer accepts generic assistant-option spill as final corrected text during OCR flows.

New behavior:

- generic menu-like options such as `transcribe/correct it`, `translate it`, `identify the language`, or `help write something similar` are treated as invalid salvage in enhancer mode
- low-signal transcript echoes such as quoted/backticked restatements now fall back to OCR or transcript instead of surviving as the final correction
- transcript-equivalent rewrites that carry useful formatting improvements still survive, for example punctuation or internal capitalization fixes

Added regressions for:

- exact failing clarifying-list output with OCR fallback
- same clarifying-list output without OCR, which now falls back to transcript instead of a generic menu item

## Files

- `voicewin-core/src/llm.rs`
- `voicewin-core/src/enhancement.rs`
- `voicewin-engine/src/session.rs`
- `voicewin-engine/src/engine.rs`
- `voicewin-engine/tests/engine_integration.rs`
- `voicewin-runtime/src/history.rs`
- `voicewin-runtime/src/ipc.rs`
- `voicewin-appcore/src/service.rs`
- `voicewin-tauri/src/lib/types.ts`
- `voicewin-tauri/src/ui/HistoryPage.tsx`
- `voicewin-tauri/src/ui/HistoryPage.test.tsx`
- `voicewin-tauri/src/ui/PromptsPage.tsx`
- `voicewin-tauri/src/ui/PromptsPage.test.tsx`

## Verification

Verified on March 20, 2026:

- `cargo fmt --all`
- `cargo test -q -p voicewin-core`
- `cargo test -q -p voicewin-engine`
- `cargo test -q -p voicewin-appcore`
- `cargo test -q -p voicewin-runtime`
- `cargo test -q`
- `cargo check -q --examples -p voicewin-appcore`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

All passed.

## Live validation

Validated on March 20, 2026 against:

- base: `https://cc2.caaa.tech/v1`
- provider: `openai_compatible`
- api: `responses_sse`
- model: `gpt-5.4`

Used:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- `visual_context_mode=ocr`
- screenshot fixture: `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`

Observed:

- final output: `VoiceWin`
- visual dispatch: `Ocr`
- OCR elapsed: `2021 ms`
- OCR first token: `1913 ms`
- OCR text chars: `8`
- provider elapsed: `2166 ms`
- end-to-end elapsed: `4192 ms`

This confirms the telemetry path is populated in a real gateway call, not just in unit/UI tests.

### Stop-path comparison after OCR hardening

Validated on March 20, 2026 against:

- base: `https://cc2.caaa.tech/v1`
- provider: `openai_compatible`
- api: `responses_sse`
- model: `gpt-5.4`
- example: `voicewin-appcore/examples/live_session_stop_latency.rs`
- transcript: `voice wen`
- screenshot fixture: `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`
- rounds: `3`

#### OCR mode

Prompt:

- `Correct the dictated product name using OCR text when helpful. Return only the corrected product name as plain text. No explanation.`

Observed:

- all `3/3` rounds returned `VoiceWin`
- no warning was emitted in any round
- visual dispatch stayed stable at `ocr/display`
- stop wall min/avg/max: `2858 / 5770 / 10489 ms`
- enhancement min/avg/max: `1047 / 2230 / 3576 ms`
- first-token min/avg/max: `920 / 2129 / 3460 ms`
- OCR elapsed min/avg/max: `1810 / 3538 / 6912 ms`
- OCR first-token min/avg/max: `1809 / 3517 / 6849 ms`
- OCR text chars min/avg/max: `8 / 8 / 8`

Conclusion:

- the previously observed `Transcribe/correct it` failure did not recur
- OCR mode is now correct on this real stop-path benchmark, but it remains substantially slower and more variable than screenshot mode because the OCR sidecar dominates end-to-end latency

#### Screenshot mode

Prompt:

- `Correct the dictated product name using the attached screenshot if needed. Return only the corrected product name as plain text. No explanation.`

Observed:

- all `3/3` rounds returned `VoiceWin`
- visual dispatch stayed stable at `screenshot/display`
- stop wall min/avg/max: `1371 / 2776 / 4164 ms`
- enhancement min/avg/max: `1370 / 2774 / 4164 ms`
- first-token min/avg/max: `1242 / 2609 / 3907 ms`
- OCR metrics stayed at `0`
- one round required assistant-framing stripping, but final output remained correct

Conclusion:

- screenshot remains the lower-latency and lower-variance choice for multimodal-capable models
- OCR should stay as an explicit fallback/compatibility mode rather than the default performance path

## Notes

The current product implication is straightforward:

- for GPT-5.4 on this gateway, prefer `auto` or `screenshot` when multimodal input is available
- keep `ocr` available for non-multimodal or user-preferred flows
- use the new visual runtime telemetry to make OCR latency costs visible in History and preview benchmarking instead of guessing
