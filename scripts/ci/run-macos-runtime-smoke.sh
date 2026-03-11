#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SMOKE_DIR="$ROOT_DIR/voicewin-tauri/src-tauri/target/ci-smoke/macos-runtime"
STDOUT_LOG="$SMOKE_DIR/stdout.log"
EVIDENCE_LOG="$SMOKE_DIR/voicewin.log"
TARGET_FILE="$SMOKE_DIR/textedit-runtime-target.txt"
TRANSCRIPT="VoiceWin runtime smoke transcript"
PROVENANCE_PATTERN='^VoiceWin startup: version=.* git_sha=.*$'
START_PATTERN='^VOICEWIN_RUNTIME_SMOKE_START version=.* git_sha=.*$'
SUCCESS_PATTERN='^VOICEWIN_RUNTIME_SMOKE_OK version=.* git_sha=.*$'
DEFAULT_APP_CANDIDATES=(
  "$ROOT_DIR/voicewin-tauri/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/VoiceWin.app"
  "$ROOT_DIR/voicewin-tauri/src-tauri/target/release/bundle/macos/VoiceWin.app"
)

cleanup() {
  osascript >/dev/null 2>&1 <<APPLE || true
with timeout of 10 seconds
  tell application "TextEdit"
    if not running then
      return
    end if

    repeat with docRef in documents
      try
        if POSIX path of (path of docRef) is "$TARGET_FILE" then
          try
            save docRef
          end try
          close docRef saving yes
          exit repeat
        end if
      end try
    end repeat
  end tell
end timeout
APPLE
}

prepare_textedit_target() {
  local deadline=$((SECONDS + 30))

  open -a "TextEdit" "$TARGET_FILE"

  while (( SECONDS < deadline )); do
    if osascript >/dev/null 2>&1 <<APPLE
with timeout of 2 seconds
  tell application "TextEdit"
    if not running then
      error "TextEdit not running yet"
    end if

    activate

    if (count of documents) = 0 then
      error "TextEdit has no documents yet"
    end if

    set docRef to front document
    try
      if POSIX path of (path of docRef) is "$TARGET_FILE" then
        set text of docRef to ""
        save docRef
        return
      end if
    end try

    error "TextEdit target document not ready yet"
  end tell
end timeout
APPLE
    then
      return
    fi

    sleep 0.5
  done

  echo "ERROR: timed out waiting for TextEdit target document." >&2
  exit 1
}

focus_textedit() {
  open -a "TextEdit" "$TARGET_FILE" >/dev/null 2>&1 || true
  osascript >/dev/null 2>&1 <<'APPLE' || true
with timeout of 5 seconds
  tell application "TextEdit" to activate
end timeout
APPLE
}

refocus_textedit_until_exit() {
  local app_pid="$1"
  local deadline=$((SECONDS + 30))

  while (( SECONDS < deadline )); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
      break
    fi

    focus_textedit || true
    sleep 0.25
  done
}

save_textedit_target() {
  local deadline=$((SECONDS + 10))

  while (( SECONDS < deadline )); do
    if osascript >/dev/null 2>&1 <<APPLE
with timeout of 2 seconds
  tell application "TextEdit"
    activate
    repeat with docRef in documents
      try
        if POSIX path of (path of docRef) is "$TARGET_FILE" then
          save docRef
          return
        end if
      end try
    end repeat

    error "TextEdit target document not found"
  end tell
end timeout
APPLE
    then
      return
    fi

    sleep 0.5
  done

  echo "ERROR: TextEdit target document not found." >&2
  exit 1
}

trap cleanup EXIT

mkdir -p "$SMOKE_DIR"
rm -f "$STDOUT_LOG" "$EVIDENCE_LOG" "$TARGET_FILE"
: > "$TARGET_FILE"

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

prepare_textedit_target
focus_textedit

echo "Launching macOS runtime smoke executable: $app_binary"
VOICEWIN_RUNTIME_SMOKE_TEST=1 \
VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT="$TRANSCRIPT" \
VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS="TextEdit" \
"$app_binary" >"$STDOUT_LOG" 2>&1 &
pid=$!
refocus_textedit_until_exit "$pid" &
refocus_pid=$!
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

wait "$refocus_pid" || true

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
  echo "ERROR: macOS runtime smoke app did not exit within 30 seconds." >&2
  exit 1
fi

if (( exit_code != 0 )); then
  echo "ERROR: macOS runtime smoke app exited with code $exit_code." >&2
  exit 1
fi

if [[ ! -f "$STDOUT_LOG" ]]; then
  echo "ERROR: macOS runtime smoke process output log not found: $STDOUT_LOG" >&2
  exit 1
fi

provenance_line="$(grep -nEm1 "$PROVENANCE_PATTERN" "$STDOUT_LOG" | cut -d: -f1 || true)"
start_line="$(grep -nEm1 "$START_PATTERN" "$STDOUT_LOG" | cut -d: -f1 || true)"
success_line="$(grep -nEm1 "$SUCCESS_PATTERN" "$STDOUT_LOG" | cut -d: -f1 || true)"

if [[ -z "$provenance_line" ]]; then
  echo "ERROR: Startup provenance line not found in macOS runtime smoke output: $STDOUT_LOG" >&2
  exit 1
fi

if [[ -z "$start_line" ]]; then
  echo "ERROR: Runtime smoke start marker not found in macOS process output: $STDOUT_LOG" >&2
  exit 1
fi

if [[ -z "$success_line" ]]; then
  echo "ERROR: Runtime smoke success marker not found in macOS process output: $STDOUT_LOG" >&2
  exit 1
fi

if (( provenance_line >= start_line )); then
  echo "ERROR: Startup provenance line did not appear before runtime smoke start marker: $STDOUT_LOG" >&2
  exit 1
fi

if (( start_line >= success_line )); then
  echo "ERROR: Runtime smoke start marker did not appear before success marker: $STDOUT_LOG" >&2
  exit 1
fi

save_textedit_target
actual_text="$(python3 - "$TARGET_FILE" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
print(path.read_text(encoding='utf-8'), end='')
PY
)"

if [[ "$actual_text" != "$TRANSCRIPT" ]]; then
  echo "ERROR: TextEdit document contents mismatch. Expected '$TRANSCRIPT' but found '$actual_text'." >&2
  exit 1
fi

cp "$STDOUT_LOG" "$EVIDENCE_LOG"
grep -En "$SUCCESS_PATTERN" "$EVIDENCE_LOG"
echo "Runtime smoke marker found in process output: $STDOUT_LOG"
echo "TextEdit target contents matched transcript: $TARGET_FILE"
echo "Runtime smoke evidence copied to $EVIDENCE_LOG"
