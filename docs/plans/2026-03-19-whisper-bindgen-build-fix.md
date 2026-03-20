# Whisper Build Fix

Date: 2026-03-19

## Problem

`cargo check -p voicewin-appcore` and `cargo check` in `voicewin-tauri/src-tauri` were blocked on
this machine by `whisper-rs-sys 0.14.1` generating invalid layout assertions at build time:

- `_IO_FILE` size assertion overflow
- `whisper_full_params` size assertion overflow

The failure came from generated bindings in:

- `target/debug/build/whisper-rs-sys-*/out/bindings.rs`

## Root Cause

`whisper-rs-sys` generates bindings with bindgen by default. Its `build.rs` also supports a safer
fallback path:

- if `WHISPER_DONT_GENERATE_BINDINGS` is set, copy bundled `src/bindings.rs` instead

That fallback is enough to unblock this environment.

## Repo-Level Fix

Set this in workspace cargo config:

```toml
[env]
WHISPER_DONT_GENERATE_BINDINGS = "1"
```

Implemented in:

- `/.cargo/config.toml`

This makes normal workspace builds use bundled bindings instead of machine-local bindgen output.

## Verification

After applying the config and rebuilding `whisper-rs-sys`:

- `cargo check -q -p voicewin-appcore` passed
- `cargo test -q -p voicewin-appcore` passed
- `cargo check -q` in `voicewin-tauri/src-tauri` passed
- `cargo test -q` in `voicewin-tauri/src-tauri` passed

## Operational Note

If a developer already built the repo before this fix landed, Cargo may still hold stale generated
bindings in the existing target directory. A one-time clean of `whisper-rs-sys` may be needed:

- workspace root:
  - `cargo clean -p whisper-rs-sys`
- `voicewin-tauri/src-tauri`:
  - `cargo clean -p whisper-rs-sys`

Fresh clones should not need that cleanup.
