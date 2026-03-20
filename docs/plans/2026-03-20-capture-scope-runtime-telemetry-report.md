# 2026-03-20 Capture Scope Runtime Telemetry Report

## Why this pass

VoiceWin already allowed configuring visual capture scope:

- `display`
- `foreground_window`

But runtime telemetry only preserved the requested scope. If the platform fell back from `foreground_window` to `display`, History and preview benchmarking still looked like foreground-window capture had succeeded. That made OS-specific debugging and latency analysis ambiguous.

## Implemented

### New capture metadata

Added screenshot capture metadata in `voicewin_core::context::ScreenshotCaptureMetadata`:

- `actual_scope`
- `capture_elapsed_ms`
- `fallback_reason`

Added `CapturedScreenshot` in `voicewin_engine::traits` and extended `ContextSnapshot` with:

- `screenshot_metadata`

`AppContextProvider::capture_screenshot(...)` now returns `CapturedScreenshot` so the platform boundary can preserve metadata instead of only an image artifact.

### Runtime telemetry propagation

Added fields to `voicewin_core::llm::VisualContextRuntime`:

- `capture_actual_scope`
- `screenshot_capture_elapsed_ms`
- `capture_fallback_reason`

Those fields are now propagated through:

- engine session runs
- appcore prompt preview
- history persistence / IPC / TS types via serde

### Platform and test providers

Updated:

- `voicewin-platform/src/windows.rs`
- `voicewin-platform/src/macos.rs`
- `voicewin-platform/src/test.rs`
- appcore/engine test providers

Behavior:

- Windows now records real foreground-window vs display fallback metadata
- macOS records the current display fallback for requested foreground-window capture with
  `fallback_reason=foreground_window_not_implemented`
- test providers now emit deterministic synthetic metadata for non-native tests and live examples

### UI / benchmark surfaces

Updated:

- `voicewin-tauri/src/ui/HistoryPage.tsx`
- `voicewin-tauri/src/ui/PromptsPage.tsx`

Changes:

- latency lines now include screenshot capture cost when available:
  - `Capture 19 ms`
- context/runtime lines now show actual capture scope when it differs:
  - `captured:display`
  - `captured display`
- fallback reasons are surfaced for debugging:
  - `fallback:foreground_window_not_implemented`

Benchmark visual labels now distinguish actual fallback variants when they differ from the requested scope.

## Tests added / tightened

### Engine

`voicewin-engine/tests/engine_integration.rs`

- validated that requested `foreground_window` capture can carry:
  - `capture_actual_scope=display`
  - `screenshot_capture_elapsed_ms=17`
  - `capture_fallback_reason=no_foreground_window`

### Appcore

`voicewin-appcore/src/service.rs` tests

- validated prepared session context preserves screenshot capture metadata
- validated prompt preview preserves capture metadata in `visual_context_runtime`

### Frontend

Updated History/Prompts tests so rendered runtime strings cover:

- capture latency
- actual capture scope
- fallback reason

## Verification

Executed successfully:

- `cargo fmt --all`
- `cargo test -q -p voicewin-core -p voicewin-engine -p voicewin-appcore`
- `cargo check -q --examples -p voicewin-appcore`
- `cargo test -q`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

## Live example validation

Validated on 2026-03-20 with:

- provider: `openai_compatible`
- API: `responses_sse`
- model: `gpt-5.4`
- example: `voicewin-appcore/examples/live_session_stop_latency.rs`
- scope: `VOICEWIN_LIVE_VISUAL_CAPTURE_SCOPE=foreground_window`

Observed emitted telemetry:

- `capture_actual_scope=foreground_window`
- `visual_dispatch=ocr/foreground_window/ocr_prepared`
- `screen_ocr_source=prepared`
- `final_text=VoiceWin`

This live example uses the test context provider, so it validates the end-to-end serialization/output path, not a native OS fallback. Real fallback behavior is covered by unit/integration tests and platform logging, but still needs runtime validation on macOS/Windows hosts.

## Net effect

VoiceWin now distinguishes:

- requested capture scope
- actual capture scope
- capture latency
- fallback reason

This removes a major ambiguity in visual-context debugging and gives later latency work a cleaner signal when foreground-window capture is unavailable or degraded on a specific OS path.
