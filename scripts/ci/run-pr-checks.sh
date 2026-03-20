#!/usr/bin/env bash
set -euo pipefail

if [ -z "${CARGO_HOME:-}" ]; then
  default_cargo_home="${HOME}/.cargo"
  cache_probe_dir="${default_cargo_home}/registry/cache"
  probe_failed=0

  if [ -d "$cache_probe_dir" ]; then
    probe_file="${cache_probe_dir}/.voicewin-write-probe"
    if ! ( : > "$probe_file" ) 2>/dev/null; then
      probe_failed=1
    else
      rm -f "$probe_file"
    fi
  fi

  if [ "$probe_failed" -eq 1 ]; then
    export CARGO_HOME="${TMPDIR:-/tmp}/voicewin-cargo-home"
    mkdir -p "$CARGO_HOME"
    echo "run-pr-checks: default cargo cache is read-only; using CARGO_HOME=$CARGO_HOME"
  fi
fi

cargo test --locked
cargo test --locked --manifest-path voicewin-tauri/src-tauri/Cargo.toml

pushd voicewin-tauri >/dev/null
bun install --frozen-lockfile
bun run test
bun run typecheck
bun run build
popd >/dev/null
