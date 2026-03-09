#!/usr/bin/env bash
set -euo pipefail

cargo test --locked
cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml

pushd voicewin-tauri >/dev/null
bun install --frozen-lockfile
bun run test
bun run typecheck
bun run build
popd >/dev/null
