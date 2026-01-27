# GGML Local STT Migration Plan

Last updated: 2026-01-27

## Goal

Make local Whisper transcription work out of the box by switching the local model pipeline from **GGUF (`.gguf`)** to **whisper.cpp GGML (`.bin`)** models.

Decisions:

- Bootstrap model bundled with the app: `ggml-tiny.bin` (multilingual) copied to `models/bootstrap.bin`.
- Downloadable models should also be GGML `.bin` with pinned SHA256.
- Existing user configs that point at `.gguf` should be auto-migrated without wiping unrelated settings.

## Work Items (Tracked)

### 1) Bundle + Bootstrap Copy

- [x] Download `ggml-tiny.bin` and add as `voicewin-tauri/src-tauri/resources/models/bootstrap.bin`
- [x] Update Tauri bundle resources list to include `resources/models/bootstrap.bin`
- [x] Update `ensure_bootstrap_model(...)` to resolve/copy `bootstrap.bin` instead of `bootstrap.gguf`
- [x] Update bootstrap validation constants (SHA256 + min size)

### 2) Runtime Model Format Support (GGML)

- [x] Add GGML magic detection + validation helpers in `voicewin-runtime/src/models.rs`
- [x] Keep GGUF detection for user-friendly error messages (but stop accepting it as valid local STT model)
- [x] Update `BOOTSTRAP_MODEL_FILENAME` to `bootstrap.bin`
- [x] Update `PREFERRED_LOCAL_STT_MODEL_FILENAME` to a `.bin` model (used for "preferred" checks)

### 3) Model Catalog + Downloads

- [x] Replace GGUF-only `whisper_catalog()` entries with GGML `.bin` entries
- [x] Update `download_model` to validate GGML `.bin` (and keep SHA256 verification)
- [x] Ensure the catalog URLs work in restricted regions (prefer `hf-mirror.com`, keep official HF as fallback)

### 4) Config Validation + Auto-Migration

- [x] Update config validation: local STT model must be GGML `.bin` (not GGUF)
- [x] Replace "reset to defaults" behavior with a targeted migration:
  - if config points to GGUF/missing model: switch only `defaults.stt_model` to bundled `bootstrap.bin`
  - preserve other settings
- [x] Ensure migration runs for tray/hotkey-only flows (service init)

### 5) Better Errors

- [x] In `LocalWhisperSttProvider`, detect GGUF early and return a clear error explaining `.bin` requirement

### 6) UI/Status Polishing (Minimal)

- [x] Update `get_model_status` to validate `.bin` models
- [x] Update Overview model labels to match new bootstrap (`Tiny`) / preferred (`Base`)

### 7) Verification

- [x] Add unit tests for GGML magic detection/validation
- [x] Run `cargo test` (workspace)

## Progress Log

- 2026-01-27: Plan created; starting implementation.
- 2026-01-27: Bundled `bootstrap.bin` (ggml-tiny), migrated validation/catalog/config to GGML `.bin`, and added fallback downloads.
