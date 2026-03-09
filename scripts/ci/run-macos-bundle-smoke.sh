#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SMOKE_DIR="$ROOT_DIR/voicewin-tauri/src-tauri/target/ci-smoke/macos"
STDOUT_LOG="$SMOKE_DIR/stdout.log"
EVIDENCE_LOG="$SMOKE_DIR/voicewin.log"
MARKER="VOICEWIN_SMOKE_OK"
PROVENANCE_PATTERN='^VoiceWin startup: version=.* git_sha=.*$'
MARKER_PATTERN='^VOICEWIN_SMOKE_OK version=.* git_sha=.*$'
DEFAULT_APP_CANDIDATES=(
  "$ROOT_DIR/voicewin-tauri/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/VoiceWin.app"
  "$ROOT_DIR/voicewin-tauri/src-tauri/target/release/bundle/macos/VoiceWin.app"
)

mkdir -p "$SMOKE_DIR"
rm -f "$STDOUT_LOG" "$EVIDENCE_LOG"

artifact_path="${1:-}"
if [[ -z "$artifact_path" ]]; then
  for candidate in "${DEFAULT_APP_CANDIDATES[@]}"; do
    if [[ -d "$candidate" ]]; then
      artifact_path="$candidate"
      break
    fi
  done

  if [[ -z "$artifact_path" ]]; then
    echo "ERROR: Could not locate built macOS app bundle. Expected one of:" >&2
    printf '  %s\n' "${DEFAULT_APP_CANDIDATES[@]}" >&2
    exit 1
  fi
fi

if [[ ! -d "$artifact_path" ]]; then
  echo "ERROR: macOS app bundle not found: $artifact_path" >&2
  exit 1
fi

app_binary=""
for candidate in "$artifact_path"/Contents/MacOS/*; do
  if [[ -f "$candidate" && -x "$candidate" ]]; then
    app_binary="$candidate"
    break
  fi
done

if [[ -z "$app_binary" ]]; then
  echo "ERROR: macOS app executable not found or not executable under $artifact_path/Contents/MacOS" >&2
  exit 1
fi

echo "Launching macOS smoke executable: $app_binary"
VOICEWIN_SMOKE_TEST=1 "$app_binary" >"$STDOUT_LOG" 2>&1 &
pid=$!
exit_code=-1

for _ in {1..30}; do
  if ! kill -0 "$pid" 2>/dev/null; then
    if wait "$pid"; then
      exit_code=0
    else
      exit_code=$?
    fi
    break
  fi

  sleep 1
done

if (( exit_code == -1 )); then
  kill "$pid" 2>/dev/null || true
  for _ in {1..5}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi

    sleep 1
  done

  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
  fi

  wait "$pid" 2>/dev/null || true
  echo "ERROR: macOS smoke app did not exit within 30 seconds." >&2
  exit 1
fi

if (( exit_code != 0 )); then
  echo "ERROR: macOS smoke app exited with code $exit_code." >&2
  exit 1
fi

if [[ ! -f "$STDOUT_LOG" ]]; then
  echo "ERROR: macOS smoke process output log not found: $STDOUT_LOG" >&2
  exit 1
fi

provenance_line="$(grep -nEm1 "$PROVENANCE_PATTERN" "$STDOUT_LOG" | cut -d: -f1 || true)"
marker_line="$(grep -nEm1 "$MARKER_PATTERN" "$STDOUT_LOG" | cut -d: -f1 || true)"

if [[ -z "$provenance_line" ]]; then
  echo "ERROR: Startup provenance line not found in macOS process output: $STDOUT_LOG" >&2
  exit 1
fi

if [[ -z "$marker_line" ]]; then
  echo "ERROR: Smoke marker not found in macOS process output: $STDOUT_LOG" >&2
  exit 1
fi

if (( provenance_line >= marker_line )); then
  echo "ERROR: Startup provenance line did not appear before smoke marker in macOS process output: $STDOUT_LOG" >&2
  exit 1
fi

cp "$STDOUT_LOG" "$EVIDENCE_LOG"
grep -Fn "$MARKER" "$EVIDENCE_LOG"
echo "Smoke marker found in process output: $STDOUT_LOG"
echo "Smoke evidence copied to $EVIDENCE_LOG"
