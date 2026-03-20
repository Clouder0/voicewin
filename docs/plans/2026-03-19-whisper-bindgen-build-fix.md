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

## Initial Repo-Level Fix

Set this in workspace cargo config:

```toml
[env]
WHISPER_DONT_GENERATE_BINDINGS = "1"
```

Implemented in:

- `/.cargo/config.toml`

This makes workspace builds use bundled bindings instead of machine-local bindgen output.

## Windows Follow-up

On 2026-03-20, native Windows CI showed that the bundled `whisper-rs-sys` bindings are Linux-flavored
and fail on Windows with layout assertion overflows in `_G_fpos_t`, `_G_fpos64_t`, and `_IO_FILE`.

That means the original global Cargo-config approach was too broad. Cargo accepts target-scoped
`env` entries in `cargo config get`, but still warns that they are unused during real builds, so
that path was not reliable.

The fix was moved out of repo-global Cargo config and into Linux-only execution paths instead:

- remove `WHISPER_DONT_GENERATE_BINDINGS` from `/.cargo/config.toml`
- set it explicitly in Linux verification scripts (currently `scripts/ci/run-pr-checks.sh`)
- let native Windows builds use upstream bindgen behavior

Result:

- Linux scripted verification keeps the local-build workaround
- native Windows builds fall back to bindgen, which is what upstream expects there

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
