# 2026-03-20 Visual Context Implementation Spec

## Goal

Replace the legacy boolean screenshot/OCR seam with a real visual-context model that supports:

- distinct `screenshot` vs `ocr` behavior,
- configurable capture scope,
- profile overrides,
- strong logging for OS-dependent capture paths,
- backward-compatible config migration.

This batch must stay latency-aware and test-driven.

## Scope

### In scope

- new visual-context config types in core domain
- config migration from legacy `use_ocr`
- prompt support for OCR-derived text
- backend routing rules for `off/auto/screenshot/ocr`
- capture-scope plumbing (`foreground_window` vs `display`)
- profile/default/UI wiring
- history/context label updates
- structured logs around visual capture / OCR routing / fallback

### Out of scope

- perfect native OCR on every platform
- browser-tab capture
- multi-window app capture
- screenshot artifact persistence in history

## Product Model

### Visual mode

- `off`
- `auto`
- `screenshot`
- `ocr`

Rules:

- `off`
  - skip all visual work
- `auto`
  - if selected provider/API supports attached images, use `screenshot`
  - otherwise try `ocr`
- `screenshot`
  - attach image directly when selected provider/API supports images
  - otherwise warn and continue without visual context
- `ocr`
  - capture image
  - derive OCR text
  - inject OCR text into prompt
  - do not attach image to the main enhancement request

### Capture scope

- `display`
- `foreground_window`

Default:

- `display`

Reason:

- preserves current behavior for existing configs after migration
- avoids surprise privacy/behavior changes

Future default can change after native foreground-window capture is validated.

## Backward Compatibility

Legacy config field:

- `context.use_ocr: bool`

Migration rules:

- if new `visual_context_mode` is present, use it
- otherwise:
  - `use_ocr = true` -> `visual_context_mode = screenshot`
  - `use_ocr = false` -> `visual_context_mode = off`

Do not serialize `use_ocr` back out in newly saved configs.

## Prompt Model

Add dedicated OCR field to enhancement context:

- `screen_ocr_text: Option<String>`

Prompt block:

- `<SCREEN_OCR_TEXT>...</SCREEN_OCR_TEXT>`

Rules:

- OCR text is separate from window metadata
- OCR text is optional
- screenshot and OCR text are not both attached by default

## Backend Routing Rules

### Selected provider/API supports attached images

- `auto` -> screenshot
- `screenshot` -> screenshot
- `ocr` -> OCR sidecar

### Selected provider/API does not support attached images

- `auto` -> OCR sidecar
- `ocr` -> OCR sidecar
- `screenshot` -> warn + no visual context

## OCR Sidecar Strategy

This batch uses an OCR sidecar request through the existing LLM provider stack.

Reason:

- no bundled local OCR engine exists in repo today
- allows end-to-end OCR mode now
- keeps the new product model real instead of UI-only

Rules:

- OCR sidecar uses the same provider kind / base URL / model / API key
- if the selected API kind cannot attach images, the OCR sidecar upgrades to the provider’s image-capable API kind when possible:
  - `openai_compatible` -> `responses_sse`
  - `gemini` -> `stream_generate_content_sse`
- if OCR sidecar fails, warn and continue without OCR text

## Capture Scope Implementation

### Current batch

- Windows:
  - `display` -> current primary-screen path
  - `foreground_window` -> best-effort window-rect capture from the visible desktop
- macOS:
  - `display` -> current main-display path
  - `foreground_window` -> log unsupported and fall back to display for now

Reason:

- Windows already has foreground `HWND`
- macOS exact-window capture requires additional API work not safely validated in this Linux host

## Logging Requirements

Every visual-context attempt should log:

- resolved visual mode
- capture scope
- provider kind / api kind / model
- whether screenshot attachment is allowed
- whether OCR sidecar was attempted
- whether capture succeeded
- whether OCR succeeded
- elapsed times
- fallback reason when downgraded

Platform capture logs should include:

- requested scope
- chosen native path
- dimensions
- elapsed ms
- failure reason

## Tests

### Core/backend

- legacy `use_ocr` deserializes into new visual mode
- `auto` resolves to screenshot when API supports images
- `auto` resolves to OCR when API does not support images
- `screenshot` on text-only API warns and skips OCR
- OCR text is injected into prompt and image is not attached
- OCR sidecar upgrades OpenAI-compatible text API to `responses_sse`
- capture options propagate requested scope

### Frontend

- settings can save visual mode and capture scope
- profile overrides can save visual mode
- warnings update correctly for screenshot mode on text-only APIs
- `auto`/`ocr` copy does not show the old `use_ocr` wording

## Verification

Required before delivery:

- `cargo fmt --all`
- `cargo test -q`
- `cargo test -q` in `voicewin-tauri/src-tauri`
- `npm run typecheck`
- `npm test`

Live validation should cover at least:

- OpenAI-compatible `responses_sse` screenshot path still works
- OpenAI-compatible `chat_completions` + OCR mode produces OCR text via sidecar and improves a text-only cleanup prompt
