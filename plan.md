
This plan implements `voicewin/design-draft.md` (v3.0) as the UI source of truth.

We will not preserve or maintain the legacy UI (Settings/Diagnostics/etc). The app surfaces are:
- Main window shell + 4 pages (Overview, Profiles, Model Library, History)
- Overlay HUD (topmost, no focus steal)

---

## 0) Constraints / Non-Goals

- Do not reintroduce legacy UI.
- Prefer token-based styling (use `voicewin/voicewin-tauri/src/ui/tokens.css`).
- Avoid destructive git commands (`git checkout`, hard resets, etc.).
- Windows-first UX fidelity (other platforms best-effort).
- Overlay must not steal focus.

---

## 1) Approved Addenda / Decisions

These are intentional deviations or clarifications to the frozen design spec:

1) Hotkey
- Default hotkey is `Ctrl+Space`.
- User-customizable and persisted.
- If registration fails, show an error and keep the last working binding.

2) Overlay HUD positioning
- Overlay window is fit-content (variable width).
- Overlay is draggable; position persisted.
- Provide a "Reset HUD Position" action; default is bottom-center, 80px above monitor bottom.

3) Profiles "Pick Window"
- "Pick Window" captures current foreground app and writes a `ProcessNameEquals` matcher.

4) Overlay success outcome
- Success label is "Inserted" when insertion succeeds.
- "Copied" is only used if we ever add a clipboard-only fallback.

5) Clipboard behavior
- Always restore clipboard after attempting insertion.
- If insertion fails, rely on History for recovery and show actionable overlay Error linking to History.

6) Overlay Error state
- Add an actionable overlay Error state.

---

## 2) Known Issues (What Must Be Fixed Next)

Critical:
- Windows vibrancy code will not compile on Windows due to `Color::from_rgba(...)` usage; `window_vibrancy::Color` is a tuple.
- Overlay is currently made click-through (`set_ignore_cursor_events(true)`), making Stop/Cancel/History/drag impossible.
- Overlay position persistence is incorrect: it persists on every pointer-up (including button clicks).
- SessionController always sets stage `Done` on Ok, even when `RunSessionResponse.stage == "failed"`.
- Model catalog URLs are wrong (404), and `.sha256` sidecars are not available for the likely correct public GGUF repos.

High:
- Overlay Success state + 1500ms success delay is not implemented per spec.
- Overlay positioning drifts after fit-content resize and on HiDPI due to mixing fixed physical constants with logical resizing.
- History hover actions are broken by inline opacity; per-row Delete missing.
- Models UI doesn't disable Download during progress; backend doesn't report downloading state.

---

## 3) Implementation Milestones (Refined)

### Milestone A — Windows Build Unblock (Vibrancy)
Goal: ensure Windows builds compile.

Work:
- Fix `voicewin/voicewin-tauri/src-tauri/src/main.rs` acrylic call to use `(u8,u8,u8,u8)` tuple.

Acceptance:
- Windows `cargo check` succeeds for `voicewin/voicewin-tauri/src-tauri`.

---

### Milestone B — Overlay Interactivity (No Click-Through + Correct Drag/Persist)
Goal: make the overlay usable: buttons clickable, drag works, persistence correct.

Work:
- Remove default click-through mode for overlay window.
- Redesign dragging so it does not steal button clicks:
  - Drag only starts from a dedicated drag region (not the entire pill).
  - Buttons never trigger dragging.
- Fix persistence: do not persist overlay position on normal clicks.
  - Preferred approach: in Rust, listen to overlay `WindowEvent::Moved` and persist ONLY while a "user is dragging" flag is active.
  - Use `overlay_drag_begin`/`overlay_drag_end` commands to toggle that flag.

Acceptance:
- Stop/Cancel/History/Dismiss buttons work.
- Dragging works.
- Overlay position only persists after real drag movement.

---

### Milestone C — Fit-Content Sizing + DPI-Safe Positioning
Goal: overlay stays bottom-centered as its size changes; no drift on HiDPI.

Work:
- Recenter logic must not use hard-coded `OVERLAY_WIDTH`.
- Compute default position using:
  - monitor work area (physical)
  - overlay `outer_size()` (physical)
