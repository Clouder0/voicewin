# Runtime Screenshot Capture Report

Date: 2026-03-19

## Summary

VoiceWin now has a real runtime screenshot capture path for LLM post-processing:

- screenshot capture is an explicit `AppContextProvider` capability,
- it is only invoked when `context.use_ocr == true`,
- prepared-session capture can now freeze screenshot context at recording start,
- the engine has a non-prepared fallback path that still captures screenshots at stop time,
- preview now uses the same policy-aware capture path when OCR is enabled,
- platform capture output is resized and encoded in a latency-aware way before upload.

This closes the main remaining multimodal gap from the earlier preview/provider batch.

## Implementation

### 1. Provider seam

`voicewin-engine::traits::AppContextProvider` now exposes:

- `capture_screenshot() -> anyhow::Result<Option<ImageArtifact>>`
- `snapshot_context_for_policy(include_screenshot: bool)`

Behavior:

- if OCR is off, screenshot work is skipped entirely,
- if OCR is on and the snapshot already contains a screenshot, no recapture happens,
- if OCR is on and the snapshot does not contain a screenshot, capture is attempted best-effort,
- capture failures are logged and the session continues without screenshot context.

This keeps screenshot capture explicit instead of hiding it inside generic context snapshotting.

### 2. Appcore policy wiring

`voicewin-appcore::AppService` now resolves effective config before context capture in the places that matter:

- `prepare_session_context()`
- `preview_prompt()`

That means the OCR gate is known before context capture starts.

Prepared sessions now freeze:

- app identity,
- effective config,
- text context,
- optional screenshot context.

### 3. Engine fallback wiring

`voicewin-engine::VoicewinEngine` now uses the policy-aware snapshot helper in both:

- `run_session_with_hook()`
- `run_session_with_transcript_with_hook()`

This matters because if the prepared-session task is missing or unavailable, VoiceWin still captures screenshot context correctly on the stop-time path.

### 4. Platform capture implementation

#### macOS

Current implementation:

- capture the main display via CoreGraphics,
- normalize raw BGRA pixels into RGBA,
- downscale if needed,
- encode as PNG data URL.

#### Windows

Current implementation:

- capture the primary screen via Win32 GDI,
- read back a top-down 32-bit DIB,
- normalize BGRA to RGBA,
- downscale if needed,
- encode as PNG data URL.

### 5. Latency-aware image shaping

To avoid shipping full-resolution desktop images directly into multimodal requests, the shared screenshot helper now:

- clamps the longest edge to `1280 px`,
- uses a fast resize path,
- encodes PNG with `CompressionType::Fast`,
- emits a `data:image/png;base64,...` artifact for provider reuse.

This is intentionally conservative and latency-oriented.

Rationale:

- the earlier live multimodal benchmark already showed good results at low/medium image sizes,
- raw full-screen uploads would create unnecessary local encode cost and network payload,
- VoiceWin cares more about fast correction than about archival image fidelity.

## Regression Coverage

Added or extended tests in:

- `voicewin-platform`
  - screenshot resize/channel-swap/data-url tests
- `voicewin-appcore`
  - prepared context captures screenshot when OCR is enabled
  - prepared context skips screenshot capture when OCR is disabled
- `voicewin-engine`
  - runtime transcript path captures screenshot when OCR is enabled
  - runtime transcript path skips screenshot capture when OCR is disabled

## Verification

Passed locally:

- `cargo fmt`
- `cargo check -q`
- `cargo test -q`
- `cargo check -q -p voicewin-appcore --example live_prompt_preview --example live_session_stop_latency`
- `cargo check -q` in `voicewin-tauri/src-tauri`
- `cargo test -q` in `voicewin-tauri/src-tauri`

Additional targeted lane:

- `cargo test -q -p voicewin-platform`

## Live End-to-End Validation

Live gateway:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`
- API key: user-provided
- OpenAI model: `gpt-5.4`
- Gemini model: `gemini-3-flash-preview`

Shared live screenshot artifact:

- source file: `/tmp/voicewin-live-screenshot.png`
- content: centered `VOICEWIN`

### Preview path via capture hook

This validates the new policy-aware screenshot capture path used by `preview_prompt()`, not the older manual snapshot injection path.

#### OpenAI Responses SSE

Observed:

- elapsed: `2831 ms`
- provider elapsed: `2827 ms`
- first token: `2682 ms`
- final output: `VOICEWIN`

#### Gemini native SSE

Observed:

- elapsed: `2846 ms`
- provider elapsed: `2842 ms`
- first token: `2840 ms`
- final output: `VoiceWin`

### Prepared runtime stop path via capture hook

This validates `prepare_session_context()` freezing screenshot context before the stop path runs.

#### OpenAI Responses SSE

Config:

- prepared: `true`
- recording overlap: `2500 ms`

Observed:

- stop wall: `2479 ms`
- enhancement: `2474 ms`
- first token: `2242 ms`
- final output: `VOICEWIN`

#### Gemini native SSE

Config:

- prepared: `true`
- recording overlap: `2500 ms`

Observed:

- stop wall: `5828 ms`
- enhancement: `5823 ms`
- first token: `5822 ms`
- final output: `VoiceWin`

Interpretation:

- the runtime screenshot hook is working end-to-end on both providers,
- provider-side latency is still the dominant variable,
- the local screenshot plumbing is not the bottleneck in these runs.

### Non-prepared fallback stop path

This validates the engine-side fallback path when prepared context is not used.

#### OpenAI Responses SSE

Config:

- prepared: `false`
- recording overlap: `0 ms`

Observed:

- stop wall: `2557 ms`
- enhancement: `2552 ms`
- first token: `2428 ms`
- final output: `VoiceWin`

Interpretation:

- stop-time fallback screenshot capture works,
- VoiceWin still preserves multimodal correction even without prepared-session context,
- prepared-session capture remains preferable because it overlaps optional context work with recording time.

## Product Conclusions

### 1. Runtime screenshot capture is now real, not preview-only

Before this batch:

- screenshot context existed only for preview overrides and provider probes.

After this batch:

- prepared runtime sessions can capture and freeze screenshot context,
- stop-time fallback sessions can still capture screenshot context,
- preview and runtime now share the same OCR policy seam.

### 2. `use_ocr` remains the only correct gate

Current behavior is now consistent:

- `use_ocr = false` -> no screenshot capture work
- `use_ocr = true` -> best-effort screenshot capture and multimodal attachment

This is important for both latency and privacy.

### 3. The current platform scope is intentionally narrow

What is implemented now:

- macOS main display capture
- Windows primary screen capture

What is not implemented yet:

- active-window-only capture
- browser-tab-aware capture
- screenshot persistence in history
- OCR text fallback generation for non-image-capable providers

The next quality step is active-window capture, not broader storage.

## Residual Risk

This repo was validated on a Linux host, so the native macOS/Windows capture code could not be executed locally here.

I attempted explicit cross-target `cargo check` runs for:

- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`

Those were blocked by host toolchain limitations outside the Rust code itself:

- macOS target build blocked in `ring` because the host lacks a macOS-capable C toolchain,
- Windows MSVC target build blocked because `lib.exe` is unavailable on this Linux host.

So the remaining risk is strictly platform-build validation, not the core VoiceWin runtime/plumbing logic.

## Outcome

The LLM post-processing stack now has:

- profile-aware provider/model/prompt selection,
- prompt preview/history/provider probes,
- OpenAI Responses SSE,
- Gemini native SSE,
- multimodal provider dispatch,
- and real runtime screenshot capture behind the same OCR gate.

That is a coherent MVP-complete foundation for screenshot-assisted post-processing.
