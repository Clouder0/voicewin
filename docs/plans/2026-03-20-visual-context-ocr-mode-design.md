# 2026-03-20 Visual Context / OCR Mode Design

## Goal

Clarify how VoiceWin should evolve visual context for LLM post-processing now that the MVP has:

- real screenshot capture for multimodal models,
- no true OCR fallback yet,
- profile-aware context toggles,
- strict latency sensitivity.

This note focuses on product shape and architecture, not implementation details.

## Current State

Today the codebase has one boolean gate:

- `voicewin-core/src/context.rs`
  - `ContextToggles.use_ocr`

But shipped behavior is no longer “OCR”:

- when enabled and the selected provider supports images, VoiceWin captures a screenshot and attaches it to the model request,
- when the provider does not support images, VoiceWin skips screenshot capture and continues without visual context.

Relevant code:

- `voicewin-core/src/context.rs`
- `voicewin-core/src/llm.rs`
- `voicewin-engine/src/traits.rs`
- `voicewin-appcore/src/service.rs`
- `voicewin-platform/src/windows.rs`
- `voicewin-platform/src/macos.rs`

Current platform capture scope is narrow:

- macOS: main display capture
- Windows: primary screen capture

Not implemented yet:

- real OCR extraction,
- active-window capture,
- app-scoped capture,
- visual-mode separation (`screenshot` vs `ocr`),
- capture-scope configuration.

## Core Product Insight

`OCR` and `screenshot` are **not** the same setting.

They share a **capture source**, but differ in how the captured pixels are consumed:

- **Screenshot mode**
  - send the image to an image-capable model
- **OCR mode**
  - run local OCR on the image
  - inject extracted text into the prompt
- **Auto mode**
  - prefer screenshot for image-capable models
  - fall back to OCR for text-only models

So the product should separate:

1. **whether visual context is enabled**
2. **how visual context is delivered**
3. **what part of the desktop is captured**

## Recommended User Model

### 1. Visual mode

Recommended setting:

- `off`
- `auto` (**recommended**)
- `screenshot`
- `ocr`

Behavior:

- `off`
  - no visual capture work
- `auto`
  - image-capable provider -> screenshot
  - text-only provider -> OCR
- `screenshot`
  - capture image, attach image only
  - if provider cannot accept images, warn and continue without visual context
- `ocr`
  - capture image, run OCR, inject OCR text only
  - works for both text-only and multimodal models

Important rule:

- do **not** send both screenshot and OCR text by default
- that adds prompt bloat, conflicting signals, and latency
- if we ever support hybrid mode, keep it advanced and explicit

### 2. Capture scope

Recommended cross-platform scope values:

- `foreground_window` (**recommended default**)
- `display`

For `display`, a second setting is useful:

- `display_with_foreground_window` (**recommended**)
- `primary_display`

Why not make `foreground_app` the core cross-platform primitive?

- On Windows, the clean official capture target is a **window** (`HWND`), not “all windows of app”.
- On macOS, app-scoped capture is possible, but it can include multiple windows from the same app, which is less predictable and worse for privacy.
- Product-wise, the user usually means “the thing I’m typing into right now”, which is a **window**, not the whole app.

Conclusion:

- ship `foreground_window` first
- treat `foreground_app` as future/advanced/platform-specific work, not the base abstraction

## Recommended Config Shape

### Replace the legacy boolean seam

Current:

- `context.use_ocr: bool`

Recommended direction:

```rust
enum VisualContextMode {
    Off,
    Auto,
    Screenshot,
    Ocr,
}

enum VisualCaptureScope {
    ForegroundWindow,
    Display,
}

enum DisplaySelection {
    ForegroundDisplay,
    PrimaryDisplay,
}
```

Then add a dedicated visual config object on defaults + effective config + profile overrides.

## Recommended Runtime Model

### 1. Separate capture from delivery

Current code couples platform capture directly to an encoded screenshot artifact (`data:image/png;base64,...`).

That is okay for screenshot-only mode, but wrong for OCR mode.

Recommended direction:

- platform layer returns a raw or lightly wrapped captured frame
- shared visual pipeline decides:
  - resize policy
  - image encoding
  - OCR extraction
  - prompt attachment

This keeps one capture source and multiple output paths.

### 2. OCR text must get its own prompt field

Do **not** overload:

- `current_window_context`

Recommended:

- add dedicated OCR text field/artifact
- render as a dedicated block, for example:
  - `<SCREEN_OCR_TEXT>...</SCREEN_OCR_TEXT>`

This keeps app metadata, normal text context, and OCR-derived text separate.

### 3. Deadlines are mandatory

Latency rule:

- screenshot capture and OCR are optional context
- they must obey a deadline
- if they miss the budget, enhancement proceeds without them

Good default:

- start visual capture during prepared-session time
- allow OCR to race in the background
- drop OCR if not ready by stop time

## Platform Feasibility

### Windows

Current implementation:

- foreground app identity already uses the actual foreground `HWND`
- screenshot capture still grabs the primary screen

Feasible next step:

- switch from full-screen GDI capture to foreground-window capture

Best direction:

- Windows Graphics Capture interop (`IGraphicsCaptureItemInterop::CreateForWindow`)
- this targets a single window directly

Avoid making `PrintWindow` the main path:

- it is synchronous/blocking
- app-controlled rendering makes behavior less predictable

### macOS

Current implementation:

- app identity uses `NSWorkspace.frontmostApplication`
- screenshot capture uses `CGDisplay::main().image()`

Clean modern direction:

- ScreenCaptureKit window capture

Important constraint:

- VoiceWin currently declares macOS minimum version `11.0`
- ScreenCaptureKit is a newer API surface, so we need:
  - a version-gated implementation, or
  - an older-API fallback, or
  - a raised minimum version later

## OCR Engine Recommendation

Use OCR as a compatibility layer, not the primary path.

Recommended product order:

1. multimodal screenshot path first
2. OCR fallback second

Recommended engineering order:

1. implement the visual-mode model (`off/auto/screenshot/ocr`)
2. implement `foreground_window` capture
3. implement local OCR extraction
4. benchmark OCR latency and quality

Important packaging caveat:

- Windows `Windows.Media.Ocr` requires package identity / MSIX
- current VoiceWin bundle target is NSIS + DMG, not MSIX
- so Windows OCR should not assume WinRT OCR is safely available in the current shipping lane

That makes a bundled cross-platform OCR engine or a platform-split strategy more realistic than relying on WinRT OCR alone.

## Product Recommendation

### Ship this first

1. **Visual context**
   - Off / Auto / Screenshot / OCR
2. **Capture target**
   - Foreground window / Display
3. **Per-profile overrides**
   - visual mode
   - capture target
4. **Advanced global knobs**
   - screenshot max edge
   - OCR timeout budget

### Default recommendation

- visual mode: `auto`
- capture target: `foreground_window`
- display selection: `foreground_display`

This is the best balance of:

- privacy
- relevance
- latency
- cross-platform predictability

## Strong Recommendation

Do **not** model the next version as:

- “OCR = on/off”

Model it as:

- **visual context mode** + **capture scope**

That is the clean abstraction that supports:

- multimodal screenshot models,
- text-only OCR fallback,
- profile-specific behavior,
- future window/app-specific capture,
- and latency-aware execution.