- Apply the same logic in:
  - initial placement
  - tray "Reset HUD Position"
  - after `overlay_set_size` (recenter if there is no saved user position)
- Update overlay webview sizing to re-measure when content changes (stage change), not only on first show.

Acceptance:
- Overlay remains centered during stage transitions and after resize.
- Reset action returns it to correct spec default (bottom-center, 80px offset).

---

### Milestone D — Session → Overlay State Machine (Spec Accurate)
Goal: stages and timing match design-draft.

Work:
- Fix incorrect stage handling in `voicewin/voicewin-tauri/src-tauri/src/session_controller.rs`:
  - Do not set `Done` when engine stage is `failed`.
  - Map `RunSessionResponse.stage` to overlay stages.
- Implement Cancel during processing:
  - Current Cancel only works in Recording.
  - Implement a best-effort cancellation mechanism (abort in-flight session task and prevent insertion where possible).
- Wire real pipeline stages:
  - Add an AppService entrypoint that runs `VoicewinEngine::run_session_with_hook` and forwards stage updates.
  - SessionController updates to `Transcribing` / `Enhancing` / `Inserting` as the engine progresses.
- Implement Success flow:
  - Render Success UI (checkmark + "Inserted").
  - Hold for 1500ms, then play exit animation and dismiss.
  - Remove backend auto-hide that conflicts with the 1500ms spec.
- Error flow:
  - Show actionable Error state and link to History.

Acceptance:
- Overlay shows Recording → Processing (with correct labels) → Success (1500ms) → Exit.
- Insertion failures show Error (actionable) and do not pretend success.

---

### Milestone E — History Page (Hover Actions + Delete)
Goal: spec-correct History actions.

Work:
- Fix hover actions visibility by removing inline `opacity: 0` and letting CSS handle it.
- Replace hardcoded border colors with tokens (`--stroke-card`).
- Add per-row Delete:
  - Preferred: add a stable `id` to HistoryEntry (backward compatible), delete by id.
  - Minimal alternative: delete by (ts_unix_ms, text) if we want to avoid schema changes.
- Fix React keys to avoid ts collision.

Acceptance:
- Actions appear only on hover.
- Copy + Delete work per row.

---

### Milestone F — Model Library (Real GGUF Catalog + Integrity + Download State)
Goal: downloads actually work and UI state matches spec.

Work:
- Replace broken catalog URLs with working public GGUF sources.
  - Prefer `FL33TW00D-HF/whisper-*` GGUF models (public, verified to resolve).
- Replace `.sha256` sidecar dependency with a workable integrity strategy:
  - Option A (pinned): store expected SHA256 in catalog for each model and verify while streaming.
  - Option B (dynamic): fetch metadata from HuggingFace resolve response headers (e.g. `x-linked-etag`) before streaming the redirected URL.
  - Choose one and implement consistently.
- Harden downloader:
  - Ensure `.download` temp cleanup on all error paths.
  - Throttle progress events.
  - Track in-progress downloads in backend and return `downloading: true` from `list_models`.
- UI updates:
  - Disable Download button while downloading.
  - Clear progress state on completion.
  - Use token colors for progress track.

Acceptance:
- At least one model downloads end-to-end and validates.
- "Downloading" state behaves per design-draft.

---

### Milestone G — Styling Gaps / Token Cleanup
Goal: close remaining spec-visible polish gaps.

Work:
- Implement `.vw-micHero` CSS per `voicewin/design-draft.md`.
- Remove hardcoded RGBA colors where tokens exist (History borders, model progress track, overlay bars, etc.).

Acceptance:
- Overview mic hero matches spec shape and border.
- Dark theme does not break obvious borders/fills.

---

## 4) Verification / QA

Automated (Linux):
- `cargo test` in `voicewin/`
- `cargo check` in `voicewin/voicewin-tauri/src-tauri`
- `bun run test` and `bun run build` in `voicewin/voicewin-tauri`

Manual:
- Overlay: click Stop/Cancel/History/Dismiss; drag; restart app and confirm persisted position; Reset HUD Position.
- Pipeline: verify stage transitions and success delay.
- History: hover actions; copy; delete single row; clear all.
- Models: download; progress; retry failure; set active.

Windows-specific:
- Confirm vibrancy compiles and overlay interactivity is correct on Windows.
